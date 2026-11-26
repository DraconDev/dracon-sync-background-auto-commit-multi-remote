//! Git retains the desired branch tip. This file retains each destination's
//! last confirmed tip and retry state, independently of daemon memory.
use super::*;
use crate::daemon::RemoteFailInfo;
use crate::git::{push_resources, PublicationTarget};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STATE_FILE: &str = "dracon-sync.publication.json";

#[derive(Clone, Default, Deserialize, Serialize)]
struct Destination {
    endpoint: String,
    confirmed_tip: Option<String>,
    #[serde(default)]
    pending_tip: Option<String>,
    failed_tip: Option<String>,
    failure: Option<RemoteFailInfo>,
}

type Branches = HashMap<String, HashMap<String, Destination>>;

#[derive(Clone)]
pub(in crate::sync) struct Queue {
    path: PathBuf,
    branches: Branches,
    endpoints: HashMap<String, Option<String>>,
}

impl Queue {
    pub(super) fn load(
        repo: &Path,
        has_origin: bool,
        remotes: &[crate::policy::RemoteConfig],
        target: &PublicationTarget,
    ) -> Option<Self> {
        let path = crate::git::sync_lease::repository_identity(repo)
            .ok()?
            .join(STATE_FILE);
        let branches = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                eprintln!(
                    "⚠️ {} publication state could not be read: {error}",
                    repo.display()
                );
                HashMap::new()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => {
                eprintln!(
                    "⚠️ {} publication state could not be read: {error}",
                    repo.display()
                );
                HashMap::new()
            }
        };
        let origin = has_origin.then_some("origin");
        let endpoints = origin
            .into_iter()
            .chain(remotes.iter().map(|remote| remote.name.as_str()))
            .map(|name| (name.to_owned(), push_endpoint(repo, name)))
            .collect();
        let mut queue = Self {
            path,
            branches,
            endpoints,
        };
        queue.schedule(repo, target);
        Some(queue)
    }

    fn schedule(&mut self, repo: &Path, target: &PublicationTarget) {
        let entries = self.branches.entry(target.branch().to_owned()).or_default();
        let mut changed = false;
        entries.retain(|name, _| {
            let kept = self.endpoints.contains_key(name);
            changed |= !kept;
            kept
        });
        for (name, endpoint) in &self.endpoints {
            let Some(endpoint) = endpoint else { continue };
            let entry = entries.entry(name.clone()).or_default();
            if entry.endpoint != *endpoint {
                *entry = Destination {
                    endpoint: endpoint.clone(),
                    ..Destination::default()
                };
            }
            let next = (!confirmed(repo, entry, name, target)).then_some(target.object_id());
            if entry.pending_tip.as_deref() != next {
                entry.pending_tip = next.map(str::to_owned);
                changed = true;
            }
        }
        if changed {
            self.checkpoint();
        }
    }

    fn destination(&self, branch: &str, name: &str) -> Option<&Destination> {
        let destination = self.branches.get(branch)?.get(name)?;
        self.endpoints
            .get(name)?
            .as_ref()
            .filter(|endpoint| **endpoint == destination.endpoint)?;
        Some(destination)
    }

    fn record(&mut self, target: &PublicationTarget, name: &str, failure: Option<RemoteFailInfo>) {
        let Some(endpoint) = self.endpoints.get(name).and_then(Option::as_ref) else {
            return;
        };
        let entry = self
            .branches
            .entry(target.branch().to_owned())
            .or_default()
            .entry(name.to_owned())
            .or_default();
        if entry.endpoint != *endpoint {
            *entry = Destination {
                endpoint: endpoint.clone(),
                ..Destination::default()
            };
        }
        if failure.is_some() {
            entry.failed_tip = Some(target.object_id().to_owned());
            entry.pending_tip = Some(target.object_id().to_owned());
        } else {
            entry.confirmed_tip = Some(target.object_id().to_owned());
            entry.failed_tip = None;
            entry.pending_tip = None;
        }
        entry.failure = failure;
        self.checkpoint();
    }

    fn checkpoint(&self) {
        if let Err(error) = self.save() {
            eprintln!(
                "⚠️ {} publication state write failed: {error}",
                self.path.display()
            );
        }
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(dir) = self.path.parent() else {
            return Ok(());
        };
        let file = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer(file.as_file(), &self.branches)?;
        file.as_file().sync_all()?;
        file.persist(&self.path)?;
        std::fs::File::open(dir)?.sync_all()
    }
}

