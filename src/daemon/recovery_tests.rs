use super::*;

async fn finished(handle: &tokio::task::AbortHandle) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !handle.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("test worker must settle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_status_waits_for_teardown_and_healthy_peer_progresses() {
    struct DropSignal(Arc<AtomicBool>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let now = Instant::now();
    let hung = PathBuf::from("hung-status");
    let healthy = PathBuf::from("healthy-status");
    let dropped = Arc::new(AtomicBool::new(false));
    let signal = dropped.clone();
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let path = hung.clone();
    let handle: StatusJoin = tokio::spawn(async move {
        let _guard = DropSignal(signal);
        entered.send(()).unwrap();
        // Deliberately resist async cancellation for a bounded interval.
        // The test controls release; even an assertion failure cannot leak it.
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        std::future::pending::<()>().await;
        (path, Ok(dracon_git::types::RepoStatus::new()))
    });
    let abort = handle.abort_handle();
    entered_rx.await.unwrap();
    let mut jobs = FuturesUnordered::new();
    jobs.push(handle);
    let path = healthy.clone();
    let fast: StatusJoin =
        tokio::spawn(async move { (path, Ok(dracon_git::types::RepoStatus::new())) });
    let fast_abort = fast.abort_handle();
    finished(&fast_abort).await;
    jobs.push(fast);
    let mut pending = HashSet::from([hung.clone(), healthy.clone()]);
    let mut aborts = HashMap::from([(hung.clone(), abort.clone().into()), (healthy.clone(), fast_abort.into())]);
    let mut since = HashMap::from([
        (hung.clone(), now - Duration::from_secs(61)),
        (healthy.clone(), now),
    ]);
    let mut results = HashMap::new();
    collect_ready_status(
        &mut jobs,
        &mut pending,
        &mut aborts,
        &mut since,
        &mut results,
    );
    assert!(results.remove(&healthy).unwrap().0.is_ok());
    assert!(!pending.contains(&healthy));
    assert!(abort_wedged_status_inspection(
        &mut pending,
        &mut since,
        &mut aborts,
        &hung,
        now
    ));
    assert!(pending.contains(&hung));
    assert!(!dropped.load(Ordering::SeqCst));
    collect_ready_status(
        &mut jobs,
        &mut pending,
        &mut aborts,
        &mut since,
        &mut results,
    );
    assert!(
        pending.contains(&hung),
        "a retry may not overlap unacknowledged abort"
    );
    assert!(!abort_wedged_status_inspection(
        &mut pending,
        &mut since,
        &mut aborts,
        &hung,
        now
    ));
    release.send(()).unwrap();
    finished(&abort).await;
    // is_finished can become visible before FuturesUnordered's ready-queue
    // notification is consumed. Exercise repeated nonblocking pulse drains,
    // rather than assuming a single poll must observe that notification.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !jobs.is_empty() {
            collect_ready_status(
                &mut jobs,
                &mut pending,
                &mut aborts,
                &mut since,
                &mut results,
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert!(pending.is_empty());
    assert!(aborts.is_empty());
    assert!(jobs.is_empty());
    assert!(results
        .remove(&hung)
        .unwrap()
        .0
        .unwrap_err()
        .downcast_ref::<tokio::task::JoinError>()
        .unwrap()
        .is_cancelled());
}

#[tokio::test]
async fn recovery_classification_panic_and_cancellation_release_known_owners() {
    let now = Instant::now();
    let panic_repo = PathBuf::from("panic-classifier");
    let cancelled_repo = PathBuf::from("cancelled-classifier");
    let mut jobs = FuturesUnordered::new();
    let panic_task: ClassificationJoin =
        tokio::spawn(async { panic!("injected classifier panic") });
    let panic_abort = panic_task.abort_handle();
    let cancelled: ClassificationJoin = tokio::spawn(std::future::pending());
    let cancel_abort = cancelled.abort_handle();
    let mut owners = HashMap::from([
        (panic_repo.clone(), panic_abort.clone().into()),
        (cancelled_repo.clone(), cancel_abort.clone().into()),
    ]);
    let mut pending = HashSet::from([panic_repo.clone(), cancelled_repo.clone()]);
    let mut since = HashMap::from([
        (panic_repo.clone(), (now, Duration::from_secs(30))),
        (
            cancelled_repo.clone(),
            (now - Duration::from_secs(91), Duration::from_secs(30)),
        ),
    ]);
    jobs.push(panic_task);
    jobs.push(cancelled);
    assert!(abort_wedged_classification(
        &mut pending,
        &mut since,
        &mut owners,
        &cancelled_repo,
        now
    ));
    assert!(pending.contains(&cancelled_repo));
    assert!(!abort_wedged_classification(
        &mut pending,
        &mut since,
        &mut owners,
        &cancelled_repo,
        now
    ));
    finished(&panic_abort).await;
    finished(&cancel_abort).await;
    let mut results = HashMap::new();
    collect_ready_classifications(
        &mut jobs,
        &mut pending,
        &mut owners,
        &mut since,
        &mut results,
        &mut HashMap::new(),
    );
    assert!(pending.is_empty());
    assert!(owners.is_empty());
    assert!(since.is_empty());
    assert!(results
        .remove(&panic_repo)
        .unwrap()
        .0
        .unwrap_err()
        .downcast_ref::<tokio::task::JoinError>()
        .unwrap()
        .is_panic());
    assert!(results
        .remove(&cancelled_repo)
        .unwrap()
        .0
        .unwrap_err()
        .downcast_ref::<tokio::task::JoinError>()
        .unwrap()
        .is_cancelled());
}

#[test]
fn recovery_abandoned_reservations_release_without_touching_live_owners() {
    let repo = PathBuf::from("abandoned");
    let now = Instant::now();
    let mut pending = HashSet::from([repo.clone()]);
    let mut status_since = HashMap::new(); // missing metadata as well as owner
    let mut owners = HashMap::new();
    assert!(abort_wedged_status_inspection(
        &mut pending,
        &mut status_since,
        &mut owners,
        &repo,
        now
    ));
    assert!(pending.is_empty());
    pending.insert(repo.clone());
    let mut classifier_since = HashMap::new();
    assert!(abort_wedged_classification(
        &mut pending,
        &mut classifier_since,
        &mut owners,
        &repo,
        now
    ));
    assert!(pending.is_empty());
}

#[tokio::test]
async fn recovery_tagged_sync_join_releases_all_bookkeeping_on_panic_in_quiet_cycle() {
    let repo = PathBuf::from("panicked-sync");
    let now = Instant::now();
    let handle: SyncTaskJoin = tokio::spawn(async { panic!("injected sync panic") });
    let abort = handle.abort_handle();
    let mut in_flight = HashSet::from([repo.clone()]);
    let mut workers = HashMap::from([(repo.clone(), abort.clone())]);
    let mut cycle = HashSet::from([repo.clone()]);
    let mut detached_since = HashMap::from([(repo.clone(), now)]);
    let mut recovery = recovery::SyncRecovery::default();
    recovery.reserve(&repo);
    let mut detached = FuturesUnordered::new();
    detached.push(tagged_sync_join(repo.clone(), handle));
    finished(&abort).await;
    // The exact ready-only collector used by quiet production cycles.
    let (path, _, result) = next_ready_result(&mut detached).await.unwrap();
    assert_eq!(path, repo);
    assert!(result
        .as_ref()
        .unwrap_err()
        .downcast_ref::<tokio::task::JoinError>()
        .unwrap()
        .is_panic());
    release_sync_owner(
        &path,
        &mut in_flight,
        &mut workers,
        &mut cycle,
        &mut detached_since,
    );
    recovery.finish(&path, result.is_err(), now);
    assert!(
        in_flight.is_empty() && workers.is_empty() && cycle.is_empty() && detached_since.is_empty()
    );
    assert!(detached.is_empty());
    assert!(!recovery.ready(&repo, now));
    assert!(recovery.ready(&repo, now + Duration::from_secs(1)));
    // A later worker has its own fresh execution age, not a phantom detach age.
    recovery
        .reserve(&repo)
        .set(now + Duration::from_secs(901))
        .unwrap();
    assert!(recovery
        .expired(now + Duration::from_secs(901), Duration::from_secs(900))
        .is_empty());
}

#[tokio::test]
async fn recovery_fifo_healthy_syncs_run_after_hung_holder_joins() {
    let now = Instant::now();
    let sem = Arc::new(tokio::sync::Semaphore::new(1));
    let mut state = recovery::SyncRecovery::default();
    let a = PathBuf::from("hung");
    let b = PathBuf::from("healthy-b");
    let c = PathBuf::from("healthy-c");
    let mut in_flight = HashSet::new();
    let mut workers = HashMap::new();
    let mut detached = FuturesUnordered::new();
    let (events, mut rx) = tokio::sync::mpsc::unbounded_channel();
    for repo in [&a, &b, &c] {
        assert!(reserve_sync(&mut in_flight, repo));
        let start = state.reserve(repo);
        let sem = sem.clone();
        let path = repo.clone();
        let events = events.clone();
        let (queued, queued_rx) = tokio::sync::oneshot::channel();
        let is_hung = repo == &a;
        let handle: SyncTaskJoin = tokio::spawn(async move {
            let acquire = sem.acquire_owned();
            tokio::pin!(acquire);
            // Register the FIFO waiter before signalling the test, without
            // relying on yields or wall-clock sleeps to establish queue order.
            let permit = match futures::poll!(acquire.as_mut()) {
                std::task::Poll::Ready(permit) => {
                    queued.send(()).unwrap();
                    permit.unwrap()
                }
                std::task::Poll::Pending => {
                    queued.send(()).unwrap();
                    acquire.await.unwrap()
                }
            };
            let _permit = permit;
            start.set(now).unwrap();
            events.send(path.clone()).unwrap();
            if is_hung {
                std::future::pending::<()>().await;
            }
            (HashMap::new(), Ok(SyncOutcome::NothingToDo))
        });
        workers.insert(repo.clone(), handle.abort_handle());
        detached.push(tagged_sync_join(repo.clone(), handle));
        queued_rx.await.unwrap();
    }
    assert_eq!(rx.recv().await.unwrap(), a);
    assert_eq!(
        state.expired(now + Duration::from_secs(901), Duration::from_secs(900)),
        vec![a.clone()]
    );
    request_worker_cancellation(&workers, &a);
    assert!(!reserve_sync(&mut in_flight, &a), "request is not teardown");
    assert!(
        !reserve_sync(&mut in_flight, &b),
        "queued owner must keep its place"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        assert_eq!(rx.recv().await.unwrap(), b);
        assert_eq!(rx.recv().await.unwrap(), c);
        let mut cycle = in_flight.clone();
        let mut ages = HashMap::new();
        while let Some((repo, _, result)) = detached.next().await {
            release_sync_owner(&repo, &mut in_flight, &mut workers, &mut cycle, &mut ages);
            state.finish(&repo, result.is_err(), now);
        }
        assert!(cycle.is_empty());
    })
    .await
    .unwrap();
    assert!(in_flight.is_empty());
    assert!(workers.is_empty());
    assert_eq!(sem.available_permits(), 1);
    assert!(!state.ready(&a, now));
    assert!(state.ready(&b, now) && state.ready(&c, now));
}
