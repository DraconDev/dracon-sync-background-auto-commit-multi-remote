//! Daemon push containment: every enrolled daemon `git push` (and therefore
//! its pre-push hook, `pack-objects` and SSH children) runs in its OWN
//! transient systemd user service under [`PUSH_SLICE`], not in the daemon's
//! cgroup.
//!
//! ADDED 2026-11-13 (OOM restart loop). The 2026-11-11 host hardening capped
//! the daemon's system unit at `MemoryMax=1400M`. browser-extensions-shared's
//! pre-push gate runs up to 8 workspace typechecks in parallel; inside the
//! daemon cgroup that crossed the cap, the kernel killed the WHOLE daemon,
//! systemd restarted it 30 s later, and the same push started again: 43
//! OOM kills in 3 h, 186 commits unpublished, and every other repository's
//! in-flight work aborted every ~4 minutes. A hook is repository-supplied
//! code; its memory must not be the daemon's memory.
//!
//! Contract:
//! * The daemon sets NO memory limit here. The budget is the slice's, owned by
//!   the host configuration (NixOS `systemd.user.slices.dracon-sync-push`).
//!   Without that configuration the slice exists with no limits, which is the
//!   pre-2026-11-11 behavior minus the shared fate.
//! * A unit that exhausts its budget is killed alone; `systemd-run` reports
//!   `Finished with result: oom-kill`, which the push classifier treats as a
//!   local hook failure (fail fast, no retry storm).
//! * Killing the `systemd-run` client does NOT stop its unit (measured on
//!   systemd 258), so timeout and cancellation stop the unit explicitly.
//!   `RuntimeMaxSec` is the backstop for a daemon that dies mid-push, and
//!   [`stop_orphaned_push_units`] reaps survivors at daemon start.
//! * The unit runs under the SAME filesystem sandbox as the daemon that
//!   launched it: the daemon's own unit's `ProtectSystem`, `ProtectHome`,
//!   `ReadWritePaths`, `PrivateTmp`, … are read back from the service manager
//!   and applied to every push unit ([`mirrored_sandbox`]). Leaving the
//!   daemon's cgroup must not mean leaving its sandbox, and reading the live
//!   values means host config and push units cannot drift apart.
//! * When no user manager is reachable (CI, containers) or
//!   `DRACON_SYNC_PUSH_ISOLATION=0`, the push runs directly as before.
#[cfg(test)]
#[path = "push_unit_tests.rs"]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Host-configured budget for all daemon pushes together.
pub(crate) const PUSH_SLICE: &str = "dracon-sync-push.slice";
#[cfg(not(test))]
const UNIT_PREFIX: &str = "dracon-push-";
/// Tests share the operator's user manager; a distinct prefix keeps a live
/// daemon's startup sweep from stopping a test's push and vice versa.
#[cfg(test)]
const UNIT_PREFIX: &str = "dracon-push-test-";
const TEST_UNIT_PREFIX: &str = "dracon-push-test-";
const DISABLE_ENV: &str = "DRACON_SYNC_PUSH_ISOLATION";

/// Service-manager bookkeeping of the DAEMON's own unit. Forwarding these
/// would make the push claim the daemon's notify socket / journal stream.
const MANAGER_ENV: &[&str] = &[
    "INVOCATION_ID",
    "JOURNAL_STREAM",
    "NOTIFY_SOCKET",
    "LISTEN_PID",
    "LISTEN_FDS",
    "LISTEN_FDNAMES",
    "WATCHDOG_PID",
    "WATCHDOG_USEC",
    "SYSTEMD_EXEC_PID",
    "MANAGERPID",
    "MAINPID",
    "MEMORY_PRESSURE_WATCH",
    "MEMORY_PRESSURE_WRITE",
    "CREDENTIALS_DIRECTORY",
    "TRIGGER_UNIT",
    "TRIGGER_PATH",
    "TRIGGER_TIMER_REALTIME_USEC",
    "TRIGGER_TIMER_MONOTONIC_USEC",
];

#[derive(Clone, Debug)]
struct Manager {
    uid: u32,
    runner: PathBuf,
    control: PathBuf,
}

fn locate(name: &str, path: Option<&OsString>) -> Option<PathBuf> {
    let path = path.cloned().or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|found| found.canonicalize().ok())
}

pub(crate) fn disabled_by_env(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "off" | "false" | "no" | "direct")
    )
}

fn manager() -> Option<Manager> {
    manager_reporting(true)
}