pub(crate) fn pending(repo: &Path) -> bool {
    let Ok(common) = crate::git::sync_lease::repository_identity(repo) else {
        return false;
    };
    let Ok(bytes) = std::fs::read(common.join(STATE_FILE)) else {
        return false;
    };
    let Ok(branches) = serde_json::from_slice::<Branches>(&bytes) else {
        return false;
    };
    let branch = crate::git::current_branch_from_head(repo).unwrap_or_else(|| "main".to_owned());
    branches
        .get(&branch)
        .is_some_and(|entries| entries.values().any(|entry| entry.pending_tip.is_some()))
}

pub(crate) fn clear_retry(repo: &Path) -> Result<()> {
    let common = crate::git::sync_lease::repository_identity(repo)?;
    let path = common.join(STATE_FILE);
    if !path.try_exists()? {
        return Ok(());
    }
    let _lease = crate::git::sync_lease::try_acquire_publication(repo)?
        .context("publication is active; retry after it finishes")?;
    let mut branches: Branches = serde_json::from_slice(&std::fs::read(&path)?)?;
    for destination in branches.values_mut().flat_map(HashMap::values_mut) {
        destination.failure = None;
        destination.failed_tip = None;
    }
    Queue {
        path,
        branches,
        endpoints: HashMap::new(),
    }
    .save()?;
    Ok(())
}

fn push_endpoint(repo: &Path, name: &str) -> Option<String> {
    let output = crate::policy::std_git_command()
        .args(["remote", "get-url", "--push", "--all", name])
        .current_dir(repo)
        .output()
        .ok()?;
    (output.status.success() && !output.stdout.is_empty())
        .then(|| format!("{:x}", Sha256::digest(&output.stdout)))
}

fn paused(destination: &Destination, target: &PublicationTarget, now: u64, incident: bool) -> bool {
    let Some(fail) = &destination.failure else {
        return false;
    };
    if fail.last_attempt_unix == 0 || fail.last_attempt_unix > now {
        return false;
    }
    // A repo hook's refusal is about the revision it inspected. A repaired
    // revision can be tried immediately, while a transport outage cools down
    // even if commits continue to arrive.
    if crate::git::is_local_hook_rejection(&fail.last_error)
        && destination.failed_tip.as_deref() != Some(target.object_id())
    {
        return false;
    }
    crate::daemon::mirror_push_paused_impl(fail, now, incident) || now - fail.last_attempt_unix < 30
}

fn confirmed(
    repo: &Path,
    destination: &Destination,
    name: &str,
    target: &PublicationTarget,
) -> bool {
    if destination.confirmed_tip.as_deref() != Some(target.object_id())
        || destination.failure.is_some()
    {
        return false;
    }
    // A receipt is historical. An invalidated or changed tracking ref must
    // allow reconciliation with the destination again.
    let tracking = format!("refs/remotes/{name}/{}", target.branch());
    git2::Repository::open(repo)
        .and_then(|repo| repo.refname_to_id(&tracking))
        .is_ok_and(|oid| oid.to_string() == target.object_id())
}

enum Leg {
    Deferred,
    Confirmed,
    Attempted(PublicationTarget, Result<()>),
}

async fn admitted_tip(
    repo: &Path,
    planned: &PublicationTarget,
    attached_branch: Option<&str>,
    github: bool,
) -> Result<Option<PublicationTarget>> {
    let current_branch = crate::git::current_branch_from_head(repo);
    if current_branch.as_deref() != attached_branch {
        return Ok(None);
    }
    let Some(branch) = current_branch.as_deref() else {
        // Detached nested worktrees publish the already qualified object.
        return Ok(Some(planned.clone()));
    };
    let Some(current) = pin_current_tip(repo, branch).await? else {
        return Ok(None);
    };
    if current == *planned {
        return Ok(Some(current));
    }
    let child = crate::git::spawn_git_command_cancellable(
        repo,
        &[
            "merge-base",
            "--is-ancestor",
            planned.object_id(),
            current.object_id(),
        ],
        "publication-ancestry",
    )?;
    let (status, _, _) = tokio::time::timeout(
        Duration::from_secs(20),
        crate::git::run_git_captured_output(child, repo, "publication-ancestry"),
    )
    .await
    .context("publication ancestry deadline exceeded")??;
    if !status.success() || crate::git::current_branch_from_head(repo) != current_branch {
        return Ok(None);
    }
    if github {
        // The round's GitHub size check qualified its original tip. A new
        // tip needs another verdict. This guard reads HEAD, so use its verdict
        // only while HEAD still agrees with the selected object.
        let (too_large, _) = crate::git::github_pack_too_large_async(repo, None).await;
        if too_large || crate::git::head_tip(repo).as_deref() != Some(current.object_id()) {
            return Ok(Some(planned.clone()));
        }
    }
    Ok(Some(current))
}

