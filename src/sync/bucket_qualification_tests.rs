//! Hermetic daemon-worker -> real runner -> real publisher qualification.
//! Only storage primitives are substituted; no prebuilt manifest or success verdict.
use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const MANIFEST: &str = "bucket-assets.manifest.json";
const SOURCES: [&str; 2] = ["public/a.png", "public/b.png"];
struct Fixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
    policy: SyncPolicy,
    source: [Vec<u8>; 2],
}
fn git(cwd: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("timeout")
        .arg("20s")
        .arg(crate::policy::git_binary())
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "fixture Git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn read(path: impl AsRef<Path>) -> Vec<u8> {
    std::fs::read(path).unwrap()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        // Isolated temporary repo only, never operator repositories or global hooks.
        // Identity is inherited unchanged from the operator/CI Git configuration.
        git(&repo, &["config", "core.hooksPath", "/dev/null"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/operator/qualification.git",
            ],
        );
        let helper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/fixtures/bucket-daemon-transport.mjs");
        let setup = Command::new("timeout")
            .args(["20s", "node"])
            .arg(helper)
            .arg("setup")
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(
            setup.status.success(),
            "setup: {}",
            String::from_utf8_lossy(&setup.stderr)
        );
        git(
            &repo,
            &[
                "add",
                "--",
                "package.json",
                "package-lock.json",
                "index.html",
                "bucket-runner.json",
                ".dracon/dracon-sync.toml",
            ],
        );
        git(&repo, &["commit", "-qm", "qualification consumer baseline"]);
        git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let policy = toml::from_str::<SyncPolicy>(
            r#"
auto_commit = true
auto_push = false
auto_pull = false
auto_stage_untracked = true
auto_bump_versions = false
auto_harden_with_warden = false
standard_files_auto = false
build_artifact_cleanup = false
stage_op_timeout_secs = 30
"#,
        )
        .unwrap();
        let source = SOURCES.map(|p| read(repo.join(p)));
        assert!(source.iter().all(|b| b.len() > 1024));
        assert!(!repo.join(MANIFEST).exists());
        assert!(!repo.join("bucket-binding.json").exists());
        assert!(std::fs::read_dir(temp.path().join("state"))
            .unwrap()
            .next()
            .is_none());
        Self {
            temp,
            repo,
            policy,
            source,
        }
    }
    fn mode(&self, mode: &str) {
        std::fs::write(
            self.temp.path().join("control.json"),
            json!({"mode": mode}).to_string(),
        )
        .unwrap();
    }
    fn head(&self) -> Vec<u8> {
        git(&self.repo, &["rev-parse", "HEAD"])
    }
    fn operations(&self) -> Vec<Value> {
        String::from_utf8(read(self.temp.path().join("operations.jsonl")))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    fn stored(&self) -> usize {
        self.operations()
            .iter()
            .filter(|o| o["op"] == "stored")
            .count()
    }
    fn intact(&self) {
        for (p, expected) in SOURCES.iter().zip(&self.source) {
            assert_eq!(&read(self.repo.join(p)), expected, "local data lost: {p}");
            assert!(
                git(&self.repo, &["ls-files", "--", p]).is_empty(),
                "bucket materialization was staged"
            );
        }
    }
    async fn cycle(&self) -> SyncOutcome {
        // Same production worker entry called by the daemon, including cleaning,
        // discovery, measured pressure, bridge child, verification and Git commit.
        let outcome = tokio::time::timeout(
            Duration::from_secs(90),
            sync_repo(
                &self.repo,
                &self.policy,
                &BTreeSet::new(),
                10000,
                None,
                false,
                None,
            ),
        )
        .await
        .expect("bounded daemon worker")
        .expect("worker result");
        if outcome == SyncOutcome::Blocked {
            eprintln!("qualification operations: {:?}", self.operations());
            eprintln!("staged paths: {}", String::from_utf8_lossy(&git(&self.repo, &["diff", "--cached", "--name-only"])));
        }
        outcome
    }
    fn assert_enrolled(&self) -> (Vec<u8>, PathBuf, Vec<u8>) {
        self.intact();
        let bytes = read(self.repo.join(MANIFEST));
        let manifest: Value = serde_json::from_slice(&bytes).unwrap();
        let entries = manifest["assets"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        for (path, expected) in SOURCES.iter().zip(&self.source) {
            let entry = entries.iter().find(|a| a["sourcePath"] == *path).unwrap();
            assert_eq!(entry["bucketed"], true);
            assert_eq!(entry["sha256"], hash(expected));
            assert_eq!(entry["bytes"], expected.len() as u64);
        }
        assert_eq!(
            git(&self.repo, &["show", &format!("HEAD:{MANIFEST}")]),
            bytes
        );
        assert_eq!(
            git(&self.repo, &["show", "HEAD:bucket-binding.json"]),
            read(self.repo.join("bucket-binding.json"))
        );
        let index = self.temp.path().join("state").join(format!(
            "{}.index.json",
            manifest["repository"].as_str().unwrap()
        ));
        let index_bytes = read(&index);
        let parsed: Value = serde_json::from_slice(&index_bytes).unwrap();
        assert_eq!(parsed["manifestDigest"], hash(&bytes));
        assert_eq!(parsed["namespace"], manifest["namespace"]);
        (bytes, index, index_bytes)
    }
}

#[tokio::test]
async fn bucket_qualification_daemon_new_enrollment_retry_and_fresh_restore() {
    let f = Fixture::new();
    let before = f.head();
    assert_eq!(f.cycle().await, SyncOutcome::Synced);
    assert_ne!(
        before,
        f.head(),
        "daemon must commit NEW enrollment, not merely inspect old files"
    );
    let (manifest, index, index_bytes) = f.assert_enrolled();
    let head = f.head();
    assert_eq!(f.stored(), 2);
    assert_ne!(f.cycle().await, SyncOutcome::Blocked);
    assert_eq!(f.head(), head);
    assert_eq!(read(f.repo.join(MANIFEST)), manifest);
    assert_eq!(read(index), index_bytes);
    assert_eq!(f.stored(), 2);
    f.intact();
    let reader = f.temp.path().join("reader");
    git(
        f.temp.path(),
        &[
            "clone",
            "-q",
            "--no-hardlinks",
            f.repo.to_str().unwrap(),
            reader.to_str().unwrap(),
        ],
    );
    std::fs::create_dir_all(reader.join("public")).unwrap();
    for p in SOURCES {
        assert!(
            !reader.join(p).exists(),
            "fresh clone unexpectedly contains materialization"
        );
    }
    let binding_before = read(f.temp.path().join("state/bindings.json"));
    let request = json!({"schemaVersion":1, "operation":"restore", "root":reader,
        "profilePath": f.temp.path().join("profile.json")});
    use std::io::Write;
    let mut child = Command::new("timeout")
        .args(["30s", "node"])
        .arg(f.temp.path().join("runner.bundle.mjs"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let verdict: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(verdict["status"], "restored");
    for (p, bytes) in SOURCES.iter().zip(&f.source) {
        let restored = read(reader.join(p));
        assert_eq!(&restored, bytes);
        assert_eq!(hash(&restored), hash(bytes));
    }
    assert_eq!(
        read(f.temp.path().join("state/bindings.json")),
        binding_before
    );
    assert_eq!(f.stored(), 2, "consumer cannot upload");
    assert!(f
        .operations()
        .iter()
        .any(|o| o["op"] == "download" && o["role"] == "consumer"));
}

#[tokio::test]
async fn bucket_qualification_daemon_outage_partial_upload_and_recovery() {
    for (mode, expected_stored) in [("outage", 0), ("partial", 1)] {
        let f = Fixture::new();
        let head = f.head();
        let index = git(&f.repo, &["ls-files", "--stage", "-z"]);
        f.mode(mode);
        assert_eq!(
            f.cycle().await,
            SyncOutcome::Blocked,
            "{mode} must not appear synced"
        );
        assert_eq!(head, f.head());
        assert_eq!(index, git(&f.repo, &["ls-files", "--stage", "-z"]));
        assert!(!f.repo.join(MANIFEST).exists());
        f.intact();
        assert_eq!(f.stored(), expected_stored);
        assert!(f.operations().iter().any(|o| o["op"] == "verdict"
            && o["status"] == "refused"
            && o["commitEligible"] == false));
        f.mode("healthy");
        assert_eq!(f.cycle().await, SyncOutcome::Synced);
        let (manifest, index, index_bytes) = f.assert_enrolled();
        let recovered_head = f.head();
        assert_eq!(
            f.stored(),
            2,
            "retry must reuse previously verified remote object"
        );
        assert_ne!(f.cycle().await, SyncOutcome::Blocked);
        assert_eq!(f.stored(), 2);
        assert_eq!(read(f.repo.join(MANIFEST)), manifest);
        assert_eq!(read(index), index_bytes);
        assert_eq!(f.head(), recovered_head);
        f.intact();
    }
}

#[tokio::test]
async fn bucket_qualification_daemon_concurrent_manifest_preserves_independent_writer() {
    let f = Fixture::new();
    let head = f.head();
    let index = git(&f.repo, &["ls-files", "--stage", "-z"]);
    f.mode("race");
    assert_eq!(f.cycle().await, SyncOutcome::Blocked);
    assert_eq!(head, f.head());
    assert_eq!(index, git(&f.repo, &["ls-files", "--stage", "-z"]));
    f.intact();
    let concurrent = b"{\n  \"independentWriter\": \"must survive qualification refusal\"\n}\n";
    assert_eq!(read(f.repo.join(MANIFEST)), concurrent);
    assert!(f
        .operations()
        .iter()
        .any(|o| o["op"] == "verdict" && o["status"] == "refused"));
}
