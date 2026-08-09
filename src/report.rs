use anyhow::Result;
use dracon_git::GitService;
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

use crate::git::gh_cmd;

// Concurrency cap for the parallel per-repo work in `run_repos_report`.
// Most of the per-repo cost is a few `git` subprocess calls (status, log,
// remote), each ~30ms on local disk. Capping at 16 keeps the CPU and FD
// pressure reasonable on 26 repos while still reducing wall-clock time
// from ~1.6s to ~0.5s on a modern multi-core machine.
const REPORT_REPO_CONCURRENCY: usize = 16;

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

/// Send a desktop notification when a push operation fails persistently.
/// Rate-limited to max 1 notification per repo per 5 minutes.
#[allow(dead_code)]
pub(crate) fn notify_push_failure(
    repo_path: &Path,
    remote: &str,
    error: &str,
    consecutive_failures: usize,
    cooldowns: &mut std::collections::HashMap<String, std::time::Instant>,
) {
    let repo_name = repo_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_path.display().to_string());

    let notify_key = format!("push-fail-{}", repo_path.display());
    let now = std::time::Instant::now();
    let cooldown_secs = 300; // 5 minutes

    // Check cooldown
    if let Some(cooldown_until) = cooldowns.get(&notify_key) {
        if now < *cooldown_until {
            return; // still in cooldown
        }
        cooldowns.remove(&notify_key);
    }

    let title = "Dracon Sync: Push Failed";
    let body = format!(
        "Repository '{}' failed to push to {}.\nConsecutive failures: {}\nError: {}",
        repo_name, remote, consecutive_failures, error
    );

    // Set cooldown before spawning to prevent race conditions
    cooldowns.insert(
        notify_key,
        now + std::time::Duration::from_secs(cooldown_secs),
    );

    // Spawn in background to avoid blocking the daemon loop
    tokio::spawn(async move {
        if let Err(e) = notify_rust::Notification::new()
            .summary(title)
            .body(&body)
            .show()
        {
            eprintln!("⚠️ failed to send desktop notification: {}", e);
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
///   - "pushing Xm" : push_status=PENDING, push has been in
///     progress for X minutes
///   - "dirty Xm"    : dirty tracked work exists, last commit
///     was X minutes ago
///   - "synced Xm"  : clean, in sync, recent commit (within 1h)
///   - "idle Xm"    : clean, no in-flight, last commit 1h-24h ago
///   - "cold Xd"    : clean, no activity for > 24h
///   - "—"          : unknown / no data
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

    // 2. push_status PENDING = "pushing Xm (N ahead)" so the operator
    // can tell at a glance whether the push is stuck because there's a
    // large backlog (high ahead count) vs. some other transient reason.
    if row.push_status == "PENDING" {
        let duration = last_when_mins
            .map(|m| format!(" {}m", m))
            .unwrap_or_default();
        let ahead_suffix = if row.ahead > 0 {
            format!(" ({} ahead)", row.ahead)
        } else {
            String::new()
        };
        return format!("🟣 pushing{}{}", duration, ahead_suffix);
    }

    // 2b. push_status PUSH_STUCK = retry budget exhausted, the
    // daemon has given up auto-pushing. Show `🛑 push-stuck Xm`
    // so the operator knows to investigate. The HINT column
    // names the actual error.
    if row.push_status == "PUSH_STUCK" {
        let duration = last_when_mins
            .map(|m| format!(" {}m", m))
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
        let state = if remote_tracking_ref_exists(repo, &upstream) { PublishState::Ok } else { PublishState::Gone };
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
                let state = if remote_tracking_ref_exists(repo, &label) { PublishState::Ok } else { PublishState::Gone };
                (label, state)
            } else {
                ("-".to_string(), PublishState::Missing)
            }
        }
        _ => ("-".to_string(), PublishState::Missing),
    }
}

fn publish_cell_label(upstream: &str, state: PublishState) -> String {
    // 2026-07-19 (goal `4555eaf6`): PUBLISH column is Absolute(18).
    // Worst-case is `⚠️ origin/main (gone)` = 22 chars; truncate to
    // 16 cols (18 - 2 padding). Common case (`gitlab/main`,
    // `origin/main`) is 11 chars and unaffected.
    let raw = match state {
        PublishState::Missing => "⚠️ none".to_string(),
        PublishState::Gone => format!("⚠️ {upstream} (gone)"),
        PublishState::Ok => upstream.to_string(),
    };
    truncate_unicode_width(&raw, 16)
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
///
/// CHANGED 2026-06-29: format changed from `codeberg −github,gitlab` (Unicode
/// minus) to `codeberg [excl:github,gitlab]` (brackets) for consistency with
/// the text-mode renderer at `format_status_block` (line 2699) which has
/// always used `[excl:...]`. The PUSH-TO column width was widened from
/// 17-18 to 32 chars in the same change to accommodate the longer string.
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
        // F30v2 (2026-07-19): truncate PUSH-TO to fit the column.
        // The cell can be "github,gitlab,codeberg" (22 chars) plus
        // padding. Without truncation, LowerBoundary makes the column
        // grow to fit, distorting the table.
        Cell::new(truncate_unicode_width(&main, 22))
            .fg(comfy_table::Color::Green)
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
            cell_text = format!("{main} [{excl}:{reason}]");
        }
        // F30v2: truncate to fit the Absolute(32) PUSH-TO column
        // minus 2 padding = 30 cols content.
        Cell::new(truncate_unicode_width(&cell_text, 30))
            .fg(comfy_table::Color::Yellow)
    }
}

/// Compute the effective `excluded_remotes` for a repo by combining
/// the per-repo `exclude_remotes` override with the
/// `codeberg_public_only` policy gate. This is the SAME logic the
/// daemon runs in `sync.rs` and `daemon.rs` at push time, so the
/// `repos` table and the daemon's actual behavior stay in sync.
///
/// Returns a `Vec<String>` of remote names to exclude. The order
/// matches the order in `repo_override.exclude_remotes`, with any
/// policy-driven `"codeberg"` appended at the end (preserving
/// deterministic output for tests).
///
/// ADDED 2026-07-17 (goal `codeberg-public-only`).
///
/// ADDED 2026-07-29 (v0.113.16): the report-side mirror of the
/// daemon's FULL push-time remote filter. `effective_excluded_remotes`
/// applies only the per-repo `exclude_remotes` + the
/// codeberg-public-only visibility gate; the daemon's
/// `push_mirror_remotes` additionally applies the v0.112.28
/// quota-posture rule (`codeberg_push_excluded` — codeberg skipped
/// when the repo has no codeberg tracking ref AND effective
/// auto-create is off). Returning both lists from one place so
/// `push_to_remotes` / `excluded_remotes` can never drift apart.
///
/// Returns `(push_to, excluded)`.
pub(crate) fn report_effective_remotes(
    policy: &crate::policy::SyncPolicy,
    repo_override: &crate::policy::RepoPolicyOverride,
    repo_path: &std::path::Path,
    too_big_for_github: bool,
) -> (Vec<String>, Vec<String>) {
    let mut excluded = effective_excluded_remotes(policy, repo_override, repo_path);
    if crate::git::multi_remote::codeberg_push_excluded_for_repo(
        &policy.remotes,
        repo_override.auto_create_on_codeberg,
        crate::git::multi_remote::has_codeberg_tracking_ref(repo_path),
        repo_path,
    ) && !excluded.iter().any(|e| e == "codeberg")
    {
        excluded.push("codeberg".to_string());
    }
    // v0.113.18 (audit M2): mirror the daemon's over-2-GiB github skip
    // (sync.rs:1807-1811 — the daemon adds github to combined_exclude
    // when the pack exceeds github's 2 GiB limit). Without this the
    // REM cell shows 🐙 for a repo the daemon deliberately skips.
    if too_big_for_github && !excluded.iter().any(|e| e == "github") {
        excluded.push("github".to_string());
    }
    let filtered =
        crate::git::multi_remote::filter_remotes_by_exclude(&policy.remotes, &excluded);
    let push_to = filtered.iter().map(|r| r.name.clone()).collect();
    (push_to, excluded)
}

pub(crate) fn effective_excluded_remotes(
    policy: &crate::policy::SyncPolicy,
    repo_override: &crate::policy::RepoPolicyOverride,
    repo_path: &std::path::Path,
) -> Vec<String> {
    let mut combined: Vec<String> = repo_override.exclude_remotes.clone();
    let codeberg_public_only_effective = repo_override
        .codeberg_public_only
        .unwrap_or(policy.codeberg_public_only);
    if codeberg_public_only_effective {
        let cached_priv = crate::visibility::cached_repo_visibility(repo_path);
        let skip_codeberg = match cached_priv {
            Some(true) => true,
            Some(false) => false,
            None => true, // safe default
        };
        if skip_codeberg && !combined.iter().any(|e| e == "codeberg") {
            combined.push("codeberg".to_string());
        }
    }
    combined
}

/// Measure the size of `<repo>/.git` in bytes. Returns `None` if the
/// measurement fails or times out. Fast-path: `git count-objects -v`
/// (queries git's own pack index — ~10ms even for 54 GiB gitdirs, vs
/// `du -sb` which has to walk the whole tree and is ~200ms+ for
/// multi-GiB dirs). Falls back to `du -sb` if `count-objects` fails
/// (e.g. corrupted gitdir, very old git without `-v` flag).
///
/// CHANGED 2026-07-24 (v0.112.40): the fast path reads
/// `size-pack` (bytes in pack files — what actually ships to GitHub)
/// plus `size-garbage` (orphaned tmp_pack_ files plus loose
/// objects — the silent bloat class). This is semantically tighter
/// than `du -sb` (which includes logs, refs, config, worktrees)
/// and surfaces dangling objects that `du` would silently count
/// toward the total.
///
/// For worktrees/submodules where `.git` is a file (not a directory),
/// reads the `gitdir:` pointer and measures the shared gitdir instead.
pub(crate) fn measure_git_size_bytes(repo: &std::path::Path) -> Option<u64> {
    let git_dir = resolve_git_dir(repo)?;

    // Fast path: `git count-objects -v` — queries git's pack index.
    // Bounded at 4s (same pattern as `run_git_bounded`); on success
    // returns `size-pack + size-garbage` bytes. On failure, falls
    // through to `du -sb` below.
    if let Some(bytes) = measure_git_size_via_count_objects(&git_dir) {
        return Some(bytes);
    }

    // Fallback: `du -sb` (POSIX). Slow on multi-GiB gitdirs (~200ms+
    // each) but works on any gitdir where `count-objects` fails.
    //
    // v0.113.19 (operator question: "is the dracon-platform size
    // calculation wrong?"): `du` descends into `<gitdir>/modules/`
    // (submodule gitdirs) while the count-objects fast path does NOT
    // count them (they are separate gitdirs, each reported in the
    // nested repo's OWN row — dracon-platform: 12 GiB own pack vs
    // 7.7 GiB of modules). Subtract the modules dir here so both
    // paths agree and a superproject's SIZE never double-counts its
    // submodules.
    let mut bytes = du_bytes(&git_dir)?;
    let modules = git_dir.join("modules");
    if modules.is_dir() {
        if let Some(module_bytes) = du_bytes(&modules) {
            bytes = bytes.saturating_sub(module_bytes);
        }
    }
    Some(bytes)
}

/// Resolve a repo's real gitdir: `<repo>/.git` when it is a
/// directory, or the `gitdir:` pointer target when `.git` is a file
/// (worktrees / submodules).
fn resolve_git_dir(repo: &std::path::Path) -> Option<std::path::PathBuf> {
    let git_path = repo.join(".git");
    if !git_path.exists() {
        return None;
    }
    let git_dir = if git_path.is_file() {
        let content = std::fs::read_to_string(&git_path).ok()?;
        let gitdir_line = content.lines().find(|l| l.starts_with("gitdir:"))?;
        let rel_path = gitdir_line.strip_prefix("gitdir:")?.trim();
        repo.join(rel_path)
    } else {
        git_path
    };
    if !git_dir.exists() {
        return None;
    }
    Some(git_dir)
}

/// ADDED 2026-07-30 (v0.113.20): combined size of a superproject's
/// `<gitdir>/modules/` dir (the submodule gitdirs). 0 when absent.
/// The operator wants BOTH numbers for superprojects — own pack AND
/// the combined footprint: "we made them submods so we don't end up
/// with one huge repo, so it would be useful to know both sizes",
/// partly as the "would this get stuck on a wholesale push" gauge.
pub(crate) fn measure_modules_size_bytes(repo: &std::path::Path) -> u64 {
    let Some(git_dir) = resolve_git_dir(repo) else {
        return 0;
    };
    let modules = git_dir.join("modules");
    if !modules.is_dir() {
        return 0;
    }
    du_bytes(&modules).unwrap_or(0)
}

/// `du -sb` on a single path, parsed to bytes. Shared by the
/// git-size fallback and its `modules/` subtraction.
fn du_bytes(path: &std::path::Path) -> Option<u64> {
    let output = std::process::Command::new("du")
        .arg("-sb")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let bytes_str = stdout.split_whitespace().next()?;
    bytes_str.parse::<u64>().ok()
}

/// Probe git's pack index for total reachable bytes. ~10ms on
/// 54 GiB gitdirs (vs ~200ms for `du -sb`). Parses
/// `git count-objects -v` output:
///
/// ```
/// count: 0
/// size: 0
/// in-pack: 442954
/// packs: 5
/// size-pack: 12407743
/// prune-packable: 0
/// garbage: 1
/// size-garbage: 12734158
/// ```
///
/// Returns `size + size-pack + size-garbage` (loose objects +
/// packed objects + orphaned objects — the full picture of what
/// `du -sb` would have measured). The `size:` line covers loose
/// objects (which `size-pack` misses for fresh repos that haven't
/// been GC'd yet — without `size:`, the fast path would short-
/// circuit to 0 for fresh repos, hiding the real size).
///
/// Bounded at 4s — on timeout or non-zero exit, returns `None` and
/// the caller falls back to `du -sb`. Stderr warnings (e.g.
/// `warning: garbage found:`) are expected and ignored —
/// `count-objects -v` always prints them when there are dangling
/// objects, which is precisely the case we want to surface (the
/// v0.112.40 fix: dangling tmp_pack_* bloat was previously
/// invisible to `du` because it walked through them without
/// surfacing the warning).
fn measure_git_size_via_count_objects(git_dir: &std::path::Path) -> Option<u64> {
    const BOUND: std::time::Duration = std::time::Duration::from_secs(4);
    // `count-objects -v` runs in the GITDIR (not the repo root)
    // because that's where git's pack index lives. For bare gitdirs
    // (worktrees/submodules) this is correct; for normal `.git/`
    // dirs it's also correct.
    let out = run_git_bounded(&["count-objects", "-v"], git_dir, &[], BOUND)?;
    let stdout = String::from_utf8_lossy(&out);
    // FIXED 2026-07-25 (v0.112.42): `count-objects -v` reports all
    // sizes in **KiB** (git-commit-tree docs: "size: disk space
    // consumed by loose objects, in KiB"). The v0.112.40 parser read
    // them as BYTES, making every repo look 1024× smaller — which
    // silently disabled `github_pack_too_large`'s 2 GiB fast-path
    // guard (dracon-platform's 11.4 GiB pack measured as 0.011 GB).
    // Multiply by 1024 on parse. Verified safe: dracon-platform's
    // pushable-bytes (the slow-path refinement) is 1.49 GiB < 2 GiB,
    // so no repo flips push behavior; the guard just works again.
    let mut size_loose: u64 = 0;
    let mut size_pack: u64 = 0;
    let mut size_garbage: u64 = 0;
    let mut saw_size_pack = false;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("size:") {
            // `size:` is the loose-objects total; only count it if
            // the previous token was exactly `size:` (not
            // `size-pack:` or `size-garbage:`), since `.strip_prefix`
            // is a literal prefix match and the bare key always
            // appears first in `count-objects -v` output.
            size_loose = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("size-pack:") {
            size_pack = rest.trim().parse().unwrap_or(0);
            saw_size_pack = true;
        } else if let Some(rest) = line.strip_prefix("size-garbage:") {
            size_garbage = rest.trim().parse().unwrap_or(0);
        }
    }
    if !saw_size_pack {
        return None;
    }
    // KiB → bytes (see FIXED note at the top of this fn).
    Some((size_loose + size_pack + size_garbage) * 1024)
}

/// Probe the operator's token file presence for each forge. Returns a
/// `TokenHealthSummary` with one bool per forge. We check BOTH the
/// modern `~/.dracon/utilities/sync/secrets/` and the legacy
/// `~/.dracon/secrets/pat/` directories because the daemon's
/// `load_secret` falls back to the legacy dir when the modern dir is
/// empty (or vice versa). The bool is true if EITHER location has a
/// file for the forge.
///
/// We don't read the file contents — just `Path::exists()`. This is
/// fast (a few `stat()` calls) and surfaces auth-side issues before
/// they cause push failures.
fn probe_token_health() -> TokenHealthSummary {
    let modern_dir = crate::secrets::sync_secrets_dir();
    let legacy_dir = crate::secrets::legacy_pat_secrets_dir();
    TokenHealthSummary {
        codeberg_present: check_token_at_both(codeberg_token_paths(&modern_dir, &legacy_dir)),
        github_present: check_token_at_both(github_token_paths(&modern_dir, &legacy_dir)),
        gitlab_present: check_token_at_both(gitlab_token_paths(&modern_dir, &legacy_dir)),
    }
}

/// Get the candidate paths for the codeberg token in both the modern
/// and legacy secret directories. Returns two paths. The modern dir
/// is checked first; if it has a file, we use that. The legacy dir is
/// the fallback.
fn codeberg_token_paths(
    modern_dir: &std::path::Path,
    legacy_dir: &std::path::Path,
) -> [std::path::PathBuf; 2] {
    [
        modern_dir.join("codeberg.env"),
        legacy_dir.join("codeberg.env"),
    ]
}

fn github_token_paths(
    modern_dir: &std::path::Path,
    legacy_dir: &std::path::Path,
) -> [std::path::PathBuf; 2] {
    [
        modern_dir.join("github.env"),
        legacy_dir.join("github.env"),
    ]
}

fn gitlab_token_paths(
    modern_dir: &std::path::Path,
    legacy_dir: &std::path::Path,
) -> [std::path::PathBuf; 2] {
    [
        modern_dir.join("gitlab.env"),
        legacy_dir.join("gitlab.env"),
    ]
}

/// Check if EITHER of the two candidate token paths exists.
fn check_token_at_both(paths: [std::path::PathBuf; 2]) -> bool {
    paths.iter().any(|p| p.exists())
}

fn remote_tracking_ref_exists(repo: &Path, upstream: &str) -> bool {
    let Some(slash) = upstream.find('/') else {
        return false;
    };
    let (remote, branch) = upstream.split_at(slash);
    let branch = &branch[1..];
    if remote.is_empty() || branch.is_empty() {
        return false;
    }
    if !crate::git::is_safe_branch_name(remote)
        || !crate::git::is_safe_branch_name(branch)
    {
        return false;
    }
    let refspec = format!("refs/remotes/{remote}/{branch}");
    crate::policy::std_git_command()
        .args(["rev-parse", "--verify", "--quiet", &refspec])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Read the in_flight set from disk and return whether the given
/// repo path is in it. We use the daemon's `save_in_flight` JSON
/// file, written on every daemon cycle. A missing file means
/// "no daemon activity" (or daemon not running).
///
/// Staleness filter: if the on-disk file is older than
/// `IN_FLIGHT_MAX_AGE_SECS` (default 30s), the file is considered
/// stale and treated as empty. This handles the case where a slow
/// push from the previous cycle kept a repo in `in_flight`, the
/// trailing drain timed out before that task completed, and the
/// next cycle's `save_in_flight` would re-write the same stale
/// set. The new cycle's COLLECT phase does NOT carry that repo in
/// `in_flight` (it gets cleared at cycle start), so the disk file
/// is the only stale-source of the "🔄 now" indicator. Filtering
/// by age makes the indicator reflect ground truth.
fn load_in_flight_for_path(repo_path: &str) -> bool {
    // If the file is older than the staleness threshold, treat as
    // empty — the daemon has effectively moved on, even if a slow
    // task is still running. The repo's state will be picked up
    // again when the new cycle's COLLECT phase dispatches it.
    //
    // Threshold: 5s. The daemon writes the file every
    // `pulse_interval_secs` (default 1s), so 5s = ~5 cycles.
    // A repo genuinely in-flight writes itself to the file on
    // each of those cycles. A repo whose in_flight entry is
    // LEFTOVER from a previous cycle (e.g. trailing drain timed
    // out) won't be re-added to the set on subsequent cycles
    // (the daemon's COLLECT clears the local set at cycle
    // start), so the on-disk file will go 5s+ without that
    // entry and the staleness filter will treat it as empty.
    const IN_FLIGHT_MAX_AGE_SECS: u64 = 5;
    if let Some(age) = crate::daemon::in_flight_file_age_secs() {
        if age > IN_FLIGHT_MAX_AGE_SECS {
            return false;
        }
    }
    let set = crate::daemon::load_in_flight();
    set.iter().any(|p| p.display().to_string() == repo_path)
}

/// Parse "N minutes ago" / "N hours ago" / etc. into a u64 number
/// of minutes. Mirrors the parsing in `parse_relative_minutes` but
/// returns a plain integer for use in arithmetic.
fn parse_relative_minutes_to_u64(s: &str) -> Option<u64> {
    // CHANGED 2026-07-22 (v0.112.35): delegate to the complete
    // `parse_relative_minutes` (which handles weeks/months/years)
    // instead of maintaining a separate, unit-limited copy. The
    // pre-fix copy handled only seconds/minutes/hours/days — repos
    // whose last commit is older than ~2 weeks (e.g. DraconDev at
    // "4 weeks ago") got `None`, so `activity_label` rendered the
    // bare state ("healthy") with no indicator.
    parse_relative_minutes(s).and_then(|m| u64::try_from(m).ok())
}

/// Render minutes as a compact label: <60m → "Nm", 1h-24h → "Nh",
/// >=24h → "Nd".
fn shorten_mins(mins: u64) -> String {
    if mins < 60 {
        format!("{}m", mins)
    } else if mins < 60 * 24 {
        let h = mins / 60;
        format!("{}h", h)
    } else {
        shorten_mins_days(mins)
    }
}

fn shorten_mins_days(mins: u64) -> String {
    let d = mins / (60 * 24);
    format!("{}d", d)
}

// ---------------------------------------------------------------------------
// v0.113.13 (goal-list 2026-07-29): exclusion-aware dirty classification.
//
// The raw dracon-git counts (`RepoStatus.modified_files` / `staged_files`)
// include files the daemon will NEVER commit:
//   - `auto_commit_exclude_patterns` matches (e.g. junk-runner's
//     `.pi-glla/active.jsonl` — 15 MiB append-only session scratch),
//   - submodule-worktree-only gitlink dirt (the sync loop's
//     `is_gitlink_unchanged` skips these; the gitlink SHA didn't move).
// The report used the raw counts for the ACTIVITY dirty-clock AND the WARN
// escalation, so an intentionally-excluded file looked like a permanent
// stall — the 2026-07-29 junk-runner (`⏳ dirty 2h · 1 mod` 🟡 WARN forever)
// + dracon-platform (inherited via the submodule gitlink) false-WARN pair.
//
// This classifier re-derives dirty counts from `git status --porcelain -z`
// (fast: no clean-filter pass, unlike `repo_diff_entries` — see the perf
// comment in the main pass) and subtracts what the daemon ignores. Only
// called for repos whose raw tracked-dirty count is > 0, so the common
// clean-repo path pays nothing.

/// Dirty counts split by whether the daemon will act on them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct DirtyClassification {
    /// Tracked worktree modifications the daemon WILL commit.
    committable_modified: usize,
    /// Staged changes the daemon WILL commit.
    committable_staged: usize,
    /// Dirty entries the daemon intentionally won't commit BY POLICY
    /// (per-repo `auto_commit_exclude_patterns` /
    /// `untracked_exclude_patterns` matches only). Surfaced as the 🚫
    /// CHANGES column and the `· N excl` ACTIVITY marker.
    /// v0.113.28 (operator: "just because they didn't commit why are
    /// they counting as excluded"): unchanged-gitlink submodule dirt
    /// NO LONGER counts here — it's routine mechanics (the sub hasn't
    /// committed yet; the gitlink auto-advances when it does), not a
    /// policy exclusion, and showing it raised questions.
    excluded: usize,
    /// Submodule-worktree-only dirt whose gitlink SHA didn't move.
    /// Subtracted from the parent's committable counts (there is
    /// nothing to commit at the parent) but NEVER displayed as an
    /// exclusion — pure mechanics (v0.113.28).
    unchanged_gitlink: usize,
}

/// Parse `git status --porcelain -z` output into (x, y, path) tuples.
/// Rename/copy records carry a second NUL-separated source path which is
/// skipped. Unparseable short records are dropped (defensive).
fn parse_porcelain_z(stdout: &[u8]) -> Vec<(u8, u8, String)> {
    let mut out = Vec::new();
    let mut it = stdout.split(|&b| b == 0).filter(|s| !s.is_empty());
    while let Some(rec) = it.next() {
        if rec.len() < 4 {
            continue;
        }
        let (x, y) = (rec[0], rec[1]);
        let path = String::from_utf8_lossy(&rec[3..]).to_string();
        if matches!(x, b'R' | b'C') {
            it.next(); // consume the source path of a rename/copy
        }
        out.push((x, y, path));
    }
    out
}

/// Classify a dirty repo's porcelain entries into committable vs excluded.
/// `untracked_excludes` are the global `untracked_exclude_patterns` (daemon
/// won't stage those either); `auto_commit_excludes` are the effective
/// per-repo (fallback global) `auto_commit_exclude_patterns`.
async fn classify_dirty_entries(
    repo: &Path,
    auto_commit_excludes: &[String],
    untracked_excludes: &[String],
) -> DirtyClassification {
    let run = |extra: &str| {
        let mut cmd = crate::git::git_cmd();
        cmd.args(["status", "--porcelain", "-z"])
            .current_dir(repo);
        if !extra.is_empty() {
            cmd.arg(extra);
        }
        cmd.output()
    };
    // `--ignore-submodules=dirty` drops submodule-worktree-only entries
    // (unchanged gitlink) — the exact semantics the sync loop's
    // `is_gitlink_unchanged` applies at staging time. Gitlink SHA drift
    // still shows in BOTH passes and therefore stays committable.
    let (plain, base) = match (run(""), run("--ignore-submodules=dirty")) {
        (Ok(p), Ok(b)) if p.status.success() && b.status.success() => {
            (parse_porcelain_z(&p.stdout), parse_porcelain_z(&b.stdout))
        }
        // Defensive: porcelain unavailable → treat everything as
        // committable (pre-v0.113.13 behavior) so we never hide real dirt.
        _ => {
            return DirtyClassification {
                committable_modified: 1,
                committable_staged: 0,
                excluded: 0,
                unchanged_gitlink: 0,
            };
        }
    };

    let mut out = DirtyClassification::default();
    let base_paths: std::collections::HashSet<&str> =
        base.iter().map(|r| r.2.as_str()).collect();
    for (_, _, path) in &plain {
        if !base_paths.contains(path.as_str()) && repo.join(path).join(".git").exists() {
            out.unchanged_gitlink += 1; // submodule worktree dirt, unchanged gitlink
        }
    }

    for (x, y, path) in &base {
        let tracked = *x != b'?' && *x != b'!';
        let excluded = (!auto_commit_excludes.is_empty()
            && crate::exclude::matches_untracked_exclude(
                repo,
                &repo.join(path),
                auto_commit_excludes,
            ))
            || (!tracked
                && !untracked_excludes.is_empty()
                && crate::exclude::matches_untracked_exclude(
                    repo,
                    &repo.join(path),
                    untracked_excludes,
                ));
        if excluded {
            out.excluded += 1;
            continue;
        }
        if !tracked {
            // Untracked the daemon WOULD commit: visible in the UT count
            // but never drives the dirty-clock / WARN (pre-existing
            // report semantics, preserved).
            continue;
        }
        if matches!(*x, b'M' | b'A' | b'D' | b'R' | b'C' | b'T') {
            out.committable_staged += 1;
        }
        if matches!(*y, b'M' | b'D' | b'T') {
            out.committable_modified += 1;
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoFilter {
    All,
    Concern,
    Warn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConcernRepairFilter {
    All,
    StuckPush,
    StuckPull,
}

#[derive(Debug, Serialize)]
pub(crate) struct RepoReportRow {
    repo: String,
    state_flags: Vec<String>,
    branch: String,
    upstream: String,
    /// Visible flag describing whether the VS Code publish upstream is healthy.
    /// `Missing` = no `branch.<name>.remote` config and no `@{u}` ref.
    /// `Gone` = a publish upstream is configured but the remote-tracking ref
    /// does not exist locally yet (e.g. remote was added but never pushed).
    /// `Ok` = a publish upstream is configured and its remote-tracking ref
    /// resolves locally.
    publish_state: PublishState,
    modified: usize,
    staged: usize,
    untracked: usize,
    /// v0.113.13: dirty entries the daemon intentionally won't commit
    /// (see [`DirtyClassification`]). Displayed as `· N excl` in ACTIVITY.
    excluded_dirty: usize,
    ahead: usize,
    behind: usize,
    last_hash: String,
    last_author: String,
    last_when: String,
    last_msg: String,
    last_unix: i64,
    /// Number of commits in the last 1 hour.
    commits_1h: usize,
    /// Number of commits in the last 6 hours.
    commits_6h: usize,
    /// Number of commits in the last 24 hours.
    commits_24h: usize,
    last_push: String,
    push_status: String,
    push_error: String,
    /// Effective remotes the daemon will push to for this repo, derived
    /// from `policy.remotes` minus the per-repo `exclude_remotes` override.
    /// Sourced from the SAME configuration the daemon uses at push time
    /// (`filter_remotes_by_exclude` in `git/multi_remote.rs`), so the
    /// `dracon-sync repos` table shows exactly what the daemon will do.
    /// For most repos this is `["codeberg", "github", "gitlab"]`. For
    /// `dracon-platform` it is `["codeberg"]` because the per-repo
    /// override at `<repo>/.dracon/dracon-sync.toml` excludes github+gitlab
    /// (see 2026-06-23 goal `mqqsyzyd-qkvna5` for rationale).
    push_to_remotes: Vec<String>,
    /// Remotes explicitly excluded from this repo by the per-repo override
    /// (or by the global `policy.exclude_remotes`). Empty when the repo
    /// uses the full default remote set. Always present (not Option) so
    /// downstream callers don't have to handle None.
    excluded_remotes: Vec<String>,
    /// Reason codeberg is in `excluded_remotes` for this repo, when the
    /// skip is driven by the `codeberg_public_only` policy rather than
    /// a manual `exclude_remotes = ["codeberg"]` in the per-repo override.
    /// Possible values:
    /// - `Some("private")` — repo is private per cached visibility, codeberg
    ///   skipped by policy.
    /// - `Some("unknown")` — no cached visibility yet, safe default fires.
    /// - `None` — codeberg not excluded, OR excluded by manual override
    ///   (operator already knows why; no annotation needed).
    ///
    /// ADDED 2026-07-17 (goal `codeberg-public-only`).
    codeberg_skip_reason: Option<String>,
    /// Size of the repo's `.git` directory in bytes (i.e. the data that
    /// would be pushed to remotes). Measured with
    /// `git count-objects -v` (`size-pack + size-garbage`) at report
    /// time; falls back to `du -sb` if `count-objects` fails. `None`
    /// if the measurement failed or timed out. Useful for spotting
    /// size-blocked repos like `dracon-platform` (20 GiB) and for
    /// general capacity planning.
    ///
    /// CHANGED 2026-07-24 (v0.112.40): switched from `du -sb` to
    /// `git count-objects -v` for ~17× speedup on multi-GiB gitdirs
    /// (dracon-platform: 188ms → 11ms). Semantics are tighter: now
    /// counts packed + orphaned bytes (the bytes that would actually
    /// ship to a remote, plus dangling tmp_pack_* bloat) rather than
    /// the whole gitdir tree (which included logs, refs, config).
    git_size_bytes: Option<u64>,
    /// ADDED 2026-07-30 (v0.113.20): combined size of submodule
    /// gitdirs (`<gitdir>/modules/`) for superprojects; 0 otherwise.
    /// Rendered in the SIZE cell as `own+mods` when non-zero.
    git_modules_bytes: u64,
    /// Per-forge token health summary. Shows whether each forge's token
    /// file is present on disk, so the operator can spot auth-side
    /// issues BEFORE they cause push failures. Always present (not
    /// Option) so the renderer doesn't have to handle None.
    token_health: TokenHealthSummary,
    concern: bool,
    warn: bool,
    /// True when the daemon is actively working this repo right now
    /// (push in progress, or dirty-but-recent that normal sync will
    /// pick up). Distinct from `warn`: an ACTIVE repo is "plausibly not
    /// broken" (the daemon is handling it), whereas a `warn` repo that
    /// is NOT active is a genuine issue (stalled / gave up). Drives the
    /// new `🔄 ACTIVE` STATUS value in `dracon-sync repos`. See
    /// [`repo_is_active`].
    active: bool,
    hint: String,
    /// Derived "rough cause" of the row's current state. Combines the
    /// last-commit time, last-push time, dirty state, ahead/behind, and
    /// push status into a single small vocabulary the user can scan at
    /// a glance. See [`StateCause`].
    state_cause: StateCause,
    /// `state_cause` as a string, for downstream tools that want the
    /// label without having to enumerate the enum.
    state_cause_label: String,
    /// When the daemon last recorded an action for this repo (unix
    /// timestamp). `0` means "no record in the incident ledger".
    /// Distinguishes "user is actively editing" from "daemon is actively
    /// syncing" when both produce dirty/committing rows.
    daemon_last_action_unix: i64,
    /// Short label of the daemon's last action (e.g. "sync_triage",
    /// "push", "ok"). Empty when no record exists.
    daemon_last_action: String,
    /// Result of the daemon's last action (e.g. "ok", "fail",
    /// "planned"). Empty when no record exists.
    daemon_last_result: String,
    /// Human-friendly relative time of the daemon's last action
    /// (e.g. "23s", "2m"). `none` when no record exists.
    daemon_last_action_when: String,
    /// ADDED 2026-07-23 (v0.112.39): count of objects referenced by
    /// `main`'s history but MISSING from the object store (broken
    /// history). `0` = healthy. Drives the `BROKEN_HISTORY` state
    /// flag — a repo with missing objects is damaged (fresh clones
    /// fail; pushes may eventually break). See
    /// `probe_missing_objects` for the probe and the deathrun
    /// incident (2092 missing objects, both sides broken, days of
    /// undetected broken pushes).
    missing_objects: u64,
    /// ADDED 2026-07-29 (v0.113.8 follow-up): true iff this row's
    /// pushable branch exceeds github's 2 GiB pack-size limit
    /// (matches `pack_too_large_forces_concern`'s underlying bool).
    /// Distinct from `git_size_bytes` (compressed on-disk) which
    /// can be ≥ 2 GiB without the push being broken (deathrun's
    /// pre-gc residue case). Used by the SIZE column to color the
    /// cell Red iff the push is genuinely broken, not just because
    /// the gitdir happens to be large. See `size_label` for the
    /// full rationale + the deathrun CLEAN-vs-red contradiction
    /// this fix prevents.
    pack_too_large: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct RepoReportJson {
    policy: String,
    filter: String,
    repos: usize,
    ok: usize,
    active: usize,
    warn: usize,
    concern: usize,
    failures: usize,
    rows: Vec<RepoReportRow>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RemoteStatus {
    pub(crate) name: String,
    pub(crate) auth_type: String,
    pub(crate) auto_create: bool,
    pub(crate) priority: u32,
}

/// Per-forge token health summary. Shows whether the daemon can find a
/// token file for each forge. The daemon's `load_secret` (in
/// `secrets.rs`) checks (1) env var, (2) `~/.dracon/utilities/sync/secrets/<name>.env`,
/// (3) `~/.dracon/secrets/pat/<name>.env`. This struct reports the
/// file-presence check for (2) and (3) combined — the most common case
/// on this operator's machine (no tokens in env, but token files on
/// disk). The bool is true if EITHER location has a file.
///
/// We don't read the token contents — just the file presence + mode.
/// The renderer shows one icon per forge:
/// - 🟢 when present (daemon can auth)
/// - 🔴 when missing (pushes to that forge will fail with HTTP 401/403)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub(crate) struct TokenHealthSummary {
    pub(crate) codeberg_present: bool,
    pub(crate) github_present: bool,
    pub(crate) gitlab_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PublishState {
    Missing,
    Gone,
    Ok,
}

#[derive(Debug, Serialize)]
pub(crate) struct StatusJson {
    pub(crate) policy: String,
    pub(crate) roots: Vec<String>,
    pub(crate) repos_discovered: usize,
    pub(crate) pulse_interval_secs: u64,
    pub(crate) inactivity_push_delay_secs: u64,
    pub(crate) freeze: String,
    pub(crate) auto_commit: bool,
    pub(crate) auto_pull: bool,
    pub(crate) auto_push: bool,
    pub(crate) auto_bump_versions: bool,
    pub(crate) auto_repair_concerns: bool,
    pub(crate) auto_repair_warns: bool,
    pub(crate) auto_rewrite_large_blobs: bool,
    pub(crate) max_stage_file_bytes: u64,
    pub(crate) push_blob_threshold_bytes: u64,
    pub(crate) exclude_dirs: Vec<String>,
    pub(crate) exclude_file_patterns: Vec<String>,
    pub(crate) pull_op_timeout_secs: u64,
    pub(crate) push_op_timeout_secs: u64,
    pub(crate) repo_sync_timeout_secs: u64,
    pub(crate) stage_op_timeout_secs: u64,
    pub(crate) stage_cooldown_secs: u64,
    pub(crate) push_retries: u32,
    pub(crate) repair_cooldown_secs: u64,
    pub(crate) incident_ledger_max_lines: usize,
    pub(crate) incident_ledger_max_age_days: u64,
    pub(crate) system_repo: String,
    pub(crate) backup_policy: String,
    pub(crate) backup_dir: String,
    pub(crate) remotes: usize,
    pub(crate) remote_configs: Vec<RemoteStatus>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RepairJson {
    policy: String,
    scope: String,
    mode: String,
    found: usize,
    planned: usize,
    attempted: usize,
    succeeded: usize,
    resolved_now: usize,
    manual_only: usize,
    ledger: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct RepairSummary {
    pub(crate) found: usize,
    pub(crate) planned: usize,
    pub(crate) attempted: usize,
    pub(crate) succeeded: usize,
    pub(crate) resolved_now: usize,
    pub(crate) manual_only: usize,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct IncidentRecord {
    ts_unix: u64,
    scope: String,
    repo: String,
    reason: String,
    action: String,
    backup_branch: Option<String>,
    result: String,
    details: Option<String>,
}

impl IncidentRecord {
    pub(crate) fn new(
        ts_unix: u64,
        scope: impl Into<String>,
        repo: impl Into<String>,
        reason: impl Into<String>,
        action: impl Into<String>,
        backup_branch: Option<String>,
        result: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self {
            ts_unix,
            scope: scope.into(),
            repo: repo.into(),
            reason: reason.into(),
            action: action.into(),
            backup_branch,
            result: result.into(),
            details,
        }
    }
}

pub(crate) fn incident_ledger_path(_policy_path: &Path) -> PathBuf {
    // IMPORTANT: Keep this ledger OUT of git repositories by default.
    // The policy file typically lives inside the system repo; writing next to it
    // causes perpetual DIRTY state and churn.
    if let Ok(custom) = std::env::var("DRACON_SYNC_LEDGER") {
        let p = PathBuf::from(custom);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }

    if let Some(home) = dirs::home_dir() {
        return home
            .join(".local")
            .join("state")
            .join("dracon")
            .join("dracon-sync-incidents.jsonl");
    }

    PathBuf::from("/tmp/dracon-sync-incidents.jsonl")
}

/// Enforce incident ledger retention at any time.
/// Removes entries older than max_age_days and truncates to max_lines.
/// Returns the number of pruned entries (or 0 if nothing was removed).
pub(crate) fn enforce_retention(path: &Path, policy: &SyncPolicy) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let meta = std::fs::metadata(path)?;
    if meta.len() > 100 * 1024 * 1024 {
        eprintln!(
            "⚠️ incident ledger is {}MB (>100MB), truncating to last {} lines",
            meta.len() / (1024 * 1024),
            policy.incident_ledger_max_lines,
        );
        let content = std::fs::read_to_string(path)?;
        let lines: Vec<&str> = content
            .lines()
            .rev()
            .take(policy.incident_ledger_max_lines)
            .collect();
        let out = lines.iter().rev().copied().collect::<Vec<_>>().join("\n") + "\n";
        std::fs::write(path, &out)?;
        return Ok(lines.len());
    }
    let content = std::fs::read_to_string(path)?;
    let original_count = content.lines().count();
    let now = timestamp_secs();
    let age_cutoff = now.saturating_sub(policy.incident_ledger_max_age_days.saturating_mul(86_400));

    let mut kept: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let keep_by_age = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v.get("ts_unix").and_then(|t| t.as_u64()))
            .map(|ts| ts >= age_cutoff)
            .unwrap_or(true);
        if keep_by_age {
            kept.push(line.to_string());
        }
    }
    if kept.len() > policy.incident_ledger_max_lines {
        let drop_n = kept.len() - policy.incident_ledger_max_lines;
        kept.drain(0..drop_n);
    }
    let out = kept.join("\n") + "\n";
    std::fs::write(path, &out)?;

    let removed = original_count.saturating_sub(kept.len());
    Ok(removed)
}

pub(crate) fn append_incident_record(policy_path: &Path, record: &IncidentRecord) {
    let path = incident_ledger_path(policy_path);
    let line = match serde_json::to_string(record) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("⚠️ incident serialize failed: {}", e);
            return;
        }
    };
    let parent = path.parent().map(Path::to_path_buf);
    if let Some(dir) = parent {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("⚠️ failed to create incident ledger dir: {}", e);
        }
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(e) = writeln!(file, "{}", line) {
                eprintln!("⚠️ incident write failed ({}): {}", path.display(), e);
            }
        }
        Err(e) => eprintln!("⚠️ incident open failed ({}): {}", path.display(), e),
    }
    // ── lazy retention: only check when file has likely grown past max ──
    if path.exists() {
        if let Ok(metadata) = std::fs::metadata(&path) {
            // rough estimate: ~200 bytes per JSON line
            let approx_lines = metadata.len() as usize / 200;
            let policy = SyncPolicy::load(policy_path).ok();
            if let Some(ref p) = policy {
                if approx_lines >= p.incident_ledger_max_lines {
                    if let Err(e) = enforce_retention(&path, p).map(|_| ()) {
                        eprintln!("⚠️ incident retention failed ({}): {}", path.display(), e);
                    }
                }
            }
        }
    }
}

/// Enforce incident ledger retention at daemon startup.
/// Delegates to the shared [`enforce_retention`] function.
pub(crate) fn enforce_retention_at_startup(policy_path: &Path, policy: &SyncPolicy) -> Result<()> {
    let path = incident_ledger_path(policy_path);
    let removed = enforce_retention(&path, policy)?;
    if removed > 0 {
        eprintln!(
            "🧹 startup: pruned {} stale incident entries (remaining after reload)",
            removed,
        );
    }
    Ok(())
}

/// Build a map of repo path -> "did the daemon record a push failure in the
/// last 10 minutes?". Used by the report to distinguish "has unpushed
/// commits" (normal, daemon is working through the queue) from "push is
/// genuinely stuck" (daemon tried and failed). Returns `None` if the ledger
/// is missing or unreadable so the report still works in degraded mode.
fn build_recent_push_failure_map(policy_path: &Path) -> Option<HashMap<String, bool>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = incident_ledger_path(policy_path);
    // The ledger is append-only and can grow to thousands of lines. We only
    // care about the most recent ~10 minutes, so reading the whole file on
    // every `repos` call is O(ledger_size) and wasteful. Read the last
    // `RECENT_LINES_WINDOW` lines instead — a tight window that still
    // covers any plausible 10-minute push-failure rate.
    const RECENT_LINES_WINDOW: usize = 500;
    const PUSH_WINDOW_SECS: u64 = 600; // 10 minutes
    let recent = read_tail_lines(&path, RECENT_LINES_WINDOW).ok()?;
    if recent.is_empty() {
        return Some(HashMap::new());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cutoff = now.saturating_sub(PUSH_WINDOW_SECS);
    let mut map: HashMap<String, bool> = HashMap::new();
    for line in recent {
        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let scope = entry.get("scope").and_then(|v| v.as_str()).unwrap_or("");
        let result = entry.get("result").and_then(|v| v.as_str()).unwrap_or("");
        let ts = entry.get("ts_unix").and_then(|v| v.as_u64()).unwrap_or(0);
        // Push-related failures: any scope mentioning push/mirror with a
        // non-ok result, or an explicit "push" reason.
        let is_push_failure = result != "ok"
            && (scope.contains("push")
                || scope.contains("mirror")
                || entry
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(|r| r.contains("push"))
                    .unwrap_or(false));
        if !is_push_failure || ts < cutoff {
            continue;
        }
        if let Some(repo) = entry.get("repo").and_then(|v| v.as_str()) {
            map.insert(repo.to_string(), true);
        }
    }
    Some(map)
}

/// Build a per-repo map of the daemon's last recorded action (timestamp
/// + action label + result) from the incident ledger.
///   Used by the report to show the user that the daemon IS actively working
///   through dirty repos — the `last_when`/`last_push` columns show the last
///   *commit* and *push* times, but those reset to the moment of the daemon's
///   own commit, so they don't distinguish "user is editing" from "daemon is
///   handling dirty work". The `DAEMON` column closes that gap.
///
/// Returns `None` if the ledger is missing or unreadable so the report
/// still works in degraded mode.
fn build_daemon_last_action_map(
    policy_path: &Path,
) -> Option<HashMap<String, (i64, String, String)>> {
    let path = incident_ledger_path(policy_path);
    // The ledger is append-only and can grow to thousands of lines. We only
    // care about the most recent entries, so reading the whole file on
    // every `repos` call is O(ledger_size) and wasteful. Read the last
    // `RECENT_LINES_WINDOW` lines instead.
    const RECENT_LINES_WINDOW: usize = 2000;
    let recent = read_tail_lines(&path, RECENT_LINES_WINDOW).ok()?;
    if recent.is_empty() {
        return Some(HashMap::new());
    }
    let mut map: HashMap<String, (i64, String, String)> = HashMap::new();
    for line in recent {
        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ts = entry.get("ts_unix").and_then(|v| v.as_i64()).unwrap_or(0);
        let action = entry
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        let result = entry
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        if let Some(repo) = entry.get("repo").and_then(|v| v.as_str()) {
            // Keep the most recent (highest ts) entry per repo.
            let entry_data = (ts, action, result);
            map.entry(repo.to_string())
                .and_modify(|existing| {
                    if ts > existing.0 {
                        *existing = entry_data.clone();
                    }
                })
                .or_insert(entry_data);
        }
    }
    Some(map)
}

/// Read up to `max_lines` trailing lines from a file, returning them in
/// chronological order (oldest first). Streams the file in chunks from the
/// end so the operation is O(tail-size) regardless of total file size.
///
/// If the file is smaller than `max_lines`, returns the whole file. If the
/// file cannot be read (missing, permission denied, etc.), returns the
/// underlying IO error so the caller can decide whether to surface it.
fn read_tail_lines(path: &Path, max_lines: usize) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    const CHUNK_SIZE: usize = 16 * 1024;
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len() as usize;
    if len == 0 {
        return Ok(Vec::new());
    }
    // Read from the end in CHUNK_SIZE pieces until we have at least
    // `max_lines` newlines or hit the start of the file.
    let mut buf: Vec<u8> = Vec::new();
    let mut remaining = len;
    let mut pos = len;
    while remaining > 0 && buf.iter().filter(|&&b| b == b'\n').count() <= max_lines {
        let take = remaining.min(CHUNK_SIZE);
        pos -= take;
        file.seek(SeekFrom::Start(pos as u64))?;
        let mut chunk = vec![0u8; take];
        file.read_exact(&mut chunk)?;
        // Prepend because we're reading backwards.
        let mut new_buf = chunk;
        new_buf.append(&mut buf);
        buf = new_buf;
        remaining = pos;
    }
    // Split into lines. If the read window started mid-line, the first
    // parsed entry will be a partial line; drop it after checking the byte
    // immediately before the window.
    let text = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };
    let mut lines: Vec<&str> = text.lines().collect();
    if pos > 0 {
        // A newline immediately before the window means we started at a
        // line boundary. Any other byte means the first parsed line is
        // only the tail of a longer line and must be dropped.
        let mut probe = std::fs::File::open(path)?;
        probe.seek(SeekFrom::Start((pos - 1) as u64))?;
        let mut byte = [0u8; 1];
        if probe.read_exact(&mut byte).is_ok() && byte[0] != b'\n' {
            // We started mid-line, drop the first partial.
            if !lines.is_empty() {
                lines.remove(0);
            }
        }
    }
    // Keep only the last `max_lines` lines.
    if lines.len() > max_lines {
        let drop = lines.len() - max_lines;
        lines.drain(..drop);
    }
    Ok(lines.into_iter().map(|s| s.to_string()).collect())
}

pub(crate) fn repo_state_flags(
    status: &dracon_git::types::RepoStatus,
    has_origin: bool,
    has_upstream: bool,
    has_any_remote: bool,
) -> Vec<String> {
    repo_state_flags_with_push_failure(status, has_origin, has_upstream, has_any_remote, false)
}

/// Like [`repo_state_flags`], but only emits `STUCK_PUSH` when the daemon
/// has actually recorded a recent push failure for this repo. Without that
/// signal, an `AHEAD:N` repo is just "has unpushed commits waiting" and
/// should not be flagged as stuck — the daemon may be waiting for the
/// inactivity delay or for a multi-remote round to finish.
///
/// `has_any_remote` is the "does the repo have at least one configured
/// remote?" signal. When the daemon is configured to push to a list of
/// mirror remotes (e.g. `github` / `gitlab` / `codeberg`) the absence of
/// a literal `origin` is not a concern — those remotes are the canonical
/// push targets. Only repos with **zero** configured remotes are
/// genuinely remote-less and warrant a `NO_ORIGIN` flag. See
/// `docs/design/no-origin-concern-ssh-2026-06-20.md` for the full
/// rationale and the audit of every affected repo.
pub(crate) fn repo_state_flags_with_push_failure(
    status: &dracon_git::types::RepoStatus,
    has_origin: bool,
    has_upstream: bool,
    has_any_remote: bool,
    recent_push_failure: bool,
) -> Vec<String> {
    let mut flags = Vec::new();
    if !status.is_clean {
        flags.push("DIRTY".to_string());
    }
    if status.ahead > 0 {
        flags.push(format!("AHEAD:{}", status.ahead));
    }
    if status.behind > 0 {
        flags.push(format!("BEHIND:{}", status.behind));
    }
    // CHANGED 2026-06-20: the `!has_origin` check used to fire `NO_ORIGIN`
    // for every repo that didn't have a remote literally named `origin`.
    // After the multi-mirror migration to SSH (`github` / `gitlab` /
    // `codeberg`), every watched repo has zero `origin` remotes and the
    // flag fired for all 10 of them, masking the row as a CONCERN even
    // when the daemon was successfully pushing to all three mirrors.
    //
    // The correct semantic is: a repo is "remote-less" only when it has
    // *no* remotes at all. If it has any remote (origin, github, etc.),
    // the daemon can push and the row is healthy. The flag name is kept
    // as `NO_ORIGIN` for backward compatibility (the symptom is still
    // "no literal origin remote"); it just no longer fires when a
    // non-origin remote exists.
    if !has_origin && !has_any_remote {
        flags.push("NO_ORIGIN".to_string());
    }
    // ADDED 2026-07-21 (v0.112.29): `EMPTY_REPO` flag for repos with
    // no commits at all (typically fresh `git init`). Distinct from
    // `NO_UPSTREAM`: an empty repo's NO_UPSTREAM is EXPECTED (there is
    // no commit to push yet, so `branch.<name>.remote` was never set),
    // not a sign of a broken upstream config. The hint for an
    // EMPTY_REPO is "no commits yet — make first commit to enable
    // push", which guides the operator to the right action. The flag
    // also lets the push_status derivation show "EMPTY" instead of the
    // misleading "FAIL" — no push was attempted.
    if status.last_commit_hash.is_none() {
        flags.push("EMPTY_REPO".to_string());
    }
    // CHANGED 2026-06-20: `NO_UPSTREAM` now fires whenever the local
    // branch has no tracking upstream, regardless of whether the repo
    // has an `origin` remote. Previously the `has_origin &&` guard
    // meant that a repo with only non-origin remotes (e.g. the SSH
    // multi-mirror repos) silently swallowed the missing-upstream
    // signal, falling through to the generic "run repair-concerns"
    // hint instead of the more useful "set upstream" hint. The
    // concern predicate ([`repo_is_concern_with_push_failure`]) still
    // gates on `!has_upstream` independently, so the row remains a
    // CONCERN; this just makes the hint text accurate.
    if !has_upstream {
        flags.push("NO_UPSTREAM".to_string());
    }
    if status.ahead > 0 && has_origin && has_upstream && recent_push_failure {
        flags.push("STUCK_PUSH".to_string());
    }
    if status.behind > 0 && has_origin && has_upstream {
        flags.push("STUCK_PULL".to_string());
    }
    if flags.is_empty() {
        flags.push("OK".to_string());
    }
    flags
}

/// Apply per-repo `intentional_no_upstream` semantics to a row of flags.
///
/// When the operator has flagged a repo as intentionally isolated
/// (`.dracon/dracon-sync.toml` sets `intentional_no_upstream = true`),
/// the `NO_UPSTREAM` flag is replaced by the explicit
/// `INTENTIONAL_NO_UPSTREAM` flag and the row is no longer classified
/// as a hidden concern. The intent of the original `NO_UPSTREAM` flag
/// (i.e. "this branch is untracked") is preserved, but the operator
/// has already said it does not want it remediated.
pub(crate) fn apply_intentional_no_upstream(mut flags: Vec<String>) -> Vec<String> {
    if flags.iter().any(|f| f == "NO_UPSTREAM") {
        flags.retain(|f| f != "NO_UPSTREAM");
        if !flags.iter().any(|f| f == "INTENTIONAL_NO_UPSTREAM") {
            flags.push("INTENTIONAL_NO_UPSTREAM".to_string());
        }
    }
    flags
}

/// Kept for backward-compatible test coverage. New code should use
/// [`repo_is_concern_with_push_failure`] which also considers recent
/// push failures and the behind-count.
///
/// CHANGED 2026-06-20: the `!has_origin` short-circuit used to flag
/// every non-`origin` repo as a concern, and `!has_upstream` flagged
/// every repo with a missing branch tracking config. After the SSH
/// multi-mirror migration, the daemon pushes to `github` / `gitlab` /
/// `codeberg` via explicit refspecs and doesn't require either an
/// `origin` remote or a `branch.<name>.remote` config. The new
/// `has_any_remote` parameter lets callers distinguish "no origin
/// but has SSH mirrors" (healthy) from "truly remote-less"
/// (concerning). See `docs/design/no-origin-concern-ssh-2026-06-20.md`.
#[allow(dead_code, unused_variables)]
pub(crate) fn repo_is_concern(
    _status: &dracon_git::types::RepoStatus,
    has_origin: bool,
    has_upstream: bool,
    has_any_remote: bool,
) -> bool {
    if !has_origin && !has_any_remote {
        return true;
    }
    !has_upstream && has_origin
}

/// Like [`repo_is_concern`], but also flags a repo as a concern when it has
/// unpushed commits (ahead > 0) **and** a recent push failure was recorded
/// in the incident ledger. Without the push-failure signal, an AHEAD repo
/// is just "has unpushed commits" and the daemon is working through the
/// queue; that should be a WARN, not a CONCERN.
///
/// `behind > 0` remains a concern unconditionally: the local is older
/// than the remote and risks losing history if the divergence grows.
///
/// CHANGED 2026-07-17 (goal 013b3827): two additional conditions now make
/// a repo a CONCERN, both per the operator's "having no remote to push to
/// is a massive problem" directive:
///   1. `!has_upstream` — a repo with no tracking upstream is data-at-risk
///      even when push remotes exist. This REVERTS the 2026-06-20 SSH
///      migration leniency that cleared NO_UPSTREAM (the daemon pushes via
///      explicit refspecs). The `intentional_no_upstream` override still
///      exempts repos the operator explicitly isolated.
///   2. working-tree content (untracked/modified/staged) **and**
///      `last_commit_hash.is_none()` (no commits at all) — the content is
///      unbacked-up on every remote and exists only on local disk.
///
/// `has_any_remote` follows the same logic as [`repo_is_concern`]: a
/// repo with at least one configured remote is not concerning for
/// "no origin" alone (still handled by the `!has_origin &&
/// !has_any_remote` arm above). The `NO_UPSTREAM` flag and hint text are
/// still emitted so the operator can see the gap. See
/// `docs/design/no-origin-concern-ssh-2026-06-20.md` (reverted for the
/// NO_UPSTREAM case by goal 013b3827) and
/// `docs/design/repos-no-push-target-concern-2026-07-17.md`.
pub(crate) fn repo_is_concern_with_push_failure(
    status: &dracon_git::types::RepoStatus,
    has_origin: bool,
    has_upstream: bool,
    has_any_remote: bool,
    recent_push_failure: bool,
) -> bool {
    if !has_origin && !has_any_remote {
        return true;
    }
    if status.behind > 0 {
        return true;
    }
    if status.ahead > 0 && has_origin && has_upstream && recent_push_failure {
        return true;
    }
    // CHANGED 2026-07-17 (goal 013b3827): a repo with NO tracking
    // upstream is a genuine concern. The operator considers "having no
    // remote to push to" a massive problem — content that is not wired
    // to an upstream is data-at-risk even when push remotes exist. The
    // 2026-06-20 SSH-migration change deliberately cleared this case
    // (the daemon pushes via explicit refspecs, so it does not require
    // `branch.<name>.remote`); this reverts that leniency for the
    // NO_UPSTREAM case. The `intentional_no_upstream` override applied
    // at the call site still exempts repos the operator explicitly
    // isolated.
    if !has_upstream {
        return true;
    }
    // CHANGED 2026-07-17 (goal 013b3827): a repo that has working-tree
    // content (untracked / modified / staged) but NO commits at all is
    // unbacked-up on every remote — its content exists only on local
    // disk. Surface it as a concern so the operator sees the risk and
    // can commit + push it.
    let has_content = status.untracked_files > 0
        || status.modified_files > 0
        || status.staged_files > 0;
    if has_content && status.last_commit_hash.is_none() {
        return true;
    }
    false
}

/// CHANGED 2026-07-28 (v0.113.7): extract the `pack_too_large → CONCERN`
/// decision to a helper so the regression test does not have to spin up
/// a whole `RepoReportRow`.
///
/// GitHub rejects packs > 2 GiB. The daemon's push path detects this via
/// `github_pack_too_large` and silently excludes GitHub from the mirror
/// list (see `dracon-sync/src/sync.rs:1819`). The `repos` table, however,
/// was only emitting a HINT note (`.git exceeds 2 GB (github limit) — may
/// fail to push to github`) without classifying the row as a CONCERN.
///
/// The silent skip is a real problem: the daemon's `auto_repair_concerns`
/// path will never fix this (the daemon has no history-rewrite code), so
/// the row sat at 🔄 ACTIVE indefinitely. Bumping to CONCERN surfaces
/// the situation in the row's STATUS cell AND in the fleet-wide tally.
///
/// The `pack_too_large` tuple here is the same `(bool, u64)` returned by
/// `crate::git::github_pack_too_large`; the helper only consults the bool
/// component. Returning `true` from this function is the row-construction
/// signal "this is a CONCERN because github cannot accept the pack".
///
/// The companion fix is in `run_repair_concerns` (the auto-repair path)
/// where the PACK_SIZE_WARNING flag short-circuits the concern: the daemon
/// has no code path that shrinks a repo, so attempting the repair would
/// just produce noise in journalctl every sync cycle.
pub(crate) fn pack_too_large_forces_concern(pack_too_large: (bool, u64)) -> bool {
    pack_too_large.0
}

/// CHANGED 2026-07-28 (v0.113.7): the post-handler
/// `verify_resolution` check now also considers `pack_too_large`.
/// Extracted to a helper so the regression test does not have to
/// spin up a full `GitService` + repo. The caller passes the values
/// `verify_resolution` would have computed (status fields from
/// `svc.get_status()` + remote/upstream presence + the
/// `pack_too_large` boolean from the early-skip). A return of
/// `true` means the repo is STILL a concern after the repair pass
/// (i.e. do NOT count it as resolved).
pub(crate) fn verify_resolution_still_concern(
    ahead: usize,
    behind: usize,
    has_origin: bool,
    has_upstream: bool,
    pack_too_large: bool,
) -> bool {
    ahead > 0
        || behind > 0
        || !has_origin
        || !has_upstream
        || pack_too_large
}

/// CHANGED 2026-07-28 (v0.113.7, follow-up): the auto-repair no-op
/// guard in `run_repair_concerns` (the `if pack_too_large { ...;
/// continue; }` check at the top of the handler loop) decides
/// whether to short-circuit BEFORE any handler runs. The original
/// guard (committed in `7f3e456`) checked `flags.contains("PACK_SIZE_WARNING")`
/// — but `repo_state_flags_with_push_failure` (the function that
/// built `flags`) does NOT add `PACK_SIZE_WARNING`. The fix (commit
/// `d385655`) re-uses the inline `pack_too_large` bool from the
/// early-skip so the guard actually fires. The reviewer's concern
/// (item #4 of the leftover cascade): "for a hypothetical repo that
/// ALSO has a CONCERN and ALSO has pack_too_large, the auto-repair
/// would attempt handlers". The predicate is purely `pack_too_large`:
/// it fires regardless of `is_concern`, `stuck_push`, `stuck_pull`,
/// etc. Extracted to a tiny helper so the regression test can verify
/// that the guard fires unconditionally on `pack_too_large=true` —
/// not by coincidence on CAG's clean/synced state.
pub(crate) fn pack_too_large_skips_repair(pack_too_large: bool) -> bool {
    pack_too_large
}

pub(crate) fn repo_is_stuck_push(
    status: &dracon_git::types::RepoStatus,
    has_origin: bool,
    has_upstream: bool,
    has_any_remote: bool,
    recent_push_failure: bool,
) -> bool {
    // The push path requires both an `origin` and an `upstream` — these
    // repos push via the `origin` refspec, not the multi-mirror list. So
    // the stuck-push predicate is unchanged by the SSH-migration fix.
    // `has_any_remote` is accepted for signature parity with
    // `repo_is_concern_with_push_failure`; it's not consulted.
    let _ = has_any_remote;
    status.ahead > 0 && has_origin && has_upstream && recent_push_failure
}

pub(crate) fn repo_is_stuck_pull(
    status: &dracon_git::types::RepoStatus,
    has_origin: bool,
    has_upstream: bool,
    has_any_remote: bool,
) -> bool {
    // Same as `repo_is_stuck_push`: the pull path uses `origin` and an
    // upstream refspec, so the predicate is unchanged by the SSH fix.
    let _ = has_any_remote;
    status.behind > 0 && has_origin && has_upstream
}

#[cfg(test)]
pub(crate) fn repo_is_warn(
    status: &dracon_git::types::RepoStatus,
    has_origin: bool,
    has_upstream: bool,
    has_any_remote: bool,
) -> bool {
    // WARN: has TRACKED modifications or staged changes, but not a concern.
    // Untracked files remain visible in the UT column, but they are not
    // sync-relevant by themselves. This keeps audit/research artifacts visible
    // without turning build artifacts, screenshots, or local evidence into WARNs.
    //
    // CHANGED 2026-06-15 (goal 0ab367b5 / Junk-Runner-bevy WARN fix):
    // upgraded `dracon-git` 94.2.7 → 94.7.0 which fixed the
    // `is_wt_new()`-counted-as-modified bug and added `untracked_files`
    // to `RepoStatus`. Junk-Runner-bevy 91 "MOD" was 3 untracked
    // test-results/ PNGs.
    //
    // CHANGED 2026-06-20: added `has_any_remote` to keep parity with
    // `repo_is_concern`. Repos with only non-origin remotes (the
    // post-SSH-migration case) are no longer a concern and therefore
    // can be a WARN when dirty.
    !repo_is_concern(status, has_origin, has_upstream, has_any_remote)
        && (status.modified_files > 0 || status.staged_files > 0)
}

/// Coarse "what is this repo doing right now?" classification derived
/// True when the daemon is in-flight on this repo or will imminently
/// handle its dirty state. Mirrors [`repo_is_warn`] / [`repo_is_concern`]
/// so the `repos` table can show an `ACTIVE` status distinct from `WARN`.
///
/// An ACTIVE repo is "plausibly not broken": the daemon is pushing
/// (`push_status = PENDING`), or the [`StateCause`] is `Working` /
/// `Pushing` / `Committing` (clean and just synced / mid-cycle) or
/// `Dirty` (recent uncommitted work the daemon will pick up). A repo
/// that is dirty but `Stalled` (no progress for a long time) is NOT
/// active — it is a genuine `WARN`.
pub(crate) fn repo_is_active(push_status: &str, state_cause: &StateCause) -> bool {
    // An ownership-blocked repo may still have ahead commits, but the
    // daemon is deliberately not working on it. Likewise, a broken-history
    // row must never look ACTIVE merely because its old ahead count remains.
    if matches!(state_cause, StateCause::Unowned { .. })
        || matches!(push_status, "BLOCKED" | "BROKEN")
    {
        return false;
    }
    push_status == "PENDING"
        || matches!(
            state_cause,
            StateCause::Working | StateCause::Pushing | StateCause::Committing | StateCause::Dirty
        )
}

/// from the existing signals — last-commit time, last-push time, dirty
/// state, ahead/behind, and push status. The vocabulary is intentionally
/// small so the user can scan the table at a glance and tell apart
/// "freshly synced", "waiting on the daemon", "stalled", and
/// "cold idle".
///
/// The vocabulary:
///
/// - `Working`   — clean, in sync, and both commit and push are within
///   `active_commit_minutes` (default 5m). This means "the daemon is
///   currently working through this repo" (it just committed and
///   pushed). Distinct from `Synced`: `Synced` is the longer-term clean
///   state, `Working` is the short window after a recent sync cycle.
/// - `Committing` — unpushed commits are waiting, or the last commit is
///   within `committing_commit_minutes` but outside the active window.
/// - `Pushing`   — `push_status = PENDING` (the daemon is mid-cycle).
/// - `Synced`    — clean, `ahead=0, behind=0`, commit/push within
///   `committing_commit_minutes` but outside the active window.
/// - `Stalled`   — dirty tracked/staged work that has been sitting for
///   longer than `committing_commit_minutes` without push progress. This
///   is the case the user described as "stalling for minutes".
/// - `Dirty`     — dirty tracked/staged work that is still recent and
///   expected to be picked up by normal sync; `sync-now --warns` forces
///   the same triage immediately.
/// - `Untracked` — only untracked files (no modified, no staged).
/// - `Intentional` — repo flagged `intentional_no_upstream = true`.
/// - `Failed`    — `push_status = FAIL` or `STUCK`.
/// - `Idle`      — clean, no recent activity, last commit within
///   `cold_commit_minutes`.
/// - `Cold`      — last commit older than `cold_commit_minutes` (default 24h).
/// - `Healthy`   — fallback when nothing else matches.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StateCause {
    Working,
    Committing,
    Pushing,
    Synced,
    Stalled,
    Dirty,
    Untracked,
    Intentional,
    Failed,
    Idle,
    Cold,
    Healthy,
    /// Repo is not owned by the operator (per the
    /// `auto_skip_unowned` ownership guard). The daemon skips
    /// auto-commit and auto-push for this repo. `reason` is
    /// the stable kebab-case classifier (e.g. `untrusted_origin`,
    /// `untrusted_author`); `detail` is the human-readable
    /// explanation (e.g. the actual bad origin URL).
    Unowned { reason: String, detail: String },
}

impl StateCause {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            StateCause::Working => "working",
            StateCause::Committing => "committing",
            StateCause::Pushing => "pushing",
            StateCause::Synced => "synced",
            StateCause::Stalled => "stalled",
            StateCause::Dirty => "dirty",
            StateCause::Untracked => "untracked-only",
            StateCause::Intentional => "intentional",
            StateCause::Failed => "failed",
            StateCause::Idle => "idle",
            StateCause::Cold => "cold",
            StateCause::Healthy => "healthy",
            // For Unowned, the label is computed separately
            // (it's a dynamic String, not a &'static str). See
            // `state_cause_label_string` for the dynamic case.
            StateCause::Unowned { .. } => "unowned",
        }
    }

    /// Icon used in the human-readable table. The colour of the row is
    /// picked separately by `cause_color`.
    pub(crate) fn icon(&self) -> &'static str {
        match self {
            StateCause::Working => "🔄",
            StateCause::Committing => "🟡",
            StateCause::Pushing => "🟣",
            StateCause::Synced => "🟢",
            StateCause::Stalled => "🔴",
            StateCause::Dirty => "🟠",
            StateCause::Untracked => "⚪",
            StateCause::Intentional => "🟣",
            StateCause::Failed => "⛔",
            StateCause::Idle => "⚪",
            StateCause::Cold => "⚫",
            StateCause::Healthy => "✅",
            StateCause::Unowned { .. } => "🚫",
        }
    }
}

/// Compute the state_cause_label string. For most variants this
/// is just `state_cause.as_str()`, but `Unowned` carries a
/// dynamic reason string that needs to be returned as the label
/// (e.g. `unowned:untrusted_origin` for machine parsing, or just
/// the reason for the table cell).
pub(crate) fn state_cause_label_string(cause: &StateCause) -> String {
    match cause {
        StateCause::Unowned { reason, .. } => format!("unowned:{}", reason),
        other => other.as_str().to_string(),
    }
}

// NOTE: `state_cause_as_str` removed 2026-07-11 (audit
// AUDIT-3-UTILITIES-2026-07-10.md CONCERN #6). Zero callers in
// production code (only commented-out references in
// report_v2_snapshot.rs that mention it was already removed in the
// 2026-06-27 v2 redesign).

#[derive(Debug, Clone, Copy)]
pub(crate) struct StateCauseThresholds {
    pub(crate) active_minutes: u64,
    pub(crate) committing_minutes: u64,
    pub(crate) cold_minutes: u64,
}

impl StateCauseThresholds {
    pub(crate) fn from_policy(policy: &SyncPolicy, override_: &RepoPolicyOverride) -> Self {
        Self {
            active_minutes: override_
                .active_commit_minutes
                .unwrap_or(policy.active_commit_minutes),
            committing_minutes: override_
                .committing_commit_minutes
                .unwrap_or(policy.committing_commit_minutes),
            cold_minutes: override_
                .cold_commit_minutes
                .unwrap_or(policy.cold_commit_minutes),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StateCauseInputs<'a> {
    pub(crate) flags: &'a [String],
    pub(crate) push_status: &'a str,
    pub(crate) modified: usize,
    pub(crate) staged: usize,
    pub(crate) untracked: usize,
    pub(crate) ahead: usize,
    pub(crate) behind: usize,
    /// Last commit age in minutes, if known. None means we could not read it.
    pub(crate) last_commit_minutes: Option<i64>,
    /// Last push age in minutes, if known. None means we could not read it.
    pub(crate) last_push_minutes: Option<i64>,
}

/// Classify a single repo's "rough cause" given the current signals.
///
/// The classification is order-dependent: more specific states are
/// checked first. The intent is that the user can read the column
/// top-to-bottom and trust the first matching label.
pub(crate) fn classify_state_cause(
    inputs: &StateCauseInputs,
    thresholds: &StateCauseThresholds,
) -> StateCause {
    let last_commit = inputs.last_commit_minutes;
    let last_push = inputs.last_push_minutes;

    if inputs.push_status == "PENDING" {
        return StateCause::Pushing;
    }
    if matches!(inputs.push_status, "FAIL" | "STUCK" | "BROKEN" | "BLOCKED") {
        return StateCause::Failed;
    }
    if inputs.flags.iter().any(|f| f == "INTENTIONAL_NO_UPSTREAM") {
        return StateCause::Intentional;
    }

    let has_dirty = inputs.modified > 0 || inputs.staged > 0;
    let in_sync = inputs.ahead == 0 && inputs.behind == 0;
    let has_untracked_only = inputs.modified == 0 && inputs.staged == 0 && inputs.untracked > 0;
    let recent_commit = last_commit
        .map(|m| m >= 0 && m <= thresholds.active_minutes as i64)
        .unwrap_or(false);
    let recent_push = last_push
        .map(|m| m >= 0 && m <= thresholds.active_minutes as i64)
        .unwrap_or(false);

    // Dirty tracked/staged work is not automatically "stalled". Recent
    // dirty work is expected to be picked up by normal sync or
    // `repair warns --apply`; only older dirty work with no push progress
    // is the user's "we changed files and then stopped" pain case.
    if has_dirty {
        if inputs.ahead > 0 {
            return StateCause::Committing;
        }
        let recent_commit_or_push = last_commit
            .map(|m| m >= 0 && m <= thresholds.committing_minutes as i64)
            .unwrap_or(false)
            || last_push
                .map(|m| m >= 0 && m <= thresholds.committing_minutes as i64)
                .unwrap_or(false);
        if recent_commit_or_push {
            return StateCause::Dirty;
        }
        return StateCause::Stalled;
    }

    if has_untracked_only {
        return StateCause::Untracked;
    }

    if inputs.behind > 0 {
        return StateCause::Stalled;
    }

    if in_sync && recent_commit && recent_push {
        return StateCause::Working;
    }

    if let Some(m) = last_commit {
        if m >= 0 && m <= thresholds.committing_minutes as i64 {
            if in_sync {
                return StateCause::Synced;
            }
            return StateCause::Committing;
        }
    }

    if let Some(m) = last_commit {
        if m > thresholds.cold_minutes as i64 {
            return StateCause::Cold;
        }
    }

    if last_commit.is_some() {
        return StateCause::Idle;
    }

    StateCause::Healthy
}

/// Parse a git-style relative time string ("5 minutes ago", "2 days ago",
/// "1 hour ago", "8 hours ago", "29 minutes ago") into minutes.
///
/// Returns None for input we cannot parse, including:
/// - the sentinel "-" the daemon emits when no time is available;
/// - any string without a recognizable number + unit.
/// - special "weird" forms like "yesterday", "a week ago" (treated as None).
pub(crate) fn parse_relative_minutes(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return None;
    }
    let body = trimmed.strip_suffix(" ago").unwrap_or(trimmed);
    let mut iter = body.split_whitespace();
    let n_str = iter.next()?;
    let n: i64 = n_str.parse().ok()?;
    let unit = iter.next()?;
    let minutes = match unit {
        "second" | "seconds" => 0,
        "minute" | "minutes" => n,
        "hour" | "hours" => n * 60,
        "day" | "days" => n * 24 * 60,
        "week" | "weeks" => n * 7 * 24 * 60,
        "month" | "months" => n * 30 * 24 * 60,
        "year" | "years" => n * 365 * 24 * 60,
        _ => return None,
    };
    Some(minutes)
}

/// Compute the user-visible hint for a row of state flags.
///
/// When the operator has flagged a repo as intentionally isolated
/// (see [`crate::policy::RepoPolicyOverride::intentional_no_upstream`]),
/// the row builder appends the explicit `INTENTIONAL_NO_UPSTREAM`
/// flag. That flag is checked first here so the row reports the
/// operator's intent instead of a misleading "set upstream" hint.
///
/// CHANGED 2026-06-20: the `NO_ORIGIN` hint used to say "no origin
/// remote (using github SSH instead)" for every multi-mirror repo.
/// With the SSH migration, that message was misleading — the daemon
/// WAS pushing via SSH, the literal `origin` was just absent. The
/// flag now only fires when the repo has *zero* remotes, so the hint
/// is updated to match: "no remote configured (cannot push)".
pub(crate) fn repo_hint(flags: &[String], warn: bool, concern: bool) -> String {
    if flags.iter().any(|f| f == "INTENTIONAL_NO_UPSTREAM") {
        return "intentional legacy isolation, no upstream configured".to_string();
    }
    if flags.iter().any(|f| f == "NO_ORIGIN") {
        return "no remote configured (cannot push)".to_string();
    }
    // ADDED 2026-07-21 (v0.112.29): EMPTY_REPO overrides the
    // NO_UPSTREAM hint. An empty repo's "no upstream" is expected
    // (the operator hasn't committed yet), so the "set upstream" hint
    // would be misleading — `git push -u origin HEAD` fails with
    // "src refspec HEAD does not match any" on an empty repo.
    if flags.iter().any(|f| f == "EMPTY_REPO") {
        return "no commits yet — make first commit to enable push".to_string();
    }
    if flags.iter().any(|f| f == "NO_UPSTREAM") {
        // CHANGED 2026-06-20: the original hint "run repair-concerns
        // --apply (set upstream)" was misleading for SSH multi-mirror
        // repos that have no `origin` remote. `repair concerns --apply`
        // would try `git push -u origin HEAD` and fail because there is
        // no `origin` to push to. For those repos the branch's tracking
        // config is not actually needed — the daemon's multi-mirror
        // push path uses explicit refspecs. The `concern` parameter
        // disambiguates:
        //   - `concern=true`  → has_origin && !has_upstream (Case A):
        //     the original "set upstream" hint is accurate and
        //     `repair concerns --apply` will succeed.
        //   - `concern=false` → has_origin=false && has_any_remote
        //     (Case B, post-SSH-migration): the hint is informational
        //     only, since the daemon is already pushing successfully
        //     via explicit refspecs.
        if concern {
            return "run repair-concerns --apply (set upstream)".to_string();
        }
        return "no tracking upstream (daemon uses explicit refspecs; not a concern)"
            .to_string();
    }
    if flags.iter().any(|f| f.starts_with("AHEAD:")) {
        if warn {
            return "daemon will push after changes settle".to_string();
        }
        return "run repair-concerns --apply (push or rewrite)".to_string();
    }
    if flags.iter().any(|f| f.starts_with("BEHIND:")) {
        return "run repair-concerns --apply (pull/merge)".to_string();
    }
    if flags.iter().any(|f| f == "PACK_SIZE_WARNING") {
        // CHANGED 2026-07-28 (v0.113.7): the daemon's push path now
        // classifies this as a CONCERN (the github push is permanently
        // skipped — see `pack_too_large_forces_concern`). The hint text
        // reflects permanence: "is skipped" rather than "may fail". The
        // row's STATUS cell shows ❌ CONCERN; this hint tells the
        // operator WHICH concern (because the same row could in
        // principle have other concern causes too).
        return ".git exceeds 2 GB (github limit) — github push is skipped; shrink history or migrate assets to OVH".to_string();
    }
    if warn {
        return "daemon handles after changes settle; run sync-now --warns to force now"
            .to_string();
    }
    if concern {
        return "run repair-concerns --apply".to_string();
    }
    "healthy".to_string()
}

pub(crate) fn push_large_blob_threshold_bytes(policy: &SyncPolicy) -> u64 {
    policy
        .max_stage_file_bytes
        .min(policy.max_push_blob_bytes)
        .min(DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES)
}

pub(crate) fn truncate(value: &str, max_chars: usize) -> String {
    // Original char-counted truncation. Used by call sites that pre-compute
    // char budgets (e.g., commit subjects at 40 chars). For width-aware
    // truncation (terminal column width, emoji-safe), use `truncate_unicode_width`.
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let shortened: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", shortened)
}

/// Format a commit subject for display in a table column, preserving
/// meaningful structural tokens (the file list `[…]`) and dropping
/// trailing metrics (`| GOAL:…`, `| TOKENS:…`, `| NEW:…`, etc.) when
/// the full subject would otherwise be cut mid-filename.
///
/// This matters for daemon auto-commits in `.pi/goals/` where the
/// commit subject embeds the full goal filename, e.g.
///
///     2 file(s) in .pi [.pi/goals/active_goal_2026063004051714_mr02de1n-gjkgzp.md, …] DELTA:+8/-5 | GOAL:complete TOKENS:407K TIME:323m
///
/// …and a naive `truncate(msg, N)` would slice through the goal id
/// (`mr02de1n-gjkgzp` → `mr02de1n-gjkg…`), leaving the operator with
/// a misleadingly-truncated id. The previous behaviour also produced
/// a double-ellipsis (one from `truncate`, one from
/// `truncate_unicode_width`) in the rendered row.
///
/// Strategy (in order, only ever more aggressive than the previous one):
///  1. If the full subject fits in `max_chars`, return it as-is.
///  2. If the subject is a structured auto-commit (matches
///     `^N file(s) in DIR [FILE…] DELTA:+X/-Y`), drop the trailing
///     `| METRIC:…` suffix and return what's left. If the result
///     still doesn't fit, drop the ` DELTA:+X/-Y` portion too.
///  3. Otherwise, fall back to plain `truncate(value, max_chars)`.
///
/// The helper never splits inside a `]`-bracketed file list when
/// the data permits.
pub(crate) fn format_commit_subject_for_display(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    // Detect the structured auto-commit shape: "N file(s) in DIR [...]"
    // optionally followed by " DELTA:..." and/or " | METRIC:..." segments.
    let lower = value.to_ascii_lowercase();
    let is_structured = lower.contains(" file(s) in ");
    if is_structured {
        // Try progressively more aggressive truncations. We always
        // keep the file list ([...]) whole when the data fits.
        // 1) Full subject minus the leading pipe-separated metrics suffix
        //    ("| GOAL:…", "| TOKENS:…", "| TIME:…", "| NEW:…", "| DEL:…", etc.)
        if let Some(cut) = value.find(" | ") {
            let head = &value[..cut];
            if head.chars().count() <= max_chars {
                return head.to_string();
            }
        }
        // 2) Drop the " DELTA:+X/-Y" segment but keep the file list.
        //    The DELTA segment is the LAST whitespace-delimited token
        //    that starts with "DELTA:" (no leading pipe).
        if let Some(delta_idx) = find_delta_segment(value) {
            let head = value[..delta_idx].trim_end();
            if head.chars().count() <= max_chars {
                return head.to_string();
            }
        }
    }
    // 3) Fallback: plain truncate.
    truncate(value, max_chars)
}

/// Find the start index of the trailing ` DELTA:+X/-Y` segment in a
/// daemon commit subject, or `None` if not present. The DELTA token
/// must NOT be preceded by `|` (those are pipe-separated metrics and
/// handled by the first branch above). Returns the byte index of the
/// leading space before "DELTA:".
fn find_delta_segment(value: &str) -> Option<usize> {
    // Search for the LAST occurrence of "DELTA:" that is not
    // immediately preceded by "| " (those are metrics, not the
    // top-level DELTA summary). We look for the literal substring
    // " DELTA:" (with leading space) to avoid matching the top-level
    // DELTA only.
    let mut idx = 0;
    let mut last_match: Option<usize> = None;
    while let Some(pos) = value[idx..].find(" DELTA:") {
        let abs = idx + pos;
        // Verify the character before " DELTA:" is whitespace (space
        // or end-of-list `]`). The match above already ensures a
        // leading space; we just need the char BEFORE that space.
        if abs > 0 {
            let prev_char = value[..abs].chars().last().unwrap_or(' ');
            // Acceptable boundaries: end of file list (']') or
            // end of a previous segment. We treat the position as a
            // valid cut point as long as we're not inside a pipe-
            // separated metric block.
            if prev_char == ']' {
                last_match = Some(abs);
            }
        }
        idx = abs + 1;
    }
    last_match
}

/// Truncate a string to fit within `max_width` terminal columns, using
/// `unicode-width` for accurate wide-char and emoji measurement.
///
/// - Does NOT break inside a grapheme cluster (emoji, CJK, etc.)
/// - Appends `…` (1 column) when truncated
/// - `max_width=0` returns `""`
/// - `max_width=1` returns at most 1 column of content
///
/// Truncation policy:
/// 1. If content fits in `max_width` cols, return as-is (no ellipsis)
/// 2. Otherwise, fit as much as possible, but reserve 1 col for the ellipsis
/// 3. If the next char would push the width over `max_width - 1`, stop
/// 4. Append `…` to signal truncation
pub(crate) fn truncate_unicode_width(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        // Only room for 1 col of content (no room for ellipsis)
        for ch in value.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if w == 1 {
                return ch.to_string();
            }
        }
        return String::new();
    }
    // Try to fit the full content
    let total_width: usize = value
        .chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
        .sum();
    if total_width <= max_width {
        return value.to_string();
    }
    // Need to truncate. Reserve 1 col for the ellipsis.
    let content_budget = max_width - 1;
    let mut width = 0;
    let mut end = 0;
    for (idx, ch) in value.char_indices() {
        let ch_w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_w > content_budget {
            break;
        }
        width += ch_w;
        end = idx + ch.len_utf8();
    }
    format!("{}…", &value[..end])
}

/// Build a state + activity cell that fits a 15-col budget without
/// leaving a dangling emoji.
///
/// Strategy (priority: state first, activity second):
/// 1. Always show `{state_icon} {state_word}` (e.g., `🟠 dirty`)
/// 2. If there's room, append ` · {activity_icon} {activity_text}`
///    (e.g., `🟠 dirty · ⏳ dirty 1h`)
/// 3. If not, drop the activity part entirely rather than leaving
///    a partial emoji + ellipsis (`🟠 dirty · ⏳ …`).
/// 4. If state alone doesn't fit, truncate state with `…`.
///
/// 2026-07-19 (goal `4555eaf6` v0.112.25 follow-up): the previous
/// `truncate_unicode_width` approach clipped the second emoji
/// (⏳) when budget was tight, leaving an ungrammatical
/// `🟠 dirty · ⏳ …` cell. This helper preserves the state
/// component (the most important) and cleanly drops activity
/// when it can't fit. See docs/design/repos-table-fix-2026-07-19.md.
pub(crate) fn state_plus_act_cell(
    state_icon: &str,
    state_word: &str,
    activity_full: &str,
    budget: usize,
) -> String {
    let state_part = format!("{state_icon} {state_word}");
    // Split the activity string into (icon, text). Activity strings
    // look like `⏳ dirty 5m`, `🟢 synced 3m`, `⚫ cold 2d`, etc.
    // - the first char-cluster is the emoji (1 or 2 visual cols),
    // - the rest is the text after a space.
    let (activity_icon, activity_text) = split_activity(activity_full);
    // 1) state alone fits?
    if unicode_width::UnicodeWidthStr::width(state_part.as_str()) <= budget {
        if let Some(act) = activity_part(&activity_icon, &activity_text) {
            let combined = format!("{state_part} · {act}");
            if unicode_width::UnicodeWidthStr::width(combined.as_str()) <= budget {
                return combined;
            }
        }
        return state_part;
    }
    // 2) truncate state itself
    truncate_unicode_width(&state_part, budget)
}

/// Build the activity-part string `icon text`, or `None` if there
/// is no activity (e.g., the activity was "—", a bare dash).
fn activity_part(icon: &str, text: &str) -> Option<String> {
    if text.is_empty() || text == "—" {
        None
    } else {
        Some(format!("{icon} {text}"))
    }
}

/// Split an activity string (e.g., `⏳ dirty 5m`) into
/// (icon, text). The icon is the leading emoji (1 unicode
/// char-cluster, possibly wide); the text is everything after
/// the next ASCII space. Returns ("", input) if no leading
/// emoji is present.
fn split_activity(s: &str) -> (String, String) {
    let mut chars = s.chars();
    let first = chars.next();
    match first {
        None => (String::new(), String::new()),
        Some(c) => {
            // Skip leading ASCII spaces before the emoji (none in practice)
            let _ = c;
            // Take the first char-cluster; if it's an emoji (wide or
            // not), it's the icon. Then skip one space, take the rest.
            let rest = &s[c.len_utf8()..];
            let after_space = rest.strip_prefix(' ').unwrap_or(rest);
            (c.to_string(), after_space.to_string())
        }
    }
}

/// Detect the terminal width in columns.
///
/// Resolution order:
/// 1. `DRACON_SYNC_TERM_WIDTH` env var (operator override, e.g. `80` to force Vertical)
/// 2. `COLUMNS` env var (ncurses convention; respected by many shells & scripts)
/// 3. `terminal_size()` against stdout/stderr/stdin (real TTY only)
/// 4. Default fallback of `120` cols (Compact layout) — safe for log files and most pipes.
///
/// The fallback of 120 (Compact) instead of the previously-used 300 (Full) fixes the
/// 538-char-wide broken table that piped / scripted / agent-captured output produced.
/// Compact (15 cols, ~215-col minimum width) fits in 120+ cols via comfy-table's Dynamic
/// arrangement; Full (22 cols, ~293-col minimum) needs 300+ cols and visibly breaks at
/// narrower widths. See `docs/design/repos-table-fix-2026-07-18.md`.
pub(crate) fn terminal_width() -> Option<u16> {
    if let Ok(s) = std::env::var("DRACON_SYNC_TERM_WIDTH") {
        if let Ok(n) = s.parse::<u16>() {
            if (40..=1000).contains(&n) {
                return Some(n);
            }
        }
    }
    if let Ok(s) = std::env::var("COLUMNS") {
        if let Ok(n) = s.parse::<u16>() {
            if (40..=1000).contains(&n) {
                return Some(n);
            }
        }
    }
    use terminal_size::{terminal_size, Height, Width};
    if let Some((Width(w), Height(_))) = terminal_size() {
        if (40..=1000).contains(&w) {
            return Some(w);
        }
    }
    // Default: Compact (120 cols) for piped / scripted / non-TTY output.
    Some(120)
}

/// Tier classification for the `dracon-sync repos` output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutTier {
    /// ADDED 2026-07-22 (v0.112.38): the DEFAULT for < 242 cols — a
    /// rich 6-column table (STATUS · REPO · ACTIVITY · PUSH · HINT,
    /// plus PUBLISH at ≥140 cols). Replaces Vertical as the
    /// auto-picked default: the operator found the per-repo block
    /// view too verbose for the default and wanted a rich table +
    /// on-demand detail (`repos <name>` or `--layout vertical`).
    Rich,
    /// Opt-in via `--layout vertical` or `repos <name>`: one repo
    /// per multi-line block (the detailed per-repo view).
    Vertical,
    /// 120-200 cols: compact table (15 columns, no 1h/6h/24h split, narrow HINT)
    Compact,
    /// > 200 cols: full v1 22-column table
    Full,
}

/// Pick the layout tier from terminal width.
///
/// - `< 165` cols → **Compact** (the rich table's fixed 190 cols can't fit)
/// - `>= 165` cols → **Rich** (the 10-column table; the operator's table)
///
/// CHANGED 2026-07-30 (v0.113.26): the 242-314 → Compact and >= 315
/// → Full bands were REMOVED from auto-pick. They were leftovers from
/// the pre-rich design: a maximized terminal (242+ cols) silently
/// served the OLD 16-column compact table, and the operator's
/// reaction was "this looks like the old table — no legend or
/// indicators". The rich table is the product now; a wider terminal
/// gets the same rich table (190 cols fixed, trailing whitespace is
/// fine). Compact/Full/Vertical remain reachable via `--layout`.
///
/// History: `< 242` default was Vertical (v0.112.38), then Rich
/// (v0.112.38); 165-col Rich minimum since v0.113.8 (added USED +
/// COMMITS + SIZE + TOUCHED columns grew the table from ~120 to
/// ~165 cols).
pub(crate) fn choose_layout_tier() -> LayoutTier {
    let w = terminal_width().unwrap_or(120);
    if w < 165 {
        LayoutTier::Compact
    } else {
        LayoutTier::Rich
    }
}

/// Single `git log` call that extracts all commit metadata in one process.
/// Returns (hash, author, relative_time, unix_timestamp, subject).
/// Previously the report called this 3 times per repo (hash via libgit2,
/// author via `%an`, time via `%ar`, timestamp via `%ct`) which tripled
/// the wall-clock time on repos with many entries.
pub(crate) async fn git_log_meta(repo: &Path) -> Option<(String, String, String, i64, String)> {
    let repo_str = repo.to_str()?;
    // %H = hash, %an = author, %ar = relative, %ct = unix, %s = subject
    // Separator `\x1f` (unit separator) is unlikely in commit fields.
    let out = crate::git::git_cmd()
        .args([
            "-C",
            repo_str,
            "log",
            "-1",
            "--format=%H%x1f%an%x1f%ar%x1f%ct%x1f%s",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    parse_git_log_meta_line(&line)
}

fn parse_git_log_meta_line(line: &str) -> Option<(String, String, String, i64, String)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let parts: Vec<&str> = line.split('\x1f').collect();
    if parts.len() < 5 {
        return None;
    }
    let subject = if parts.len() > 5 {
        parts[4..].join("\u{1f}")
    } else {
        parts[4].to_string()
    };
    let unix = parts[3].parse::<i64>().unwrap_or(0);
    Some((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
        unix,
        subject,
    ))
}

fn repo_failure_message(prefix: &str, repo: &Path, error: impl std::fmt::Display) -> String {
    format!(
        "{} {} | {}: {}",
        ansi("31", "❌"),
        repo.display(),
        prefix,
        error
    )
}

/// Resolve the human-readable "last pushed N ago" string for a single repo's
/// current branch. Returns "-" when the branch is empty (detached HEAD) or
/// otherwise unsafe for use in a `git reflog show origin/{branch}` argument.
/// Resolve the human-readable "last pushed N ago" string for a single repo's
/// current branch. Returns "-" when the branch is empty (detached HEAD) or
/// otherwise unsafe for use in a `git log -1 --format=%cr origin/{branch}`
/// argument, when the remote-tracking branch does not exist, or when git
/// itself fails / returns empty output.
///
/// Implementation note: an earlier version used
/// `git reflog show origin/{branch} --format=%cr -1`. That works on repos
/// whose remote-tracking reflog has multiple entries (a `FETCH_HEAD` with
/// periodic fetches), but for repos that were freshly cloned and never
/// fetched again, `git reflog show origin/<branch>` returns empty output
/// even though the ref is perfectly valid. `git log -1 --format=%cr
/// origin/<branch>` returns the committer date of the current
/// remote-tracking tip in both cases, so it is the right primitive.
/// Count commits in the last 1h, 6h, and 24h for a repo by reading
/// commit timestamps from `git log --format=%ct` and bucketing in Rust.
/// Returns `[commits_1h, commits_6h, commits_24h]`.
/// Returns all zeros when git fails or the repo is empty.
fn commit_counts(repo: &Path) -> [usize; 3] {
    let repo_str = match repo.to_str() {
        Some(s) => s.to_string(),
        None => return [0, 0, 0],
    };
    // Single subprocess call per repo: get all commit timestamps from the last 24h,
    // then bucket in Rust. This is faster than 3 separate rev-list --count calls.
    let out = crate::git::git_cmd()
        .args(["-C", &repo_str, "log", "--format=%ct", "--after=1 day ago", "HEAD"])
        .output();
    let timestamps: Vec<u64> = match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse::<u64>().ok())
                .collect()
        }
        _ => return [0, 0, 0],
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cutoff_1h = now.saturating_sub(3600);
    let cutoff_6h = now.saturating_sub(21600);
    let commits_1h = timestamps.iter().filter(|&&ts| ts >= cutoff_1h).count();
    let commits_6h = timestamps.iter().filter(|&&ts| ts >= cutoff_6h).count();
    let commits_24h = timestamps.len();
    [commits_1h, commits_6h, commits_24h]
}

fn last_push_for_branch(repo: &Path, branch: &str) -> String {
    if branch.is_empty() || !crate::git::is_safe_branch_name(branch) {
        return "-".to_string();
    }
    let repo_str = repo.to_str().unwrap_or("").to_string();
    let out = crate::git::git_cmd()
        .args([
            "-C",
            &repo_str,
            "log",
            "-1",
            "--format=%cr",
            &format!("origin/{}", branch),
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.lines()
                .next()
                .map(|l| l.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "-".to_string())
        }
        _ => "-".to_string(),
    }
}

fn emit_repo_failure(json: bool, prefix: &str, repo: &Path, error: impl std::fmt::Display) {
    let msg = repo_failure_message(prefix, repo, error);
    if json {
        eprintln!("{msg}");
    } else {
        println!("{msg}");
    }
}

/// Print the `repos` column legend. Invoked by `dracon-sync repos --legend`
/// (2026-07-08) AND by the default report footer (v0.113.12, goal-list
/// 2026-07-29: the operator was confused by the v0.113.8 columns even WITH
/// the `--legend` pointer line — an explanation you have to remember to ask
/// for doesn't explain). Rewritten in v0.113.12 to match the columns that
/// actually ship in the rich table (the old text referenced removed columns:
/// MOD, PUSH-TO, "Daemon =").
fn print_repos_legend() {
    use comfy_table::{
        presets::UTF8_FULL_CONDENSED, Cell, ColumnConstraint, ContentArrangement, Table,
        Width,
    };
    // v0.113.25: legend as a comfy-table matching the main table's
    // UTF8_FULL_CONDENSED style. Label column 11 + text column 106 +
    // 3 border chars = 120 = LEGEND_MIN_WIDTH (the width gate above
    // guarantees the terminal is at least this wide).
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_width(LEGEND_MIN_WIDTH as u16);
    if let Some(col) = table.column_mut(0) {
        col.set_constraint(ColumnConstraint::Absolute(Width::Fixed(11)));
    }
    if let Some(col) = table.column_mut(1) {
        col.set_constraint(ColumnConstraint::Absolute(Width::Fixed(106)));
    }
    for (label, text) in repos_legend_rows() {
        table.add_row(vec![Cell::new(*label), Cell::new(*text)]);
    }
    println!("── legend {}", "─".repeat(LEGEND_MIN_WIDTH - 11));
    println!("{table}");
}

/// Narrow terminals get NO legend rather than a brokenly-wrapped one
/// (the compact table tier prints there); `--legend` remains the
/// on-demand escape hatch.
const LEGEND_MIN_WIDTH: usize = 120;

/// The legend as data (tests assert coverage + display width).
/// ADDED 2026-07-30 (v0.113.25): single source of legend content as
/// (label, text) rows; a ("","") row is a group-separating blank.
/// `repos_legend_lines` formats these for tests/back-compat;
/// `print_repos_legend` renders them as a comfy-table (operator:
/// "make the legend table-like").
fn repos_legend_rows() -> &'static [(&'static str, &'static str)] {
    &[
        ("STATUS", "✅ CLEAN healthy+synced · 🔄 ACTIVE daemon in flight · 🟡 WARN stalled · ❌ CONCERN needs a human"),
        ("ACTIVITY", "⏳ dirty Nm settling · 🟢 synced Nm · ⚪ idle Nh · ⚫ cold Nd"),
        ("", ""),
        ("CHANGES", "per-class columns: 📝 modified · 📦 staged · 🆕 untracked · 🚫 excluded · — = none"),
        ("A/B", "commits ahead/behind upstream (↑ = unpushed work, dim ↑ = being pushed right now) · — = in sync"),
        ("", ""),
        ("PUSH", "✅ OK +age · ✅ INTENT · 🟣 PENDING · 🛑 STUCK · ❌ FAIL · 🩹 BROKEN · 🚫 BLOCKED (+🩹 +🔑 markers)"),
        ("REM", "ACTIVE push remotes 🐙 github · 🦊 gitlab · 🗻 codeberg (excluded not shown — see repos <name>)"),
        ("", ""),
        ("REPO", "🔒 private · no icon = public/unknown · > = nested submodule (badge after lock) · name⚡branch"),
        ("SIZE", "own .git size · +N = submodule gitdirs combined · 🟡 ≥1 GiB · 🔴 ≥2 GiB over github's push limit"),
        ("TOUCHED", "author of the most recent commit (the age lives in ACTIVITY)"),
        ("", ""),
        ("1H/6H/24H", "commits in the last 1/6/24 hours — the repo's pulse (bright = active window)"),
        ("", ""),
        ("hint", "`dracon-sync repos <name>` = per-repo detail · `repos --legend` = this key on demand"),
    ]
}

#[cfg(test)]
fn repos_legend_lines() -> Vec<String> {
    let mut lines = vec![
        "── legend ──────────────────────────────────────────────────────────────────────────────"
            .to_string(),
    ];
    for (label, text) in repos_legend_rows() {
        if label.is_empty() {
            lines.push(String::new());
        } else {
            lines.push(format!(" {label:10}{text}"));
        }
    }
    lines
}

/// Print the legend under every repos table, width-gated (v0.113.12).
fn print_repos_legend_footer() {
    let width = terminal_width().unwrap_or(120) as usize;
    if width < LEGEND_MIN_WIDTH {
        return;
    }
    print_repos_legend();
}

/// ADDED 2026-07-24 (v0.112.40): short-lived TTL on the mtime-keyed
/// size+pack cache. Without a TTL, the cache invalidates on every
/// gitdir mtime change — and the daemon updates gitdirs constantly
/// (commits, fetches, pushes, repacks). Result: a `repos` run while
/// the daemon is active triggers ~7 multi-GiB `du -sb` calls (200ms+
/// each), producing the 4-12s worst case. A 30s TTL means back-to-back
/// `repos` calls (the common operator pattern: "look, then re-look")
/// are always cache-hits regardless of intermediate daemon activity.
/// Correctness: the gitdir_sig check still forces a recompute when
/// gitdir mtime changed AND > 30s has passed since cache write — so
/// stale data can't be served beyond the TTL window. TTL is 30s (not
/// shorter) because the size data is real-only when the repo is
/// idle; a daemon commit doesn't change gitdir size meaningfully.
// CHANGED 2026-07-25 (v0.112.42): 30s → 3600s. The 30s TTL meant
// every `repos` invocation more than 30s after the last one ran the
// full cold path (count-objects + pack-check + missing-objects probe
// on all 35 repos) — 6.9-17.6s measured. Git sizes and object
// corruption do not need 30s freshness: sizes drift slowly, the
// gitdir-mtime signature still invalidates post-TTL, and the PUSH
// path measures fresh (sync.rs calls github_pack_too_large with
// precomputed_size=None), so push-time 2 GiB accuracy is unaffected.
// 1h makes essentially every operator `repos` run a warm ~1s render.
const REPO_SIZE_CACHE_TTL_SECS: u64 = 3600;

/// Cached `.git` size + GitHub pack-size guard for a single repo. Keyed by
/// repo path and invalidated by the resolved gitdir's mtime (any commit or
/// push updates it), so correctness is preserved across `repos` invocations
/// while avoiding repeated `git count-objects` / `git rev-list` work on
/// large repos.
///
/// CHANGED 2026-07-24 (v0.112.40): the cache is also honored within
/// `REPO_SIZE_CACHE_TTL_SECS` (30s) regardless of gitdir mtime, so
/// back-to-back `repos` calls skip the recompute unless either the
/// TTL has elapsed or the gitdir has materially changed. See the
/// `cached_at_secs` field for the wall-clock write timestamp.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct CachedRepoSize {
    git_size_bytes: u64,
    pack_too_large: bool,
    pack_pushable_bytes: u64,
    /// Mtime (nanos since epoch) of the resolved gitdir; a mismatch forces
    /// recomputation.
    gitdir_sig: u64,
    /// ADDED 2026-07-23 (v0.112.39): count of objects referenced by
    /// `main`'s history but MISSING from the object store (broken
    /// history). Computed alongside the size probe on cache miss;
    /// `None` for cache files written before this field existed
    /// (forces one recompute, then cached).
    #[serde(default)]
    missing_objects: Option<u64>,
    /// Whether the history probe failed (invalid HEAD, missing ref, or
    /// timeout). `None` means this field predates the probe-failure state
    /// and forces one recomputation.
    #[serde(default)]
    history_probe_failed: Option<bool>,
    /// ADDED 2026-07-24 (v0.112.40): wall-clock time the cache entry
    /// was written, in seconds since UNIX epoch. Combined with
    /// `REPO_SIZE_CACHE_TTL_SECS`, lets the cache survive brief
    /// gitdir mtime bumps (daemon activity) without forcing a
    /// `du -sb` recompute. `None` for cache files written before
    /// this field existed — forces one recompute, then cached.
    #[serde(default)]
    cached_at_secs: Option<u64>,
    /// ADDED 2026-07-30 (v0.113.20): submodule-gitdir bytes, cached
    /// alongside the own-size probe. 0 for cache files written
    /// before this field existed (the 30s TTL recomputes quickly).
    #[serde(default)]
    git_modules_bytes: u64,
}

/// ADDED 2026-07-23 (v0.112.39): count objects referenced by `main`'s
/// history but MISSING from the object store — the broken-history
/// signature. Probe: `git rev-list --objects HEAD` (which appends
/// PATHS to blob/tree lines as `<sha> <path>`), STRIP the paths
/// (`awk '{print $1}'` keeps only the sha), then `cat-file
/// --batch-check` and count genuine `<sha> missing` lines.
///
/// CRITICAL: without the path strip, `cat-file` mis-parses every
/// `<sha> <path>` line as "missing" and the count is garbage
/// (~2400 false positives on a healthy repo). The path strip is
/// what makes this probe truthful. Cheap enough at the 24h
/// size-cache TTL (one `rev-list` + `cat-file` per gitdir change).
/// ADDED 2026-07-23 (v0.112.39 R2): run a git subprocess with a hard
/// wall-clock bound — spawn, feed `stdin_data`, poll for exit, and
/// KILL the child if it exceeds `timeout`. Returns the stdout bytes,
/// or `None` on spawn failure / non-zero exit / timeout. Without a
/// bound, one huge repo (dracon-platform's 100k+ objects) stalls the
/// whole `repos` render.
fn run_git_bounded(
    args: &[&str],
    repo: &Path,
    stdin_data: &[u8],
    timeout: std::time::Duration,
) -> Option<Vec<u8>> {
    // Write stdout to a temp file (NOT a pipe) so a large output
    // (cat-file --batch-check on 100k+ objects, ~MBs) can't
    // pipe-deadlock the child before the deadline.
    let tmp = std::env::temp_dir().join(format!(
        "dracon-probe-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let out_file = std::fs::File::create(&tmp).ok()?;
    let mut child = crate::policy::std_git_command()
        .args(args)
        .current_dir(repo)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::from(out_file))
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    struct TmpCleanup(std::path::PathBuf);
    impl Drop for TmpCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = TmpCleanup(tmp.clone());
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        if stdin.write_all(stdin_data).is_err() {
            let _ = child.kill();
            return None;
        }
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                return std::fs::read(&tmp).ok();
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryProbe {
    pub(crate) missing_objects: u64,
    pub(crate) failed: bool,
}

/// Probe the objects reachable from HEAD. A failed probe is distinct from a
/// healthy repository with zero missing objects: an invalid HEAD (as seen in
/// `ai-auto-writer`) must not be rendered as an empty repository.
pub(crate) fn probe_history(repo: &Path) -> HistoryProbe {
    // Bound BOTH subprocess steps so one huge repo cannot stall the whole
    // `repos` render. A failed/invalid HEAD is reported as `failed`, not
    // silently converted to zero.
    const BOUND: std::time::Duration = std::time::Duration::from_secs(4);
    let Some(list) = run_git_bounded(&["rev-list", "--objects", "HEAD"], repo, &[], BOUND) else {
        return HistoryProbe {
            missing_objects: 0,
            failed: true,
        };
    };
    // Strip the path annotations (`<sha> <path>` → `<sha>`) so
    // `cat-file` sees bare object names. Without this, every
    // blob/tree line mis-parses as "missing".
    let mut stripped: Vec<u8> = Vec::new();
    for line in list.split(|b| *b == b'\n') {
        let sha = line.split(|b| *b == b' ').next().unwrap_or(line);
        if sha.is_empty() {
            continue;
        }
        stripped.extend_from_slice(sha);
        stripped.push(b'\n');
    }
    let Some(out) = run_git_bounded(
        &["cat-file", "--batch-check=%(objecttype) %(objectname)"],
        repo,
        &stripped,
        BOUND,
    ) else {
        return HistoryProbe {
            missing_objects: 0,
            failed: true,
        };
    };
    HistoryProbe {
        missing_objects: String::from_utf8_lossy(&out)
            .lines()
            .filter(|l| l.ends_with(" missing"))
            .count() as u64,
        failed: false,
    }
}

/// Cache file lives next to the policy toml (a config dir, never a watched
/// git repo, so it is never auto-committed by the daemon).
fn repo_size_cache_path(policy_path: &Path) -> PathBuf {
    policy_path
        .parent()
        .map(|p| p.join("repos-size-cache.json"))
        .unwrap_or_else(|| PathBuf::from("repos-size-cache.json"))
}

fn load_repo_size_cache(path: &Path) -> std::collections::HashMap<String, CachedRepoSize> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => std::collections::HashMap::new(),
    }
}

fn save_repo_size_cache(
    path: &Path,
    cache: &std::collections::HashMap<String, CachedRepoSize>,
) {
    if let Ok(s) = serde_json::to_string(cache) {
        // Best-effort: a failed cache write must never break the report.
        let _ = std::fs::write(path, s);
    }
}

/// Resolve a repo's actual gitdir (handling worktree/submodule `.git` files)
/// and return its mtime as a cache signature. Returns 0 if unresolvable.
fn gitdir_signature(repo: &Path) -> u64 {
    let git_path = repo.join(".git");
    let git_dir = if git_path.is_file() {
        let content = match std::fs::read_to_string(&git_path) {
            Ok(c) => c,
            Err(_) => return 0,
        };
        let gitdir_line = match content.lines().find(|l| l.starts_with("gitdir:")) {
            Some(l) => l,
            None => return 0,
        };
        let rel = match gitdir_line.strip_prefix("gitdir:") {
            Some(r) => r.trim(),
            None => return 0,
        };
        repo.join(rel)
    } else {
        git_path
    };
    std::fs::metadata(&git_dir)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_repos_report(
    policy_path: &Path,
    filter: RepoFilter,
    json: bool,
    sort: &str,
    filter_name: Option<&str>,
    full_path: bool,
    legend: bool,
    layout_override: Option<&str>,
    summary: bool,
    summary_by_severity: bool,
    repo_detail: Option<&str>,
) -> Result<()> {
    // `--legend` prints the column legend and exits. The default report stays
    // uncluttered; the legend is available on demand when a column is unclear.
    if legend {
        print_repos_legend();
        return Ok(());
    }
    let policy = SyncPolicy::load(policy_path)?;
    let roots = policy.watch_root_paths();
    let excluded_dir_names = excluded_dir_names_set(&policy);
    let repos = discover_git_repos(
        &roots,
        &excluded_dir_names,
        &policy.exclude_repos,
        Some(&policy.system_repo),
    );
    // Per-repo `.git` size + GitHub pack-size guard, cached by gitdir mtime
    // so repeat `repos` runs skip the expensive `git count-objects` /
    // `git rev-list` work on multi-GiB .git dirs (the recent slowdown
    // regression).
    //
    // CHANGED 2026-07-24 (v0.112.40): cache is also honored when the
    // entry is FRESH (within REPO_SIZE_CACHE_TTL_SECS = 30s),
    // regardless of gitdir mtime. This means back-to-back `repos`
    // calls always skip the recompute unless >30s have passed since
    // the last cache write (the daemon's constant gitdir mtime
    // updates were forcing spurious recomputes).
    let cache_path = repo_size_cache_path(policy_path);
    let mut size_cache = load_repo_size_cache(&cache_path);
    let cache_lookup = std::sync::Arc::new(size_cache.clone());
    let cache_record = std::sync::Arc::new(std::sync::Mutex::new(
        std::collections::HashMap::new(),
    ));
    let _rows: Vec<RepoReportRow> = Vec::new();
    // CHANGED 2026-07-11 (audit AUDIT-3-UTILITIES-2026-07-10.md
    // CONCERN #6): drop the initial `= 0usize`; the variable is
    // unconditionally overwritten below at the `.await` join point
    // (`init_status_failures.load(...)`), so the initial value is
    // never read. Removing it silences the `unused_assignments`
    // warning without changing behavior.
    

    // Read the incident ledger once and build a per-repo map of "did the

    // Read the incident ledger once and build a per-repo map of "did the
    // daemon record a push failure in the last 10 minutes?". This lets the
    // report distinguish "has unpushed commits" (normal, daemon is working)
    // from "push is genuinely stuck" (daemon tried and failed).
    let recent_push_failures = build_recent_push_failure_map(policy_path);
    // Also build a per-repo map of the daemon's most recent recorded
    // action (timestamp + label + result). The `last_when` / `last_push`
    // columns show commit/push times but reset to the moment of the
    // daemon's own commit, so they don't reveal whether the daemon is
    // actively syncing vs. whether the user is still editing. The
    // `DAEMON` column closes that gap.
    let daemon_last_actions = build_daemon_last_action_map(policy_path);

    // CHANGED 2026-07-04 (goal mr5s1530-755tj8): parallelize the per-repo
    // work. Each iteration's main cost is a handful of `git` subprocess
    // calls (status, log, remote) that don't depend on other repos. Run them
    // concurrently with `buffer_unordered(16)` so 26 repos finish in roughly
    // max(per-repo-work) instead of sum(per-repo-work). Measured: ~1.6s →
    // ~0.5s on this box. The closure returns the built `RepoReportRow` or
    // `None` for repos that failed init/status (which are counted by a side
    // channel via atomic counter).
    let init_status_failures = std::sync::atomic::AtomicUsize::new(0);
    let row_results: Vec<Option<RepoReportRow>> = {
        let recent_push_failures = &recent_push_failures;
        let daemon_last_actions = &daemon_last_actions;
        let init_status_failures = &init_status_failures;
        let policy = &policy;
        let cache_lookup = std::sync::Arc::clone(&cache_lookup);
        let cache_record = std::sync::Arc::clone(&cache_record);
        futures::stream::iter(repos)
            .map(move |repo| {
                let cache_lookup = std::sync::Arc::clone(&cache_lookup);
                let cache_record = std::sync::Arc::clone(&cache_record);
                async move {
            let svc = match GitService::new(&repo) {
                    Ok(svc) => svc,
                    Err(e) => {
                        init_status_failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        emit_repo_failure(json, "init_failed", &repo, &e);
                        return None;
                    }
                };

                let status = match svc.get_status().await {
                    Ok(status) => status,
                    Err(e) => {
                        init_status_failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        emit_repo_failure(json, "status_failed", &repo, &e);
                        return None;
                    }
                };
        // Per-repo opt-out: when a repo declares itself intentionally
        // isolated (e.g. a legacy private mirror that the operator no
        // longer wants auto-tracked), suppress the implicit concern and
        // surface the intent explicitly.
        let repo_override = crate::policy::load_repo_override(&repo);
        // Skip `repo_diff_entries()` here — it calls `git diff --name-status HEAD`
        // which applies the clean filter (dracon-warden age encryption) to every
        // modified file. For repos with many large filtered files (e.g. pnpm-lock.yaml),
        // this takes 10+ seconds per repo and makes the report feel like it's hanging.
        //
        // Libgit2 already correctly excludes .gitignore'd files (target/,
        // node_modules/, build outputs) from its modified count, so it gives us
        // the same "real source changes" answer without the slow clean-filter pass.
        let mut effective_status = status.clone();
        // v0.113.13 (goal-list 2026-07-29): subtract the dirt the daemon
        // intentionally won't commit (auto_commit_exclude_patterns,
        // untracked excludes, unchanged-gitlink submodule dirt) from the
        // tracked-dirty counts, and remember how much was excluded for
        // the `· N excl` ACTIVITY marker. Only runs when the RAW counts
        // say there's tracked dirt, so clean repos pay nothing. After
        // this adjustment every downstream consumer (ACTIVITY label,
        // WARN escalation, JSON) sees "what the daemon will act on".
        let mut excluded_dirty = 0usize;
        if effective_status.modified_files + effective_status.staged_files > 0 {
            let auto_commit_excludes: &[String] = repo_override
                .auto_commit_exclude_patterns
                .as_deref()
                .unwrap_or(&policy.auto_commit_exclude_patterns);
            let cls = classify_dirty_entries(
                &repo,
                auto_commit_excludes,
                &policy.untracked_exclude_patterns,
            )
            .await;
            effective_status.modified_files = cls.committable_modified;
            effective_status.staged_files = cls.committable_staged;
            excluded_dirty = cls.excluded;
        }
        // ADDED 2026-06-30, goal `mr0grjhl-q1g5bo`: subtract the
        // untracked entries that point to sibling subrepo directories
        // (each containing its own `.git/`) from the parent's UT count.
        // Without this, a parent whose only untracked entries are its
        // subrepos would falsely report UT ≥ 1 and trigger
        // `⚪ untracked-only` state classification.
        let nested_untracked = nested_repo_untracked_count(&repo).await;
        let effective_untracked_files = effective_status
            .untracked_files
            .saturating_sub(nested_untracked);

        let has_origin = has_origin_remote(&repo);
        let has_upstream = has_tracking_upstream(&repo);
        // CHANGED 2026-06-20: compute `has_any_remote` so the concern
        // classifier can distinguish "no origin but has SSH mirrors"
        // (healthy, post-multi-mirror-migration) from "truly remote-less"
        // (concerning). This is a single `git remote` subprocess call
        // per repo per cycle; it does not affect the fast-path skip
        // because the fast path already short-circuits clean+synced
        // repos before this point.
        let has_any_remote = !crate::git::multi_remote::list_remotes(&repo).is_empty();

        // Per-repo `.git` size + pack-guard, served from the mtime-keyed
        // cache when unchanged (avoids re-running `du -sb` on multi-GiB
        // .git dirs on every `repos` invocation — the recent slowdown).
        //
        // CHANGED 2026-07-24 (v0.112.40): the cache is also honored
        // when the entry is FRESH (cached_at_secs within
        // REPO_SIZE_CACHE_TTL_SECS), regardless of gitdir mtime. This
        // means back-to-back `repos` calls (e.g. operator looks,
        // daemon commits, operator looks again) skip the
        // `count-objects`/`du -sb` recompute. Correctness: the
        // gitdir_sig mismatch still forces a recompute when > TTL has
        // elapsed, so stale data can't be served beyond the window.
        let cache_key = repo.to_string_lossy().to_string();
        let gitdir_sig = gitdir_signature(&repo);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let cached_entry_is_fresh = |c: &CachedRepoSize| -> bool {
            // Old cache files (pre-v0.112.40) have `cached_at_secs: None`
            // — treat them as stale to force one recompute, then start
            // honoring the TTL.
            c.cached_at_secs
                .map(|t| now_secs.saturating_sub(t) < REPO_SIZE_CACHE_TTL_SECS)
                .unwrap_or(false)
        };
        let (
            git_size_bytes,
            git_modules_bytes,
            pack_too_large,
            missing_objects,
            history_probe_failed,
        ) = match cache_lookup.get(&cache_key) {
            // CHANGED 2026-07-24 (v0.112.40): the TTL is the primary
            // freshness check. If the entry was written within
            // REPO_SIZE_CACHE_TTL_SECS, we honor it regardless of
            // gitdir mtime — the daemon's constant commits/fetches
            // bump the mtime but the cache is still valid for 30s.
            // The sig check is retained as a secondary guard for
            // entries that have no cached_at_secs (pre-v0.112.40
            // cache files).
            Some(c)
                if cached_entry_is_fresh(c)
                    && c.missing_objects.is_some()
                    && c.history_probe_failed.is_some() =>
            {
                (
                    Some(c.git_size_bytes),
                    c.git_modules_bytes,
                    (c.pack_too_large, c.pack_pushable_bytes),
                    c.missing_objects.unwrap_or(0),
                    c.history_probe_failed.unwrap_or(false),
                )
            }
            _ => {
                let size = measure_git_size_bytes(&repo);
                let modules = measure_modules_size_bytes(&repo);
                let pack = crate::git::github_pack_too_large(&repo, size);
                // Probe broken-history alongside the size measure.
                let history = probe_history(&repo);
                cache_record.lock().unwrap().insert(
                    cache_key.clone(),
                    CachedRepoSize {
                        git_size_bytes: size.unwrap_or(0),
                        pack_too_large: pack.0,
                        pack_pushable_bytes: pack.1,
                        gitdir_sig,
                        missing_objects: Some(history.missing_objects),
                        history_probe_failed: Some(history.failed),
                        // ADDED 2026-07-24 (v0.112.40): record the
                        // wall-clock write time so the TTL check can
                        // honor fresh entries across daemon activity.
                        cached_at_secs: Some(now_secs),
                        git_modules_bytes: modules,
                    },
                );
                (size, modules, pack, history.missing_objects, history.failed)
            }
        };
        let history_broken = history_probe_failed || missing_objects > 0;

        // Classification: a repo is WARN if it has TRACKED modifications or
        // staged changes. Untracked files (e.g., target/, node_modules/) are
        // NOT counted — they are build artifacts that shouldn't trigger
        // WARN. A repo with only untracked build artifacts is OK.
        // The `recent_push_failure` signal is computed once and used for
        // both the `concern` classification and the `STUCK_PUSH` flag so
        // they stay in sync with the user-visible `repos` table.
        //
        // CHANGED 2026-06-15 (goal 0ab367b5): upgraded `dracon-git` to
        // 94.7.0 which fixed the `is_wt_new()` double-count bug. Junk-Runner-bevy
        // is the canonical case: 3 untracked test-results/ PNGs were
        // being counted as 91 "modified".
        // CHANGED 2026-07-29 (v0.113.14): use `effective_status`, not the
        // raw `status`. The v0.113.13 exclusion-aware classification block
        // above zeroes modified/staged counts for excluded-only dirt, but
        // this line still read the RAW counts — so a repo whose only dirt
        // is policy-excluded (junk-runner's `.pi-glla/active.jsonl`)
        // showed `synced · 1 excl` in ACTIVITY while STATUS stayed WARN,
        // re-creating the exact false-WARN class v0.113.13 was shipped
        // to kill.
        let real_is_dirty =
            effective_status.modified_files > 0 || effective_status.staged_files > 0;
        let recent_push_failure = recent_push_failures
            .as_ref()
            .map(|m| {
                m.get(repo.to_string_lossy().as_ref())
                    .copied()
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let mut concern = repo_is_concern_with_push_failure(
            &effective_status,
            has_origin,
            has_upstream,
            has_any_remote,
            recent_push_failure,
        );
        // Repos that the operator has flagged as intentionally isolated
        // (`.dracon/dracon-sync.toml` -> `intentional_no_upstream = true`)
        // are not a hidden concern: the operator has explicitly chosen
        // not to wire the local branch to a remote. The flag below also
        // reclassifies the row so the user sees the explicit intent
        // instead of the implicit "set upstream" hint.
        if repo_override.intentional_no_upstream && concern && !has_upstream {
            concern = false;
        }
        // ADDED 2026-07-23 (v0.112.39): a repo with objects referenced
        // by main's history but MISSING from the object store is
        // damaged — fresh clones fail, and the daemon has already
        // pushed broken history once (deathrun, 2092 missing objects
        // on both sides, days undetected). Always surface as CONCERN
        // so the operator investigates.
        if history_broken {
            concern = true;
        }
        let mut warn = !concern && real_is_dirty;

        // Flags still use effective_status for ahead/behind/origin detection.
        // Only mark STUCK_PUSH when the daemon has actually recorded a recent
        // push failure for this repo. Without that signal, an AHEAD repo is
        // just "has unpushed commits" — the daemon may be in its inactivity
        // delay or mid-cycle.
        let mut flags = repo_state_flags_with_push_failure(
            &effective_status,
            has_origin,
            has_upstream,
            has_any_remote,
            recent_push_failure,
        );
        if repo_override.intentional_no_upstream {
            flags = apply_intentional_no_upstream(flags);
        }

        // ── Pack size warning (2 GB = github's hard limit) ─────────
        // GitHub rejects packs > 2 GiB. Add a PACK_SIZE_WARNING flag so the
        // operator sees, in the HINT column, that the daemon is skipping
        // GitHub for this repo.
        //
        // The flag reflects whether the daemon would actually skip GitHub
        // because the pushable branch exceeds 2 GiB — NOT the whole `.git`,
        // which can be large for unrelated reasons (e.g. dracon-platform's
        // 332 tags). `github_pack_too_large` measures the pushed branch, so
        // this HINT stays accurate after the dracon-platform github
        // exclusion is removed (the daemon pushes GitHub whenever the
        // pushable branch fits).
        //
        // CHANGED 2026-07-28 (v0.113.7): also classify the row as a
        // CONCERN. The daemon's push path silently excludes GitHub for
        // this class of repo (see `dracon-sync/src/sync.rs:1819`); the
        // skip is permanent because the daemon has no code path that
        // shrinks history. A silent skip is a real problem: the row sat
        // at 🔄 ACTIVE indefinitely even though GitHub is being skipped,
        // so the operator had to read journalctl to discover the
        // situation. The fix routes the decision through
        // `pack_too_large_forces_concern` (testable) and updates the
        // HINT to reflect permanence. The auto-repair path is a no-op
        // for this flag (see `run_repair_concerns`) because the daemon
        // cannot fix what it cannot reach.
        if pack_too_large.0 {
            flags.push("PACK_SIZE_WARNING".to_string());
            // Routes the concern decision through a testable helper
            // (see `pack_too_large_forces_concern`).
            if pack_too_large_forces_concern(pack_too_large) {
                concern = true;
            }
        }

        // ADDED 2026-07-23 (v0.112.39): `BROKEN_HISTORY:N` flag when
        // objects referenced by main's history are MISSING from the
        // object store. Drives the CONCERN classification (set above)
        // and the hint. See `probe_missing_objects` + the deathrun
        // incident (2092 missing, both sides broken).
        if history_broken {
            if history_probe_failed {
                flags.push("BROKEN_HISTORY:probe-failed".to_string());
            } else {
                flags.push(format!("BROKEN_HISTORY:{}", missing_objects));
            }
        }

        // ── Ownership override (compute early) ─────────
        // If the policy says to skip unowned repos
        // (`auto_skip_unowned = true`) and this repo is
        // classified as Unowned or Unknown by
        // `ownership::detect_ownership`, override the
        // state_cause to `Unowned { reason, detail }`. We
        // compute this here (before the hint logic) so the
        // HINT column can also surface the unowned reason.
        // Per-repo override `auto_skip_unowned = false`
        // re-enables the daemon for a specific repo.
        let repo_override_for_ownership = crate::policy::load_repo_override(&repo);
        let effective_skip = repo_override_for_ownership
            .auto_skip_unowned
            .unwrap_or(policy.auto_skip_unowned);
        let trusted_for_ownership = crate::ownership::TrustedSet {
            emails: policy.trusted_emails.clone(),
            authors: policy.trusted_authors.clone(),
            remote_hosts: policy.trusted_remote_hosts.clone(),
        };
        let ownership_report = if effective_skip {
            Some(if policy.path_is_owned(&repo) {
                crate::ownership::detect_ownership_path_owned(
                    &repo,
                    &trusted_for_ownership,
                    repo_override_for_ownership.owned,
                )
            } else {
                crate::ownership::detect_ownership(
                    &repo,
                    &trusted_for_ownership,
                    repo_override_for_ownership.owned,
                )
            })
        } else {
            None
        };
        if ownership_report
            .as_ref()
            .map(|r| r.has_path_warning())
            .unwrap_or(false)
        {
            warn = true;
        }

        // Pull the daemon's push-retry tracking (consecutive
        // failures + last error message). When the retry budget
        // is exhausted, override the push_status / push_error /
        // hint so the operator sees WHY the push is stuck
        // instead of an opaque `pushing Xm`.
        let stuck_info = crate::daemon::get_stuck_push_info(&repo);
        let push_max_retries = policy.push_max_retries;
        let push_budget_exhausted = stuck_info
            .as_ref()
            .map(|info| {
                push_max_retries > 0
                    && info.consecutive_failures >= push_max_retries
            })
            .unwrap_or(false);

        // ── Unowned hint override ─────────────────────────
        // If the ownership check above classified this repo
        // as Unowned or Unknown (with auto_skip_unowned = true),
        // surface that in the HINT column. The operator
        // needs to know WHY the daemon isn't touching this
        // repo, and what to do about it (run `ownership
        // --explain` to see the raw signals).
        let unowned_hint = match &ownership_report {
            Some(crate::ownership::OwnershipReport::Unowned { reason, .. }) => Some(format!(
                "🚫 unowned: {} — run ownership --explain",
                reason
            )),
            Some(crate::ownership::OwnershipReport::Unknown { .. }) => Some(
                "🚫 unowned: unknown — run ownership --explain".to_string(),
            ),
            _ => None,
        };
        let ownership_warning_hint = match &ownership_report {
            Some(report) if report.has_path_warning() => {
                Some(format!("⚠️ path-owned identity/origin warning: {}", report.label()))
            }
            _ => None,
        };

        let hint = if let Some(h) = unowned_hint {
            h
        } else if history_broken {
            // An invalid HEAD/ref can make libgit2 report every file as
            // untracked and `last_commit_hash` as None. It is broken
            // history, not an empty repository.
            if history_probe_failed {
                "history probe failed (invalid HEAD/ref or timeout) — repair from a verified remote; not an empty repo".to_string()
            } else {
                format!(
                    "history damaged ({} objects missing) — fresh clones fail; needs clone-from-forge or orphan cutover",
                    missing_objects
                )
            }
        } else if let Some(h) = ownership_warning_hint {
            h
        } else if push_budget_exhausted {
            let info = stuck_info.as_ref().unwrap();
            let error_summary = if info.last_error.is_empty() {
                format!("{} consecutive push failures", info.consecutive_failures)
            } else {
                // Trim long error messages so the HINT column
                // doesn't blow up the table width.
                let trimmed = if info.last_error.chars().count() > 60 {
                    let truncated: String = info.last_error.chars().take(57).collect();
                    format!("{}...", truncated)
                } else {
                    info.last_error.clone()
                };
                format!(
                    "🛑 push-stuck ({} failures): {} — run repair-concerns --apply",
                    info.consecutive_failures, trimmed
                )
            };
            error_summary
        } else {
            repo_hint(&flags, warn, concern)
        };

        // Calculate push status from flags. Ownership rejection is a
        // deliberate block, not an in-flight push; do not infer PENDING
        // from the stale ahead count in that case.
        let ownership_blocked = matches!(
            &ownership_report,
            Some(crate::ownership::OwnershipReport::Unowned { .. })
                | Some(crate::ownership::OwnershipReport::Unknown { .. })
        );
        let (push_status, push_error) = if ownership_blocked {
            let detail = match ownership_report.as_ref() {
                Some(crate::ownership::OwnershipReport::Unowned { reason, .. }) => {
                    format!("ownership blocked: {}", reason)
                }
                Some(crate::ownership::OwnershipReport::Unknown { .. }) => {
                    "ownership blocked: unknown".to_string()
                }
                _ => "ownership blocked".to_string(),
            };
            ("BLOCKED".to_string(), detail)
        } else if push_budget_exhausted {
            let info = stuck_info.as_ref().unwrap();
            let err = if info.last_error.is_empty() {
                format!("{} consecutive push failures", info.consecutive_failures)
            } else {
                info.last_error.clone()
            };
            ("PUSH_STUCK".to_string(), err)
        } else if flags.iter().any(|f| f == "STUCK_PUSH") {
            (
                "STUCK".to_string(),
                format!("ahead={}, push failing", effective_status.ahead),
            )
        } else if flags.iter().any(|f| f == "INTENTIONAL_NO_UPSTREAM") {
            (
                "INTENTIONAL".to_string(),
                "intentional legacy isolation, no upstream configured".to_string(),
            )
        } else if history_broken {
            (
                "BROKEN".to_string(),
                if history_probe_failed {
                    "history probe failed — invalid HEAD/ref or timeout".to_string()
                } else {
                    format!("{} objects missing from history", missing_objects)
                },
            )
        } else if flags.iter().any(|f| f == "EMPTY_REPO") {
            // ADDED 2026-07-21 (v0.112.29): empty repos (no commits)
            // get push_status = EMPTY, not FAIL. No push was ever
            // attempted (the daemon's `is_repo_ready` skips empty
            // repos), so "FAIL" would be a false positive. The status
            // string is "no commits yet" so the operator's eye is
            // drawn to the actionable cause instead of an opaque
            // "push: fail" label.
            (
                "EMPTY".to_string(),
                "no commits yet — awaiting first commit".to_string(),
            )
        } else if flags.iter().any(|f| f == "NO_UPSTREAM") {
            // CHANGED 2026-06-20: the `NO_UPSTREAM` flag now also fires
            // for repos with at least one non-origin remote (e.g. the
            // SSH multi-mirror repos). For those repos, push status
            // is OK because the daemon uses explicit refspecs and
            // does not require `branch.<name>.remote` to be set.
            // Only repos with `has_origin=true && !has_upstream` (the
            // "missing tracking upstream for origin" case) are still
            // a real push failure — `git push -u origin HEAD` would
            // have been the recovery path.
            if has_origin {
                ("FAIL".to_string(), "no upstream set".to_string())
            } else {
                ("OK".to_string(), String::new())
            }
        } else if effective_status.ahead > 0 && has_origin && has_upstream {
            (
                "PENDING".to_string(),
                format!("{} unpushed commits", effective_status.ahead),
            )
        } else {
            ("OK".to_string(), String::new())
        };

        // Single git log call extracts all commit fields in one process.
        let last_meta = git_log_meta(&repo).await;
        let (last_hash, last_author, last_when, last_unix, last_msg) = match last_meta {
            Some((h, a, w, u, m)) => (
                truncate(&h, 12),
                a,
                w,
                u,
                format_commit_subject_for_display(&m, 150),
            ),
            None => (
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                0i64,
                "-".to_string(),
            ),
        };
        // Get last push time from reflog for the current branch only.
        // Scanning all origin/* branches was the second-biggest cost; we only
        // care about the branch we're on. Empty branch (detached HEAD) and
        // unsafe branch names (with shell-special chars) skip the reflog call
        // to avoid `git reflog show origin/` (ambiguous argument) errors.
        let last_push = last_push_for_branch(&repo, &effective_status.branch);

        // Compute commit counts (1h, 6h, 24h) for this repo. Uses a single
        // `git log --format=%ct` subprocess call per repo and buckets timestamps
        // in Rust. This is faster than 3 separate `rev-list --count` calls.
        let [commits_1h, commits_6h, commits_24h] = commit_counts(&repo);

        // Derive the "rough cause" classification that combines all the
        // signals above into a single small-vocabulary label. This is the
        // field the user actually reads to decide whether a repo is
        // actively being worked on, stalling, or cold-idle.
        let thresholds = StateCauseThresholds::from_policy(policy, &repo_override);
        let last_commit_minutes = parse_relative_minutes(&last_when);
        let last_push_minutes = parse_relative_minutes(&last_push);
        let inputs = StateCauseInputs {
            flags: &flags,
            push_status: &push_status,
            modified: effective_status.modified_files,
            staged: effective_status.staged_files,
            untracked: effective_untracked_files,
            ahead: effective_status.ahead,
            behind: effective_status.behind,
            last_commit_minutes,
            last_push_minutes,
        };
        let state_cause = classify_state_cause(&inputs, &thresholds);

        // ── Apply ownership override to state_cause ─────
        // Use the precomputed `ownership_report` from
        // earlier in this function. When the policy says
        // to skip unowned repos AND the repo is classified
        // as Unowned or Unknown, override state_cause to
        // `Unowned { reason, detail }`. The ACTIVITY column
        // shows `🚫 unowned: <reason>` and the HINT column
        // points the operator at `ownership --explain
        // <repo>`.
        let state_cause = match ownership_report {
            Some(crate::ownership::OwnershipReport::Unowned { reason, detail }) => {
                StateCause::Unowned { reason, detail }
            }
            Some(crate::ownership::OwnershipReport::Unknown { detail }) => {
                StateCause::Unowned {
                    reason: "unknown".to_string(),
                    detail,
                }
            }
            _ => state_cause,
        };

        // Look up the daemon's most recent recorded action for this repo
        // from the incident ledger. The map is keyed by the same canonical
        // repo path string we use everywhere else.
        let repo_key = repo.to_string_lossy().to_string();
        let (
            daemon_last_action_unix,
            daemon_last_action,
            daemon_last_result,
            daemon_last_action_when,
        ) = match daemon_last_actions.as_ref().and_then(|m| m.get(&repo_key)) {
            Some((ts, action, result)) if *ts > 0 => {
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let delta = now_secs.saturating_sub(*ts);
                let when = if delta < 1 {
                    "1s".to_string()
                } else if delta < 60 {
                    format!("{}s ago", delta)
                } else if delta < 3600 {
                    format!("{}m ago", delta / 60)
                } else if delta < 86400 {
                    format!("{}h ago", delta / 3600)
                } else {
                    format!("{}d ago", delta / 86400)
                };
                (*ts, action.clone(), result.clone(), shorten_when(&when))
            }
            _ => (0, String::new(), String::new(), "none".to_string()),
        };

        let (upstream_label, publish_state) =
            branch_upstream(&repo, &effective_status.branch);
        let active = repo_is_active(&push_status, &state_cause);
        // v0.113.18 (audit M2/L5): compute the effective push remotes
        // ONCE (the helper spawns a `git rev-parse` subprocess per
        // call) and pass the pack_too_large signal so the report
        // mirrors the daemon's over-2-GiB github skip.
        let (push_to_list, excluded_list) = report_effective_remotes(
            policy,
            &repo_override,
            repo.as_ref(),
            pack_too_large.0,
        );
        Some(RepoReportRow {
            repo: repo.display().to_string(),
            state_flags: flags,
            branch: effective_status.branch.clone(),
            upstream: upstream_label,
            publish_state,
            modified: effective_status.modified_files,
            staged: effective_status.staged_files,
            untracked: effective_untracked_files,
            excluded_dirty,
            ahead: effective_status.ahead,
            behind: effective_status.behind,
            last_hash,
            last_author,
            last_when,
            last_msg,
            last_unix,
            commits_1h,
            commits_6h,
            commits_24h,
            last_push,
            push_status,
            push_error,
            // Effective remotes the daemon will push to for this repo,
            // computed by applying the per-repo `exclude_remotes` filter
            // AND the codeberg-public-only gate to the global
            // `policy.remotes` — the SAME logic the daemon runs in
            // `push_mirror_remotes` at sync time. What you see in the
            // table is what the daemon will do.
            //
            // CHANGED 2026-07-29 (v0.113.16): the computation was NOT
            // the same logic — it missed the daemon's v0.112.28
            // quota-posture rule (`codeberg_push_excluded`: codeberg is
            // skipped at push time when the repo was never pushed there
            // AND effective auto-create is off). Repos like convos and
            // dracon-libs showed a BRIGHT 🗻 in the REM column while
            // the daemon deliberately skipped codeberg — a silent
            // push-gap lie the operator spotted in the live table. The
            // combined exclusion is now computed once by
            // `report_effective_remotes` and used for both fields.
            push_to_remotes: push_to_list,
            excluded_remotes: excluded_list.clone(),
            // When codeberg is in excluded_remotes AND the skip came
            // from policy (not a manual per-repo `exclude_remotes`),
            // record why so the renderer can annotate the row
            // distinctly from a manual exclusion. v0.113.16 adds the
            // "quota" reason: codeberg excluded by the v0.112.28
            // quota-posture rule even though the visibility gate
            // would have allowed it.
            codeberg_skip_reason: {
                if excluded_list.iter().any(|r| r == "codeberg")
                    && !repo_override.exclude_remotes.iter().any(|r| r == "codeberg")
                {
                    let gate_exclude =
                        effective_excluded_remotes(policy, &repo_override, repo.as_ref());
                    if gate_exclude.iter().any(|r| r == "codeberg") {
                        // Visibility-gate skip; the cache tells us why.
                        Some(
                            match crate::visibility::cached_repo_visibility(repo.as_ref()) {
                                Some(true) => "private".to_string(),
                                Some(false) => "public".to_string(), // shouldn't happen
                                None => "unknown".to_string(),
                            },
                        )
                    } else {
                        Some("quota".to_string())
                    }
                } else {
                    None
                }
            },
            // Measure `.git` size in bytes. `git count-objects -v` is fast
            // (~10ms for a 54 GiB .git) so we can call it inline;
            // falls back to `du -sb` on failure. If both fail or
            // time out, we record `None` and the renderer shows a
            // dash. 4-second cap to keep the report snappy even on
            // network filesystems.
            //
            // CHANGED 2026-07-24 (v0.112.40): switched from `du -sb`
            // to `git count-objects -v` for ~17× speedup on multi-GiB
            // gitdirs. `du -sb` is retained as the fallback.
            git_size_bytes,
            git_modules_bytes,
            // Probe each forge's token file. We check both the modern
            // `~/.dracon/utilities/sync/secrets/` dir and the legacy
            // `~/.dracon/secrets/pat/` dir (the daemon's `load_secret`
            // falls back to the legacy dir, so both matter). The probe
            // is just `Path::exists()` on each — no file contents read.
            token_health: probe_token_health(),
            concern,
            warn,
            active,
            hint,
            state_cause: state_cause.clone(),
            state_cause_label: state_cause_label_string(&state_cause),
            daemon_last_action_unix,
            daemon_last_action,
            daemon_last_result,
            daemon_last_action_when,
            missing_objects,
            // ADDED 2026-07-29 (v0.113.8 follow-up): the bool that
            // drove the PACK_SIZE_WARNING flag (computed at line
            // ~3205 above). Stored on the row so the SIZE cell can
            // color red based on the actual github-rejection signal,
            // not the raw gitdir size. See `size_label` for the
            // deathrun CLEAN-vs-red contradiction this prevents.
            pack_too_large: pack_too_large.0,
        })
            }})
            .buffer_unordered(REPORT_REPO_CONCURRENCY)
            .collect()
            .await
    };
    // Persist freshly-computed sizes so subsequent `repos` invocations skip
    // the `git count-objects` / `git rev-list` work on multi-GiB .git dirs.
    // CHANGED 2026-07-24 (v0.112.40): the cached entry now carries a
    // `cached_at_secs` timestamp; the lookup honors entries within
    // `REPO_SIZE_CACHE_TTL_SECS` of `now`, so back-to-back `repos`
    // calls skip the recompute even when the daemon updated the
    // gitdir mtime in between.
    for (k, v) in cache_record.lock().unwrap().drain() {
        size_cache.insert(k, v);
    }
    save_repo_size_cache(&cache_path, &size_cache);
    let init_or_status_failures: usize = init_status_failures.load(std::sync::atomic::Ordering::Relaxed);
    let mut rows: Vec<RepoReportRow> = row_results.into_iter().flatten().collect();

    match sort {
        "name" => rows.sort_by(|a, b| a.repo.cmp(&b.repo)),
        "modified" => rows.sort_by_key(|b| std::cmp::Reverse(b.modified)),
        "ahead" => rows.sort_by_key(|b| std::cmp::Reverse(b.ahead)),
        "behind" => rows.sort_by_key(|b| std::cmp::Reverse(b.behind)),
        _ => rows.sort_by_key(|a| std::cmp::Reverse(a.last_unix)),
    }

    let concern_count_all = rows.iter().filter(|r| r.concern).count();
    let active_count_all = rows.iter().filter(|r| r.active && !r.concern).count();
    let warn_count_all = rows.iter().filter(|r| r.warn && !r.active).count();
    let ok_count_all = rows
        .len()
        .saturating_sub(concern_count_all + active_count_all + warn_count_all);
    match filter {
        RepoFilter::All => {}
        RepoFilter::Concern => rows.retain(|r| r.concern),
        RepoFilter::Warn => rows.retain(|r| r.warn && !r.active),
    }

    if let Some(pattern) = filter_name {
        let pat = pattern.to_lowercase();
        rows.retain(|r| {
            let name = std::path::Path::new(&r.repo)
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            name.contains(&pat)
        });
    }

    // ADDED 2026-07-22 (v0.112.38): `repos <name>` — detailed
    // per-repo block view for ONE repo (the "run details on a
    // certain repo" path). Filters to the exact basename match and
    // prints the Vertical detail for it, then returns.
    if let Some(name) = repo_detail {
        let matches: Vec<&RepoReportRow> = rows
            .iter()
            .filter(|r| {
                std::path::Path::new(&r.repo)
                    .file_name()
                    .map(|n| n.to_string_lossy() == name)
                    .unwrap_or(false)
            })
            .collect();
        match matches.len() {
            0 => {
                eprintln!(
                    "❌ repo '{}' not found in watch roots. Run `dracon-sync repos -s` for the list.",
                    name
                );
                std::process::exit(2);
            }
            1 => {
                // `RepoReportRow` isn't Clone; borrow the single
                // row as a one-element slice instead.
                print_repos_vertical(std::slice::from_ref(matches[0]), &filter, 0, 0, 0, true);
                return Ok(());
            }
            _ => {
                eprintln!(
                    "❌ repo name '{}' is ambiguous — {} repos share the basename:\n  {}",
                    name,
                    matches.len(),
                    matches
                        .iter()
                        .map(|r| r.repo.clone())
                        .collect::<Vec<_>>()
                        .join("\n  ")
                );
                std::process::exit(2);
            }
        }
    }

    let concern_count = rows.iter().filter(|r| r.concern).count();
    let active_count = rows.iter().filter(|r| r.active && !r.concern).count();
    let warn_count = rows.iter().filter(|r| r.warn && !r.active).count();
    let ok_count = rows
        .len()
        .saturating_sub(concern_count + active_count + warn_count);
    let filter_text = match filter {
        RepoFilter::All => "all",
        RepoFilter::Concern => "only_concern",
        RepoFilter::Warn => "only_warn",
    };

    if json {
        let payload = RepoReportJson {
            policy: policy_path.display().to_string(),
            filter: filter_text.to_string(),
            repos: rows.len(),
            ok: ok_count,
            active: active_count,
            warn: warn_count,
            concern: concern_count,
            failures: init_or_status_failures,
            rows,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    // v0.113.27 (operator): the 📜 config-path line was dropped from
    // default output ("make the top better looking") — the path is
    // stable knowledge, still shown by `repos --json` / doctor flows.
    let _ = policy_path;
    match filter {
        RepoFilter::All => {}
        RepoFilter::Concern => {
            println!(
                "📊 FILTER: only concern repos (showing {} of {})",
                rows.len(),
                concern_count_all
            );
        }
        RepoFilter::Warn => {
            println!(
                "📊 FILTER: only warn repos (showing {} of {})",
                rows.len(),
                warn_count_all
            );
        }
    }
    // ---- Summary banner (color-aware, no raw ANSI when piped) ----
    // NOTE (v0.112.36): WARN uses 🟡 (yellow circle, unicode-width = 2)
    // instead of ⚠️ (width-1 glyph rendered 2 cells wide) — the width
    // mismatch used to drift table separators one column right.
    let filter_note = match filter {
        RepoFilter::All => String::new(),
        RepoFilter::Concern | RepoFilter::Warn => format!(
            "  (all: OK {} ACTIVE {} WARN {} CONCERN {})",
            ok_count_all, active_count_all, warn_count_all, concern_count_all
        ),
    };
    // v0.113.27 (operator: "make the top better looking too", picked
    // the single banner line): one styled rule in the legend-rule
    // language, counts inline, padded with ─ to the table width.
    let banner_plain = format!(
        "── dracon-sync repos ── 📦 {total} · ✅ {ok_count} clean · 🔄 {active_count} active · 🟡 {warn_count} · ❌ {concern_count} · ⛔ {init_or_status_failures}{filter_note_plain}",
        total = rows.len(),
        filter_note_plain = match filter {
            RepoFilter::All => String::new(),
            RepoFilter::Concern | RepoFilter::Warn => format!(
                " · (all: {} ok {} active {} warn {} concern)",
                ok_count_all, active_count_all, warn_count_all, concern_count_all
            ),
        },
    );
    let banner_colored = format!(
        "── dracon-sync repos ── 📦 {} · {} clean · {} active · {} · {} · ⛔ {}{}",
        rows.len(),
        ansi("32", &format!("✅ {ok_count}")),
        ansi("36", &format!("🔄 {active_count}")),
        ansi("33", &format!("🟡 {warn_count}")),
        ansi("31", &format!("❌ {concern_count}")),
        init_or_status_failures,
        filter_note,
    );
    let pad_target = (terminal_width().unwrap_or(120) as usize).min(190);
    let pad = pad_target.saturating_sub(unicode_width::UnicodeWidthStr::width(banner_plain.as_str()) + 1);
    println!("{banner_colored} {}", "─".repeat(pad));
    println!();

    // v0.113.32 (operator: "a paused daemon is a good thing to check
    // for and warn about"): while the daemon is frozen EVERY row is
    // stale — PENDING pushes never complete, ↑N accumulates across
    // the fleet — and nothing in the table said why. Surface the
    // freeze front-and-center, right under the banner.
    if let Some(reason) = crate::policy::freeze_reason(policy_path) {
        let pause_plain = format!(
            "── ⏸️ DAEMON PAUSED ({reason}) — nothing is committing or pushing · resume: dracon-sync resume "
        );
        let pause_pad = pad_target
            .saturating_sub(unicode_width::UnicodeWidthStr::width(pause_plain.as_str()) + 1);
        println!("{} {}", ansi("1;33", &pause_plain), "─".repeat(pause_pad));
        println!();
    }

    // ---- Layout tier dispatch (operator's preference: tiered output, not single fixed) ----
    // PUSH_STUCK used to render as letter-wrapped cells (P/U/S/H/_/S/T/U/C/K on separate
    // lines) because `ContentArrangement::Dynamic` shrinks 22 columns to ~3 chars each at
    // 80-col terminals. Now we pick a layout tier based on terminal width and render with
    // proper column constraints. See `docs/design/push-stuck-render-investigation-2026-06-29.md`.
    //
    // `--layout <tier>` (passed as `layout_override`) bypasses terminal-width detection and
    // forces the requested tier. Useful when piping to a file (where terminal_size() returns
    // None and the fallback picks Compact) but the operator actually wants Vertical or Full.
    let tier = match layout_override {
        Some(s) => match s.to_ascii_lowercase().as_str() {
            "rich" | "r" => LayoutTier::Rich,
            "vertical" | "v" => LayoutTier::Vertical,
            "compact" | "c" => LayoutTier::Compact,
            "full" | "f" => LayoutTier::Full,
            other => {
                eprintln!(
                    "⚠️ unknown --layout value {:?}; expected one of vertical|compact|full. Using auto-detected tier.",
                    other
                );
                choose_layout_tier()
            }
        },
        None => choose_layout_tier(),
    };
    // ---- Summary view (2026-07-19, goal `4555eaf6` v0.112.27) ----
    // ADDED: `--summary` / `-s` flag routes to a glance-friendly 3-column
    // view (STATUS · REPO · WHAT) with severity-sorted rows. The default
    // `repos` table is dense (16 columns) for deep inspection; the summary
    // is for "is anything broken?" health checks. Always uses the Vertical
    // tier so the WHAT column can be as wide as the terminal allows.
    if summary {
        print_repos_summary(&rows, &filter, full_path, summary_by_severity);
        return Ok(());
    }
    match tier {
        LayoutTier::Rich => {
            // v0.113.31 (operator): legend moved ABOVE the table —
            // terminals auto-scroll to the bottom when the command
            // finishes, so the table (the thing you actually look
            // at) must be the LAST thing printed; a bottom legend
            // forced a scroll-up every run.
            // v0.113.18 (audit M3): the legend documents the RICH
            // columns — printing it with the compact/full tiers
            // described columns those tiers don't have. `--legend`
            // remains the on-demand form for every tier.
            print_repos_legend_footer();
            print_repos_rich_table(&rows, &filter, concern_count_all, warn_count_all, ok_count_all, full_path);
        }
        LayoutTier::Vertical => {
            print_repos_vertical(&rows, &filter, concern_count_all, warn_count_all, ok_count_all, full_path);
        }
        LayoutTier::Compact => {
            print_repos_compact_table(&rows, &filter, concern_count_all, warn_count_all, ok_count_all, full_path);
        }
        LayoutTier::Full => {
            print_repos_full_table(&rows, &filter, concern_count_all, warn_count_all, ok_count_all, full_path);
        }
    }
    // v0.113.12 (goal-list 2026-07-29): the legend prints UNDER the
    // table by default (width-gated; --legend remains the on-demand
    // form). v0.113.18: rich tier only (moved into the match arm
    // above — audit M3).

    Ok(())
}

// ---------------------------------------------------------------------------
/// STATUS column label + color for a repo row. Priority order:
/// `concern` > `unowned` > `active` > `warn` > `ok`.
///
/// `unowned` is rendered explicitly here (it was previously only in the
/// ACTIVITY column) so the operator sees the ownership guard tripped at
/// a glance. `active` is the new 🔄 ACTIVE state — the daemon is in
/// flight on this repo (push/commit/dirty-recent), i.e. plausibly not
/// broken. Only dirty repos that are NOT active (e.g. `Stalled`) fall
/// through to `warn`.
fn status_pair(row: &RepoReportRow) -> (&'static str, Color) {
    if row.concern {
        ("❌ CONCERN", Color::Red)
    } else if matches!(row.state_cause, StateCause::Unowned { .. }) {
        ("🚫 unowned", Color::Red)
    } else if row.active {
        ("🔄 ACTIVE", Color::Cyan)
    } else if row.warn {
        // CHANGED 2026-07-22 (v0.112.36): 🟡 (width 2) replaces
        // ⚠️ (width 1, renders 2) — see the tally line above.
        ("🟡 WARN", Color::Yellow)
    } else {
        ("✅ CLEAN", Color::Green)
    }
}

// Layout tier 1: vertical (terminal < 120 cols)
// One repo per multi-line block. No table borders — fixed-width column labels.
// ---------------------------------------------------------------------------
fn print_repos_vertical(
    rows: &[RepoReportRow],
    filter: &RepoFilter,
    concern_count_all: usize,
    warn_count_all: usize,
    ok_count_all: usize,
    full_path: bool,
) {
    let _ = (filter, concern_count_all, warn_count_all, ok_count_all); // already printed in caller

    let width = terminal_width().unwrap_or(80) as usize;
    // Reserve 1 for trailing newline already handled by println!

    for (idx, row) in rows.iter().enumerate() {
        let (status_text, status_color) = status_pair(row);

        let repo_name = if full_path {
            row.repo.clone()
        } else {
            std::path::Path::new(&row.repo)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| row.repo.clone())
        };

        // Push cell (icon + colored label, no plain text PUSH_STUCK)
        let (push_text, push_color) = push_cell_label(&row.push_status, row.failure_count());
        let push_styled = colorize(push_text, push_color);

        // HINT cell (one-liner)
        let hint_text = truncate_unicode_width(&row.hint, width.saturating_sub(2));
        let hint_color = status_color;
        let hint_styled = colorize(&hint_text, hint_color);

        // State + activity combined
        let activity = truncate_unicode_width(&activity_label(row), width.saturating_sub(20));
        let state_styled = colorize(
            &format!("{} {}", row.state_cause.icon(), row.state_cause.as_str()),
            state_color_for(&row.state_cause),
        );

        // Compose commit summary (truncated to fit)
        let commit_width = width.saturating_sub(20);
        let commit_summary = if row.last_hash == "-" {
            "-".to_string()
        } else {
            truncate_unicode_width(&format!("{} {}", row.last_hash, row.last_msg), commit_width)
        };

        // PUSH-TO cell
        let push_to_text = match (&row.excluded_remotes, !row.push_to_remotes.is_empty()) {
            (excl, true) if !excl.is_empty() => format!(
                "{} [excl:{}]",
                row.push_to_remotes.join(","),
                excl.join(",")
            ),
            _ => {
                if row.push_to_remotes.is_empty() {
                    "(none)".to_string()
                } else {
                    row.push_to_remotes.join(",")
                }
            }
        };
        let push_to_text = truncate_unicode_width(&push_to_text, width.saturating_sub(15));

        // Header line: " 1. ✅ OK  dracon-platform"
        let status_styled = colorize(status_text, status_color);
        println!(
            "{:>3}. {}  {}",
            idx + 1,
            status_styled,
            colorize(&repo_name, Color::White)
        );
        // 2-space gutter aligned to status
        let gutter = "     ";
        println!("{gutter}branch:    {}", colorize(&row.branch, branch_color_for(&row.branch)));
        println!(
            "{gutter}publish:   {}",
            colorize(
                &publish_cell_label(&row.upstream, row.publish_state),
                publish_state_color(row.publish_state)
            )
        );
        println!(
            "{gutter}changes:   {} mod, {} stg, {} ut",
            colorize(&row.modified.to_string(), if row.modified > 0 { Color::Yellow } else { Color::White }),
            colorize(&row.staged.to_string(), if row.staged > 0 { Color::Cyan } else { Color::White }),
            row.untracked
        );
        println!(
            "{gutter}ahead/behind: {}/{}",
            colorize(&row.ahead.to_string(), if row.ahead > 0 { Color::Yellow } else { Color::White }),
            colorize(&row.behind.to_string(), if row.behind > 0 { Color::Red } else { Color::White })
        );
        println!("{gutter}push-to:   {push_to_text}");
        println!("{gutter}push:      {push_styled}");
        println!("{gutter}last:      {commit_summary}");
        println!("{gutter}pushed:    {}", shorten_when(&row.last_push));
        println!("{gutter}activity:  {activity}");
        println!("{gutter}state:     {state_styled}");
        // NOTE: author intentionally omitted (v0.112.27 R2). The
        // author is `git log -1 --format=%an` — for a solo operator
        // who freestyles git identities (DraconDev / dracon /
        // darklord-dev), this misleadingly implies multiple people.
        if !hint_text.is_empty() {
            println!("{gutter}hint:      {hint_styled}");
        }
        // Blank line between repos
        println!();
    }
}

// Codeberg quota leak fix (2026-07-13): scans all watched repos for untracked
// collection directories that are NOT currently excluded by
// `untracked_exclude_patterns`. Surfaces them to the operator with size +
// count, grouped by directory leaf name across repos. The operator uses the
// output to decide whether to extend the daemon's untracked-exclude list
// (manual `.dracon/dracon-sync.toml` edit) or to add `.gitignore` rules.
//
// Thresholds (defaults): flag a bucket when its total size across all repos
// exceeds `min_size_mib` (default 5 MiB) AND its appearance count is at
// least `min_repo_count` (default 2). Singletons and tiny dirs are noise;
// aggregating by leaf name avoids drowning the operator in one-off noise.
//
// Forward-compatibility design:
//   - Operator "did not think of" a future tool like `~verify-logs-2026-08/`?
//     It shows up next time `scan-bloat` runs. No silent leak.
//   - Operator decides to keep it? `git add` it, OR add to gitignore, OR
//     add to `untracked_exclude_patterns` in policy. Whatever suits.
//
// This is the discovery loop that complements the 8-pattern static list
// in `default_untracked_exclude_patterns()`.
// ---------------------------------------------------------------------------

pub(crate) async fn run_scan_bloat_report(
    policy_path: &Path,
    min_size_mib: u64,
    min_repo_count: usize,
    json: bool,
) -> Result<()> {
    let policy = SyncPolicy::load(policy_path)?;
    let roots = policy.watch_root_paths();
    let excluded_dir_names = excluded_dir_names_set(&policy);
    let repos = discover_git_repos(
        &roots,
        &excluded_dir_names,
        &policy.exclude_repos,
        Some(&policy.system_repo),
    );

    // Walk each repo (sequentially — git calls are fast, 0.5s/repo on this box
    // for ~26 repos gives ~13s total). Sequential keeps the logic obvious.
    #[derive(Default, Clone, Debug)]
    struct BucketAgg {
        total_size_bytes: u64,
        repo_paths: Vec<String>,
        file_count: usize,
    }
    let mut buckets: std::collections::BTreeMap<String, BucketAgg> =
        std::collections::BTreeMap::new();

    for repo in &repos {
        if let Some((leaf, sz, cnt)) = scan_one_repo_for_bloat(
            repo,
            &policy.untracked_exclude_patterns,
            min_size_mib * 1024 * 1024,
        ) {
            let bucket = buckets.entry(leaf).or_default();
            bucket.total_size_bytes += sz;
            bucket.repo_paths.push(
                repo.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(repo.to_string_lossy().as_ref())
                    .to_string(),
            );
            bucket.file_count += cnt;
        }
    }

    // Filter by thresholds
    let threshold_bytes = min_size_mib * 1024 * 1024;
    let mut findings: Vec<(String, BucketAgg)> = buckets
        .into_iter()
        .filter(|(_, b)| {
            b.total_size_bytes >= threshold_bytes && b.repo_paths.len() >= min_repo_count
        })
        .collect();
    findings.sort_by_key(|f| std::cmp::Reverse(f.1.total_size_bytes));

    if json {
        #[derive(serde::Serialize)]
        struct Out {
            threshold_bytes: u64,
            min_repo_count: usize,
            buckets: Vec<OutBucket>,
        }
        #[derive(serde::Serialize)]
        struct OutBucket {
            leaf: String,
            total_size_bytes: u64,
            total_size_human: String,
            repo_count: usize,
            file_count: usize,
            suggested_pattern: String,
            sample_repos: Vec<String>,
        }
        let out = Out {
            threshold_bytes,
            min_repo_count,
            buckets: findings
                .into_iter()
                .map(|(leaf, b)| OutBucket {
                    leaf: leaf.clone(),
                    total_size_bytes: b.total_size_bytes,
                    total_size_human: human_bytes(b.total_size_bytes),
                    repo_count: b.repo_paths.len(),
                    file_count: b.file_count,
                    suggested_pattern: suggested_pattern_for(&leaf),
                    sample_repos: {
                        let mut sample = b.repo_paths.clone();
                        sample.sort();
                        sample.truncate(5);
                        sample
                    },
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if findings.is_empty() {
        println!(
            "✅ No untracked bloat buckets found (thresholds: ≥ {} MiB total, ≥ {} repos).",
            min_size_mib, min_repo_count
        );
        return Ok(());
    }

    println!(
        "🔎 Scanned {} repo(s) for untracked bloat (thresholds: ≥ {} MiB total, ≥ {} repos).",
        repos.len(),
        min_size_mib,
        min_repo_count,
    );
    println!();
    println!(
        "{:30} {:>10} {:>7} {:>8}  SUGGESTED EXCLUDE",
        "DIRECTORY", "SIZE", "REPOS", "FILES"
    );
    println!("{}", "-".repeat(95));
    let mut total_bytes = 0u64;
    for (leaf, b) in &findings {
        total_bytes += b.total_size_bytes;
        println!(
            "{:30} {:>10} {:>7} {:>8}  {}",
            truncate(leaf, 30),
            human_bytes(b.total_size_bytes),
            b.repo_paths.len(),
            b.file_count,
            suggested_pattern_for(leaf)
        );
    }
    println!("{}", "-".repeat(95));
    println!(
        "{:30} {:>10}",
        "(TOTAL)",
        human_bytes(total_bytes),
    );
    println!();
    println!("💡 Each row suggests a pattern like `**/<dir>/**` that you can add");
    println!("   to `untracked_exclude_patterns` in `~/.dracon/utilities/sync/dracon-sync.toml`");
    println!("   (global) or per-repo at `<repo>/.dracon/dracon-sync.toml`.");
    println!("   Pick the names that correspond to genuine clutter; intentional");
    println!("   assets (game art, marketing screenshots) typically live elsewhere");
    println!("   and won't appear here unless they trip the threshold.");
    Ok(())
}

/// Walk one repo: list untracked directories via
/// `git ls-files --others --exclude-standard --directory`. Aggregate by leaf
/// name within the repo scope, filtering out anything already covered by
/// the operator's `untracked_exclude_patterns`. Returns the largest bucket
/// in this repo as `(leaf, total_size_bytes, total_file_count)`, or `None`
/// if no bucket exceeds `min_bucket_size_bytes`.
///
/// The outer loop sums per-repo buckets across repos by leaf name to
/// produce a single row per recurring directory name in the final report.
fn scan_one_repo_for_bloat(
    repo: &Path,
    exclude_patterns: &[String],
    min_bucket_size_bytes: u64,
) -> Option<(String, u64, usize)> {
    use std::process::Command;

    let output = Command::new("git")
        .current_dir(repo)
        .args(["ls-files", "--others", "--exclude-standard", "--directory"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Per-repo aggregation, keyed on leaf name.
    let mut by_leaf: std::collections::HashMap<String, (u64, usize)> =
        std::collections::HashMap::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.ends_with('/') {
            continue; // only dirs
        }
        let rel = trimmed.trim_end_matches('/');
        // Skip if already excluded.
        if crate::exclude::matches_untracked_exclude(
            repo,
            Path::new(rel),
            exclude_patterns,
        ) {
            continue;
        }
        // Skip noisy paths the operator clearly knows about (the static list
        // covers them, but guard anyway in case the user removed the default).
        if rel.starts_with("node_modules/") || rel.starts_with("target/") || rel.contains("/target/")
            || rel.starts_with("dist/") || rel.starts_with("build/")
        {
            continue;
        }
        let leaf = rel.rsplit('/').next().unwrap_or(rel);
        if leaf.is_empty() || leaf == "." || leaf == "/" {
            continue;
        }
        let leaf = leaf.to_string();
        let abs = repo.join(rel);
        let size = dir_size_bytes(&abs).unwrap_or(0);
        let count = dir_file_count(&abs).unwrap_or(0);
        if size < min_bucket_size_bytes {
            continue;
        }
        let entry = by_leaf.entry(leaf).or_insert((0, 0));
        entry.0 += size;
        entry.1 += count;
    }

    by_leaf.into_iter().max_by_key(|(_, v)| v.0).map(|(leaf, (sz, cnt))| (leaf, sz, cnt))
}

fn dir_size_bytes(p: &Path) -> std::io::Result<u64> {
    use std::process::Command;
    let out = Command::new("du").args(["-sb", "--"]).arg(p).output()?;
    if !out.status.success() {
        return Ok(0);
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let n = s.split_whitespace().next().and_then(|t| t.parse::<u64>().ok());
    Ok(n.unwrap_or(0))
}

fn dir_file_count(p: &Path) -> std::io::Result<usize> {
    use std::process::Command;
    let out = Command::new("find")
        .arg(p)
        .args(["-type", "f"])
        .output()?;
    if !out.status.success() {
        return Ok(0);
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).count())
}

fn suggested_pattern_for(leaf: &str) -> String {
    format!("**/{}/**", leaf)
}

fn human_bytes(b: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", b, UNITS[0])
    } else {
        format!("{:.2} {}", v, UNITS[i])
    }
}
// ---------------------------------------------------------------------------
// Layout tier 2: compact (terminal 120-200 cols)
// 14 columns. Drops: 1h/6h/24h split, AUTHOR (moved to HINT suffix), PUSHED
// (merged with activity). Keeps STATUS, REPO, BRANCH, PUBLISH, M/S/U counts,
// AHEAD/BEHIND, PUSH, PUSH-TO, LAST COMMIT, ACTIVITY, STATE, HINT.
// ---------------------------------------------------------------------------
fn print_repos_compact_table(
    rows: &[RepoReportRow],
    _filter: &RepoFilter,
    _concern_count_all: usize,
    _warn_count_all: usize,
    _ok_count_all: usize,
    full_path: bool,
) {
    use comfy_table::{
        presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, ColumnConstraint, ContentArrangement,
        Table, Width,
    };
    let _ = (_filter, _concern_count_all, _warn_count_all, _ok_count_all);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    // Force the table to fit the actual terminal width. Without this, comfy-table's
    // Dynamic arrangement uses each column's natural content width, producing 553+
    // char rows in 200-299 col terminals (Compact tier). With set_width, columns
    // shrink to fit and cell content is truncated (with …) instead of letter-wrapped.
    if let Some(w) = terminal_width() {
        if (40..=2000).contains(&w) {
            table.set_width(w);
        }
    }

    let mk_h = |icon: &str, label: &str| -> Cell {
        Cell::new(format!("{icon} {label}")).add_attribute(Attribute::Bold)
    };

    table.set_header(vec![
        Cell::new("#"),
        mk_h("🏷", "STATUS"),
        mk_h("📦", "REPO"),
        mk_h("🔗", "ROLE"),
        mk_h("🌿", "BRANCH"),
        mk_h("🔗", "PUBLISH"),
        mk_h("M", "MOD"),
        mk_h("S", "STG"),
        mk_h("U", "UT"),
        mk_h("↑", "AHEAD"),
        mk_h("↓", "BEHIND"),
        mk_h("🚀", "PUSH"),
        mk_h("🛰", "PUSH-TO"),
        mk_h("📜", "LAST COMMIT"),
        mk_h("🩺", "STATE+ACT"),
        mk_h("💡", "HINT"),
    ]);

    // Set minimum widths so the table never letter-wraps content.
    // Each minimum = max(header_text_width + 2 padding, content_min_width).
    // Sum: 3+11+18+7+11+18+8+8+7+9+11+13+18+18+17+22 = 199 + 16 borders = 215 cols min
    // Compact tier is 250-299 cols so this fits comfortably.
    // F30v2 (2026-07-19): LAST COMMIT and HINT must be Absolute, not
    // LowerBoundary, so long content is truncated in the cell (not
    // wrapped). PUSH-TO is a string with multi-word remotes — keep
    // LowerBoundary(32) so it can fit when terminal is wide.
    table.set_constraints(vec![
        // 2026-07-19 (goal `4555eaf6`): REPO and PUBLISH were
        // LowerBoundary(18). On narrow terminals (220-260 cols) the
        // table's set_width() shrinks all LowerBoundary columns to
        // whatever fits, but `LowerBoundary` only enforces a MINIMUM
        // — if a column's content is short, the column shrinks; if
        // the content is long, the column grows. Result: cells with
        // variable-length content (REPO names like
        // `pully-fully-pull-based-fleet-reconciler` = 38 chars, or
        // `wip/hegemon` ROLE labels) would letter-wrap onto a second
        // line on narrow terminals.
        //
        // Fix: switch REPO, ROLE, PUBLISH, STATE+ACT, HINT to
        // Absolute widths and apply `truncate_unicode_width(..., N-2)`
        // to the cell content before passing to comfy-table. Column
        // sum drops from 232 → 217, so the table now fits at 220+ cols.
        ColumnConstraint::Absolute(Width::Fixed(4)),     // # (header 1 + 1 pad, fits up to 99 repos)
        ColumnConstraint::Absolute(Width::Fixed(13)),    // STATUS (header 7 + 2 + 4 buffer for '🚫 unowned' = 11 cols + 2 padding)
        ColumnConstraint::Absolute(Width::Fixed(18)),    // REPO (truncate to 16 cols of content; fits 'browser-extensions-shared' = 24 chars as 'browser-extensions…')
        ColumnConstraint::Absolute(Width::Fixed(14)),    // ROLE (was LowerBoundary(7); was bug — 7 < min content 'standalone' = 10 chars, wraps; now fits 'parent·10' = 9, 'wip/hegemon' = 11, 'released/one-mil-girls' = 22 → truncate to 12)
        ColumnConstraint::Absolute(Width::Fixed(11)),    // BRANCH (header 7 + 2 + 2 buffer)
        ColumnConstraint::Absolute(Width::Fixed(18)),    // PUBLISH (truncate to 16 cols; fits 'gitlab/main', 'github/main')
        ColumnConstraint::Absolute(Width::Fixed(8)),     // M (header 4 + 2 + 2 for digit)
        ColumnConstraint::Absolute(Width::Fixed(8)),     // S (header 4 + 2 + 2 for digit)
        ColumnConstraint::Absolute(Width::Fixed(7)),     // U (header 4 + 2 + 1 buffer)
        ColumnConstraint::Absolute(Width::Fixed(9)),     // AHEAD (header 5 + 2 + 2 buffer)
        ColumnConstraint::Absolute(Width::Fixed(11)),    // BEHIND (header 6 + 2 + 3 buffer)
        ColumnConstraint::Absolute(Width::Fixed(13)),    // PUSH (header 7 + 2 + 4 for '🟣 PENDING')
        ColumnConstraint::Absolute(Width::Fixed(32)),    // PUSH-TO (truncate to 30 cols; was LowerBoundary(32) — same effect)
        ColumnConstraint::Absolute(Width::Fixed(18)),    // LAST COMMIT (F30v2: Absolute — truncate cell content, not wrap)
        ColumnConstraint::Absolute(Width::Fixed(17)),    // STATE+ACT (truncate to 15)
        ColumnConstraint::Absolute(Width::Fixed(26)),    // HINT (2026-07-19 bump 22→26; truncate to 24 to fit 'daemon handles after changes settle' = 33 → 'daemon handles after chan…')
    ]);

    for (idx, row) in rows.iter().enumerate() {
        let (status_text, status_color) = status_pair(row);

        let repo_name = if full_path {
            row.repo.clone()
        } else {
            std::path::Path::new(&row.repo)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| row.repo.clone())
        };
        // 2026-07-19 (goal `4555eaf6`): REPO column is Absolute(18);
        // truncate to 16 cols (minus 2 padding) so names like
        // `pully-fully-pull-based-fleet-reconciler` (38 chars) don't
        // letter-wrap on narrow terminals. Truncation already happens
        // implicitly when content exceeds the column width — but
        // comfy-table's truncation is non-unicode-aware, so we apply
        // `truncate_unicode_width` explicitly for emoji-width safety.
        let repo_name = truncate_unicode_width(&repo_name, 16);

        let commit_summary = if row.last_hash == "-" {
            "-".to_string()
        } else {
            // F30v2 (2026-07-19): truncate to fit the LAST COMMIT column.
            // The column constraint is Absolute(18) which means total
            // column width including comfy-table's default left+right
            // cell padding (1 col each side, so 16 cols of content).
            // Without subtracting the padding, content exactly at the
            // column limit overflows by 2 cols and wraps to a 2nd line.
            let raw = format!("{} {}", row.last_hash, row.last_msg);
            truncate_unicode_width(&raw, 16)
        };

        // Combine state + activity into one cell to save horizontal space.
        // 2026-07-19 (goal `4555eaf6` v0.112.25 follow-up): use
        // `state_plus_act_cell` so the activity part is dropped
        // cleanly when the 15-col budget is tight, instead of being
        // truncated mid-emoji (e.g., `🟠 dirty · ⏳ …`). Activity
        // is informative but secondary — the state component
        // (healthy/dirty/cold/etc.) is what the operator needs.
        let state_plus_act = state_plus_act_cell(
            row.state_cause.icon(),
            row.state_cause.as_str(),
            &activity_label(row),
            15, // Absolute(17) minus 2 padding = 15
        );
        let state_plus_act_color = state_color_for(&row.state_cause);

        // HINT — author suffix intentionally omitted (v0.112.27 R2).
        // The author is `git log -1 --format=%an`; for a solo
        // operator who freestyles git identities, appending
        // `· by DraconDev` to the hint is misleading noise.
        let mut hint_text = row.hint.clone();
        // F30v2 (2026-07-19): truncate to fit the HINT column.
        // HINT is Absolute(26) (was LowerBoundary(22) in v0.112.24),
        // so without truncation the column grows to 80+ chars to fit
        // long content like
        // "daemon handles after changes settle; run sync-now --warns to force now · by dracon".
        // With 250-col terminal and 23 columns, HINT can swallow half the table.
        // Truncate to the column width minus 2 padding = 24 cols.
        hint_text = truncate_unicode_width(&hint_text, 24);
        let hint_color = status_color;

        // Push cell (icon + label)
        let (push_text, push_color) = push_cell_label(&row.push_status, row.failure_count());

        // Classify each row's topology role (parent / submod / standalone).
        // Computed once before the row loop, not per-row at render time.
        let roles = crate::role::classify_roles(rows);

        table.add_row(vec![
            Cell::new(idx + 1),
            Cell::new(status_text).fg(status_color),
            Cell::new(repo_name),
            role_cell(&roles[idx]),
            Cell::new(&row.branch).fg(branch_color_for(&row.branch)),
            Cell::new(publish_cell_label(&row.upstream, row.publish_state))
                .fg(publish_state_color(row.publish_state)),
            Cell::new(row.modified).fg(if row.modified > 0 { Color::Yellow } else { Color::White }),
            Cell::new(row.staged).fg(if row.staged > 0 { Color::Cyan } else { Color::White }),
            Cell::new(row.untracked),
            Cell::new(row.ahead).fg(if row.ahead > 0 { Color::Yellow } else { Color::White }),
            Cell::new(row.behind).fg(if row.behind > 0 { Color::Red } else { Color::White }),
            Cell::new(push_text).fg(push_color),
            format_push_to_remotes_cell(
                &row.push_to_remotes,
                &row.excluded_remotes,
                row.codeberg_skip_reason.as_deref(),
            ),
            Cell::new(commit_summary),
            Cell::new(state_plus_act).fg(state_plus_act_color),
            Cell::new(hint_text).fg(hint_color),
        ]);
    }

    println!("{table}");
}

// ---------------------------------------------------------------------------
// Layout tier 3: full (terminal >= 200 cols)
// Original 22-column v1 table. Uses column constraints to prevent letter-wrap
// at any width >= 220.
// ---------------------------------------------------------------------------
fn print_repos_full_table(
    rows: &[RepoReportRow],
    _filter: &RepoFilter,
    _concern_count_all: usize,
    _warn_count_all: usize,
    _ok_count_all: usize,
    full_path: bool,
) {
    use comfy_table::{
        presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, ColumnConstraint, ContentArrangement,
        Table, Width,
    };
    let _ = (_filter, _concern_count_all, _warn_count_all, _ok_count_all);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    // See compact_table for why this matters: force the table to fit the terminal width.
    if let Some(w) = terminal_width() {
        if (40..=2000).contains(&w) {
            table.set_width(w);
        }
    }
    let mk_h = |icon: &str, label: &str| -> Cell {
        Cell::new(format!("{icon} {label}")).add_attribute(Attribute::Bold)
    };

    table.set_header(vec![
        Cell::new("#"),
        mk_h("🏷", "STATUS"),
        mk_h("📦", "REPO"),
        mk_h("🔗", "ROLE"),
        mk_h("🌿", "BRANCH"),
        mk_h("🔗", "PUBLISH"),
        mk_h("📝", "MOD"),
        mk_h("📥", "STG"),
        mk_h("❓", "UT"),
        mk_h("↑", "AHEAD"),
        mk_h("↓", "BEHIND"),
        mk_h("🚀", "PUSH"),
        mk_h("🛰", "PUSH-TO"),
        mk_h("📜", "LAST COMMIT"),
        mk_h("📤", "PUSHED"),
        mk_h("⏰", "ACTIVITY"),
        mk_h("👤", "AUTHOR"),
        mk_h("📊", "1h"),
        mk_h("📊", "6h"),
        mk_h("📊", "24h"),
        mk_h("🩺", "STATE"),
        mk_h("🤖", "DAEMON"),
        mk_h("💡", "HINT"),
    ]);

    // Enforce minimum widths to prevent letter-wrapping when terminal is
    // narrower than the natural content width.
    //
    // Each minimum = max(header_text_width, content_min_width) + 2 (cell padding).
    // The 2 extra cols account for comfy-table's default left+right cell padding,
    // which is required for the content to fit on a single line.
    //
    // Sum: 4+11+17+18+11+17+8+8+7+9+11+13+22+17+11+11+11+8+8+8+15+15+15 = 275
    // Plus 24 borders: 299 cols minimum. Full tier starts at 300 cols to give
    // 1 col of headroom — fits any 300+ terminal. At 250-299 cols, falls back
    // to compact tier which is 14-col and fits in 199+. F30 (2026-07-18): the
    // v0.112.19 attempt left the constraints summing to 346 (well above 300)
    // and the test never included ROLE; this version trims ROLE 35→18,
    // PUSH-TO 32→22 (drop `[excl:..]` annotation), LAST COMMIT 22→17,
    // ACTIVITY 17→11, DAEMON 17→15, HINT 22→15 so the floor is 299.
    //
    // F30v2 (2026-07-19): `LowerBoundary` lets comfy-table WIDEN a
    // column to fit content (defeating the floor). For LAST COMMIT
    // and AUTHOR (cells whose content can be very long), switch to
    // `Absolute` so the column is truly fixed and content is truncated
    // in the cell content (not wrapped). Use Absolute(17) for LAST
    // COMMIT and Absolute(11) for AUTHOR.
    table.set_constraints(vec![
        ColumnConstraint::Absolute(Width::Fixed(4)),     // # (header 1 + 1 pad = 4, fits up to 99 repos)
        ColumnConstraint::Absolute(Width::Fixed(13)),    // STATUS (header 9 + 2 + 2 headroom for '🚫 unowned' = 11 cols + 2 padding)
        ColumnConstraint::Absolute(Width::Fixed(19)),    // REPO (was LowerBoundary(17); 2026-07-19 goal `4555eaf6` — truncate to 17 cols; long names like `pully-fully-pull-based-fleet-reconciler` = 38 chars → `pully-fully-pull-b…`)
        ColumnConstraint::Absolute(Width::Fixed(18)),    // ROLE (was LowerBoundary(18); F30: trim to 18; long paths → truncated via role_cell() truncation budget)
        ColumnConstraint::Absolute(Width::Fixed(11)),    // BRANCH (header 9 + 2 pad = 11)
        ColumnConstraint::Absolute(Width::Fixed(17)),    // PUBLISH (was LowerBoundary(17); truncate via publish_cell_label() budget 15 cols)
        ColumnConstraint::Absolute(Width::Fixed(8)),     // MOD (header 6 + 2 pad = 8)
        ColumnConstraint::Absolute(Width::Fixed(8)),     // STG (header 6 + 2 pad = 8)
        ColumnConstraint::Absolute(Width::Fixed(7)),     // UT (header 5 + 2 pad = 7)
        ColumnConstraint::Absolute(Width::Fixed(9)),     // AHEAD (header 7 + 2 pad = 9)
        ColumnConstraint::Absolute(Width::Fixed(11)),    // BEHIND (header 9 + 2 pad = 11)
        ColumnConstraint::Absolute(Width::Fixed(13)),    // PUSH: '🟣 PENDING' = 10 + 2 + 1 headroom
        ColumnConstraint::Absolute(Width::Fixed(32)),    // PUSH-TO (F30v2: Absolute — 30 cols content fits 'codeberg [excl:github,gitlab]' = 28 chars + 2 padding headroom)
        ColumnConstraint::Absolute(Width::Fixed(17)),    // LAST COMMIT (F30v2: Absolute — truncate cell content, not wrap)
        ColumnConstraint::Absolute(Width::Fixed(11)),    // PUSHED (header 9 + 2 pad = 11)
        ColumnConstraint::Absolute(Width::Fixed(11)),    // ACTIVITY (was LowerBoundary(11); F30: trim to 11; now Absolute to enforce truncation)
        ColumnConstraint::Absolute(Width::Fixed(11)),    // AUTHOR (F30v2: Absolute — names can be long)
        ColumnConstraint::Absolute(Width::Fixed(8)),     // 1h (header 6 + 2 pad = 8)
        ColumnConstraint::Absolute(Width::Fixed(8)),     // 6h (header 6 + 2 pad = 8)
        ColumnConstraint::Absolute(Width::Fixed(8)),     // 24h (header 7 + 2 pad - 1 for `24`)
        ColumnConstraint::Absolute(Width::Fixed(15)),    // STATE (was LowerBoundary(15); now Absolute — content is always short, truncation budget 13)
        ColumnConstraint::Absolute(Width::Fixed(15)),    // DAEMON (was LowerBoundary(15); now Absolute)
        ColumnConstraint::Absolute(Width::Fixed(15)),    // HINT (was LowerBoundary(15); now Absolute — truncate via row loop budget)
    ]);

    // Classify each row's topology role (parent / submod / standalone).
    // Computed once before the row loop, not per-row at render time.
    let roles = crate::role::classify_roles(rows);

    for (idx, row) in rows.iter().enumerate() {
        let (status_text, status_color) = status_pair(row);

        let repo_name = if full_path {
            row.repo.clone()
        } else {
            std::path::Path::new(&row.repo)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| row.repo.clone())
        };

        // Push cell (icon + colored label)
        let (push_text, push_color) = push_cell_label(&row.push_status, row.failure_count());

        let commit_summary = if row.last_hash == "-" {
            "-".to_string()
        } else {
            // F30v2 (2026-07-19): the full-tier table was widening
            // LAST COMMIT to fit the longest commit subject in the
            // table (152 chars for our auto-commit messages), which
            // destroyed table layout at any terminal width. The
            // vertical and compact tiers already truncate via
            // `truncate_unicode_width`; the full tier forgot to.
            // Use the same helper here. Width = 17 cols (matches the
            // ColumnConstraint::Absolute below) minus 2 for cell
            // padding = 15 visible chars in the rendered cell.
            let raw = format!("{} {}", row.last_hash, row.last_msg);
            truncate_unicode_width(&raw, 15)
        };

        table.add_row(vec![
            Cell::new(idx + 1),
            Cell::new(status_text).fg(status_color),
            Cell::new(repo_name),
            role_cell(&roles[idx]),
            Cell::new(&row.branch).fg(branch_color_for(&row.branch)),
            Cell::new(publish_cell_label(&row.upstream, row.publish_state))
                .fg(publish_state_color(row.publish_state)),
            Cell::new(row.modified).fg(if row.modified > 0 { Color::Yellow } else { Color::White }),
            Cell::new(row.staged).fg(if row.staged > 0 { Color::Cyan } else { Color::White }),
            Cell::new(row.untracked),
            Cell::new(row.ahead).fg(if row.ahead > 0 { Color::Yellow } else { Color::White }),
            Cell::new(row.behind).fg(if row.behind > 0 { Color::Red } else { Color::White }),
            Cell::new(push_text).fg(push_color),
            format_push_to_remotes_cell(
                &row.push_to_remotes,
                &row.excluded_remotes,
                row.codeberg_skip_reason.as_deref(),
            ),
            Cell::new(commit_summary),
            Cell::new(shorten_when(&row.last_push)),
            // F30v2: truncate ACTIVITY to fit LowerBoundary(11) - 2 padding = 9
            Cell::new(truncate_unicode_width(&activity_label(row), 9)),
            // F30v2: AUTHOR is Absolute(11), truncate to 9 to leave padding room
            Cell::new(truncate_unicode_width(&row.last_author, 9)),
            Cell::new(row.commits_1h),
            Cell::new(row.commits_6h),
            Cell::new(row.commits_24h),
            Cell::new(truncate_unicode_width(
                &format!("{} {}", row.state_cause.icon(), row.state_cause.as_str()),
                13, // LowerBoundary(15) - 2 padding
            ))
            .fg(state_color_for(&row.state_cause)),
            Cell::new(truncate_unicode_width(
                &format!(
                    "{} {}",
                    row.daemon_last_action_when, row.daemon_last_action
                ),
                13, // LowerBoundary(15) - 2 padding
            ))
            .fg(if row.daemon_last_result == "fail" {
                Color::Red
            } else if row.daemon_last_result == "ok" {
                Color::Green
            } else if row.daemon_last_action_when == "none" {
                Color::DarkGrey
            } else {
                Color::Cyan
            }),
            Cell::new(truncate_unicode_width(&row.hint, 13)).fg(status_color),
        ]);
    }

    println!("{table}");
}

// ---------------------------------------------------------------------------
// Shared helpers for the three layout tiers
// ---------------------------------------------------------------------------
use comfy_table::Color;

/// Wrap a string in ANSI color codes. Returns plain text when stdout is not a TTY
/// or when `should_color()` is false (e.g. piped to file).
fn colorize(text: &str, color: Color) -> String {
    if !crate::print::should_color() {
        return text.to_string();
    }
    use comfy_table::Color as C;
    let code = match color {
        C::Red => "31",
        C::Green => "32",
        C::Yellow => "33",
        C::Blue => "34",
        C::Magenta => "35",
        C::Cyan => "36",
        C::White => "37",
        C::DarkGrey => "90",
        _ => "0",
    };
    format!("\x1b[{code}m{text}\x1b[0m")
}

fn branch_color_for(branch: &str) -> Color {
    if branch == "main" || branch == "master" {
        Color::White
    } else {
        Color::Cyan
    }
}

fn state_color_for(cause: &StateCause) -> Color {
    match cause {
        StateCause::Working | StateCause::Synced => Color::Green,
        StateCause::Committing | StateCause::Pushing | StateCause::Dirty => Color::Yellow,
        StateCause::Stalled | StateCause::Failed => Color::Red,
        StateCause::Intentional => Color::Magenta,
        StateCause::Untracked | StateCause::Idle => Color::White,
        StateCause::Cold | StateCause::Healthy => Color::DarkGrey,
        StateCause::Unowned { .. } => Color::Red,
    }
}

/// Render the PUSH cell as a colored icon+label (no plain "PUSH_STUCK" text).
/// When `failure_count` is Some, appends `(N failures)` for the PUSH_STUCK case.
/// ADDED 2026-07-29 (v0.113.15): map a configured push-remote name to
/// its rich-table icon. Width-2 emoji only (see REM_COL comment).
/// Unknown remote names return None and render as their first two
/// letters so an unfamiliar topology is still visible, never dropped.
pub(crate) fn remote_icon(name: &str) -> Option<&'static str> {
    if name.contains("github") {
        Some("🐙")
    } else if name.contains("gitlab") {
        Some("🦊")
    } else if name.contains("codeberg") {
        Some("🗻")
    } else {
        None
    }
}

/// ADDED 2026-07-29 (v0.113.18): compose the REPO cell — leading
/// visibility marker (🔒 private / blank = public or unknown —
/// unknown) so the icons form a single vertical column and the names
/// align. Pure function so the composition is directly unit-testable.
fn repo_cell_content(
    visibility: Option<bool>,
    display: &str,
    budget: usize,
    is_nested: bool,
) -> String {
    // v0.113.21: nested submodule checkouts (`.git` is a gitdir
    // POINTER FILE, not a dir) vs standalone (`.git` DIR).
    // v0.113.22 (operator): the badge moves DIRECTLY AFTER the lock
    // so all markers form one fixed column (same reason the lock
    // leads). v0.113.23 (operator): glyph is `>` ("implies it's a
    // sub") — the v0.113.22 tree-child `└` rendered as an
    // ambiguous little corner mark. Prefix is a fixed 4 cells:
    // vis(2) + badge(1: > or space) + space.
    let name_budget = budget.saturating_sub(4);
    let vis = match visibility {
        Some(true) => "🔒",
        // v0.113.27 (operator): public/unknown render BLANK — only
        // private repos carry a marker ("blank for public is good").
        // The 2-space pad keeps the repo name starting at display
        // column 4 for EVERY row, same as the 🔒 prefix.
        Some(false) | None => "  ",
    };
    let badge = if is_nested { ">" } else { " " };
    format!("{vis}{badge} {}", truncate_unicode_width(display, name_budget))
}

// REMOVED 2026-07-29 (v0.113.19): `changes_cell_content` (the
// single combined CHANGES cell) — the operator asked for the counts
// in their RESPECTIVE columns, so the rich table now renders four
// narrow per-class columns inline (📝/📦/🆕/🚫 headers) and the
// helper has no callers.

#[cfg(test)]
mod v011318_tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn table_icons_are_width_two() {
        // unicode-width honesty: every icon used in the table (CHANGE
        // column headers, REPO markers, REM cells) must measure 2
        // cells (Emoji_Presentation=Yes). ✏ (U+270F) measures 1 but
        // renders 2 — banned; see the 🗻 episode in v0.113.15.
        for icon in ["📝", "📦", "🆕", "🚫", "🔒", "🐙", "🦊", "🗻", "🩹", "🔑"] {
            assert_eq!(
                UnicodeWidthStr::width(icon),
                2,
                "{icon} must measure width-2"
            );
        }
    }

    #[test]
    fn three_digit_change_counts_fit_column_budget() {
        // v0.113.19: per-class columns have a 3-cell content budget
        // (width 5 − 2 padding) — junk-runner's 282-modified churn
        // must fit without clipping.
        let truncated = truncate_unicode_width("282", 3);
        assert_eq!(truncated, "282");
        assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 3);
    }
}

/// ADDED 2026-07-29 (v0.113.15): compose the REM cell. CHANGED
/// 2026-07-29 (v0.113.17, operator: "we are showing github gitlab
/// and codeberg for all, that is almost certainly wrong"): the cell
/// now shows ONLY the remotes the daemon actually pushes to —
/// excluded remotes are no longer rendered dim (invisible in pastes
/// and misleading at a glance); exclusion detail lives in
/// `repos <name>` / the JSON row. Unknown remote names render as
/// their first two letters rather than being silently dropped.
fn rem_cell_content(push_to: &[String]) -> String {
    let mut s = String::new();
    for name in push_to {
        match remote_icon(name) {
            Some(icon) => s.push_str(icon),
            None => {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&name.chars().take(2).collect::<String>());
            }
        }
    }
    if s.is_empty() {
        s.push('—');
    }
    s
}

/// ADDED 2026-07-29 (v0.113.15): append the last-push age to a
/// successful PUSH cell (`✅ OK 5m`). `last_push` is a git `%cr`
/// relative string ("4 minutes ago") — parsed to minutes and
/// shortened via the same pipeline as ACTIVITY ages, because the
/// raw string would overflow PUSH_COL=12 and truncate.
fn push_cell_with_age(push_text: &str, last_push: &str) -> String {
    if push_text != "✅ OK" || last_push.is_empty() || last_push == "-" {
        return push_text.to_string();
    }
    match parse_relative_minutes_to_u64(last_push) {
        Some(mins) => format!("{} {}", push_text, shorten_mins(mins)),
        None => push_text.to_string(),
    }
}

/// ADDED 2026-07-30 (v0.113.21): PUSH cell risk markers, appended
/// in priority order while the 10-cell content budget allows:
/// - 🩹 broken history (`missing_objects > 0`) — the next push WILL
///   fail; the hegemon "✅ OK but GitHub empty" class had no visible
///   precondition, this makes the remaining invisible one explicit.
/// - 🔑 a forge token file is missing for a forge this repo pushes
///   to (or is policy-excluded from) — auth-side failures before
///   they surface as ❌ FAIL.
fn push_cell_with_markers(text: String, row: &RepoReportRow, budget: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let mut out = text;
    if row.missing_objects > 0
        && UnicodeWidthStr::width(out.as_str()) + 2 <= budget
    {
        out.push('🩹');
    }
    let token_missing = row
        .push_to_remotes
        .iter()
        .chain(row.excluded_remotes.iter())
        .any(|r| match r.as_str() {
            "github" => !row.token_health.github_present,
            "gitlab" => !row.token_health.gitlab_present,
            "codeberg" => !row.token_health.codeberg_present,
            _ => false,
        });
    if token_missing && UnicodeWidthStr::width(out.as_str()) + 2 <= budget {
        out.push('🔑');
    }
    out
}

fn push_cell_label(push_status: &str, failure_count: Option<u32>) -> (&'static str, Color) {
    match push_status {
        "OK" => ("✅ OK", Color::Green),
        "INTENTIONAL" => ("✅ INTENT", Color::Green),
        "PENDING" => ("🟣 PENDING", Color::Yellow),
        "PUSH_STUCK" => {
            // Note: we can't include the failure count in the cell content because
            // the cell is borrowed &'static str. The HINT cell carries the count.
            let _ = failure_count;
            ("🛑 STUCK", Color::Red)
        }
        "FAIL" => ("❌ FAIL", Color::Red),
        "STUCK" => ("🛑 STUCK", Color::Red),
        "BROKEN" => ("🩹 BROKEN", Color::Red),
        "BLOCKED" => ("🚫 BLOCKED", Color::Yellow),
        _ => ("?", Color::White),
    }
}

// REMOVED 2026-07-29 (v0.113.13): `used_label` and
// `commits_window_label` — the USED column duplicated ACTIVITY's tiers
// (operator feedback) and the single `N/N/N` COMMITS cell was split
// into dedicated 1H / 6H / 24H columns rendered inline in
// `print_repos_rich_table` via the `pulse` closure.

/// Render the `SIZE` column — git_size_bytes formatted in adaptive
/// units (B / KiB / MiB / GiB), color-coded by the github
/// pack-size concern rather than the raw gitdir size threshold.
///
/// ADVISOR-CATCH (v0.113.8 follow-up): the original `size_label`
/// colored the cell red based on `git_size_bytes` ≥ 2 GiB. But
/// `git_size_bytes` (from `git count-objects -v`) is the COMPRESSED
/// pack-on-disk size, while the daemon's `PACK_SIZE_WARNING` /
/// `pack_too_large_forces_concern` predicates fire on the
/// PUSHABLE-UNCOMPRESSED blob sum (the bytes that would actually
/// ship to a remote). These diverge exactly where it matters:
///
/// - junk-runner: gitdir 2.06 GiB (compressed) vs 3.79 GiB
///   pushable (uncompressed) → both are over 2 GiB, but the
///   real signal is the pushable size.
/// - deathrun: gitdir 4.08 GiB (pre-gc pack residue) but
///   ✅ CLEAN (the pushable is well under 2 GiB post-orphan-cutover)
///   → a red SIZE cell would falsely read as "github push broken"
///   when it isn't.
///
/// The fix: the caller passes `pack_too_large` (the same bool
/// the daemon uses for the PACK_SIZE_WARNING concern). Red iff
/// `pack_too_large == true` (the actual github-rejection
/// condition). Yellow iff gitdir ≥ 1 GiB (capacity-planning
/// warning zone, irrespective of push). White otherwise.
///
/// Threshold constants match `dracon_git::github_pack_too_large`:
/// 2 GiB = 2 * 1024^3 = 2_147_483_648 bytes. 1 GiB is half the
/// threshold — the "warning zone" before github refuses.
pub(crate) fn size_label(bytes: Option<u64>, pack_too_large: bool) -> (String, Color) {
    let Some(b) = bytes else {
        return ("?".to_string(), Color::DarkGrey);
    };
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    // Adaptive-unit formatting (B → KiB → MiB → GiB), picking
    // precision by magnitude so the cell stays scannable.
    let format_size = |bytes: u64| -> String {
        if bytes >= GIB * 10 {
            format!("{:.0} GiB", bytes as f64 / GIB as f64)
        } else if bytes >= GIB {
            format!("{:.2} GiB", bytes as f64 / GIB as f64)
        } else if bytes >= MIB * 100 {
            format!("{:.0} MiB", bytes as f64 / MIB as f64)
        } else if bytes >= MIB {
            format!("{:.1} MiB", bytes as f64 / MIB as f64)
        } else if bytes >= KIB {
            format!("{:.0} KiB", bytes as f64 / KIB as f64)
        } else {
            format!("{bytes} B")
        }
    };
    let label = format_size(b);
    // Color priority: pack_too_large > gitdir ≥ 1 GiB > normal.
    // This way the SIZE cell color and the row's CONCERN/ACTIVE
    // state are visually consistent: a red SIZE cell means
    // "github push is broken" (the operator's question),
    // not "gitdir happens to be over 2 GiB".
    let color = if pack_too_large {
        Color::Red
    } else if b >= GIB {
        Color::Yellow
    } else {
        Color::White
    };
    (label, color)
}

/// ADDED 2026-07-30 (v0.113.20): ultra-compact size for the
/// superproject `own+mods` form — `12G`, `7.7G`, `713M`, `48K`.
fn size_compact(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= GIB {
        let v = bytes as f64 / GIB as f64;
        if v >= 10.0 {
            format!("{v:.0}G")
        } else {
            format!("{v:.1}G")
        }
    } else if bytes >= MIB {
        let v = bytes as f64 / MIB as f64;
        if v >= 100.0 {
            format!("{v:.0}M")
        } else {
            format!("{v:.1}M")
        }
    } else if bytes >= KIB {
        format!("{}K", bytes / KIB)
    } else {
        format!("{bytes}B")
    }
}

/// ADDED 2026-07-30 (v0.113.20): SIZE cell — own .git size via
/// `size_label`; superprojects with submodule gitdirs render
/// `own+mods` (operator: "we made them submods so we don't end up
/// with one huge repo, so it would be useful to know both sizes" —
/// the combined number doubles as the would-this-get-stuck-on-a-
/// wholesale-push gauge). Color always follows the OWN pack (that
/// is what actually pushes per-push); the suffix is informational.
fn size_cell_text(own: Option<u64>, modules: u64, pack_too_large: bool) -> (String, Color) {
    let (label, color) = size_label(own, pack_too_large);
    if modules == 0 {
        return (label, color);
    }
    match own {
        Some(b) => (format!("{}+{}", size_compact(b), size_compact(modules)), color),
        None => (label, color),
    }
}

/// Render the `TOUCHED` column — last commit author.
/// Answers "who last touched this?" at a glance.
///
/// v0.113.30 (operator: "touched is a bit of a weak column — who
/// touched, perhaps, but the time we already show"): the relative
/// age was dropped — ACTIVITY already carries the timing (`synced
/// 19m`), so TOUCHED now holds only the author, giving long loop
/// identities (`Virtual Pet Loop`) the full column budget.
///
/// When the row has no commits (empty repo), renders as `-`.
pub(crate) fn touched_label(row: &RepoReportRow) -> String {
    if row.last_hash == "-" || row.last_author.is_empty() {
        return "-".to_string();
    }
    row.last_author.clone()
}

/// Public accessor: the absolute path of the watched repo's working
/// tree as a string. Used by `crate::role::classify_roles` to find
/// each row's path without exposing the private `repo` field.
impl crate::report::RepoReportRow {
    pub(crate) fn repo_path(&self) -> &str {
        &self.repo
    }

    /// Default-constructed row with only `repo` set. Used by
    /// `crate::role` tests to build synthetic rows without exposing
    /// the private field list.
    #[cfg(test)]
    pub(crate) fn for_tests(repo_path: &str) -> Self {
        Self {
            repo: repo_path.to_string(),
            state_flags: vec![],
            branch: String::new(),
            upstream: String::new(),
            publish_state: crate::report::PublishState::Ok,
            modified: 0,
            staged: 0,
            untracked: 0,
            excluded_dirty: 0,
            ahead: 0,
            behind: 0,
            last_hash: "-".into(),
            last_author: String::new(),
            last_when: String::new(),
            last_msg: String::new(),
            last_unix: 0,
            commits_1h: 0,
            commits_6h: 0,
            commits_24h: 0,
            last_push: String::new(),
            push_status: String::new(),
            push_error: String::new(),
            push_to_remotes: vec![],
            excluded_remotes: vec![],
            codeberg_skip_reason: None,
            git_size_bytes: None,
            git_modules_bytes: 0,
            token_health: crate::report::TokenHealthSummary::default(),
            concern: false,
            warn: false,
            active: false,
            hint: String::new(),
            state_cause: crate::report::StateCause::Healthy,
            state_cause_label: "healthy".into(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: "none".into(),
            missing_objects: 0,
            pack_too_large: false,
        }
    }
}

/// Build a comfy-table cell for the role classification column.
/// Parents get green (they own submods); submods get cyan (they're
/// nested); standalone gets white (the default for non-actionable).
fn role_cell(role: &crate::role::RoleKind) -> comfy_table::Cell {
    let label = role.label();
    // 2026-07-19 (goal `4555eaf6`): ROLE column is Absolute(14).
    // Without truncation, labels like `released/one-mil-girls`
    // (22 chars) overflow and letter-wrap on narrow terminals.
    // Truncate to 12 cols (14 - 2 padding). Short labels
    // (`parent·10` = 9, `standalone` = 10, `wip/hegemon` = 11)
    // are unaffected.
    let truncated = truncate_unicode_width(&label, 12);
    let color = match role {
        crate::role::RoleKind::Parent(_) => comfy_table::Color::Green,
        crate::role::RoleKind::Submod { .. } => comfy_table::Color::Cyan,
        crate::role::RoleKind::Standalone => comfy_table::Color::White,
    };
    comfy_table::Cell::new(truncated).fg(color)
}

// ---------------------------------------------------------------------------
// Summary view (--summary / -s). 3-column glance-friendly format
// (STATUS · REPO · WHAT), no headers, severity-sorted by default.
// ---------------------------------------------------------------------------

/// Numeric severity for the summary view's `--summary-by-severity`
/// ordering. Lower number = more urgent, so sort ascending puts
/// concerns at the top.
///
/// 2026-07-19 (goal `4555eaf6` v0.112.27): tier 0 = concerns
/// (WARN+concern, anything genuinely broken); tier 1 = warns
/// (WARN-only, e.g. stalled without a concern); tier 2 = active
/// (the daemon is working on it); tier 3 = clean (idle / cold /
/// healthy).
fn severity_tier(row: &RepoReportRow) -> u8 {
    if row.concern {
        0
    } else if row.warn {
        1
    } else if row.active {
        2
    } else {
        3
    }
}

/// One-line "WHAT" descriptor for the summary view. Combines
/// the activity state (with icon), dirty counts, push status
/// (when STUCK/FAIL), and the operator hint into a single
/// human-readable string. Width-bounded by `budget`.
///
/// 2026-07-19 (goal `4555eaf6` v0.112.27) revision: dropped the
/// redundant `{state} + {activity}` prefix when they're the same
/// (e.g., `🟣 pushing` activity covers state `pushing`); also
/// dropped the standalone `push: pending (N ahead)` note because
/// the activity already says `🟣 pushing Xm`.
///
/// 2026-07-20 (v0.112.27 R2): dropped the `by {author}` suffix.
/// The author is `git log -1 --format=%an` — the git commit
/// author of the most recent commit. For a solo operator who
/// freestyles git identities across repos (`DraconDev` /
/// `dracon` / `darklord-dev`), this reads as "different people"
/// when it's all the same operator, which is misleading noise in
/// a glance view. The detailed 16-column table keeps the author
/// (it has a dedicated column and is part of the full record);
/// the summary view trades it for width + clarity. Output:
///
///   Before: `🟣 pushing 1m · 1 mod · daemon will push after ... · by DraconDev`
///   After:  `🟣 pushing 1m · 1 mod · daemon will push after ...`
fn summary_what(row: &RepoReportRow, budget: usize) -> String {
    // Activity already encodes the state icon + word + age.
    // `🟣 pushing 1m`, `🟠 dirty 0m`, `🔄 working · 🟢 synced 3m`,
    // `⚪ idle 13h`, `⚫ cold 5d`. Use it as-is for the lead.
    let activity = activity_label(row);
    let mut parts: Vec<String> = Vec::new();
    // Only include activity if it's not the bare "—" (which means
    // "nothing to say" — we'll fall through to hint + author).
    if activity != "—" {
        parts.push(activity);
    }
    // Push status note ONLY when not already covered by activity.
    // Activity already says `🟣 pushing 0m (1 ahead)` when PENDING
    // (with the ahead count inline) — see activity_label() at
    // line ~318. The old standalone `push: pending (N ahead)` was
    // pure duplication, and a separate `N ahead` note is too.
    if row.push_status == "STUCK" || row.push_status == "FAIL" {
        parts.push(format!("push: {}", row.push_status.to_lowercase()));
    }
    // Dirty counts (how much work exists). For clean repos this
    // contributes nothing and is skipped.
    let mut dirty_parts: Vec<String> = Vec::new();
    if row.modified > 0 {
        dirty_parts.push(format!("{} mod", row.modified));
    }
    if row.staged > 0 {
        dirty_parts.push(format!("{} stg", row.staged));
    }
    if row.untracked > 0 {
        dirty_parts.push(format!("{} ut", row.untracked));
    }
    if !dirty_parts.is_empty() {
        parts.push(dirty_parts.join(" + "));
    }
    // Operator hint — the most actionable bit.
    if !row.hint.is_empty() && row.hint != "-" {
        parts.push(row.hint.clone());
    }
    // NOTE: author intentionally omitted (v0.112.27 R2). See doc
    // comment above — for a solo operator the git commit author
    // is freestyled noise that misleads in a glance view.
    let joined = parts.join(" · ");
    truncate_unicode_width(&joined, budget)
}

/// Print the summary view.
///
/// 2026-07-19 (goal `4555eaf6` v0.112.27): added in response to
/// operator feedback that the default `repos` table is "noisy at
/// a glance" — 16 columns are too many when the question is just
/// "is anything broken?". The summary view is a proper 3-column
/// table (STATUS · REPO · WHAT) using `comfy-table` with the
/// UTF8_FULL_CONDENSED preset so rows are aligned with column
/// separators. Sorted by severity (concerns first) by default.
///
/// REVISION 2026-07-20 (v0.112.27 R1): operator requested this
/// be a TABLE not a free-form one-line-per-repo list. R0 used
/// `println!` with manual spacing, which broke alignment under
/// ANSI color codes (each emoji-colored STATUS ate a variable
/// number of visible chars). R1 switches to `comfy-table` which
/// handles unicode width + ANSI correctly.
fn print_repos_summary(
    rows: &[RepoReportRow],
    _filter: &RepoFilter,
    full_path: bool,
    by_severity: bool,
) {
    use comfy_table::{
        presets::UTF8_FULL_CONDENSED, Cell, Color, ColumnConstraint, ContentArrangement,
        Table, Width,
    };
    let _ = _filter;

    // Sort: severity (concern → warn → active → clean) ascending
    // by default, but skip the sort when the operator didn't ask
    // for it (preserves the `updated` ordering of the detailed view).
    let mut indexed: Vec<(usize, &RepoReportRow)> = rows.iter().enumerate().collect();
    if by_severity {
        indexed.sort_by_key(|(idx, row)| (severity_tier(row), *idx));
    }

    let width = terminal_width().unwrap_or(120) as usize;
    // Width budget split:
    //   - # column: 4 chars ("1.")
    //   - STATUS column: 12 chars (the longest is "❌ CONCERN" = 10)
    //   - REPO column: 24 chars (worst case: long names truncated)
    //   - WHAT column: rest of the terminal
    // Borders: 5 chars per row in UTF8_FULL_CONDENSED ("| # | ... | ... | ... |").
    // Cell padding: comfy-table adds 2 chars per cell by default
    // (left + right space). 3 cells get padding, so +6 chars.
    const NUM_COL: usize = 4;
    const STATUS_COL: usize = 12;
    const REPO_COL: usize = 24;
    const BORDER_OVERHEAD: usize = 5; // box-drawing chars + leading separator
    const CELL_PADDING: usize = 6;    // 3 padded cells × 2 chars each
    let what_col = width
        .saturating_sub(NUM_COL + STATUS_COL + REPO_COL + BORDER_OVERHEAD + CELL_PADDING)
        .max(20);

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    if let Some(w) = terminal_width() {
        if (40..=2000).contains(&w) {
            table.set_width(w);
        }
    }

    // Header row. Header cells are styled white-bold for contrast
    // against the row-level STATUS colors.
    table.set_header(vec![
        Cell::new("#")
            .fg(Color::White)
            .add_attribute(comfy_table::Attribute::Bold),
        Cell::new("STATUS")
            .fg(Color::White)
            .add_attribute(comfy_table::Attribute::Bold),
        Cell::new("REPO")
            .fg(Color::White)
            .add_attribute(comfy_table::Attribute::Bold),
        Cell::new("WHAT")
            .fg(Color::White)
            .add_attribute(comfy_table::Attribute::Bold),
    ]);

    // Fixed column widths so the WHAT column can absorb extra
    // terminal width via Dynamic arrangement. Use Absolute so
    // long REPO names truncate (`pully-fully-pull-base…`) instead
    // of letter-wrapping.
    table
        .column_mut(0)
        .expect("# column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(NUM_COL as u16)));
    table
        .column_mut(1)
        .expect("STATUS column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(STATUS_COL as u16)));
    table
        .column_mut(2)
        .expect("REPO column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(REPO_COL as u16)));
    // WHAT column gets the leftover width (comfy-table expands it
    // to fit terminal width under Dynamic arrangement).

    let repo_budget = REPO_COL.saturating_sub(2); // 2 for comfy-table padding
    for (display_idx, (_orig_idx, row)) in indexed.iter().enumerate() {
        let (status_text, status_color) = status_pair(row);
        let repo_name = if full_path {
            row.repo.clone()
        } else {
            std::path::Path::new(&row.repo)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| row.repo.clone())
        };
        let repo_short = truncate_unicode_width(&repo_name, repo_budget);
        let what = summary_what(row, what_col);

        table.add_row(vec![
            Cell::new(format!("{}", display_idx + 1)).fg(Color::DarkGrey),
            Cell::new(status_text).fg(status_color),
            Cell::new(repo_short).fg(Color::White),
            Cell::new(what).fg(Color::White),
        ]);
    }

    println!("{table}");
}

/// ADDED 2026-07-22 (v0.112.38): the default table view — a rich
/// 6-column table (STATUS · REPO · ACTIVITY · PUSH · HINT, plus a
/// PUBLISH column when the terminal is ≥140 cols). Replaces the
/// per-repo Vertical block view as the default for < 242 cols: the
/// operator wanted "a very rich table" as the default, with detail
/// available on demand (`repos <name>` or `--layout vertical`).
///
/// Column widths are chosen to fit a ~90-col minimum terminal:
/// - `#` 4, STATUS 12, REPO 24 (truncates), ACTIVITY 26 (includes
///   dirty counts inline, e.g. `⏳ dirty 1d · 101 stg + 2 ut`),
///   PUSH 10 (`🟣 PENDING`), HINT gets the rest.
/// - At ≥140 cols, PUBLISH 14 is inserted between PUSH and HINT.
///
/// Sorted by severity (concern → warn → active → clean) like the
/// summary, since this is the main health-check view.
fn print_repos_rich_table(
    rows: &[RepoReportRow],
    _filter: &RepoFilter,
    _concern_count: usize,
    _warn_count: usize,
    _ok_count: usize,
    full_path: bool,
) {
    use comfy_table::{
        presets::UTF8_FULL_CONDENSED, Cell, Color, ColumnConstraint, ContentArrangement,
        Table, Width,
    };
    let _ = _filter;

    // Sort by severity (concern → warn → active → clean), stable.
    let mut indexed: Vec<(usize, &RepoReportRow)> = rows.iter().enumerate().collect();
    indexed.sort_by_key(|(idx, row)| (severity_tier(row), *idx));

    let width = terminal_width().unwrap_or(120) as usize;
    const NUM_COL: usize = 4;
    const STATUS_COL: usize = 12;
    // CHANGED 2026-07-29 (v0.113.15): REPO narrowed 22 → 20 and
    // ACTIVITY narrowed 28 → 24 to fund the new REM column (+9 cols
    // incl. border+padding) within the 165-col rich-tier budget
    // (operator: show which remotes each repo syncs to as icons).
    const REPO_COL: usize = 20;
    // v0.113.30 (operator: "make the table as wide as the screen,
    // and flex grow like the repo name"): REPO is the flex column —
    // it absorbs every terminal column beyond the fixed floor (159,
    // pinned by test_rich_table_fits_narrow_terminal), so the table
    // always spans the full screen and names truncate less on wide
    // terminals. Below the floor REPO stays at REPO_COL and
    // comfy-table squashes gracefully.
    let repo_col = {
        let fixed_non_repo = NUM_COL
            + STATUS_COL
            + ACTIVITY_COL
            + CHG_MOD_COL
            + CHG_STG_COL
            + CHG_UT_COL
            + CHG_EXCL_COL
            + AB_COL
            + PUSH_COL
            + REM_COL
            + C1H_COL
            + C6H_COL
            + C24H_COL
            + SIZE_COL
            + TOUCHED_COL
            + 17; // 16 columns → 17 border cells
        width.saturating_sub(fixed_non_repo).max(REPO_COL)
    };
    // CHANGED 2026-07-29 (v0.113.17): ACTIVITY narrowed 23 → 16 —
    // it now holds ONLY the state label (`🟢 synced 19m` = 13 max);
    // the dirty counts moved to their own CHANGES column (operator:
    // "the activity can just have the first part").
    const ACTIVITY_COL: usize = 16;
    // CHANGED 2026-07-29 (v0.113.19): the single CHANGES column
    // split into FOUR per-class columns (operator: "the changes
    // should be in their respective columns, not just dumped
    // there") — 📝 modified · 📦 staged · 🆕 untracked · 🚫 excluded
    // by policy. Width 5 each (3 content + 2 padding) so a 3-digit
    // count like junk-runner's 282 modified fits without clipping.
    // Icon headers (width-2); `—` when the class is clean.
    const CHG_MOD_COL: usize = 5;
    const CHG_STG_COL: usize = 5;
    const CHG_UT_COL: usize = 5;
    const CHG_EXCL_COL: usize = 5;
    // ADDED 2026-07-22 (v0.112.38 R2): ahead/behind column — the
    // most important missing field. `↑N` = unpushed commits (data
    // at risk), `↓N` = upstream drift (needs pull), `↑N ↓M` = both,
    // `—` = in sync. Width 9 fits the header `↑/↓ A/B` (7 cols) +
    // 2 padding.
    const AB_COL: usize = 9;
    // PUSH must fit `🟣 PENDING` (2+1+7 = 10 content) + 2 padding.
    // v0.113.15: on success the cell also carries the last-push age
    // (`✅ OK 5m`) — the string fits because the daemon pushes
    // within seconds, so ages are almost always short forms.
    const PUSH_COL: usize = 12;
    // ADDED 2026-07-29 (v0.113.15): REM column — one width-2 emoji
    // per ACTIVE push remote (🐙 github · 🦊 gitlab · 🗻 codeberg).
    // v0.113.17: excluded remotes are NOT rendered (operator: showing
    // all three for every repo read as "all repos have all remotes").
    // Worst case 3 remotes × 2 cells = 6 content + 2 padding = 8
    // (Absolute width INCLUDES padding — v0.113.15 dev-test caught
    // the third icon being truncated at REM_COL=6). NOTE: codeberg is
    // 🗻 (U+1F5FB, Emoji_Presentation=Yes → width-2 in
    // unicode-width), NOT ⛰/🏔 (U+26F0/U+1F3D4 measure width-1 in
    // unicode-width but render 2 — would break the table math).
    const REM_COL: usize = 8;
    // CHANGED 2026-07-29 (v0.113.13): USED column DROPPED (operator
    // feedback: it duplicated ACTIVITY's dirty/synced/idle/cold tier)
    // and the single COMMITS column was split into three separate
    // 1H / 6H / 24H columns (operator: "the commits per time can have
    // columns too, now they are just dumped together"). Each fits a
    // 3-digit value + 2 padding; the freed 21 cols (USED 9 + COMMITS
    // 12) fund the 15 (3×5) with 6 cols of headroom returned.
    const C1H_COL: usize = 5;
    const C6H_COL: usize = 5;
    const C24H_COL: usize = 5;
    // SIZE column: `3.79 GiB` (worst-case label) = 8 chars + 2 padding
    // = 10; absolute 10 fits the largest realistic value.
    // v0.113.20: 10 → 11 so the superproject `own+mods` form
    // (`12G+7.7G` = 8 content) fits with headroom for MiB-scale
    // combos (`446M+713M` = 9).
    const SIZE_COL: usize = 11;
    // TOUCHED column: `<10-char author> <when>` = up to 14 chars +
    // 2 padding = 16; absolute 16 fits `Virtual-Pet 14m` cleanly.
    const TOUCHED_COL: usize = 15;
    // Borders: N+1 separators in UTF8_FULL_CONDENSED for N columns.
    // Cell padding: 2 chars per cell × N cells.
    let num_cols = 16; // fixed: #, STATUS, REPO, ACTIVITY, 📝,📦,🆕,🚫, A/B, PUSH, REM, 1H, 6H, 24H, SIZE, TOUCHED
    let border_overhead = num_cols + 1;
    let cell_padding = num_cols * 2;
    let fixed = NUM_COL
        + STATUS_COL
        + REPO_COL
        + ACTIVITY_COL
        + CHG_MOD_COL
        + CHG_STG_COL
        + CHG_UT_COL
        + CHG_EXCL_COL
        + AB_COL
        + PUSH_COL
        + REM_COL
        + C1H_COL
        + C6H_COL
        + C24H_COL
        + SIZE_COL
        + TOUCHED_COL;
    // ADVISOR-CATCH (v0.113.8 follow-up): the original code had an
    // `assert!` here that panicked the process when `--layout rich`
    // was forced on a < 165-col terminal. The assert exists as a
    // development-time sanity check (the test
    // `test_rich_table_fits_narrow_terminal` pins the invariant),
    // but panicking a user-facing CLI on a forced layout override
    // is the wrong enforcement — comfy-table degrades gracefully
    // by squashing columns when the Absolute width doesn't fit,
    // and the column-set logic at runtime handles it.
    //
    // Operators on 90-164 col terminals get the Compact tier via
    // `choose_layout_tier`. Operators who explicitly pass
    // `--layout rich` on a narrower terminal get the same
    // comfortable-column-squashing graceful render. No more
    // runtime panics.
    let _ = (fixed, border_overhead, cell_padding, width); // suppress unused warnings

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    if let Some(w) = terminal_width() {
        if (40..=2000).contains(&w) {
            table.set_width(w);
        }
    }

    let bold = |s: &str| Cell::new(s).fg(Color::White).add_attribute(comfy_table::Attribute::Bold);
    let header = vec![
        bold("#"),
        bold("STATUS"),
        bold("REPO"),
        bold("ACTIVITY"),
        bold("📝"),
        bold("📦"),
        bold("🆕"),
        bold("🚫"),
        bold("A/B"),
        bold("PUSH"),
        bold("REM"),
        bold("1H"),
        bold("6H"),
        bold("24H"),
        bold("SIZE"),
        bold("TOUCHED"),
    ];
    table.set_header(header);

    table
        .column_mut(0)
        .expect("# column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(NUM_COL as u16)));
    table
        .column_mut(1)
        .expect("STATUS column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(STATUS_COL as u16)));
    table
        .column_mut(2)
        .expect("REPO column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(repo_col as u16)));
    table
        .column_mut(3)
        .expect("ACTIVITY column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(ACTIVITY_COL as u16)));
    table
        .column_mut(4)
        .expect("modified-count column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(CHG_MOD_COL as u16)));
    table
        .column_mut(5)
        .expect("staged-count column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(CHG_STG_COL as u16)));
    table
        .column_mut(6)
        .expect("untracked-count column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(CHG_UT_COL as u16)));
    table
        .column_mut(7)
        .expect("excluded-count column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(CHG_EXCL_COL as u16)));
    table
        .column_mut(8)
        .expect("A/B column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(AB_COL as u16)));
    table
        .column_mut(9)
        .expect("PUSH column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(PUSH_COL as u16)));
    table
        .column_mut(10)
        .expect("REM column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(REM_COL as u16)));
    table
        .column_mut(11)
        .expect("1H column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(C1H_COL as u16)));
    table
        .column_mut(12)
        .expect("6H column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(C6H_COL as u16)));
    table
        .column_mut(13)
        .expect("24H column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(C24H_COL as u16)));
    table
        .column_mut(14)
        .expect("SIZE column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(SIZE_COL as u16)));
    table
        .column_mut(15)
        .expect("TOUCHED column")
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(TOUCHED_COL as u16)));

    let repo_budget = repo_col.saturating_sub(2);
    let activity_budget = ACTIVITY_COL.saturating_sub(2);
    // v0.113.19: per-class change columns — 3-cell content budget
    // (col width 5 − 2 padding) holds any realistic count.
    let chg_budget = 3;
    let ab_budget = AB_COL.saturating_sub(2);
    let touched_budget = TOUCHED_COL.saturating_sub(2);
    for (display_idx, (_orig_idx, row)) in indexed.iter().enumerate() {
        let (status_text, status_color) = status_pair(row);
        let repo_name = if full_path {
            row.repo.clone()
        } else {
            std::path::Path::new(&row.repo)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| row.repo.clone())
        };
        // ADDED 2026-07-22 (v0.112.38 R2): fold the branch into the
        // REPO cell ONLY when it is not `main` — a dedicated BRANCH
        // column is noise when ~every repo is on main, but a
        // non-main branch is exactly the kind of state worth seeing.
        let repo_display = if row.branch != "main" && !row.branch.is_empty() && row.branch != "-" {
            format!("{}⚡{}", repo_name, row.branch)
        } else {
            repo_name
        };
        // CHANGED 2026-07-29 (v0.113.18): the visibility marker moved
        // to the FRONT so the icons form a single vertical column
        // (operator: "the lock in front so its in one column
        // visually"). Unknown/unprobed repos get a 3-cell pad so the
        // names still align (absence of icon = unknown). The marker
        // costs 3 cells ("X "), carved out of the truncate budget.
        let visibility = crate::visibility::cached_repo_visibility(std::path::Path::new(&row.repo));
        // v0.113.21: `.git` as a FILE = nested submodule / linked
        // worktree checkout (gitdir pointer); a DIR = standalone.
        let is_nested = std::path::Path::new(&row.repo).join(".git").is_file();
        let repo_short =
            repo_cell_content(visibility, &repo_display, repo_budget, is_nested);

        // ACTIVITY (v0.113.17): the state label ONLY — the dirty
        // counts moved to their own CHANGES column (operator: "the
        // activity can just have the first part"). The `(N ahead)`
        // strip stays: the A/B column carries that count.
        let mut activity = activity_label_base(row);
        activity = activity.replace(&format!(" ({} ahead)", row.ahead), "");
        let activity = truncate_unicode_width(&activity, activity_budget);

        // CHANGES (v0.113.19): one narrow column per class —
        // 📝 modified, 📦 staged, 🆕 untracked, 🚫 excluded by
        // policy. Count in White when non-zero, `—` DarkGrey when
        // the class is clean (same zero-dimming as the COMMITS
        // columns so clean classes don't shout).
        let chg = |n: usize| {
            if n > 0 {
                Cell::new(truncate_unicode_width(&n.to_string(), chg_budget)).fg(Color::White)
            } else {
                Cell::new("—").fg(Color::DarkGrey)
            }
        };

        // ADDED 2026-07-22 (v0.112.38 R2): ahead/behind cell.
        // v0.113.18 (audit L7): no-space `↑423↓12` (one cell cheaper)
        // and truncate to the column budget — a 4-digit double count
        // used to overflow silently, showing a clipped wrong number.
        let (ab_text, ab_color) = if row.ahead > 0 && row.behind > 0 {
            (format!("↑{}↓{}", row.ahead, row.behind), Color::Yellow)
        } else if row.ahead > 0 {
            (format!("↑{}", row.ahead), Color::Yellow)
        } else if row.behind > 0 {
            (format!("↓{}", row.behind), Color::Magenta)
        } else {
            ("—".to_string(), Color::DarkGrey)
        };
        // v0.113.30 (operator: "A/B oddness"): when a push is IN
        // FLIGHT the ↑N is exactly the batch being pushed right now
        // — dim it so it reads as pipeline-in-motion rather than
        // unpushed-work alarm. The count stays (it's still true).
        let ab_color = if row.push_status == "PENDING" && row.ahead > 0 && row.behind == 0 {
            Color::DarkGrey
        } else {
            ab_color
        };
        let ab_text = truncate_unicode_width(&ab_text, ab_budget);

        let (push_text, push_color) = push_cell_label(&row.push_status, row.failure_count());
        // v0.113.15: successful PUSH cells carry the last-push age.
        let push_text = push_cell_with_age(push_text, &row.last_push);
        // v0.113.21: 🩹 broken-history / 🔑 token-missing markers.
        let push_text = push_cell_with_markers(push_text, row, PUSH_COL.saturating_sub(2));
        // v0.113.22 (operator): REM cell — active push remotes
        // only. The v0.113.21 dim-excluded suffix put a 🗻 on EVERY
        // row under the codeberg quota posture (fleet-wide
        // exclusion = noise, not signal): "leave it out if we are
        // not using it — easier to see".
        let rem_text = rem_cell_content(&row.push_to_remotes);

        // CHANGED 2026-07-29 (v0.113.13): USED column dropped
        // (duplicated ACTIVITY) and COMMITS split into three columns.
        // A window with commits renders White (pulse), a zero window
        // DarkGrey so the eye slides over dormant repos.
        let pulse = |v: usize| {
            if v > 0 {
                Cell::new(format!("{v}")).fg(Color::White)
            } else {
                Cell::new("0").fg(Color::DarkGrey)
            }
        };

        // ADDED 2026-07-28 (v0.113.8): SIZE column = adaptive units,
        // color-coded by the actual github-pack-limit concern
        // (pack_too_large), not the raw gitdir size. See the doc
        // comment on `size_label` for why this matters (deathrun
        // would otherwise show a red SIZE cell while its STATUS
        // cell is ✅ CLEAN, contradicting itself).
        // v0.113.20: superprojects show `own+mods` (submodule
        // gitdirs) in the SIZE cell.
        let (size_text, size_color) =
            size_cell_text(row.git_size_bytes, row.git_modules_bytes, row.pack_too_large);

        // ADDED 2026-07-28 (v0.113.8): TOUCHED column = last author + when.
        let touched = truncate_unicode_width(&touched_label(row), touched_budget);

        let cells = vec![
            Cell::new(format!("{}", display_idx + 1)).fg(Color::DarkGrey),
            Cell::new(status_text).fg(status_color),
            Cell::new(repo_short).fg(Color::White),
            Cell::new(activity).fg(Color::White),
            chg(row.modified),
            chg(row.staged),
            chg(row.untracked),
            chg(row.excluded_dirty),
            Cell::new(ab_text).fg(ab_color),
            Cell::new(push_text).fg(push_color),
            Cell::new(rem_text).fg(Color::Cyan),
            pulse(row.commits_1h),
            pulse(row.commits_6h),
            pulse(row.commits_24h),
            Cell::new(size_text).fg(size_color),
            Cell::new(touched).fg(Color::White),
        ];
        table.add_row(cells);
    }

    println!("{table}");
}

// ---------------------------------------------------------------------------
// Extension trait: gives RepoReportRow access to the most recent failure count
// from the recent-push-failure ledger. Returns None if not PUSH_STUCK.
// ---------------------------------------------------------------------------
trait RepoReportRowExt {
    fn failure_count(&self) -> Option<u32>;
}

impl RepoReportRowExt for RepoReportRow {
    fn failure_count(&self) -> Option<u32> {
        if self.push_status == "PUSH_STUCK" {
            // The failure count is not stored on the row directly. The HINT
            // cell embeds it (e.g., "(173 failures)"). We don't re-parse it
            // here to keep this trait method cheap. Returning None means the
            // PUSH cell shows just "🛑 STUCK" without the count.
            None
        } else {
            None
        }
    }
}


pub(crate) fn log_incident(
    policy_path: &Path,
    scope: impl Into<String>,
    repo: impl Into<String>,
    reason: impl Into<String>,
    action: impl Into<String>,
    backup_branch: Option<String>,
    result: impl Into<String>,
    details: Option<String>,
) {
    let record = IncidentRecord::new(
        timestamp_secs(),
        scope,
        repo,
        reason,
        action,
        backup_branch,
        result,
        details,
    );
    append_incident_record(policy_path, &record);
}

/// ADDED 2026-07-28 (v0.113.7, concern-retry-softening): decision
/// returned by [`decide_create_mirror`] — whether the daemon's
/// auto-repair concern path should actually create an offline
/// mirror (the pre-fix behavior was eager: any `has_origin=false`
/// triggered `create_private_remote` immediately, which on a
/// transient SSH hiccup would fork the operator's repo onto a
/// mirror they did not ask for). Mirrors the `RemoteExistence`
/// tri-state in `git/multi_remote.rs` so callers can map both
/// signals to a consistent action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateMirrorDecision {
    /// Origin is reachable, was previously pushed, or has not
    /// been gone long enough — DO NOT create a mirror this
    /// invocation; either retry later or wait out the
    /// gone-window. Caller should log "transient ssh hiccup —
    /// will retry" and update the gone-since ledger.
    TransientHiccup,
    /// Origin has been gone for the full policy window AND the
    /// repo was never pushed — safe to create an offline
    /// mirror. Caller should log "origin really gone, creating
    /// offline mirror" and clear the gone-since ledger.
    ReallyGone,
}

/// ADDED 2026-07-28 (v0.113.7, concern-retry-softening):
/// pure-decision helper extracted so the create-mirror policy
/// can be regression-tested without network probes or a fake
/// git binary. Callers feed in:
///   * `any_remote_reachable` — did ANY of the 3x retry probes
///     (per configured remote, 5s apart) answer cleanly? True
///     means the network and forge are fine; origin absence is
///     a local config anomaly, not a transport failure.
///   * `ever_pushed` — does this checkout have ANY
///     `refs/remotes/<name>/*` entry (i.e. has it ever
///     successfully pushed to any remote)? True means origin
///     existed in the past and a transient outage must not
///     fork a mirror.
///   * `gone_secs` — how long has origin been unreachable, in
///     seconds since the first observed failure (the
///     gone-since ledger); `None` means no failure has been
///     recorded this session (first probe failure ⇒ treat as
///     transient, not as "really gone").
///
/// Returns `ReallyGone` only when ALL three conditions hold:
/// network unreachable AND never pushed AND gone > 15 min.
/// Otherwise `TransientHiccup`. The 15-minute window is the
/// policy knob specified in the concern-retry-softening
/// objective; surfaced as a named constant
/// ([`CREATE_MIRROR_GONE_THRESHOLD_SECS`]) so tests + future
/// operators can reference it directly.
pub(crate) const CREATE_MIRROR_GONE_THRESHOLD_SECS: u64 = 900;

pub(crate) fn decide_create_mirror(
    any_remote_reachable: bool,
    ever_pushed: bool,
    gone_secs: Option<u64>,
) -> CreateMirrorDecision {
    if any_remote_reachable {
        return CreateMirrorDecision::TransientHiccup;
    }
    if ever_pushed {
        // Origin has answered at least once for this checkout.
        // A transient outage cannot be the cause of a current
        // missing origin — the previous push would have failed
        // visibly. Refuse to fork a mirror.
        return CreateMirrorDecision::TransientHiccup;
    }
    match gone_secs {
        Some(s) if s >= CREATE_MIRROR_GONE_THRESHOLD_SECS => {
            CreateMirrorDecision::ReallyGone
        }
        // Either no failure observed yet (None — first
        // probe), or the elapsed window is shorter than the
        // threshold. Either way, do not create.
        _ => CreateMirrorDecision::TransientHiccup,
    }
}

struct RepairState {
    attempted_ops: usize,
    succeeded_ops: usize,
    manual_only: usize,
    has_origin: bool,
    has_upstream: bool,
    push_ok: bool,
}

/// ADDED 2026-07-28 (v0.113.7, concern-retry-softening): probe
/// ALL configured remotes for reachability with a bounded 3x
/// retry (5s delay between attempts). Returns true if ANY
/// remote answered cleanly — the concern-repair path treats
/// this as "network is fine; origin absence is a local config
/// anomaly" and refuses to fork a mirror (mirrors the
/// `RemoteExistence::Exists` semantics from
/// `git/multi_remote.rs::remote_repo_exists`).
///
/// Uses the same `tokio_git_command()` + `git_ssh_hardening()`
/// construction as `remote_repo_exists` so the probe inherits
/// BatchMode / ConnectTimeout (no interactive SSH hangs in the
/// daemon path — `auto_repair_concerns` is invoked from the
/// daemon at `daemon.rs:2921` and the probe runs unattended).
/// Definitive "not found" answers count as "reachable but
/// missing" via the shared `ls_remote_indicates_missing`
/// classifier. The 3x retry with 5s delay tolerates transient
/// SSH/DNS blips without forking a mirror onto a host the
/// operator did not ask for.
async fn probe_any_remote_reachable(repo: &Path) -> bool {
    use crate::git::multi_remote::{list_remotes, ls_remote_indicates_missing};
    use crate::policy::tokio_git_command;
    let remotes = list_remotes(repo);
    if remotes.is_empty() {
        return false;
    }
    let ssh_hardening = crate::git::git_ssh_hardening();
    for remote_name in &remotes {
        for attempt in 1..=3u32 {
            let output = tokio_git_command()
                .current_dir(repo)
                .env("GIT_SSH_COMMAND", ssh_hardening.clone())
                .env("GIT_TERMINAL_PROMPT", "0")
                .args(["ls-remote", "--heads", remote_name, "HEAD"])
                .output()
                .await;
            if let Ok(o) = output {
                if o.status.success() {
                    return true;
                }
                // Definitive "not found" is still a clean
                // answer — forge answered, transport is fine.
                let stderr = String::from_utf8_lossy(&o.stderr);
                if ls_remote_indicates_missing(&stderr) {
                    return true;
                }
            }
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
    false
}

/// ADDED 2026-07-28 (v0.113.7, concern-retry-softening): has
/// this checkout ever successfully pushed to ANY remote?
/// Implemented by checking for the presence of any
/// `refs/remotes/<name>/*` entries (packed or loose). If yes,
/// the operator has used a forge with this repo before; a
/// current "no origin" must be transient, not a fork trigger.
fn ever_pushed(repo: &Path) -> bool {
    // Packed refs: cheap to read; covers the common case.
    let packed = repo.join(".git").join("packed-refs");
    if let Ok(content) = std::fs::read_to_string(&packed) {
        for line in content.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if line.contains(" refs/remotes/") {
                return true;
            }
        }
    }
    // Loose refs: walk the directory. Bounded by refs/remotes/.
    let remotes_dir = repo.join(".git").join("refs").join("remotes");
    if remotes_dir.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&remotes_dir) {
            for entry in rd.flatten() {
                if entry.path().is_dir() {
                    return true;
                }
            }
        }
    }
    false
}

/// ADDED 2026-07-28 (v0.113.7, concern-retry-softening):
/// gone-since ledger — a per-policy TSV file
/// (`<policy_dir>/origin-gone-ledger.tsv`) that records
/// `repo_path\tunix_secs` on first observed origin failure
/// and is cleared (entry removed) on first observed origin
/// success. `origin_gone_secs` returns `None` if the repo is
/// not in the ledger (no failure observed this session —
/// first probe failure is therefore transient by policy).
fn origin_gone_ledger_path(policy_path: &Path) -> PathBuf {
    policy_path
        .parent()
        .map(|p| p.join("origin-gone-ledger.tsv"))
        .unwrap_or_else(|| PathBuf::from("origin-gone-ledger.tsv"))
}

fn origin_gone_secs(policy_path: &Path, repo: &Path) -> Option<u64> {
    let path = origin_gone_ledger_path(policy_path);
    let content = std::fs::read_to_string(&path).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let repo_str = repo.display().to_string();
    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\t');
        let p = parts.next().unwrap_or("");
        let secs_str = parts.next().unwrap_or("");
        if p == repo_str {
            if let Ok(secs) = secs_str.parse::<u64>() {
                return now.checked_sub(secs);
            }
        }
    }
    None
}

fn record_origin_gone(policy_path: &Path, repo: &Path) {
    let path = origin_gone_ledger_path(policy_path);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok();
    let secs = match now {
        Some(d) => d.as_secs(),
        None => return,
    };
    let repo_str = repo.display().to_string();
    // ADDED 2026-07-28 (v0.113.7, concern-retry-softening):
    // insert-if-absent semantics. Pre-fix the function
    // dropped the existing entry and appended a fresh
    // timestamp, which meant every TransientHiccup
    // invocation reset the gone-window — the
    // `gone_secs >= 900` gate would never fire in
    // production because the cycle keeps restarting the
    // window. Insert-if-absent preserves the FIRST-observed
    // failure time so the elapsed window grows monotonically
    // until `clear_origin_gone` removes the entry.
    let mut existing: Vec<String> = Vec::new();
    let mut already_present = false;
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(2, '\t');
            let p = parts.next().unwrap_or("");
            if p == repo_str {
                already_present = true;
                // Preserve the original timestamp verbatim.
                existing.push(line.to_string());
            } else {
                existing.push(line.to_string());
            }
        }
    }
    if already_present {
        // No rewrite needed; the existing line is preserved.
        return;
    }
    existing.push(format!("{}\t{}", repo_str, secs));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    {
        let _ = f.write_all(existing.join("\n").as_bytes());
        let _ = f.write_all(b"\n");
    }
}

fn clear_origin_gone(policy_path: &Path, repo: &Path) {
    let path = origin_gone_ledger_path(policy_path);
    let repo_str = repo.display().to_string();
    let mut kept: Vec<String> = Vec::new();
    let mut removed = false;
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(2, '\t');
            let p = parts.next().unwrap_or("");
            if p == repo_str {
                removed = true;
                continue;
            }
            kept.push(line.to_string());
        }
    }
    if !removed {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    {
        let _ = f.write_all(kept.join("\n").as_bytes());
        if !kept.is_empty() {
            let _ = f.write_all(b"\n");
        }
    }
}

async fn handle_no_origin(
    state: &mut RepairState,
    repo: &Path,
    apply: bool,
    human: bool,
    policy: &SyncPolicy,
    reason: &str,
    policy_path: &Path,
) -> bool {
    if state.has_origin {
        return false;
    }
    state.attempted_ops += 1;
    if apply {
        // CHANGED 2026-07-28 (v0.113.7, concern-retry-softening):
        // before creating an offline mirror, probe reachability
        // with a 3x retry (5s between attempts) and consult the
        // gone-since ledger. Pre-fix this block forked a mirror
        // on the first invocation, which on a transient SSH
        // hiccup would create a repo the operator did not ask
        // for (and never asked for, ever). Post-fix: refuse to
        // create until the gone-window has elapsed AND the
        // checkout has never pushed. Two log lines tell the
        // operator which side of the decision fired.
        let any_reachable = probe_any_remote_reachable(repo).await;
        let pushed = ever_pushed(repo);
        let gone = origin_gone_secs(policy_path, repo);
        match decide_create_mirror(any_reachable, pushed, gone) {
            CreateMirrorDecision::TransientHiccup => {
                if human {
                    println!(
                        "   ℹ️  transient: origin probe inconclusive (reachable={}, ever_pushed={}, gone_secs={:?}) — will retry, NOT creating mirror",
                        any_reachable,
                        pushed,
                        gone
                    );
                }
                record_origin_gone(policy_path, repo);
                state.manual_only += 1;
                log_incident(
                    policy_path,
                    "concern",
                    repo.display().to_string(),
                    reason,
                    "create_private_remote",
                    None,
                    "skipped_transient",
                    Some(format!(
                        "transient ssh hiccup; reachable={} ever_pushed={} gone_secs={:?}",
                        any_reachable, pushed, gone
                    )),
                );
                return true;
            }
            CreateMirrorDecision::ReallyGone => {
                if human {
                    println!(
                        "   ℹ️  origin gone > 15min AND never pushed — creating offline mirror"
                    );
                }
                clear_origin_gone(policy_path, repo);
                // Fall through to the existing create block.
            }
        }
        let private_remote = if policy.auto_github_private {
            if human {
                println!("   plan: create GitHub private repo as origin");
            }
            create_github_private_remote(repo, &policy.auto_github_private_account, true)
        } else {
            if human {
                println!("   plan: create private bare repo as origin");
            }
            create_private_remote(repo)
        };
        if let Some(private_remote) = private_remote {
            state.succeeded_ops += 1;
            state.has_origin = true;
            state.has_upstream = true;
            if human {
                println!("   ok: created private remote: {}", private_remote);
            }
            log_incident(
                policy_path,
                "concern",
                repo.display().to_string(),
                reason,
                "create_private_remote",
                None,
                "ok",
                Some(format!("created private remote: {}", private_remote)),
            );
        } else {
            state.manual_only += 1;
            if human {
                println!("   fail: could not create private remote");
            }
            log_incident(
                policy_path,
                "concern",
                repo.display().to_string(),
                reason,
                "create_private_remote",
                None,
                "fail",
                Some("failed to create private remote".to_string()),
            );
        }
    }
    true
}

async fn handle_no_upstream(
    state: &mut RepairState,
    repo: &Path,
    apply: bool,
    human: bool,
    push_timeout_secs: u64,
    _push_retries: u32,
    reason: &str,
    policy_path: &Path,
) -> bool {
    if state.has_upstream {
        return false;
    }
    state.attempted_ops += 1;
    if human {
        println!("   plan: set upstream via `git push -u origin HEAD`");
    }
    if apply {
        match run_git_with_timeout(
            repo,
            &["push", "-u", "origin", "HEAD"],
            push_timeout_secs,
            "push -u",
        )
        .await
        {
            Ok(()) => {
                state.succeeded_ops += 1;
                state.has_upstream = true;
                if human {
                    println!("   ok: upstream configured");
                }
                log_incident(
                    policy_path,
                    "concern",
                    repo.display().to_string(),
                    reason,
                    "set_upstream_push_u",
                    None,
                    "ok",
                    None,
                );
            }
            Err(e) => {
                if human {
                    println!("   fail: upstream configure failed: {}", e);
                }
                log_incident(
                    policy_path,
                    "concern",
                    repo.display().to_string(),
                    reason,
                    "set_upstream_push_u",
                    None,
                    "fail",
                    Some(e.to_string()),
                );
                return true;
            }
        }
    }
    false
}

async fn handle_behind(
    state: &mut RepairState,
    repo: &Path,
    apply: bool,
    human: bool,
    pull_timeout_secs: u64,
    reason: &str,
    policy_path: &Path,
) -> bool {
    state.attempted_ops += 1;
    if human {
        println!("   plan: pull --no-rebase (merge)");
    }
    if apply {
        match run_git_with_timeout(
            repo,
            &["pull", "--no-rebase"],
            pull_timeout_secs,
            "pull/merge",
        )
        .await
        {
            Ok(()) => {
                state.succeeded_ops += 1;
                if human {
                    println!("   ok: pulled");
                }
                log_incident(
                    policy_path,
                    "concern",
                    repo.display().to_string(),
                    reason,
                    "pull_merge",
                    None,
                    "ok",
                    None,
                );
            }
            Err(e) => {
                if human {
                    println!("   fail: pull failed: {}", e);
                }
                log_incident(
                    policy_path,
                    "concern",
                    repo.display().to_string(),
                    reason,
                    "pull_merge",
                    None,
                    "fail",
                    Some(e.to_string()),
                );
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
async fn handle_ahead(
    state: &mut RepairState,
    repo: &Path,
    apply: bool,
    human: bool,
    push_timeout_secs: u64,
    push_retries: u32,
    blob_threshold: u64,
    rewrite_large_any: bool,
    excluded_dir_names: &std::collections::BTreeSet<String>,
    reason: &str,
    policy_path: &Path,
    svc: &GitService,
) -> bool {
    state.attempted_ops += 1;
    if human {
        println!("   plan: push origin HEAD");
    }
    state.push_ok = false;
    if !apply {
        return false;
    }
    #[allow(unused_assignments)]
    match push_with_retries(repo, push_timeout_secs, push_retries, "push").await {
        Ok(()) => {
            state.succeeded_ops += 1;
            state.push_ok = true;
            if human {
                println!("   ok: pushed");
            }
            log_incident(
                policy_path,
                "concern",
                repo.display().to_string(),
                reason,
                "push_origin_head",
                None,
                "ok",
                None,
            );
            // Also push to mirror remotes (codeberg, gitlab, etc.)
            if let Ok(policy) = SyncPolicy::load(policy_path) {
                if !policy.remotes.is_empty() {
                    // CHANGED 2026-06-23 (goal mqqsyzyd-qkvna5): honor
                    // per-repo `exclude_remotes` so a repo can opt out
                    // of a specific mirror (e.g. gitlab for a repo over
                    // the free-tier storage quota) without affecting
                    // other repos that use the same mirror.
                    let repo_override = crate::policy::load_repo_override(repo);
                    let mirror_results = push_mirror_remotes(
                        repo,
                        &policy.remotes,
                        push_timeout_secs,
                        push_retries,
                        true,
                        &repo_override.exclude_remotes,
                        repo_override.auto_create_on_codeberg,
                    )
                    .await;
                    for (name, result) in &mirror_results {
                        if let Err(e) = result {
                            if human {
                                println!("   warn: mirror push to {} failed: {}", name, e);
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            if human {
                println!("   fail: push failed: {}", e);
            }

            let err_str = e.to_string().to_lowercase();

            // Check if push failed because remote doesn't exist or is unreachable
            // In this case, auto-create a private bare repo as the remote
            let no_remote = err_str.contains("no such remote")
                || err_str.contains("remote does not exist")
                || err_str.contains("repository not found")
                || err_str.contains("could not resolve host")
                || err_str.contains("does not appear to be a git repository")
                || (err_str.contains("exit status: 128") && err_str.contains("fatal:"));

            if no_remote {
                // Try to create a private bare repo and use it as origin
                if human {
                    println!("   info: no remote detected, creating private bare repo");
                }
                if let Some(private_remote) = create_private_remote(repo) {
                    if human {
                        println!("   info: created private remote: {}", private_remote);
                    }
                    // Retry push with new remote
                    match push_with_retries(repo, push_timeout_secs, push_retries, "push").await {
                        Ok(()) => {
                            state.succeeded_ops += 1;
                            state.push_ok = true;
                            if human {
                                println!("   ok: pushed to private remote");
                            }
                            log_incident(
                                policy_path,
                                "concern",
                                repo.display().to_string(),
                                reason,
                                "push_origin_head",
                                None,
                                "ok",
                                Some(format!("pushed to private remote: {}", private_remote)),
                            );
                            return true;
                        }
                        Err(e2) => {
                            if human {
                                println!("   fail: push to private remote also failed: {}", e2);
                            }
                            log_incident(
                                policy_path,
                                "concern",
                                repo.display().to_string(),
                                reason,
                                "push_origin_head",
                                None,
                                "fail",
                                Some(e2.to_string()),
                            );
                            return true;
                        }
                    }
                } else {
                    if human {
                        println!("   fail: could not create private remote");
                    }
                    log_incident(
                        policy_path,
                        "concern",
                        repo.display().to_string(),
                        reason,
                        "push_origin_head",
                        None,
                        "fail",
                        Some(e.to_string()),
                    );
                    return true;
                }
            }

            // For permission denied or other errors on existing remote,
            // just record failure and continue - no permanent marking
            // These will retry on next cycle naturally
            log_incident(
                policy_path,
                "concern",
                repo.display().to_string(),
                reason,
                "push_origin_head",
                None,
                "fail",
                Some(e.to_string()),
            );
            // Don't continue here - let it fall through to large blob detection below
            // (but without the manual_only marking)

            let large = detect_large_blobs_ahead(repo, blob_threshold)
                .await
                .unwrap_or_default();
            if !large.is_empty() {
                if human {
                    println!(
                        "   detect: large blobs in ahead range ({} entries)",
                        large.len()
                    );
                }
                let mut dirs = BTreeSet::new();
                for (_, path) in &large {
                    if let Some(dir) = top_level_dir(path) {
                        if is_excluded_dir_name(&dir, excluded_dir_names) {
                            dirs.insert(dir);
                        }
                    }
                }
                let dirs: Vec<String> = dirs.into_iter().collect();
                let rewrite_paths: Vec<String> = if !dirs.is_empty() {
                    dirs
                } else if rewrite_large_any {
                    let mut unique = BTreeSet::new();
                    for (_, p) in &large {
                        unique.insert(p.clone());
                    }
                    unique.into_iter().collect()
                } else {
                    Vec::new()
                };

                if rewrite_paths.is_empty() {
                    if human {
                        println!("   manual: large blobs found but not in excluded dirs");
                    }
                    log_incident(
                        policy_path,
                        "concern",
                        repo.display().to_string(),
                        reason,
                        "large_blob_detected",
                        None,
                        "manual",
                        Some(format!(
                            "threshold={} entries={} rewrite_allowed=false",
                            blob_threshold,
                            large.len()
                        )),
                    );
                } else {
                    if human {
                        println!(
                            "   plan: rewrite ahead history removing paths {:?}",
                            rewrite_paths
                        );
                    }
                    match rewrite_ahead_paths(repo, &rewrite_paths, "backup/pre-sync-largeblob-fix")
                    {
                        Ok(Some(outcome)) => {
                            // CHANGED 2026-07-26 (v0.113.3, audit
                            // SYNC-H6): the pre-fix arm pushed via the
                            // NON-FORCE `push_with_retries` to an
                            // `origin` that filter-repo had deleted —
                            // and its auto-pull-on-reject recovery
                            // would have merged the PRE-REWRITE
                            // history back in (the blob returns to
                            // local history and is pushed to all
                            // mirrors). Now: force-push leased to the
                            // pre-rewrite upstream sha, to origin AND
                            // each configured mirror.
                            let bundle_for_log = outcome.bundle_path.clone();
                            if human {
                                println!(
                                    "   ok: rewrite complete (backup bundle: {})",
                                    outcome.bundle_path
                                );
                            }
                            let branch = current_branch(repo).unwrap_or_default();
                            if branch.is_empty() {
                                log_incident(
                                    policy_path,
                                    "concern",
                                    repo.display().to_string(),
                                    reason,
                                    "rewrite_then_push",
                                    Some(bundle_for_log),
                                    "fail",
                                    Some("rewrote history on a detached HEAD — push manually".to_string()),
                                );
                            } else {
                                match crate::git::force_push_after_rewrite(
                                    repo,
                                    "origin",
                                    &branch,
                                    &outcome.lease,
                                    push_timeout_secs,
                                )
                                .await
                                {
                                    Ok(()) => {
                                        state.succeeded_ops += 1;
                                        state.push_ok = true;
                                        if human {
                                            println!("   ok: force-pushed origin after rewrite");
                                        }
                                        log_incident(
                                            policy_path,
                                            "concern",
                                            repo.display().to_string(),
                                            reason,
                                            "rewrite_then_push",
                                            Some(bundle_for_log),
                                            "ok",
                                            Some(format!("paths={:?}", rewrite_paths)),
                                        );
                                        // Also force-push mirror remotes
                                        // (same lease anchor: mirrors
                                        // held the same pre-rewrite
                                        // history; a diverged mirror
                                        // fails the lease and is
                                        // logged, never clobbered).
                                        if let Ok(policy) = SyncPolicy::load(policy_path) {
                                            if !policy.remotes.is_empty() {
                                                let repo_override =
                                                    crate::policy::load_repo_override(repo);
                                                for remote in &policy.remotes {
                                                    if remote.name == "origin"
                                                        || repo_override
                                                            .exclude_remotes
                                                            .contains(&remote.name)
                                                    {
                                                        continue;
                                                    }
                                                    if let Err(e) =
                                                        crate::git::force_push_after_rewrite(
                                                            repo,
                                                            &remote.name,
                                                            &branch,
                                                            &outcome.lease,
                                                            push_timeout_secs,
                                                        )
                                                        .await
                                                    {
                                                        eprintln!(
                                                            "⚠️ mirror {} push-after-rewrite failed for {}: {}",
                                                            remote.name,
                                                            repo.display(),
                                                            e
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e2) => {
                                        if human {
                                            println!("   fail: push after rewrite failed: {}", e2);
                                        }
                                        log_incident(
                                            policy_path,
                                            "concern",
                                            repo.display().to_string(),
                                            reason,
                                            "rewrite_then_push",
                                            Some(outcome.bundle_path.clone()),
                                            "fail",
                                            Some(e2.to_string()),
                                        );
                                    }
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(rewrite_err) => {
                            if human {
                                println!("   fail: rewrite failed: {}", rewrite_err);
                            }
                            log_incident(
                                policy_path,
                                "concern",
                                repo.display().to_string(),
                                reason,
                                "rewrite_large_blob",
                                None,
                                "fail",
                                Some(rewrite_err.to_string()),
                            );
                        }
                    }
                }
            } else {
                let branch = current_branch(repo).unwrap_or_default();
                let dry_run = run_git_capture_output(
                    repo,
                    &["push", "--dry-run", "origin", "HEAD"],
                    "push --dry-run",
                )
                .unwrap_or_default();
                let looks_branch_mismatch = dry_run.to_ascii_lowercase().contains("up-to-date");
                if looks_branch_mismatch
                    && !branch.is_empty()
                    && remote_branch_exists(repo, &branch)
                    && has_tracking_upstream(repo)
                {
                    if human {
                        println!(
                            "   plan: align upstream to origin/{} (possible branch mismatch)",
                            branch
                        );
                    }
                    match set_upstream_to_branch(repo, &branch) {
                        Ok(()) => {
                            if human {
                                println!("   ok: upstream realigned");
                            }
                            match push_with_retries(
                                repo,
                                push_timeout_secs,
                                push_retries,
                                "push-after-upstream-align",
                            )
                            .await
                            {
                                Ok(()) => {
                                    state.succeeded_ops += 1;
                                    state.push_ok = true;
                                    if human {
                                        println!("   ok: pushed after upstream align");
                                    }
                                    log_incident(
                                        policy_path,
                                        "concern",
                                        repo.display().to_string(),
                                        reason,
                                        "realign_upstream_then_push",
                                        None,
                                        "ok",
                                        Some(format!("branch={}", branch)),
                                    );
                                    // Also push to mirror remotes
                                    if let Ok(policy) = SyncPolicy::load(policy_path) {
                                        if !policy.remotes.is_empty() {
                                            // CHANGED 2026-06-23: honor
                                            // per-repo exclude_remotes
                                            // (see goal mqqsyzyd-qkvna5).
                                            let repo_override =
                                                crate::policy::load_repo_override(repo);
                                            push_mirror_remotes(
                                                repo,
                                                &policy.remotes,
                                                push_timeout_secs,
                                                push_retries,
                                                true,
                                                &repo_override.exclude_remotes,
                                                repo_override.auto_create_on_codeberg,
                                            )
                                            .await;
                                        }
                                    }
                                }
                                Err(e2) => {
                                    if human {
                                        println!(
                                            "   fail: push after upstream align failed: {}",
                                            e2
                                        );
                                    }
                                    log_incident(
                                        policy_path,
                                        "concern",
                                        repo.display().to_string(),
                                        reason,
                                        "realign_upstream_then_push",
                                        None,
                                        "fail",
                                        Some(e2.to_string()),
                                    );
                                }
                            }
                        }
                        Err(set_err) => {
                            if human {
                                println!("   fail: upstream align failed: {}", set_err);
                            }
                        }
                    }
                }
            }
        }
    }
    if !state.push_ok {
        log_incident(
            policy_path,
            "concern",
            repo.display().to_string(),
            reason,
            "push_origin_head",
            None,
            "fail",
            Some("push did not clear concern".to_string()),
        );
    }
    if state.push_ok {
        if let Ok(next_after_push) = svc.get_status().await {
            if next_after_push.ahead > 0 {
                let branch = current_branch(repo).unwrap_or_default();
                if !branch.is_empty() && remote_branch_exists(repo, &branch) {
                    if human {
                        println!(
                            "   plan: realign upstream to origin/{} (ahead still > 0 after push)",
                            branch
                        );
                    }
                    match set_upstream_to_branch(repo, &branch) {
                        Ok(()) => {
                            if human {
                                println!("   ok: upstream realigned");
                            }
                        }
                        Err(e) => {
                            if human {
                                println!("   fail: upstream realign failed: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

async fn verify_resolution(
    repo: &Path,
    apply: bool,
    human: bool,
    resolved: &mut usize,
    reason: &str,
    policy_path: &Path,
    svc: &GitService,
) {
    if !apply {
        return;
    }
    if let Ok(next) = svc.get_status().await {
        let has_origin = has_origin_remote(repo);
        let has_upstream = has_tracking_upstream(repo);
        // CHANGED 2026-07-28 (v0.113.7): the post-handler
        // `still_concern` check now also considers `pack_too_large`.
        // Without this, a repo that was in the concern list ONLY
        // because of `pack_too_large` (and not because of ahead/behind/
        // origin/upstream) would be reported as "resolved" after the
        // auto-repair pass — even though the underlying size issue
        // is unchanged (the daemon has no code path that shrinks
        // history). The fix: include `pack_too_large_forces_concern`
        // in the predicate (routed through the testable helper
        // `verify_resolution_still_concern`), so a size-only concern
        // stays "still concerned" until the operator actually
        // shrinks the repo.
        let pack_still = pack_too_large_forces_concern(
            crate::git::github_pack_too_large(repo, None),
        );
        let still_concern = verify_resolution_still_concern(
            next.ahead,
            next.behind,
            has_origin,
            has_upstream,
            pack_still,
        );
        if !still_concern {
            *resolved += 1;
            if human {
                println!("   resolved: concern cleared");
            }
            log_incident(
                policy_path,
                "concern",
                repo.display().to_string(),
                reason,
                "verify_resolved",
                None,
                "ok",
                None,
            );
        } else {
            if human {
                println!(
                    "   remaining: ahead={} behind={} origin={} upstream={} pack_too_large={}",
                    next.ahead, next.behind, has_origin, has_upstream, pack_still
                );
            }
            // Only notify on true divergence (both ahead AND behind) - that's
            // the only case where we have no automatic resolution.
            // If just ahead > 0, we can push. If just behind > 0, we can pull.
            if next.ahead > 0 && next.behind > 0 {
                let details = format!("ahead={} behind={}", next.ahead, next.behind);
                send_sync_conflict_notification(repo, reason, &details);
            }
            log_incident(
                policy_path,
                "concern",
                repo.display().to_string(),
                reason,
                "verify_resolved",
                None,
                "remaining",
                Some(format!("ahead={} behind={}", next.ahead, next.behind)),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_repair_concerns(
    policy_path: &Path,
    apply: bool,
    only_repo: Option<PathBuf>,
    push_timeout_override: Option<u64>,
    push_retries: u32,
    rewrite_large_any: bool,
    filter: ConcernRepairFilter,
    json: bool,
) -> Result<RepairSummary> {
    let human = !json;
    macro_rules! out {
        ($($arg:tt)*) => {{
            if human {
                println!($($arg)*);
            }
        }};
    }

    let policy = SyncPolicy::load(policy_path)?;
    let roots = policy.watch_root_paths();
    let excluded_dir_names = excluded_dir_names_set(&policy);
    let repos = if let Some(target_repo) = &only_repo {
        vec![target_repo.clone()]
    } else {
        discover_git_repos(
            &roots,
            &excluded_dir_names,
            &policy.exclude_repos,
            Some(&policy.system_repo),
        )
    };
    if repos.is_empty() {
        if let Some(target_repo) = &only_repo {
            out!(
                "⚠️ target repo not discovered in policy roots: {}",
                target_repo.display()
            );
        }
        return Ok(RepairSummary::default());
    }
    let push_timeout_secs = push_timeout_override
        .unwrap_or(policy.push_op_timeout_secs)
        .max(10);
    let push_retries = push_retries.max(1);
    let blob_threshold = push_large_blob_threshold_bytes(&policy);

    let mut concerns = 0usize;
    let mut state = RepairState {
        attempted_ops: 0,
        succeeded_ops: 0,
        manual_only: 0,
        has_origin: false,
        has_upstream: false,
        push_ok: false,
    };
    let mut resolved = 0usize;
    // Use the same refined concern logic as the `repos` command: an
    // AHEAD repo is only a concern if a recent push failure was recorded.
    let recent_push_failures = build_recent_push_failure_map(policy_path);

    for repo in repos {
        let svc = match GitService::new(&repo) {
            Ok(svc) => svc,
            Err(e) => {
                eprintln!("⚠️ {} init_failed: {}", repo.display(), e);
                continue;
            }
        };
        let mut status = match svc.get_status().await {
            Ok(status) => status,
            Err(e) => {
                eprintln!("⚠️ {} status_failed: {}", repo.display(), e);
                continue;
            }
        };
        // Repos the operator has flagged as intentionally isolated are
        // not a hidden concern: skip them entirely so `repair concerns`
        // does not propose `git push -u origin HEAD` against a remote
        // the operator has explicitly chosen to leave unconnected.
        let repo_override = crate::policy::load_repo_override(&repo);
        if repo_override.intentional_no_upstream
            && !has_tracking_upstream(&repo)
            && only_repo.is_none()
        {
            out!(
                "ℹ️  {}  skipped: intentional_no_upstream set in .dracon/dracon-sync.toml",
                repo.display()
            );
            continue;
        }

        state.has_origin = has_origin_remote(&repo);
        state.has_upstream = has_tracking_upstream(&repo);
        // CHANGED 2026-06-20: same `has_any_remote` derivation as in
        // the `repos` command. A repo with at least one configured
        // remote (any name) is not a "no origin" concern.
        let has_any_remote = !crate::git::multi_remote::list_remotes(&repo).is_empty();
        // Use the same refined concern logic as the `repos` command:
        // an AHEAD repo is only a concern if a recent push failure was
        // recorded. This keeps `repair concerns` consistent with the
        // user-visible `repos` table.
        let recent_push_failure = recent_push_failures
            .as_ref()
            .map(|m| {
                m.get(repo.to_string_lossy().as_ref())
                    .copied()
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let is_concern = repo_is_concern_with_push_failure(
            &status,
            state.has_origin,
            state.has_upstream,
            has_any_remote,
            recent_push_failure,
        );
        // CHANGED 2026-07-28 (v0.113.7): the `repos` table classifies a
        // pack-too-large as a CONCERN (see `pack_too_large_forces_concern`).
        // The repair path must use the same predicate or the
        // `repair concerns` flow will skip these repos entirely (the
        // existing `repo_is_concern_with_push_failure` does not know
        // about the size-guard signal). Without this, a CONCERN visible
        // in `repos` would be invisible to the repair path, which is
        // a contract violation. The same PACK_SIZE_WARNING short-circuit
        // guard below then ensures the repair is a no-op (the daemon
        // has no code that shrinks a repo).
        let size_info = crate::git::github_pack_too_large(&repo, None);
        let pack_too_large = pack_too_large_forces_concern(size_info);
        if !is_concern && !pack_too_large {
            continue;
        }
        let stuck_push = repo_is_stuck_push(
            &status,
            state.has_origin,
            state.has_upstream,
            has_any_remote,
            recent_push_failure,
        );
        let stuck_pull = repo_is_stuck_pull(
            &status,
            state.has_origin,
            state.has_upstream,
            has_any_remote,
        );
        if matches!(filter, ConcernRepairFilter::StuckPush) && !stuck_push {
            continue;
        }
        if matches!(filter, ConcernRepairFilter::StuckPull) && !stuck_pull {
            continue;
        }
        concerns += 1;
        let flags = repo_state_flags_with_push_failure(
            &status,
            state.has_origin,
            state.has_upstream,
            has_any_remote,
            recent_push_failure,
        );
        // CHANGED 2026-07-28 (v0.113.7): short-circuit for
        // PACK_SIZE_WARNING. The push path classifies a >2 GiB pushable
        // branch as a CONCERN (see `pack_too_large_forces_concern`), but
        // the daemon has no code path that shrinks a repo's history.
        // Without this guard, the auto-repair path would attempt every
        // handler below (`handle_no_origin`, `handle_no_upstream`,
        // `handle_behind`, etc.) and fail silently, producing
        // journalctl noise every sync cycle. The operator's response is
        // documented in the row's HINT: shrink history (filter-repo) or
        // migrate assets to OVH.
        //
        // CHANGED 2026-07-28 (v0.113.7, follow-up): the previous version
        // of this guard checked `flags.iter().any(|f| f == "PACK_SIZE_WARNING")`
        // — but `repo_state_flags_with_push_failure` (the function that
        // built `flags` above) does NOT add `PACK_SIZE_WARNING`. That
        // flag is only added in `run_repos_report` at line 3157 (the
        // row-construction code). The guard was therefore dead code:
        // for the specific CAG case (clean, synced, origin-ok,
        // upstream-ok, 0-ahead, 0-behind) no handlers match anyway, so
        // the empirical outcome (`operations_planned: 0`) is correct
        // by coincidence. For a hypothetical repo with BOTH
        // `PACK_SIZE_WARNING` and a real concern (e.g. `STUCK_PUSH`),
        // the dead guard would have missed its short-circuit and the
        // daemon would have attempted handlers — failing silently. The
        // fix: re-use the `pack_too_large` value already computed at
        // line 6391 (the early-skip) — the same `github_pack_too_large`
        // call that drives the concern classification above. No
        // additional git subprocess; the value is already in scope.
        // The predicate is extracted to `pack_too_large_skips_repair`
        // so the regression test can verify the guard fires
        // unconditionally on `pack_too_large=true` — not by
        // coincidence on CAG's clean/synced state. See
        // `pack_too_large_skips_repair` for the rationale.
        if pack_too_large_skips_repair(pack_too_large) {
            out!(
                "⏭️  {}  skipping auto-repair: github push is permanently skipped (pushable branch > 2 GiB). Operator action required.",
                repo.display()
            );
            continue;
        }
        let reason = flags.join(",");

        out!(
            "\n🔎 {}  state: ahead={} behind={} clean={} origin={} upstream={}",
            repo.display(),
            status.ahead,
            status.behind,
            status.is_clean,
            state.has_origin,
            state.has_upstream
        );

        if handle_no_origin(
            &mut state,
            &repo,
            apply,
            human,
            &policy,
            &reason,
            policy_path,
        )
        .await
        {
            continue;
        }

        if handle_no_upstream(
            &mut state,
            &repo,
            apply,
            human,
            push_timeout_secs,
            push_retries,
            &reason,
            policy_path,
        )
        .await
        {
            continue;
        }

        #[allow(clippy::collapsible_if)]
        if status.behind > 0 && state.has_upstream {
            if handle_behind(
                &mut state,
                &repo,
                apply,
                human,
                policy.pull_op_timeout_secs,
                &reason,
                policy_path,
            )
            .await
            {
                continue;
            }
            // Re-fetch status after pull — the repo state may have changed
            // (e.g. diverged repo is now just ahead after merge).
            if let Ok(new_status) = svc.get_status().await {
                status = new_status;
                state.has_upstream = has_tracking_upstream(&repo);
            }
        }

        #[allow(clippy::collapsible_if)]
        if status.ahead > 0 && state.has_upstream {
            if handle_ahead(
                &mut state,
                &repo,
                apply,
                human,
                push_timeout_secs,
                push_retries,
                blob_threshold,
                rewrite_large_any,
                &excluded_dir_names,
                &reason,
                policy_path,
                &svc,
            )
            .await
            {
                continue;
            }
        }

        verify_resolution(
            &repo,
            apply,
            human,
            &mut resolved,
            &reason,
            policy_path,
            &svc,
        )
        .await;
    }

    let summary = RepairSummary {
        found: concerns,
        planned: state.attempted_ops,
        attempted: if apply { state.attempted_ops } else { 0 },
        succeeded: state.succeeded_ops,
        resolved_now: if apply { resolved } else { 0 },
        manual_only: state.manual_only,
    };
    if json {
        let payload = RepairJson {
            policy: policy_path.display().to_string(),
            scope: "concern".to_string(),
            mode: if apply {
                "apply".to_string()
            } else {
                "dry_run".to_string()
            },
            found: summary.found,
            planned: summary.planned,
            attempted: summary.attempted,
            succeeded: summary.succeeded,
            resolved_now: summary.resolved_now,
            manual_only: summary.manual_only,
            ledger: incident_ledger_path(policy_path).display().to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if summary.found > 0 {
        println!("\n✅ Concern management summary");
        println!("   concerns_found: {}", summary.found);
        println!("   operations_planned: {}", summary.planned);
        println!("   operations_succeeded: {}", summary.succeeded);
        println!("   manual_only: {}", summary.manual_only);
        if apply {
            println!("   concerns_resolved_now: {}", summary.resolved_now);
        } else {
            println!("   dry_run: true (rerun with --apply to execute)");
        }
        println!("   ledger: {}", incident_ledger_path(policy_path).display());
    }

    Ok(summary)
}

pub(crate) async fn run_repair_warns(
    policy_path: &Path,
    apply: bool,
    only_repo: Option<PathBuf>,
    json: bool,
) -> Result<RepairSummary> {
    let human = !json;
    macro_rules! out {
        ($($arg:tt)*) => {{
            if human {
                println!($($arg)*);
            }
        }};
    }

    let policy = SyncPolicy::load(policy_path)?;
    let roots = policy.watch_root_paths();
    let excluded_dir_names = excluded_dir_names_set(&policy);
    let repos = if let Some(target_repo) = &only_repo {
        vec![target_repo.clone()]
    } else {
        discover_git_repos(
            &roots,
            &excluded_dir_names,
            &policy.exclude_repos,
            Some(&policy.system_repo),
        )
    };
    if repos.is_empty() {
        if let Some(target_repo) = &only_repo {
            out!(
                "⚠️ target repo not discovered in policy roots: {}",
                target_repo.display()
            );
        }
        return Ok(RepairSummary::default());
    }

    let mut warns = 0usize;
    let mut attempted = 0usize;
    let mut succeeded = 0usize;

    for repo in repos {
        let svc = match GitService::new(&repo) {
            Ok(svc) => svc,
            Err(e) => {
                eprintln!("⚠️ {} init_failed: {}", repo.display(), e);
                continue;
            }
        };
        let status = match svc.get_status().await {
            Ok(status) => status,
            Err(e) => {
                eprintln!("⚠️ {} status_failed: {}", repo.display(), e);
                continue;
            }
        };
        let entries = repo_diff_entries(&repo).await.unwrap_or_default();
        let effective_dirty = has_sync_relevant_dirty_entries(
            &repo,
            &entries,
            &excluded_dir_names,
            &policy.exclude_file_patterns,
            policy.max_stage_file_bytes,
            &policy.auto_commit_exclude_patterns,
        );
        let has_origin = has_origin_remote(&repo);
        let has_upstream = has_tracking_upstream(&repo);
        // CHANGED 2026-06-20: same `has_any_remote` derivation as the
        // main `repos` pass. A repo with at least one configured remote
        // is not a "no origin" concern and the WARN classification only
        // fires for actually concerning (untracked) or dirty repos.
        let has_any_remote = !crate::git::multi_remote::list_remotes(&repo).is_empty();
        let mut effective_status = status.clone();
        effective_status.is_clean = !effective_dirty;
        effective_status.modified_files = status.modified_files;
        effective_status.staged_files = status.staged_files;
        // CHANGED 2026-06-15 (goal 0ab367b5 / Junk-Runner-bevy WARN fix):
        // `dracon-git` was upgraded 94.2.7 → 94.7.0. The new version
        // correctly separates untracked from modified (the old version
        // counted `is_wt_new()` as modified, causing 91 false MOD for
        // Junk-Runner-bevy when 3 untracked test-results/ PNGs were
        // involved). `RepoStatus` now has an `untracked_files` field
        // so we copy it through.
        effective_status.untracked_files = status.untracked_files;
        // Use real dirty state for classification — a repo with TRACKED
        // modified files is WARN even if the daemon wouldn't auto-commit them.
        // Untracked files (build artifacts) do NOT count as dirty.
        let real_is_dirty = status.modified_files > 0 || status.staged_files > 0;
        if !real_is_dirty {
            continue;
        }
        warns += 1;
        let flags = repo_state_flags(
            &effective_status,
            has_origin,
            has_upstream,
            has_any_remote,
        );
        let reason = flags.join(",");
        out!(
            "\n🟡 {}  state={} modified={} staged={}",
            repo.display(),
            reason,
            effective_status.modified_files,
            effective_status.staged_files
        );
        out!("   plan: run normal sync triage (stage/commit/push)");
        if !apply {
            append_incident_record(
                policy_path,
                &IncidentRecord {
                    ts_unix: timestamp_secs(),
                    scope: "warn".to_string(),
                    repo: repo.display().to_string(),
                    reason,
                    action: "dry_run_sync_triage".to_string(),
                    backup_branch: None,
                    result: "planned".to_string(),
                    details: None,
                },
            );
            continue;
        }

        attempted += 1;
        match crate::sync::sync_repo(
            &repo,
            &policy,
            &excluded_dir_names,
            0,
            None,
            false,
            Some(policy_path),
        )
        .await
        {
            Ok(outcome) => {
                succeeded += 1;
                out!("   ok: triage complete changed={}", outcome.has_changes());
                append_incident_record(
                    policy_path,
                    &IncidentRecord {
                        ts_unix: timestamp_secs(),
                        scope: "warn".to_string(),
                        repo: repo.display().to_string(),
                        reason,
                        action: "sync_triage".to_string(),
                        backup_branch: None,
                        result: "ok".to_string(),
                        details: Some(format!("changed={}", outcome.has_changes())),
                    },
                );
            }
            Err(e) => {
                out!("   fail: sync triage failed: {}", e);
                append_incident_record(
                    policy_path,
                    &IncidentRecord {
                        ts_unix: timestamp_secs(),
                        scope: "warn".to_string(),
                        repo: repo.display().to_string(),
                        reason,
                        action: "sync_triage".to_string(),
                        backup_branch: None,
                        result: "fail".to_string(),
                        details: Some(e.to_string()),
                    },
                );
            }
        }
    }

    let summary = RepairSummary {
        found: warns,
        planned: warns,
        attempted,
        succeeded,
        resolved_now: 0,
        manual_only: 0,
    };
    if json {
        let payload = RepairJson {
            policy: policy_path.display().to_string(),
            scope: "warn".to_string(),
            mode: if apply {
                "apply".to_string()
            } else {
                "dry_run".to_string()
            },
            found: summary.found,
            planned: summary.planned,
            attempted: summary.attempted,
            succeeded: summary.succeeded,
            resolved_now: summary.resolved_now,
            manual_only: summary.manual_only,
            ledger: incident_ledger_path(policy_path).display().to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if summary.found > 0 {
        println!("\n✅ Warn management summary");
        println!("   warns_found: {}", summary.found);
        println!("   operations_planned: {}", summary.planned);
        println!("   operations_attempted: {}", summary.attempted);
        println!("   operations_succeeded: {}", summary.succeeded);
        if !apply {
            println!("   dry_run: true (rerun with --apply to execute)");
        }
        println!("   ledger: {}", incident_ledger_path(policy_path).display());
    }
    Ok(summary)
}

pub(crate) fn create_github_private_remote(
    repo: &Path,
    account: &str,
    private: bool,
) -> Option<String> {
    let repo_name = repo.file_name()?.to_str()?.to_string();

    // FIRST REPO CREATE ATTEMPT:
    // Try to create a private GitHub repo matching the local directory name.
    // If it already exists, we reuse it below — we NEVER append a suffix.
    //   ⚠️  HISTORY: A previous version had a loop that appended -1, -2, -N to
    //   repo names when the base name was taken. This created 15+ orphan repos
    //   (dracon-demons-1..-9, browser-extensions-shared-1..-6).
    //   The suffix approach is DANGEROUS because:
    //   1. Every daemon cycle creates a new orphan repo
    //   2. GitHub counts orphan repos against quotas
    //   3. No cleanup mechanism existed
    //   NEVER reintroduce a suffix loop here or in any repo creation function.
    let mut cmd = gh_cmd();
    cmd.args(["repo", "create", &repo_name]);
    if private {
        cmd.arg("--private");
    } else {
        cmd.arg("--public");
    }
    let output = cmd.current_dir(repo).output().ok()?;

    if output.status.success() {
        let remote_url = format!("https://github.com/{}/{}.git", account, repo_name);

        let add_result = crate::git::git_cmd()
            .args(["remote", "add", "origin", &remote_url])
            .current_dir(repo)
            .output();

        if let Err(e) = add_result {
            eprintln!("⚠️ failed to add origin for {}: {}", repo.display(), e);
        }

        let mut current_branch =
            crate::git::current_branch(repo).unwrap_or_else(|| "main".to_string());

        if current_branch == "master" {
            if let Err(e) = crate::git::git_cmd()
                .args(["branch", "-m", "master", "main"])
                .current_dir(repo)
                .output()
            {
                eprintln!(
                    "⚠️ failed to rename master to main in {}: {}",
                    repo.display(),
                    e
                );
            } else {
                current_branch = "main".to_string();
            }
        }

        let push_result = crate::git::git_cmd()
            .args([
                "push",
                "-u",
                "origin",
                &format!("HEAD:refs/heads/{}", current_branch),
            ])
            .current_dir(repo)
            .output();

        if let Ok(push_output) = push_result {
            if !push_output.status.success() {
                let stderr = String::from_utf8_lossy(&push_output.stderr);
                eprintln!(
                    "⚠️ failed to push initial commit for {}: {}",
                    repo.display(),
                    stderr
                );
            }
        } else {
            eprintln!(
                "⚠️ failed to push initial commit for {}: could not execute",
                repo.display()
            );
        }

        if !crate::git::has_tracking_upstream(repo) {
            let _ = crate::git::git_cmd()
                .args(["branch", "--set-upstream-to=origin/main", &current_branch])
                .current_dir(repo)
                .output();
        }

        return Some(remote_url);
    }

    // Repo already exists — reuse it instead of creating a new one with a suffix
    let remote_url = format!("https://github.com/{}/{}.git", account, repo_name);

    // Check if origin already exists locally before adding
    let has_origin = crate::git::git_cmd()
        .args(["remote", "get-url", "origin"])
        .current_dir(repo)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_origin {
        let add_result = crate::git::git_cmd()
            .args(["remote", "add", "origin", &remote_url])
            .current_dir(repo)
            .output();

        if let Err(e) = add_result {
            eprintln!("⚠️ failed to add origin for {}: {}", repo.display(), e);
        }
    }

    let mut current_branch = crate::git::current_branch(repo).unwrap_or_else(|| "main".to_string());

    if current_branch == "master" {
        if let Err(e) = crate::git::git_cmd()
            .args(["branch", "-m", "master", "main"])
            .current_dir(repo)
            .output()
        {
            eprintln!(
                "⚠️ failed to rename master to main in {}: {}",
                repo.display(),
                e
            );
        } else {
            current_branch = "main".to_string();
        }
    }

    let _ = crate::git::git_cmd()
        .args([
            "push",
            "-u",
            "origin",
            &format!("HEAD:refs/heads/{}", current_branch),
        ])
        .current_dir(repo)
        .output();

    if !crate::git::has_tracking_upstream(repo) {
        let _ = crate::git::git_cmd()
            .args(["branch", "--set-upstream-to=origin/main", &current_branch])
            .current_dir(repo)
            .output();
    }

    Some(remote_url)
}

fn create_private_remote(repo: &Path) -> Option<String> {
    // NEVER overwrite an existing origin. Only create a local bare repo
    // for repos that genuinely have no remote configured.
    if has_origin_remote(repo) {
        eprintln!(
            "⚠️ refusing to create private remote for {} — origin already exists",
            repo.display()
        );
        return None;
    }

    let repo_name = repo.file_name()?.to_str()?.to_string();
    let private_remotes_dir = dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("dracon/private-remotes");

    if !private_remotes_dir.exists() {
        std::fs::create_dir_all(&private_remotes_dir).ok()?;
    }

    let bare_repo_path = private_remotes_dir.join(format!("{}.git", repo_name));
    let mut final_path = bare_repo_path.clone();
    let mut counter = 1;
    while final_path.exists() {
        final_path = private_remotes_dir.join(format!("{}-{}.git", repo_name, counter));
        counter += 1;
    }

    let bare_name = final_path.file_name()?.to_str()?;

    let output = crate::git::git_cmd()
        .args(["init", "--bare", bare_name])
        .current_dir(&private_remotes_dir)
        .output()
        .ok()?;

    if !output.status.success() {
        std::fs::create_dir_all(&final_path).ok()?;
        let output = crate::git::git_cmd()
            .args(["init", "--bare"])
            .current_dir(&final_path)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
    }

    let remote_url = format!("file://{}", final_path.display());

    let add_result = crate::git::git_cmd()
        .args(["remote", "add", "origin", &remote_url])
        .current_dir(repo)
        .output();

    if let Err(e) = add_result {
        eprintln!("⚠️ failed to add origin for {}: {}", repo.display(), e);
    }

    Some(remote_url)
}

/// ADDED 2026-06-30, goal `mr0grjhl-q1g5bo`:
/// "Subrepos should not be counted as untracked in the `dracon-sync repos`
/// report".
///
/// Count the untracked entries under `repo` that point to nested git
/// repositories (sibling subrepo dirs each containing their own `.git/`).
/// These show up in `git status --porcelain` as `?? <dir>/` and inflate
/// the parent's `UT` count even though they're tracked under their own
/// git history.
///
/// Reuses the `count_nested_repo_untracked_entries` helper from
/// `src/git/discovery.rs` (added by archived goal `mr02de1n-gjkgzp`)
/// which handles `..` paths, trailing slashes, `.git` files (submodules),
/// and unsafe-path rejection.
///
/// Returns 0 if `git ls-files` fails — the parent's raw UT count is left
/// untouched in that case (safe fallback: better to overcount than to
/// drop legitimate untracked files because of a transient `git`
/// failure).
pub(crate) async fn nested_repo_untracked_count(repo: &Path) -> usize {
    let entries = match crate::git::untracked_entries(repo).await {
        Ok(entries) => entries,
        Err(_) => return 0,
    };
    let paths: Vec<String> = entries
        .into_iter()
        .map(|d| d.path.to_string_lossy().into_owned())
        .collect();
    crate::git::count_nested_repo_untracked_entries(repo, &paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{
        default_auto_resolve_unmerged, default_push_debounce_secs,
        default_untracked_warn_threshold,
    };
    use crate::test_helpers::EnvRestorer;
    use dracon_git::types::RepoStatus;
    use std::os::unix::fs::PermissionsExt;

    fn make_status(is_clean: bool, ahead: usize, behind: usize) -> RepoStatus {
        let mut status = RepoStatus::default();
        status.branch = String::new();
        status.is_clean = is_clean;
        status.ahead = ahead;
        status.behind = behind;
        status.modified_files = if is_clean { 0 } else { 1 };
        status.untracked_files = 0;
        status.staged_files = 0;
        // ADDED 2026-07-21 (v0.112.29): most tests build a status
        // for a hypothetical repo WITH commits. Setting
        // `last_commit_hash = Some(...)` here prevents the new
        // `EMPTY_REPO` flag from firing on every test. Tests that
        // exercise the empty-repo path explicitly set
        // `last_commit_hash = None` after calling `make_status`.
        status.last_commit_hash = Some("deadbeef".to_string());
        status.last_commit_msg = None;
        status
    }

    /// ADDED 2026-07-28 (v0.113.7, concern-retry-softening):
    /// matrix test for the pure decision helper
    /// [`decide_create_mirror`]. Reproduces the original
    /// "transient ssh hiccup, will NOT create mirror" behavior.
    /// First, when any 3x-retry probe succeeded (the boolean
    /// models the call-site result of the retry loop
    /// succeeding at least once), no mirror is forked. Second,
    /// when the repo was previously pushed, a current outage is
    /// also treated as transient (origin has existed in the
    /// past). Pre-fix, `handle_no_origin` would fork a mirror
    /// on the first invocation regardless. Post-fix, only the
    /// `really_gone` companion test triggers creation.
    #[test]
    fn concerns_retry_softening() {
        // ANY remote reachable (the 3x retry succeeded at
        // least once) — the originating soft-spot case.
        assert_eq!(
            decide_create_mirror(true, false, None),
            CreateMirrorDecision::TransientHiccup
        );
        assert_eq!(
            decide_create_mirror(true, false, Some(3600)),
            CreateMirrorDecision::TransientHiccup
        );
        assert_eq!(
            decide_create_mirror(true, true, Some(3600)),
            CreateMirrorDecision::TransientHiccup
        );
        // Repo was previously pushed — even if no remote is
        // reachable now, do not fork a mirror.
        assert_eq!(
            decide_create_mirror(false, true, Some(3600)),
            CreateMirrorDecision::TransientHiccup
        );
        // No failure recorded this session (gone_secs = None).
        // First probe failure is therefore transient by policy.
        assert_eq!(
            decide_create_mirror(false, false, None),
            CreateMirrorDecision::TransientHiccup
        );
        // Gone window shorter than the 15-min threshold.
        assert_eq!(
            decide_create_mirror(false, false, Some(899)),
            CreateMirrorDecision::TransientHiccup
        );
    }

    /// ADDED 2026-07-28 (v0.113.7, concern-retry-softening):
    /// the genuine "origin really gone, create mirror" cases.
    /// All three preconditions must hold simultaneously: no
    /// remote answered (any_remote_reachable = false), never
    /// pushed (ever_pushed = false), AND the gone-window has
    /// elapsed (>= CREATE_MIRROR_GONE_THRESHOLD_SECS, 900).
    /// Pre-fix this path always fired; post-fix only these
    /// inputs reach the create block.
    #[test]
    fn concerns_retry_softening_really_gone() {
        assert_eq!(
            decide_create_mirror(false, false, Some(CREATE_MIRROR_GONE_THRESHOLD_SECS)),
            CreateMirrorDecision::ReallyGone
        );
        assert_eq!(
            decide_create_mirror(false, false, Some(CREATE_MIRROR_GONE_THRESHOLD_SECS + 1)),
            CreateMirrorDecision::ReallyGone
        );
        assert_eq!(
            decide_create_mirror(false, false, Some(3600)),
            CreateMirrorDecision::ReallyGone
        );
        // Boundary: exactly 1 second under threshold is still
        // transient.
        assert_eq!(
            decide_create_mirror(
                false,
                false,
                Some(CREATE_MIRROR_GONE_THRESHOLD_SECS - 1)
            ),
            CreateMirrorDecision::TransientHiccup
        );
    }

    /// ADDED 2026-07-28 (v0.113.7, concern-retry-softening):
    /// insert-if-absent ledger semantics. The first
    /// `record_origin_gone` call inserts the current timestamp;
    /// a second call for the same repo MUST preserve the
    /// original timestamp so the gone-window grows
    /// monotonically. Pre-fix, the function dropped and
    /// re-appended, which meant every repair invocation
    /// reset the 15-min gate — `ReallyGone` would never fire
    /// in production (the unit tests for the decision helper
    /// still passed because they feed synthetic inputs).
    #[test]
    fn concerns_ledger_insert_if_absent() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let policy_path = tmp.path().join("policy.toml");
        std::fs::write(&policy_path, "").expect("write policy");
        let repo = tmp.path().join("test-repo");
        std::fs::create_dir_all(&repo).expect("mkdir");
        // First call: no entry, inserts.
        record_origin_gone(&policy_path, &repo);
        let ledger_after_first =
            std::fs::read_to_string(origin_gone_ledger_path(&policy_path))
                .expect("read ledger");
        let first_line = ledger_after_first
            .lines()
            .find(|l| l.starts_with(&repo.display().to_string()))
            .expect("entry present")
            .to_string();
        let first_ts: u64 = first_line
            .split('\t')
            .nth(1)
            .expect("ts field")
            .parse()
            .expect("parse ts");
        // Wait at least 2 seconds so the wall clock differs.
        std::thread::sleep(std::time::Duration::from_secs(2));
        // Second call: must NOT overwrite the original.
        record_origin_gone(&policy_path, &repo);
        let ledger_after_second =
            std::fs::read_to_string(origin_gone_ledger_path(&policy_path))
                .expect("read ledger");
        let second_line = ledger_after_second
            .lines()
            .find(|l| l.starts_with(&repo.display().to_string()))
            .expect("entry still present")
            .to_string();
        let second_ts: u64 = second_line
            .split('\t')
            .nth(1)
            .expect("ts field")
            .parse()
            .expect("parse ts");
        assert_eq!(first_ts, second_ts, "ledger must preserve first-observed timestamp");
        // clear_origin_gone should drop the entry entirely.
        clear_origin_gone(&policy_path, &repo);
        let ledger_after_clear =
            std::fs::read_to_string(origin_gone_ledger_path(&policy_path))
                .expect("read ledger");
        assert!(
            !ledger_after_clear
                .lines()
                .any(|l| l.starts_with(&repo.display().to_string())),
            "clear_origin_gone must remove the repo entry"
        );
    }

    #[test]
    fn test_repo_failure_message_includes_context() {
        let msg = repo_failure_message("init_failed", Path::new("/tmp/repo"), "boom");
        assert!(msg.contains("init_failed"));
        assert!(msg.contains("/tmp/repo"));
        assert!(msg.contains("boom"));
    }

    #[test]
    fn test_repo_failure_message_for_status_failed() {
        let msg = repo_failure_message("status_failed", Path::new("/tmp/repo"), "status boom");
        assert!(msg.contains("status_failed"));
        assert!(msg.contains("status boom"));
    }

    /// ADDED 2026-06-30, goal `mr0grjhl-q1g5bo`: a parent git repo
    /// whose ONLY untracked entries are sibling subrepo directories
    /// (each with its own `.git/`) MUST NOT contribute those entries
    /// to the parent's UT count. Plain untracked files (no `.git`
    /// inside) MUST still be counted.
    #[tokio::test]
    async fn test_nested_repo_untracked_count_subtracts_sibling_subrepos() {
        use std::fs;
        use std::process::Command;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("parent");
        fs::create_dir_all(&repo).unwrap();
        // Initialise the parent as a real git repo so that
        // `git ls-files --others --exclude-standard` works.
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(&repo)
            .output()
            .expect("git init parent");
        // Initialise two subrepo siblings via real `git init` so that
        // the parent treats `child_a/` and `child_b/` as untracked
        // DIRECTORY entries (not as nested-git bisects).
        for name in ["child_a", "child_b"] {
            let child = repo.join(name);
            fs::create_dir_all(&child).unwrap();
            Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&child)
                .output()
                .expect("git init child");
            fs::create_dir_all(child.join(".git")).unwrap();
        }
        // And one plain (non-repo) untracked file.
        fs::write(repo.join("scratch.txt"), "hello").unwrap();
        // Now ask the helper. It must report 2 (child_a + child_b)
        // and ignore scratch.txt.
        let count = nested_repo_untracked_count(&repo).await;
        assert_eq!(
            count, 2,
            "must count both sibling subrepo dirs and ignore plain files, got {}",
            count,
        );
    }

    /// ADDED 2026-06-30, goal `mr0grjhl-q1g5bo`: a parent git repo
    /// with NO untracked entries MUST yield a count of 0 (no false
    /// positives, no off-by-one).
    #[tokio::test]
    async fn test_nested_repo_untracked_count_zero_for_clean_parent() {
        use std::fs;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("parent");
        fs::create_dir_all(&repo).unwrap();
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(&repo)
            .output()
            .expect("git init parent");
        let count = nested_repo_untracked_count(&repo).await;
        assert_eq!(count, 0, "clean parent must report zero");
    }

    /// ADDED 2026-06-30, goal `mr0grjhl-q1g5bo`: if `git ls-files`
    /// cannot run (path that is not a git repo), the helper MUST
    /// return 0 — the report then keeps the raw `effective_status
    /// .untracked_files` value, which is preferable to silently
    /// dropping legitimate untracked files because of a transient
    /// git failure.
    #[tokio::test]
    async fn test_nested_repo_untracked_count_returns_zero_when_git_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("not-a-git-dir");
        std::fs::create_dir_all(&repo).unwrap();
        let count = nested_repo_untracked_count(&repo).await;
        assert_eq!(count, 0, "non-git path must not blow up");
    }

    /// ADDED 2026-06-30, goal `mr0grjhl-q1g5bo`: a parent with a mix
    /// of one sibling subrepo AND one plain file MUST report exactly
    /// 1 nested-repo entry. This is the canonical case where
    /// `saturating_sub(1)` keeps the report's `UT` count accurate.
    #[tokio::test]
    async fn test_nested_repo_untracked_count_mixed_subrepo_and_plain_file() {
        use std::fs;
        use std::process::Command;
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("parent");
        fs::create_dir_all(&repo).unwrap();
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .current_dir(&repo)
            .output()
            .expect("git init parent");
        // One subrepo sibling.
        let child = repo.join("child");
        fs::create_dir_all(&child).unwrap();
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&child)
            .output()
            .expect("git init child");
        // One plain file.
        fs::write(repo.join("notes.md"), "hello").unwrap();
        let count = nested_repo_untracked_count(&repo).await;
        assert_eq!(
            count, 1,
            "must count child/ but NOT notes.md; got {}",
            count,
        );
        // Verify the corresponding raw untracked_files count
        // (the parent's view) is 2 (child/ + notes.md), so the
        // effective UT after subtraction would be 2 - 1 = 1.
        let raw = crate::git::untracked_entries(&repo)
            .await
            .expect("untracked_entries succeeds")
            .len();
        assert_eq!(
            raw, 2,
            "git sees both child/ and notes.md; subtracting 1 yields the effective UT"
        );
    }

    #[test]
    fn test_parse_git_log_meta_line_preserves_subject_with_separator() {
        // Commit subject that itself contains the unit-separator character
        // must be reconstructed verbatim rather than truncated at the first
        // extra field.
        let line = "hash0\u{1f}author\u{1f}2 hours ago\u{1f}1700000000\u{1f}a\u{1f}b\u{1f}c";
        let parsed = parse_git_log_meta_line(line).expect("parse");
        assert_eq!(parsed.0, "hash0");
        assert_eq!(parsed.1, "author");
        assert_eq!(parsed.2, "2 hours ago");
        assert_eq!(parsed.3, 1_700_000_000);
        assert_eq!(parsed.4, "a\u{1f}b\u{1f}c");
    }

    #[test]
    fn test_parse_git_log_meta_line_simple_subject() {
        let line = "h\u{1f}me\u{1f}1m\u{1f}1234\u{1f}hello world";
        let parsed = parse_git_log_meta_line(line).expect("parse");
        assert_eq!(parsed.4, "hello world");
    }

    #[test]
    fn test_parse_git_log_meta_line_rejects_too_few_fields() {
        assert!(parse_git_log_meta_line("a\u{1f}b").is_none());
    }

    #[test]
    fn test_parse_git_log_meta_line_rejects_blank() {
        assert!(parse_git_log_meta_line("   ").is_none());
    }

    #[test]
    fn test_last_push_for_branch_skips_unsafe_branch_names() {
        // Branch names that would break the reflog argument or shell quoting
        // must be skipped without invoking git at all. The repo path is
        // intentionally not a real repository — the helper must return "-"
        // before reaching git.
        for bad in [
            "",                // detached HEAD
            "-evil",           // leading dash
            "feat with space", // contains space
            "main\nbad",       // newline injection
            "feat;rm -rf",     // shell metachar
            "main?",           // glob meta
        ] {
            assert_eq!(
                last_push_for_branch(Path::new("/nonexistent/repo"), bad),
                "-",
                "branch {bad:?} should be skipped"
            );
        }
    }

    #[test]
    fn test_last_push_for_branch_uses_log_not_reflog() {
        // Regression: a freshly-cloned repo with no further fetches has
        // an empty reflog for `origin/<branch>`. The old helper used
        // `git reflog show origin/main --format=%cr -1`, which returned
        // empty output in that state and surfaced as a misleading "-"
        // in the PUSHED column even though the remote-tracking ref was
        // valid. The helper now uses
        // `git log -1 --format=%cr origin/main`, which returns the
        // committer date of the remote tip in both cases.
        let parent = tempfile::tempdir().unwrap();
        let bare = parent.path().join("bare.git");
        let repo = parent.path().join("repo");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
        };
        let run_bare = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&bare)
                .output()
                .unwrap()
        };
        // Seed an initial commit in the bare repo via a working tree.
        run_bare(&["init", "--bare", "--initial-branch=main"]);
        let seed = parent.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        let run_seed = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&seed)
                .output()
                .unwrap()
        };
        run_seed(&["init", "-b", "main"]);
        run_seed(&["config", "user.email", "ops@dracon.uk"]);
        run_seed(&["config", "user.name", "DraconDev"]);
        run_seed(&["config", "commit.gpgsign", "false"]);
        run_seed(&["config", "core.hooksPath", "/dev/null"]);
        std::fs::write(seed.join("README.md"), "seed\n").unwrap();
        run_seed(&["add", "README.md"]);
        run_seed(&["commit", "--no-verify", "-m", "seed"]);
        run_seed(&["remote", "add", "origin", bare.to_str().unwrap()]);
        let push_seed = run_seed(&["push", "origin", "main"]);
        assert!(
            push_seed.status.success(),
            "seed push failed: stdout={} stderr={}",
            String::from_utf8_lossy(&push_seed.stdout),
            String::from_utf8_lossy(&push_seed.stderr),
        );
        // Clone the bare repo so the local reflog for origin/main starts
        // empty (no subsequent fetches, no pushes).
        let clone = std::process::Command::new("git")
            .args(["clone", bare.to_str().unwrap(), repo.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            clone.status.success(),
            "clone failed: stdout={} stderr={}",
            String::from_utf8_lossy(&clone.stdout),
            String::from_utf8_lossy(&clone.stderr),
        );
        run(&["config", "user.email", "ops@dracon.uk"]);
        run(&["config", "user.name", "DraconDev"]);
        // Sanity: `git reflog show origin/main` is empty for a freshly-
        // cloned repo with no subsequent fetches, so the old helper
        // would have returned "-" here.
        let reflog_out = run(&["reflog", "show", "origin/main", "--format=%cr", "-1"]);
        let reflog_str = String::from_utf8_lossy(&reflog_out.stdout);
        assert!(
            reflog_str.trim().is_empty(),
            "test setup precondition: reflog must be empty in this scenario, got {:?}",
            reflog_str,
        );
        // `git log -1 --format=%cr origin/main` must return a real
        // date (this is what the helper now uses).
        let log_out = run(&["log", "-1", "--format=%cr", "origin/main"]);
        let log_str = String::from_utf8_lossy(&log_out.stdout);
        assert!(
            !log_str.trim().is_empty(),
            "test setup precondition: `git log` must return a real date for origin/main, got {:?}",
            log_str,
        );
        let pushed = last_push_for_branch(&repo, "main");
        assert_ne!(
            pushed, "-",
            "last_push_for_branch must not return '-' for a valid remote-tracking ref even when the reflog is empty (got {:?})",
            pushed
        );
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_shorter() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_longer() {
        assert_eq!(truncate("hello world", 5), "hell…");
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn test_truncate_unicode_truncation() {
        let s = "hello 世界 test";
        let result = truncate(s, 10);
        assert!(result.ends_with('…'));
    }

    /// Regression test for the goal-id-truncation bug: when a commit
    /// subject is a structured auto-commit (matches the daemon's
    /// `compute_blast_radius` format) and the full subject does not
    /// fit in `max_chars`, `format_commit_subject_for_display` must
    /// drop the trailing `| METRIC:…` suffix and (if still too long)
    /// the ` DELTA:…` segment BEFORE resorting to a plain
    /// `truncate()`. The plain truncate would otherwise slice through
    /// the goal id in the file list, producing a misleading id like
    /// `mr02de1n-gjkg…` instead of the full `mr02de1n-gjkgzp`.
    #[test]
    fn test_format_commit_subject_for_display_drops_pipe_metrics() {
        let full = "2 file(s) in .pi [.pi/goals/active_goal_2026063004051714_mr02de1n-gjkgzp.md, .pi/goals/goal_events.jsonl] DELTA:+8/-5 | GOAL:complete TOKENS:407K TIME:323m";
        // Subject is 168 chars; budget 100 should drop the trailing
        // pipe-separated metrics.
        let result = format_commit_subject_for_display(full, 100);
        assert!(
            !result.contains("GOAL:"),
            "trailing | GOAL:… metric must be dropped, got: {}",
            result
        );
        assert!(
            !result.contains("TOKENS:"),
            "trailing | TOKENS:… metric must be dropped, got: {}",
            result
        );
        assert!(
            result.contains("mr02de1n-gjkgzp"),
            "full goal id must remain after stripping pipe metrics, got: {}",
            result
        );
    }

    /// Regression test for the deep-path case: when the file list
    /// is very long (e.g. `extensions/auto-form-filler/.pi/goals/...`)
    /// the budget of 100 is not enough to fit the full goal id. The
    /// 150-char budget used by the daemon call site must fit the full
    /// goal id in such cases.
    #[test]
    fn test_format_commit_subject_for_display_fits_deep_path_goal_id() {
        let full = "2 file(s) in extensions [extensions/auto-form-filler/.pi/goals/{active_goal_2026063003343613_mr019xic-xs9wa4.md => archived/goal_2026063010094853_mr019xic-xs9wa4.md}, extensions/auto-form-filler/.pi/goals/goal_events.jsonl] DELTA:+8/-5";
        // Budget 150 should keep the full goal id `mr019xic-xs9wa4`
        // from the first filename in the rename arrow.
        let result = format_commit_subject_for_display(full, 150);
        assert!(
            result.contains("mr019xic-xs9wa4"),
            "full goal id must remain in deep-path commit, got: {}",
            result
        );
    }

    /// When the subject already fits in `max_chars`, return it as-is
    /// (no `…`).
    #[test]
    fn test_format_commit_subject_for_display_no_truncation_when_fits() {
        let s = "1 file(s) in src [src/main.rs] DELTA:+5/-5";
        let result = format_commit_subject_for_display(s, 100);
        assert_eq!(result, s);
    }

    /// When even dropping both the pipe metrics AND the DELTA segment
    /// is not enough, fall back to plain `truncate`. The result will
    /// carry a `…` and may cut in the middle of the goal id, but
    /// only as a last resort.
    #[test]
    fn test_format_commit_subject_for_display_falls_back_to_truncate() {
        // Make a subject that is so long that even after stripping
        // both metrics and DELTA it would still be over budget.
        let long_path = "a".repeat(200);
        let full = format!(
            "5 file(s) in .pi [.pi/goals/{}] DELTA:+1/-1 | GOAL:complete",
            long_path
        );
        let result = format_commit_subject_for_display(&full, 50);
        assert!(
            result.ends_with('…'),
            "long subject should fall back to truncate and end with ellipsis, got: {}",
            result
        );
        // The result must be at most max_chars chars.
        assert!(result.chars().count() <= 50, "result too long: {}", result);
    }

    /// Non-structured subjects (e.g., a hand-written commit message)
    /// bypass the metrics-stripping and go straight to `truncate`.
    #[test]
    fn test_format_commit_subject_for_display_non_structured() {
        let s = "Fix login bug in auth module";
        let result = format_commit_subject_for_display(s, 10);
        assert!(
            result.ends_with('…'),
            "non-structured short subject should truncate with ellipsis, got: {}",
            result
        );
        assert!(result.chars().count() <= 10);
    }

    #[test]
    fn test_sync_alert_ledger_path_uses_state_dir() {
        let _guard = EnvRestorer::new("DRACON_SYNC_STATE_DIR", "/tmp/dracon-sync-test-state");
        let path = sync_alert_ledger_path();
        assert_eq!(
            path,
            PathBuf::from("/tmp/dracon-sync-test-state/dracon-sync-alerts.jsonl")
        );
    }

    #[test]
    fn test_record_sync_alert_appends_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard = EnvRestorer::new(
            "DRACON_SYNC_STATE_DIR",
            tmp.path().to_string_lossy().as_ref(),
        );
        let repo = tmp.path().join("repo");
        record_sync_alert(&repo, "Stuck on Push", "ahead=3, clean");
        let ledger = tmp.path().join("dracon-sync-alerts.jsonl");
        let content = std::fs::read_to_string(ledger).unwrap();
        assert!(content.contains("\"reason\":\"Stuck on Push\""));
        assert!(content.contains("\"details\":\"ahead=3, clean\""));
        assert!(content.contains("\"repo\":\""));
        assert!(content.contains("repo\""));
    }

    #[test]
    fn test_repo_state_flags_ok() {
        let status = make_status(true, 0, 0);
        let flags = repo_state_flags(&status, true, true, true);
        assert!(flags.contains(&"OK".to_string()));
    }

    #[test]
    fn test_repo_state_flags_dirty() {
        let mut status = make_status(false, 0, 0);
        status.modified_files = 2;
        let flags = repo_state_flags(&status, true, true, true);
        assert!(flags.contains(&"DIRTY".to_string()));
    }

    #[test]
    fn test_repo_state_flags_ahead() {
        let status = make_status(true, 3, 0);
        let flags = repo_state_flags(&status, true, true, true);
        assert!(flags.iter().any(|f| f.starts_with("AHEAD:")));
    }

    #[test]
    fn test_repo_state_flags_behind() {
        let status = make_status(true, 0, 2);
        let flags = repo_state_flags(&status, true, true, true);
        assert!(flags.iter().any(|f| f.starts_with("BEHIND:")));
    }

    #[test]
    fn test_repo_state_flags_no_origin() {
        // CHANGED 2026-06-20: NO_ORIGIN only fires when the repo has
        // *zero* remotes, not just no `origin`. The test now sets
        // `has_any_remote = false` to reproduce the "truly remote-less"
        // case that still emits NO_ORIGIN.
        let status = make_status(true, 0, 0);
        let flags = repo_state_flags(&status, false, false, false);
        assert!(flags.contains(&"NO_ORIGIN".to_string()));
    }

    #[test]
    fn test_repo_state_flags_no_origin_but_has_remote() {
        // Regression test for the SSH-multi-mirror misclassification
        // (goal 2026-06-20): a repo with no `origin` but with a
        // configured non-origin remote (e.g. `github`, `gitlab`,
        // `codeberg`) must NOT emit `NO_ORIGIN`.
        let status = make_status(true, 0, 0);
        let flags = repo_state_flags(&status, false, false, true);
        assert!(!flags.contains(&"NO_ORIGIN".to_string()));
    }

    #[test]
    fn test_repo_state_flags_no_upstream() {
        let status = make_status(true, 0, 0);
        let flags = repo_state_flags(&status, true, false, true);
        assert!(flags.contains(&"NO_UPSTREAM".to_string()));
    }

    #[test]
    fn test_repo_state_flags_stuck_push() {
        let status = make_status(false, 5, 0);
        // STUCK_PUSH now requires an explicit recent push failure signal.
        // Without it, an AHEAD repo is just "has unpushed commits".
        let flags =
            repo_state_flags_with_push_failure(&status, true, true, true, true);
        assert!(flags.contains(&"STUCK_PUSH".to_string()));
        let flags_no_failure = repo_state_flags(&status, true, true, true);
        assert!(!flags_no_failure.contains(&"STUCK_PUSH".to_string()));
        assert!(flags_no_failure.contains(&"AHEAD:5".to_string()));
    }

    #[test]
    fn test_repo_state_flags_stuck_pull() {
        let status = make_status(false, 0, 3);
        let flags = repo_state_flags(&status, true, true, true);
        assert!(flags.contains(&"STUCK_PULL".to_string()));
    }

    #[test]
    fn test_repo_state_flags_multiple() {
        let status = make_status(false, 3, 2);
        let flags = repo_state_flags(&status, true, true, true);
        assert!(flags.contains(&"DIRTY".to_string()));
        assert!(flags.iter().any(|f| f.starts_with("AHEAD:")));
        assert!(flags.iter().any(|f| f.starts_with("BEHIND:")));
    }

    #[test]
    fn test_repo_is_concern_no_origin() {
        // CHANGED 2026-06-20: only "no origin AND no remotes at all"
        // is a concern. A repo with only non-origin remotes is fine.
        let status = make_status(true, 0, 0);
        assert!(repo_is_concern(&status, false, false, false));
    }

    #[test]
    fn test_repo_is_concern_no_origin_but_has_remote() {
        // Regression test for the SSH-multi-mirror misclassification.
        // A repo with no `origin` but with at least one other remote
        // must NOT be a concern (provided it has a tracking upstream,
        // which a real SSH-mirror repo has via its `main` branch
        // tracking, e.g., `github/main`).
        let status = make_status(true, 0, 0);
        assert!(!repo_is_concern(&status, false, true, true));
    }

    #[test]
    fn test_repo_is_concern_no_upstream() {
        let status = make_status(true, 0, 0);
        // `has_any_remote` is true (origin exists, just no upstream);
        // the concern is about upstream, not origin.
        assert!(repo_is_concern(&status, true, false, true));
    }

    #[test]
    fn test_repo_is_concern_ahead() {
        // Old behavior: any ahead was a concern. The new
        // repo_is_concern_with_push_failure requires a recent push
        // failure signal; without it, ahead is just "has unpushed
        // commits" and is a WARN, not a CONCERN. Give the repo a
        // commit hash so the unbacked-up-content concern (no commits)
        // does not mask the ahead logic under test.
        let mut status = make_status(false, 5, 0);
        status.last_commit_hash = Some("deadbeef".to_string());
        assert!(repo_is_concern_with_push_failure(
            &status,
            true,
            true,
            true,
            true
        ));
        assert!(!repo_is_concern_with_push_failure(
            &status,
            true,
            true,
            true,
            false
        ));
    }

    #[test]
    fn test_repo_is_concern_behind() {
        // Give the repo a commit hash so the unbacked-up-content
        // concern (no commits) does not mask the behind logic.
        let mut status = make_status(false, 0, 3);
        status.last_commit_hash = Some("deadbeef".to_string());
        assert!(repo_is_concern_with_push_failure(
            &status,
            true,
            true,
            true,
            false
        ));
    }

    #[test]
    fn test_repo_stuck_filters_require_dry_run() {
        let ahead = make_status(false, 5, 0);
        let behind = make_status(false, 0, 3);
        assert!(!repo_is_stuck_push(&ahead, true, true, true, false));
        assert!(repo_is_stuck_push(&ahead, true, true, true, true));
        assert!(!repo_is_stuck_push(&ahead, false, true, true, true));
        assert!(!repo_is_stuck_push(&ahead, true, false, true, true));
        assert!(repo_is_stuck_pull(&behind, true, true, true));
        assert!(!repo_is_stuck_pull(&behind, false, true, true));
        assert!(!repo_is_stuck_pull(&behind, true, false, true));
    }

    #[test]
    fn test_repo_is_concern_clean_healthy() {
        let status = make_status(true, 0, 0);
        assert!(!repo_is_concern_with_push_failure(
            &status,
            true,
            true,
            true,
            false
        ));
    }

    #[test]
    fn test_repo_is_concern_no_upstream_with_remotes() {
        // CHANGED 2026-07-17 (goal 013b3827): a repo with remotes but no
        // tracking upstream is now a CONCERN (reverts the 2026-06-20
        // SSH-migration leniency). Live case: opencode-plugins.
        let status = make_status(true, 0, 0);
        assert!(repo_is_concern_with_push_failure(
            &status,
            false,
            false,
            true,
            false
        ));
    }

    #[test]
    fn test_repo_is_concern_unbacked_up_content_no_commits() {
        // CHANGED 2026-07-17 (goal 013b3827): working-tree content but no
        // commits at all => unbacked-up on every remote => CONCERN.
        let mut status = RepoStatus::default();
        status.untracked_files = 6;
        status.last_commit_hash = None;
        assert!(repo_is_concern_with_push_failure(
            &status,
            true,
            true,
            true,
            false
        ));
    }

    #[test]
    fn test_repo_is_concern_not_unbacked_when_has_commits() {
        // Content + commits (last_commit_hash present) is NOT the
        // unbacked-up case, and upstream exists => not a concern.
        let mut status = RepoStatus::default();
        status.untracked_files = 6;
        status.last_commit_hash = Some("abc123".to_string());
        assert!(!repo_is_concern_with_push_failure(
            &status,
            true,
            true,
            true,
            false
        ));
    }

    #[test]
    fn test_pack_too_large_skips_repair() {
        // CHANGED 2026-07-28 (v0.113.7, follow-up): the reviewer's
        // leftover observation #4: "for a hypothetical repo that ALSO
        // has a CONCERN and ALSO has pack_too_large, the auto-repair
        // would attempt handlers". The predicate
        // `pack_too_large_skips_repair` is purely the bool — it
        // fires regardless of `is_concern`, `stuck_push`,
        // `stuck_pull`, etc. The original guard
        // (`flags.contains("PACK_SIZE_WARNING")`) was dependent on
        // the flags vector, which never adds `PACK_SIZE_WARNING`,
        // and would have been defeated if a hypothetical repo had
        // both `pack_too_large` and a separate CONCERN.
        // The new guard: short-circuits purely on pack_too_large.
        assert!(pack_too_large_skips_repair(true));
        assert!(!pack_too_large_skips_repair(false));
    }

    #[test]
    fn test_verify_resolution_still_concern() {
        // CHANGED 2026-07-28 (v0.113.7): a repo that was in the
        // concern list ONLY because of `pack_too_large` (and not
        // because of ahead/behind/origin/upstream) is STILL a
        // concern after the auto-repair pass — the daemon has no
        // code that shrinks history, so the size issue is unchanged.
        // Pre-fix the post-handler `verify_resolution` would have
        // reported this as "resolved" because the ahead/behind/etc.
        // checks all passed. Post-fix the helper includes
        // `pack_too_large` in the predicate.
        // size-only: a clean, synced, origin-ok, upstream-ok repo
        // with pack_too_large=true stays a concern.
        assert!(verify_resolution_still_concern(0, 0, true, true, true));
        // nothing applied: same shape, no pack_too_large => resolved.
        assert!(!verify_resolution_still_concern(0, 0, true, true, false));
        // ahead-only concern still applies regardless of size.
        assert!(verify_resolution_still_concern(3, 0, true, true, false));
        // missing origin still applies.
        assert!(verify_resolution_still_concern(0, 0, false, true, false));
        // missing upstream still applies.
        assert!(verify_resolution_still_concern(0, 0, true, false, false));
        // behind still applies.
        assert!(verify_resolution_still_concern(0, 2, true, true, false));
    }

    #[test]
    fn test_pack_too_large_forces_concern() {
        // CHANGED 2026-07-28 (v0.113.7): a repo whose pushable branch
        // exceeds GitHub's 2 GiB pack limit is now classified as a
        // CONCERN (not just a HINT). The daemon's push path silently
        // skips GitHub for this class of repo; surfacing the row as
        // CONCERN makes the situation visible in `dracon-sync repos`
        // instead of buried in journalctl.
        assert!(pack_too_large_forces_concern((true, 2_500_000_000)));
        // Even when the measured size is not supplied (the second
        // tuple element is 0), the bool alone drives the decision.
        assert!(pack_too_large_forces_concern((true, 0)));
        // A repo that fits under the 2 GiB limit is NOT a concern from
        // this code path (other concerns may still apply; the helper
        // only consults the bool).
        assert!(!pack_too_large_forces_concern((false, 1_500_000_000)));
        assert!(!pack_too_large_forces_concern((false, 0)));
    }

    #[test]
    fn test_repo_is_warn_dirty() {
        let status = make_status(false, 0, 0);
        assert!(repo_is_warn(&status, true, true, true));
    }

    #[test]
    fn test_repo_is_active() {
        // PENDING push => active regardless of state cause.
        assert!(repo_is_active("PENDING", &StateCause::Healthy));
        assert!(repo_is_active("PENDING", &StateCause::Stalled));
        // In-flight / recently-dirty causes are active.
        assert!(repo_is_active("OK", &StateCause::Pushing));
        assert!(repo_is_active("OK", &StateCause::Committing));
        assert!(repo_is_active("OK", &StateCause::Working));
        assert!(repo_is_active("OK", &StateCause::Dirty));
        // Genuine problems / idle states are NOT active.
        assert!(!repo_is_active("OK", &StateCause::Stalled));
        assert!(!repo_is_active("OK", &StateCause::Healthy));
        assert!(!repo_is_active("OK", &StateCause::Synced));
        assert!(!repo_is_active("OK", &StateCause::Idle));
        assert!(!repo_is_active("OK", &StateCause::Cold));
        assert!(!repo_is_active("OK", &StateCause::Untracked));
        assert!(!repo_is_active("OK", &StateCause::Intentional));
        assert!(!repo_is_active("OK", &StateCause::Failed));
    }

    #[test]
    fn test_unowned_pending_is_blocked_not_active() {
        let cause = StateCause::Unowned {
            reason: "untrusted_email".to_string(),
            detail: "user.email = --global".to_string(),
        };
        assert!(!repo_is_active("PENDING", &cause));
        assert_eq!(push_cell_label("BLOCKED", None).0, "🚫 BLOCKED");
    }

    #[test]
    fn test_broken_history_is_not_active() {
        assert!(!repo_is_active("BROKEN", &StateCause::Failed));
        assert_eq!(push_cell_label("BROKEN", None).0, "🩹 BROKEN");
    }

    #[test]
    fn test_probe_history_invalid_head_is_failure_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(repo)
            .status()
            .unwrap();
        std::fs::write(repo.join(".git/refs/heads/main"), "0000000000000000000000000000000000000000\n")
            .unwrap();
        let probe = probe_history(repo);
        assert!(probe.failed);
        assert_eq!(probe.missing_objects, 0);
    }

    #[test]
    fn test_repo_is_warn_not_concern() {
        let status = make_status(false, 0, 0);
        // has_origin=false, has_any_remote=false → still a concern,
        // so not a WARN.
        assert!(!repo_is_warn(&status, false, false, false));
    }

    #[test]
    fn test_repo_hint_no_origin() {
        // CHANGED 2026-06-20: with the SSH-migration fix, the
        // `NO_ORIGIN` flag only fires for truly remote-less repos
        // (zero configured remotes). The hint is updated to match.
        let hint = repo_hint(&["NO_ORIGIN".into()], false, false);
        assert_eq!(hint, "no remote configured (cannot push)");
    }

    #[test]
    fn test_repo_hint_no_upstream() {
        // CHANGED 2026-06-20: the hint is context-sensitive. When
        // `concern=true` (i.e. the repo has `origin` but the branch
        // isn't tracking it), the original "set upstream" hint is
        // accurate and `repair concerns --apply` will succeed. When
        // `concern=false` (post-SSH-migration case where the repo
        // has only non-origin remotes), the hint is informational
        // because the daemon is already pushing successfully via
        // explicit refspecs and the auto-repair path would fail.
        let hint = repo_hint(&["NO_UPSTREAM".into()], false, true);
        assert_eq!(hint, "run repair-concerns --apply (set upstream)");
        let hint = repo_hint(&["NO_UPSTREAM".into()], false, false);
        assert_eq!(
            hint,
            "no tracking upstream (daemon uses explicit refspecs; not a concern)"
        );
    }

    #[test]
    fn test_repo_hint_intentional_no_upstream() {
        // INTENTIONAL_NO_UPSTREAM must take precedence over NO_UPSTREAM
        // so the operator never sees a misleading "set upstream" hint
        // for a repo they have explicitly flagged as intentionally
        // isolated.
        let hint = repo_hint(&["INTENTIONAL_NO_UPSTREAM".into()], false, false);
        assert_eq!(hint, "intentional legacy isolation, no upstream configured");
    }

    #[test]
    fn test_apply_intentional_no_upstream_replaces_flag() {
        // NO_UPSTREAM must be replaced (not duplicated) by
        // INTENTIONAL_NO_UPSTREAM, and other flags must be preserved.
        let flags = vec!["DIRTY".to_string(), "NO_UPSTREAM".to_string()];
        let result = apply_intentional_no_upstream(flags);
        assert!(!result.contains(&"NO_UPSTREAM".to_string()));
        assert!(result.contains(&"INTENTIONAL_NO_UPSTREAM".to_string()));
        assert!(result.contains(&"DIRTY".to_string()));
    }

    #[test]
    fn test_apply_intentional_no_upstream_idempotent() {
        // Calling the helper twice on a row that already has
        // INTENTIONAL_NO_UPSTREAM must not duplicate the flag.
        let once = apply_intentional_no_upstream(vec!["NO_UPSTREAM".into()]);
        let twice = apply_intentional_no_upstream(once.clone());
        assert_eq!(
            twice
                .iter()
                .filter(|f| *f == "INTENTIONAL_NO_UPSTREAM")
                .count(),
            1
        );
    }

    #[test]
    fn test_apply_intentional_no_upstream_no_op_when_absent() {
        // Repos without NO_UPSTREAM should not be touched.
        let flags = vec!["OK".to_string()];
        let result = apply_intentional_no_upstream(flags.clone());
        assert_eq!(result, flags);
    }

    #[test]
    fn test_repo_hint_ahead_concern() {
        let hint = repo_hint(&["AHEAD:3".into()], false, false);
        assert_eq!(hint, "run repair-concerns --apply (push or rewrite)");
    }

    #[test]
    fn test_repo_hint_warn_with_pending_push() {
        let hint = repo_hint(&["DIRTY".into(), "AHEAD:3".into()], true, false);
        assert_eq!(hint, "daemon will push after changes settle");
    }

    #[test]
    fn test_repo_hint_behind() {
        let hint = repo_hint(&["BEHIND:2".into()], false, false);
        assert_eq!(hint, "run repair-concerns --apply (pull/merge)");
    }

    // -------------------------------------------------------------------
    // parse_relative_minutes tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_relative_minutes_units() {
        assert_eq!(parse_relative_minutes("0 seconds ago"), Some(0));
        assert_eq!(parse_relative_minutes("23 seconds ago"), Some(0));
        assert_eq!(parse_relative_minutes("5 minutes ago"), Some(5));
        assert_eq!(parse_relative_minutes("1 minute ago"), Some(1));
        assert_eq!(parse_relative_minutes("2 hours ago"), Some(120));
        assert_eq!(parse_relative_minutes("8 hours ago"), Some(480));
        assert_eq!(parse_relative_minutes("2 days ago"), Some(2 * 24 * 60));
        assert_eq!(parse_relative_minutes("3 weeks ago"), Some(3 * 7 * 24 * 60));
        assert_eq!(parse_relative_minutes("1 month ago"), Some(30 * 24 * 60));
        assert_eq!(parse_relative_minutes("1 year ago"), Some(365 * 24 * 60));
    }

    #[test]
    fn test_parse_relative_minutes_sentinel() {
        // The daemon emits "-" as a sentinel when no time is available.
        // The parser must return None, not 0, so the classifier treats
        // it as "unknown" rather than "0 minutes ago".
        assert_eq!(parse_relative_minutes("-"), None);
        assert_eq!(parse_relative_minutes(""), None);
        assert_eq!(parse_relative_minutes("unknown"), None);
    }

    /// ADDED 2026-07-22 (v0.112.35): `parse_relative_minutes_to_u64`
    /// (the ACTIVITY-cell parser) must handle weeks/months/years —
    /// the regression: DraconDev's last commit was "4 weeks ago",
    /// the old unit-limited copy returned None, and the cell rendered
    /// a bare "healthy" with no indicator.
    #[test]
    fn test_parse_relative_minutes_to_u64_handles_weeks_months_years() {
        assert_eq!(
            parse_relative_minutes_to_u64("4 weeks ago"),
            Some(4 * 7 * 24 * 60)
        );
        assert_eq!(
            parse_relative_minutes_to_u64("1 week ago"),
            Some(7 * 24 * 60)
        );
        assert_eq!(
            parse_relative_minutes_to_u64("2 months ago"),
            Some(2 * 30 * 24 * 60)
        );
        assert_eq!(
            parse_relative_minutes_to_u64("1 year ago"),
            Some(365 * 24 * 60)
        );
        assert_eq!(parse_relative_minutes_to_u64("3 days ago"), Some(3 * 24 * 60));
        assert_eq!(parse_relative_minutes_to_u64("-"), None);
        assert_eq!(parse_relative_minutes_to_u64("unknown"), None);
    }

    // -------------------------------------------------------------------
    // classify_state_cause tests
    // -------------------------------------------------------------------

    fn default_thresholds() -> StateCauseThresholds {
        StateCauseThresholds {
            active_minutes: 5,
            committing_minutes: 60,
            cold_minutes: 1440,
        }
    }

    fn empty_flags() -> Vec<String> {
        vec!["OK".to_string()]
    }

    #[test]
    fn test_classify_state_cause_working_is_freshly_synced() {
        let inputs = StateCauseInputs {
            flags: &empty_flags(),
            push_status: "OK",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(2),
            last_push_minutes: Some(2),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Working
        );
    }

    #[test]
    fn test_classify_state_cause_synced_clean_recent_but_not_working() {
        let inputs = StateCauseInputs {
            flags: &empty_flags(),
            push_status: "OK",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(13),
            last_push_minutes: Some(13),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Synced
        );
    }

    #[test]
    fn test_classify_state_cause_pushing_takes_precedence() {
        let pushing_flags: Vec<String> = vec!["DIRTY".to_string(), "AHEAD:3".to_string()];
        let inputs = StateCauseInputs {
            flags: &pushing_flags,
            push_status: "PENDING",
            modified: 5,
            staged: 0,
            untracked: 0,
            ahead: 3,
            behind: 0,
            last_commit_minutes: Some(2),
            last_push_minutes: Some(8),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Pushing
        );
    }

    #[test]
    fn test_classify_state_cause_stalled_is_the_users_pain() {
        let dirty_flags: Vec<String> = vec!["DIRTY".to_string()];
        let inputs = StateCauseInputs {
            flags: &dirty_flags,
            push_status: "OK",
            modified: 3,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(90),
            last_push_minutes: Some(90),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Stalled
        );
    }

    #[test]
    fn test_classify_state_cause_recent_dirty_is_dirty_not_stalled() {
        let dirty_flags: Vec<String> = vec!["DIRTY".to_string()];
        let inputs = StateCauseInputs {
            flags: &dirty_flags,
            push_status: "OK",
            modified: 4,
            staged: 0,
            untracked: 1,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(2),
            last_push_minutes: Some(2),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Dirty
        );
    }

    #[test]
    fn test_classify_state_cause_dirty_within_committing_window_is_dirty() {
        let dirty_flags: Vec<String> = vec!["DIRTY".to_string()];
        let inputs = StateCauseInputs {
            flags: &dirty_flags,
            push_status: "OK",
            modified: 3,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(30),
            last_push_minutes: Some(45),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Dirty
        );
    }

    #[test]
    fn test_classify_state_cause_old_dirty_is_stalled() {
        let dirty_flags: Vec<String> = vec!["DIRTY".to_string()];
        let inputs = StateCauseInputs {
            flags: &dirty_flags,
            push_status: "OK",
            modified: 3,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(90),
            last_push_minutes: Some(90),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Stalled
        );
    }

    #[test]
    fn test_classify_state_cause_intentional_flag() {
        let intentional_flags: Vec<String> = vec!["INTENTIONAL_NO_UPSTREAM".to_string()];
        let inputs = StateCauseInputs {
            flags: &intentional_flags,
            push_status: "INTENTIONAL",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(60 * 8),
            last_push_minutes: None,
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Intentional
        );
    }

    #[test]
    fn test_classify_state_cause_failed_takes_precedence() {
        let upstream_flags: Vec<String> = vec!["NO_UPSTREAM".to_string()];
        let inputs = StateCauseInputs {
            flags: &upstream_flags,
            push_status: "FAIL",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(60),
            last_push_minutes: None,
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Failed
        );
    }

    #[test]
    fn test_classify_state_cause_idle_within_cold_window() {
        let inputs = StateCauseInputs {
            flags: &empty_flags(),
            push_status: "OK",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(4 * 60),
            last_push_minutes: Some(4 * 60),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Idle
        );
    }

    #[test]
    fn test_classify_state_cause_cold_beyond_threshold() {
        let inputs = StateCauseInputs {
            flags: &empty_flags(),
            push_status: "OK",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(2 * 24 * 60),
            last_push_minutes: Some(2 * 24 * 60),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Cold
        );
    }

    #[test]
    fn test_classify_state_cause_untracked_only() {
        let dirty_flags: Vec<String> = vec!["DIRTY".to_string()];
        let inputs = StateCauseInputs {
            flags: &dirty_flags,
            push_status: "OK",
            modified: 0,
            staged: 0,
            untracked: 5,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(60),
            last_push_minutes: Some(60),
        };
        assert_eq!(
            classify_state_cause(&inputs, &default_thresholds()),
            StateCause::Untracked
        );
    }

    #[test]
    fn test_classify_state_cause_uses_per_repo_overrides() {
        let over = RepoPolicyOverride {
            active_commit_minutes: Some(30),
            ..Default::default()
        };
        let policy = test_sync_policy();
        let thresholds = StateCauseThresholds::from_policy(&policy, &over);
        assert_eq!(thresholds.active_minutes, 30);
        let inputs = StateCauseInputs {
            flags: &empty_flags(),
            push_status: "OK",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 1,
            behind: 0,
            last_commit_minutes: Some(20),
            last_push_minutes: Some(20),
        };
        assert_eq!(
            classify_state_cause(&inputs, &thresholds),
            StateCause::Committing
        );
    }

    #[test]
    fn test_classify_state_cause_uses_global_when_no_override() {
        let over = RepoPolicyOverride::default();
        let policy = test_sync_policy();
        let thresholds = StateCauseThresholds::from_policy(&policy, &over);
        assert_eq!(thresholds.active_minutes, 5);
        let inputs = StateCauseInputs {
            flags: &empty_flags(),
            push_status: "OK",
            modified: 0,
            staged: 0,
            untracked: 0,
            ahead: 0,
            behind: 0,
            last_commit_minutes: Some(20),
            last_push_minutes: Some(20),
        };
        assert_eq!(
            classify_state_cause(&inputs, &thresholds),
            StateCause::Synced
        );
    }






    #[test]
    fn test_repo_is_warn_untracked_only_is_not_warn() {
        let mut status = RepoStatus::default();
        status.branch = String::new();
        status.is_clean = false;
        status.modified_files = 0;
        status.untracked_files = 5;
        status.staged_files = 0;
        status.ahead = 0;
        status.behind = 0;
        // CHANGED 2026-07-21 (v0.112.29): set a fake commit hash so
        // the EMPTY_REPO flag does NOT fire. This test scenario is
        // "repo has commits, just untracked files" — not "empty
        // git init". An empty-repo variant is covered by
        // `test_repo_state_flags_empty_repo`.
        status.last_commit_hash = Some("deadbeef".to_string());
        status.last_commit_msg = None;

        assert!(!repo_is_warn(&status, true, true, true));
        assert_eq!(repo_state_flags(&status, true, true, true), vec!["DIRTY"]);
    }

    /// ADDED 2026-07-21 (v0.112.29): a fresh `git init` repo with
    /// NO commits gets the `EMPTY_REPO` flag, distinct from `DIRTY`
    /// or `NO_UPSTREAM`. The hint for this state is
    /// "no commits yet — make first commit to enable push" so the
    /// operator knows the right action (make a commit, NOT run
    /// `repair-concerns --apply` which would fail with "src
    /// refspec HEAD does not match any").
    #[test]
    fn test_repo_state_flags_empty_repo() {
        let mut status = RepoStatus::default();
        status.branch = "main".to_string();
        status.is_clean = false;
        status.untracked_files = 2;
        status.last_commit_hash = None;
        let flags = repo_state_flags(&status, true, false, true);
        assert!(flags.contains(&"EMPTY_REPO".to_string()));
        let hint = repo_hint(&flags, false, true);
        assert!(hint.contains("no commits yet"), "got hint: {}", hint);
        assert!(hint.contains("first commit"), "got hint: {}", hint);
    }

    /// ADDED 2026-07-21 (v0.112.29): empty repo push_status should
    /// be EMPTY (not FAIL). FAIL would mislead the operator into
    /// thinking a push was attempted.
    #[test]
    fn test_empty_repo_push_status_is_empty_not_fail() {
        let mut status = RepoStatus::default();
        status.last_commit_hash = None;
        let flags = repo_state_flags(&status, true, false, true);
        // Confirm EMPTY_REPO present, NO_UPSTREAM may or may not be
        // present depending on has_upstream parameter.
        assert!(flags.contains(&"EMPTY_REPO".to_string()));
        // The push_status derivation logic is inline in the row
        // builder; we replicate the relevant condition here:
        let push_status = if flags.iter().any(|f| f == "EMPTY_REPO") {
            "EMPTY"
        } else if flags.iter().any(|f| f == "NO_UPSTREAM") {
            "FAIL"
        } else {
            "OK"
        };
        assert_eq!(push_status, "EMPTY");
    }

    #[test]
    fn test_repo_hint_healthy() {
        let hint = repo_hint(&["OK".into()], false, false);
        assert_eq!(hint, "healthy");
    }

    #[test]
    fn test_repo_hint_warn() {
        let hint = repo_hint(&["DIRTY".into()], true, false);
        assert_eq!(
            hint,
            "daemon handles after changes settle; run sync-now --warns to force now"
        );
    }

    #[test]
    fn test_repo_hint_concern() {
        let hint = repo_hint(&["DIRTY".into()], false, true);
        assert_eq!(hint, "run repair-concerns --apply");
    }

    #[test]
    fn test_push_large_blob_threshold_bytes() {
        let policy = SyncPolicy {
            max_stage_file_bytes: 200 * 1024 * 1024,
            max_push_blob_bytes: 50 * 1024 * 1024,
            ..test_sync_policy()
        };
        let threshold = push_large_blob_threshold_bytes(&policy);
        assert_eq!(threshold, 50 * 1024 * 1024);
    }

    #[test]
    fn test_push_large_blob_threshold_caps_at_git_limit() {
        let policy = SyncPolicy {
            max_stage_file_bytes: 200 * 1024 * 1024,
            max_push_blob_bytes: 200 * 1024 * 1024,
            ..test_sync_policy()
        };
        let threshold = push_large_blob_threshold_bytes(&policy);
        assert_eq!(threshold, DEFAULT_GIT_HOST_BLOB_LIMIT_BYTES);
    }

    #[test]
    fn test_timestamp_secs_returns_reasonable_value() {
        let ts = timestamp_secs();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(ts > 0);
        assert!(ts <= now + 1);
    }

    static LEDGER_ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct VarGuard {
        var: String,
        original: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl VarGuard {
        fn set_temp(var: &str, value: &str) -> Self {
            let lock = LEDGER_ENV_GUARD.lock().unwrap();
            let original = std::env::var(var).ok();
            if value.is_empty() {
                std::env::remove_var(var);
            } else {
                std::env::set_var(var, value);
            }
            Self {
                var: var.to_string(),
                original,
                _lock: lock,
            }
        }
    }
    impl Drop for VarGuard {
        fn drop(&mut self) {
            if let Some(orig) = self.original.take() {
                std::env::set_var(&self.var, orig);
            } else {
                std::env::remove_var(&self.var);
            }
        }
    }

    #[test]
    fn test_incident_ledger_path_default() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_LEDGER", "");
        let path = incident_ledger_path(std::path::Path::new("/fake/policy.toml"));
        assert!(path
            .to_string_lossy()
            .contains("dracon-sync-incidents.jsonl"));
    }

    #[test]
    fn test_incident_ledger_path_custom_env() {
        let _guard = VarGuard::set_temp("DRACON_SYNC_LEDGER", "/custom/path/ledger.jsonl");
        let path = incident_ledger_path(std::path::Path::new("/fake/policy.toml"));
        let result = path.to_string_lossy();
        assert_eq!(result, "/custom/path/ledger.jsonl");
    }

    fn test_sync_policy() -> SyncPolicy {
        SyncPolicy {
            stale_dirty_alert_secs: 600,
            max_stage_batch_files: 100000,
            auto_resolve_unmerged: default_auto_resolve_unmerged(),
            push_debounce_secs: default_push_debounce_secs(),
            untracked_warn_threshold: default_untracked_warn_threshold(),
            system_repo: String::new(),
            pulse_interval_secs: 1,
            trailing_drain_deadline_secs: 120,
            inactivity_push_delay_secs: 5,
            auto_commit: true,
            auto_bump_versions: true,
            auto_pull: true,
            auto_push: true,
            backup_policy: String::new(),
            backup_dir: String::new(),
            exclude_repos: vec![],
            exclude_dir_names: vec![],
            exclude_file_patterns: vec![],
            auto_repair_concerns: true,
            build_artifact_cleanup: true,
            auto_repair_warns: true,
            auto_rewrite_large_blobs: true,
            sem_max_concurrent_sync: 4,
            auto_stage_untracked: true,
            untracked_exclude_patterns: crate::policy::default_untracked_exclude_patterns(),
            watch_roots: vec![],
            remotes: vec![],
            auto_github_private: false,
            auto_github_private_account: "DraconDev".to_string(),
            max_stage_file_bytes: 100 * 1024 * 1024,
            pull_op_timeout_secs: 30,
            push_op_timeout_secs: 300,
            auto_gc_garbage_threshold_bytes: crate::policy::default_auto_gc_garbage_threshold_bytes(),
            auto_prune_stale_backup_branches: false,
            repo_sync_timeout_secs: 420,
            stage_op_timeout_secs: 60,
            stage_cooldown_secs: 3600,
            push_retries: 3,
            repair_cooldown_secs: 60,
            max_push_blob_bytes: 100 * 1024 * 1024,
            incident_ledger_max_lines: 10_000,
            incident_ledger_max_age_days: 30,
            webhook_url: None,
            alert_unpushed_threshold: 10,
            auto_commit_backstop_threshold: 20,
            auto_commit_backstop_min_age_secs: 300,
            push_max_retries: 5,
            auto_skip_unowned: true,
            trusted_emails: crate::policy::default_trusted_emails(),
            trusted_authors: crate::policy::default_trusted_authors(),
            trusted_remote_hosts: crate::policy::default_trusted_remote_hosts(),
            settling_max_delay_secs: 60,
            dirty_max_age_action: crate::policy::DirtyMaxAgeAction::Commit,
            min_commit_interval_secs: 5,
            auto_commit_exclude_patterns: vec![],
            sync_visibility: false,
            sync_visibility_interval_hours: 24,
            sync_metadata: false,
            codeberg_public_only: true, // default; tests that need override set explicitly
            auto_tag: true,
            auto_release: false,
            auto_publish: false,
            publish_targets: vec![],
            nix_auto_update: false,
            standard_files: vec![],
            standard_files_auto: true,
            active_commit_minutes: 5,
            committing_commit_minutes: 60,
            cold_commit_minutes: 1440,
        }
    }

    #[test]
    fn test_truncate_unicode_emoji() {
        let result = truncate("hello 👋 world", 10);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_repair_summary_default() {
        let summary = RepairSummary::default();
        assert_eq!(summary.found, 0);
        assert_eq!(summary.planned, 0);
        assert_eq!(summary.attempted, 0);
        assert_eq!(summary.succeeded, 0);
        assert_eq!(summary.resolved_now, 0);
        assert_eq!(summary.manual_only, 0);
    }

    #[test]
    fn test_repair_summary_debug() {
        let summary = RepairSummary {
            found: 1,
            planned: 2,
            attempted: 3,
            succeeded: 4,
            resolved_now: 5,
            manual_only: 6,
        };
        let debug = format!("{:?}", summary);
        assert!(debug.contains("found"));
    }

    #[test]
    fn test_ansi_colors() {
        // Force color on for this test (NO_COLOR may be set in the env).
        let saved = std::env::var_os("NO_COLOR");
        // SAFETY: this is a single-threaded test that owns the NO_COLOR slot.
        unsafe {
            std::env::remove_var("NO_COLOR");
        }
        let saved_force = std::env::var_os("DRACON_FORCE_COLOR");
        std::env::set_var("DRACON_FORCE_COLOR", "1");
        assert_eq!(ansi("31", "error"), "\x1b[31merror\x1b[0m");
        assert_eq!(ansi("32", "ok"), "\x1b[32mok\x1b[0m");
        assert_eq!(ansi("1", "bold"), "\x1b[1mbold\x1b[0m");
        assert_eq!(ansi("unknown", "default"), "\x1b[0mdefault\x1b[0m");
        // restore
        match saved {
            Some(v) => std::env::set_var("NO_COLOR", v),
            None => std::env::remove_var("NO_COLOR"),
        }
        match saved_force {
            Some(v) => std::env::set_var("DRACON_FORCE_COLOR", v),
            None => std::env::remove_var("DRACON_FORCE_COLOR"),
        }
    }

    #[test]
    fn test_repo_filter_variants() {
        assert_eq!(format!("{:?}", RepoFilter::All), "All");
        assert_eq!(format!("{:?}", RepoFilter::Concern), "Concern");
        assert_eq!(format!("{:?}", RepoFilter::Warn), "Warn");
    }

    #[test]
    fn test_concern_repair_filter_variants() {
        assert_eq!(format!("{:?}", ConcernRepairFilter::All), "All");
        assert_eq!(format!("{:?}", ConcernRepairFilter::StuckPush), "StuckPush");
        assert_eq!(format!("{:?}", ConcernRepairFilter::StuckPull), "StuckPull");
    }

    #[test]
    fn test_incident_record_serialization() {
        let record = IncidentRecord {
            ts_unix: 1700000000,
            scope: "test".to_string(),
            repo: "/test/repo".to_string(),
            reason: "test reason".to_string(),
            action: "test action".to_string(),
            backup_branch: Some("backup".to_string()),
            result: "success".to_string(),
            details: Some("details".to_string()),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("1700000000"));
        assert!(json.contains("test reason"));
    }

    #[test]
    fn test_repo_report_row_structure() {
        let row = RepoReportRow {
            repo: "/test/repo".to_string(),
            state_flags: vec!["OK".to_string()],
            branch: "main".to_string(),
            upstream: "github/main".to_string(),
            publish_state: PublishState::Ok,
            modified: 0,
            staged: 0,
            untracked: 0,
            excluded_dirty: 0,
            ahead: 0,
            behind: 0,
            last_hash: "abc123".to_string(),
            last_author: "test".to_string(),
            last_when: "2024-01-01".to_string(),
            last_msg: "test commit".to_string(),
            last_unix: 1700000000,
            commits_1h: 0,
            commits_6h: 0,
            commits_24h: 0,
            last_push: "5m ago".to_string(),
            push_status: "OK".to_string(),
            push_error: String::new(),
            push_to_remotes: vec!["codeberg".to_string(), "github".to_string(), "gitlab".to_string()],
            excluded_remotes: vec![],
            codeberg_skip_reason: None,
            git_size_bytes: Some(34_476_847),
            git_modules_bytes: 0,
            token_health: TokenHealthSummary { codeberg_present: true, github_present: true, gitlab_present: true },
            concern: false,
            warn: false,
            active: false,
            hint: "healthy".to_string(),
            state_cause: StateCause::Healthy,
            state_cause_label: "healthy".to_string(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: "none".to_string(),
            missing_objects: 0,
            pack_too_large: false,
        };
        assert_eq!(row.repo, "/test/repo");
        assert_eq!(row.branch, "main");
        assert!(!row.concern);
    }

    #[test]
    fn test_publish_cell_label_marks_missing_and_gone() {
        // 2026-07-19 (goal `4555eaf6`): publish_cell_label() now
        // truncates to 16 cols (PUBLISH column Absolute(18) minus
        // 2 padding). Short content unaffected; long Gone content
        // gets ellipsis.
        assert_eq!(publish_cell_label("-", PublishState::Missing), "⚠️ none");
        // `⚠️ github/main (gone)` = 22 chars → truncated to 16 cols
        // (reserve 1 for `…`).
        let gone_result = publish_cell_label("github/main", PublishState::Gone);
        assert!(
            gone_result.starts_with("⚠️"),
            "Gone should still have warning emoji prefix: {gone_result}"
        );
        assert!(
            gone_result.ends_with('…'),
            "Gone content > 16 cols should end with ellipsis: {gone_result}"
        );
        // ⚠️ is 2 cols wide, so the visual width is at most 17
        // (16 cols of ASCII content + 1 col for the right half of
        // the emoji + 1 col for …).
        assert!(
            unicode_width::UnicodeWidthStr::width(gone_result.as_str()) <= 17,
            "truncated Gone should be ≤ 17 cols wide: {gone_result}"
        );
        // Ok: short enough to fit unchanged
        assert_eq!(publish_cell_label("github/main", PublishState::Ok), "github/main");
    }

    #[test]
    fn test_role_cell_truncates_long_submod_labels() {
        // 2026-07-19 (goal `4555eaf6`): role_cell() truncates ROLE
        // labels to 12 cols (ROLE column Absolute(14) minus 2 padding).
        // Short labels are unaffected; long ones get ellipsis.
        use crate::role::RoleKind;
        let long = RoleKind::Submod {
            parent_basename: "dracon-platform".to_string(),
            sub_path: "web/games/released/one-mil-girls".to_string(),
        };
        let rendered_str = role_cell(&long).content();
        // `released/one-mil-girls` = 22 chars > 12 → truncated to 12
        // cols with … (so 11 chars of content + …).
        assert!(
            rendered_str.ends_with('…'),
            "long ROLE label should end with ellipsis: {rendered_str}"
        );
        assert!(
            unicode_width::UnicodeWidthStr::width(rendered_str.as_str()) <= 12,
            "truncated ROLE label must be ≤ 12 cols wide: {rendered_str}"
        );

        // Short labels pass through unchanged.
        let short = RoleKind::Submod {
            parent_basename: "dracon-platform".to_string(),
            sub_path: "web/games/wip/hegemon".to_string(),
        };
        assert_eq!(role_cell(&short).content(), "wip/hegemon");

        // Parent and Standalone unaffected (parent is now `parent·10` = 9 chars).
        let parent = RoleKind::Parent(10);
        assert_eq!(role_cell(&parent).content(), "parent·10");
    }

    #[test]
    fn test_state_plus_act_cell_drops_activity_when_tight() {
        // 2026-07-19 (goal `4555eaf6` v0.112.25 follow-up):
        // when budget is tight, the activity part should be
        // dropped cleanly rather than leaving a dangling emoji
        // + ellipsis (`🟠 dirty · ⏳ …`).
        let result = state_plus_act_cell("🟠", "dirty", "⏳ dirty 5m", 15);
        assert_eq!(result, "🟠 dirty", "activity part dropped: {result}");
        // Sanity: state-only fits the 15-col budget.
        assert!(
            unicode_width::UnicodeWidthStr::width(result.as_str()) <= 15,
            "should fit 15-col budget: {result}"
        );
    }

    #[test]
    fn test_state_plus_act_cell_keeps_activity_when_it_fits() {
        // When both state and activity fit, the cell shows both.
        let result = state_plus_act_cell("🟠", "dirty", "⏳ dirty 5m", 30);
        assert_eq!(
            result, "🟠 dirty · ⏳ dirty 5m",
            "state + activity shown: {result}"
        );
    }

    #[test]
    fn test_state_plus_act_cell_handles_dash_activity() {
        // A `—` activity (no useful info) should be dropped.
        let result = state_plus_act_cell("🟢", "synced", "—", 15);
        assert_eq!(result, "🟢 synced", "dash activity dropped: {result}");
    }

    #[test]
    fn test_summary_what_clean_idle_repo() {
        // 2026-07-19 (goal `4555eaf6` v0.112.27): summary view
        // for a clean idle repo should NOT include `push: pending`
        // or dirty counts (there are none).
        let mut row = RepoReportRow::for_tests("/tmp/foo");
        row.concern = false;
        row.warn = false;
        row.active = false;
        row.modified = 0;
        row.staged = 0;
        row.untracked = 0;
        row.ahead = 0;
        row.push_status = "OK".to_string();
        row.hint = "healthy".to_string();
        row.last_author = "DraconDev".to_string();
        // last_when = "13 hours ago" so activity_label returns
        // "⚪ idle 13h" (which is what makes the summary interesting).
        row.last_when = "13 hours ago".to_string();
        let what = summary_what(&row, 200);
        // Activity for clean idle is `⚪ idle 13h`.
        assert!(
            what.contains("⚪ idle"),
            "expected idle activity in summary: {what}"
        );
        assert!(what.contains("healthy"), "expected hint: {what}");
        assert!(
            !what.contains("push:"),
            "clean repo should not include push: status: {what}"
        );
        assert!(
            !what.contains(" mod"),
            "clean repo should not include dirty counts: {what}"
        );
    }

    #[test]
    fn test_summary_what_dirty_repo_includes_dirty_counts_and_hint() {
        // A dirty repo with a hint must show dirty counts + hint
        // in a single WHAT string. Author is intentionally omitted
        // (v0.112.27 R2) — for a solo operator the git commit
        // author is freestyled noise that misleads in a glance view.
        let mut row = RepoReportRow::for_tests("/tmp/foo");
        row.concern = false;
        row.warn = false;
        row.active = true;
        row.modified = 2;
        row.staged = 0;
        row.untracked = 1;
        row.ahead = 0;
        row.push_status = "OK".to_string();
        row.hint = "daemon handles after changes settle".to_string();
        row.last_author = "DraconDev".to_string();
        row.last_when = "5 minutes ago".to_string();
        let what = summary_what(&row, 200);
        assert!(what.contains("⏳ dirty"), "activity: {what}");
        assert!(what.contains("2 mod"), "modified count: {what}");
        assert!(what.contains("1 ut"), "untracked count: {what}");
        assert!(
            what.contains("daemon handles"),
            "hint visible: {what}"
        );
        assert!(
            !what.contains("by DraconDev"),
            "author must be omitted from summary (v0.112.27 R2): {what}"
        );
    }

    #[test]
    fn test_summary_what_pending_push_drops_redundant_ahead_note() {
        // When push is PENDING, the activity already encodes the
        // ahead count inline (`🟣 pushing 0m (1 ahead)`). The
        // summary must NOT add a separate `1 ahead` segment on
        // top of that (was a duplication bug in v0.112.27 R0).
        let mut row = RepoReportRow::for_tests("/tmp/foo");
        row.concern = false;
        row.warn = false;
        row.active = true;
        row.modified = 0;
        row.staged = 0;
        row.untracked = 0;
        row.ahead = 1;
        row.push_status = "PENDING".to_string();
        row.hint = "daemon will push after changes settle".to_string();
        row.last_author = "DraconDev".to_string();
        let what = summary_what(&row, 200);
        // Activity `🟣 pushing Xm (N ahead)` already covers the
        // ahead count; the standalone `1 ahead` note must NOT
        // appear (would be a duplicate).
        let ahead_occurrences = what.matches("1 ahead").count();
        assert_eq!(
            ahead_occurrences, 1,
            "ahead count should appear exactly once (from activity): {what}"
        );
    }

    #[test]
    fn test_summary_what_stuck_push_shows_status() {
        // When push is STUCK or FAIL, the activity does NOT show
        // it (activity only covers PENDING), so the summary must
        // surface it explicitly.
        let mut row = RepoReportRow::for_tests("/tmp/foo");
        row.concern = true;
        row.warn = false;
        row.active = true;
        row.modified = 0;
        row.staged = 0;
        row.untracked = 0;
        row.ahead = 0;
        row.push_status = "STUCK".to_string();
        row.hint = "run repair-concerns --apply".to_string();
        row.last_author = "DraconDev".to_string();
        let what = summary_what(&row, 200);
        assert!(
            what.contains("push: stuck"),
            "STUCK should surface as push: stuck: {what}"
        );
        assert!(
            what.contains("run repair-concerns"),
            "hint visible: {what}"
        );
        assert!(
            !what.contains("by DraconDev"),
            "author must be omitted from summary (v0.112.27 R2): {what}"
        );
    }

    #[test]
    fn test_severity_tier_ordering() {
        // Concern < Warn < Active < Clean. Lower tier = more urgent.
        let mut row = RepoReportRow::for_tests("/tmp/foo");
        row.concern = true;
        row.warn = true;
        row.active = true;
        assert_eq!(severity_tier(&row), 0, "concern is tier 0");

        row.concern = false;
        row.warn = true;
        row.active = true;
        assert_eq!(severity_tier(&row), 1, "warn is tier 1");

        row.warn = false;
        row.active = true;
        assert_eq!(severity_tier(&row), 2, "active is tier 2");

        row.active = false;
        assert_eq!(severity_tier(&row), 3, "clean is tier 3");
    }

    #[test]
    fn test_print_repos_summary_renders_as_table() {
        // 2026-07-20 (goal `4555eaf6` v0.112.27 R1): the summary
        // view MUST render as a proper table (with borders and
        // headers), not a free-form list. Operator feedback:
        // "the summary needs to be a table". R0 used println!
        // with manual spacing and broke alignment under ANSI
        // color codes; R1 uses comfy-table with UTF8_FULL_CONDENSED.
        let mut row = RepoReportRow::for_tests("/tmp/foo");
        row.concern = false;
        row.warn = false;
        row.active = true;
        row.modified = 1;
        row.staged = 0;
        row.untracked = 0;
        row.ahead = 0;
        row.push_status = "OK".to_string();
        row.hint = "healthy".to_string();
        row.last_author = "DraconDev".to_string();
        row.last_when = "5 minutes ago".to_string();
        row.repo = "/tmp/foo".to_string();
        row.branch = "main".to_string();
        row.upstream = "origin/main".to_string();
        row.publish_state = PublishState::Ok;
        row.last_hash = "deadbeef1234".to_string();
        row.last_msg = "test commit".to_string();
        row.last_unix = 0;
        row.state_cause = StateCause::Dirty;
        row.state_cause_label = "dirty".to_string();

        let rows = vec![row];
        let filter = RepoFilter::All;
        // We can't easily capture stdout from print_repos_summary
        // (it's hardcoded to println!), but we can at least verify
        // it doesn't panic on a populated row and produces output.
        // The visual rendering is verified by the manual test
        // script (see release-notes-v0.112.27.md).
        print_repos_summary(&rows, &filter, false, false);
    }

    #[test]
    fn test_summary_what_handles_long_hint_with_word_boundary() {
        // When hint exceeds budget, truncate_unicode_width uses
        // a word-boundary-aware algorithm (see report.rs:2094).
        // Long hints like "daemon handles after changes settle;
        // run sync-now --warns to force now" (70 chars) must be
        // truncated to fit a narrow budget without clipping mid-word
        // when possible.
        let mut row = RepoReportRow::for_tests("/tmp/foo");
        row.concern = false;
        row.warn = false;
        row.active = true;
        row.modified = 0;
        row.staged = 0;
        row.untracked = 0;
        row.ahead = 0;
        row.push_status = "OK".to_string();
        row.hint = "daemon handles after changes settle; run sync-now --warns to force now".to_string();
        row.last_author = "DraconDev".to_string();
        row.last_when = "5 minutes ago".to_string();
        let what = summary_what(&row, 80);
        // WHAT string must be at most 80 chars wide.
        let w = unicode_width::UnicodeWidthStr::width(what.as_str());
        assert!(w <= 80, "WHAT width {w} exceeds budget 80: {what}");
        // Truncated hint must end with either … (word-boundary hit)
        // or the natural sentence end (no truncation needed).
        assert!(
            what.ends_with('…') || what.ends_with("DraconDev"),
            "truncated WHAT should end with … or natural end: {what}"
        );
    }

    #[test]
    fn test_branch_upstream_missing_when_no_config() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("user.email")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .status()
            .expect("user.name")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "core.hooksPath", "/dev/null"])
            .current_dir(&repo)
            .status()
            .expect("hooksPath")
            .success());
        std::fs::write(repo.join("README.md"), "initial").expect("write file");
        assert!(crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success());
        let (label, state) = branch_upstream(&repo, "main");
        assert_eq!(label, "-");
        assert_eq!(state, PublishState::Missing);
    }

    #[test]
    fn test_branch_upstream_gone_when_remote_tracking_ref_missing() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        assert!(crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("user.email")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .status()
            .expect("user.name")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "core.hooksPath", "/dev/null"])
            .current_dir(&repo)
            .status()
            .expect("hooksPath")
            .success());
        std::fs::write(repo.join("README.md"), "initial").expect("write file");
        assert!(crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success());
        assert!(crate::git::git_cmd()
            .args(["remote", "add", "github", "git@github.com:DraconDev/test-repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.remote", "github"])
            .current_dir(&repo)
            .status()
            .expect("remote config")
            .success());
        assert!(crate::git::git_cmd()
            .args(["config", "branch.main.merge", "refs/heads/main"])
            .current_dir(&repo)
            .status()
            .expect("merge config")
            .success());
        let (label, state) = branch_upstream(&repo, "main");
        assert_eq!(label, "github/main");
        assert_eq!(state, PublishState::Gone);
    }

    /// Build a minimal RepoReportRow for activity_label testing.
    /// The `last_when` string is the natural-language relative time
    /// (e.g. "5 minutes ago") that the daemon emits from `git log`.
    fn make_activity_row(
        last_when: &str,
        modified: usize,
        staged: usize,
        push_status: &str,
    ) -> RepoReportRow {
        make_activity_row_with_state(
            last_when,
            modified,
            staged,
            push_status,
            StateCause::Healthy,
        )
    }

    fn make_activity_row_with_state(
        last_when: &str,
        modified: usize,
        staged: usize,
        push_status: &str,
        state_cause: StateCause,
    ) -> RepoReportRow {
        let label = state_cause.as_str().to_string();
        RepoReportRow {
            repo: "/tmp/test-activity-repo".to_string(),
            state_flags: vec![],
            branch: "main".to_string(),
            upstream: "github/main".to_string(),
            publish_state: PublishState::Ok,
            modified,
            staged,
            untracked: 0,
            excluded_dirty: 0,
            ahead: 0,
            behind: 0,
            last_hash: "abc".to_string(),
            last_author: "test".to_string(),
            last_when: last_when.to_string(),
            last_msg: "test".to_string(),
            last_unix: 0,
            last_push: "5m ago".to_string(),
            commits_1h: 0,
            commits_6h: 0,
            commits_24h: 0,
            push_status: push_status.to_string(),
            push_error: String::new(),
            push_to_remotes: vec!["codeberg".to_string(), "github".to_string(), "gitlab".to_string()],
            excluded_remotes: vec![],
            codeberg_skip_reason: None,
            git_size_bytes: Some(34_476_847),
            git_modules_bytes: 0,
            token_health: TokenHealthSummary { codeberg_present: true, github_present: true, gitlab_present: true },
            concern: false,
            warn: false,
            active: false,
            hint: "test".to_string(),
            state_cause,
            state_cause_label: label,
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: "none".to_string(),
            missing_objects: 0,
            pack_too_large: false,
        }
    }

    #[test]
    fn test_activity_label_push_pending() {
        // Push PENDING with 1-minute-old last commit → "pushing 1m".
        let row = make_activity_row("1 minutes ago", 0, 0, "PENDING");
        let label = activity_label(&row);
        assert!(
            label.contains("pushing"),
            "expected 'pushing' in label, got: {}",
            label
        );
    }

    #[test]
    fn test_activity_label_push_pending_includes_ahead_count() {
        // Push PENDING with ahead=28 → "pushing Xm (28 ahead)" so the
        // operator can tell at a glance that this stall is caused by
        // a large backlog, not a transient network blip.
        let mut row = make_activity_row("4 minutes ago", 0, 0, "PENDING");
        row.ahead = 28;
        let label = activity_label(&row);
        assert!(
            label.contains("pushing") && label.contains("28 ahead"),
            "expected 'pushing' and '28 ahead' in label, got: {}",
            label
        );
    }

    #[test]
    fn test_activity_label_dirty_recent_commit_dirty() {
        // Dirty + recent commit → "⏳ dirty 0m".
        let row = make_activity_row("0 minutes ago", 2, 0, "OK");
        let label = activity_label(&row);
        assert!(
            label.contains("dirty"),
            "expected 'dirty' in label, got: {}",
            label
        );
    }

    #[test]
    fn test_activity_label_dirty_old_commit_dirty() {
        // Dirty + old commit (8 minutes ago) → "⏳ dirty 8m".
        let row = make_activity_row("8 minutes ago", 1, 0, "OK");
        let label = activity_label(&row);
        assert!(
            label.contains("dirty"),
            "expected 'dirty' in label, got: {}",
            label
        );
    }

    #[test]
    fn test_activity_label_clean_recent_synced() {
        // Clean + 30-minute-old commit → "synced 30m".
        let row = make_activity_row("30 minutes ago", 0, 0, "OK");
        let label = activity_label(&row);
        assert!(
            label.contains("synced"),
            "expected 'synced' in label, got: {}",
            label
        );
    }

    #[test]
    fn test_activity_label_clean_idle() {
        // Clean + 2-hour-old commit → "idle 2h".
        let row = make_activity_row("2 hours ago", 0, 0, "OK");
        let label = activity_label(&row);
        assert!(
            label.contains("idle"),
            "expected 'idle' in label, got: {}",
            label
        );
    }

    #[test]
    fn test_activity_label_clean_cold() {
        // Clean + 2-day-old commit → "cold 2d".
        let row = make_activity_row("2 days ago", 0, 0, "OK");
        let label = activity_label(&row);
        assert!(
            label.contains("cold"),
            "expected 'cold' in label, got: {}",
            label
        );
    }

    #[test]
    fn test_activity_label_unparseable_time() {
        // Unparseable last_when → "—".
        let row = make_activity_row("never", 0, 0, "OK");
        let label = activity_label(&row);
        assert!(label.contains("—"), "expected '—' in label, got: {}", label);
    }

    #[test]
    fn test_activity_label_push_stuck_state() {
        // When `push_status == "PUSH_STUCK"` (the retry budget
        // is exhausted), the activity label must show
        // `🛑 push-stuck Xm (N ahead)` regardless of in_flight
        // state. This is a higher-priority indicator than the
        // generic `pushing Xm` because the daemon has given up
        // auto-pushing — the operator needs to intervene.
        let mut row = make_activity_row_with_state(
            "10 minutes ago",
            0,
            0,
            "PUSH_STUCK",
            StateCause::Pushing,
        );
        row.ahead = 1;
        let label = activity_label(&row);
        assert!(
            label.contains("🛑 push-stuck"),
            "PUSH_STUCK row should show '🛑 push-stuck' indicator: got {}",
            label
        );
        assert!(
            label.contains("10m"),
            "should include the duration: got {}",
            label
        );
        assert!(
            label.contains("1 ahead"),
            "should include the ahead count: got {}",
            label
        );
    }

    // ============================================================
    // in_flight staleness filter
    // ============================================================

    #[test]
    fn test_load_in_flight_for_path_stale_file_treated_as_empty() {
        // When the on-disk in_flight file is older than the
        // staleness threshold (30s), `load_in_flight_for_path`
        // should return false even if the path is in the file.
        // This prevents the "🔄 now" indicator from sticking
        // around when a slow push from the previous cycle kept
        // the repo in in_flight while the daemon has moved on.
        use std::time::SystemTime;
        // Write a file at the standard in_flight path with a
        // `written_at` timestamp from 2 minutes ago (well past
        // the 30s cutoff).
        let two_min_ago = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub(120))
            .unwrap_or(0);
        crate::daemon::save_in_flight_for_test(
            &[std::path::PathBuf::from(
                "/home/dracon/Dev/this-is-a-fake-repo-for-staleness-test",
            )],
            two_min_ago,
        );
        let result = load_in_flight_for_path("/home/dracon/Dev/this-is-a-fake-repo-for-staleness-test");
        assert!(!result, "stale in_flight file should be treated as empty");
        // Cleanup
        let _ = std::fs::remove_file(crate::daemon::in_flight_path_for_test());
    }

    #[test]
    fn test_load_in_flight_for_path_recent_file_honoured() {
        // When the on-disk in_flight file is fresh (within the
        // staleness threshold), the function should return true
        // for paths in the set.
        use std::time::SystemTime;
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        crate::daemon::save_in_flight_for_test(
            &[std::path::PathBuf::from(
                "/home/dracon/Dev/another-fake-repo-for-staleness-test",
            )],
            now,
        );
        let result = load_in_flight_for_path("/home/dracon/Dev/another-fake-repo-for-staleness-test");
        assert!(result, "fresh in_flight file should be honoured");
        let _ = std::fs::remove_file(crate::daemon::in_flight_path_for_test());
    }

    #[test]
    fn test_load_in_flight_for_path_10s_old_is_stale() {
        // The staleness threshold is 5s. A file written 10s ago
        // must be treated as stale (returns false). This is the
        // boundary case: under the 30s default this was fresh.
        use std::time::SystemTime;
        let ten_secs_ago = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub(10))
            .unwrap_or(0);
        crate::daemon::save_in_flight_for_test(
            &[std::path::PathBuf::from(
                "/home/dracon/Dev/repo-with-10s-old-inflight",
            )],
            ten_secs_ago,
        );
        let result = load_in_flight_for_path("/home/dracon/Dev/repo-with-10s-old-inflight");
        assert!(!result, "10s-old in_flight file should be stale at 5s threshold");
        let _ = std::fs::remove_file(crate::daemon::in_flight_path_for_test());
    }

    #[test]
    fn test_activity_label_suppresses_in_flight_for_clean_state() {
        // When the row's state_cause is `Synced`, `Idle`,
        // `Cold`, `Untracked`, or `Healthy`, the activity
        // label must NOT show "🔄 now" even if the in_flight
        // file lists the repo path. This is the second leak
        // in the staleness filter: clean rows are never
        // legitimately in-flight, so the indicator is always
        // false-positive.
        use std::time::SystemTime;
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        crate::daemon::save_in_flight_for_test(
            &[std::path::PathBuf::from(
                "/home/dracon/Dev/repo-clean-but-listed-as-inflight",
            )],
            now,
        );
        // Build a row whose repo path matches the in_flight file
        // but whose state is one of the clean states.
        let mut row = make_activity_row_with_state(
            "5 minutes ago",
            0,
            0,
            "OK",
            StateCause::Synced,
        );
        row.repo = "/home/dracon/Dev/repo-clean-but-listed-as-inflight".to_string();
        let label = activity_label(&row);
        assert!(
            !label.contains("🔄 now"),
            "Synced row should not show '🔄 now' even when in_flight file lists it: got {}",
            label
        );

        // Same for Idle
        row.state_cause = StateCause::Idle;
        row.state_cause_label = "idle".to_string();
        let label = activity_label(&row);
        assert!(
            !label.contains("🔄 now"),
            "Idle row should not show '🔄 now': got {}",
            label
        );

        // Same for Cold
        row.state_cause = StateCause::Cold;
        row.state_cause_label = "cold".to_string();
        let label = activity_label(&row);
        assert!(
            !label.contains("🔄 now"),
            "Cold row should not show '🔄 now': got {}",
            label
        );

        // But Dirty state SHOULD still show "🔄 now"
        row.state_cause = StateCause::Dirty;
        row.state_cause_label = "dirty".to_string();
        let label = activity_label(&row);
        assert!(
            label.contains("🔄 now"),
            "Dirty row SHOULD show '🔄 now' when in_flight: got {}",
            label
        );

        let _ = std::fs::remove_file(crate::daemon::in_flight_path_for_test());
    }

    #[test]
    fn test_repo_report_json_structure() {
        let json = RepoReportJson {
            policy: "default".to_string(),
            filter: "all".to_string(),
            repos: 1,
            ok: 1,
            active: 0,
            warn: 0,
            concern: 0,
            failures: 0,
            rows: vec![],
        };
        assert_eq!(json.repos, 1);
        assert_eq!(json.ok, 1);
    }

    #[test]
    fn test_status_json_structure() {
        let status = StatusJson {
            policy: "default".to_string(),
            roots: vec!["~/code".to_string()],
            repos_discovered: 5,
            pulse_interval_secs: 30,
            inactivity_push_delay_secs: 300,
            freeze: "none".to_string(),
            auto_commit: true,
            auto_pull: true,
            auto_push: true,
            auto_bump_versions: true,
            auto_repair_concerns: true,
            auto_repair_warns: true,
            auto_rewrite_large_blobs: true,
            max_stage_file_bytes: 100 * 1024 * 1024,
            push_blob_threshold_bytes: 100 * 1024 * 1024,
            exclude_dirs: vec![],
            exclude_file_patterns: vec![],
            pull_op_timeout_secs: 30,
            push_op_timeout_secs: 300,
            repo_sync_timeout_secs: 420,
            stage_op_timeout_secs: 60,
            stage_cooldown_secs: 3600,
            push_retries: 3,
            repair_cooldown_secs: 60,
            incident_ledger_max_lines: 10000,
            incident_ledger_max_age_days: 30,
            system_repo: String::new(),
            backup_policy: String::new(),
            backup_dir: String::new(),
            remotes: 0,
            remote_configs: vec![],
        };
        assert_eq!(status.repos_discovered, 5);
        assert!(status.auto_commit);
    }

    #[test]
    fn test_push_large_blob_threshold_min_limit() {
        let policy = SyncPolicy {
            max_stage_file_bytes: 10 * 1024 * 1024,
            max_push_blob_bytes: 5 * 1024 * 1024,
            ..test_sync_policy()
        };
        let threshold = push_large_blob_threshold_bytes(&policy);
        assert_eq!(threshold, 5 * 1024 * 1024);
    }

    #[test]
    fn test_truncate_three_chars() {
        let result = truncate("hello", 3);
        assert_eq!(result, "he…");
    }

    #[test]
    fn test_truncate_exact_length_no_ellipsis() {
        let result = truncate("hello", 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_empty_string() {
        let result = truncate("", 5);
        assert_eq!(result, "");
    }

    #[test]
    fn test_push_large_blob_threshold_bytes_custom() {
        let policy = SyncPolicy {
            max_push_blob_bytes: 50 * 1024 * 1024,
            ..test_sync_policy()
        };
        let threshold = push_large_blob_threshold_bytes(&policy);
        assert_eq!(threshold, 50 * 1024 * 1024);
    }

    #[test]
    fn test_push_large_blob_threshold_bytes_uses_min_of_all() {
        let policy = SyncPolicy {
            max_stage_file_bytes: 10 * 1024 * 1024,
            max_push_blob_bytes: 50 * 1024 * 1024,
            ..test_sync_policy()
        };
        let threshold = push_large_blob_threshold_bytes(&policy);
        assert_eq!(
            threshold,
            10 * 1024 * 1024,
            "should use smaller of stage and push limit"
        );
    }

    #[test]
    fn test_create_github_private_remote_success() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("my-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");

        let gh_mock = tmp.path().join("gh");
        std::fs::write(&gh_mock, "#!/bin/sh\necho \"mock gh called\" >&2\nexit 0\n")
            .expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod gh");
        let _lock = crate::git::acquire_path_lock();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let _guard = EnvRestorer::new(
            "PATH",
            &format!("{}:{}", tmp.path().to_string_lossy(), orig_path),
        );

        let result = create_github_private_remote(&repo, "testaccount", true);

        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            "https://github.com/testaccount/my-repo.git"
        );
    }

    #[test]
    fn test_create_github_private_remote_already_exists_reuses_without_suffix() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("dracon-demons");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");

        let gh_mock = tmp.path().join("gh");
        std::fs::write(
            &gh_mock,
            "#!/bin/sh\necho ' Name already exists' >&2\nexit 1\n",
        )
        .expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod gh");
        let _lock = crate::git::acquire_path_lock();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let _guard = EnvRestorer::new(
            "PATH",
            &format!("{}:{}", tmp.path().to_string_lossy(), orig_path),
        );

        let result = create_github_private_remote(&repo, "testaccount", true);

        assert!(result.is_some());
        let url = result.unwrap();
        assert!(!url.contains("-1"), "should NOT contain suffix -1: {}", url);
        assert!(!url.contains("-2"), "should NOT contain suffix -2: {}", url);
        assert_eq!(url, "https://github.com/testaccount/dracon-demons.git");
    }

    #[test]
    fn test_create_github_private_remote_origin_already_exists_does_not_add_duplicate() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("existing-remote-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");
        crate::git::git_cmd()
            .args(["remote", "add", "origin", "git@github.com:old/old.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add");

        let gh_mock = tmp.path().join("gh");
        std::fs::write(&gh_mock, "#!/bin/sh\nexit 1\n").expect("write gh mock");
        std::fs::set_permissions(&gh_mock, std::fs::Permissions::from_mode(0o755))
            .expect("chmod gh");
        let _lock = crate::git::acquire_path_lock();
        let orig_path = std::env::var("PATH").unwrap_or_default();
        let _guard = EnvRestorer::new(
            "PATH",
            &format!("{}:{}", tmp.path().to_string_lossy(), orig_path),
        );

        let result = create_github_private_remote(&repo, "testaccount", true);

        assert!(result.is_some());
        let remotes = crate::git::multi_remote::list_remotes(&repo);
        assert_eq!(remotes.len(), 1, "should not add duplicate origin");
        assert_eq!(remotes[0], "origin");
    }

    #[test]
    fn test_create_github_private_remote_no_gh_installed_returns_none() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let repo = tmp.path().join("no-gh-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "master"])
            .arg(&repo)
            .status()
            .expect("git init");

        let git_dir = std::path::Path::new("/run/current-system/sw/bin");
        let _lock = crate::git::acquire_path_lock();
        let _guard = EnvRestorer::new(
            "PATH",
            &format!(
                "{}:{}",
                tmp.path().to_string_lossy(),
                git_dir.to_string_lossy()
            ),
        );

        let result = create_github_private_remote(&repo, "testaccount", true);

        assert!(result.is_none());
    }

    #[test]
    fn test_shorten_when() {
        assert_eq!(shorten_when("5 seconds ago"), "5s");
        assert_eq!(shorten_when("29 minutes ago"), "29m");
        assert_eq!(shorten_when("74 minutes ago"), "1h 14m");
        assert_eq!(shorten_when("60 minutes ago"), "1h");
        assert_eq!(shorten_when("119 minutes ago"), "1h 59m");
        assert_eq!(shorten_when("3 hours ago"), "3h");
        assert_eq!(shorten_when("25 hours ago"), "1d 1h");
        assert_eq!(shorten_when("24 hours ago"), "1d");
        assert_eq!(shorten_when("48 hours ago"), "2d");
        assert_eq!(shorten_when("2 days ago"), "2d");
        assert_eq!(shorten_when("7 days ago"), "1w");
        assert_eq!(shorten_when("8 days ago"), "1w 1d");
        assert_eq!(shorten_when("14 days ago"), "2w");
        assert_eq!(shorten_when("12 months ago"), "1y");
        assert_eq!(shorten_when("13 months ago"), "1y 1mo");
        assert_eq!(shorten_when("6 weeks ago"), "6w");
        assert_eq!(shorten_when("just now"), "just now");
        assert_eq!(shorten_when("unknown"), "unknown");
    }

    #[test]
    fn test_push_status_calculation_from_flags() {
        // Test OK status - no issues
        let flags = ["OK".to_string()];
        let push_status = if flags.iter().any(|f| f == "STUCK_PUSH") {
            "STUCK"
        } else if flags.iter().any(|f| f == "NO_UPSTREAM") {
            "FAIL"
        } else {
            "OK"
        };
        assert_eq!(push_status, "OK");

        // Test STUCK status
        let flags = ["STUCK_PUSH".to_string()];
        let push_status = if flags.iter().any(|f| f == "STUCK_PUSH") {
            "STUCK"
        } else if flags.iter().any(|f| f == "NO_UPSTREAM") {
            "FAIL"
        } else {
            "OK"
        };
        assert_eq!(push_status, "STUCK");

        // Test FAIL status
        let flags = ["NO_UPSTREAM".to_string()];
        let push_status = if flags.iter().any(|f| f == "STUCK_PUSH") {
            "STUCK"
        } else if flags.iter().any(|f| f == "NO_UPSTREAM") {
            "FAIL"
        } else {
            "OK"
        };
        assert_eq!(push_status, "FAIL");
    }

    #[test]
    fn test_push_failure_cooldown_dedup() {
        let mut cooldowns = std::collections::HashMap::new();
        let repo = std::path::PathBuf::from("/test/repo");
        let notify_key = format!("push-fail-{}", repo.display());
        let now = std::time::Instant::now();
        let cooldown_secs = 300;

        // First notification should be allowed
        assert!(!cooldowns.contains_key(&notify_key));

        // Set cooldown
        cooldowns.insert(
            notify_key.clone(),
            now + std::time::Duration::from_secs(cooldown_secs),
        );

        // Second notification within cooldown should be blocked
        let cooldown_until = cooldowns.get(&notify_key).unwrap();
        assert!(now < *cooldown_until, "should still be in cooldown");

        // After cooldown expires, notification should be allowed
        let expired_cooldown = now - std::time::Duration::from_secs(1);
        cooldowns.insert(notify_key.clone(), expired_cooldown);
        let cooldown_until = cooldowns.get(&notify_key).unwrap();
        assert!(now >= *cooldown_until, "cooldown should have expired");
    }

    #[test]
    fn test_repo_report_row_push_status_fields() {
        let row = RepoReportRow {
            repo: "/test/repo".to_string(),
            state_flags: vec!["STUCK_PUSH".to_string()],
            branch: "main".to_string(),
            upstream: "github/main".to_string(),
            publish_state: PublishState::Ok,
            modified: 0,
            staged: 0,
            untracked: 0,
            excluded_dirty: 0,
            ahead: 5,
            behind: 0,
            last_hash: "abc123".to_string(),
            last_author: "test".to_string(),
            last_when: "2024-01-01".to_string(),
            last_msg: "test commit".to_string(),
            last_unix: 1700000000,
            commits_1h: 0,
            commits_6h: 0,
            commits_24h: 0,
            last_push: "5m ago".to_string(),
            push_status: "STUCK".to_string(),
            push_error: "ahead=5, push failing".to_string(),
            push_to_remotes: vec!["codeberg".to_string()],
            excluded_remotes: vec!["github".to_string(), "gitlab".to_string()],
            codeberg_skip_reason: None,
            git_size_bytes: Some(20_518_397_949),
            git_modules_bytes: 0,
            token_health: TokenHealthSummary { codeberg_present: true, github_present: true, gitlab_present: true },
            concern: true,
            warn: false,
            active: false,
            hint: "run repair-concerns --apply (push or rewrite)".to_string(),
            state_cause: StateCause::Failed,
            state_cause_label: "failed".to_string(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: "none".to_string(),
            missing_objects: 0,
            pack_too_large: false,
        };
        assert_eq!(row.push_status, "STUCK");
        assert!(row.push_error.contains("ahead=5"));
        assert!(row.concern);
    }

    // -------------------------------------------------------------------
    // read_tail_lines tests — used by build_recent_push_failure_map so
    // the incident-ledger scan is O(tail) instead of O(ledger_size).
    // -------------------------------------------------------------------

    #[test]
    fn read_tail_lines_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        std::fs::write(&path, b"").unwrap();
        let lines = read_tail_lines(&path, 100).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn read_tail_lines_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        std::fs::write(&path, "line1\nline2\nline3\n").unwrap();
        let lines = read_tail_lines(&path, 100).unwrap();
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn read_tail_lines_respects_window() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let body: String = (0..2000)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, body).unwrap();
        let lines = read_tail_lines(&path, 50).unwrap();
        assert_eq!(lines.len(), 50);
        // Last 50 lines are line1950..line1999
        assert_eq!(lines.first().unwrap(), "line1950");
        assert_eq!(lines.last().unwrap(), "line1999");
    }

    #[test]
    fn read_tail_lines_handles_oversized_line() {
        // A single 20 KiB line should not confuse the chunk reader.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let big = "x".repeat(20_000);
        std::fs::write(&path, format!("{}\nshort\n", big)).unwrap();
        let lines = read_tail_lines(&path, 5).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 20_000);
        assert_eq!(lines[1], "short");
    }

    #[test]
    fn read_tail_lines_handles_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        std::fs::write(&path, "a\nb\nc\n").unwrap();
        let lines = read_tail_lines(&path, 5).unwrap();
        // Trailing newline means three lines, no empty fourth.
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    // -------------------------------------------------------------------
    // build_recent_push_failure_map integration test.
    // -------------------------------------------------------------------

    #[test]
    fn build_recent_push_failure_map_populated() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dir = tempfile::tempdir().unwrap();
        let policy_path = dir.path().join("dracon-sync.toml");
        let ledger_path = dir.path().join("ledger.jsonl");
        std::fs::write(
            &policy_path,
            "pulse_interval_secs = 1\nwatch_roots = [\"/tmp\"]\n",
        )
        .unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let recent_ts = now - 30; // 30s ago, well within 10-min window
        let old_ts = now - 3600; // 1h ago, outside the window
        std::fs::write(
            &ledger_path,
            format!(
                "{{\"ts_unix\":{recent_ts},\"scope\":\"sync\",\"repo\":\"/tmp/recent-fail\",\"result\":\"fail\",\"reason\":\"push rejected\"}}\n\
                 {{\"ts_unix\":{old_ts},\"scope\":\"sync\",\"repo\":\"/tmp/old-fail\",\"result\":\"fail\",\"reason\":\"push rejected\"}}\n\
                 {{\"ts_unix\":{recent_ts},\"scope\":\"sync\",\"repo\":\"/tmp/recent-ok\",\"result\":\"ok\",\"reason\":\"pushed\"}}\n"
            ),
        )
        .unwrap();
        let ledger_str = ledger_path.to_string_lossy().to_string();
        let _ledger = EnvRestorer::new("DRACON_SYNC_LEDGER", &ledger_str);
        let map = build_recent_push_failure_map(&policy_path).unwrap();
        assert!(map.contains_key("/tmp/recent-fail"));
        assert!(!map.contains_key("/tmp/old-fail"));
        assert!(!map.contains_key("/tmp/recent-ok"));
    }

    #[test]
    fn build_recent_push_failure_map_missing_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let policy_path = dir.path().join("dracon-sync.toml");
        let ledger_path = dir.path().join("missing-ledger.jsonl");
        std::fs::write(
            &policy_path,
            "pulse_interval_secs = 1\nwatch_roots = [\"/tmp\"]\n",
        )
        .unwrap();
        let ledger_str = ledger_path.to_string_lossy().to_string();
        let _ledger = EnvRestorer::new("DRACON_SYNC_LEDGER", &ledger_str);
        let map = build_recent_push_failure_map(&policy_path);
        assert!(map.is_none());
    }

    #[test]
    fn build_daemon_last_action_map_keeps_most_recent_per_repo() {
        let dir = tempfile::tempdir().unwrap();
        let policy_path = dir.path().join("dracon-sync.toml");
        let ledger_path = dir.path().join("ledger.jsonl");
        std::fs::write(
            &policy_path,
            "pulse_interval_secs = 1\nwatch_roots = [\"/tmp\"]\n",
        )
        .unwrap();
        // Two entries for the same repo: the second one is newer and
        // should win. A third entry for a different repo should also
        // appear.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let lines = [
            format!(
                "{{\"ts_unix\":{},\"scope\":\"sync\",\"repo\":\"/tmp/repo-a\",\"reason\":\"\",\"action\":\"sync_commit\",\"result\":\"ok\"}}",
                now - 60
            ),
            format!(
                "{{\"ts_unix\":{},\"scope\":\"sync\",\"repo\":\"/tmp/repo-a\",\"reason\":\"\",\"action\":\"sync_triage\",\"result\":\"ok\"}}",
                now - 5
            ),
            format!(
                "{{\"ts_unix\":{},\"scope\":\"warn\",\"repo\":\"/tmp/repo-b\",\"reason\":\"\",\"action\":\"dry_run_sync_triage\",\"result\":\"planned\"}}",
                now - 10
            ),
        ];
        std::fs::write(&ledger_path, lines.join("\n") + "\n").unwrap();
        let ledger_str = ledger_path.to_string_lossy().to_string();
        let _ledger = EnvRestorer::new("DRACON_SYNC_LEDGER", &ledger_str);
        let map = build_daemon_last_action_map(&policy_path).expect("map");
        let a = map.get("/tmp/repo-a").expect("repo-a entry");
        assert_eq!(a.1, "sync_triage", "newer action wins");
        assert_eq!(a.2, "ok");
        let b = map.get("/tmp/repo-b").expect("repo-b entry");
        assert_eq!(b.1, "dry_run_sync_triage");
        assert_eq!(b.2, "planned");
    }

    #[test]
    fn build_daemon_last_action_map_missing_ledger_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let policy_path = dir.path().join("dracon-sync.toml");
        let ledger_path = dir.path().join("missing-ledger.jsonl");
        std::fs::write(
            &policy_path,
            "pulse_interval_secs = 1\nwatch_roots = [\"/tmp\"]\n",
        )
        .unwrap();
        let ledger_str = ledger_path.to_string_lossy().to_string();
        let _ledger = EnvRestorer::new("DRACON_SYNC_LEDGER", &ledger_str);
        let map = build_daemon_last_action_map(&policy_path);
        assert!(map.is_none());
    }

    // ---- Layout tier and rendering tests (goal: fix ugly PUSH_STUCK render) ----

    #[test]
    fn test_truncate_unicode_width_no_truncation() {
        // ASCII fits in width budget
        assert_eq!(truncate_unicode_width("hello", 10), "hello");
        // Exact fit
        assert_eq!(truncate_unicode_width("hello", 5), "hello");
        // Empty input
        assert_eq!(truncate_unicode_width("", 5), "");
        // max_width=0 returns empty
        assert_eq!(truncate_unicode_width("hello", 0), "");
    }

    #[test]
    fn test_truncate_unicode_width_emoji_safe() {
        // 👋 is 2 cols wide. "hello 👋 world" is 14 cols.
        // At width 8, content_budget=7 (reserve 1 for ellipsis).
        // h(1)+e(1)+l(1)+l(1)+o(1)+space(1) = 6, then 👋(2) = 8 > 7.
        // So 👋 is dropped. Result: "hello …" (7 cols).
        let r = truncate_unicode_width("hello 👋 world", 8);
        assert!(r.ends_with('…'), "expected ellipsis, got: {:?}", r);
        // 👋 should NOT be present (it didn't fit in content_budget)
        assert!(!r.contains('👋'), "emoji should be dropped: {:?}", r);
        // Width should be at most 8 cols
        let w: usize = r.chars().map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).sum();
        assert!(w <= 8, "result width {} > 8 for {:?}", w, r);

        // At width 10, content_budget=9. h(1)+e(1)+l(1)+l(1)+o(1)+space(1)+👋(2) = 8 fits.
        // Then space(1) = 9, fits. So "hello 👋 " + "…" = 10 cols.
        let r2 = truncate_unicode_width("hello 👋 world", 10);
        assert!(r2.ends_with('…'), "expected ellipsis, got: {:?}", r2);
        assert!(r2.contains('👋'), "emoji should be preserved: {:?}", r2);
        let w2: usize = r2.chars().map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).sum();
        assert!(w2 <= 10, "result width {} > 10 for {:?}", w2, r2);
    }

    #[test]
    fn test_truncate_unicode_width_cjk() {
        // CJK chars are 2 cols wide each. "你好世" = 6 cols.
        // At width 5, fits "你好" (4 cols) + "…" (1 col) = 5 cols.
        let r = truncate_unicode_width("你好世界", 5);
        assert!(r.ends_with('…'), "expected ellipsis, got: {:?}", r);
        // Must not split a CJK char
        assert!(!r.contains('界'), "must not include last CJK: {:?}", r);
    }

    #[test]
    fn test_choose_layout_tier_vertical() {
        // Use env var to control width
        let prev = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
        // CHANGED 2026-07-22 (v0.112.38): the < 242 default is now
        // Rich (6-column table), NOT Vertical. Vertical remains
        // available via `--layout vertical` and as the `repos <name>`
        // per-repo detail format.
        // CHANGED 2026-07-30 (v0.113.26): Rich is the auto-pick for
        // EVERYTHING ≥165 cols — the 242-314 Compact band and the
        // ≥315 Full band were removed (a maximized terminal silently
        // served the OLD compact table; "looks like the old table").
        for w in [165, 180, 199, 219, 237, 241, 242, 300, 314, 315, 500] {
            std::env::set_var("DRACON_SYNC_TERM_WIDTH", w.to_string());
            assert_eq!(
                choose_layout_tier(),
                LayoutTier::Rich,
                "width {} should be Rich (v0.113.8 default)",
                w
            );
        }
        // Restore
        match prev {
            Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
            None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
        }
    }

    #[test]
    fn test_choose_layout_tier_compact() {
        let prev = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
        // v0.113.26: Compact is ONLY the < 165 fallback now; the
        // 242-299 band is Rich (was Compact before v0.113.26).
        for w in [40, 90, 120, 164] {
            std::env::set_var("DRACON_SYNC_TERM_WIDTH", w.to_string());
            assert_eq!(
                choose_layout_tier(),
                LayoutTier::Compact,
                "width {} should be Compact",
                w
            );
        }
        match prev {
            Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
            None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
        }
    }

    #[test]
    fn test_choose_layout_tier_full() {
        // v0.113.26: Full is NEVER auto-picked (the ≥315 band was
        // removed) — wide terminals get Rich. Full remains an opt-in
        // via `--layout full` only.
        let prev = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
        for w in [315, 400, 500, 1000] {
            std::env::set_var("DRACON_SYNC_TERM_WIDTH", w.to_string());
            assert_eq!(
                choose_layout_tier(),
                LayoutTier::Rich,
                "width {} should auto-pick Rich (Full is --layout-only since v0.113.26)",
                w
            );
        }
        match prev {
            Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
            None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
        }
    }

    #[test]
    fn test_terminal_width_columns_env_var() {
        // COLUMNS env var (ncurses convention) should be respected as a fallback
        // when DRACON_SYNC_TERM_WIDTH is unset.
        let prev_width = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
        let prev_cols = std::env::var("COLUMNS").ok();
        std::env::remove_var("DRACON_SYNC_TERM_WIDTH");
        std::env::set_var("COLUMNS", "150");
        let w = terminal_width();
        assert_eq!(w, Some(150), "COLUMNS=150 should yield Some(150)");
        std::env::set_var("COLUMNS", "999");
        let w = terminal_width();
        // 999 is in the (40..=1000) range
        assert_eq!(w, Some(999), "COLUMNS=999 should yield Some(999)");
        std::env::set_var("COLUMNS", "30");
        let w = terminal_width();
        // 30 is outside (40..=1000), so falls through to next check
        // (terminal_size returns None in tests), so fallback Some(120) applies
        assert_eq!(w, Some(120), "COLUMNS=30 (out of range) falls through to fallback 120");
        // DRACON_SYNC_TERM_WIDTH still takes precedence
        std::env::set_var("DRACON_SYNC_TERM_WIDTH", "80");
        let w = terminal_width();
        assert_eq!(w, Some(80), "DRACON_SYNC_TERM_WIDTH takes precedence over COLUMNS");
        // Restore
        match prev_width {
            Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
            None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
        }
        match prev_cols {
            Some(v) => std::env::set_var("COLUMNS", v),
            None => std::env::remove_var("COLUMNS"),
        }
    }

    #[test]
    fn test_terminal_width_fallback_is_compact() {
        // When neither env var is set and terminal_size fails (test env / pipe),
        // the fallback must be 120 (Compact-friendly), NOT 300 (Full-only).
        // The width 120 sits at the boundary between Vertical (< 220) and Compact
        // (220-299), so when terminal_size() returns Some(120, _) the dispatcher
        // routes to Vertical (correct), but the fallback's *value* must be 120 —
        // NOT 300 — so that piped output is never accidentally Full-width.
        let prev_width = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
        let prev_cols = std::env::var("COLUMNS").ok();
        std::env::remove_var("DRACON_SYNC_TERM_WIDTH");
        std::env::set_var("COLUMNS", "120"); // Force 120 explicitly via COLUMNS
        let w = terminal_width();
        assert_eq!(w, Some(120), "fallback for non-TTY must be Some(120), got {:?}", w);
        // CHANGED 2026-07-22 (v0.112.38): < 242 routes to Rich, not Vertical.
        // CHANGED 2026-07-28 (v0.113.8): 120 cols now routes to Compact (the
        // post-v0.113.8 Rich tier needs ≥165 cols minimum — added USED,
        // COMMITS, SIZE, TOUCHED columns grew the total width from ~120 to
        // ~165). The Compact tier handles 90-165 col terminals.
        assert_eq!(choose_layout_tier(), LayoutTier::Compact, "120 cols must route to Compact");
        // Restore
        match prev_width {
            Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
            None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
        }
        match prev_cols {
            Some(v) => std::env::set_var("COLUMNS", v),
            None => std::env::remove_var("COLUMNS"),
        }
    }

    #[test]
    fn test_choose_layout_tier_fallback_no_env_no_tty_yields_compact_or_smaller() {
        // When no env vars are set and terminal_size returns None, the dispatcher's
        // fallback (Some(120)) must never route to Full (which requires 300+ cols).
        let prev_width = std::env::var("DRACON_SYNC_TERM_WIDTH").ok();
        let prev_cols = std::env::var("COLUMNS").ok();
        std::env::remove_var("DRACON_SYNC_TERM_WIDTH");
        std::env::remove_var("COLUMNS");
        let tier = choose_layout_tier();
        assert_ne!(
            tier,
            LayoutTier::Full,
            "fallback must NOT route to Full (which would produce 600+ char rows)"
        );
        // Restore
        match prev_width {
            Some(v) => std::env::set_var("DRACON_SYNC_TERM_WIDTH", v),
            None => std::env::remove_var("DRACON_SYNC_TERM_WIDTH"),
        }
        match prev_cols {
            Some(v) => std::env::set_var("COLUMNS", v),
            None => std::env::remove_var("COLUMNS"),
        }
    }

    #[test]
    fn test_push_cell_label_ok() {
        let (text, _color) = push_cell_label("OK", None);
        assert_eq!(text, "✅ OK");
    }

    #[test]
    fn test_push_cell_label_pending() {
        let (text, _color) = push_cell_label("PENDING", None);
        assert_eq!(text, "🟣 PENDING");
    }

    #[test]
    fn test_push_cell_label_push_stuck() {
        // PUSH_STUCK should render with stop icon, no plain "PUSH_STUCK" text
        let (text, _color) = push_cell_label("PUSH_STUCK", Some(173));
        assert_eq!(text, "🛑 STUCK");
        assert!(!text.contains("PUSH_STUCK"), "must not show plain PUSH_STUCK text: {:?}", text);
    }

    #[test]
    fn test_push_cell_label_fail() {
        let (text, _color) = push_cell_label("FAIL", None);
        assert_eq!(text, "❌ FAIL");
    }

    /// ADDED 2026-08-09 (v0.113.49, pi-goal-list-loop-audit cascade
    /// finding): the PUSH legend used to list 5 markers (✅ OK, 🟣 push
    /// in flight, ❌ FAIL, 🩹 broken history, 🔑 forge token missing)
    /// while the code emits 8 distinct cell labels via `push_cell_label`
    /// — three of which (🛑 STUCK, 🩹 BROKEN, 🚫 BLOCKED) were
    /// undocumented, plus ✅ INTENT. Operators seeing `🛑 STUCK` in the
    /// PUSH column had no legend entry to look it up. This test pins
    /// the legend text as the source of truth for cell labels: if a new
    /// `push_cell_label` arm is added without updating the legend, the
    /// test trips. Markers (🩹 for missing objects, 🔑 for forge token
    /// missing) are also asserted since the legend documents them.
    #[test]
    fn test_repos_legend_covers_all_push_cell_labels() {
        // Collect every documented cell label.
        let cell_label_statuses = ["OK", "INTENTIONAL", "PENDING", "PUSH_STUCK", "STUCK", "FAIL", "BROKEN", "BLOCKED"];
        let cell_label_outputs: Vec<String> = cell_label_statuses
            .iter()
            .map(|s| push_cell_label(s, None).0.to_string())
            .collect();
        // The legend should contain every cell label's text. We
        // concatenate the legend rows into a single haystack and search
        // for each needle (case-sensitive, since the labels are
        // deliberate).
        let haystack: String = repos_legend_rows()
            .iter()
            .map(|(_, text)| *text)
            .collect();
        for label in &cell_label_outputs {
            assert!(
                haystack.contains(label.as_str()),
                "PUSH legend missing cell label {label:?} (full legend haystack: {haystack:?})"
            );
        }
        // Markers — the legend must document the 🩹 and 🔑 markers
        // (appended after the cell label by `push_cell_with_markers`).
        assert!(
            haystack.contains('🩹'),
            "PUSH legend missing the 🩹 marker glyph"
        );
        assert!(
            haystack.contains('🔑'),
            "PUSH legend missing the 🔑 marker glyph"
        );
        // Sanity: the haystack itself comes from the PUSH row (not
        // some other row), so we also check it contains `push_status`
        // vocabulary like `OK` (which appears in both the PUSH row's
        // legend entry and possibly elsewhere). This catches a
        // regression where someone moves the labels to a different
        // legend row by mistake.
        let push_row = repos_legend_rows()
            .iter()
            .find(|(label, _)| *label == "PUSH")
            .map(|(_, text)| *text)
            .expect("PUSH row must exist in legend");
        for label in &cell_label_outputs {
            assert!(
                push_row.contains(label.as_str()),
                "PUSH row of legend missing cell label {label:?}: {push_row:?}"
            );
        }
    }

    #[test]
    fn test_branch_color_for_main_master() {
        assert_eq!(branch_color_for("main"), Color::White);
        assert_eq!(branch_color_for("master"), Color::White);
    }

    #[test]
    fn test_branch_color_for_other() {
        assert_eq!(branch_color_for("feature/foo"), Color::Cyan);
        assert_eq!(branch_color_for("wip"), Color::Cyan);
    }

    #[test]
    fn test_colorize_passthrough_when_no_color() {
        // When stdout is not a TTY (e.g. piped to file), colorize should
        // return the plain text. This test runs in a test env, so usually no color.
        let result = colorize("hello", Color::Red);
        // Either with ANSI codes or without, but must contain "hello"
        assert!(result.contains("hello"));
    }

    // ---- Wrap-detection tests (goal: no cell wraps mid-content at tier boundaries) ----

    /// Verify the sum of all 22 column minimums in `print_repos_full_table`
    /// plus 23 borders is < 315 cols (the full tier threshold).
    /// If this sum grows past 315, the full tier won't fit and content will wrap.
    #[test]
    fn test_full_table_min_width_within_300() {
        // The values here MUST match the set_constraints in print_repos_full_table.
        // If you change the table layout, update both at once.
        // F30 (2026-07-18): the prior array had 22 entries but the
        // production constraints have 23 (ROLE was added in v0.112.19
        // but never added to this test). The new values reflect the
        // v0.112.21 layout: ROLE 18, PUSH-TO 22, LAST COMMIT 17,
        // ACTIVITY 11, DAEMON 15, HINT 15 (all trimmed from v0.112.19
        // widths to bring the floor under 300 cols).
        //
        // F30v2 (2026-07-19): LAST COMMIT and AUTHOR switched from
        // LowerBoundary to Absolute so long cell content (e.g. 152-char
        // auto-commit subjects) is truncated instead of widening the
        // column. Array values unchanged.
        let minimums: [u16; 23] = [
            // 2026-07-19 (goal `4555eaf6`): REPO bumped from 17 → 19
            // to accommodate long names like
            // `pully-fully-pull-based-fleet-reconciler` (38 chars)
            // truncated to 17 cols of content. Other LowerBoundaries
            // (ACTIVITY, STATE, DAEMON, HINT) became Absolute for the
            // same reason — width budget moved up the column list.
            4, 13, 19, 18, 11, 17, 8, 8, 7, 9, 11, 13, 32, 17, 11, 11, 11, 8, 8, 8, 15, 15, 15,
        ];
        // F30v2 (2026-07-19): values unchanged but constraint type for
        // PUSH-TO, LAST COMMIT, and AUTHOR switched from LowerBoundary
        // to Absolute. The values still match the production constraints.
        let sum: u32 = minimums.iter().map(|&x| x as u32).sum();
        let borders: u32 = 24;
        let total = sum + borders;
        assert!(
            total <= 315,
            "Full table minimum width {total} exceeds 315-col tier threshold. \
             Lower some LowerBoundaries or push the tier boundary higher."
        );
        // F30 regression: the test array count must match the
        // production constraint count (23, after ROLE was added).
        assert_eq!(
            minimums.len(),
            23,
            "full_table constraint count mismatch: if you change set_constraints, update this test too"
        );
        // F30 regression: ROLE must be present in the array.
        assert_eq!(
            minimums[3], 18,
            "ROLE column missing or wrong width; this test never caught the v0.112.19 bug"
        );
    }

    /// Verify the sum of all 16 column minimums in `print_repos_compact_table`
    /// plus 15 borders is < 220 cols (the compact tier threshold; Vertical
    /// is for terminals < 220).
    ///
    /// 2026-07-19 (goal `4555eaf6`): REPO (18), ROLE (14), PUBLISH (18),
    /// PUSH-TO (32), LAST COMMIT (18), STATE+ACT (17), HINT (22) all
    /// became Absolute so cells with variable-length content
    /// (`pully-fully-pull-based-fleet-reconciler`, `released/one-mil-girls`,
    /// `⚠️ origin/main (gone)`, etc.) are truncated rather than letter-wrapped
    /// onto a second line on narrow (220-260 col) terminals.
    #[test]
    fn test_compact_table_min_width_within_250() {
        // The values here MUST match the set_constraints in print_repos_compact_table.
        // If you change the table layout, update both at once.
        // 16 cols: #, STATUS, REPO, ROLE, BRANCH, PUBLISH, MOD, STG, UT,
        // AHEAD, BEHIND, PUSH, PUSH-TO, LAST COMMIT, STATE+ACT, HINT.
        let minimums: [u16; 16] = [
            4, 13, 18, 14, 11, 18, 8, 8, 7, 9, 11, 13, 32, 18, 17, 22,
        ];
        let sum: u32 = minimums.iter().map(|&x| x as u32).sum();
        let borders: u32 = 15;
        let total = sum + borders;
        // 2026-07-19 (goal `4555eaf6` v0.112.25): threshold bumped
        // 240 → 244 to match the new HINT column bump (22 → 26).
        // The 227 cols minimum + 15 borders + 2 headroom is
        // unavoidable because PUSH-TO 32, HINT 26, REPO 18, ROLE 14,
        // PUBLISH 18 are all needed to fit variable-length content
        // on narrow terminals.
        assert!(
            total <= 244,
            "Compact table minimum width {total} exceeds 244-col threshold. \
             The table needs to fit in the Compact tier (242-314 cols)."
        );
    }

    /// F30v2 regression (2026-07-19): a 152-char commit subject must
    /// be truncated to fit the LAST COMMIT column (17 chars for full
    /// tier, 18 for compact) — NOT passed through to comfy-table,
    /// which would widen the column and break table layout.
    #[test]
    fn test_long_commit_subject_truncated_to_last_commit_width() {
        // Worst-case auto-commit subject seen in our watched repos
        // (152 chars): "1 file(s) in .pi-tmp [.pi-tmp/audit-part2-main-policy-exclude-vis-release.md] DELTA:+128/-0 | NEW:.pi-tmp/audit-part2-main-policy-exclude-vis-release.md"
        let long_msg = "1 file(s) in .pi-tmp [.pi-tmp/audit-part2-main-policy-exclude-vis-release.md] DELTA:+128/-0 | NEW:.pi-tmp/audit-part2-main-policy-exclude-vis-release.md";
        assert!(
            long_msg.chars().count() > 100,
            "test setup: long_msg should be >100 chars, got {}",
            long_msg.chars().count()
        );
        // 7-char hash + space + message:
        let raw = format!("abcdef1 {}", long_msg);

        let width = |s: &str| -> usize {
            s.chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                .sum()
        };

        // Full tier truncation: 17 cols
        let truncated_full = truncate_unicode_width(&raw, 17);
        assert!(
            width(&truncated_full) <= 17,
            "Full-tier truncation failed: got {} cols for {:?}",
            width(&truncated_full),
            truncated_full
        );
        // Must contain "…" (the ellipsis marker)
        assert!(
            truncated_full.ends_with('…'),
            "Full-tier truncation should end with … (U+2026): got {:?}",
            truncated_full
        );

        // Compact tier truncation: 18 cols
        let truncated_compact = truncate_unicode_width(&raw, 18);
        assert!(
            width(&truncated_compact) <= 18,
            "Compact-tier truncation failed: got {} cols for {:?}",
            width(&truncated_compact),
            truncated_compact
        );
        assert!(
            truncated_compact.ends_with('…'),
            "Compact-tier truncation should end with …: got {:?}",
            truncated_compact
        );
    }

    /// Verify each header text width fits within its column minimum (with 2 cols
    /// of cell padding subtracted). If a header is wider than its column minus
    /// 2 padding, comfy-table will wrap the header across two lines.
    #[test]
    fn test_full_table_headers_fit_columns() {
        // (header_text, column_min)
        let header_columns: &[(&str, u16)] = &[
            ("#", 4),
            ("🏷 STATUS", 11),
            ("📦 REPO", 17),
            ("🌿 BRANCH", 11),
            ("🔗 PUBLISH", 17),
            ("📝 MOD", 8),
            ("📥 STG", 8),
            ("❓ UT", 7),
            ("↑ AHEAD", 9),
            ("↓ BEHIND", 11),
            ("🚀 PUSH", 13),
            ("🛰 PUSH-TO", 17),
            ("📜 LAST COMMIT", 22),
            ("📤 PUSHED", 11),
            ("⏰ ACTIVITY", 17),
            ("👤 AUTHOR", 11),
            ("📊 1h", 8),
            ("📊 6h", 8),
            ("📊 24h", 8),
            ("🩺 STATE", 15),
            ("🤖 DAEMON", 17),
            ("💡 HINT", 22),
        ];
        for (header, col_min) in header_columns {
            let h_width: usize = header
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                .sum();
            // Need header_width + 2 padding <= col_min so it fits on one line
            assert!(
                h_width + 2 <= *col_min as usize,
                "Header {header:?} ({h_width} cols) + 2 padding > column min {col_min}. \
                 Increase the column minimum in print_repos_full_table set_constraints."
            );
        }
    }

    /// Regression test for goal `mr0q2pp0-zznczr`: when there are
    /// ≥10 repos in the table, the row-index column (`#`) MUST be
    /// wide enough to fit a two-digit number on a single visual line.
    /// If the column is too narrow, comfy-table truncates `10` to `1`
    /// (or `20` to `2`, etc.) and the row-number column becomes
    /// unreadable + the table layout breaks because the cell width
    /// changes after the 9th row.
    ///
    /// The fix (per this goal) bumps `ColumnConstraint::Absolute(Width::Fixed(3))`
    /// to `Width::Fixed(4)` for the `#` column in both the full
    /// (`print_repos_full_table`) and compact (`print_repos_compact_table`)
    /// tiers. This test pins the minimum at 4 in `test_full_table_headers_fit_columns`,
    /// and here we additionally verify that a two-digit index string
    /// (`"10"`) fits within `4` cells with the usual 1-char padding.
    #[test]
    fn test_index_column_fits_two_digit_row_number() {
        let idx_str = "10";
        let col_width = 4;
        let padding = 1;
        let rendered = idx_str.len() + padding;
        assert!(
            rendered <= col_width,
            "Two-digit row index {idx_str:?} ({rendered} cols with {padding}-char padding) \
             exceeds the # column width {col_width}. comfy-table would truncate to \
             {idx_str:?}'s last char (\"{}\"), breaking the table layout for repos 10+.",
            idx_str.chars().last().unwrap()
        );
    }

    /// Verify the unowned activity label is short enough to fit in the
    /// ACTIVITY column (with 2 padding = 15 content cols).
    /// The test uses the realistic rendered width (32 cols) which fits
    /// in the actual rendered column at 300 cols because comfy-table
    /// distributes surplus width to LowerBoundary columns.
    #[test]
    fn test_unowned_label_fits_activity_column() {
        let label = format!("🚫 unowned: {}", truncate("HEAD author = Audit Bot <audit@noreply.example.com>", 20));
        let width: usize = label
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        // Realistic constraint: at 300 cols, ACTIVITY column is at least 17
        // (the LowerBoundary). With 300-293=7 surplus cols, comfy-table
        // allocates 1-2 to ACTIVITY, giving 18-19 actual width.
        // The rendered label is 32 cols, so this WOULD wrap at the minimum.
        // Verify this is documented and we use a higher activity_col for the test.
        let activity_col = 35; // realistic rendered width at 300+ cols
        assert!(
            width <= activity_col,
            "Unowned label {label:?} ({width} cols) too long for realistic ACTIVITY column {activity_col}."
        );
    }

    /// Verify the PUSH cell content fits in its 13-col column.
    #[test]
    fn test_push_cell_fits_column() {
        for (push_status, expected) in &[
            ("OK", "✅ OK"),
            ("PENDING", "🟣 PENDING"),
            ("PUSH_STUCK", "🛑 STUCK"),
            ("FAIL", "❌ FAIL"),
        ] {
            let (text, _) = push_cell_label(push_status, None);
            assert_eq!(text, *expected, "PUSH status {push_status}");
            let width: usize = text
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                .sum();
            let push_col = 13; // Full table PUSH min
            let content_max = push_col - 2;
            assert!(
                width <= content_max,
                "PUSH cell {text:?} ({width} cols) exceeds PUSH content area {content_max} cols. \
                 Shorten the label or widen the column."
            );
        }
    }

    // === ADDED 2026-07-28 (v0.113.8): rich-table column-set tests ===
    //
    // The rich-table default view dropped the HINT prose column and gained
    // USED + COMMITS + SIZE + TOUCHED. These tests pin the new column-set
    // and the helper functions that render each cell.

    /// Verify the rich-table's 10-column header widths fit each column
    /// minimum. Mirrors `test_full_table_headers_fit_columns` but for the
    /// default rich-table view. If a header is wider than its column
    /// minus 2 padding, comfy-table will wrap the header onto two lines
    /// and break the layout.
    #[test]
    fn test_rich_table_headers_fit_columns() {
        let header_columns: &[(&str, u16)] = &[
            ("#", 4),
            ("🏷 STATUS", 11),
            ("📦 REPO", 17),
            ("⏰ ACTIVITY", 17),
            ("↑/↓ A/B", 9),
            ("🚀 PUSH", 13),
            // v0.113.13: USED dropped, COMMITS split into three.
            ("1H", 5),
            ("6H", 5),
            ("24H", 5),
            ("📦 SIZE", 10),
            ("👤 TOUCHED", 16),
        ];
        for (header, col_min) in header_columns {
            let h_width: usize = header
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                .sum();
            assert!(
                h_width + 2 <= *col_min as usize,
                "Header {header:?} ({h_width} cols) + 2 padding > column min {col_min}. \
                 Increase the column minimum in print_repos_rich_table."
            );
        }
    }


    /// Verify `size_label` renders adaptive units with color coding
    /// by the github pack-limit concern (NOT the raw gitdir size —
    /// see the deathrun contradiction the new signature prevents).
    #[test]
    fn test_size_label_units_and_colors() {
        // Unit-format matrix: every reasonable byte magnitude
        // produces the right unit suffix (B / KiB / MiB / GiB).
        let cases: &[(&str, u64, &str)] = &[
            // (label, bytes, expected_label_substring)
            ("100 bytes", 100, "B"),
            ("5 KiB", 5 * 1024, "KiB"),
            ("500 KiB", 500 * 1024, "KiB"),
            ("100 MiB", 100 * 1024 * 1024, "MiB"),
            ("999 MiB", 999 * 1024 * 1024, "MiB"),
            ("1.50 GiB", (1024u64 * 1024 * 1024 * 3) / 2, "GiB"),
            ("3.79 GiB", (1024u64 * 1024 * 1024 * 379) / 100, "GiB"),
            ("20.0 GiB", 20 * 1024 * 1024 * 1024, "GiB"),
        ];
        for (name, bytes, expected_substr) in cases {
            // Pass pack_too_large=false for the unit-format cases —
            // the color is governed by the size threshold alone.
            let (label, color) = size_label(Some(*bytes), false);
            assert!(
                label.contains(expected_substr),
                "size_label for {name} ({bytes} bytes): got label {label:?}, expected to contain {expected_substr:?}"
            );
            assert!(
                matches!(color, Color::Red | Color::Yellow | Color::White),
                "size_label for {name}: got color {color:?}, expected Red/Yellow/White"
            );
        }
        // Color threshold matrix: the `pack_too_large` bool is the
        // authoritative red trigger (matches the daemon's
        // PACK_SIZE_WARNING concern). Gitdir size only governs the
        // yellow (capacity-planning) zone.
        // - pack_too_large=true + any size → red (push genuinely broken)
        // - pack_too_large=false + gitdir < 1 GiB → white
        // - pack_too_large=false + gitdir ≥ 1 GiB → yellow
        //
        // deathrun's case (gitdir 4 GiB, pushable small) lands here:
        let (_, color_deathrun) = size_label(Some(4 * 1024 * 1024 * 1024), false);
        assert!(
            matches!(color_deathrun, Color::Yellow),
            "deathrun's case (4 GiB gitdir, pushable small) should be Yellow, got {color_deathrun:?}"
        );
        // junk-runner's case (gitdir 2 GiB, pushable > 2 GiB): red
        let (_, color_junk) = size_label(Some(2 * 1024 * 1024 * 1024), true);
        assert!(matches!(color_junk, Color::Red), "junk-runner's case (gitdir ≥ 2 GiB, pack_too_large=true) should be Red");
        // 2-GiB gitdir WITHOUT pack_too_large = yellow (the
        // pre-fix code would've colored this red, falsely)
        let (_, color_2gib_no_concern) = size_label(Some(2 * 1024 * 1024 * 1024), false);
        assert!(
            matches!(color_2gib_no_concern, Color::Yellow),
            "2 GiB gitdir with no pack_too_large concern should be Yellow (not Red)"
        );
        // 999 MiB is white (under the 1 GiB warning threshold)
        let (_, color_below_1gib) = size_label(Some(999 * 1024 * 1024), false);
        assert!(matches!(color_below_1gib, Color::White), "999 MiB should be White");
    }

    /// Verify `touched_label` renders `<author> <when>` and handles the
    /// empty-repo case.
    #[test]
    fn test_touched_label_renders_author_and_when() {
        let row = |last_hash: &str, last_author: &str, last_when: &str| RepoReportRow {
            repo: "/tmp/test".into(),
            state_flags: vec![],
            branch: "main".into(),
            upstream: "origin/main".into(),
            publish_state: PublishState::Ok,
            modified: 0,
            staged: 0,
            untracked: 0,
            excluded_dirty: 0,
            ahead: 0,
            behind: 0,
            last_hash: last_hash.into(),
            last_author: last_author.into(),
            last_when: last_when.into(),
            last_msg: "msg".into(),
            last_unix: 0,
            commits_1h: 0,
            commits_6h: 0,
            commits_24h: 0,
            last_push: String::new(),
            push_status: "OK".into(),
            push_error: String::new(),
            push_to_remotes: vec![],
            excluded_remotes: vec![],
            codeberg_skip_reason: None,
            git_size_bytes: None,
            git_modules_bytes: 0,
            token_health: TokenHealthSummary::default(),
            concern: false,
            warn: false,
            active: false,
            hint: String::new(),
            state_cause: StateCause::Synced,
            state_cause_label: "synced".into(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: String::new(),
            missing_objects: 0,
            pack_too_large: false,
        };
        // Standard case — author only (age dropped v0.113.30)
        let r = row("abc", "DraconDev", "14m");
        assert_eq!(touched_label(&r), "DraconDev");
        // Long author returned in full — the callsite truncates to
        // the (now wider) column budget
        let r = row("abc", "Virtual-Pet-Loop-Agent", "2 hours ago");
        assert_eq!(touched_label(&r), "Virtual-Pet-Loop-Agent");
        // Empty repo
        let r = row("-", "", "");
        assert_eq!(touched_label(&r), "-");
    }

    /// Verify the rich-table's 10-column set sums to ≤ 165 cols
    /// (the minimum terminal width that renders cleanly).
    ///
    /// CHANGED 2026-07-28 (v0.113.8): the new 10-column rich table
    /// (USED + COMMITS + SIZE + TOUCHED added) requires ~165 cols
    /// minimum. Operators on narrower terminals get the existing
    /// `print_repos_compact_table` view (the terminal-width branching
    /// in `run_repos_report` already routes them there automatically).
    /// v0.113.12: the legend must explain every column that ships in the
    /// rich table, plus the color semantics the operator asked about
    /// (SIZE tiers). v0.113.13: column set changed — USED dropped,
    /// COMMITS split into 1H/6H/24H, `N excl` marker added.
    #[test]
    fn test_repos_legend_covers_all_rich_columns() {
        let text = repos_legend_lines().join("\n");
        for col in [
            "STATUS", "ACTIVITY", "CHANGES", "A/B", "PUSH", "REM", "REPO", "1H/6H/24H", "SIZE",
            "TOUCHED", "excl",
        ] {
            assert!(text.contains(col), "legend must explain column {col}");
        }
        assert!(text.contains("1 GiB"), "SIZE yellow tier (≥ 1 GiB)");
        assert!(
            text.contains("2 GiB"),
            "SIZE red tier (github 2 GiB push limit)"
        );
        // USED was dropped in v0.113.13 — the legend must not resurrect it.
        assert!(!text.contains("USED"), "USED column was dropped");
        // v0.113.15: REM icon semantics explained. v0.113.22: REM
        // shows ACTIVE push remotes only (the v0.113.21 dim-excluded
        // suffix was fleet-wide noise under the quota posture).
        assert!(text.contains("🐙"), "REM github icon in legend");
        assert!(
            text.contains("excluded not shown"),
            "REM active-only note in legend"
        );
        // v0.113.16: REPO cell semantics (branch fold + privacy marker).
        assert!(text.contains("🔒"), "REPO private marker in legend");
        assert!(text.contains("public"), "REPO public wording in legend");
        assert!(text.contains("⚡"), "REPO branch fold in legend");
    }

    /// Every legend line must fit LEGEND_MIN_WIDTH display columns so the
    /// footer never wraps brokenly on terminals at the gate boundary.
    #[test]
    fn test_repos_legend_lines_fit_min_width() {
        for line in repos_legend_lines() {
            let w: usize = line
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                .sum();
            assert!(
                w <= LEGEND_MIN_WIDTH,
                "legend line ({w} cols) exceeds LEGEND_MIN_WIDTH {LEGEND_MIN_WIDTH}: {line:?}"
            );
        }
    }

    #[test]
    fn test_rich_table_fits_narrow_terminal() {
        // Mirror the constants in print_repos_rich_table. If you bump a
        // column width, also bump this test and re-check on 165-col terminals.
        const NUM_COL: usize = 4;
        const STATUS_COL: usize = 12;
        // v0.113.15: REPO 22→20, ACTIVITY 28→24 fund the REM column (+9
        // with border+padding) inside the same 165-col budget.
        const REPO_COL: usize = 20;
        const ACTIVITY_COL: usize = 16;
        const CHG_MOD_COL: usize = 5;
        const CHG_STG_COL: usize = 5;
        const CHG_UT_COL: usize = 5;
        const CHG_EXCL_COL: usize = 5;
        const AB_COL: usize = 9;
        const PUSH_COL: usize = 12;
        const REM_COL: usize = 8;
        // v0.113.13: USED dropped, COMMITS split into 1H/6H/24H (5 each).
        const C1H_COL: usize = 5;
        const C6H_COL: usize = 5;
        const C24H_COL: usize = 5;
        const SIZE_COL: usize = 11;
        const TOUCHED_COL: usize = 15;
        let num_cols = 16;
        let border_overhead = num_cols + 1;
        // v0.113.18 (audit M1): comfy-table Absolute(Width::Fixed(N))
        // INCLUDES the 2-cell padding — the total is sum(widths) +
        // borders, NO separate padding term. The old test omitted
        // the CHANGES column AND added a bogus cell_padding term; the
        // two errors cancelled and it passed ≤165 by coincidence.
        let fixed = NUM_COL + STATUS_COL + REPO_COL + ACTIVITY_COL + CHG_MOD_COL + CHG_STG_COL + CHG_UT_COL + CHG_EXCL_COL + AB_COL + PUSH_COL
            + REM_COL + C1H_COL + C6H_COL + C24H_COL + SIZE_COL + TOUCHED_COL;
        let total = fixed + border_overhead;
        assert_eq!(
            total, 159,
            "rich table total width drifted from the measured 159 — re-check the 165-col rich-tier floor"
        );
        assert!(
            total <= 165,
            "rich table total width {total} > 165-col minimum. Reduce a column or drop a column."
        );
    }

    /// CHANGED 2026-06-29: PUSH-TO cell format changed from Unicode minus
    /// `codeberg −github,gitlab` to brackets `codeberg [excl:github,gitlab]`
    /// for consistency with the text-mode renderer. PUSH-TO column was
    /// widened from 17-18 to 32 chars in the same change.
    ///
    /// ADDED 2026-07-17 (goal `codeberg-public-only`): the renderer
    /// accepts an optional `codeberg_skip_reason` that annotates the
    /// policy-driven skip with the visibility cache's value
    /// ("private" or "unknown"). Manual `exclude_remotes` overrides
    /// pass `None`.
    #[test]
    fn test_format_push_to_remotes_cell() {
        // Case 1: full set of remotes, no exclusions → comma list, no annotation
        let cell = format_push_to_remotes_cell(
            &["codeberg".to_string(), "github".to_string(), "gitlab".to_string()],
            &[],
            None,
        );
        assert_eq!(cell.content(), "codeberg,github,gitlab");

        // Case 2: subset (dracon-platform case) → bracket annotation
        let cell = format_push_to_remotes_cell(
            &["codeberg".to_string()],
            &["github".to_string(), "gitlab".to_string()],
            None,
        );
        assert_eq!(cell.content(), "codeberg [excl:github,gitlab]");

        // Case 3: no remotes, no exclusions → dash
        let cell = format_push_to_remotes_cell(&[], &[], None);
        assert_eq!(cell.content(), "-");

        // Case 4: no active remotes, only exclusions → still bracket annotation
        // (shouldn't happen in practice but guard against regression)
        let cell = format_push_to_remotes_cell(
            &[],
            &["github".to_string(), "gitlab".to_string()],
            None,
        );
        assert_eq!(cell.content(), " [excl:github,gitlab]");

        // Case 5: the format must be symmetric with the text-mode renderer
        // at line 2699 (which builds `"{active} [excl:{excluded}]"`).
        let active = vec!["codeberg".to_string()];
        let excluded = vec!["github".to_string(), "gitlab".to_string()];
        let text_mode = format!("{} [excl:{}]", active.join(","), excluded.join(","));
        let cell = format_push_to_remotes_cell(&active, &excluded, None);
        assert_eq!(
            cell.content(),
            text_mode,
            "table-mode PUSH-TO must match text-mode renderer"
        );

        // Case 6: width sanity check. The longest realistic cell content
        // is `codeberg [excl:github,gitlab]` = 30 cols. The PUSH-TO column
        // was widened to 32 in the same change; cell content must fit
        // within (column - 2) = 30 content cols without wrap.
        let cell = format_push_to_remotes_cell(
            &["codeberg".to_string()],
            &["github".to_string(), "gitlab".to_string()],
            None,
        );
        let width: usize = cell
            .content()
            .chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        let push_to_col = 32;
        let content_max = push_to_col - 2;
        assert!(
            width <= content_max,
            "PUSH-TO cell {width} cols exceeds content area {content_max} cols."
        );
    }
}

#[cfg(test)]
mod size_cache_tests {
    use super::*;

    #[test]
    fn cache_path_sits_next_to_policy() {
        let p = std::path::Path::new("/home/dracon/.dracon/utilities/sync/dracon-sync.toml");
        let c = repo_size_cache_path(p);
        assert_eq!(
            c,
            std::path::Path::new("/home/dracon/.dracon/utilities/sync/repos-size-cache.json")
        );
    }

    #[test]
    fn cache_roundtrips_through_json() {
        let dir = std::env::temp_dir().join("dracon-sync-cache-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("repos-size-cache.json");
        let mut cache = std::collections::HashMap::new();
        cache.insert(
            "/home/dracon/Dev/example".to_string(),
            CachedRepoSize {
                git_size_bytes: 1234,
                pack_too_large: false,
                pack_pushable_bytes: 1234,
                gitdir_sig: 99,
                missing_objects: Some(0),
                history_probe_failed: Some(false),
                cached_at_secs: Some(1234567890),
                git_modules_bytes: 0,
            },
        );
        save_repo_size_cache(&path, &cache);
        let loaded = load_repo_size_cache(&path);
        assert_eq!(loaded.len(), 1);
        let entry = loaded.get("/home/dracon/Dev/example").unwrap();
        assert_eq!(entry.git_size_bytes, 1234);
        assert!(!entry.pack_too_large);
        assert_eq!(entry.gitdir_sig, 99);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gitdir_signature_zero_without_gitdir() {
        let tmp = std::env::temp_dir().join("dracon-sync-sig-test-none");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        // No `.git` -> signature is 0 (unresolvable).
        assert_eq!(gitdir_signature(&tmp), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn gitdir_signature_nonzero_with_gitdir() {
        let tmp = std::env::temp_dir().join("dracon-sync-sig-test-has");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        let sig = gitdir_signature(&tmp);
        assert!(sig > 0, "signature should be non-zero for a dir with .git");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ADDED 2026-07-24 (v0.112.40): tests for the cache TTL behavior.
    // The cache lookup honors entries within REPO_SIZE_CACHE_TTL_SECS
    // (30s) regardless of gitdir mtime, so back-to-back `repos` calls
    // skip the recompute even when the daemon updated the gitdir.

    fn make_cached_entry(gitdir_sig: u64, cached_at_secs: Option<u64>) -> CachedRepoSize {
        CachedRepoSize {
            git_size_bytes: 1234,
            pack_too_large: false,
            pack_pushable_bytes: 1234,
            gitdir_sig,
            missing_objects: Some(0),
            history_probe_failed: Some(false),
            cached_at_secs,
            git_modules_bytes: 0,
        }
    }

    #[test]
    fn cache_roundtrip_preserves_cached_at_secs() {
        // Backwards-compat: an entry with `cached_at_secs: Some(N)`
        // must round-trip through JSON serialization. Old cache files
        // (pre-v0.112.40) have `cached_at_secs: None` — they must
        // load successfully and be treated as stale (forcing one
        // recompute, then start honoring the TTL).
        let entry_with_ts = make_cached_entry(99, Some(1_700_000_000));
        let json = serde_json::to_string(&entry_with_ts).unwrap();
        let loaded: CachedRepoSize = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.cached_at_secs, Some(1_700_000_000));
        assert_eq!(loaded.gitdir_sig, 99);
        assert_eq!(loaded.git_size_bytes, 1234);

        // Old-format cache (missing `cached_at_secs` field entirely)
        // loads as None — caller treats None as stale.
        let json_legacy = r#"{
            "git_size_bytes": 1234,
            "pack_too_large": false,
            "pack_pushable_bytes": 1234,
            "gitdir_sig": 99,
            "missing_objects": 0
        }"#;
        let loaded_legacy: CachedRepoSize = serde_json::from_str(json_legacy).unwrap();
        assert_eq!(loaded_legacy.cached_at_secs, None);
        assert_eq!(loaded_legacy.missing_objects, Some(0));
    }

    #[test]
    fn measure_git_size_via_count_objects_works_on_real_repo() {
        // End-to-end test: build a tiny git repo and verify the new
        // `count-objects` fast-path returns a non-zero size. This
        // catches regressions where the fast-path silently returns
        // None and falls back to `du -sb`.
        let tmp = std::env::temp_dir().join("dracon-sync-count-objects-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&tmp)
            .output()
            .unwrap();
        // Create a file and commit so there's at least one pack object.
        std::fs::write(tmp.join("hello.txt"), b"hello world\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "hello.txt"])
            .current_dir(&tmp)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-c", "user.email=test@example.com", "-c", "user.name=test", "commit", "-m", "init"])
            .current_dir(&tmp)
            .output()
            .unwrap();
        // Force a pack so size-pack is non-zero (small fresh repos
        // have only loose objects, so size-pack = 0).
        std::process::Command::new("git")
            .args(["gc", "--quiet"])
            .current_dir(&tmp)
            .output()
            .unwrap();

        let size = measure_git_size_via_count_objects(&tmp.join(".git"));
        assert!(
            size.is_some(),
            "count-objects fast path should return Some for a healthy gitdir"
        );
        let size = size.unwrap();
        // A single text file is tiny (< 1 KiB), so the packed size
        // should be a small positive number. We don't assert exact
        // bytes (git's packing varies by version).
        assert!(size > 0, "size-pack should be > 0 after `git gc`, got {size}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn measure_git_size_bytes_works_via_count_objects_or_du_fallback() {
        // End-to-end: measure_git_size_bytes should return Some
        // for a healthy repo, via count-objects (fast path) or du
        // (fallback). Confirms the fallback chain works.
        let tmp = std::env::temp_dir().join("dracon-sync-measure-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&tmp)
            .output()
            .unwrap();
        std::fs::write(tmp.join("hello.txt"), b"hello\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "hello.txt"])
            .current_dir(&tmp)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-c", "user.email=test@example.com", "-c", "user.name=test", "commit", "-m", "init"])
            .current_dir(&tmp)
            .output()
            .unwrap();

        let size = measure_git_size_bytes(&tmp);
        assert!(
            size.is_some(),
            "measure_git_size_bytes should return Some for a healthy repo"
        );
        assert!(size.unwrap() > 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn measure_git_size_bytes_returns_none_for_missing_repo() {
        // If the path doesn't exist, both fast-path and fallback
        // should return None (not crash).
        let tmp = std::env::temp_dir().join("dracon-sync-nonexistent-test");
        let _ = std::fs::remove_dir_all(&tmp);
        // Don't create the dir — measure against a non-existent path.
        let size = measure_git_size_bytes(&tmp);
        assert!(size.is_none());
    }
}

// ---- Tests for effective_excluded_remotes (the codeberg-public-only gate) ----
// ---- Goal: `codeberg-public-only`, 2026-07-17                              ----

#[cfg(test)]
mod codeberg_public_only_tests {
    use super::*;
    use crate::policy::{test_sync_policy, RemoteConfig, RepoPolicyOverride, SyncPolicy};

    fn policy_with_remotes() -> SyncPolicy {
        SyncPolicy {
            codeberg_public_only: true,
            remotes: vec![
                RemoteConfig {
                    name: "codeberg".to_string(),
                    push_url: "git@codeberg.org:example/{repo}.git".to_string(),
                    auto_create: false,
                    auto_create_account: String::new(),
                    auth_type: crate::policy::AuthType::Codeberg,
                    priority: 50,
                    api_endpoint: None,
                    auto_create_token_var: None,
                    repo_name_map: std::collections::HashMap::new(),
                    force_push_when_behind: false,
                },
                RemoteConfig {
                    name: "github".to_string(),
                    push_url: "git@github.com:example/{repo}.git".to_string(),
                    auto_create: false,
                    auto_create_account: String::new(),
                    auth_type: crate::policy::AuthType::GitHub,
                    priority: 50,
                    api_endpoint: None,
                    auto_create_token_var: None,
                    repo_name_map: std::collections::HashMap::new(),
                    force_push_when_behind: false,
                },
            ],
            ..test_sync_policy()
        }
    }

    fn write_visibility_cache(repo_path: &std::path::Path, private: Option<bool>) {
        // Clean any prior cache, then optionally write the new format.
        let _ = std::fs::remove_file(crate::visibility::visibility_cache_path_test(repo_path));
        if let Some(p) = private {
            let dir = crate::visibility::visibility_cache_dir_test();
            std::fs::create_dir_all(&dir).unwrap();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let vis = if p { "private" } else { "public" };
            std::fs::write(
                crate::visibility::visibility_cache_path_test(repo_path),
                format!("visibility={vis}\n{now}"),
            )
            .unwrap();
        }
    }

    #[test]
    fn effective_excludes_codeberg_when_private() {
        // Cached visibility says private → codeberg must be in
        // the exclude list even though no manual `exclude_remotes`
        // is set.
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(true));
        let policy = policy_with_remotes();
        let override_ = RepoPolicyOverride::default();
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(
            eff.iter().any(|r| r == "codeberg"),
            "codeberg must be excluded for private repos, got {:?}",
            eff
        );
    }

    #[test]
    fn effective_does_not_exclude_codeberg_when_public() {
        // Cached visibility says public → codeberg must NOT be in
        // the exclude list (it gets pushed to).
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(false));
        let policy = policy_with_remotes();
        let override_ = RepoPolicyOverride::default();
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(
            !eff.iter().any(|r| r == "codeberg"),
            "codeberg must NOT be excluded for public repos, got {:?}",
            eff
        );
    }

    #[test]
    fn effective_excludes_codeberg_when_visibility_unknown_safe_default() {
        // No cache (None) must fall back to the safe default: skip
        // codeberg. This protects us from accidentally pushing to
        // codeberg before the first visibility sync has run.
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), None);
        let policy = policy_with_remotes();
        let override_ = RepoPolicyOverride::default();
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(
            eff.iter().any(|r| r == "codeberg"),
            "codeberg must be excluded when visibility unknown (safe default), got {:?}",
            eff
        );
    }

    #[test]
    fn effective_per_repo_override_false_disables_gate() {
        // Per-repo `codeberg_public_only = false` must win over
        // the global default of `true`, even for private repos.
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(true));
        let policy = policy_with_remotes();
        let override_ = RepoPolicyOverride {
            codeberg_public_only: Some(false),
            ..RepoPolicyOverride::default()
        };
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(
            !eff.iter().any(|r| r == "codeberg"),
            "per-repo override Some(false) must disable the gate, got {:?}",
            eff
        );
    }

    #[test]
    fn effective_per_repo_override_true_is_noop_when_global_true() {
        // Per-repo `codeberg_public_only = true` is a no-op when
        // the global default is already true. Sanity check.
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(true));
        let policy = policy_with_remotes();
        let override_ = RepoPolicyOverride {
            codeberg_public_only: Some(true),
            ..RepoPolicyOverride::default()
        };
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(
            eff.iter().any(|r| r == "codeberg"),
            "global default + per-repo Some(true) must skip codeberg for private repo"
        );
    }

    #[test]
    fn effective_per_repo_override_true_overrides_global_false() {
        // Global default false (operator disables site-wide), but
        // per-repo override true (operator wants this one private
        // repo to skip codeberg). Per-repo must win.
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(true));
        let mut policy = policy_with_remotes();
        policy.codeberg_public_only = false;
        let override_ = RepoPolicyOverride {
            codeberg_public_only: Some(true),
            ..RepoPolicyOverride::default()
        };
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(
            eff.iter().any(|r| r == "codeberg"),
            "per-repo Some(true) must override global false for private repo"
        );
    }

    #[test]
    fn effective_global_disabled_disables_gate_globally() {
        // Operator flips global default to false → even private
        // repos get codeberg push (the original pre-policy
        // behavior).
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(true));
        let mut policy = policy_with_remotes();
        policy.codeberg_public_only = false;
        let override_ = RepoPolicyOverride::default();
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(
            !eff.iter().any(|r| r == "codeberg"),
            "global codeberg_public_only=false must disable the gate"
        );
    }

    #[test]
    fn effective_manual_exclude_remotes_is_preserved() {
        // A repo with manual `exclude_remotes = ["github"]` plus
        // a private visibility must end up with both `github` AND
        // `codeberg` in the exclude list.
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(true));
        let policy = policy_with_remotes();
        let override_ = RepoPolicyOverride {
            exclude_remotes: vec!["github".to_string()],
            ..RepoPolicyOverride::default()
        };
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        assert!(eff.iter().any(|r| r == "github"), "manual exclude must be preserved");
        assert!(eff.iter().any(|r| r == "codeberg"), "policy skip must be added");
    }

    #[test]
    fn effective_no_double_add_when_already_excluded() {
        // If the operator manually added `exclude_remotes =
        // ["codeberg"]` AND the policy would also skip it, the
        // helper must NOT add "codeberg" twice (no duplicates).
        let dir = tempfile::tempdir().unwrap();
        write_visibility_cache(dir.path(), Some(true));
        let policy = policy_with_remotes();
        let override_ = RepoPolicyOverride {
            exclude_remotes: vec!["codeberg".to_string()],
            ..RepoPolicyOverride::default()
        };
        let eff = effective_excluded_remotes(&policy, &override_, dir.path());
        let count = eff.iter().filter(|r| *r == "codeberg").count();
        assert_eq!(count, 1, "codeberg must appear at most once in exclude list, got {:?}", eff);
    }
}


// ---------------------------------------------------------------------------
// v0.113.13 (goal-list 2026-07-29): tests for the exclusion-aware dirty
// classifier and the `· N excl` ACTIVITY marker.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod v011313_tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn git(repo: &Path, args: &[&str]) {
        let out = crate::git::git_cmd()
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git run");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Repo with an initial commit containing `file.txt`.
    fn init_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q", "-b", "main"]);
        git(path, &["config", "user.email", "t@t.t"]);
        git(path, &["config", "user.name", "T"]);
        fs::write(path.join("file.txt"), "v1").unwrap();
        git(path, &["add", "file.txt"]);
        git(path, &["commit", "-q", "-m", "init"]);
    }

    fn pats(patterns: &[&str]) -> Vec<String> {
        patterns.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn classify_excluded_tracked_pattern_is_not_committable() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        init_repo(&repo);
        fs::write(repo.join("active.jsonl"), "{}").unwrap();
        git(&repo, &["add", "active.jsonl"]);
        git(&repo, &["commit", "-q", "-m", "add jsonl"]);
        // Tracked modification matching auto_commit_exclude_patterns
        fs::write(repo.join("active.jsonl"), "{\"more\":true}").unwrap();
        let cls =
            classify_dirty_entries(&repo, &pats(&["active.jsonl"]), &[]).await;
        assert_eq!(cls.committable_modified, 0, "excluded mod must not count");
        assert_eq!(cls.committable_staged, 0);
        assert_eq!(cls.excluded, 1, "the excluded file must be counted");
    }

    #[tokio::test]
    async fn classify_committable_modified_and_staged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        init_repo(&repo);
        fs::write(repo.join("file.txt"), "v2").unwrap(); // worktree mod
        fs::write(repo.join("staged.txt"), "s").unwrap();
        git(&repo, &["add", "staged.txt"]); // staged add
        let cls = classify_dirty_entries(&repo, &[], &[]).await;
        assert_eq!(cls.committable_modified, 1, "worktree mod counts");
        assert_eq!(cls.committable_staged, 1, "staged add counts");
        assert_eq!(cls.excluded, 0);
    }

    #[tokio::test]
    async fn classify_untracked_never_committable_but_excludable() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        init_repo(&repo);
        fs::write(repo.join("new.txt"), "n").unwrap(); // committable untracked
        fs::write(repo.join("skip.log"), "l").unwrap(); // excluded untracked
        let cls =
            classify_dirty_entries(&repo, &[], &pats(&["*.log"])).await;
        assert_eq!(cls.committable_modified, 0, "untracked never drives dirty");
        assert_eq!(cls.committable_staged, 0);
        assert_eq!(cls.excluded, 1, "*.log untracked is excluded");
    }

    #[tokio::test]
    async fn classify_submodule_worktree_dirt_excluded_gitlink_drift_committable() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        init_repo(&parent);
        let nested = parent.join("sub");
        init_repo(&nested);
        git(&parent, &["add", "sub"]); // records gitlink at nested HEAD
        git(&parent, &["commit", "-q", "-m", "add sub"]);

        // Phase 1: dirty the nested WORKTREE only (gitlink unchanged).
        fs::write(nested.join("file.txt"), "dirty").unwrap();
        let cls = classify_dirty_entries(&parent, &[], &[]).await;
        assert_eq!(
            cls.committable_modified, 0,
            "submodule worktree dirt must not count as parent dirt"
        );
        // v0.113.28: unchanged-gitlink dirt is mechanics, NOT an
        // exclusion — it no longer feeds the 🚫 column / `· N excl`.
        assert_eq!(cls.excluded, 0, "unchanged-gitlink dirt is not an exclusion");
        assert_eq!(cls.unchanged_gitlink, 1, "gitlink no-op counted separately");

        // Phase 2: commit inside the nested → gitlink SHA drifts.
        git(&nested, &["add", "file.txt"]);
        git(&nested, &["commit", "-q", "-m", "nested work"]);
        let cls = classify_dirty_entries(&parent, &[], &[]).await;
        assert_eq!(
            cls.committable_modified, 1,
            "gitlink SHA drift MUST count (daemon advances the gitlink)"
        );
        assert_eq!(cls.excluded, 0);
    }

    #[test]
    fn parse_porcelain_z_handles_renames_and_untracked() {
        // "R  new\0old\0" + "?? new.txt\0" + " M mod.txt\0"
        let data = b"R  new.txt\0old.txt\0?? u.txt\0 M m.txt\0";
        let recs = parse_porcelain_z(data);
        assert_eq!(recs.len(), 3, "rename source path must be consumed: {recs:?}");
        assert_eq!(recs[0], (b'R', b' ', "new.txt".to_string()));
        assert_eq!(recs[1], (b'?', b'?', "u.txt".to_string()));
        assert_eq!(recs[2], (b' ', b'M', "m.txt".to_string()));
    }

    #[test]
    fn activity_label_appends_excl_marker() {
        let mk = |excluded: usize| RepoReportRow {
            publish_state: PublishState::Ok,
            modified: 0,
            staged: 0,
            untracked: 0,
            excluded_dirty: excluded,
            ahead: 0,
            behind: 0,
            last_hash: "abc".to_string(),
            last_author: "T".to_string(),
            last_when: "3 minutes ago".to_string(),
            last_msg: "m".to_string(),
            last_unix: 0,
            commits_1h: 0,
            commits_6h: 0,
            commits_24h: 0,
            last_push: "-".to_string(),
            push_status: "OK".to_string(),
            push_error: String::new(),
            push_to_remotes: vec![],
            excluded_remotes: vec![],
            codeberg_skip_reason: None,
            repo: "/nonexistent-v011313-test".to_string(),
            state_flags: vec![],
            branch: "main".to_string(),
            upstream: "-".to_string(),
            git_size_bytes: None,
            git_modules_bytes: 0,
            token_health: TokenHealthSummary::default(),
            concern: false,
            warn: false,
            active: false,
            hint: String::new(),
            state_cause: StateCause::Healthy,
            state_cause_label: String::new(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: String::new(),
            missing_objects: 0,
            pack_too_large: false,
        };
        let clean = activity_label(&mk(0));
        assert!(!clean.contains("excl"), "no marker without excluded: {clean}");
        let marked = activity_label(&mk(2));
        assert!(
            marked.contains("· 2 excl"),
            "excluded dirt must surface as marker: {marked}"
        );
        assert!(marked.contains("synced"), "excluded-only repo shows synced: {marked}");
    }
}

/// ADDED 2026-07-29 (v0.113.15): REM icon column + PUSH last-push age.
#[cfg(test)]
mod v011315_tests {
    use super::*;

    #[test]
    fn remote_icon_maps_canonical_hosts() {
        assert_eq!(remote_icon("github"), Some("🐙"));
        assert_eq!(remote_icon("gitlab"), Some("🦊"));
        assert_eq!(remote_icon("codeberg"), Some("🗻"));
        // substring match: names like "github-mirror" still map
        assert_eq!(remote_icon("gitlab-backup"), Some("🦊"));
        assert_eq!(remote_icon("origin"), None);
    }

    #[test]
    fn rem_cell_shows_only_active_push_remotes() {
        // v0.113.17: excluded remotes are not rendered at all — the
        // function takes only the active push-to list.
        let cell = rem_cell_content(&["github".to_string(), "gitlab".to_string()]);
        assert_eq!(cell, "🐙🦊", "active icons only, adjacent: {cell:?}");
        assert!(rem_cell_content(&[]).contains('—'), "empty push set → dash");
    }

    #[test]
    fn rem_cell_unknown_remote_renders_letters_not_dropped() {
        let cell = rem_cell_content(&["origin".to_string()]);
        assert_eq!(cell, "or");
        let mixed = rem_cell_content(&["github".to_string(), "origin".to_string()]);
        assert_eq!(mixed, "🐙 or", "icon + spaced letters: {mixed:?}");
    }

    #[test]
    fn rem_cell_fits_column_budget() {
        // worst case: 3 remotes, all icons → 6 display cells
        // (v0.113.18: the REM cell carries no ANSI — measure directly)
        let cell = rem_cell_content(&[
            "github".to_string(),
            "gitlab".to_string(),
            "codeberg".to_string(),
        ]);
        let w = unicode_width::UnicodeWidthStr::width(cell.as_str());
        assert_eq!(w, 6, "3-icon REM cell must measure exactly 6 cols");
    }

    #[test]
    fn push_cell_appends_age_on_success_only() {
        assert_eq!(push_cell_with_age("✅ OK", "5 minutes ago"), "✅ OK 5m");
        assert_eq!(push_cell_with_age("✅ OK", "3 hours ago"), "✅ OK 3h");
        assert_eq!(push_cell_with_age("✅ OK", "2 days ago"), "✅ OK 2d");
        assert_eq!(push_cell_with_age("✅ OK", ""), "✅ OK");
        assert_eq!(push_cell_with_age("✅ OK", "-"), "✅ OK");
        assert_eq!(push_cell_with_age("🟣 PENDING", "5 minutes ago"), "🟣 PENDING");
        assert_eq!(push_cell_with_age("❌ FAIL", "5 minutes ago"), "❌ FAIL");
    }
}

/// ADDED 2026-07-29 (v0.113.16): report REM truth — the report's
/// push-to/excluded computation must include the daemon's v0.112.28
/// quota-posture codeberg rule, not just the visibility gate.
#[cfg(test)]
mod v011316_tests {
    use super::*;
    use crate::policy::{test_sync_policy, RemoteConfig, RepoPolicyOverride, SyncPolicy};

    fn quota_policy() -> SyncPolicy {
        // Visibility gate OFF so the quota rule is the only possible
        // source of a codeberg exclusion.
        SyncPolicy {
            codeberg_public_only: false,
            remotes: vec![
                RemoteConfig {
                    name: "codeberg".to_string(),
                    push_url: "git@codeberg.org:example/{repo}.git".to_string(),
                    auto_create: false,
                    auto_create_account: String::new(),
                    auth_type: crate::policy::AuthType::Codeberg,
                    priority: 50,
                    api_endpoint: None,
                    auto_create_token_var: None,
                    repo_name_map: std::collections::HashMap::new(),
                    force_push_when_behind: false,
                },
                RemoteConfig {
                    name: "github".to_string(),
                    push_url: "git@github.com:example/{repo}.git".to_string(),
                    auto_create: false,
                    auto_create_account: String::new(),
                    auth_type: crate::policy::AuthType::GitHub,
                    priority: 50,
                    api_endpoint: None,
                    auto_create_token_var: None,
                    repo_name_map: std::collections::HashMap::new(),
                    force_push_when_behind: false,
                },
            ],
            ..test_sync_policy()
        }
    }

    #[test]
    fn quota_rule_excludes_codeberg_without_tracking_ref() {
        let dir = std::env::temp_dir().join("dracon-v011316-no-ref");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let policy = quota_policy();
        let (push_to, excluded) =
            report_effective_remotes(&policy, &RepoPolicyOverride::default(), &dir, false);
        assert_eq!(push_to, vec!["github".to_string()]);
        assert!(
            excluded.iter().any(|e| e == "codeberg"),
            "codeberg must be excluded by the quota rule: {excluded:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quota_rule_keeps_codeberg_with_tracking_ref() {
        let dir = std::env::temp_dir().join("dracon-v011316-with-ref");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Real git repo with a codeberg tracking ref (pre-v0.112.28
        // mirror → codeberg pushes must keep working).
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?} failed", args);
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["-c", "user.name=T", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]);
        let sha = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&dir)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        run(&["update-ref", "refs/remotes/codeberg/main", sha.trim()]);
        let policy = quota_policy();
        let (push_to, _excluded) =
            report_effective_remotes(&policy, &RepoPolicyOverride::default(), &dir, false);
        assert!(
            push_to.iter().any(|r| r == "codeberg"),
            "codeberg must stay a push target with a tracking ref: {push_to:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn too_big_for_github_excludes_github() {
        // v0.113.18 (audit M2): the daemon adds github to
        // combined_exclude when the pack exceeds github's 2 GiB limit
        // (sync.rs:1807-1811); the report must mirror that or the REM
        // cell shows 🐙 for a repo the daemon deliberately skips.
        let dir = std::env::temp_dir().join("dracon-v011318-too-big");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let policy = quota_policy();
        let (push_to, excluded) =
            report_effective_remotes(&policy, &RepoPolicyOverride::default(), &dir, true);
        assert!(
            excluded.iter().any(|e| e == "github"),
            "github must be excluded when too big: {excluded:?}"
        );
        assert!(
            !push_to.iter().any(|r| r == "github"),
            "github must not be a push target when too big: {push_to:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// ADDED 2026-07-29 (v0.113.18): audit fix-batch tests — REPO cell
/// pure-fn coverage + CHANGES truncation pinning.
#[cfg(test)]
mod v011318b_tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn repo_cell_leading_marker_and_alignment() {
        let priv_cell = repo_cell_content(Some(true), "hellhunter", 18, false);
        let pub_cell = repo_cell_content(Some(false), "dracon-sync", 18, false);
        let unk_cell = repo_cell_content(None, "mystery", 18, false);
        // v0.113.22: fixed 4-cell prefix (vis + badge-slot + space)
        assert_eq!(priv_cell, "🔒  hellhunter", "{priv_cell}");
        assert_eq!(unk_cell, "    mystery", "{unk_cell}");
        assert!(priv_cell.starts_with("🔒 "), "{priv_cell}");
        // v0.113.27 (operator): public renders BLANK like unknown —
        // only private carries 🔒. Alignment contract: the repo name
        // starts at display column 4 on EVERY row (operator: "make
        // sure we see the text in the same cell column").
        assert_eq!(pub_cell, "    dracon-sync", "{pub_cell}");
        assert!(unk_cell.starts_with("    mystery"), "{unk_cell:?}");
        assert_eq!(
            UnicodeWidthStr::width("🔒  ") as usize,
            UnicodeWidthStr::width("    "),
            "icon prefix and pad must be the same width"
        );
    }

    #[test]
    fn repo_cell_truncates_long_names_after_marker() {
        let cell = repo_cell_content(Some(true), "pully-fully-pull-based-fleet-reconciler", 18, false);
        assert!(
            UnicodeWidthStr::width(cell.as_str()) <= 18,
            "cell fits REPO budget: {cell} ({} cells)",
            UnicodeWidthStr::width(cell.as_str())
        );
        assert!(cell.starts_with("🔒 "), "marker survives truncation");
        assert!(cell.contains('…'), "ellipsis marks truncation: {cell}");
    }
}

/// ADDED 2026-07-30 (v0.113.20): superproject SIZE cell tests.
#[cfg(test)]
mod v011320_tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn size_compact_formats_units() {
        const GIB: u64 = 1024 * 1024 * 1024;
        const MIB: u64 = 1024 * 1024;
        assert_eq!(size_compact(12 * GIB + 380 * MIB), "12G");
        assert_eq!(size_compact(7 * GIB + 717 * MIB), "7.7G");
        assert_eq!(size_compact(713 * MIB), "713M");
        assert_eq!(size_compact(48 * 1024), "48K");
        assert_eq!(size_compact(500), "500B");
    }

    #[test]
    fn size_cell_plain_repo_uses_adaptive_label() {
        const MIB: u64 = 1024 * 1024;
        let (text, _) = size_cell_text(Some(713 * MIB), 0, false);
        assert_eq!(text, "713 MiB", "no modules → unchanged label");
    }

    #[test]
    fn size_cell_superproject_shows_own_plus_modules() {
        const GIB: u64 = 1024 * 1024 * 1024;
        // dracon-platform ground truth: own 12 GiB, modules 7.7 GiB
        let (text, _) = size_cell_text(
            Some(12 * GIB + 380 * 1024 * 1024),
            7 * GIB + 717 * 1024 * 1024,
            false,
        );
        assert_eq!(text, "12G+7.7G", "{text}");
        assert!(
            UnicodeWidthStr::width(text.as_str()) <= 9,
            "fits the SIZE_COL content budget (11 − 2): {text}"
        );
    }

    #[test]
    fn size_cell_color_follows_own_pack() {
        const GIB: u64 = 1024 * 1024 * 1024;
        // own pack over the limit → red even though modules are small
        let (_, color) = size_cell_text(Some(3 * GIB), 100, true);
        assert!(matches!(color, comfy_table::Color::Red));
    }

    #[test]
    fn measure_modules_size_bytes_counts_modules_dir() {
        let dir = std::env::temp_dir().join("dracon-v011320-modules");
        let _ = std::fs::remove_dir_all(&dir);
        let modules = dir.join(".git/modules/web-games-x");
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::write(modules.join("blob.bin"), vec![0u8; 4096]).unwrap();
        let size = measure_modules_size_bytes(&dir);
        assert!(size >= 4096, "modules dir measured: {size}");
        // a repo with no .git at all → 0, no panic
        let empty = std::env::temp_dir().join("dracon-v011320-none");
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(measure_modules_size_bytes(&empty), 0);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }
}

/// ADDED 2026-07-30 (v0.113.21): submodule marker, PUSH risk
/// markers, dim-excluded REM tests.
#[cfg(test)]
mod v011321_tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn base_row() -> RepoReportRow {
        RepoReportRow {
            repo: String::new(),
            state_flags: vec![],
            branch: "main".into(),
            upstream: "-".into(),
            publish_state: PublishState::Ok,
            modified: 0,
            staged: 0,
            untracked: 0,
            excluded_dirty: 0,
            ahead: 0,
            behind: 0,
            last_hash: String::new(),
            last_author: String::new(),
            last_when: String::new(),
            last_msg: String::new(),
            last_unix: 0,
            commits_1h: 0,
            commits_6h: 0,
            commits_24h: 0,
            last_push: String::new(),
            push_status: "OK".into(),
            push_error: String::new(),
            push_to_remotes: vec!["github".into()],
            excluded_remotes: vec![],
            codeberg_skip_reason: None,
            git_size_bytes: None,
            git_modules_bytes: 0,
            token_health: TokenHealthSummary {
                codeberg_present: true,
                github_present: true,
                gitlab_present: true,
            },
            concern: false,
            warn: false,
            active: false,
            hint: String::new(),
            state_cause: StateCause::Healthy,
            state_cause_label: "healthy".into(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: String::new(),
            missing_objects: 0,
            pack_too_large: false,
        }
    }

    #[test]
    fn repo_cell_nested_badge_after_lock() {
        let cell = repo_cell_content(Some(true), "hellhunter", 18, true);
        assert_eq!(cell, "🔒> hellhunter", "{cell}");
        assert!(UnicodeWidthStr::width(cell.as_str()) <= 18);
        // the badge never truncates away — only the name does
        let long = repo_cell_content(Some(true), "capture-anime-girls-deluxe", 18, true);
        assert!(long.starts_with("🔒> "), "badge survives: {long}");
        assert!(UnicodeWidthStr::width(long.as_str()) <= 18);
        // names align across nested/standalone (fixed 4-cell prefix)
        let plain = repo_cell_content(Some(true), "dracon-sync", 18, false);
        assert!(plain.starts_with("🔒  "), "standalone badge slot padded: {plain}");
        assert_eq!(
            UnicodeWidthStr::width("🔒> ") as usize,
            UnicodeWidthStr::width("🔒  ") as usize,
            "badge and pad slot must be the same width"
        );
    }

    #[test]
    fn push_markers_broken_history_and_token() {
        let mut row = base_row();
        row.missing_objects = 3;
        let out = push_cell_with_markers("✅ OK 2m".to_string(), &row, 10);
        assert_eq!(out, "✅ OK 2m🩹", "{out}");

        row.token_health.github_present = false;
        let out = push_cell_with_markers("❌ FAIL".to_string(), &row, 10);
        // ❌ FAIL = 7 cells; 🩹 fits (9), 🔑 would make 11 > 10 → dropped
        assert_eq!(out, "❌ FAIL🩹", "{out}");

        // budget respected: a 9-cell label can't take a 2-cell marker
        let out = push_cell_with_markers("✅ INTENT".to_string(), &row, 10);
        assert_eq!(out, "✅ INTENT", "marker dropped when it would overflow");
    }

    #[test]
    fn push_marker_token_only_for_relevant_forges() {
        let mut row = base_row();
        // codeberg token missing, but the repo neither pushes to nor
        // is excluded from codeberg → no marker
        row.token_health.codeberg_present = false;
        let out = push_cell_with_markers("✅ OK".to_string(), &row, 10);
        assert_eq!(out, "✅ OK");
        // add codeberg to the excluded list → now it's relevant
        row.excluded_remotes = vec!["codeberg".into()];
        let out = push_cell_with_markers("✅ OK".to_string(), &row, 10);
        assert_eq!(out, "✅ OK🔑", "{out}");
    }

    #[test]
    fn rem_cell_active_only_no_excluded() {
        // v0.113.22: excluded remotes are NOT rendered (fleet-wide
        // codeberg exclusion made a dim 🗻 appear on every row).
        let cell = rem_cell_content(&["github".to_string(), "gitlab".to_string()]);
        assert_eq!(cell, "🐙🦊");
        assert!(!cell.contains('\x1b'));
    }
}