async fn destination_leg(
    repo: &Path,
    plan: &PublicationPlan,
    policy: &SyncPolicy,
    name: &str,
    host: Option<&str>,
    incident: bool,
) -> Leg {
    let planned = plan.target.as_ref().expect("upload has a target");
    let queue = plan.queue.as_ref().expect("upload has a queue");
    let previous = queue.destination(planned.branch(), name);
    if previous
        .is_some_and(|entry| paused(entry, planned, crate::policy::timestamp_secs(), incident))
    {
        return Leg::Deferred;
    }
    let mut attempted = None;
    let result = push_resources::publication_destination(repo, async {
        if queue.endpoints.get(name) != Some(&push_endpoint(repo, name)) {
            return Ok(Leg::Deferred);
        }
        let Some(target) = admitted_tip(
            repo,
            planned,
            plan.attached_branch.as_deref(),
            host == Some("github.com"),
        )
        .await?
        else {
            return Ok(Leg::Deferred);
        };
        if previous.is_some_and(|entry| confirmed(repo, entry, name, &target)) {
            return Ok(Leg::Confirmed);
        }
        attempted = Some(target.clone());
        if name == "origin" {
            Box::pin(crate::git::push_with_retries_for_target(
                repo,
                plan.timeout_secs,
                policy.push_retries,
                "push",
                Some(&target),
            ))
            .await?;
        } else {
            // A pinned mirror upload never executes force-push reconciliation.
            Box::pin(crate::git::multi_remote::push_to_named_remote_for_target(
                repo,
                name,
                plan.timeout_secs,
                policy.push_retries,
                false,
                Some(&target),
            ))
            .await?;
        }
        Ok(Leg::Confirmed)
    })
    .await;
    match (attempted, result) {
        (Some(target), result) => Leg::Attempted(target, result.map(|_| ())),
        (None, Ok(leg)) => leg,
        (None, Err(error)) => {
            eprintln!(
                "⚠️ {} could not prepare publication to {name}: {error:#}",
                repo.display()
            );
            Leg::Deferred
        }
    }
}

