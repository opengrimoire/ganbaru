//! The model a post chooses for the new work it starts, and how continuations keep it.

use super::routing::{
    ROUTED_ASSIGNMENT_ID, post_request, recover_on_desk, reply_request, review_work_in,
    routed_input, set_review_state,
};
use super::*;

fn explicit_model(model_id: &str) -> ChatAssignmentModelOverride {
    ChatAssignmentModelOverride {
        provider_managed_model: false,
        model_id: Some(ModelId::new(model_id).unwrap()),
        model_options: Vec::new(),
    }
}

/// The stored override columns of `assignment_id`.
async fn stored_override(pool: &SqlitePool, assignment_id: &str) -> (Option<i64>, Option<String>) {
    sqlx::query_as(
        "SELECT override_model_selection_schema_version, override_model_selection_data
         FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Gives the seeded review work the override `model_override`, as a synced creation would.
async fn set_review_override(pool: &SqlitePool, model_override: &ChatAssignmentModelOverride) {
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE chat_work_assignments
         SET override_model_selection_schema_version = 1, override_model_selection_data = ?1
         WHERE id = ?2",
    )
    .bind(serde_json::to_string(model_override).unwrap())
    .bind(ROUTED_ASSIGNMENT_ID)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query("UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1")
        .execute(&mut *transaction)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
}

#[test]
fn model_override_on_active_work_is_refused() {
    tauri::async_runtime::block_on(async {
        for state in ["queued", "working", "waiting_for_answer"] {
            let pool = review_work_in(state).await;
            let mut request = reply_request("command:override", None);
            request.model_override = Some(explicit_model("model:other"));

            let Err(error) = post_request(&pool, &request).await else {
                panic!("a model override on {state} work posted");
            };

            assert_eq!(error.code, ChatErrorCode::Validation);
            assert_eq!(error.field.as_deref(), Some("modelOverride"));
            assert_eq!(
                error.details.as_deref(),
                Some(&json!({ "reason": "model_override_on_active_work" }))
            );
            let inputs: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM chat_work_assignment_inputs WHERE assignment_id = ?",
            )
            .bind(ROUTED_ASSIGNMENT_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(inputs, 1, "{state} work keeps only its trigger");
        }
    });
}

#[test]
fn a_model_override_must_name_one_selection() {
    let mut request = message("Hello");
    request.model_override = Some(ChatAssignmentModelOverride {
        provider_managed_model: true,
        model_id: Some(ModelId::new("model:other").unwrap()),
        model_options: Vec::new(),
    });
    assert_eq!(
        validate_message_request(&request)
            .unwrap_err()
            .field
            .as_deref(),
        Some("modelOverride")
    );
    request.model_override = Some(ChatAssignmentModelOverride {
        provider_managed_model: false,
        model_id: None,
        model_options: Vec::new(),
    });
    assert!(validate_message_request(&request).is_err());
    request.model_override = Some(ChatAssignmentModelOverride {
        provider_managed_model: true,
        model_id: None,
        model_options: Vec::new(),
    });
    assert!(validate_message_request(&request).is_ok());
    let mut large = explicit_model("model:other");
    large.model_options.push(ModelOptionSelection {
        key: "notes".to_owned(),
        value: ModelOptionValue::Text("m".repeat(super::super::common::MAX_MODEL_OVERRIDE_CHARS)),
    });
    request.model_override = Some(large);
    assert!(validate_message_request(&request).is_err(), "too large");
}

#[test]
fn a_new_assignment_stores_its_model_override() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        let mut request = reply_request("command:override", None);
        let chosen = explicit_model("model:other");
        request.model_override = Some(chosen.clone());

        let posted = post_request(&pool, &request).await.unwrap();

        let assignment_id = posted.result.assignment.expect("the post starts work").id;
        assert_ne!(assignment_id.as_str(), ROUTED_ASSIGNMENT_ID);
        let (version, data) = stored_override(&pool, assignment_id.as_str()).await;
        assert_eq!(version, Some(1));
        let stored: ChatAssignmentModelOverride =
            serde_json::from_str(&data.expect("the override is stored")).unwrap();
        assert_eq!(stored, chosen);
    });
}

