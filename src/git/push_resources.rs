//! Daemon-only push isolation. CLI/manual Git workloads retain their priority.
//!
//! Local work has its existing semaphore. A push phase temporarily gives its
//! permit back, so waiting on hook/network work cannot exhaust commit capacity.
//! Each daemon push command takes fleet-wide admission before starting Git.
//! Repository ownership remains enforced separately per checkout.
#[cfg(test)]
#[path = "push_resources_round_tests.rs"]
mod round_tests;
#[cfg(test)]
#[path = "push_resources_tests.rs"]
mod tests;

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::future::Future;
use std::io::Read;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

struct LocalWork {
    semaphore: Arc<Semaphore>,
    permit: RefCell<Option<OwnedSemaphorePermit>>,
}

/// Retained by the isolated unit across asynchronous cancellation cleanup.
pub(super) struct RunningPush {
    _fleet: OwnedSemaphorePermit,
    _repo: super::repo_maintenance::PushActivity,
}

tokio::task_local! {
    static LOCAL_WORK: LocalWork;
    static PUSH_PHASE: bool;
    static SMALL_PUBLICATION: bool;
    static POLICY_PATH: std::path::PathBuf;
    static RUNNING_PUSH: Arc<RunningPush>;
    #[cfg(test)]
    static TEST_ADMISSION: Arc<Semaphore>;
}

// Normal admission still reserves both permits for a heavy push. An extra
// short push can overlap it when the host can absorb another process.
static PUBLICATION_ADMISSION: OnceLock<Arc<Semaphore>> = OnceLock::new();
static SHORT_PUBLICATION: OnceLock<Arc<Semaphore>> = OnceLock::new();

async fn acquire_publication(
    primary: Arc<Semaphore>,
    weight: u32,
    overflow: Option<&Arc<Semaphore>>,
    can_overlap: impl Fn() -> bool,
) -> Result<OwnedSemaphorePermit> {
    let normal = primary.acquire_many_owned(weight);
    let Some(overflow) = overflow else {
        return normal
            .await
            .context("publication admission semaphore closed");
    };
    tokio::select! {
        biased;
        permit = normal => permit.context("publication admission semaphore closed"),
        permit = async {
            loop {
                if can_overlap() {
                    if let Ok(permit) = overflow.clone().try_acquire_owned() {
                        break permit;
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        } => Ok(permit),
    }
}

fn publication_admission() -> Arc<Semaphore> {
    #[cfg(test)]
    if let Ok(test) = TEST_ADMISSION.try_with(Arc::clone) {
        return test;
    }
    PUBLICATION_ADMISSION
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
}

pub(crate) async fn daemon_scope<T>(
    policy_path: std::path::PathBuf,
    work: impl Future<Output = T>,
) -> T {
    POLICY_PATH.scope(policy_path, work).await
}

pub(crate) fn daemon_maintenance_held() -> bool {
    POLICY_PATH
        .try_with(|_| crate::daemon::memory_admission::maintenance_held())
        .unwrap_or(false)
}

#[cfg(test)]
pub(crate) async fn with_test_publication_admission<T>(
    semaphore: Arc<Semaphore>,
    work: impl Future<Output = T>,
) -> T {
    TEST_ADMISSION.scope(semaphore, work).await
}

pub(crate) fn daemon_publication() -> bool {
    POLICY_PATH.try_with(|_| ()).is_ok()
}

/// Admit a destination before selecting its immutable source. The selected
/// source stays pinned across transport fallbacks within this admission.
pub(crate) async fn publication_destination<T>(
    repo: &std::path::Path,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    command(repo, &["push"], work).await
}

// Warden's installed wrapper can chain a repo-specific hook. Only its
// ordinary wrapper and seeded copies qualify for the short-push lane.
fn ordinary_hook(path: &std::path::Path) -> bool {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) => return error.kind() == std::io::ErrorKind::NotFound,
    };
    if file
        .metadata()
        .map_or(true, |meta| !meta.is_file() || meta.len() > 128 * 1024)
    {
        return false;
    }
    let mut source = String::new();
    if file
        .take(128 * 1024 + 1)
        .read_to_string(&mut source)
        .is_err()
    {
        return false;
    }
    source
        .lines()
        .any(|line| line == "# dracon-warden-managed-hook-v2")
        && source
            .lines()
            .filter(|line| line.starts_with("DRACON_FOREIGN_HOOK="))
            .eq(["DRACON_FOREIGN_HOOK=''"])
}

pub(crate) async fn ordinary_repository(repo: &std::path::Path) -> bool {
    let Ok(common) = super::sync_lease::repository_identity(repo) else {
        return false;
    };
    let probe = async {
        let mut child = super::spawn_git_command_cancellable_stdin(
            repo,
            &["config", "--path", "--get", "core.hooksPath"],
            "publication hooks path",
        )?;
        drop(child.stdin.take());
        let (status, output, _) =
            super::run_git_captured_output(child, repo, "publication hooks path").await?;
        let local = common.join("hooks/pre-push");
        match status.code() {
            Some(0) => {
                let path = std::str::from_utf8(&output)?.trim_end_matches('\n');
                anyhow::ensure!(
                    !path.is_empty() && !path.contains('\n'),
                    "invalid hooks path"
                );
                let effective = repo.join(path).join("pre-push");
                Ok(ordinary_hook(&effective) && ordinary_hook(&local))
            }
            Some(1) => Ok(ordinary_hook(&local)),
            _ => anyhow::bail!("cannot resolve hooks path"),
        }
    };
    matches!(
        tokio::time::timeout(Duration::from_secs(3), probe).await,
        Ok(Ok(true))
    )
}

pub(crate) async fn sized_publication<T>(small: bool, work: impl Future<Output = T>) -> T {
    SMALL_PUBLICATION.scope(small, work).await
}

pub(crate) async fn with_local_work<T>(
    semaphore: Arc<Semaphore>,
    permit: OwnedSemaphorePermit,
    work: impl Future<Output = T>,
) -> T {
    LOCAL_WORK
        .scope(
            LocalWork {
                semaphore,
                permit: RefCell::new(Some(permit)),
            },
            work,
        )
        .await
}

/// This boundary encloses the entire push phase, including concurrent mirror
/// futures. Individual mirror commands must not independently restore the
/// shared local-work permit while another mirror is still in its hook.
/// Cancellation drops the enclosing sync worker and all its owned resources.
pub(crate) async fn push_phase<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    if PUSH_PHASE.try_with(|active| *active).unwrap_or(false) {
        return work.await;
    }
    let local = LOCAL_WORK.try_with(|local| {
        let semaphore = local.semaphore.clone();
        let permit = local.permit.borrow_mut().take();
        (semaphore, permit)
    });
    let Ok((semaphore, permit)) = local else {
        return work.await;
    };
    anyhow::ensure!(
        permit.is_some(),
        "nested push phase would violate local-work ownership"
    );
    drop(permit);
    let result = PUSH_PHASE.scope(true, work).await;
    // Restore local execution ownership before any subsequent mutation. This
    // can wait behind healthy local workers, without holding a push-command
    // slot or changing the repository's exclusive scheduler reservation.
    let permit = semaphore
        .acquire_owned()
        .await
        .context("local-work semaphore closed")?;
    LOCAL_WORK.with(|local| {
        *local.permit.borrow_mut() = Some(permit);
    });
    result
}