pub(in crate::sync) async fn upload(
    repo: &Path,
    policy: &SyncPolicy,
    plan: &mut PublicationPlan,
    remote_host: impl Fn(&str) -> Option<String>,
    incident_hosts: &HashSet<String>,
) -> Result<PushReport> {
    use futures::{FutureExt, StreamExt};

    let original = plan.target.as_ref().expect("upload has a target");
    let mut names: Vec<_> = plan.remotes.iter().collect();
    names.sort_by_key(|remote| remote.priority);
    let mut names: Vec<_> = names
        .into_iter()
        .map(|remote| remote.name.as_str())
        .collect();
    if plan.has_origin {
        names.insert(0, "origin");
    }
    let queue = plan.queue.as_ref().expect("upload has a queue");
    let mut failures = std::mem::take(&mut plan.remote_failures);
    failures.retain(|_, failure| failure.last_attempt_unix == 0);
    for name in &names {
        if let Some(failure) = queue
            .destination(original.branch(), name)
            .and_then(|entry| entry.failure.as_ref())
        {
            failures.insert((*name).to_owned(), failure.clone());
        }
    }

    // Own all futures in this round. Persist each completed leg before
    // waiting for another so a slow mirror or shutdown cannot lose it.
    let mut pending = futures::stream::FuturesUnordered::new();
    let plan_read = &*plan;
    for name in names {
        let host = remote_host(name);
        let incident = host
            .as_ref()
            .is_some_and(|host| incident_hosts.contains(host));
        pending.push(async move {
            let leg = std::panic::AssertUnwindSafe(destination_leg(
                repo,
                plan_read,
                policy,
                name,
                host.as_deref(),
                incident,
            ))
            .catch_unwind()
            .await
            .unwrap_or(Leg::Deferred);
            (name, leg)
        });
    }
    // The in-flight futures read the round-start snapshot. The writer owns
    // its own copy and atomically checkpoints the latest merged results.
    let mut updated = queue.clone();
    let mut attempted = false;
    let mut degraded = false;
    let mut failed = false;
    let mut interrupted = None;
    while let Some((name, leg)) = pending.next().await {
        match leg {
            Leg::Deferred => degraded = true,
            Leg::Confirmed => {}
            Leg::Attempted(target, Ok(())) => {
                attempted = true;
                failures.remove(name);
                updated.record(&target, name, None);
            }
            Leg::Attempted(_, Err(error)) if crate::daemon::push_error_is_cancellation(&error) => {
                interrupted = Some(error);
            }
            Leg::Attempted(_, Err(error))
                if crate::git::is_push_unit_resource_kill(&error.to_string()) =>
            {
                eprintln!(
                    "⚠️ {} publication to {name} interrupted by resource pressure: {error}",
                    repo.display()
                );
                degraded = true;
            }
            Leg::Attempted(target, Err(error)) => {
                attempted = true;
                failed = true;
                let error = crate::ownership::redact_url_credentials(&format!("{error:#}"));
                eprintln!(
                    "⚠️ {} publication to {name} failed: {error}",
                    repo.display()
                );
                let previous = updated.destination(target.branch(), name);
                let consecutive = previous
                    .filter(|entry| {
                        entry.failure.as_ref().is_some_and(|failure| {
                            !crate::git::is_local_hook_rejection(&failure.last_error)
                        }) || entry.failed_tip.as_deref() == Some(target.object_id())
                    })
                    .and_then(|entry| entry.failure.as_ref().map(|failure| failure.consecutive))
                    .unwrap_or(0)
                    .saturating_add(1);
                let failure = RemoteFailInfo {
                    consecutive,
                    last_error: error,
                    last_attempt_unix: crate::policy::timestamp_secs(),
                };
                updated.record(&target, name, Some(failure.clone()));
                failures.insert(name.to_owned(), failure);
            }
        }
    }
    drop(pending);
    plan.remote_failures = failures;
    plan.queue = Some(updated);
    if !failed {
        if let Some(error) = interrupted {
            return Err(error);
        }
    }
    if !attempted {
        return Ok(PushReport::AllPaused);
    }
    Ok(PushReport::Attempted {
        ok: !failed,
        degraded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = crate::policy::std_git_command()
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn commit(repo: &Path, message: &str) -> PublicationTarget {
        git(repo, &["commit", "--allow-empty", "-m", message]);
        PublicationTarget::new(&git(repo, &["rev-parse", "HEAD"]), "main").unwrap()
    }

    #[tokio::test]
    async fn live_tip_is_refreshed_at_admission_and_a_changed_branch_defers() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path();
        git(repo, &["init", "-b", "main"]);
        let planned = commit(repo, "original");

        let limiter = std::sync::Arc::new(tokio::sync::Semaphore::new(2));
        let occupied = limiter.clone().acquire_many_owned(2).await.unwrap();
        let mut admitted = Box::pin(push_resources::with_test_publication_admission(
            limiter,
            push_resources::background_pushes(push_resources::publication_destination(
                repo,
                admitted_tip(repo, &planned, Some("main"), false),
            )),
        ));
        assert!(tokio::time::timeout(Duration::from_millis(20), admitted.as_mut())
            .await
            .is_err());
        let newer = commit(repo, "arrived while waiting");
        drop(occupied);
        assert_eq!(admitted.await.unwrap(), Some(newer.clone()));
        git(repo, &["checkout", "-b", "other"]);
        assert_eq!(
            admitted_tip(repo, &planned, Some("main"), false)
                .await
                .unwrap(),
            None
        );
        git(repo, &["checkout", "main"]);
        git(repo, &["reset", "--hard", planned.object_id()]);
        let alternate = commit(repo, "rewritten sibling");
        assert_ne!(alternate, newer);
        assert_eq!(
            admitted_tip(repo, &newer, Some("main"), false)
                .await
                .unwrap(),
            None
        );
    }

    #[test]
    fn pending_and_retry_state_survives_reload_per_destination() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path();
        git(repo, &["init", "-b", "main"]);
        git(repo, &["remote", "add", "origin", "./github.git"]);
        git(repo, &["remote", "add", "gitlab", "./gitlab.git"]);
        let target = commit(repo, "initial");
        let mirror: crate::policy::RemoteConfig =
            toml::from_str("name = 'gitlab'\npush_url = './gitlab.git'\npriority = 10").unwrap();
        let remotes = [mirror];
        let mut queue = Queue::load(repo, true, &remotes, &target).unwrap();
        assert!(
            pending(repo),
            "intent must be durable before the first push"
        );
        queue.record(&target, "origin", None);
        git(
            repo,
            &["update-ref", "refs/remotes/origin/main", target.object_id()],
        );
        let outage = RemoteFailInfo {
            consecutive: 3,
            last_error: "network unavailable".to_owned(),
            last_attempt_unix: 100,
        };
        queue.record(&target, "gitlab", Some(outage));
        drop(queue);

        let queue = Queue::load(repo, true, &remotes, &target).unwrap();
        let github = queue.destination("main", "origin").unwrap();
        assert!(confirmed(repo, github, "origin", &target));
        assert!(github.pending_tip.is_none());
        let gitlab = queue.destination("main", "gitlab").unwrap();
        assert!(paused(gitlab, &target, 101, false));
        assert!(!paused(
            gitlab,
            &target,
            100 + crate::daemon::MIRROR_PAUSE_REPROBE_SECS,
            false
        ));

        let latest = commit(repo, "local work continued");
        let queue = Queue::load(repo, true, &remotes, &latest).unwrap();
        assert!(!paused(
            queue.destination("main", "origin").unwrap(),
            &latest,
            101,
            false
        ));
        assert!(paused(
            queue.destination("main", "gitlab").unwrap(),
            &latest,
            101,
            false
        ));
        git(
            repo,
            &["remote", "set-url", "--push", "gitlab", "./replacement.git"],
        );
        let queue = Queue::load(repo, true, &remotes, &latest).unwrap();
        assert!(!paused(
            queue.destination("main", "gitlab").unwrap(),
            &latest,
            101,
            false
        ));
        assert!(pending(repo));
        clear_retry(repo).unwrap();
    }
    async fn round(
        repo: &Path,
        target: PublicationTarget,
        remotes: &[crate::policy::RemoteConfig],
    ) -> PublicationPlan {
        let queue = Queue::load(repo, true, remotes, &target);
        let mut plan = PublicationPlan {
            target: Some(target),
            timeout_secs: 15,
            has_origin: true,
            origin_url: None,
            remotes: remotes.to_vec(),
            attached_branch: Some("main".to_owned()),
            queue,
            remote_failures: HashMap::new(),
        };
        let policy: SyncPolicy = toml::from_str("push_retries = 1").unwrap();
        let report = Box::pin(upload(repo, &policy, &mut plan, |_| None, &HashSet::new())).await;
        report.unwrap();
        plan
    }

    #[tokio::test]
    async fn healthy_destination_advances_then_failed_mirror_catches_up() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path();
        let github = repo.join("github.git");
        let gitlab = repo.join("gitlab.git");
        git(repo, &["init", "-b", "main"]);
        git(repo, &["config", "core.hooksPath", "/dev/null"]);
        git(repo, &["init", "--bare", github.to_str().unwrap()]);
        git(repo, &["remote", "add", "origin", github.to_str().unwrap()]);
        git(repo, &["remote", "add", "gitlab", gitlab.to_str().unwrap()]);
        let remotes = [toml::from_str("name = 'gitlab'\npush_url = './gitlab.git'").unwrap()];

        let first = commit(repo, "first");
        let first_round = Box::pin(round(repo, first.clone(), &remotes)).await;
        assert!(first_round.remote_failures.contains_key("gitlab"));
        assert_eq!(
            git(&github, &["rev-parse", "refs/heads/main"]),
            first.object_id()
        );
        drop(first_round);

        let latest = commit(repo, "local work during outage");
        git(repo, &["init", "--bare", gitlab.to_str().unwrap()]);
        let mut second = Box::pin(round(repo, latest.clone(), &remotes))
            .await
            .queue
            .unwrap();
        assert_eq!(
            git(&github, &["rev-parse", "refs/heads/main"]),
            latest.object_id()
        );
        assert!(git(&gitlab, &["for-each-ref", "refs/heads"]).is_empty());

        // Advance the persisted attempt time past the retry window.
        second
            .branches
            .get_mut("main")
            .unwrap()
            .get_mut("gitlab")
            .unwrap()
            .failure
            .as_mut()
            .unwrap()
            .last_attempt_unix -= crate::daemon::MIRROR_PAUSE_REPROBE_SECS;
        second.save().unwrap();
        drop(second);
        let recovered = Box::pin(round(repo, latest.clone(), &remotes)).await;
        assert!(recovered.remote_failures.is_empty());
        assert_eq!(
            git(&gitlab, &["rev-parse", "refs/heads/main"]),
            latest.object_id()
        );
        assert!(!pending(repo));
    }
}
