use anyhow::Result;
use dracon_git::GitService;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::git::gh_cmd;
#[path = "current_health.rs"]
mod current_health;

// Concurrency cap for the parallel per-repo work in `run_repos_report`.
// Match status admission in the scheduler: filtered/nested inspections can
// last seconds, and sixteen concurrent full scans compete with live sync.
const REPORT_REPO_CONCURRENCY: usize = 4;

// FIXED 2026-10-09 (audit F135): per-repo budget for the scan-bloat
// `git ls-files --others --directory` walk. That walk reads the whole
// worktree index plus every untracked directory, so its cost scales with
// repo size, and it used to run with no bound at all — a wedged git (or a
// pathological 37 GiB `.git` of the hegemony/hegemon class) held the
// report open indefinitely. 60s is ~120x the measured steady-state cost
// (0.5s/repo on this box for ~26 repos) and still generous for the
// largest watched repo.
const SCAN_BLOAT_GIT_TIMEOUT_SECS: u64 = 60;

#[derive(Serialize)]
struct SyncAlertEntry {
    ts_unix: u64,
    repo: String,
    reason: String,
    details: String,
}

fn sync_alert_ledger_path() -> PathBuf {
    if let Ok(state_dir) = std::env::var("DRACON_SYNC_STATE_DIR") {
        if !state_dir.is_empty() {
            return PathBuf::from(state_dir).join("dracon-sync-alerts.jsonl");
        }
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("state")
        .join("dracon")
        .join("dracon-sync-alerts.jsonl")
}

pub(crate) fn record_sync_alert(repo_path: &Path, reason: &str, details: &str) {
    let repo = repo_path
        .to_string_lossy()
        .trim_end_matches('/')
        .to_string();
    let entry = SyncAlertEntry {
        ts_unix: crate::policy::timestamp_secs(),
        repo,
        reason: reason.to_string(),
        details: details.to_string(),
    };
    let line = match serde_json::to_string(&entry) {
        Ok(line) => line,
        Err(e) => {
            eprintln!("⚠️ failed to serialize sync alert: {}", e);
            return;
        }
    };
    let path = sync_alert_ledger_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!(
                "⚠️ failed to create sync alert dir {}: {}",
                parent.display(),
                e
            );
            return;
        }
    }
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut file) => {
            if let Err(e) = writeln!(file, "{line}") {
                eprintln!("⚠️ failed to write sync alert {}: {}", path.display(), e);
            }
        }
        Err(e) => eprintln!("⚠️ failed to open sync alert {}: {}", path.display(), e),
    }
    eprintln!("🔔 sync alert: {} — {}: {}", entry.repo, reason, details);
}

/// One-shot action-required desktop page (Critical, sticky).
///
/// CHANGED 2026-09-20 (v0.113.84): the daemon sweep no longer calls
/// this — its sites record log-only and feed the single cycle-end
/// summary (`sync_summary_notification`). Remaining caller is the
/// interactive `repair` CLI path (user-triggered, single page).
pub(crate) fn send_sync_conflict_notification(repo_path: &Path, reason: &str, details: &str) {
    record_sync_alert(repo_path, reason, details);

    let repo_name = repo_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_path.display().to_string());

    let title = format!("Dracon Sync: {}", reason);
    let body = format!(
        "Repository '{}' needs manual resolution.\nReason: {}\nDetails: {}",
        repo_name, reason, details
    );

    // Spawn in background to avoid blocking the daemon loop
    tokio::spawn(async move {
        if let Err(e) = notify_rust::Notification::new()
            .summary(&title)
            .body(&body)
            .urgency(notify_rust::Urgency::Critical)
            .show()
        {
            eprintln!("⚠️ failed to send desktop notification: {}", e);
        }
    });
}

