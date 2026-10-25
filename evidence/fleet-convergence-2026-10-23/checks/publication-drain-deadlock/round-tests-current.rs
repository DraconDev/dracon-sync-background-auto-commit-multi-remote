use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use std::time::Duration;
use tokio::sync::{oneshot, Notify};

#[tokio::test]
async fn completed_upload_cannot_hold_capacity_while_draining_a_queued_refresh() {
    completion_race(false).await;
}

#[tokio::test]
async fn failed_upload_cannot_hold_capacity_while_draining_a_queued_refresh() {
    completion_race(true).await;
}

async fn completion_race(failed: bool) {
    let capacity = Arc::new(Semaphore::new(1));
    let permit = capacity.clone().acquire_owned().await.unwrap();
    let (finish_upload, upload_done) = oneshot::channel();
    let tick = Arc::new(Notify::new());
    let refreshed = Arc::new(AtomicBool::new(false));
    let scope_capacity = capacity.clone();
    let refresh_marker = refreshed.clone();
    let timer = tick.clone();
    let mut actor = Box::pin(with_local_work(scope_capacity, permit, async move {
        publication_round(
            Box::pin(async {
                upload_done.await.unwrap();
                if failed {
                    anyhow::bail!("retained local hook failure");
                }
                Ok(17)
            }),
            || {
                let marker = refresh_marker.clone();
                async move {
                    local_phase(async move {
                        marker.store(true, Ordering::SeqCst);
                        Ok(())
                    })
                    .await
                    .unwrap();
                    false
                }
            },
            move |_| timer.clone().notified_owned(),
        )
        .await
    }));
    assert!(matches!(futures::poll!(actor.as_mut()), Poll::Pending));
    // B is healthy unrelated local work. Completion queues restoration while
    // B owns the sole permit, before a refresh timer becomes ready.
    let other = capacity.clone().acquire_owned().await.unwrap();
    finish_upload.send(()).unwrap();
    assert!(matches!(futures::poll!(actor.as_mut()), Poll::Pending));
    tick.notify_one();
    assert!(matches!(futures::poll!(actor.as_mut()), Poll::Pending));
    assert!(matches!(futures::poll!(actor.as_mut()), Poll::Pending));
    drop(other);
    let result = tokio::time::timeout(Duration::from_secs(1), actor).await;
    assert!(
        result.is_ok(),
        "publication restoration must not deadlock a later queued refresh"
    );
    let report = result.unwrap();
    if failed {
        assert!(report
            .unwrap_err()
            .to_string()
            .contains("retained local hook failure"));
    } else {
        assert_eq!(report.unwrap(), 17);
    }
    assert_eq!(capacity.available_permits(), 1);
    // Once upload already ended, it is also valid to finish without starting
    // another refresh; scheduler redispatch can then commit subsequent work.
}

#[tokio::test]
async fn started_refresh_finishes_before_outer_capacity_is_restored() {
    let capacity = Arc::new(Semaphore::new(1));
    let permit = capacity.clone().acquire_owned().await.unwrap();
    let (finish_upload, upload_done) = oneshot::channel();
    let finish_refresh = Arc::new(Notify::new());
    let tick = Arc::new(Notify::new());
    let entered = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(AtomicBool::new(false));
    let timer = tick.clone();
    let started = entered.clone();
    let finished = completed.clone();
    let release = finish_refresh.clone();
    let mut actor = Box::pin(with_local_work(capacity.clone(), permit, async move {
        publication_round(
            Box::pin(async {
                upload_done.await.unwrap();
                Ok(())
            }),
            || {
                let started = started.clone();
                let finished = finished.clone();
                let release = release.clone();
                async move {
                    local_phase(async move {
                        started.store(true, Ordering::SeqCst);
                        release.notified().await;
                        finished.store(true, Ordering::SeqCst);
                        Ok(())
                    })
                    .await
                    .unwrap();
                    false
                }
            },
            move |_| timer.clone().notified_owned(),
        )
        .await
    }));
    assert!(matches!(futures::poll!(actor.as_mut()), Poll::Pending));
    tick.notify_one();
    for _ in 0..3 {
        assert!(matches!(futures::poll!(actor.as_mut()), Poll::Pending));
    }
    assert!(entered.load(Ordering::SeqCst));
    finish_upload.send(()).unwrap();
    assert!(matches!(futures::poll!(actor.as_mut()), Poll::Pending));
    assert!(
        !completed.load(Ordering::SeqCst),
        "upload completion cannot cancel active mutation"
    );
    assert_eq!(capacity.available_permits(), 0);
    finish_refresh.notify_one();
    tokio::time::timeout(Duration::from_secs(1), actor)
        .await
        .unwrap()
        .unwrap();
    assert!(completed.load(Ordering::SeqCst));
    assert_eq!(capacity.available_permits(), 1);
}

#[test]
fn coordinator_frame_does_not_embed_the_upload_payload() {
    let payload = [7_u8; 64 * 1024];
    let upload = async move {
        std::future::pending::<()>().await;
        Ok(std::hint::black_box(payload)[0])
    };
    assert!(std::mem::size_of_val(&upload) >= payload.len());
    let round = publication_round(
        Box::pin(upload),
        || async { false },
        |_| std::future::pending::<()>(),
    );
    // This is a stack-safety budget, not an incidental observed size. Large
    // uploads must not be multiplied through the nested debug async frames.
    const MAX_COORDINATOR_FRAME_BYTES: usize = 8 * 1024;
    assert!(std::mem::size_of_val(&round) < MAX_COORDINATOR_FRAME_BYTES);
}