/// Whether daemon pushes would be isolated in this environment (for health).
/// Callers outside the daemon must not trigger the daemon's one-time log.
pub(crate) fn available() -> bool {
    manager_reporting(false).is_some()
}

fn manager_reporting(log: bool) -> Option<Manager> {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    let report = |why: &str| {
        if log && !REPORTED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "⚠️ push isolation unavailable ({why}); daemon pushes and their hooks run in the daemon's own cgroup"
            );
        }
    };
    if disabled_by_env(std::env::var(DISABLE_ENV).ok().as_deref()) {
        report(&format!("{DISABLE_ENV} disables it"));
        return None;
    }
    #[cfg(target_os = "linux")]
    let uid = unsafe { libc::getuid() };
    #[cfg(not(target_os = "linux"))]
    {
        report("not Linux");
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        if !Path::new(&format!("/run/user/{uid}/bus")).exists() {
            report("no systemd user manager bus");
            return None;
        }
        let (Some(runner), Some(control)) =
            (locate("systemd-run", None), locate("systemctl", None))
        else {
            report("systemd-run/systemctl not on PATH");
            return None;
        };
        Some(Manager {
            uid,
            runner,
            control,
        })
    }
}

impl Manager {
    fn bus_env(&self) -> [(&'static str, String); 2] {
        [
            ("XDG_RUNTIME_DIR", format!("/run/user/{}", self.uid)),
            (
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path=/run/user/{}/bus", self.uid),
            ),
        ]
    }
    fn control(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.control);
        command
            .envs(self.bus_env())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        command
    }
}

fn unit_name() -> Option<String> {
    use std::io::Read;
    let mut random = [0u8; 12];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .ok()?;
    let hex: String = random.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!("{UNIT_PREFIX}{hex}.service"))
}

/// The environment the direct child WOULD have had: the daemon's own
/// environment with the command's explicit set/remove edits applied, minus
/// the daemon unit's service-manager bookkeeping.
fn effective_environment(command: &std::process::Command) -> BTreeMap<OsString, OsString> {
    let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                env.insert(key.to_owned(), value.to_owned());
            }
            None => {
                env.remove(key);
            }
        }
    }
    for key in MANAGER_ENV {
        env.remove(std::ffi::OsStr::new(key));
    }
    env
}

/// Build the `systemd-run` argv for one push. Values never appear on the
/// command line (`--setenv=KEY` copies from systemd-run's own environment),
/// so credential-bearing variables stay out of `/proc/*/cmdline`.
/// Exec-sandbox settings copied from the daemon's own unit onto each push unit.
const MIRRORED_SANDBOX: &[&str] = &[
    "ProtectSystem",
    "ProtectHome",
    "ReadWritePaths",
    "ReadOnlyPaths",
    "InaccessiblePaths",
    "PrivateTmp",
    "PrivateDevices",
    "NoNewPrivileges",
    "LockPersonality",
    "RestrictRealtime",
    "RestrictSUIDSGID",
];
const PATH_LIST_SANDBOX: &[&str] = &["ReadWritePaths", "ReadOnlyPaths", "InaccessiblePaths"];

/// Turn `systemctl show -p <MIRRORED_SANDBOX…>` output into `Key=Value`
/// properties, keeping only settings that actually restrict something.
/// Path entries gain systemd's `-` (ignore-if-missing) prefix: the unit must
/// not fail to start because a listed path is absent, and an absent path has
/// nothing to protect.
pub(crate) fn mirror_properties(show: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in show.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if !MIRRORED_SANDBOX.contains(&key) || value.is_empty() || value == "no" {
            continue;
        }
        if PATH_LIST_SANDBOX.contains(&key) {
            let mut seen = std::collections::BTreeSet::new();
            let paths: Vec<String> = value
                .split_whitespace()
                .filter(|p| seen.insert(p.trim_start_matches(['-', '+']).to_owned()))
                .map(|p| {
                    if p.starts_with(['-', '+']) {
                        p.to_owned()
                    } else {
                        format!("-{p}")
                    }
                })
                .collect();
            if !paths.is_empty() {
                out.push(format!("{key}={}", paths.join(" ")));
            }
        } else {
            out.push(format!("{key}={value}"));
        }
    }
    out
}