#[test]
fn continuation_copies_model_override() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;
        let chosen = explicit_model("model:other");
        set_review_override(&pool, &chosen).await;
        let queued = post_request(
            &pool,
            &reply_request("command:queue", Some(ChatFollowupRouting::Queue)),
        )
        .await
        .unwrap();
        set_review_state(&pool, "completed").await;

        assert_eq!(recover_on_desk(&pool).await, 1);

        let (follow_up_id, _, _) = routed_input(&pool, &queued).await;
        assert_ne!(follow_up_id, ROUTED_ASSIGNMENT_ID);
        assert_eq!(
            stored_override(&pool, &follow_up_id).await,
            stored_override(&pool, ROUTED_ASSIGNMENT_ID).await,
            "the continuation runs with the model its work was asked with"
        );
    });
}

#[test]
fn a_continuation_without_a_previous_model_override_uses_the_default() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;

        let posted = post_request(&pool, &reply_request("command:continue", None))
            .await
            .unwrap();

        let assignment_id = posted.result.assignment.expect("the post starts work").id;
        assert_eq!(
            stored_override(&pool, assignment_id.as_str()).await,
            (None, None)
        );
    });
}

#[test]
fn a_scheduled_message_cannot_choose_a_model_override() {
    let mut request = message("Later");
    request.model_override = Some(explicit_model("model:other"));
    let error = super::super::scheduling::refuse_scheduled_post_choices(&request).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(error.field.as_deref(), Some("modelOverride"));
}

#[test]
fn a_model_override_without_a_teammate_is_refused() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        let mut request = message("Plain channel note");
        request.client_command_id = ChatCommandId::new("command:no-teammate").unwrap();
        request.channel_id = ChatChannelId::new("channel:review").unwrap();
        request.model_override = Some(explicit_model("model:other"));

        let Err(error) = post_request(&pool, &request).await else {
            panic!("a model override reached no teammate and posted");
        };

        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("modelOverride"));
        assert_eq!(
            error.details.as_deref(),
            Some(&json!({ "reason": "model_override_without_teammate" }))
        );
        let receipts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_organizational_command_receipts
             WHERE client_command_id = 'command:no-teammate'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(receipts, 0, "the refused post leaves nothing behind");
    });
}

#[test]
fn a_mention_that_records_no_work_keeps_its_model_override_for_start() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        set_review_agent_home_device(&pool, None).await;
        let desk_writers = chat_writers_of(&["device:desk"]);
        let mut request = reply_request("command:mention-only", None);
        let chosen = explicit_model("model:other");
        request.model_override = Some(chosen.clone());

        let posted = persist_post(
            &pool,
            &request,
            None,
            PostingDevice::this_device("device:phone", &desk_writers),
        )
        .await
        .unwrap();

        let assignments: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_work_assignments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            assignments, 1,
            "a device that cannot run the teammate records the mention only"
        );
        let started = assignments::persist_started_assignment(
            &pool,
            &posted.item_id,
            &ChatParticipantId::new("participant:review-agent").unwrap(),
            PostingDevice::this_device("device:desk", &desk_writers),
            &UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap(),
        )
        .await
        .unwrap();
        let (version, data) = stored_override(&pool, started.as_str()).await;
        assert_eq!(version, Some(1));
        let stored: ChatAssignmentModelOverride =
            serde_json::from_str(&data.expect("Start applies the mention's override")).unwrap();
        assert_eq!(stored, chosen);
    });
}

#[test]
fn a_reply_without_a_mention_that_records_no_work_refuses_its_model_override() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("completed").await;
        set_review_agent_home_device(&pool, None).await;
        let desk_writers = chat_writers_of(&["device:desk"]);
        let mut request = message("continue");
        request.client_command_id = ChatCommandId::new("command:implicit").unwrap();
        request.channel_id = ChatChannelId::new("channel:review").unwrap();
        request.reply_thread_id = Some(ChatReplyThreadId::new("reply-thread:review").unwrap());
        request.model_override = Some(explicit_model("model:other"));

        let Err(error) = persist_post(
            &pool,
            &request,
            None,
            PostingDevice::this_device("device:phone", &desk_writers),
        )
        .await
        else {
            panic!("an implicit continuation dropped its model override and posted");
        };

        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("modelOverride"));
        assert_eq!(
            error.details.as_deref(),
            Some(&json!({ "reason": "model_override_needs_mention" }))
        );
    });
}
