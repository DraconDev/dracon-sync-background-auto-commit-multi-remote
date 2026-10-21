//! Scheduler-owned recovery state. Cancellation is a request, never a release.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct SyncRecovery {
    running: HashMap<PathBuf, Arc<OnceLock<Instant>>>,
    budgets: HashMap<PathBuf, Duration>,
    cancelling: HashSet<PathBuf>,
    failures: HashMap<PathBuf, usize>,
    retry_after: HashMap<PathBuf, Instant>,
}

impl SyncRecovery {
    /// Copy-only view: queued reservations are not execution, and a cancel
    /// request does not release ownership. Never mutates recovery state.
    pub(super) fn observations(
        &self,
        now: Instant,
    ) -> std::collections::BTreeMap<PathBuf, super::scheduler_evidence::RepoEvidence> {
        use super::scheduler_evidence::{Phase, RepoEvidence};
        let mut result = std::collections::BTreeMap::new();
        for (repo, until) in &self.retry_after {
            if *until > now {
                result.insert(
                    repo.clone(),
                    RepoEvidence {
                        phase: Some(Phase::Retrying),
                        retry_secs: Some(until.duration_since(now).as_secs().max(1)),
                        ..Default::default()
                    },
                );
            }
        }
        for (repo, started) in &self.running {
            let phase = if self.cancelling.contains(repo) {
                Phase::Cancelling
            } else if started.get().is_some() {
                Phase::Working
            } else {
                Phase::Queued
            };
            result.insert(
                repo.clone(),
                RepoEvidence {
                    phase: Some(phase),
                    execution_secs: started
                        .get()
                        .map(|at| now.saturating_duration_since(*at).as_secs()),
                    ..Default::default()
                },
            );
        }
        result
    }

    /// Called before spawn. The worker stamps this only after acquiring a
    /// semaphore permit: queued workers retain FIFO position, without a false
    /// execution timeout. Admission remains protected by `in_flight`.
    pub(super) fn reserve(&mut self, repo: &Path) -> Arc<OnceLock<Instant>> {
        let started = Arc::new(OnceLock::new());
        self.running.insert(repo.to_path_buf(), started.clone());
        started
    }

    pub(super) fn set_budget(&mut self, repo: &Path, budget: Duration) {
        self.budgets.insert(repo.to_path_buf(), budget);
    }

    pub(super) fn ready(&self, repo: &Path, now: Instant) -> bool {
        self.retry_after.get(repo).is_none_or(|until| now >= *until)
    }

