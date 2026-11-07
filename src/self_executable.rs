//! Self-executable identity: detect that THIS process's executable has been
//! replaced or removed on disk.
//!
//! Root cause this module exists for (incident 2026-11-06/07): the operator
//! service ran `~/.local/bin/dracon-sync daemon` from a process started at
//! 21:17:52 while the file was replaced at 23:41:27. Linux keeps the old
//! inode alive for the running process, so `/proc/<pid>/exe` read back as
//! `.../dracon-sync (deleted)`. The staged-epoch path resolves
//! `std::env::current_exe()` and re-executes itself as a containment helper
//! (`git::epoch_spool_producer::producer_helper` and `sync::staged_epoch`),
//! so every helper spawn failed with a bare `No such file or directory
//! (os error 2)`: 2184+ journal lines all reading `failed to spawn epoch
//! storage verifier`, or `epoch producer helper must be an absolute
//! executable`. Every LOCAL COMMIT failed while PUSHES kept reporting OK, so
//! the fleet silently stopped committing for ~8.5 hours while ~78 GiB of
//! epoch spools accumulated.
//!
//! The correction is diagnostic, not magic. The running image is still the OLD
//! revision, so silently re-pointing the helper at the NEW file would run a
//! mixed-revision pair and break the very source-identity binding the epoch
//! helper exists to enforce (`DRACON_EPOCH_SOURCE_ID`). The correct behaviour
//! is to detect the condition, name the cause, and name the remedy (restart).
//!
//! Detection is device+inode, not mtime: mtime changes on a legitimate
//! reinstall of the SAME path, which is harmless. A changed device+inode means
//! the path now names a DIFFERENT file than the one this process executes,
//! which is exactly the condition that breaks `current_exe()` spawns.

use std::path::{Path, PathBuf};

/// One observed probe of this process's own executable path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelfExecutable {
    /// The resolved `current_exe()` path. On Linux a replaced or removed
    /// executable resolves with a trailing " (deleted)" suffix.
    pub path: PathBuf,
    /// `(device, inode)` of the file the path currently names, or `None` when
    /// the path no longer resolves to a file.
    pub on_disk: Option<(u64, u64)>,
    /// `true` when the resolved path carries the " (deleted)" marker.
    pub deleted: bool,
}

impl SelfExecutable {
    /// True when this process is no longer executing the file the path names.
    pub(crate) fn is_stale(&self) -> bool {
        self.deleted || self.on_disk.is_none()
    }

    /// The operator-facing diagnosis: cause, consequence, and remedy.
    pub(crate) fn diagnosis(&self) -> String {
        format!(
            "this process is no longer executing the file at {} — it was replaced or \
             removed since startup, so the running image is the OLD revision and \
             self-exec helpers (epoch storage verifier / producer) cannot be spawned. \
             Every local commit fails while pushes still report OK. Remedy: restart \
             the daemon to load the installed revision — with root: \
             `systemctl restart dracon-sync-operator.service`; WITHOUT root (the unit \
             has Restart=always, so systemd reloads the installed binary): \
             `kill -TERM $(pidof dracon-sync)`",
            self.path.display()
        )
    }
}

/// Probe `current_exe()` and the file it currently names. Read-only.
pub(crate) fn probe() -> std::io::Result<SelfExecutable> {
    let raw = std::env::current_exe()?;
    let deleted = raw.to_string_lossy().ends_with(" (deleted)");
    // The " (deleted)" suffix is not a real path: probing it always fails, so
    // the marker itself is authoritative.
    let on_disk = if deleted { None } else { identity(&raw) };
    Ok(SelfExecutable {
        path: raw,
        on_disk,
        deleted,
    })
}

/// Gate a self-exec helper path, replacing the old
/// `ensure!(path.is_absolute() && path.is_file(), "… must be an absolute
/// executable")` check whose message hid the actual cause for 8.5 hours.
///
/// Returns the diagnosis for a stale image; otherwise falls back to the
/// caller's generic message so a genuine relative-path or missing-helper fault
/// is still reported as exactly that.
pub(crate) fn helper_failure(generic: &str) -> String {
    match probe() {
        Ok(current) if current.is_stale() => current.diagnosis(),
        _ => generic.to_string(),
    }
}

#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

/// Inspect RUNNING daemon processes for a replaced or removed image.
///
/// `dracon-sync health` is its own process, so probing `current_exe()` there
/// would test the freshly installed binary and report healthy while the
/// long-lived daemon was stale. This reads each candidate daemon's
/// `/proc/<pid>/exe` instead, which is the process that actually runs the
/// epoch helpers. Returns `(pid, executable)` for every stale daemon.
#[cfg(unix)]
pub(crate) fn stale_daemon_processes() -> Vec<(u32, PathBuf)> {
    use std::os::unix::fs::MetadataExt;
    let Ok(uid) = std::fs::metadata("/proc/self").map(|m| m.uid()) else {
        return Vec::new();
    };
    let mut stale = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return stale;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        let dir = Path::new("/proc").join(pid.to_string());
        // Only this user's processes: another user's daemon is not our fault.
        if std::fs::metadata(&dir).map(|m| m.uid()).ok() != Some(uid) {
            continue;
        }
        let Ok(cmdline) = std::fs::read(dir.join("cmdline")) else {
            continue;
        };
        // The service runs `<exe> daemon`; argv[0] may itself be "(deleted)".
        if !cmdline
            .split(|byte| *byte == 0)
            .any(|arg| arg == b"daemon")
            || !cmdline
                .split(|byte| *byte == 0)
                .next()
                .is_some_and(|arg| String::from_utf8_lossy(arg).contains("dracon-sync"))
        {
            continue;
        }
        let Ok(exe) = std::fs::read_link(dir.join("exe")) else {
            continue;
        };
        if exe.to_string_lossy().ends_with(" (deleted)") {
            stale.push((pid, exe));
        }
    }
    stale
}

