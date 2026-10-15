use super::*;

#[tokio::test]
async fn recovery_completed_status_racing_cancellation_retains_original_age() {
    let repo = PathBuf::from("finished-between-collect-and-watchdog");
    let now = Instant::now();
    let old = now - Duration::from_secs(61);
    let path = repo.clone();
    let task: StatusJoin = tokio::spawn(async move {
        (path, Ok(dracon_git::types::RepoStatus::new()))
    });
    let abort = task.abort_handle();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !abort.is_finished() { tokio::task::yield_now().await; }
    }).await.unwrap();
    let mut pending = HashSet::from([repo.clone()]);
    let mut owners = HashMap::from([(repo.clone(), abort.into())]);
    let mut since = HashMap::from([(repo.clone(), old)]);
    let mut jobs = FuturesUnordered::new();
    jobs.push(task);
    let mut results = HashMap::new();
    // Model a completion arriving just AFTER the pre-watchdog drain. Aborting
    // an already-finished task is a no-op; its successful result must not be
    // given a new timestamp or accepted as a fresh status snapshot.
    assert!(abort_wedged_status_inspection(&mut pending, &mut since, &mut owners, &repo, now));
    assert!(!abort_wedged_status_inspection(&mut pending, &mut since, &mut owners, &repo, now));
    tokio::time::timeout(Duration::from_secs(5), async {
        while !jobs.is_empty() {
            collect_ready_status(&mut jobs, &mut pending, &mut owners, &mut since, &mut results);
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    let (outcome, stamp) = results.remove(&repo).unwrap();
    assert!(outcome.is_ok());
    assert_eq!(stamp, old);
    assert!(now.duration_since(stamp) >= Duration::from_secs(STATUS_RESULT_MAX_AGE_SECS));
    assert!(pending.is_empty() && owners.is_empty() && since.is_empty());
}