/// The daemon's own unit from `/proc/self/cgroup`: `(user_manager, name)`.
pub(crate) fn own_unit(cgroup: &str) -> Option<(bool, String)> {
    let path = cgroup.lines().find_map(|l| l.strip_prefix("0::"))?;
    let name = path.rsplit('/').next()?;
    if !name.ends_with(".service") {
        // A scope (terminal, test harness) carries no exec sandbox to mirror.
        return None;
    }
    Some((path.contains("/user@"), name.to_owned()))
}

/// The daemon's sandbox, read once per process. Empty when the daemon is not
/// a service (CLI, tests) or the manager cannot be asked; the latter is logged
/// because the push would then run less confined than its daemon.
fn mirrored_sandbox() -> &'static [String] {
    static SANDBOX: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    SANDBOX.get_or_init(|| {
        let Some((user, unit)) = std::fs::read_to_string("/proc/self/cgroup")
            .ok()
            .as_deref()
            .and_then(own_unit)
        else {
            return Vec::new();
        };
        let mut command = std::process::Command::new("systemctl");
        if user {
            command.arg("--user");
        }
        command.args(["show", &unit]);
        for key in MIRRORED_SANDBOX {
            command.args(["-p", key]);
        }
        match command
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(output) if output.status.success() => {
                mirror_properties(&String::from_utf8_lossy(&output.stdout))
            }
            _ => {
                eprintln!(
                    "⚠️ push isolation: could not read {unit}'s sandbox; push units will run WITHOUT it"
                );
                Vec::new()
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn runner_args(
    unit: &str,
    repo: &Path,
    program: &Path,
    args: &[OsString],
    env_keys: &[String],
    runtime_max_secs: u64,
    stop_grace: Duration,
    sandbox: &[String],
) -> Vec<OsString> {
    let mut out: Vec<OsString> = [
        "--user",
        "--wait",
        "--pipe",
        "--collect",
        "--service-type=exec",
        "--expand-environment=no",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    out.push(format!("--unit={unit}").into());
    out.push(format!("--slice={PUSH_SLICE}").into());
    for property in [
        "KillMode=control-group".to_string(),
        "SendSIGKILL=yes".to_string(),
        format!("TimeoutStopSec={}s", stop_grace.as_secs().max(1)),
        format!("RuntimeMaxSec={}s", runtime_max_secs.max(1)),
        // Same low priority the direct path set through pre_exec.
        "Nice=19".to_string(),
    ] {
        out.push(format!("--property={property}").into());
    }
    for property in sandbox {
        out.push(format!("--property={property}").into());
    }
    let mut workdir = OsString::from("--working-directory=");
    workdir.push(repo.as_os_str());
    out.push(workdir);
    for key in env_keys {
        out.push(format!("--setenv={key}").into());
    }
    out.push("--".into());
    out.push(program.as_os_str().to_owned());
    out.extend(args.iter().cloned());
    out
}

/// Wrap an already-configured Git push command in a transient user service.
/// Returns `None` when isolation is unavailable; the caller then spawns the
/// original command directly.
pub(super) fn wrap(
    git: &std::process::Command,
    repo: &Path,
    runtime_max_secs: u64,
) -> Option<(tokio::process::Command, String)> {
    let manager = manager()?;
    let env = effective_environment(git);
    let program = PathBuf::from(git.get_program());
    let program = if program.is_absolute() {
        program
    } else {
        locate(program.to_str()?, env.get(std::ffi::OsStr::new("PATH")))?
    };
    let repo = std::path::absolute(repo).ok()?;
    let unit = unit_name()?;
    // Keys that cannot be passed losslessly are refused rather than dropped:
    // a silently missing variable could change which credentials Git uses.
    let mut keys = Vec::with_capacity(env.len());
    for key in env.keys() {
        let key = key.to_str()?;
        if key.is_empty() || key.contains('=') {
            return None;
        }
        keys.push(key.to_owned());
    }
    let args: Vec<OsString> = git.get_args().map(|a| a.to_owned()).collect();
    let mut command = tokio::process::Command::new(&manager.runner);
    command.env_clear().envs(&env).envs(manager.bus_env());
    command.args(runner_args(
        &unit,
        &repo,
        &program,
        &args,
        &keys,
        runtime_max_secs,
        super::ops::SIGTERM_GRACE,
        mirrored_sandbox(),
    ));
    command.stdin(std::process::Stdio::null());
    Some((command, unit))
}

fn units() -> &'static Mutex<HashMap<u32, String>> {
    static UNITS: std::sync::OnceLock<Mutex<HashMap<u32, String>>> = std::sync::OnceLock::new();
    UNITS.get_or_init(Default::default)
}

/// Bind a spawned `systemd-run` client PID to its unit.
pub(super) fn register(pid: u32, unit: String) {
    units()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(pid, unit);
}

/// The client was reaped; its PID may be reused, so drop the binding.
pub(super) fn forget(pid: u32) {
    units()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&pid);
}

fn take(pid: u32) -> Option<String> {
    units()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&pid)
}

/// Graceful stop for a timed-out push: systemd sends TERM to the whole unit,
/// waits `TimeoutStopSec` (= [`super::ops::SIGTERM_GRACE`]) and then KILLs.
/// Returns true when `pid` was an isolated push client.
pub(super) async fn stop(pid: u32) -> bool {
    let Some(unit) = take(pid) else {
        return false;
    };
    let Some(manager) = manager() else {
        return true;
    };
    let mut command = tokio::process::Command::from(manager.control());
    command
        .args(["--user", "stop", "--", &unit])
        .kill_on_drop(true);
    let budget = super::ops::SIGTERM_GRACE + Duration::from_secs(15);
    match tokio::time::timeout(budget, command.output()).await {
        Ok(Ok(output)) if output.status.success() => {}
        Ok(Ok(output)) => eprintln!(
            "⚠️ push unit {unit}: stop failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Ok(Err(error)) => eprintln!("⚠️ push unit {unit}: stop failed to run: {error}"),
        Err(_) => eprintln!("⚠️ push unit {unit}: stop did not finish in {budget:?}"),
    }
    true
}

/// Immediate kill from a `Drop` (cancellation). `Drop` cannot await, so the
/// manager calls run on a detached thread; the unit's processes receive
/// SIGKILL within the manager round-trip, matching the direct path's
/// TERM-then-KILL backstop.
pub(super) fn kill_now(pid: u32) {
    let Some(unit) = take(pid) else {
        return;
    };
    let Some(manager) = manager() else {
        return;
    };
    std::thread::spawn(move || {
        let _ = manager
            .control()
            .args(["--user", "kill", "--signal=SIGKILL", "--", &unit])
            .output();
        let _ = manager
            .control()
            .args(["--user", "stop", "--", &unit])
            .output();
    });
}

fn is_daemon_push_unit(name: &str) -> bool {
    let daemon_owned = if UNIT_PREFIX == TEST_UNIT_PREFIX {
        name.starts_with(TEST_UNIT_PREFIX)
    } else {
        name.starts_with(UNIT_PREFIX) && !name.starts_with(TEST_UNIT_PREFIX)
    };
    daemon_owned && name.ends_with(".service")
}

/// Parse `systemctl list-units --plain --no-legend` output into unit names.
fn listed_units(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| is_daemon_push_unit(name))
        .map(str::to_owned)
        .collect()
}

/// A push unit survives its daemon when the daemon dies mid-push (OOM, crash,
/// restart). Its hook may still be running and would race the new daemon's
/// push of the same repository, so the new daemon stops every survivor once
/// at start-up. Returns the number stopped.
pub(crate) async fn stop_orphaned_push_units() -> usize {
    let Some(manager) = manager() else {
        return 0;
    };
    let mut list = tokio::process::Command::from(manager.control());
    list.args([
        "--user",
        "list-units",
        "--all",
        "--plain",
        "--no-legend",
        "--no-pager",
        &format!("{UNIT_PREFIX}*.service"),
    ])
    .stdout(std::process::Stdio::piped())
    .kill_on_drop(true);
    let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(30), list.output()).await else {
        eprintln!("⚠️ push isolation: could not list orphaned push units");
        return 0;
    };
    let orphans = listed_units(&String::from_utf8_lossy(&output.stdout));
    let mut stopped = 0;
    for unit in &orphans {
        let mut stop = tokio::process::Command::from(manager.control());
        stop.args(["--user", "stop", "--", unit]).kill_on_drop(true);
        let budget = super::ops::SIGTERM_GRACE + Duration::from_secs(15);
        if let Ok(Ok(out)) = tokio::time::timeout(budget, stop.output()).await {
            if out.status.success() {
                stopped += 1;
                continue;
            }
        }
        eprintln!("⚠️ push isolation: could not stop orphaned {unit}");
    }
    if stopped > 0 {
        eprintln!(
            "🧹 push isolation: stopped {stopped} orphaned push unit(s) left by a previous daemon"
        );
    }
    stopped
}