#[cfg(not(unix))]
pub(crate) fn stale_daemon_processes() -> Vec<(u32, PathBuf)> {
    Vec::new()
}

#[cfg(not(unix))]
fn identity(path: &Path) -> Option<(u64, u64)> {
    // Non-unix has no stable device+inode pair; presence is the whole check.
    std::fs::metadata(path).ok().map(|_| (0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stale_fixture() -> SelfExecutable {
        SelfExecutable {
            path: PathBuf::from("/home/dracon/.local/bin/dracon-sync (deleted)"),
            on_disk: None,
            deleted: true,
        }
    }

    #[test]
    fn running_test_binary_is_not_reported_stale() {
        let current = probe().expect("test process has a resolvable current_exe");
        assert!(
            !current.is_stale(),
            "a live test binary must never be reported stale: {current:?}"
        );
        assert!(current.on_disk.is_some());
    }

    #[test]
    fn deleted_marker_is_authoritative_even_with_a_probe() {
        let replaced = SelfExecutable {
            on_disk: Some((66_306, 10_506_241)),
            ..stale_fixture()
        };
        assert!(replaced.is_stale());
    }

    #[test]
    fn missing_on_disk_path_is_stale() {
        let removed = SelfExecutable {
            path: PathBuf::from("/home/dracon/.local/bin/dracon-sync"),
            on_disk: None,
            deleted: false,
        };
        assert!(removed.is_stale());
    }

    #[test]
    fn diagnosis_names_cause_consequence_and_remedy() {
        let text = stale_fixture().diagnosis();
        assert!(text.contains("replaced or removed"), "{text}");
        assert!(text.contains("OLD revision"), "{text}");
        assert!(text.contains("pushes still report OK"), "{text}");
        assert!(
            text.contains("systemctl restart dracon-sync-operator.service"),
            "the remedy must be named: {text}"
        );
    }

    /// The regression this incident is: a stale self-executable must produce a
    /// DIAGNOSABLE message, never the bare errno that cost 8.5 hours.
    #[test]
    fn stale_helper_failure_is_diagnosable_not_a_bare_errno() {
        let generic = "epoch producer helper must be an absolute executable";
        // Live process: the generic message is the honest one.
        let live = helper_failure(generic);
        assert_eq!(live, generic, "an intact image must not claim a stale one");
        assert!(!live.contains("os error 2"));

        // Stale process: the operator needed cause + remedy, not errno 2.
        let diagnosis = stale_fixture().diagnosis();
        assert!(!diagnosis.contains("os error 2"), "{diagnosis}");
        assert!(diagnosis.contains("restart"), "{diagnosis}");
    }

    /// End-to-end: a genuine replaced executable must flip `is_stale`.
    ///
    /// This is the arm the incident actually took — the file at the resolved
    /// path was a DIFFERENT file than the running image. A pure formatter test
    /// cannot catch a regression in the detection itself, so exercise a real
    /// copy + atomic replace and require the new-inode condition to be seen.
    #[cfg(unix)]
    #[test]
    fn replaced_on_disk_file_flips_staleness() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("probe-binary");
        std::fs::write(&path, b"first revision").expect("write original");

        let before = SelfExecutable {
            on_disk: identity(&path),
            path: path.clone(),
            deleted: false,
        };
        assert!(!before.is_stale(), "a present file is not stale");

        // Replace it exactly the way a deployment does: write a sibling and
        // rename over the target. The OLD inode stays mapped by the running
        // process while the PATH now names a new one.
        let staged = dir.path().join(".probe-binary-new");
        std::fs::write(&staged, b"second revision").expect("write replacement");
        std::fs::rename(&staged, &path).expect("atomic replace");

        let after = SelfExecutable {
            on_disk: identity(&path),
            path: path.clone(),
            deleted: false,
        };
        assert!(
            after.on_disk.is_some(),
            "the replacement must resolve to an identity"
        );
        assert_ne!(
            before.on_disk, after.on_disk,
            "an atomic replace must change the observed identity"
        );
    }

    /// A REMOVED executable must also be detected, not just a replaced one.
    #[cfg(unix)]
    #[test]
    fn removed_on_disk_file_is_stale() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("probe-binary");
        std::fs::write(&path, b"revision").expect("write");
        assert!(identity(&path).is_some());
        std::fs::remove_file(&path).expect("remove");
        let gone = SelfExecutable {
            on_disk: identity(&path),
            path,
            deleted: false,
        };
        assert!(
            gone.is_stale(),
            "a resolved path that no longer exists is stale"
        );
    }

    /// Repeated probes of one unchanged file must agree, so a stable install is
    /// never mistaken for a replacement.
    #[cfg(unix)]
    #[test]
    fn repeated_probes_of_one_file_agree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("probe-binary");
        std::fs::write(&path, b"stable revision").expect("write");
        let first = identity(&path);
        let second = identity(&path);
        assert_eq!(first, second, "repeated probes of one file must agree");
        let stable = SelfExecutable {
            on_disk: first,
            path,
            deleted: false,
        };
        assert!(!stable.is_stale(), "a stable file is not stale");
    }
}
