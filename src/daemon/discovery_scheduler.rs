//! Single-flight discovery outside the fleet scheduling thread.
//!
//! Slow filesystems may make a blocking tree walk uncancellable. Keep at most
//! one such walk alive; never timeout/respawn it into a blocking-thread leak.
//! While it runs, schedule from the last complete, same-configuration snapshot.

use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Scope {
    pub roots: Vec<PathBuf>,
    pub excluded_dirs: BTreeSet<String>,
    pub excluded_repos: Vec<String>,
    pub system_repo: String,
}

pub(super) struct Discovery {
    pending: Option<tokio::task::JoinHandle<(Scope, Vec<PathBuf>)>>,
    snapshot: Option<(Scope, Vec<PathBuf>)>,
    last_attempt: Option<Scope>,
    next_scan: Instant,
    started: Instant,
    warned: bool,
}

impl Discovery {
    pub fn new() -> Self {
        Self {
            pending: None,
            snapshot: None,
            last_attempt: None,
            next_scan: Instant::now(),
            started: Instant::now(),
            warned: false,
        }
    }

    pub async fn poll(&mut self, scope: Scope) -> Option<Vec<PathBuf>> {
        self.poll_with(scope, |scope| {
            crate::git::discover_git_repos(
                &scope.roots,
                &scope.excluded_dirs,
                &scope.excluded_repos,
                Some(&scope.system_repo),
            )
        })
        .await
    }

    async fn poll_with<F>(&mut self, scope: Scope, discover: F) -> Option<Vec<PathBuf>>
    where
        F: FnOnce(&Scope) -> Vec<PathBuf> + Send + 'static,
    {
        let now = Instant::now();
        if self.pending.as_ref().is_some_and(|job| job.is_finished()) {
            let job = self.pending.take().expect("finished discovery task");
            match job.await {
                Ok((completed_scope, repos)) if completed_scope == scope => {
                    self.snapshot = Some((completed_scope, repos));
                    self.next_scan = now + Duration::from_secs(30);
                }
                Ok(_) => {} // Configuration changed during this walk: do not adopt it.
                Err(error) => {
                    eprintln!(
                        "⚠️ repository discovery failed: {error}; retaining previous snapshot"
                    );
                    self.next_scan = now + Duration::from_secs(30);
                }
            }
        }
        let same_scope = self.snapshot.as_ref().is_some_and(|(old, _)| *old == scope);
        if self.pending.is_none()
            && (now >= self.next_scan || self.last_attempt.as_ref() != Some(&scope))
        {
            self.last_attempt = Some(scope.clone());
            self.started = now;
            self.warned = false;
            self.pending = Some(tokio::task::spawn_blocking(move || {
                let repos = discover(&scope);
                (scope, repos)
            }));
        } else if self.pending.is_some()
            && !self.warned
            && now.duration_since(self.started) >= Duration::from_secs(60)
        {
            self.warned = true;
            eprintln!("⚠️ repository discovery exceeds 60s; no duplicate walk started; retaining last matching snapshot");
        }
        // Never feed a partially collected set (or a previous configuration)
        // into the daemon's retain/prune operations.
        if same_scope {
            self.snapshot.as_ref().map(|(_, repos)| repos.clone())
        } else {
            None
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "failure_recovery_tests.rs"]
mod failure_recovery_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope {
            roots: vec!["/watch".into()],
            excluded_dirs: BTreeSet::new(),
            excluded_repos: vec![],
            system_repo: String::new(),
        }
    }

    #[tokio::test]
    async fn blocked_refresh_keeps_snapshot_without_blocking_or_duplicate_walks() {
        let mut discovery = Discovery::new();
        discovery.snapshot = Some((scope(), vec!["/watch/healthy".into()]));
        let (release, wait) = std::sync::mpsc::channel();
        let snapshot = discovery
            .poll_with(scope(), move |_| {
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                vec!["/watch/new".into()]
            })
            .await
            .unwrap();
        assert_eq!(snapshot, vec![PathBuf::from("/watch/healthy")]);
        for _ in 0..10 {
            let result = tokio::time::timeout(
                Duration::from_millis(100),
                discovery.poll_with(scope(), |_| panic!("duplicate discovery")),
            )
            .await
            .unwrap();
            assert_eq!(result.unwrap(), snapshot);
        }
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !discovery.pending.as_ref().unwrap().is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let refreshed = discovery
            .poll_with(scope(), |_| panic!("unexpected rescan"))
            .await
            .unwrap();
        assert_eq!(refreshed, vec![PathBuf::from("/watch/new")]);
    }

    #[tokio::test]
    async fn changed_configuration_never_uses_old_snapshot() {
        let mut discovery = Discovery::new();
        discovery.snapshot = Some((scope(), vec!["/watch/old".into()]));
        let mut changed = scope();
        changed.roots = vec!["/other".into()];
        assert!(discovery
            .poll_with(changed.clone(), |_| vec!["/other/new".into()])
            .await
            .is_none());
        tokio::time::timeout(Duration::from_secs(3), async {
            while !discovery.pending.as_ref().unwrap().is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            discovery
                .poll_with(changed, |_| panic!("unexpected rescan"))
                .await
                .unwrap(),
            vec![PathBuf::from("/other/new")]
        );
    }
}