/// Transient desktop notification for statistical alerts
/// (dispatch-starved, pile-up class): informational pages the
/// operator never has to act on directly. Normal urgency with a
/// 15s expiry so resolved pages auto-dismiss instead of stacking
/// in the notification center until manually cleared (Critical
/// notifications are sticky on most desktops — the 2026-09-19
/// spam reports were largely already-resolved pages still
/// visible). Genuine action-required conflicts keep Critical via
/// `send_sync_conflict_notification`. v0.113.79.
pub(crate) fn send_sync_stat_notification(repo_path: &Path, reason: &str, details: &str) {
    record_sync_alert(repo_path, reason, details);
    let repo_name = repo_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_path.display().to_string());
    let title = format!("Dracon Sync: {}", reason);
    let body = format!(
        "Repository '{}' — statistical notice (no action required).\nReason: {}\nDetails: {}",
        repo_name, reason, details
    );
    tokio::spawn(async move {
        if let Err(e) = notify_rust::Notification::new()
            .summary(&title)
            .body(&body)
            .urgency(notify_rust::Urgency::Normal)
            .timeout(notify_rust::Timeout::Milliseconds(15_000))
            .show()
        {
            eprintln!("⚠️ failed to send desktop notification: {}", e);
        }
    });
}

/// Summary-notification state (ADDED 2026-09-20, v0.113.84): the daemon
/// keeps at most ONE tray item, updated in place via freedesktop
/// replaces_id and replaced with an auto-dismissing all-clear on
/// resolve. Persisted (not in-memory) so a restart replaces the
/// pre-restart item instead of stacking a second one. Lives under
/// ~/.local/state (never inside a watched repo); tiny, atomic write,
/// fails open (corrupt/missing → post fresh).
#[derive(Serialize, Deserialize)]
struct SummaryNotifyState {
    id: u32,
    title: String,
    body: String,
}

fn summary_state_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/dracon/dracon-sync-summary-notify.json")
}

