use super::*;
use crate::coordination::push_requests::tests::{ask_push, item_id};
use crate::coordination::reports::tests::{NOW, migrated_pool, seed};
use crate::git::push::tests::PushFixture;
use crate::git::{push_object, push_target};

const LATER: &str = "2026-11-12T10:00:00.000Z";

async fn requested_pool() -> (SqlitePool, String) {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    ask_push(&pool, "request-1").await.unwrap();
    (pool, item_id("request-1"))
}

fn refusal(result: ChatResult<PushClaim>) -> Option<PushRequestRefusal> {
    PushRequestRefusal::from_error(&result.unwrap_err())
}

#[tokio::test]
async fn of_attempts_racing_for_a_request_exactly_one_claims_it() {
    let (pool, item_id) = requested_pool().await;
    let attempts: Vec<_> = (0..8)
        .map(|_| {
            let (pool, item_id) = (pool.clone(), item_id.clone());
            tokio::spawn(async move { claim_push_request(&pool, &item_id, "process-a", NOW).await })
        })
        .collect();
    let mut results = Vec::new();
    for attempt in attempts {
        results.push(attempt.await.unwrap());
    }
    let winners = results
        .iter()
        .filter(|result| matches!(result, Ok(PushClaim::Fresh)))
        .count();
    assert_eq!(winners, 1);
    for result in results.into_iter().filter(Result::is_err) {
        assert_eq!(refusal(result), Some(PushRequestRefusal::InProgress));
    }

    let object_id = "a".repeat(40);
    mark_push_request_pushed(&pool, &item_id, "process-a", &object_id, NOW)
        .await
        .unwrap();
    assert_eq!(
        refusal(claim_push_request(&pool, &item_id, "process-a", NOW).await),
        Some(PushRequestRefusal::AlreadyPushed)
    );
}

#[tokio::test]
async fn a_claim_a_stopped_process_left_is_taken_over_with_its_commit() {
    let (pool, item_id) = requested_pool().await;
    let object_id = "b".repeat(40);
    assert_eq!(
        claim_push_request(&pool, &item_id, "process-a", NOW)
            .await
            .unwrap(),
        PushClaim::Fresh
    );
    record_push_object(&pool, &item_id, "process-a", &object_id, NOW)
        .await
        .unwrap();

    assert_eq!(
        claim_push_request(&pool, &item_id, "process-b", LATER)
            .await
            .unwrap(),
        PushClaim::Interrupted {
            object_id: Some(object_id.clone())
        }
    );
    assert_eq!(
        refusal(claim_push_request(&pool, &item_id, "process-b", LATER).await),
        Some(PushRequestRefusal::InProgress)
    );
    let stale = mark_push_request_pushed(&pool, &item_id, "process-a", &object_id, LATER).await;
    assert_eq!(stale.unwrap_err().code, ChatErrorCode::Conflict);
    release_push_claim(&pool, &item_id, "process-a", LATER)
        .await
        .unwrap();
    assert_eq!(
        refusal(claim_push_request(&pool, &item_id, "process-b", LATER).await),
        Some(PushRequestRefusal::InProgress),
        "a stale release must not free the new holder's claim"
    );
}

#[tokio::test]
async fn a_released_claim_is_retried_as_interrupted_or_pushed() {
    let (pool, item_id) = requested_pool().await;
    claim_push_request(&pool, &item_id, "process-a", NOW)
        .await
        .unwrap();
    release_push_claim(&pool, &item_id, "process-a", NOW)
        .await
        .unwrap();
    assert_eq!(
        claim_push_request(&pool, &item_id, "process-a", LATER)
            .await
            .unwrap(),
        PushClaim::Interrupted { object_id: None }
    );

    let object_id = "c".repeat(64);
    mark_push_request_pushed(&pool, &item_id, "process-a", &object_id, LATER)
        .await
        .unwrap();
    release_push_claim(&pool, &item_id, "process-a", LATER)
        .await
        .unwrap();
    assert_eq!(
        claim_push_request(&pool, &item_id, "process-a", LATER)
            .await
            .unwrap(),
        PushClaim::Pushed { object_id }
    );
}

#[tokio::test]
async fn a_claim_records_only_full_object_ids() {
    let (pool, item_id) = requested_pool().await;
    claim_push_request(&pool, &item_id, "process-a", NOW)
        .await
        .unwrap();
    let malformed = record_push_object(&pool, &item_id, "process-a", "HEAD", NOW).await;
    assert_eq!(malformed.unwrap_err().code, ChatErrorCode::Validation);
}

/// A request whose first attempt recorded the commit of `fixture` and then stopped, taken over
/// by `process-b`.
async fn interrupted_request(fixture: &PushFixture) -> (SqlitePool, String, GitPushTarget) {
    let (pool, item_id) = requested_pool().await;
    let target = push_target(fixture.folder(), Some("origin"))
        .await
        .expect("the fixture branch should have a target");
    claim_push_request(&pool, &item_id, "process-a", NOW)
        .await
        .unwrap();
    record_push_object(&pool, &item_id, "process-a", &target.object_id, NOW)
        .await
        .unwrap();
    (pool, item_id, target)
}

#[tokio::test]
async fn an_interrupted_push_whose_commit_arrived_is_settled_without_pushing_again() {
    let fixture = PushFixture::new().await;
    let (pool, item_id, target) = interrupted_request(&fixture).await;
    push_object(fixture.folder(), &target).await.unwrap();

    assert_eq!(
        claim_push_request(&pool, &item_id, "process-b", LATER)
            .await
            .unwrap(),
        PushClaim::Interrupted {
            object_id: Some(target.object_id.clone())
        }
    );
    let arrived = recover_interrupted_push(
        &pool,
        fixture.folder(),
        &item_id,
        "process-b",
        &target,
        None,
        LATER,
    )
    .await
    .unwrap();
    assert!(arrived);
    assert_eq!(
        refusal(claim_push_request(&pool, &item_id, "process-b", LATER).await),
        Some(PushRequestRefusal::AlreadyPushed)
    );
    assert_eq!(
        claim_push_request(&pool, &item_id, "process-c", LATER)
            .await
            .unwrap(),
        PushClaim::Pushed {
            object_id: target.object_id
        }
    );
}

#[tokio::test]
async fn an_interrupted_push_whose_commit_never_arrived_stays_claimed_for_a_new_push() {
    let fixture = PushFixture::new().await;
    let (pool, item_id, target) = interrupted_request(&fixture).await;

    claim_push_request(&pool, &item_id, "process-b", LATER)
        .await
        .unwrap();
    let arrived = recover_interrupted_push(
        &pool,
        fixture.folder(),
        &item_id,
        "process-b",
        &target,
        None,
        LATER,
    )
    .await
    .unwrap();
    assert!(!arrived);
    assert_eq!(fixture.remote_branch("main").await, None);
    assert_eq!(
        refusal(claim_push_request(&pool, &item_id, "process-b", LATER).await),
        Some(PushRequestRefusal::InProgress)
    );
    push_object(fixture.folder(), &target).await.unwrap();
    mark_push_request_pushed(&pool, &item_id, "process-b", &target.object_id, LATER)
        .await
        .unwrap();
    assert_eq!(
        fixture.remote_branch("main").await.as_deref(),
        Some(target.object_id.as_str())
    );
}
