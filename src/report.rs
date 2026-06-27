use anyhow::Result;
use dracon_git::GitService;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};

use crate::git::gh_cmd;

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
    default_auto_resolve_unmerged, default_push_debounce_secs, default_untracked_warn_threshold,
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
///                    (currently being processed)
///   - "pushing Xm" : push_status=PENDING, push has been in
///                    progress for X minutes
///   - "dirty Xm"    : dirty tracked work exists, last commit
///                    was X minutes ago
///   - "synced Xm"  : clean, in sync, recent commit (within 1h)
///   - "idle Xm"    : clean, no in-flight, last commit 1h-24h ago
///   - "cold Xd"    : clean, no activity for > 24h
///   - "—"          : unknown / no data
pub(crate) fn activity_label(row: &RepoReportRow) -> String {
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
    if let StateCause::Unowned { detail, .. } = &row.state_cause {
        return format!("🚫 unowned: {}", truncate(detail, 40));
    }

    let has_dirty = row.modified > 0 || row.staged > 0;

    // 3. dirty repo — show time since last commit.
    if has_dirty {
        return format!(
            "⏳ dirty {}",
            last_when_mins
                .map(|m| shorten_mins(m))
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
        let state = remote_tracking_ref_exists(repo, &upstream)
            .then_some(PublishState::Ok)
            .unwrap_or(PublishState::Gone);
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
                let state = remote_tracking_ref_exists(repo, &label)
                    .then_some(PublishState::Ok)
                    .unwrap_or(PublishState::Gone);
                (label, state)
            } else {
                ("-".to_string(), PublishState::Missing)
            }
        }
        _ => ("-".to_string(), PublishState::Missing),
    }
}