fn load_summary_state() -> Option<SummaryNotifyState> {
    let bytes = std::fs::read(summary_state_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_summary_state(state: &SummaryNotifyState) {
    let path = summary_state_path();
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let bytes = match serde_json::to_vec(state) {
        Ok(b) => b,
        Err(_) => return,
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, &bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Pure render of the single summary item: sorted (order-insensitive),
/// capped at 5 lines with an overflow tail. Tested below.
pub(crate) fn render_summary_notification(issues: &[(String, String)]) -> (String, String) {
    let mut sorted = issues.to_vec();
    sorted.sort();
    let n = sorted.len();
    let title = if n == 1 {
        "Dracon Sync: 1 issue".to_string()
    } else {
        format!("Dracon Sync: {n} issues")
    };
    let mut body = String::new();
    for (repo, reason) in sorted.iter().take(5) {
        body.push_str(&format!("{repo} — {reason}\n"));
    }
    if n > 5 {
        body.push_str(&format!("…and {} more — see `dracon-sync repos`", n - 5));
    }
    (title, body.trim_end().to_string())
}

/// Sync the single daemon tray item to the current issue set.
/// An empty set plus a stored item replaces the stale sticky with an
/// auto-dismissing (15s, Normal) all-clear, then drops state. An
/// unchanged render stays silent (no re-show: re-popping every cycle
/// would be the spam this replaces). A changed set shows Critical
/// with replaces_id (first show: no id).
/// Send failures only log (headless/CI must never break the cycle).
pub(crate) fn sync_summary_notification(issues: &[(String, String)]) {
    let prev = load_summary_state();
    if issues.is_empty() {
        if let Some(st) = prev {
            let replaces = st.id;
            tokio::spawn(async move {
                if let Err(e) = notify_rust::Notification::new()
                    .summary("Dracon Sync: all clear")
                    .body("previously reported issues resolved — converging normally")
                    .urgency(notify_rust::Urgency::Normal)
                    .timeout(notify_rust::Timeout::Milliseconds(15_000))
                    .id(replaces)
                    .show()
                {
                    eprintln!("⚠️ failed to send all-clear notification: {}", e);
                }
            });
            let _ = std::fs::remove_file(summary_state_path());
        }
        return;
    }
    let (title, body) = render_summary_notification(issues);
    if let Some(st) = &prev {
        if st.title == title && st.body == body {
            return;
        }
    }
    let replaces = prev.map(|s| s.id);
    tokio::spawn(async move {
        let mut n = notify_rust::Notification::new();
        n.summary(&title)
            .body(&body)
            .urgency(notify_rust::Urgency::Critical);
        if let Some(id) = replaces {
            n.id(id);
        }
        match n.show() {
            Ok(handle) => save_summary_state(&SummaryNotifyState {
                id: handle.id(),
                title,
                body,
            }),
            Err(e) => eprintln!("⚠️ failed to send summary notification: {}", e),
        }
    });
}

use crate::exclude::{
    excluded_dir_names_set, has_sync_relevant_dirty_entries, is_excluded_dir_name,
};
use crate::git::multi_remote::push_mirror_remotes;
use crate::git::{
    current_branch, detect_large_blobs_ahead, discover_git_repos, has_origin_remote,
    has_tracking_upstream, push_with_retries, remote_branch_exists, repo_diff_entries,
    rewrite_ahead_paths, run_git_capture_output, run_git_with_timeout, set_upstream_to_branch,
    top_level_dir,
};
use crate::policy::{
    timestamp_secs, RepoPolicyOverride, SyncPolicy, DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES,
};

fn ansi(color: &str, text: &str) -> String {
    if !crate::print::should_color() {
        return text.to_string();
    }
    let codes = match color {
        "31" => "31",
        "32" => "32",
        "33" => "33",
        "34" => "34",
        "35" => "35",
        "36" => "36",
        "37" => "37",
        "1" => "1",
        _ => "0",
    };
    format!("\x1b[{}m{}\x1b[0m", codes, text)
}

fn shorten_when(s: &str) -> String {
    let s = s.trim();

    // Parse "N minutes ago" and convert to hours+minutes if >= 60
    if let Some(rest) = s.strip_suffix(" minutes ago") {
        if let Ok(mins) = rest.parse::<u64>() {
            if mins >= 60 {
                let h = mins / 60;
                let m = mins % 60;
                if m == 0 {
                    return format!("{}h", h);
                }
                return format!("{}h {}m", h, m);
            }
            return format!("{}m", mins);
        }
    }
    if let Some(rest) = s.strip_suffix(" minute ago") {
        if let Ok(mins) = rest.parse::<u64>() {
            return format!("{}m", mins);
        }
    }

    // Convert seconds to minutes if >= 60
    if let Some(rest) = s.strip_suffix(" seconds ago") {
        if let Ok(secs) = rest.parse::<u64>() {
            if secs >= 60 {
                let m = secs / 60;
                let s_remainder = secs % 60;
                if s_remainder == 0 {
                    return format!("{}m", m);
                }
                return format!("{}m {}s", m, s_remainder);
            }
            return format!("{}s", secs);
        }
    }
    if let Some(rest) = s.strip_suffix(" second ago") {
        if let Ok(secs) = rest.parse::<u64>() {
            return format!("{}s", secs);
        }
    }

    // Convert hours to days when >= 24
    if let Some(rest) = s.strip_suffix(" hours ago") {
        if let Ok(hrs) = rest.parse::<u64>() {
            if hrs >= 24 {
                let d = hrs / 24;
                let h = hrs % 24;
                if h == 0 {
                    return format!("{}d", d);
                }
                return format!("{}d {}h", d, h);
            }
            return format!("{}h", hrs);
        }
    }
    if let Some(rest) = s.strip_suffix(" hour ago") {
        if let Ok(hrs) = rest.parse::<u64>() {
            return format!("{}h", hrs);
        }
    }

    // Convert days to weeks when >= 7
    if let Some(rest) = s.strip_suffix(" days ago") {
        if let Ok(days) = rest.parse::<u64>() {
            if days >= 7 {
                let w = days / 7;
                let d = days % 7;
                if d == 0 {
                    return format!("{}w", w);
                }
                return format!("{}w {}d", w, d);
            }
            return format!("{}d", days);
        }
    }
    if let Some(rest) = s.strip_suffix(" day ago") {
        if let Ok(days) = rest.parse::<u64>() {
            return format!("{}d", days);
        }
    }

    // Convert months to years when >= 12
    if let Some(rest) = s.strip_suffix(" months ago") {
        if let Ok(months) = rest.parse::<u64>() {
            if months >= 12 {
                let y = months / 12;
                let mo = months % 12;
                if mo == 0 {
                    return format!("{}y", y);
                }
                return format!("{}y {}mo", y, mo);
            }
            return format!("{}mo", months);
        }
    }
    if let Some(rest) = s.strip_suffix(" month ago") {
        if let Ok(months) = rest.parse::<u64>() {
            return format!("{}mo", months);
        }
    }

    // Weeks and years stay as-is (w, y)
    s.replace(" weeks ago", "w")
        .replace(" week ago", "w")
        .replace(" years ago", "y")
        .replace(" year ago", "y")
}

/// Render the ACTIVITY column. The original column was just the
/// time of the last commit (a duplicate of the LAST COMMIT column),
/// which made it impossible to tell whether a row was "actively
/// being processed" or "stalled" when the timestamp was the same
/// across many rows. This function returns a real activity label:
///
///   - "now"        : daemon has an in-flight task for this repo
///     (currently being processed)
///   - "waiting Xm" : push_status=PENDING, but no fresh in-flight
///     marker exists; the commit is queued for a retry or remote
///     confirmation rather than being shown as actively pushing
///   - "dirty Xm"    : dirty tracked work exists, last commit
///     was X minutes ago
///   - "synced Xm"  : clean, in sync, recent commit (within 1h)
///   - "idle Xm"    : clean, no in-flight, last commit 1h-24h ago
///   - "cold Xd"    : clean, no activity for > 24h
///   - "—"          : unknown / no data
// Legacy formatter retained only for its compatibility fixtures. Production
// convergence uses current_health and never infers work age from commit age.
#[cfg(test)]
pub(crate) fn activity_label(row: &RepoReportRow) -> String {
    let base = activity_label_base(row);
    // v0.113.13 (goal-list 2026-07-29): surface daemon-excluded dirty
    // entries as `· N excl` so the operator can see that e.g.
    // `.pi-glla/active.jsonl` sits dirty BY POLICY without it driving
    // the dirty-clock or WARN (those already use the adjusted counts).
    if row.excluded_dirty > 0 {
        format!("{} · {} excl", base, row.excluded_dirty)
    } else {
        base
    }
}

#[cfg(test)]
fn activity_label_base(row: &RepoReportRow) -> String {
    // Parse the last_when string ("N minutes ago", "N hours ago", etc.)
    // into a number of minutes. Returns None if unparseable.
    let last_when_mins = parse_relative_minutes_to_u64(&row.last_when);
    let in_flight = load_in_flight_for_path(&row.repo);

    // 1. in-flight = "now" — but only for rows whose state can
    //    legitimately be in-flight. A `Synced` / `Idle` / `Cold` /
    //    `Untracked` / `Healthy` row is clean; the in_flight
    //    entry for it is leftover from a previous cycle and
    //    should be ignored. This eliminates false "🔄 now"
    //    indicators on OK/idle/synced repos.
    if in_flight {
        let in_flight_state_suppressed = matches!(
            row.state_cause,
            StateCause::Synced
                | StateCause::Idle
                | StateCause::Cold
                | StateCause::Untracked
                | StateCause::Healthy
        );
        if !in_flight_state_suppressed {
            return "🔄 now".to_string();
        }
    }

    // 2. PENDING without a fresh in-flight marker is a queued/retry
    // state, not proof that a git push process is currently running.
    // The report command can observe the daemon between cycles, after a
    // transient network failure, or while the remote-tracking ref catches
    // up. Show that distinction directly instead of claiming "pushing".
    if row.push_status == "PENDING" {
        // FIXED 2026-10-03 (audit R3-L36): shorten large durations
        // ("43200m" truncated and dropped columns; "30d" fits).
        let duration = last_when_mins
            .map(|m| format!(" {}", shorten_mins(m)))
            .unwrap_or_default();
        let ahead_suffix = if row.ahead > 0 {
            format!(" ({} ahead)", row.ahead)
        } else {
            String::new()
        };
        // v0.113.87: while the daemon is frozen, "waiting" is the wrong
        // word — nothing is waiting to be dispatched, the dispatcher is
        // switched off. Name the cause on the row. The number stays the
        // ROW's own wait age; the freeze's age is on the banner/footer.
        if row.frozen_secs.is_some() {
            return format!("⏸️ frozen{duration}{ahead_suffix}");
        }
        return format!("🟡 waiting{}{}", duration, ahead_suffix);
    }

    // 2b. push_status PUSH_STUCK = retry budget exhausted, the
    // daemon has given up auto-pushing. Show `🛑 push-stuck Xm`
    // so the operator knows to investigate. The HINT column
    // names the actual error.
    if row.push_status == "PUSH_STUCK" {
        // FIXED 2026-10-03 (audit R3-L36): shorten large durations
        // ("43200m" truncated and dropped columns; "30d" fits).
        let duration = last_when_mins
            .map(|m| format!(" {}", shorten_mins(m)))
            .unwrap_or_default();
        let ahead_suffix = if row.ahead > 0 {
            format!(" ({} ahead)", row.ahead)
        } else {
            String::new()
        };
        return format!("🛑 push-stuck{}{}", duration, ahead_suffix);
    }

    // 2c. Unowned = "🚫 unowned: <reason>" so the operator
    // knows the daemon is intentionally not touching this repo.
    // The detail is aggressively truncated to 20 chars so the cell fits
    // in a 15-col ACTIVITY column with the leading label (13 cols) and
    // room for the truncation marker. Full reason is in the HINT column.
    if let StateCause::Unowned { detail, .. } = &row.state_cause {
        return format!("🚫 unowned: {}", truncate(detail, 20));
    }

    if matches!(row.state_cause, StateCause::Stalled) {
        return format!(
            "🔴 stalled {}",
            last_when_mins
                .map(shorten_mins)
                .unwrap_or_else(|| "?".to_string())
        );
    }

    let has_dirty = row.modified > 0 || row.staged > 0;

    // 3. dirty repo — show time since last commit.
    if has_dirty {
        return format!(
            "⏳ dirty {}",
            last_when_mins
                .map(shorten_mins)
                .unwrap_or_else(|| "?".to_string())
        );
    }

    // 5-7. clean repos: synced / idle / cold
    match last_when_mins {
        None => "—".to_string(),
        Some(m) if m < 60 => format!("🟢 synced {}m", m),
        Some(m) if m < 60 * 24 => {
            format!("⚪ idle {}", shorten_mins(m))
        }
        Some(m) => format!("⚫ cold {}", shorten_mins_days(m)),
    }
}

fn branch_upstream(repo: &Path, branch: &str) -> (String, PublishState) {
    let upstream = crate::policy::std_git_command()
        .args(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        });
    if let Some(upstream) = upstream.filter(|s| !s.is_empty()) {
        let state = if remote_tracking_ref_exists(repo, &upstream) {
            PublishState::Ok
        } else {
            PublishState::Gone
        };
        return (upstream, state);
    }

    if !crate::git::is_safe_branch_name(branch) {
        return ("-".to_string(), PublishState::Missing);
    }
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");
    let remote = crate::policy::std_git_command()
        .args(["config", "--get", &remote_key])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        });
    let merge = crate::policy::std_git_command()
        .args(["config", "--get", &merge_key])
        .current_dir(repo)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        });
    match (remote, merge) {
        (Some(remote), Some(merge)) if merge.starts_with("refs/heads/") => {
            let branch = merge.strip_prefix("refs/heads/").unwrap_or("");
            if crate::git::is_safe_branch_name(branch) {
                let label = format!("{remote}/{branch}");
                let state = if remote_tracking_ref_exists(repo, &label) {
                    PublishState::Ok
                } else {
                    PublishState::Gone
                };
                (label, state)
            } else {
                ("-".to_string(), PublishState::Missing)
            }
        }
        _ => ("-".to_string(), PublishState::Missing),
    }
}

