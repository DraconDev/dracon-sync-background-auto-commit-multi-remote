//! One concurrent fault episode, not a shell aggregation of independent tests.
//! All faults, hooks, local remotes and simulated objects live under TempDir.
use super::*;
use crate::daemon::recovery::SyncRecovery;
use crate::git::push_resources::{push_phase, with_local_work};
use crate::policy::SyncPolicy;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;

struct Fleet {
    root: tempfile::TempDir,
    watch: PathBuf,
    repos: Vec<PathBuf>,
    policy: SyncPolicy,
}
impl Fleet {
    fn git(repo: &Path, args: &[&str]) -> String {
        let out = crate::git::git_cmd()
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "fixture Git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let watch = root.path().join("watched");
        std::fs::create_dir(&watch).unwrap();
        let mut repos = Vec::new();
        for name in ["healthy", "filter", "hook", "bucket"] {
            let repo = watch.join(name);
            let bare = root.path().join(format!("{name}.git"));
            Self::git(
                root.path(),
                &["init", "--bare", "-q", bare.to_str().unwrap()],
            );
            Self::git(
                root.path(),
                &["init", "-q", "-b", "main", repo.to_str().unwrap()],
            );
            let hooks = repo.join(".git/fixture-hooks");
            std::fs::create_dir(&hooks).unwrap();
            // Ignore any installed init-template hooks as well as global hooks.
            // Disposable per-repo isolation, never global/production config.
            Self::git(
                &repo,
                &["config", "core.hooksPath", hooks.to_str().unwrap()],
            );
            Self::git(&repo, &["config", "filter.dracon.clean", "cat"]);
            std::fs::write(repo.join("data"), "before\n").unwrap();
            Self::git(&repo, &["add", "--", "data"]);
            Self::git(&repo, &["commit", "-qm", "fixture baseline"]);
            Self::git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
            Self::git(&repo, &["push", "-q", "-u", "origin", "main"]);
            repos.push(repo);
        }
        // Inherit the operator/CI's existing Git identity; never invent one.
        let name = Self::git(&repos[0], &["config", "user.name"]);
        let email = Self::git(&repos[0], &["config", "user.email"]);
        // Deserialize as production does: derive(Default) deliberately has
        // zero numeric limits, whereas serde supplies operational defaults.
        let mut policy: SyncPolicy = toml::from_str(
            r#"
auto_github_private = false
auto_pull = false
auto_push = false
auto_bump_versions = false
auto_commit = true
stage_op_timeout_secs = 3
"#,
        )
        .unwrap();
        policy.remotes.clear();
        policy.trusted_authors = vec![name];
        policy.trusted_emails = vec![email];
        Self {
            root,
            watch,
            repos,
            policy,
        }
    }
    fn scope(&self) -> Scope {
        Scope {
            roots: vec![self.watch.clone()],
            excluded_dirs: BTreeSet::new(),
            excluded_repos: vec![],
            system_repo: String::new(),
        }
    }
    fn head(repo: &Path) -> String {
        Self::git(repo, &["rev-parse", "HEAD"])
    }
    fn remote(repo: &Path) -> String {
        Self::git(repo, &["ls-remote", "origin", "refs/heads/main"])
            .split_whitespace()
            .next()
            .unwrap()
            .into()
    }
    fn count(repo: &Path, file: &str) -> usize {
        std::fs::read_to_string(repo.join(".git").join(file))
            .unwrap_or_default()
            .lines()
            .count()
    }
    async fn sync(&self, repo: &Path, capacity: Arc<Semaphore>) -> crate::sync::SyncOutcome {
        let permit = capacity.clone().acquire_owned().await.unwrap();
        with_local_work(
            capacity,
            permit,
            crate::sync::sync_repo(
                repo,
                &self.policy,
                &Default::default(),
                0,
                None,
                false,
                None,
            ),
        )
        .await
        .unwrap()
    }
    fn script(repo: &Path, path: &str, body: &str) {
        let path = repo.join(".git").join(path);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn configure_bucket(&self) {
        let repo = &self.repos[3];
        let config = serde_json::json!({"schemaVersion":1,"enabled":true,"manifest":"manifest.json","assetRoot":".","workflowScript":"workflow.mjs","uploaderCredentials": self.root.path().join("synthetic-uploader"),"consumerCredentials":self.root.path().join("synthetic-consumer")});
        std::fs::write(repo.join("bucket-assets.json"), config.to_string()).unwrap();
        std::fs::write(repo.join("manifest.json"), r#"{"schemaVersion":1,"bucket":"dracon-lfs","assets":[{"sourcePath":"data","visibility":"private","regenerable":true}]}"#).unwrap();
        // The simulated storage boundary has NO network imports, SDK, URLs or
        // real credentials. The daemon's verification/staging path stays real.
        std::fs::write(repo.join("workflow.mjs"), r#"import fs from 'node:fs';
import crypto from 'node:crypto';
fs.appendFileSync('.git/upload-attempts','attempt\n');
if(fs.existsSync('.git/bucket-outage')) process.exit(1);
const m=JSON.parse(fs.readFileSync('manifest.json')), a=m.assets[0], bytes=fs.readFileSync(a.sourcePath);
const sha=crypto.createHash('sha256').update(bytes).digest('hex');
fs.mkdirSync('.git/simulated-store',{recursive:true});
fs.writeFileSync('.git/simulated-store/'+sha,bytes);
if(a.sha256!==sha || !a.bucketed){a.sha256=sha;a.bytes=bytes.length;a.bucketed=true;fs.writeFileSync('manifest.json',JSON.stringify(m));}
const manifestSha256=crypto.createHash('sha256').update(fs.readFileSync('manifest.json')).digest('hex');
console.log(JSON.stringify({ok:true,manifestSha256}));
"#).unwrap();
        Self::git(
            repo,
            &[
                "add",
                "--",
                "bucket-assets.json",
                "manifest.json",
                "workflow.mjs",
            ],
        );
        Self::git(repo, &["commit", "-qm", "fixture bucket configuration"]);
        Self::git(repo, &["push", "-q", "origin", "main"]);
    }
}

struct OwnedTask<T>(Option<tokio::task::JoinHandle<T>>);
impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}
impl<T> OwnedTask<T> {
    async fn join(mut self) -> Result<T, tokio::task::JoinError> {
        let result = tokio::time::timeout(Duration::from_secs(8), self.0.as_mut().unwrap())
            .await
            .expect("owned task failed to settle");
        self.0.take();
        result
    }
}
// A blocked filesystem walk is not cancellable. Releasing the injected barrier
// on every exit, including assertion failure, lets the blocking pool shut down.
struct ReleaseWalk(Option<std::sync::mpsc::Sender<()>>);
impl Drop for ReleaseWalk {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}
async fn pids(repo: &Path, marker: &str) -> Vec<u32> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(repo.join(".git").join(marker)) {
                let pids: Vec<u32> = text
                    .split_whitespace()
                    .map(|p| p.parse().unwrap())
                    .collect();
                assert_eq!(pids.len(), 2);
                assert!(pids.iter().all(|p| *p > 0));
                return pids;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fault did not enter")
}
async fn stopped(pids: &[u32]) {
    tokio::time::timeout(Duration::from_secs(5), async {
        for pid in pids {
            loop {
                match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                    Ok(stat)
                        if matches!(
                            stat.rsplit_once(") ").unwrap().1.chars().next(),
                            Some('Z' | 'X')
                        ) =>
                    {
                        break
                    }
                    Ok(_) => tokio::time::sleep(Duration::from_millis(10)).await,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                    Err(e) => panic!("cannot establish process state: {e}"),
                }
            }
        }
    })
    .await
    .expect("owned fault descendants survived teardown");
}
async fn publish(repo: &Path, capacity: Arc<Semaphore>) -> anyhow::Result<()> {
    let permit = capacity.clone().acquire_owned().await.unwrap();
    with_local_work(
        capacity,
        permit,
        push_phase(crate::git::push_with_retries(
            repo,
            40,
            3,
            "integrated fixture push",
        )),
    )
    .await
}

#[tokio::test]
async fn integrated_failure_recovery_harness() {
    tokio::time::timeout(Duration::from_secs(60), episode())
        .await
        .expect("integrated episode exceeded its wall-clock budget");
}
async fn episode() {
    let fleet = Fleet::new();
    let _git =
        crate::test_helpers::GitBinRestorer::new(crate::policy::git_binary().to_str().unwrap());
    let [healthy, filter, hook, bucket] = fleet.repos.as_slice() else {
        unreachable!()
    };
    fleet.configure_bucket();
    let capacity = Arc::new(Semaphore::new(1));
    let healthy_baseline = Fleet::head(healthy);
    let mut discovery = Discovery::new();
    let mut initial = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(snapshot) = discovery.poll(fleet.scope()).await {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    initial.sort();
    let mut expected = fleet.repos.clone();
    expected.sort();
    assert_eq!(initial, expected);
    discovery.next_scan = Instant::now();
    let (send, wait) = std::sync::mpsc::channel();
    let mut release_walk = ReleaseWalk(Some(send));
    let scans = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let entered = scans.clone();
    discovery
        .poll_with(fleet.scope(), move |scope| {
            entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            wait.recv_timeout(Duration::from_secs(50))
                .expect("unreleased walk");
            crate::git::discover_git_repos(
                &scope.roots,
                &scope.excluded_dirs,
                &scope.excluded_repos,
                Some(&scope.system_repo),
            )
        })
        .await
        .unwrap();

    Fleet::script(filter, "fault-filter.sh", "#!/bin/sh\nif [ -e .git/filter-stall ]; then\n (trap '' TERM; exec sleep 90) </dev/null >/dev/null 2>&1 &\n helper=$!\n printf '%s %s\\n' \"$$\" \"$helper\" > .git/filter-pids.tmp\n mv .git/filter-pids.tmp .git/filter-pids\n wait\nfi\ncat\n");
    std::fs::write(filter.join(".gitattributes"), "data filter=stall\n").unwrap();
    Fleet::git(
        filter,
        &["config", "filter.stall.clean", "sh .git/fault-filter.sh"],
    );
    Fleet::git(filter, &["config", "filter.stall.required", "true"]);
    Fleet::git(filter, &["add", "--", ".gitattributes"]);
    Fleet::git(filter, &["commit", "-qm", "fixture required filter"]);
    Fleet::git(filter, &["push", "-q", "origin", "main"]);
    // Git's stat cache must see equal-size content as changed, not same tick.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    std::fs::write(filter.join("data"), "after!\n").unwrap();
    std::fs::write(filter.join(".git/filter-stall"), "stall").unwrap();
    let filter_baseline = Fleet::head(filter);
    let mut recovery = SyncRecovery::default();
    let at = Instant::now();
    recovery.reserve(filter).set(at).unwrap();
    let path = filter.clone();
    let inspection = OwnedTask(Some(tokio::spawn(async move {
        crate::git::inspect_status(&path, Duration::from_secs(40)).await
    })));
    let filter_pids = pids(filter, "filter-pids").await;

    std::fs::write(hook.join("data"), "hook mutation\n").unwrap();
    Fleet::git(hook, &["add", "--", "data"]);
    Fleet::git(hook, &["commit", "-qm", "fixture unpublished hook change"]);
    let hook_baseline = Fleet::remote(hook);
    Fleet::script(hook, "fixture-hooks/pre-push", "#!/bin/sh\nset -eu\necho attempt >> .git/hook-attempts\nif [ -e .git/hook-reject ]; then\n (trap '' TERM; exec sleep 90) </dev/null >/dev/null 2>&1 &\n helper=$!\n printf '%s %s\\n' \"$$\" \"$helper\" > .git/hook-pids.tmp\n mv .git/hook-pids.tmp .git/hook-pids\n while [ ! -e .git/hook-release ]; do sleep 0.01; done\n kill -KILL \"$helper\" 2>/dev/null || :\n wait \"$helper\" 2>/dev/null || :\n echo 'pre-push: fixture policy rejects publication' >&2\n exit 1\nfi\n");
    std::fs::write(hook.join(".git/hook-reject"), "reject").unwrap();
    let path = hook.clone();
    let lane = capacity.clone();
    let pushing = OwnedTask(Some(tokio::spawn(
        async move { publish(&path, lane).await },
    )));
    let hook_pids = pids(hook, "hook-pids").await;
    std::fs::write(bucket.join("data"), "explicitly regenerated bucket asset\n").unwrap();
    std::fs::write(bucket.join(".git/bucket-outage"), "outage").unwrap();
    let bucket_baseline = Fleet::head(bucket);
    let manifest_baseline = std::fs::read(bucket.join("manifest.json")).unwrap();
    let raw = std::fs::read(bucket.join("data")).unwrap();
    let mut bucket_attempts = 0;
    let mut healthy_heads = Vec::new();
    for tick in 0..2 {
        // Production recovery clock is injected through its normal Instant
        // parameters; no sleeping through minute-long backoff in a test.
        let now = at + Duration::from_secs(tick * 120);
        assert!(recovery.ready(bucket, now));
        recovery.reserve(bucket).set(now).unwrap();
        assert!(matches!(
            fleet.sync(bucket, capacity.clone()).await,
            crate::sync::SyncOutcome::Blocked
        ));
        bucket_attempts += 1;
        recovery.finish(bucket, true, now);
        for _ in 0..100 {
            assert!(!recovery.ready(bucket, now));
            assert_eq!(Fleet::count(bucket, "upload-attempts"), bucket_attempts);
            let mut snapshot = discovery
                .poll_with(fleet.scope(), |_| panic!("duplicate discovery walk"))
                .await
                .unwrap();
            snapshot.sort();
            assert_eq!(snapshot, expected);
        }
        assert_eq!(Fleet::head(bucket), bucket_baseline);
        assert_eq!(Fleet::remote(bucket), bucket_baseline);
        assert_eq!(
            std::fs::read(bucket.join("manifest.json")).unwrap(),
            manifest_baseline
        );
        assert_eq!(std::fs::read(bucket.join("data")).unwrap(), raw);
        assert!(!bucket.join(".git/simulated-store").exists());
        std::fs::write(healthy.join("data"), format!("healthy progress {tick}\n")).unwrap();
        fleet.sync(healthy, capacity.clone()).await;
        healthy_heads.push(Fleet::head(healthy));
        assert_ne!(healthy_heads[tick as usize], healthy_baseline);
        assert_eq!(
            Fleet::git(healthy, &["show", "HEAD:data"]),
            format!("healthy progress {tick}")
        );
        // Push admission is deliberately single-flight. Independent local
        // commits must progress, but publication queues behind the slow hook.
        assert_eq!(Fleet::remote(healthy), healthy_baseline);
        assert!(!inspection.0.as_ref().unwrap().is_finished());
        assert!(!pushing.0.as_ref().unwrap().is_finished());
        assert!(!discovery.pending.as_ref().unwrap().is_finished());
        assert_eq!(Fleet::head(filter), filter_baseline);
        assert_eq!(Fleet::remote(hook), hook_baseline);
    }
    assert_ne!(healthy_heads[0], healthy_heads[1]);
    assert_eq!(scans.load(std::sync::atomic::Ordering::SeqCst), 1);
    eprintln!("PASS: healthy inspection/commits progress during all four faults; blocked storage preserves HEAD, manifest and raw bytes");

    std::fs::write(hook.join(".git/hook-release"), "release to reject").unwrap();
    let failure = pushing.join().await.unwrap().unwrap_err();
    assert!(crate::git::is_local_hook_rejection(&failure.to_string()));
    assert_eq!(
        Fleet::count(hook, "hook-attempts"),
        1,
        "permanent refusal must not spend configured three attempts"
    );
    assert_eq!(Fleet::remote(hook), hook_baseline);
    stopped(&hook_pids).await;
    let now = at + Duration::from_secs(241);
    assert!(recovery
        .expired(now, Duration::from_secs(1))
        .contains(filter));
    assert!(recovery.expired(now, Duration::from_secs(1)).is_empty());
    inspection.0.as_ref().unwrap().abort();
    assert!(inspection.join().await.unwrap_err().is_cancelled());
    stopped(&filter_pids).await;
    recovery.finish(filter, true, now);
    assert!(!recovery.ready(filter, now));
    assert_eq!(Fleet::head(filter), filter_baseline);
    assert_eq!(Fleet::remote(filter), filter_baseline);
    eprintln!("PASS: bounded storage retries, fail-fast hook rejection and typed inspection cancellation clean owned descendants");

    std::fs::remove_file(filter.join(".git/filter-stall")).unwrap();
    std::fs::remove_file(hook.join(".git/hook-reject")).unwrap();
    std::fs::remove_file(bucket.join(".git/bucket-outage")).unwrap();
    release_walk.0.take().unwrap().send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !discovery.pending.as_ref().unwrap().is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut refreshed = discovery
        .poll_with(fleet.scope(), |_| {
            panic!("unexpected retry after walk join")
        })
        .await
        .unwrap();
    refreshed.sort();
    assert_eq!(refreshed, expected);
    assert!(discovery.pending.is_none());
    let recovered_at = now + Duration::from_secs(120);
    for repo in [filter, bucket] {
        assert!(recovery.ready(repo, recovered_at));
        assert!(
            !crate::git::inspect_status(repo, Duration::from_secs(3))
                .await
                .unwrap()
                .is_clean
        );
        fleet.sync(repo, capacity.clone()).await;
        publish(repo, capacity.clone()).await.unwrap();
        recovery.finish(repo, false, recovered_at);
        assert!(recovery.ready(repo, recovered_at));
        assert_eq!(Fleet::remote(repo), Fleet::head(repo));
    }
    publish(healthy, capacity.clone()).await.unwrap();
    assert_eq!(Fleet::remote(healthy), Fleet::head(healthy));
    publish(hook, capacity.clone()).await.unwrap();
    assert_eq!(Fleet::count(hook, "hook-attempts"), 2);
    assert_eq!(Fleet::remote(hook), Fleet::head(hook));
    assert_ne!(Fleet::head(filter), filter_baseline);
    assert_ne!(Fleet::head(bucket), bucket_baseline);
    assert_eq!(Fleet::git(bucket, &["show", "HEAD:data"]), "before");
    let manifest: serde_json::Value =
        serde_json::from_str(&Fleet::git(bucket, &["show", "HEAD:manifest.json"])).unwrap();
    let digest = manifest["assets"][0]["sha256"].as_str().unwrap();
    assert_eq!(
        std::fs::read(bucket.join(".git/simulated-store").join(digest)).unwrap(),
        raw
    );
    assert_eq!(Fleet::count(bucket, "upload-attempts"), 3);
    let recovered_head = Fleet::head(bucket);
    fleet.sync(bucket, capacity.clone()).await;
    assert_eq!(
        Fleet::head(bucket),
        recovered_head,
        "unchanged materialization must not create another commit"
    );
    assert_eq!(capacity.available_permits(), 1);
    stopped(&filter_pids).await;
    stopped(&hook_pids).await;
    eprintln!("PASS: discovery joins once; cleared filter/hook/storage faults recover; verified references publish without raw blobs or duplicate commits; all permits returned");
}
