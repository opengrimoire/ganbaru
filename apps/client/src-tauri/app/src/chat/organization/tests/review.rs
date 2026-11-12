//! Accept and Request changes on ready-for-review work, and the review a reply records.

use super::super::workflow::set_assignment_state;
use super::*;

const REVIEWED_ASSIGNMENT_ID: &str = "assignment:review";
const REVIEW_TIME: &str = "2026-08-04T17:03:00.000Z";

/// The seeded review work with its stranded replies removed, so only the review under test
/// routes to it, in `state`.
async fn reviewed_work(state: &str) -> (SqlitePool, ChatWorkAssignmentRead) {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    sqlx::query(
        "DELETE FROM chat_work_assignment_inputs
         WHERE assignment_id = ?1 AND routing_kind <> 'trigger'",
    )
    .bind(REVIEWED_ASSIGNMENT_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE chat_work_assignments SET state = ?1 WHERE id = ?2")
        .bind(state)
        .bind(REVIEWED_ASSIGNMENT_ID)
        .execute(&pool)
        .await
        .unwrap();
    let assignment = read_assignment(&pool, &reviewed_id()).await.unwrap();
    (pool, assignment)
}

fn reviewed_id() -> ChatWorkAssignmentId {
    ChatWorkAssignmentId::new(REVIEWED_ASSIGNMENT_ID).unwrap()
}

async fn assignment_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_work_assignments")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Posts the review note `note` on `assignment` from `device:desk`, which writes Chat only
/// when `desk_writes_chat`.
async fn request_changes(
    pool: &SqlitePool,
    assignment: &ChatWorkAssignmentRead,
    note: &str,
    desk_writes_chat: bool,
) -> ChatResult<super::super::PersistedPost> {
    let request =
        assignments::review_note_request(pool, &assignment.id, assignment.revision, note).await?;
    let writers = chat_writers_of(if desk_writes_chat {
        &["device:desk"]
    } else {
        &[]
    });
    let review = ReviewedWork {
        assignment_id: &assignment.id,
        expected_revision: assignment.revision,
    };
    match write_post(
        pool,
        &request,
        None,
        Some(review),
        PostingDevice::this_device("device:desk", &writers),
    )
    .await?
    {
        PostOutcome::Posted(posted) => Ok(posted),
        PostOutcome::Refused(refusal) => Err(refusal),
    }
}

/// Replies routed to `assignment_id` that still wait for delivery.
async fn pending_reply_count(pool: &SqlitePool, assignment_id: &ChatWorkAssignmentId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM chat_work_assignment_inputs
         WHERE assignment_id = ? AND routing_kind <> 'trigger' AND delivery_state = 'pending'",
    )
    .bind(assignment_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn deliver_input(pool: &SqlitePool, input_id: &str) {
    sqlx::query(
        "UPDATE chat_work_assignment_inputs
         SET delivery_state = 'delivered', delivered_at = '2026-08-04T17:02:30.000Z'
         WHERE id = ?",
    )
    .bind(input_id)
    .execute(pool)
    .await
    .unwrap();
}

fn assert_review_refusal(error: &ChatError, reason: &str) {
    assert_eq!(error.code, ChatErrorCode::Conflict, "{error}");
    assert_eq!(
        error
            .details
            .as_deref()
            .and_then(|details| details["reason"].as_str()),
        Some(reason),
        "{error}"
    );
}

fn assert_reviewed(assignment: &ChatWorkAssignmentRead, outcome: ChatAssignmentReviewOutcome) {
    assert_eq!(assignment.state, ChatWorkAssignmentState::Completed);
    assert!(assignment.settled_at.is_some());
    let review = assignment
        .review
        .as_ref()
        .expect("the work carries no review");
    assert_eq!(review.outcome, outcome);
    assert_eq!(
        review.reviewed_by_participant_id.as_str(),
        LOCAL_PARTICIPANT_ID
    );
}

#[test]
fn accepting_reviewed_work_completes_it_with_the_reviewer() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;
        assert!(before.review.is_none());

        let accepted = assignments::accept_reviewed_assignment(&pool, &before.id, before.revision)
            .await
            .unwrap();

        assert_reviewed(&accepted, ChatAssignmentReviewOutcome::Accepted);
        assert_eq!(accepted.revision, before.revision + 1);
        assert_eq!(assignment_count(&pool).await, 1, "Accept created work");
    });
}

#[test]
fn accept_refuses_work_that_is_not_ready_for_review() {
    tauri::async_runtime::block_on(async {
        for state in ["working", "completed", "failed", "cancelled"] {
            let (pool, before) = reviewed_work(state).await;

            let error = assignments::accept_reviewed_assignment(&pool, &before.id, before.revision)
                .await
                .unwrap_err();

            assert_eq!(error.code, ChatErrorCode::InvalidStateTransition, "{state}");
            let after = read_assignment(&pool, &before.id).await.unwrap();
            assert_eq!(after.revision, before.revision, "{state}");
            assert!(after.review.is_none(), "{state}");
        }
    });
}

