//! Health evidence for the daemon's resource containment.
//!
//! ADDED 2026-11-13 (OOM restart loop). For three hours the daemon was
//! OOM-killed every ~4 minutes (43 kills, systemd restart counter 26+) while
//! `dracon-sync health` stayed green: each restart produced a fresh, healthy-
//! looking process. Two facts would have made it visible immediately:
//!
//! 1. how often the daemon STARTED recently — a restart loop of any cause;
//! 2. whether the unit's memory limits are coherent. A stale runtime drop-in
//!    (`/run/systemd/system/dracon-sync-operator.service.d/10-resource-headroom.conf`)
//!    set `MemoryHigh=1536M` above the newer `MemoryMax=1400M`, so the soft
//!    reclaim limit could never engage and every overrun was a hard kill.
#[cfg(test)]
#[path = "containment_tests.rs"]
mod tests;

use std::path::PathBuf;

/// The system unit that runs the daemon on the operator host.
pub(crate) const DAEMON_UNIT: &str = "dracon-sync-operator.service";
/// Starts within [`RESTART_WINDOW_SECS`] at which the daemon is in a loop.
/// A deploy restarts it once or twice; four in an hour is not maintenance.
pub(crate) const RESTART_LOOP_STARTS: usize = 4;
pub(crate) const RESTART_WINDOW_SECS: u64 = 3600;
const LEDGER_RETENTION_SECS: u64 = 86_400;
const LEDGER_MAX_ENTRIES: usize = 512;

pub(crate) fn starts_ledger_path() -> PathBuf {
    if let Ok(state_dir) = std::env::var("DRACON_SYNC_STATE_DIR") {
        if !state_dir.is_empty() {
            return PathBuf::from(state_dir).join("dracon-sync-daemon-starts");
        }
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/dracon/dracon-sync-daemon-starts")
}

fn parse_starts(text: &str) -> Vec<u64> {
    text.lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .collect()
}

/// Ledger text after recording a start at `now`, pruned to the retention
/// window and entry cap.
pub(crate) fn ledger_with_start(existing: &str, now: u64) -> String {
    let mut starts: Vec<u64> = parse_starts(existing)
        .into_iter()
        .filter(|t| *t <= now && now - *t <= LEDGER_RETENTION_SECS)
        .collect();
    starts.push(now);
    let skip = starts.len().saturating_sub(LEDGER_MAX_ENTRIES);
    starts[skip..].iter().map(|t| format!("{t}\n")).collect()
}

/// Number of recorded starts within `window_secs` before `now`.
pub(crate) fn starts_within(text: &str, now: u64, window_secs: u64) -> usize {
    parse_starts(text)
        .into_iter()
        .filter(|t| *t <= now && now - *t <= window_secs)
        .count()
}

/// Record one daemon start. Best effort: a failure is logged and never
/// prevents the daemon from starting.
pub(crate) fn record_daemon_start() {
    let path = starts_ledger_path();
    let now = crate::policy::timestamp_secs();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let next = ledger_with_start(&existing, now);
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, next)?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(error) = result {
        eprintln!("⚠️ could not record daemon start in {}: {error}", path.display());
    }
    let recent = starts_within(&existing, now, RESTART_WINDOW_SECS) + 1;
    if recent >= RESTART_LOOP_STARTS {
        eprintln!(
            "🚨 dracon-sync daemon started {recent} times in the last hour — a crash/OOM restart loop; see `journalctl -u {DAEMON_UNIT} | grep -E 'oom-kill|Main process exited'`"
        );
    }
}

fn parse_limit(value: &str) -> Option<u64> {
    match value.trim() {
        "" | "infinity" => None,
        v => v.parse().ok(),
    }
}

/// A finite `MemoryHigh` at or above a finite `MemoryMax` means the soft
/// limit can never throttle: the cgroup goes straight from unconstrained to
/// OOM-killed.
pub(crate) fn memory_limit_finding(high: &str, max: &str, drop_ins: &str) -> Option<String> {
    let (high, max) = (parse_limit(high)?, parse_limit(max)?);
    if high < max {
        return None;
    }
    let mib = |b: u64| b / (1024 * 1024);
    let mut finding = format!(
        "{DAEMON_UNIT}: MemoryHigh {}M >= MemoryMax {}M, so soft reclaim never engages and overruns are OOM kills",
        mib(high),
        mib(max)
    );
    let runtime: Vec<&str> = drop_ins
        .split_whitespace()
        .filter(|p| p.starts_with("/run/"))
        .collect();
    if !runtime.is_empty() {
        finding.push_str(&format!(
            "; runtime drop-in(s) override the host config: {}",
            runtime.join(" ")
        ));
    }
    Some(finding)
}

/// `systemctl show` key=value output for the daemon unit, or None when the
/// unit is not installed on this host.
pub(crate) fn daemon_unit_properties() -> Option<std::collections::BTreeMap<String, String>> {
    let output = std::process::Command::new("systemctl")
        .args([
            "show",
            DAEMON_UNIT,
            "-p",
            "LoadState",
            "-p",
            "MemoryHigh",
            "-p",
            "MemoryMax",
            "-p",
            "DropInPaths",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let props: std::collections::BTreeMap<String, String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    (props.get("LoadState").map(String::as_str) == Some("loaded")).then_some(props)
}
