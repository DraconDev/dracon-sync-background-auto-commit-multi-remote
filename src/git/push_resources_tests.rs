use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

#[tokio::test]
async fn push_commands_overlap_and_cancellation_drops_only_owned_work() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let entered = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    PUSH_PHASE
        .scope(true, async {
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
            let second_entered = Arc::new(AtomicBool::new(false));
            let second_dropped = Arc::new(AtomicBool::new(false));
            let signal = second_entered.clone();
            let cleanup = second_dropped.clone();
            let mut second = Box::pin(command(&["push"], async move {
                let _guard = Dropped(cleanup);
                signal.store(true, Ordering::SeqCst);
                std::future::pending::<()>().await;
                Ok(())
            }));
            assert!(matches!(futures::poll!(second.as_mut()), Poll::Pending));
            assert!(second_entered.load(Ordering::SeqCst));
            assert!(!dropped.load(Ordering::SeqCst));
            // A third enrolled push starts while both earlier commands remain
            // blocked: no replacement process-wide push-count cap exists.
            command(&["push"], async { Ok(()) }).await.unwrap();
            // Inspection and unenrolled/manual push also remain independent.
            command(&["status"], async { Ok(()) }).await.unwrap();
            PUSH_PHASE
                .scope(false, command(&["push"], async { Ok(()) }))
                .await
                .unwrap();
            drop(first);
            assert!(dropped.load(Ordering::SeqCst));
            assert!(!second_dropped.load(Ordering::SeqCst));
            drop(second);
            assert!(second_dropped.load(Ordering::SeqCst));
        })
        .await;
}

#[tokio::test]
async fn local_refresh_is_bounded_and_leaves_publication_yielded() {
    let local = Arc::new(Semaphore::new(1));
    let permit = local.clone().acquire_owned().await.unwrap();
    with_local_work(local.clone(), permit, async {
        push_phase(async {
            assert_eq!(local.available_permits(), 1);
            local_phase(async {
                assert_eq!(local.available_permits(), 0);
                local_phase(async {
                    assert_eq!(local.available_permits(), 0);
                    Ok(())
                })
                .await
            })
            .await?;
            assert_eq!(
                local.available_permits(),
                1,
                "refresh must not retain local admission during upload"
            );
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(local.available_permits(), 0);
    })
    .await;
    assert_eq!(local.available_permits(), 1);
}

#[tokio::test]
async fn cancelled_local_refresh_drops_work_before_releasing_its_capacity() {
    struct ObserveDrop {
        semaphore: Arc<Semaphore>,
        dropped_while_admitted: Arc<AtomicBool>,
    }
    impl Drop for ObserveDrop {
        fn drop(&mut self) {
            self.dropped_while_admitted
                .store(self.semaphore.available_permits() == 0, Ordering::SeqCst);
        }
    }
    let local = Arc::new(Semaphore::new(1));
    let permit = local.clone().acquire_owned().await.unwrap();
    with_local_work(local.clone(), permit, async {
        push_phase(async {
            let observed = Arc::new(AtomicBool::new(false));
            let semaphore = local.clone();
            let dropped_while_admitted = observed.clone();
            let mut first = Box::pin(local_phase(async move {
                let _guard = ObserveDrop {
                    semaphore,
                    dropped_while_admitted,
                };
                std::future::pending::<()>().await;
                Ok(())
            }));
            assert!(matches!(futures::poll!(first.as_mut()), Poll::Pending));
            assert_eq!(local.available_permits(), 0);
            let mut second = Box::pin(local_phase(async { Ok(()) }));
            assert!(matches!(futures::poll!(second.as_mut()), Poll::Pending));
            drop(first);
            assert!(observed.load(Ordering::SeqCst));
            second.await?;
            assert_eq!(local.available_permits(), 1);
            Ok(())
        })
        .await
        .unwrap();
    })
    .await;
    assert_eq!(local.available_permits(), 1);
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
            a.unwrap();
            b.unwrap();
            assert_eq!(
                local.available_permits(),
                1,
                "mirror completion may not reacquire the local permit early"
            );
            anyhow::bail!("injected hook failure")
        })
        .await;
        assert!(result.is_err());
        assert_eq!(
            local.available_permits(),
            0,
            "local mutation must regain its permit even after error"
        );
    })
    .await;
    assert_eq!(local.available_permits(), 1);
}