#[test]
fn reviews_refuse_a_stale_revision() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;

        let error = assignments::accept_reviewed_assignment(&pool, &before.id, before.revision + 1)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::StaleRevision);

        let stale = ChatWorkAssignmentRead {
            revision: before.revision + 1,
            ..before.clone()
        };
        let error = request_changes(&pool, &stale, "Cover the empty case", true)
            .await
            .err()
            .expect("a stale review note posted");
        assert_eq!(error.code, ChatErrorCode::StaleRevision);
        let after = read_assignment(&pool, &before.id).await.unwrap();
        assert_eq!(after.state, ChatWorkAssignmentState::ReadyForReview);
        assert_eq!(assignment_count(&pool).await, 1);
        let replies: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_conversation_items WHERE reply_thread_id = ?",
        )
        .bind(before.reply_thread_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(replies, 2, "the refused note stayed in the thread");
    });
}

#[test]
fn review_notes_must_have_text_within_the_message_limit() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;
        let oversized = "a".repeat(super::super::common::MAX_MESSAGE_BYTES + 1);

        for note in ["", " \n\t ", oversized.as_str()] {
            let error = assignments::review_note_request(&pool, &before.id, before.revision, note)
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Validation);
            assert_eq!(error.field.as_deref(), Some("note"));
        }
    });
}

#[test]
fn review_notes_post_as_plain_text_in_the_reviewed_thread() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;

        let request = assignments::review_note_request(
            &pool,
            &before.id,
            before.revision,
            "  Cover the empty case\r\n\nand the *long* one  ",
        )
        .await
        .unwrap();

        assert_eq!(
            request.reply_thread_id.as_ref(),
            Some(&before.reply_thread_id)
        );
        assert_eq!(request.channel_id.as_str(), "channel:review");
        assert_eq!(
            request.normalized_markdown,
            "Cover the empty case\n\nand the *long* one"
        );
        assert_eq!(
            request.rich_content.value,
            json!({ "lines": [
                { "runs": [{ "text": "Cover the empty case", "marks": [] }] },
                { "runs": [] },
                { "runs": [{ "text": "and the *long* one", "marks": [] }] },
            ] })
        );
        assert!(request.references.is_empty());
        validate_message_request(&request).unwrap();
    });
}

#[test]
fn requesting_changes_continues_in_one_follow_up_and_records_the_request() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;

        let posted = request_changes(&pool, &before, "Cover the empty case", true)
            .await
            .unwrap();

        let follow_up_id = posted.assignment_write.assignment_id.clone().unwrap();
        assert_ne!(follow_up_id, before.id);
        assert_eq!(dispatch_job_count(&pool, &follow_up_id).await, 1);
        let follow_up = read_assignment(&pool, &follow_up_id).await.unwrap();
        assert_eq!(follow_up.previous_assignment_id.as_ref(), Some(&before.id));
        assert_eq!(follow_up.triggering_message_item_id, posted.item_id);
        let reviewed = read_assignment(&pool, &before.id).await.unwrap();
        assert_reviewed(&reviewed, ChatAssignmentReviewOutcome::ChangesRequested);

        let repeated = request_changes(&pool, &before, "Cover the empty case", true)
            .await
            .unwrap();
        assert_eq!(repeated.item_id, posted.item_id, "the repeat posted again");
        assert_eq!(assignment_count(&pool).await, 2);
    });
}

#[test]
fn requesting_changes_without_a_device_for_the_teammate_is_refused_and_posts_nothing() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;

        let error = request_changes(&pool, &before, "Cover the empty case", false)
            .await
            .err()
            .expect("a note no device can run was posted");

        assert_review_refusal(&error, "no_executing_device");
        assert_eq!(assignment_count(&pool).await, 1, "a follow-up was created");
        let after = read_assignment(&pool, &before.id).await.unwrap();
        assert_eq!(after.state, ChatWorkAssignmentState::ReadyForReview);
        assert_eq!(after.revision, before.revision);
        assert!(after.review.is_none());
        let notes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_communication_message_revisions
             WHERE normalized_markdown = 'Cover the empty case'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(notes, 0, "the refused note was posted");
        let receipts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_organizational_command_receipts
             WHERE client_command_id LIKE 'assignment-review:%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(receipts, 0, "a refused note kept its receipt");
    });
}

