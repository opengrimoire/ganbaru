use super::*;

fn runtime(capacity: usize) -> (MusicSessionState, mpsc::Receiver<Message>) {
    let (sender, receiver) = mpsc::channel(capacity);
    (
        MusicSessionState {
            sender,
            lifecycle: LifecycleControl::new(),
        },
        receiver,
    )
}

#[tokio::test]
async fn full_command_queue_cannot_lose_resume_or_reopen_an_older_freeze() {
    let (runtime, mut receiver) = runtime(1);
    assert!(
        runtime
            .sender
            .try_send(Message {
                request: Request::Snapshot,
                expected_vault: None,
                reply: None,
            })
            .is_ok()
    );
    assert!(
        freeze_owner(&runtime, Duration::from_secs(1))
            .await
            .is_err()
    );
    let freeze = *runtime.lifecycle.subscribe().borrow();
    assert_eq!(freeze.intent, LifecycleIntent::Freeze);
    assert!(runtime.lifecycle.is_revoked());

    resume_owner(&runtime).unwrap();
    let resume = *runtime.lifecycle.subscribe().borrow();
    assert_eq!(resume.intent, LifecycleIntent::Resume);
    assert!(
        !runtime
            .lifecycle
            .matches(freeze.revision, LifecycleIntent::Freeze)
    );
    assert!(!runtime.lifecycle.complete_resume(freeze.revision));
    assert!(runtime.lifecycle.is_revoked());
    assert!(matches!(
        receiver.recv().await.unwrap().request,
        Request::Snapshot
    ));
    assert!(receiver.try_recv().is_err());
    assert!(runtime.lifecycle.complete_resume(resume.revision));
    assert!(!runtime.lifecycle.is_revoked());
}

#[tokio::test]
async fn stalled_freeze_times_out_and_a_late_queued_request_cannot_freeze_after_resume() {
    let (runtime, mut receiver) = runtime(1);
    let error = freeze_owner(&runtime, Duration::from_millis(5))
        .await
        .unwrap_err();
    assert!(error.contains("did not acknowledge"));
    let pending = receiver.recv().await.unwrap();
    let Request::Freeze { revision } = pending.request else {
        panic!("quiescence must submit a revision-bound freeze");
    };
    assert!(runtime.lifecycle.matches(revision, LifecycleIntent::Freeze));
    resume_owner(&runtime).unwrap();
    assert!(!runtime.lifecycle.matches(revision, LifecycleIntent::Freeze));
    assert!(
        pending
            .reply
            .unwrap()
            .send(Ok(new_policy().projection(false, now_ms())))
            .is_err()
    );
}

#[tokio::test]
async fn freeze_requires_owner_acknowledgement_and_reports_a_lost_reply() {
    let (runtime, mut receiver) = runtime(1);
    let admitted = runtime.clone();
    let freeze =
        tokio::spawn(async move { freeze_owner(&admitted, Duration::from_secs(10)).await });
    let pending = receiver.recv().await.unwrap();
    assert!(!freeze.is_finished());
    assert!(runtime.lifecycle.is_revoked());
    pending
        .reply
        .unwrap()
        .send(Ok(new_policy().projection(false, now_ms())))
        .unwrap();
    freeze.await.unwrap().unwrap();
    assert!(runtime.lifecycle.is_revoked());

    let admitted = runtime.clone();
    let lost = tokio::spawn(async move { freeze_owner(&admitted, Duration::from_secs(10)).await });
    drop(receiver.recv().await.unwrap());
    assert!(
        lost.await
            .unwrap()
            .unwrap_err()
            .contains("receive Music quiescence")
    );
    assert!(runtime.lifecycle.is_revoked());
}

#[test]
fn stopped_owner_rejects_reactivation_without_changing_its_lifecycle_request() {
    let (runtime, receiver) = runtime(1);
    runtime.lifecycle.request(LifecycleIntent::Freeze).unwrap();
    let before = *runtime.lifecycle.subscribe().borrow();
    drop(receiver);
    assert!(
        resume_owner(&runtime)
            .unwrap_err()
            .contains("owner stopped")
    );
    let after = *runtime.lifecycle.subscribe().borrow();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.intent, before.intent);
}