/// Coordinate one upload with optional local refreshes. Delay and refresh
/// factories keep the completion/admission ordering directly testable.
pub(crate) async fn publication_round<T, P, R, W>(
    publication: std::pin::Pin<Box<P>>,
    mut refresh_work: impl FnMut() -> R,
    mut wait: impl FnMut(bool) -> W,
) -> Result<T>
where
    P: Future<Output = Result<T>>,
    R: Future<Output = bool>,
    W: Future<Output = ()>,
{
    // Upload completion must not restore the outer permit before a queued
    // refresh drains: that refresh may already be waiting on the same FIFO
    // semaphore. Keep the whole coordination/drain inside the yielded scope.
    push_phase(async move {
        // Box at the CALL boundary as well: storing a large upload future
        // inline in this async function can overflow debug worker stacks
        // before the first poll ever reaches an internal Box::pin.
        let mut publication = publication;
        let mut refresh: Option<std::pin::Pin<Box<R>>> = None;
        let mut active = true;
        let report = loop {
            tokio::select! {
                result = &mut publication => break result,
                result = async { refresh.as_mut().expect("guarded refresh").await }, if refresh.is_some() => {
                    active = result;
                    refresh = None;
                }
                () = wait(active), if refresh.is_none() => {
                    refresh = Some(Box::pin(refresh_work()));
                }
            }
        };
        drop(publication);
        if let Some(pending) = refresh {
            pending.await;
        }
        report
    })
    .await
}

/// Admit a fresh local pass while the enclosing actor has yielded its local
/// permit for publication. The nested scope owns only this pass's permit;
/// finishing it must leave the enclosing publication scope yielded. Nested
/// local helpers reuse admission rather than deadlocking a one-permit budget.
/// This is capacity ownership only: the pass must also acquire its repository
/// mutation lease before touching local state.
pub(crate) async fn local_phase<T>(work: impl Future<Output = Result<T>>) -> Result<T> {
    let admission =
        LOCAL_WORK.try_with(|local| (local.semaphore.clone(), local.permit.borrow().is_some()));
    let Ok((semaphore, admitted)) = admission else {
        // Interactive/one-shot work is not enrolled in daemon admission.
        return work.await;
    };
    if admitted {
        return work.await;
    }
    let permit = semaphore
        .clone()
        .acquire_owned()
        .await
        .context("local-work semaphore closed")?;
    with_local_work(semaphore, permit, work).await
}

