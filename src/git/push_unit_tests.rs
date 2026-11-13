use super::*;

#[test]
fn runner_args_isolate_into_the_push_slice_without_leaking_values() {
    let args = runner_args(
        "dracon-push-test-00.service",
        Path::new("/repo with space"),
        Path::new("/usr/bin/git"),
        &[
            "push".into(),
            "origin".into(),
            "HEAD:refs/heads/main".into(),
        ],
        &["GIT_SSH_COMMAND".into(), "GH_TOKEN".into()],
        3600,
        Duration::from_secs(25),
        &[
            "ProtectSystem=strict".into(),
            "ReadWritePaths=-/home/u/Dev".into(),
        ],
    );
    let text: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    for expected in [
        "--user",
        "--wait",
        "--pipe",
        "--collect",
        "--expand-environment=no",
        "--unit=dracon-push-test-00.service",
        "--slice=dracon-sync-push.slice",
        "--property=KillMode=control-group",
        "--property=TimeoutStopSec=25s",
        "--property=RuntimeMaxSec=3600s",
        "--property=Nice=19",
        "--working-directory=/repo with space",
        "--setenv=GIT_SSH_COMMAND",
        "--setenv=GH_TOKEN",
        "--property=ProtectSystem=strict",
        "--property=ReadWritePaths=-/home/u/Dev",
    ] {
        assert!(
            text.iter().any(|a| a == expected),
            "missing {expected}: {text:?}"
        );
    }
    // The daemon sets no memory limit itself: the budget is the host slice's.
    assert!(!text.iter().any(|a| a.contains("Memory")), "{text:?}");
    // Values never reach argv; only the key names do.
    assert!(!text
        .iter()
        .any(|a| a.starts_with("--setenv=") && a.contains('=') && a.matches('=').count() > 1));
    let sep = text.iter().position(|a| a == "--").unwrap();
    assert_eq!(
        &text[sep + 1..],
        ["/usr/bin/git", "push", "origin", "HEAD:refs/heads/main"]
    );
}

#[test]
fn effective_environment_applies_edits_and_drops_daemon_manager_state() {
    let mut command = std::process::Command::new("git");
    command
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .env("NOTIFY_SOCKET", "/run/systemd/notify")
        .env_remove("HOME");
    let env = effective_environment(&command);
    assert_eq!(
        env.get(std::ffi::OsStr::new("GIT_SSH_COMMAND"))
            .map(|v| v.as_os_str()),
        Some(std::ffi::OsStr::new("ssh -o BatchMode=yes"))
    );
    assert!(!env.contains_key(std::ffi::OsStr::new("HOME")));
    // Forwarding the daemon unit's notify socket would let a hook speak for it.
    assert!(!env.contains_key(std::ffi::OsStr::new("NOTIFY_SOCKET")));
    assert!(!env.contains_key(std::ffi::OsStr::new("INVOCATION_ID")));
}

#[test]
fn orphan_sweep_only_claims_its_own_prefix() {
    let listing = "dracon-push-test-aa.service loaded active running /usr/bin/git push\n\
                   dracon-push-test-bb.service loaded inactive dead x\n\
                   dracon-epoch-cc.service loaded active running x\n\
                   dracon-push-test-dd.scope loaded active running x\n";
    // Test builds own only the test prefix.
    assert_eq!(
        listed_units(listing),
        ["dracon-push-test-aa.service", "dracon-push-test-bb.service"]
    );
}

#[test]
fn isolation_kill_switch_values() {
    for off in ["0", "off", "FALSE", " no ", "direct"] {
        assert!(disabled_by_env(Some(off)), "{off}");
    }
    for on in [None, Some("1"), Some("on"), Some("")] {
        assert!(!disabled_by_env(on), "{on:?}");
    }
}

#[test]
fn unit_names_are_unique_and_prefixed() {
    let a = unit_name().unwrap();
    let b = unit_name().unwrap();
    assert_ne!(a, b);
    assert!(
        a.starts_with(TEST_UNIT_PREFIX) && a.ends_with(".service"),
        "{a}"
    );
}

#[test]
fn sandbox_is_mirrored_from_the_live_daemon_unit() {
    // Verbatim `systemctl show` of dracon-sync-operator.service, 2026-11-13
    // (note the duplicated bridge-fixture path from the merged drop-in).
    let show = "ReadWritePaths=/home/dracon/.dracon /home/dracon/Dev /home/dracon/.local/state/dracon /home/dracon/.ssh /home/dracon/dracon/bridge-fixture /home/dracon/dracon/bridge-fixture\n\
ReadOnlyPaths=\nInaccessiblePaths=\nPrivateTmp=yes\nPrivateDevices=yes\nProtectHome=read-only\n\
ProtectSystem=strict\nNoNewPrivileges=yes\nLockPersonality=yes\nRestrictRealtime=yes\nRestrictSUIDSGID=no\nMemoryMax=1468006400\n";
    assert_eq!(
        mirror_properties(show),
        [
            "ReadWritePaths=-/home/dracon/.dracon -/home/dracon/Dev -/home/dracon/.local/state/dracon -/home/dracon/.ssh -/home/dracon/dracon/bridge-fixture",
            "PrivateTmp=yes",
            "PrivateDevices=yes",
            "ProtectHome=read-only",
            "ProtectSystem=strict",
            "NoNewPrivileges=yes",
            "LockPersonality=yes",
            "RestrictRealtime=yes",
        ]
    );
    // Resource limits are the slice's business, never copied.
    assert!(!mirror_properties(show)
        .iter()
        .any(|p| p.starts_with("Memory")));
}

#[test]
fn own_unit_distinguishes_services_from_scopes() {
    assert_eq!(
        own_unit("0::/system.slice/dracon-sync-operator.service\n"),
        Some((false, "dracon-sync-operator.service".into()))
    );
    assert_eq!(
        own_unit("0::/user.slice/user-1000.slice/user@1000.service/app.slice/dracon-sync.service"),
        Some((true, "dracon-sync.service".into()))
    );
    assert_eq!(
        own_unit("0::/user.slice/user-1000.slice/user@1000.service/app.slice/tmux-spawn-1.scope"),
        None
    );
}
