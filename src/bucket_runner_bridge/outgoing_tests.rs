use super::*;
use std::process::Command;

fn run(repo: &Path, args: &[&str]) -> Vec<u8> {
    let output = crate::test_helpers::test_git_cmd()
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "fixture git failed: {args:?}");
    output.stdout
}
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    run(repo, &["init", "-q", "-b", "main"]);
    run(repo, &["config", "user.name", "DraconDev"]);
    run(repo, &["config", "user.email", "fixture@dracon.local"]);
    run(repo, &["config", "core.hooksPath", "/dev/null"]);
    run(repo, &["commit", "--allow-empty", "-qm", "baseline"]);
    run(repo, &["config", "branch.main.remote", "origin"]);
    run(repo, &["config", "branch.main.merge", "refs/heads/main"]);
    run(
        repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    temp
}
fn policy() -> SyncPolicy {
    SyncPolicy {
        auto_stage_untracked: true,
        max_stage_file_bytes: 1024 * 1024,
        max_stage_batch_files: 1000,
        ..SyncPolicy::default()
    }
}
fn put(repo: &Path, path: &str, bytes: usize) {
    std::fs::write(repo.join(path), vec![b'x'; bytes]).unwrap();
}

#[tokio::test]
async fn accounting_real_git_deduplicates_history_and_selected_worktree() {
    let td = fixture();
    let repo = td.path();
    let p = policy();
    assert_eq!(measure(repo, &p).await.unwrap(), 0);
    put(repo, "a.txt", 700);
    run(repo, &["add", "--", "a.txt"]);
    run(repo, &["commit", "-qm", "pending"]);
    put(repo, "same.txt", 700);
    assert_eq!(measure(repo, &p).await.unwrap(), 700);
    std::fs::remove_file(repo.join("a.txt")).unwrap();
    // A worktree deletion cannot remove a historical blob from outgoing bytes.
    assert_eq!(measure(repo, &p).await.unwrap(), 700);
    put(repo, "different.txt", 300);
    assert_eq!(measure(repo, &p).await.unwrap(), 1000);
}