    /// Returns each expired execution once. Keep both ownership and the
    /// permit until the actual worker joins; never kill by PID/name or unlink
    /// a Git lock. A non-yielding task cannot be safely force-replaced.
    pub(super) fn expired(&mut self, now: Instant, budget: Duration) -> Vec<PathBuf> {
        self.running
            .iter()
            .filter_map(|(repo, started)| {
                let started = started.get()?;
                let allowed = self.budgets.get(repo).copied().unwrap_or(budget);
                if now.saturating_duration_since(*started) >= allowed
                    && self.cancelling.insert(repo.clone())
                {
                    Some(repo.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Only a joined worker can finish. Errors (including panics/cancellation)
    /// yield capacity with capped backoff; success resets the failure streak.
    pub(super) fn finish(&mut self, repo: &Path, failed: bool, now: Instant) {
        self.budgets.remove(repo);
        self.running.remove(repo);
        self.cancelling.remove(repo);
        if failed {
            let count = self.failures.entry(repo.to_path_buf()).or_default();
            *count = count.saturating_add(1);
            self.retry_after.insert(
                repo.to_path_buf(),
                now + super::classification_backoff_secs(*count),
            );
        } else {
            self.failures.remove(repo);
            self.retry_after.remove(repo);
        }
    }
}

/// Oldest admitted probe first, with never-admitted repos ahead of repeat
/// customers. Stable sorting preserves the existing deadline/result hints for
/// equal admission stamps. This ordering applies across priority tiers so an
/// always-dirty early repo cannot monopolize a bounded probe pool.
pub(super) fn fair_probe_order(repos: &mut [PathBuf], admitted: &HashMap<PathBuf, Instant>) {
    repos.sort_by_key(|repo| admitted.get(repo).copied());
}

/// A total-operation backstop must not undercut the independently bounded
/// push commands it supervises. Include one possible new commit, every target,
/// configured retries and the process runner's progress hard limit. Local-only
/// work retains the normal short backstop; arithmetic never wraps.
pub(super) fn execution_budget(
    policy: &crate::policy::SyncPolicy,
    ahead: u64,
    has_origin: bool,
    commit_only: bool,
) -> Duration {
    let local = policy.repo_sync_timeout_secs.max(900);
    if !policy.auto_push || commit_only {
        return Duration::from_secs(local);
    }
    let targets = (policy.remotes.len() as u64).saturating_add(u64::from(has_origin));
    let idle =
        crate::sync::scale_push_timeout(policy.push_op_timeout_secs, ahead.saturating_add(1));
    let push = crate::git::progress_hard_timeout_secs(idle)
        .saturating_mul(u64::from(policy.push_retries.max(1)))
        .saturating_mul(targets);
    Duration::from_secs(local.saturating_add(push))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_budget_respects_scaled_multi_remote_commands_and_still_expires() {
        let mut policy = crate::policy::SyncPolicy::default();
        policy.auto_push = true;
        policy.push_op_timeout_secs = 900;
        policy.repo_sync_timeout_secs = 960;
        policy.push_retries = 1;
        let budget = execution_budget(&policy, 7, true, false);
        assert!(budget > Duration::from_secs(1800));
        assert_eq!(
            execution_budget(&policy, 7, true, true),
            Duration::from_secs(960)
        );
        let now = Instant::now();
        let repo = Path::new("long-guarded-push");
        let mut recovery = SyncRecovery::default();
        recovery.reserve(repo).set(now).unwrap();
        recovery.set_budget(repo, budget);
        assert!(recovery
            .expired(now + Duration::from_secs(900), Duration::from_secs(900))
            .is_empty());
        assert_eq!(
            recovery.expired(now + budget, Duration::from_secs(900)),
            vec![repo.to_path_buf()]
        );
        recovery.finish(repo, false, now + budget);
        assert!(!recovery.budgets.contains_key(repo));
        policy.push_op_timeout_secs = u64::MAX;
        assert_eq!(
            execution_budget(&policy, u64::MAX, true, false).as_secs(),
            u64::MAX
        );
    }

    #[test]
    fn queued_workers_keep_fifo_and_execution_retries_are_bounded() {
        let now = Instant::now();
        let budget = Duration::from_secs(900);
        let a = Path::new("hung");
        let b = Path::new("healthy-waiter");
        let mut recovery = SyncRecovery::default();
        let running = recovery.reserve(a);
        let queued = recovery.reserve(b);
        running.set(now).unwrap();
        assert_eq!(
            recovery.expired(now + budget, budget),
            vec![a.to_path_buf()]
        );
        assert!(recovery.expired(now + budget * 2, budget).is_empty());
        assert!(
            recovery.running.contains_key(a),
            "abort request retains ownership"
        );
        assert!(queued.get().is_none(), "queue wait is not execution time");
        recovery.finish(a, true, now + budget);
        assert!(!recovery.ready(a, now + budget));
        assert!(recovery.ready(a, now + budget + Duration::from_secs(1)));
        queued.set(now + budget).unwrap();
        assert!(recovery.expired(now + budget, budget).is_empty());
        for n in 2..=100 {
            recovery.reserve(a);
            recovery.finish(a, true, now);
            let backoff = super::super::classification_backoff_secs(n);
            assert!(backoff <= Duration::from_secs(300));
            assert!(!recovery.ready(a, now + backoff - Duration::from_nanos(1)));
            assert!(recovery.ready(a, now + backoff));
        }
        recovery.finish(a, false, now);
        assert!(recovery.ready(a, now));
        assert!(!recovery.failures.contains_key(a));
    }

    #[test]
    fn fair_admission_serves_every_repo_despite_repeated_early_failures() {
        let now = Instant::now();
        let repos: Vec<_> = (0..12)
            .map(|n| PathBuf::from(format!("repo-{n}")))
            .collect();
        let mut admitted = HashMap::new();
        let mut seen = HashSet::new();
        // Adversarial fixed priority order on every pulse; four slots, with
        // the early four always dirty/failing. Unseen peers still get turns.
        for pulse in 0..3 {
            let mut order = repos.clone();
            fair_probe_order(&mut order, &admitted);
            for repo in order.into_iter().take(4) {
                seen.insert(repo.clone());
                admitted.insert(repo, now + Duration::from_secs(pulse));
            }
        }
        assert_eq!(seen.len(), repos.len());
    }
}
