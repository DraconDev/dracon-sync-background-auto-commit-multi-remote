//! Daemon-only push isolation. CLI/manual Git workloads retain their priority.
//!
//! Local work has its existing semaphore. A push phase temporarily gives its
//! permit back, so waiting on hook/network work cannot exhaust commit capacity.
//! Actual push commands have a separate one-command lane and low child priority.
use anyhow::{Context, Result};
use std::cell::RefCell;
use std::future::Future;
use std::sync::{Arc, OnceLock};
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

/// Per-repository coordinator maintenance has no local-work permit to release,
/// but its push children need the same bounded lane and low priority. Call this
/// only from an owned background job, never inline on the inspection loop.
pub(crate) async fn background_pushes<T>(work: impl Future<Output = T>) -> T {
    PUSH_PHASE.scope(true, work).await
}

fn isolated_push(args: &[&str]) -> bool {
    args.first() == Some(&"push") && PUSH_PHASE.try_with(|active| *active).unwrap_or(false)
}

fn push_slots() -> Arc<Semaphore> {
    static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SLOTS.get_or_init(|| Arc::new(Semaphore::new(1))).clone()
}

/// The permit lives outside the owned process runner: cancellation drops the
/// runner (which kills its process group) before another command gets a slot.
pub(super) async fn command<T>(args: &[&str], work: impl Future<Output = Result<T>>) -> Result<T> {
    let permit = if isolated_push(args) {
        Some(
            push_slots()
                .acquire_owned()
                .await
                .context("push semaphore closed")?,
        )
    } else {
        None
    };
    // Struct fields drop in declaration order, making teardown-before-slot-
    // release explicit rather than relying on async-local drop ordering.
    struct OwnedCommand<F: Future> {
        work: std::pin::Pin<Box<F>>,
        _permit: Option<OwnedSemaphorePermit>,
    }
    let mut owned = OwnedCommand {
        work: Box::pin(work),
        _permit: permit,
    };
    futures::future::poll_fn(|cx| owned.work.as_mut().poll(cx)).await
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