#[test]
fn accept_is_refused_while_replies_wait_on_the_reviewed_work() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let before = read_assignment(&pool, &reviewed_id()).await.unwrap();

        let error = assignments::accept_reviewed_assignment(&pool, &before.id, before.revision)
            .await
            .unwrap_err();

        assert_review_refusal(&error, "pending_replies");
        let after = read_assignment(&pool, &before.id).await.unwrap();
        assert_eq!(after.state, ChatWorkAssignmentState::ReadyForReview);
        assert_eq!(after.revision, before.revision);
        assert!(after.review.is_none());
        assert_eq!(pending_reply_count(&pool, &before.id).await, 2);
    });
}

#[test]
fn accept_refuses_a_waiting_steer_and_ignores_delivered_replies() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        sqlx::query(
            "UPDATE chat_work_assignment_inputs SET routing_kind = 'steer'
             WHERE id = 'input:review-reply-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        deliver_input(&pool, "input:review-reply-2").await;
        let before = read_assignment(&pool, &reviewed_id()).await.unwrap();

        let error = assignments::accept_reviewed_assignment(&pool, &before.id, before.revision)
            .await
            .unwrap_err();
        assert_review_refusal(&error, "pending_replies");

        deliver_input(&pool, "input:review-reply-1").await;
        let accepted = assignments::accept_reviewed_assignment(&pool, &before.id, before.revision)
            .await
            .unwrap();
        assert_reviewed(&accepted, ChatAssignmentReviewOutcome::Accepted);
    });
}

#[test]
fn requesting_changes_carries_waiting_replies_into_the_follow_up() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let before = read_assignment(&pool, &reviewed_id()).await.unwrap();

        let posted = request_changes(&pool, &before, "Cover the empty case", true)
            .await
            .unwrap();

        let follow_up_id = posted.assignment_write.assignment_id.clone().unwrap();
        assert_eq!(pending_reply_count(&pool, &before.id).await, 0);
        let carried: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM chat_work_assignment_inputs
             WHERE assignment_id = ? AND delivery_state = 'pending'",
        )
        .bind(follow_up_id.as_str())
        .fetch_all(&pool)
        .await
        .unwrap();
        for reply in ["input:review-reply-1", "input:review-reply-2"] {
            assert!(
                carried.iter().any(|id| id == reply),
                "{reply} stayed behind: {carried:?}"
            );
        }
        let reviewed = read_assignment(&pool, &before.id).await.unwrap();
        assert_reviewed(&reviewed, ChatAssignmentReviewOutcome::ChangesRequested);
    });
}

#[test]
fn a_plain_reply_to_reviewed_work_records_changes_requested() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;
        let mut request = message("Also cover the edge cases");
        request.channel_id = ChatChannelId::new("channel:review").unwrap();
        request.reply_thread_id = Some(before.reply_thread_id.clone());
        let writers = chat_writers_of(&["device:desk"]);

        let posted = persist_post(
            &pool,
            &request,
            None,
            PostingDevice::this_device("device:desk", &writers),
        )
        .await
        .unwrap();

        assert!(posted.assignment_write.assignment_id.is_some());
        let reviewed = read_assignment(&pool, &before.id).await.unwrap();
        assert_reviewed(&reviewed, ChatAssignmentReviewOutcome::ChangesRequested);
    });
}

#[test]
fn cancelling_reviewed_work_clears_its_review() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;
        let accepted = assignments::accept_reviewed_assignment(&pool, &before.id, before.revision)
            .await
            .unwrap();

        let cancelled = set_assignment_state(
            &pool,
            &accepted.id,
            accepted.revision,
            ChatWorkAssignmentState::Cancelled,
            Some("Cancelled by the user"),
        )
        .await
        .unwrap();

        assert_eq!(cancelled.state, ChatWorkAssignmentState::Cancelled);
        assert!(cancelled.review.is_none());
    });
}

#[test]
fn review_times_are_recorded_at_the_review() {
    tauri::async_runtime::block_on(async {
        let (pool, before) = reviewed_work("ready_for_review").await;
        let now = UtcTimestamp::new(REVIEW_TIME).unwrap();
        let mut connection = pool.acquire().await.unwrap();

        let settled = super::super::workflow::complete_reviewed_assignment(
            &mut connection,
            &before.id,
            Some(before.revision),
            ChatAssignmentReviewOutcome::Accepted,
            LOCAL_PARTICIPANT_ID,
            &now,
        )
        .await
        .unwrap();
        drop(connection);

        assert!(settled);
        let reviewed = read_assignment(&pool, &before.id).await.unwrap();
        let review = reviewed.review.unwrap();
        assert_eq!(review.reviewed_at.as_str(), REVIEW_TIME);
        assert_eq!(
            reviewed.settled_at.as_ref().map(UtcTimestamp::as_str),
            Some(REVIEW_TIME)
        );
    });
}