fn publish_cell_label(upstream: &str, state: PublishState) -> String {
    match state {
        PublishState::Missing => "⚠️ none".to_string(),
        PublishState::Gone => format!("⚠️ {upstream} (gone)"),
        PublishState::Ok => upstream.to_string(),
    }
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
fn format_push_to_remotes_cell(
    push_to_remotes: &[String],
    excluded_remotes: &[String],
) -> comfy_table::Cell {
    use comfy_table::Cell;
    if push_to_remotes.is_empty() && excluded_remotes.is_empty() {
        return Cell::new("-").fg(comfy_table::Color::DarkGrey);
    }
    let main = push_to_remotes.join(",");
    if excluded_remotes.is_empty() {
        Cell::new(main).fg(comfy_table::Color::Green)
    } else {
        // Active remotes in green, excluded annotation in dim yellow
        // so the operator can see at a glance that the repo has been
        // deliberately limited to a subset of the default set.
        let excl = excluded_remotes.join(",");
        Cell::new(format!("{main} [excl:{excl}]"))
    }
}

/// Measure the size of `<repo>/.git` in bytes using `du -sb`. Returns
/// `None` if the measurement fails or exceeds the 2-second timeout.
/// `du -sb` is fast even on large .git dirs (40ms for 20 GiB) so the
/// timeout is just a safety net for slow network filesystems.
fn measure_git_size_bytes(repo: &std::path::Path) -> Option<u64> {
    let git_dir = repo.join(".git");
    if !git_dir.exists() {
        return None;
    }
    // Use `du -sb` (POSIX) to get total size in bytes. Fall back to
    // `du -s --block-size=1` if `du` is busybox without `-b`.
    let output = std::process::Command::new("du")
        .arg("-sb")
        .arg(&git_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // Output is "<bytes>\t<path>\n". Parse the first whitespace-separated
    // token. Use a simple split to avoid pulling in a parser.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let bytes_str = stdout.split_whitespace().next()?;
    bytes_str.parse::<u64>().ok()
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
    let s = s.trim();
    if let Some(rest) = s.strip_suffix(" minutes ago") {
        return rest.parse().ok();
    }
    if let Some(rest) = s.strip_suffix(" minute ago") {
        return rest.parse().ok();
    }
    if let Some(rest) = s.strip_suffix(" hours ago") {
        return rest.parse::<u64>().ok().map(|h| h * 60);
    }
    if let Some(rest) = s.strip_suffix(" hour ago") {
        return rest.parse::<u64>().ok().map(|h| h * 60);
    }
    if let Some(rest) = s.strip_suffix(" days ago") {
        return rest.parse::<u64>().ok().map(|d| d * 60 * 24);
    }
    if let Some(rest) = s.strip_suffix(" day ago") {
        return rest.parse::<u64>().ok().map(|d| d * 60 * 24);
    }
    if let Some(rest) = s.strip_suffix(" seconds ago") {
        return rest.parse::<u64>().ok().map(|s| s / 60);
    }
    None
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
    /// Size of the repo's `.git` directory in bytes (i.e. the data that
    /// would be pushed to remotes). Measured with `du -sb` at report
    /// time. `None` if the measurement failed or timed out. Useful for
    /// spotting size-blocked repos like `dracon-platform` (20 GiB) and
    /// for general capacity planning.
    git_size_bytes: Option<u64>,
    /// Per-forge token health summary. Shows whether each forge's token
    /// file is present on disk, so the operator can spot auth-side
    /// issues BEFORE they cause push failures. Always present (not
    /// Option) so the renderer doesn't have to handle None.
    token_health: TokenHealthSummary,
    concern: bool,
    warn: bool,
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
}

#[derive(Debug, Serialize)]
pub(crate) struct RepoReportJson {
    policy: String,
    filter: String,
    repos: usize,
    ok: usize,
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
/// + action label + result) from the incident ledger. Used by the report
/// to show the user that the daemon IS actively working through dirty
/// repos — the `last_when`/`last_push` columns show the last *commit*
/// and *push* times, but those reset to the moment of the daemon's own
/// commit, so they don't distinguish "user is editing" from "daemon is
/// handling dirty work". The `DAEMON` column closes that gap.
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
/// `has_any_remote` follows the same logic as [`repo_is_concern`]: a
/// repo with at least one configured remote is not concerning for
/// "no origin" alone, and a repo with any configured remote is not
/// concerning for "no upstream" alone — the daemon's multi-mirror
/// push path uses explicit `git push <remote> HEAD:refs/heads/<branch>`
/// refspecs, so it does not require `branch.<name>.remote` to be set
/// in the local config. The hint text and the `NO_UPSTREAM` flag are
/// still emitted (so the operator can see the gap), but the row is
/// no longer classified as a CONCERN that auto-repair will try to
/// remediate via `git push -u origin HEAD` against a non-existent
/// `origin`. See `docs/design/no-origin-concern-ssh-2026-06-20.md`.
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
    false
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

/// Borrowed `as_str` for a `&StateCause`. Required because we
/// dropped `Copy` from `StateCause` (it now carries String
/// fields in the Unowned variant). Most call sites are
/// refactored to use this.
pub(crate) fn state_cause_as_str(cause: &StateCause) -> &'static str {
    match cause {
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
        StateCause::Unowned { .. } => "unowned",
    }
}

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
    if inputs.push_status == "FAIL" || inputs.push_status == "STUCK" {
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
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let shortened: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", shortened)
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

/// Count tracked modified files in `repo` that are NOT covered by
pub(crate) async fn run_repos_report(
    policy_path: &Path,
    filter: RepoFilter,
    json: bool,
    sort: &str,
    filter_name: Option<&str>,
    full_path: bool,
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
    let mut rows: Vec<RepoReportRow> = Vec::new();
    let mut init_or_status_failures = 0usize;

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

    for repo in repos {
        let svc = match GitService::new(&repo) {
            Ok(svc) => svc,
            Err(e) => {
                init_or_status_failures += 1;
                emit_repo_failure(json, "init_failed", &repo, &e);
                continue;
            }
        };

        let status = match svc.get_status().await {
            Ok(status) => status,
            Err(e) => {
                init_or_status_failures += 1;
                emit_repo_failure(json, "status_failed", &repo, &e);
                continue;
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
        let effective_status = status.clone();

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
        let real_is_dirty = status.modified_files > 0 || status.staged_files > 0;
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
        let warn = !concern && real_is_dirty;

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
            Some(crate::ownership::detect_ownership(
                &repo,
                &trusted_for_ownership,
                repo_override_for_ownership.owned,
            ))
        } else {
            None
        };

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

        let hint = if let Some(h) = unowned_hint {
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

        // Calculate push status from flags
        let (push_status, push_error) = if push_budget_exhausted {
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
            Some((h, a, w, u, m)) => (truncate(&h, 12), a, w, u, truncate(&m, 72)),
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
        let thresholds = StateCauseThresholds::from_policy(&policy, &repo_override);
        let last_commit_minutes = parse_relative_minutes(&last_when);
        let last_push_minutes = parse_relative_minutes(&last_push);
        let inputs = StateCauseInputs {
            flags: &flags,
            push_status: &push_status,
            modified: effective_status.modified_files,
            staged: effective_status.staged_files,
            untracked: effective_status.untracked_files,
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
        rows.push(RepoReportRow {
            repo: repo.display().to_string(),
            state_flags: flags,
            branch: effective_status.branch.clone(),
            upstream: upstream_label,
            publish_state,
            modified: effective_status.modified_files,
            staged: effective_status.staged_files,
            untracked: effective_status.untracked_files,
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
            // to the global `policy.remotes` — the SAME logic the daemon
            // runs in `push_mirror_remotes` at sync time. What you see
            // in the table is what the daemon will do.
            push_to_remotes: {
                let filtered = crate::git::multi_remote::filter_remotes_by_exclude(
                    &policy.remotes,
                    &repo_override.exclude_remotes,
                );
                filtered.iter().map(|r| r.name.clone()).collect()
            },
            excluded_remotes: repo_override.exclude_remotes.clone(),
            // Measure `.git` size in bytes. `du -sb` is fast (~40ms for
            // a 20 GiB .git) so we can call it inline. If it fails or
            // times out, we record `None` and the renderer shows a
            // dash. 2-second cap to keep the report snappy even on
            // network filesystems.
            git_size_bytes: measure_git_size_bytes(&repo),
            // Probe each forge's token file. We check both the modern
            // `~/.dracon/utilities/sync/secrets/` dir and the legacy
            // `~/.dracon/secrets/pat/` dir (the daemon's `load_secret`
            // falls back to the legacy dir, so both matter). The probe
            // is just `Path::exists()` on each — no file contents read.
            token_health: probe_token_health(),
            concern,
            warn,
            hint,
            state_cause: state_cause.clone(),
            state_cause_label: state_cause_label_string(&state_cause),
            daemon_last_action_unix,
            daemon_last_action,
            daemon_last_result,
            daemon_last_action_when,
        });
    }

    match sort {
        "name" => rows.sort_by(|a, b| a.repo.cmp(&b.repo)),
        "modified" => rows.sort_by_key(|b| std::cmp::Reverse(b.modified)),
        "ahead" => rows.sort_by_key(|b| std::cmp::Reverse(b.ahead)),
        "behind" => rows.sort_by_key(|b| std::cmp::Reverse(b.behind)),
        _ => rows.sort_by_key(|a| std::cmp::Reverse(a.last_unix)),
    }

    let concern_count_all = rows.iter().filter(|r| r.concern).count();
    let warn_count_all = rows.iter().filter(|r| r.warn).count();
    let ok_count_all = rows
        .len()
        .saturating_sub(concern_count_all + warn_count_all);
    match filter {
        RepoFilter::All => {}
        RepoFilter::Concern => rows.retain(|r| r.concern),
        RepoFilter::Warn => rows.retain(|r| r.warn),
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

    let concern_count = rows.iter().filter(|r| r.concern).count();
    let warn_count = rows.iter().filter(|r| r.warn).count();
    let ok_count = rows.len().saturating_sub(concern_count + warn_count);
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
            warn: warn_count,
            concern: concern_count,
            failures: init_or_status_failures,
            rows,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("📜 {}", policy_path.display());
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
    // ---- Summary one-liner (color-aware, no raw ANSI when piped) ----
    let ok_str = ansi("32", &format!("✅ OK {ok_count}"));
    let warn_str = ansi("33", &format!("⚠️  WARN {warn_count}"));
    let concern_str = ansi("31", &format!("❌ CONCERN {concern_count}"));
    let filter_note = match filter {
        RepoFilter::All => String::new(),
        RepoFilter::Concern | RepoFilter::Warn => format!(
            "  (all: OK {} WARN {} CONCERN {})",
            ok_count_all, warn_count_all, concern_count_all
        ),
    };
    println!(
        "📦 {total} repos  {ok_str}  {warn_str}  {concern_str}  ⛔ init/status failed: {init_or_status_failures}{filter_note}",
        total = rows.len(),
    );
    println!();

    // ---- Legend line (one-liner mapping column codes to their meaning) ----
    println!(
        "ℹ️  Legend: MOD = modified tracked · STG = staged · UT = untracked · 🔗 = VS Code publish upstream — green when healthy (e.g. `github/main`), yellow ⚠️ none when no upstream is configured, yellow ⚠️ <remote/branch> (gone) when the upstream is configured but its remote-tracking ref does not exist locally · ↑ = ahead of upstream · ↓ = behind upstream · PUSH = push status · 📊 1h/6h/24h = commits in last 1h/6h/24h · STATE = derived cause (working=daemon just synced/committing/pushing/synced=clean & in sync/stalled/dirty/untracked-only/intentional/failed/idle/cold/healthy) · ACTIVITY = real activity indicator (now=daemon processing this repo · pushing Xm (N ahead)=push in progress, N unpushed commits · dirty Xm=dirty repo, last commit X minutes ago · synced/idle/cold=clean & waiting) · DAEMON = daemon's last recorded action (e.g. '23s sync_triage') so you can tell the daemon is working through dirty rows vs. you're editing right now"
    );
    println!();

    use comfy_table::{
        presets::UTF8_FULL_CONDENSED, Attribute, Cell, Color, ContentArrangement, Table,
    };

    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    // Single-line header cells: icon + space + label (no newline)
    let mk_h = |icon: &str, label: &str| -> Cell {
        Cell::new(format!("{icon} {label}")).add_attribute(Attribute::Bold)
    };
    // Set fixed column widths so the table doesn't wrap rows on 100+ col terminals
    // and truncates gracefully on narrower ones.
    table.set_header(vec![
        Cell::new("#"),
        mk_h("🏷", "STATUS"),
        mk_h("📦", "REPO"),
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

    for (idx, row) in rows.iter().enumerate() {
        let (status_text, status_color) = if row.concern {
            ("❌ CONCERN".to_string(), Color::Red)
        } else if row.warn {
            ("⚠️  WARN".to_string(), Color::Yellow)
        } else {
            ("✅ OK".to_string(), Color::Green)
        };

        let repo_name = if full_path {
            row.repo.clone()
        } else {
            std::path::Path::new(&row.repo)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| row.repo.clone())
        };

        let push_color = match row.push_status.as_str() {
            "OK" | "INTENTIONAL" => Color::Green,
            "PENDING" => Color::Yellow,
            "FAIL" | "STUCK" => Color::Red,
            _ => Color::White,
        };

        let state_color = match row.state_cause {
            StateCause::Working | StateCause::Synced => Color::Green,
            StateCause::Committing | StateCause::Pushing | StateCause::Dirty => Color::Yellow,
            StateCause::Stalled | StateCause::Failed => Color::Red,
            StateCause::Intentional => Color::Magenta,
            StateCause::Untracked | StateCause::Idle => Color::White,
            StateCause::Cold | StateCause::Healthy => Color::DarkGrey,
            // Unowned: red — the daemon is intentionally not
            // touching this repo, the operator must intervene.
            StateCause::Unowned { .. } => Color::Red,
        };

        // Color-code numeric columns based on severity
        let modified_color = if row.modified > 0 {
            Color::Yellow
        } else {
            Color::White
        };
        let staged_color = if row.staged > 0 {
            Color::Cyan
        } else {
            Color::White
        };
        let ahead_color = if row.ahead > 0 {
            Color::Yellow
        } else {
            Color::White
        };
        let behind_color = if row.behind > 0 {
            Color::Red
        } else {
            Color::White
        };

        // Color branches: main/master in bold, others in cyan
        let branch_color = if row.branch == "main" || row.branch == "master" {
            Color::White
        } else {
            Color::Cyan
        };

        // Compose a one-line commit summary: "<short-hash> <subject>"
        let commit_summary = if row.last_hash == "-" {
            "-".to_string()
        } else {
            format!("{} {}", row.last_hash, row.last_msg)
        };

        table.add_row(vec![
            Cell::new(idx + 1),
            Cell::new(status_text).fg(status_color),
            Cell::new(repo_name),
            Cell::new(&row.branch).fg(branch_color),
            Cell::new(publish_cell_label(&row.upstream, row.publish_state))
                .fg(publish_state_color(row.publish_state)),
            Cell::new(row.modified).fg(modified_color),
            Cell::new(row.staged).fg(staged_color),
            Cell::new(row.untracked),
            Cell::new(row.ahead).fg(ahead_color),
            Cell::new(row.behind).fg(behind_color),
            Cell::new(&row.push_status).fg(push_color),
            format_push_to_remotes_cell(
                &row.push_to_remotes,
                &row.excluded_remotes,
            ),
            Cell::new(commit_summary),
            Cell::new(shorten_when(&row.last_push)),
            Cell::new(activity_label(&row)),
            Cell::new(&row.last_author),
            Cell::new(row.commits_1h),
            Cell::new(row.commits_6h),
            Cell::new(row.commits_24h),
            Cell::new(format!(
                "{} {}",
                row.state_cause.icon(),
                row.state_cause.as_str()
            ))
            .fg(state_color),
            Cell::new(format!(
                "{} {}",
                row.daemon_last_action_when, row.daemon_last_action
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
            Cell::new(&row.hint).fg(if row.concern {
                Color::Red
            } else if row.warn {
                Color::Yellow
            } else {
                Color::Green
            }),
        ]);
    }

    println!("{table}");

    Ok(())
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

struct RepairState {
    attempted_ops: usize,
    succeeded_ops: usize,
    manual_only: usize,
    has_origin: bool,
    has_upstream: bool,
    push_ok: bool,
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
                        Ok(Some(backup_branch)) => {
                            let backup_branch_for_log = backup_branch.clone();
                            if human {
                                println!(
                                    "   ok: rewrite complete (backup branch: {})",
                                    backup_branch
                                );
                            }
                            match push_with_retries(
                                repo,
                                push_timeout_secs,
                                push_retries,
                                "push-after-rewrite",
                            )
                            .await
                            {
                                Ok(()) => {
                                    state.succeeded_ops += 1;
                                    state.push_ok = true;
                                    if human {
                                        println!("   ok: pushed after rewrite");
                                    }
                                    log_incident(
                                        policy_path,
                                        "concern",
                                        repo.display().to_string(),
                                        reason,
                                        "rewrite_then_push",
                                        Some(backup_branch_for_log),
                                        "ok",
                                        Some(format!("paths={:?}", rewrite_paths)),
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
                                            )
                                            .await;
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
                                        Some(backup_branch),
                                        "fail",
                                        Some(e2.to_string()),
                                    );
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
        let still_concern = next.ahead > 0 || next.behind > 0 || !has_origin || !has_upstream;
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
                    "   remaining: ahead={} behind={} origin={} upstream={}",
                    next.ahead, next.behind, has_origin, has_upstream
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
        if !is_concern {
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

#[cfg(test)]
mod tests {
    use super::*;
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
        status.last_commit_hash = None;
        status.last_commit_msg = None;
        status
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
        // commits" and is a WARN, not a CONCERN.
        let status = make_status(false, 5, 0);
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
        let status = make_status(false, 0, 3);
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
    fn test_repo_is_warn_dirty() {
        let status = make_status(false, 0, 0);
        assert!(repo_is_warn(&status, true, true, true));
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
        status.last_commit_hash = None;
        status.last_commit_msg = None;

        assert!(!repo_is_warn(&status, true, true, true));
        assert_eq!(repo_state_flags(&status, true, true, true), vec!["DIRTY"]);
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
            max_stage_batch_files: 100000,
            auto_resolve_unmerged: default_auto_resolve_unmerged(),
            push_debounce_secs: default_push_debounce_secs(),
            untracked_warn_threshold: default_untracked_warn_threshold(),
            system_repo: String::new(),
            pulse_interval_secs: 1,
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
            git_size_bytes: Some(34_476_847),
            token_health: TokenHealthSummary { codeberg_present: true, github_present: true, gitlab_present: true },
            concern: false,
            warn: false,
            hint: "healthy".to_string(),
            state_cause: StateCause::Healthy,
            state_cause_label: "healthy".to_string(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: "none".to_string(),
        };
        assert_eq!(row.repo, "/test/repo");
        assert_eq!(row.branch, "main");
        assert!(!row.concern);
    }

    #[test]
    fn test_publish_cell_label_marks_missing_and_gone() {
        assert_eq!(publish_cell_label("-", PublishState::Missing), "⚠️ none");
        assert_eq!(
            publish_cell_label("github/main", PublishState::Gone),
            "⚠️ github/main (gone)"
        );
        assert_eq!(publish_cell_label("github/main", PublishState::Ok), "github/main");
    }

    #[test]
    fn test_branch_upstream_missing_when_no_config() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success();
        crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("user.email")
            .success();
        crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .status()
            .expect("user.name")
            .success();
        crate::git::git_cmd()
            .args(["config", "core.hooksPath", "/dev/null"])
            .current_dir(&repo)
            .status()
            .expect("hooksPath")
            .success();
        std::fs::write(repo.join("README.md"), "initial").expect("write file");
        crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success();
        crate::git::git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success();
        let (label, state) = branch_upstream(&repo, "main");
        assert_eq!(label, "-");
        assert_eq!(state, PublishState::Missing);
    }

    #[test]
    fn test_branch_upstream_gone_when_remote_tracking_ref_missing() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let repo = tmp.path().join("test-repo");
        crate::git::git_cmd()
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .expect("git init")
            .success();
        crate::git::git_cmd()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .status()
            .expect("user.email")
            .success();
        crate::git::git_cmd()
            .args(["config", "user.name", "Test"])
            .current_dir(&repo)
            .status()
            .expect("user.name")
            .success();
        crate::git::git_cmd()
            .args(["config", "core.hooksPath", "/dev/null"])
            .current_dir(&repo)
            .status()
            .expect("hooksPath")
            .success();
        std::fs::write(repo.join("README.md"), "initial").expect("write file");
        crate::git::git_cmd()
            .args(["add", "README.md"])
            .current_dir(&repo)
            .status()
            .expect("git add")
            .success();
        crate::git::git_cmd()
            .args(["commit", "-m", "initial"])
            .current_dir(&repo)
            .status()
            .expect("git commit")
            .success();
        crate::git::git_cmd()
            .args(["remote", "add", "github", "git@github.com:DraconDev/test-repo.git"])
            .current_dir(&repo)
            .status()
            .expect("git remote add")
            .success();
        crate::git::git_cmd()
            .args(["config", "branch.main.remote", "github"])
            .current_dir(&repo)
            .status()
            .expect("remote config")
            .success();
        crate::git::git_cmd()
            .args(["config", "branch.main.merge", "refs/heads/main"])
            .current_dir(&repo)
            .status()
            .expect("merge config")
            .success();
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
            git_size_bytes: Some(34_476_847),
            token_health: TokenHealthSummary { codeberg_present: true, github_present: true, gitlab_present: true },
            concern: false,
            warn: false,
            hint: "test".to_string(),
            state_cause,
            state_cause_label: label,
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: "none".to_string(),
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
        let _ = crate::daemon::save_in_flight_for_test(
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
        let _ = crate::daemon::save_in_flight_for_test(
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
        let _ = crate::daemon::save_in_flight_for_test(
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
        let _ = crate::daemon::save_in_flight_for_test(
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
            git_size_bytes: Some(20_518_397_949),
            token_health: TokenHealthSummary { codeberg_present: true, github_present: true, gitlab_present: true },
            concern: true,
            warn: false,
            hint: "run repair-concerns --apply (push or rewrite)".to_string(),
            state_cause: StateCause::Failed,
            state_cause_label: "failed".to_string(),
            daemon_last_action_unix: 0,
            daemon_last_action: String::new(),
            daemon_last_result: String::new(),
            daemon_last_action_when: "none".to_string(),
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
        let lines = vec![
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
}