/// Per-repository coordinator maintenance has no local-work permit to release,
/// but its push children need the same low priority. Call this
/// only from an owned background job, never inline on the inspection loop.
pub(crate) async fn background_pushes<T>(work: impl Future<Output = T>) -> T {
    PUSH_PHASE.scope(true, work).await
}

pub(super) fn isolated_push(args: &[&str]) -> bool {
    args.first() == Some(&"push") && PUSH_PHASE.try_with(|active| *active).unwrap_or(false)
}

/// `git hook run …`: how the daemon runs a repository's COMMIT hooks around a
/// sealed commit (`sync::run_configured_hook`). ADDED 2026-11-13: after push
/// isolation shipped, dracon-platform's pre-commit bucket guard (`node
/// bucket-strategy.mjs`, ~360 MB) still ran in the daemon's cgroup; with the
/// daemon itself and its git children that pinned the unit at MemoryHigh, and
/// systemd-oomd killed the whole daemon 7 times in 15 minutes for sustained
/// pressure. Repository-supplied code never shares the daemon's cgroup.
pub(super) fn isolated_hook(args: &[&str]) -> bool {
    args.len() >= 2 && args[0] == "hook" && args[1] == "run"
}

/// `git gc` / `git repack`: the daemon's auto-gc (`maybe_auto_gc`). ADDED
/// 2026-11-14: a full gc of a 7.6 GiB repository ran `pack-objects` at
/// 860 MiB inside the daemon's 1 GiB cgroup, pinning the unit at MemoryHigh
/// for the whole 10-minute attempt. Repository maintenance is sized by the
/// repository, not by the daemon, so it gets the isolated slice's budget.
pub(super) fn isolated_maintenance(args: &[&str]) -> bool {
    matches!(args.first(), Some(&"gc") | Some(&"repack"))
}

/// Keep the command future in its caller's ownership. Dropping the caller
/// drops its process runner and kills that runner's owned process group; it
/// must neither detach publication nor serialize unrelated repositories.
///
/// A push also registers itself for its whole lifetime, hook included, so the
/// daemon's auto-gc cannot repack the repository underneath it (see
/// `repo_maintenance`); a push arriving during a repack waits for it.
pub(super) async fn command<T>(
    repo: &std::path::Path,
    args: &[&str],
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    if args.first() != Some(&"push") || running_push().is_some() {
        return work.await;
    }
    if !isolated_push(args) {
        let _push = super::repo_maintenance::begin_push(repo).await?;
        return work.await;
    }

    let queued = crate::daemon::scheduler_evidence::publication_queued(repo);
    let small = SMALL_PUBLICATION.try_with(|small| *small).unwrap_or(false);
    let weight = if small { 1 } else { 2 };
    let held = || {
        POLICY_PATH
            .try_with(|path| {
                crate::policy::freeze_reason(path).is_some()
                    || crate::daemon::memory_admission::held()
            })
            .unwrap_or(false)
    };
    let running = loop {
        while held() {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        // Wait for pending repack before taking any fleet capacity.
        let push = super::repo_maintenance::begin_push(repo).await?;
        let overflow = (small && (daemon_publication() || cfg!(test)))
            .then(|| SHORT_PUBLICATION.get_or_init(|| Arc::new(Semaphore::new(1))));
        let admission = acquire_publication(publication_admission(), weight, overflow, || {
            !held()
                && (cfg!(test) || crate::daemon::memory_admission::short_push_headroom())
        })
        .await?;
        // Pressure or a manual pause may have arrived while this command
        // waited behind another publication.
        if !held() {
            break RunningPush {
                _fleet: admission,
                _repo: push,
            };
        }
    };
    let _running = queued.started();
    let running = Arc::new(running);
    let result = RUNNING_PUSH.scope(running.clone(), work).await;
    if POLICY_PATH.try_with(|_| ()).is_ok()
        && result
            .as_ref()
            .is_err_and(|error| super::is_push_unit_resource_kill(&error.to_string()))
    {
        crate::daemon::memory_admission::resource_kill();
    }
    result
}

pub(super) fn running_push() -> Option<Arc<RunningPush>> {
    RUNNING_PUSH.try_with(Arc::clone).ok()
}

/// Apply only to the child of an enrolled daemon push. Setting our own child
/// to nice 19 is unprivileged, cannot improve its priority, and is inherited by
/// hook descendants. The daemon and interactive Git processes are untouched.
/// This is the FALLBACK path: when a systemd user manager is reachable the
/// push runs in its own transient unit instead (`push_unit`, 2026-11-13) and
/// gets `Nice=19` there. No memory limit is set by the daemon on either path.
pub(super) fn configure_child(cmd: &mut tokio::process::Command, args: &[&str]) {
    #[cfg(unix)]
    if isolated_push(args) || isolated_maintenance(args) {
        // SAFETY: pre_exec uses only an async-signal-safe syscall and creates
        // an OS error on failure; no allocation, locks or runtime access.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpriority(libc::PRIO_PROCESS, 0, 19) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    let _ = (cmd, args);
}
