//! The detail selector must not override an explicitly requested JSON format.
use std::{path::Path, process::Command, time::Duration};

fn git(repo: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn named_repository_json_is_a_real_envelope_not_vertical_text() {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path().join("fixture-repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--template=", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "DraconDev"]);
    git(&repo, &["config", "user.email", "dracsharp@gmail.com"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("file.txt"), "baseline\n").unwrap();
    git(&repo, &["add", "--", "file.txt"]);
    git(&repo, &["commit", "-q", "-m", "fixture baseline"]);
    std::fs::write(repo.join("file.txt"), "visible local work\n").unwrap();

    let policy = fixture.path().join("policy.toml");
    let mut settings = toml::map::Map::new();
    settings.insert(
        "watch_roots".into(),
        toml::Value::Array(vec![toml::Value::String(repo.to_str().unwrap().into())]),
    );
    settings.insert("auto_github_private".into(), toml::Value::Boolean(false));
    std::fs::write(&policy, toml::to_string(&settings).unwrap()).unwrap();

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_dracon-sync"));
    command
        .args(["repos", "fixture-repo", "--json"])
        .env("HOME", fixture.path())
        .env("XDG_CONFIG_HOME", fixture.path().join("config"))
        .env("XDG_STATE_HOME", fixture.path().join("state"))
        .env("DRACON_SYNC_POLICY", &policy)
        .env("DRACON_SYNC_STATE_DIR", fixture.path().join("sync-state"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("DRACON_SYNC_GIT_BIN")
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .expect("one-file report must be bounded")
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "JSON detail must not emit human text: {error}: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
    assert_eq!(payload["repos"], 1);
    assert_eq!(payload["failures"], 0);
    let rows = payload["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["repo"], repo.to_str().unwrap());
    assert_eq!(rows[0]["branch"], "main");
    assert!(
        rows[0]["modified"].as_u64().unwrap() >= 1,
        "real local work must remain visible"
    );
    assert_eq!(
        std::fs::read(repo.join("file.txt")).unwrap(),
        b"visible local work\n"
    );
}
