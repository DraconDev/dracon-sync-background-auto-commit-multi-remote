//! Daemon-only push isolation. CLI/manual Git workloads retain their priority.
//!
//! Local work has its existing semaphore. A push phase temporarily gives its
//! permit back, so waiting on hook/network work cannot exhaust commit capacity.
//! Push commands retain owned cancellation and low child priority, but have no
//! process-global admission gate. Repository ownership is enforced separately.
#[cfg(test)]
#[path = "push_resources_round_tests.rs"]
mod round_tests;
#[cfg(test)]
#[path = "push_resources_tests.rs"]
mod tests;

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

struct LocalWork {
    semaphore: Arc<Semaphore>,
    permit: RefCell<Option<OwnedSemaphorePermit>>,
}

tokio::task_local! {
    static LOCAL_WORK: LocalWork;
    static PUSH_PHASE: bool;
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

fn isolated_push(args: &[&str]) -> bool {
    args.first() == Some(&"push") && PUSH_PHASE.try_with(|active| *active).unwrap_or(false)
}

/// Keep the command future in its caller's ownership. Dropping the caller
/// drops its process runner and kills that runner's owned process group; it
/// must neither detach publication nor serialize unrelated repositories.
pub(super) async fn command<T>(_args: &[&str], work: impl Future<Output = Result<T>>) -> Result<T> {
    work.await
}

/// Apply only to the child of an enrolled daemon push. Setting our own child
/// to nice 19 is unprivileged, cannot improve its priority, and is inherited by
/// hook descendants. The daemon and interactive Git processes are untouched.
/// No memory limit, service-wide setting or external command wrapper is used.
pub(super) fn configure_child(cmd: &mut tokio::process::Command, args: &[&str]) {
    #[cfg(unix)]
    if isolated_push(args) {
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
