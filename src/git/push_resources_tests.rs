use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

#[tokio::test]
async fn mirror_commands_share_fleet_slots_and_cancellation_removes_a_waiter() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let repo = root.path();
    TEST_ADMISSION
        .scope(Arc::new(Semaphore::new(2)), async {
            sized_publication(
                true,
                background_pushes(async {
                    let hold = || command(repo, &["push"], std::future::pending::<Result<()>>());
                    let mut github = Box::pin(hold());
                    let mut gitlab = Box::pin(hold());
                    assert!(futures::poll!(&mut github).is_pending());
                    assert!(futures::poll!(&mut gitlab).is_pending());
                    assert_eq!(publication_admission().available_permits(), 0);
                    let mut queued = Box::pin(command(repo, &["push"], async { Ok(()) }));
                    assert!(futures::poll!(&mut queued).is_pending());
                    drop(queued);
                    assert_eq!(
                        crate::daemon::scheduler_evidence::publication_waiters_for_fixture(repo),
                        0
                    );

                    let mut next = Box::pin(command(repo, &["push"], async { Ok(()) }));
                    assert!(futures::poll!(&mut next).is_pending());
                    drop(github);
                    next.await.unwrap();
                    assert_eq!(publication_admission().available_permits(), 1);
                    drop(gitlab);
                }),
            )
            .await;
        })
        .await;
}

#[tokio::test]
async fn a_queued_heavy_push_leaves_one_slot_local_work_free() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let repo = root.path();
    let local = Arc::new(Semaphore::new(1));
    TEST_ADMISSION
        .scope(Arc::new(Semaphore::new(2)), async {
            let mut active = Box::pin(background_pushes(command(
                repo,
                &["push"],
                std::future::pending::<Result<()>>(),
            )));
            assert!(futures::poll!(&mut active).is_pending());
            let permit = local.clone().acquire_owned().await.unwrap();
            let waiting_repo = tempfile::tempdir().unwrap();
            std::fs::create_dir(waiting_repo.path().join(".git")).unwrap();
            let mut queued = Box::pin(with_local_work(
                local.clone(),
                permit,
                push_phase(command(waiting_repo.path(), &["push"], async { Ok(()) })),
            ));
            assert!(futures::poll!(&mut queued).is_pending());
            assert_eq!(
                crate::daemon::scheduler_evidence::publication_waiters_for_fixture(
                    waiting_repo.path()
                ),
                1
            );
            let unrelated_commit = local.clone().try_acquire_owned().unwrap();
            drop(active);
            // Publication ends, but the outer worker still needs local capacity.
            assert!(futures::poll!(&mut queued).is_pending());
            assert_eq!(publication_admission().available_permits(), 2);
            drop(unrelated_commit);
            queued.await.unwrap();
            assert_eq!(local.available_permits(), 1);
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
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let repo = root.path();
    let local = Arc::new(Semaphore::new(1));
    let permit = local.clone().acquire_owned().await.unwrap();
    with_local_work(local.clone(), permit, async {
        assert_eq!(local.available_permits(), 0);
        let result: Result<()> = push_phase(async {
            assert_eq!(local.available_permits(), 1);
            let (a, b) = tokio::join!(
                command(repo, &["push"], async { Ok(()) }),
                push_phase(command(repo, &["push"], async { Ok(()) })),
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

#[test]
fn repository_maintenance_is_isolated_like_pushes_and_hooks() {
    assert!(isolated_maintenance(&["gc", "--quiet"]));
    assert!(isolated_maintenance(&["repack", "-d", "-l", "-q"]));
    for other in [
        &["status"][..],
        &["commit", "-m", "gc"],
        &["count-objects", "-v"],
        &[],
    ] {
        assert!(!isolated_maintenance(other), "{other:?}");
    }
}

#[tokio::test]
async fn short_publication_can_use_one_extra_slot_when_memory_allows() {
    let primary = Arc::new(Semaphore::new(2));
    let extra = Arc::new(Semaphore::new(1));
    let long = acquire_publication(primary.clone(), 2, None, || true)
        .await
        .unwrap();
    let mut pressured = Box::pin(acquire_publication(
        primary.clone(),
        1,
        Some(&extra),
        || false,
    ));
    assert!(futures::poll!(&mut pressured).is_pending());
    drop(pressured);
    let short = acquire_publication(primary.clone(), 1, Some(&extra), || true)
        .await
        .unwrap();
    assert_eq!(extra.available_permits(), 0);
    let mut another = Box::pin(acquire_publication(
        primary.clone(),
        1,
        Some(&extra),
        || true,
    ));
    let mut heavy = Box::pin(acquire_publication(primary.clone(), 2, None, || true));
    assert!(futures::poll!(&mut another).is_pending());
    assert!(futures::poll!(&mut heavy).is_pending());
    drop(another);
    drop(short);
    drop(long);
    heavy.await.unwrap();
}

#[test]
fn seeded_warden_hook_is_ordinary_but_foreign_chain_is_heavy() {
    let root = tempfile::tempdir().unwrap();
    let hook = root.path().join("pre-push");
    assert!(ordinary_hook(&hook));
    std::fs::write(
        &hook,
        "#!/bin/sh\n# dracon-warden-managed-hook-v2\nDRACON_FOREIGN_HOOK=''\n",
    )
    .unwrap();
    assert!(ordinary_hook(&hook));
    std::fs::write(
        &hook,
        "#!/bin/sh\n# dracon-warden-managed-hook-v2\nDRACON_FOREIGN_HOOK=/hooks/foreign\n",
    )
    .unwrap();
    assert!(!ordinary_hook(&hook));
    std::fs::write(&hook, "#!/bin/sh\nnpm run test\n").unwrap();
    assert!(!ordinary_hook(&hook));
}