fn publish_cell_label(upstream: &str, state: PublishState, width: usize) -> String {
    // 2026-07-19 (goal `4555eaf6`): worst-case is
    // `⚠️ origin/main (gone)` = 22 chars. FIXED 2026-10-03 (audit
    // L18): the budget is PER-TIER — compact PUBLISH is Absolute(18)
    // (content 16), full is Absolute(17) (content 15). The old shared
    // 16 overflowed full by 1 col. Common case (`gitlab/main`,
    // `origin/main`) is 11 chars and unaffected at either budget.
    let raw = match state {
        PublishState::Missing => "⚠️ none".to_string(),
        PublishState::Gone => format!("⚠️ {upstream} (gone)"),
        PublishState::Ok => upstream.to_string(),
    };
    truncate_unicode_width(&raw, width)
}

fn publish_state_color(state: PublishState) -> comfy_table::Color {
    match state {
        PublishState::Missing => comfy_table::Color::Yellow,
        PublishState::Gone => comfy_table::Color::Yellow,
        PublishState::Ok => comfy_table::Color::Green,
    }
}

/// Format the "PUSH-TO" column for a single repo. Shows the effective
/// remotes the daemon will push to (the `push_to_remotes` list), and
/// if the per-repo override excludes any remotes, shows them in a
/// subscript-style annotation so the operator can see both the active
/// targets AND why some are missing.
///
/// Examples:
/// - `["codeberg", "github", "gitlab"]` excl=[] → "codeberg,github,gitlab" (green)
/// - `["codeberg"]` excl=["github", "gitlab"] → "codeberg [excl:github,gitlab]" (yellow)
/// - `[]` excl=[] → "-" (dark grey — no remotes configured at all)
/// - reason `Some("private")` excl=["codeberg", "github"] →
///   " [codeberg:private,github]" (reason annotates the codeberg
///   element only — R4-SR-10; never the whole list)
///
/// CHANGED 2026-06-29: format changed from `codeberg −github,gitlab` (Unicode
/// minus) to `codeberg [excl:github,gitlab]` (brackets) for consistency with
/// the text-mode renderer at `format_status_block` (line 2699) which has
/// always used `[excl:...]`. The PUSH-TO column width was widened from
/// 17-18 to 32 chars in the same change to accommodate the longer string.
/// Map a cached-visibility read to the `codeberg_skip_reason` reason
/// string for a visibility-gate skip.
///
/// ADDED 2026-10-03 (audit R4-SR-10): the `Some(false)` ("public")
/// arm used to emit `"public"` — a TOCTOU between the gate's
/// `cached_repo_visibility` read (which excluded codeberg) and this
/// second read (cache flipped to public in between). The gate's
/// exclusion stands regardless, so a contradictory "public" reason
/// would annotate the row with a lie; collapse it to `"unknown"`.
fn codeberg_gate_reason(cached: Option<bool>) -> &'static str {
    match cached {
        Some(true) => "private",
        Some(false) | None => "unknown",
    }
}

