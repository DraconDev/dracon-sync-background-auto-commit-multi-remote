use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

#[tokio::test]
async fn hook_command_lane_is_single_flight_and_drops_work_before_releasing_slot() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
    }
    let entered = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    PUSH_PHASE.scope(true, async {
        let signal = entered.clone();
        let cleanup = dropped.clone();
        let mut first = Box::pin(command(&["push"], async move {
            let _guard = Dropped(cleanup);
            signal.store(true, Ordering::SeqCst);
            std::future::pending::<()>().await;
            Ok(())
        }));
        assert!(matches!(futures::poll!(first.as_mut()), Poll::Pending));
        assert!(entered.load(Ordering::SeqCst));
        assert_eq!(push_slots().available_permits(), 0);
        let observed = dropped.clone();
        let mut second = Box::pin(command(&["push"], async move {
            assert!(observed.load(Ordering::SeqCst), "previous work must be dropped before admission");
            Ok(())
        }));
        assert!(matches!(futures::poll!(second.as_mut()), Poll::Pending));
        // Inspection and unenrolled/manual push are not customers of this
        // lane, even while an enrolled heavy hook owns its only slot.
        command(&["status"], async { Ok(()) }).await.unwrap();
        PUSH_PHASE.scope(false, command(&["push"], async { Ok(()) })).await.unwrap();
        drop(first);
        assert!(dropped.load(Ordering::SeqCst));
        second.await.unwrap();
        assert_eq!(push_slots().available_permits(), 1);
    }).await;
}

#[tokio::test]
async fn push_phase_restores_local_capacity_on_error_and_nested_mirror_work() {
    let local = Arc::new(Semaphore::new(1));
    let permit = local.clone().acquire_owned().await.unwrap();
    with_local_work(local.clone(), permit, async {
        assert_eq!(local.available_permits(), 0);
        let result: Result<()> = push_phase(async {
            assert_eq!(local.available_permits(), 1);
            let (a, b) = tokio::join!(
                command(&["push"], async { Ok(()) }),
                push_phase(command(&["push"], async { Ok(()) })),
            );
            a.unwrap(); b.unwrap();
            assert_eq!(local.available_permits(), 1, "mirror completion may not reacquire the local permit early");
            anyhow::bail!("injected hook failure")
        }).await;
        assert!(result.is_err());
        assert_eq!(local.available_permits(), 0, "local mutation must regain its permit even after error");
        assert_eq!(push_slots().available_permits(), 1);
    }).await;
    assert_eq!(local.available_permits(), 1);
}