#[tokio::test]
async fn accounting_keeps_lagging_mirror_objects_and_respects_exclusions() {
    let td = fixture();
    let repo = td.path();
    let mut p = policy();
    run(
        repo,
        &[
            "remote",
            "add",
            "gitlab",
            "https://example.invalid/mirror.git",
        ],
    );
    run(repo, &["update-ref", "refs/remotes/gitlab/main", "HEAD"]);
    p.remotes.push(crate::policy::RemoteConfig {
        name: "gitlab".into(),
        push_url: "https://example.invalid/mirror.git".into(),
        auto_create: false,
        auto_create_account: String::new(),
        auth_type: Default::default(),
        priority: 1,
        api_endpoint: None,
        auto_create_token_var: None,
        repo_name_map: Default::default(),
        force_push_when_behind: false,
    });
    put(repo, "lag.txt", 300);
    run(repo, &["add", "--", "lag.txt"]);
    run(repo, &["commit", "-qm", "lag"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    assert_eq!(measure(repo, &p).await.unwrap(), 300);
    p.untracked_exclude_patterns.push("ignored.txt".into());
    put(repo, "ignored.txt", 999);
    std::fs::write(repo.join(".gitignore"), "ignored-by-git.txt\n").unwrap();
    run(repo, &["add", "--", ".gitignore"]);
    run(repo, &["commit", "-qm", "ignore"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    run(repo, &["update-ref", "refs/remotes/gitlab/main", "HEAD"]);
    put(repo, "ignored-by-git.txt", 999);
    assert_eq!(measure(repo, &p).await.unwrap(), 0);
    // A policy mirror not yet configured locally is still a possible push
    // destination. Bound unknown availability by all reachable history, never
    // silently omit it or assume a zero-byte outgoing payload.
    run(repo, &["remote", "remove", "gitlab"]);
    assert!(measure(repo, &p).await.unwrap() >= 300);
    std::fs::create_dir(repo.join(".dracon")).unwrap();
    std::fs::write(
        repo.join(".dracon/dracon-sync.toml"),
        "exclude_remotes = [\"gitlab\"]\n",
    )
    .unwrap();
    run(repo, &["add", "--", ".dracon/dracon-sync.toml"]);
    run(repo, &["commit", "-qm", "explicit mirror exclusion"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    assert_eq!(measure(repo, &p).await.unwrap(), 0);
}

#[tokio::test]
async fn accounting_potential_codeberg_destination_cannot_be_omitted_by_quota_guess() {
    let td = fixture();
    let repo = td.path();
    let mut p = policy();
    put(repo, "published.txt", 700);
    run(repo, &["add", "--", "published.txt"]);
    run(repo, &["commit", "-qm", "published to primary"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    assert_eq!(measure(repo, &p).await.unwrap(), 0);
    p.remotes.push(crate::policy::RemoteConfig {
        name: "codeberg".into(),
        push_url: "https://codeberg.org/DraconDev/fixture.git".into(),
        auto_create: false,
        auto_create_account: String::new(),
        auth_type: Default::default(),
        priority: 1,
        api_endpoint: None,
        auto_create_token_var: None,
        repo_name_map: Default::default(),
        force_push_when_behind: false,
    });
    // Public visibility may activate this mirror even with auto_create=false.
    // Accounting includes the possibility irrespective of cache changes: all
    // HEAD objects bound its unknown availability without guessing empty.
    assert_eq!(measure(repo, &p).await.unwrap(), 700);
    run(repo, &["config", "--unset", "branch.main.remote"]);
    run(repo, &["update-ref", "-d", "refs/remotes/origin/main"]);
    assert_eq!(measure(repo, &p).await.unwrap(), 700);
}

#[tokio::test]
async fn accounting_actual_git_bytes_drive_inclusive_runner_pressure_boundaries() {
    let td = fixture();
    let repo = td.path();
    let p = policy();
    let module = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/bucket-runner/routing.mjs");
    for (size, expected) in [(799, false), (800, true), (801, true)] {
        put(repo, "candidate.txt", size);
        let measured = measure(repo, &p).await.unwrap();
        assert_eq!(measured, size as u64);
        let code = format!("import {{shouldRouteEligible}} from {}; console.log(shouldRouteEligible({{bytes:100}},{{projectedBytes:{measured},limitBytes:1000}}));", serde_json::to_string(&module.to_string_lossy()).unwrap());
        let output = Command::new("node")
            .args(["--input-type=module", "-e", &code])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            expected.to_string()
        );
    }
}

#[tokio::test]
async fn accounting_missing_tip_and_unreadable_git_object_fail_closed() {
    let td = fixture();
    let repo = td.path();
    let p = policy();
    run(repo, &["update-ref", "-d", "refs/remotes/origin/main"]);
    assert!(measure(repo, &p)
        .await
        .unwrap_err()
        .to_string()
        .contains("missing-publish-tip"));
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    put(repo, "lost.txt", 80);
    run(repo, &["add", "--", "lost.txt"]);
    run(repo, &["commit", "-qm", "lost"]);
    let hash = String::from_utf8(run(repo, &["rev-parse", "HEAD:lost.txt"])).unwrap();
    let hash = hash.trim();
    std::fs::remove_file(repo.join(".git/objects").join(&hash[..2]).join(&hash[2..])).unwrap();
    assert!(measure(repo, &p).await.is_err());
}

#[tokio::test]
async fn accounting_reserves_enforced_output_ceiling_without_executing_filter() {
    let td = fixture();
    let repo = td.path();
    let p = policy();
    std::fs::write(repo.join(".gitattributes"), "*.png filter=upload\n").unwrap();
    run(repo, &["add", "--", ".gitattributes"]);
    run(repo, &["commit", "-qm", "attributes"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    run(
        repo,
        &[
            "config",
            "filter.upload.clean",
            "touch FILTER_EXECUTED; cat",
        ],
    );
    run(
        repo,
        &[
            "config",
            "filter.upload.process",
            "touch FILTER_EXECUTED; cat",
        ],
    );
    put(repo, "source.png", 500);
    assert_eq!(measure(repo, &p).await.unwrap(), p.max_stage_file_bytes);
    put(repo, "second.png", 500);
    // Identical plaintext can encrypt differently: reserve per path, not OID.
    assert_eq!(measure(repo, &p).await.unwrap(), 2 * p.max_stage_file_bytes);
    let mut unbounded = p.clone();
    unbounded.max_stage_file_bytes = u64::MAX;
    assert!(measure(repo, &unbounded).await.unwrap_err().to_string()
        .contains("unbounded-filter-ceiling"));
    // A zero ceiling drops every non-empty candidate through the SAME shared
    // size gate the commit path uses, so nothing is reserved and nothing can
    // be published.
    let mut zero = p.clone();
    zero.max_stage_file_bytes = 0;
    assert_eq!(measure(repo, &zero).await.unwrap(), 0);
    assert!(!repo.join("FILTER_EXECUTED").exists());
    assert_eq!(
        std::fs::metadata(repo.join("source.png")).unwrap().len(),
        500
    );
}

#[tokio::test]
async fn accounting_refuses_zero_ceiling_for_a_filtered_candidate_that_would_be_admitted() {
    let td = fixture();
    let repo = td.path();
    let mut p = policy();
    p.max_stage_file_bytes = 0;
    std::fs::write(repo.join(".gitattributes"), "*.png filter=upload\n").unwrap();
    run(repo, &["add", "--", ".gitattributes"]);
    run(repo, &["commit", "-qm", "attributes"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    run(repo, &["config", "filter.upload.clean", "touch FILTER_EXECUTED; cat"]);
    // An empty source passes the size gate, so its filtered output would be
    // admitted with no measurable bound: refuse instead of assuming it is free.
    put(repo, "empty.png", 0);
    assert!(measure(repo, &p).await.unwrap_err().to_string()
        .contains("unbounded-filter-ceiling"));
    assert!(!repo.join("FILTER_EXECUTED").exists());
}

#[tokio::test]
async fn accounting_accepts_literal_colon_paths_without_revision_interpretation() {
    let td = fixture();
    let repo = td.path();
    std::fs::write(repo.join(".gitattributes"), "* filter=expand\n").unwrap();
    put(repo, "head:0", 20);
    run(repo, &["add", "--", ".gitattributes", "head:0"]);
    run(repo, &["commit", "-qm", "literal colon path"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    // Both tracked paths carry the filter even though raw status is clean.
    assert_eq!(measure(repo, &policy()).await.unwrap(), 2 * policy().max_stage_file_bytes);
}

#[tokio::test]
async fn accounting_reserves_tracked_filter_even_when_raw_status_is_clean() {
    let td = fixture();
    let repo = td.path();
    put(repo, "source.txt", 20);
    std::fs::write(repo.join(".gitattributes"), "source.txt filter=expand\n").unwrap();
    run(repo, &["add", "--", "source.txt", ".gitattributes"]);
    run(repo, &["commit", "-qm", "plaintext baseline"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    run(repo, &["config", "filter.expand.clean", "touch FILTER_EXECUTED; cat; head -c 4096 /dev/zero"]);
    // Same raw bytes as index: only filter-aware inspection would see dirt.
    put(repo, "source.txt", 20);
    assert_eq!(measure(repo, &policy()).await.unwrap(), policy().max_stage_file_bytes);
    assert!(!repo.join("FILTER_EXECUTED").exists());
}

#[tokio::test]
async fn accounting_refuses_unsupported_encoding_without_transcoding() {
    let td = fixture();
    let repo = td.path();
    std::fs::write(repo.join(".gitattributes"), "*.txt working-tree-encoding=UTF-16\n").unwrap();
    run(repo, &["add", "--", ".gitattributes"]);
    run(repo, &["commit", "-qm", "encoding"]);
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    put(repo, "source.txt", 20);
    assert!(measure(repo, &policy()).await.unwrap_err().to_string()
        .contains("unsupported-encoding"));
}

#[tokio::test]
async fn accounting_snapshot_rejects_real_worktree_index_head_and_tip_changes() {
    let td = fixture();
    let repo = td.path();
    let command = status_args(repo).await.unwrap();
    let initial = snapshot(repo, &command).await.unwrap();
    put(repo, "edit.txt", 10);
    let dirty = snapshot(repo, &command).await.unwrap();
    assert!(initial.ensure_unchanged(&dirty).is_err());
    run(repo, &["add", "--", "edit.txt"]);
    let staged = snapshot(repo, &command).await.unwrap();
    assert!(dirty.ensure_unchanged(&staged).is_err());
    run(repo, &["commit", "-qm", "snapshot mutation"]);
    let committed = snapshot(repo, &command).await.unwrap();
    assert!(staged.ensure_unchanged(&committed).is_err());
    run(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let pushed = snapshot(repo, &command).await.unwrap();
    assert!(committed.ensure_unchanged(&pushed).is_err());
    assert!(pushed
        .ensure_unchanged(&snapshot(repo, &command).await.unwrap())
        .is_ok());
}

#[tokio::test]
async fn accounting_ignores_replacements_for_outgoing_deleted_history() {
    let td = fixture();
    let repo = td.path();
    let p = policy();
    put(repo, "original.txt", 700);
    run(repo, &["add", "--", "original.txt"]);
    run(repo, &["commit", "-qm", "historical blob"]);
    let original = String::from_utf8(run(repo, &["rev-parse", "HEAD:original.txt"])).unwrap();
    run(repo, &["rm", "-q", "--", "original.txt"]);
    run(repo, &["commit", "-qm", "remove from worktree"]);
    put(repo, "one.txt", 1);
    let replacement =
        String::from_utf8(run(repo, &["hash-object", "-w", "--no-filters", "one.txt"])).unwrap();
    std::fs::remove_file(repo.join("one.txt")).unwrap();
    run(repo, &["replace", original.trim(), replacement.trim()]);
    assert_eq!(measure(repo, &p).await.unwrap(), 700);
}

#[tokio::test]
async fn accounting_untracked_toggle_and_read_limit_are_explicit() {
    let td = fixture();
    let repo = td.path();
    let mut p = policy();
    put(repo, "new.txt", 400);
    p.auto_stage_untracked = false;
    assert_eq!(measure(repo, &p).await.unwrap(), 0);
    p.auto_stage_untracked = true;
    p.max_stage_file_bytes = MAX_FILE + 100;
    let large = std::fs::File::create(repo.join("large.txt")).unwrap();
    large.set_len(MAX_FILE + 1).unwrap();
    assert!(measure(repo, &p)
        .await
        .unwrap_err()
        .to_string()
        .contains("file-read-limit"));
    let mut total = MAX_JS_INTEGER;
    assert!(add(&mut total, 1).is_err());
}

#[tokio::test]
async fn accounting_symlink_sizes_link_text_not_referent() {
    let td = fixture();
    let repo = td.path();
    let p = policy();
    std::os::unix::fs::symlink("/etc/passwd", repo.join("link")).unwrap();
    assert_eq!(measure(repo, &p).await.unwrap(), "/etc/passwd".len() as u64);
}