/// Redact the stuck-ledger error once at report construction.
///
/// ADDED 2026-10-03 (audit R4-SR-11): the single binding both the
/// HINT branch and the `push_error` branch consume. The daemon
/// redacts at ledger-write time, but pre-2026-08-11 ledger entries
/// and any future writer bypass would otherwise echo
/// `https://user:token@host` credentials into the table + JSON
/// verbatim. `None` stays `None` (no stuck entry for this repo).
fn redacted_stuck_last_error(stuck_info: Option<&crate::daemon::StuckRepoEntry>) -> Option<String> {
    stuck_info.map(|info| crate::ownership::redact_url_credentials(&info.last_error))
}

fn format_push_to_remotes_cell(
    push_to_remotes: &[String],
    excluded_remotes: &[String],
    codeberg_skip_reason: Option<&str>,
) -> comfy_table::Cell {
    use comfy_table::Cell;
    if push_to_remotes.is_empty() && excluded_remotes.is_empty() {
        return Cell::new("-").fg(comfy_table::Color::DarkGrey);
    }
    let main = push_to_remotes.join(",");
    if excluded_remotes.is_empty() {
        // FIXED 2026-10-03 (audit R3-L35): budget 30 like the yellow
        // path (Absolute(32) minus 2 padding) — the old 22 clipped a
        // 4-remote fleet (29 chars) on the green path only. The
        // LowerBoundary comment was stale (F30v2 made it Absolute).
        Cell::new(truncate_unicode_width(&main, 30)).fg(comfy_table::Color::Green)
    } else {
        // Active remotes in green, excluded annotation in dim yellow
        // so the operator can see at a glance that the repo has been
        // deliberately limited to a subset of the default set.
        // Format mirrors the text-mode renderer at line 2699: bracket
        // annotation is more readable than a Unicode minus sign,
        // especially when terminal fonts differ.
        //
        // ADDED 2026-07-17: when `codeberg_skip_reason` is set
        // (policy-driven skip), annotate which reason so the
        // operator can distinguish policy from manual exclusion.
        let excl = excluded_remotes.join(",");
        let mut cell_text = format!("{main} [excl:{excl}]");
        if let Some(reason) = codeberg_skip_reason {
            // v0.113.18 (audit L4): the " (reason)" suffix pushed the
            // cell past the 30-col truncation budget, clipping the
            // annotation in exactly the rows it described. Fold the
            // reason INTO the bracket: `github,gitlab [codeberg:quota]`
            // is exactly 30 cols for the common case.
            //
            // FIXED 2026-10-03 (audit R4-SR-10): the old fold attached
            // the codeberg-specific reason to the WHOLE list —
            // `github,codeberg [github,codeberg:private]` blamed github
            // too. Annotate the codeberg element only; the common
            // single-exclusion case renders byte-identical.
            let mut matched = false;
            let annotated = excluded_remotes
                .iter()
                .map(|r| {
                    if r == "codeberg" {
                        matched = true;
                        format!("{r}:{reason}")
                    } else {
                        r.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            cell_text = if matched {
                format!("{main} [{annotated}]")
            } else {
                // Unreachable: the constructor only sets the reason when
                // codeberg is excluded. Drop the annotation rather than
                // misattribute it; the JSON row keeps the reason.
                format!("{main} [{excl}]")
            };
        }
        // F30v2: truncate to fit the Absolute(32) PUSH-TO column
        // minus 2 padding = 30 cols content.
        Cell::new(truncate_unicode_width(&cell_text, 30)).fg(comfy_table::Color::Yellow)
    }
}

/// Extracted from `report_effective_remotes` on 2026-11-09 (audit item 5)
/// so the legs and the forge families travel together; the tuple-returning
/// wrapper was removed rather than left as dead code, because a caller that
/// gets only legs would reintroduce exactly the leg/forge confusion this
/// change fixes.
pub(crate) struct ReportEffectiveRemotes {
    /// Publication legs (`origin`, mirror names).
    pub legs: Vec<String>,
    /// Forge family per leg, in the same order (`github` / `gitlab` /
    /// `codeberg`, or the remote name when the host is not a recognised
    /// forge — an internal git server must never render as GitHub).
    pub forges: Vec<String>,
    /// The operator's explicit opt-outs.
    pub excluded: Vec<String>,
    /// Mirrors the forge-identity dedupe dropped.
    pub duplicate_mirrors: Vec<String>,
}

pub(crate) struct ReportEffectiveRemotes {
    /// Publication legs (`origin`, mirror names).
    pub legs: Vec<String>,
    /// Forge family per leg, in the same order (`github` / `gitlab` /
    /// `codeberg`, or the remote name when the host is not a recognised
    /// forge — an internal git server must never render as GitHub).
    pub forges: Vec<String>,
    /// The operator's explicit opt-outs.
    pub excluded: Vec<String>,
    /// Mirrors the forge-identity dedupe dropped.
    pub duplicate_mirrors: Vec<String>,
}

pub(crate) fn report_effective_remote_legs(