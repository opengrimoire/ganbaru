use super::common::{
    has_thread_eligible_mention, is_valid_teammate_effort, normalized_fts_query,
    require_unused_teammate_name, validate_message_request, validate_teammate_role,
};
use super::scheduling::{claim_scheduled_message_for_immediate_send, validate_scheduled_for};
use super::workflow::{has_authority_bearing_references, require_continuation_scope_is_unchanged};
use super::*;
use serde_json::json;
use sqlx::{Row, SqlitePool};

#[test]
fn teammate_effort_accepts_every_catalog_reasoning_level() {
    for effort in [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ] {
        assert!(is_valid_teammate_effort(effort), "rejected {effort}");
    }
    assert!(!is_valid_teammate_effort("automatic"));
}

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

async fn seed_unused_teammate(pool: &SqlitePool, teammate_id: &str, policy_id: &str) {
    sqlx::query(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, created_at, updated_at)
         VALUES (?, 'ai_teammate', 'Unused teammate', ?, ?)",
    )
    .bind(teammate_id)
    .bind("2026-08-14T17:30:00.000Z")
    .bind("2026-08-14T17:30:00.000Z")
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_teammates
            (participant_id, role, instructions, created_at, updated_at)
         VALUES (?, 'General support', '', ?, ?)",
    )
    .bind(teammate_id)
    .bind("2026-08-14T17:30:00.000Z")
    .bind("2026-08-14T17:30:00.000Z")
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_teammate_policy_revisions
            (id, teammate_id, provider_instance_id,
             model_selection_data, created_at)
         VALUES (?, ?, 'provider:test',
                 '{\"providerManagedModel\":true,\"modelId\":null,\"modelOptions\":[]}', ?)",
    )
    .bind(policy_id)
    .bind(teammate_id)
    .bind("2026-08-14T17:30:00.000Z")
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn archived_ai_teammate_names_remain_reserved_case_insensitively() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        sqlx::query(
            "INSERT INTO chat_participants
                (id, participant_kind, display_name, archived_at, created_at, updated_at)
             VALUES (?, 'ai_teammate', ?, ?, ?, ?)",
        )
        .bind("participant:archived-reviewer")
        .bind("Review Lead")
        .bind("2026-08-14T17:30:00.000Z")
        .bind("2026-08-14T17:30:00.000Z")
        .bind("2026-08-14T17:30:00.000Z")
        .execute(&pool)
        .await
        .unwrap();

        let mut connection = pool.acquire().await.unwrap();
        let error = require_unused_teammate_name(
            &mut connection,
            " review lead ",
            "participant:active-reviewer",
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("displayName"));

        require_unused_teammate_name(
            &mut connection,
            "Review Lead",
            "participant:archived-reviewer",
        )
        .await
        .unwrap();
        require_unused_teammate_name(&mut connection, "Release lead", "participant:other")
            .await
            .unwrap();
    });
}

fn message(markdown: &str) -> PostChatMessageCommand {
    PostChatMessageCommand {
        client_command_id: ChatCommandId::new("command:test").unwrap(),
        channel_id: ChatChannelId::new("channel:test").unwrap(),
        reply_thread_id: None,
        normalized_markdown: markdown.to_string(),
        rich_content: VersionedJson {
            schema_version: 1,
            value: json!({ "type": "doc", "content": [] }),
        },
        attachment_ids: Vec::new(),
        references: Vec::new(),
        execution_target: None,
        also_send_to_channel: false,
    }
}

fn participant_reference(
    reference_id: &str,
    participant_id: ChatParticipantId,
    participant_kind: ChatParticipantKind,
    label_snapshot: &str,
    plain_text_projection: &str,
    start_offset: u64,
    end_offset: u64,
) -> ChatMessageReference {
    ChatMessageReference::Participant {
        metadata: ChatReferenceMetadata {
            reference_id: ChatMessageReferenceId::new(reference_id).unwrap(),
            label_snapshot: label_snapshot.to_string(),
            start_offset,
            end_offset,
            plain_text_projection: plain_text_projection.to_string(),
        },
        participant_id,
        participant_kind,
    }
}

#[test]
fn teammate_role_is_required_and_trimmed() {
    let error = validate_teammate_role("   ").unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
    assert_eq!(error.field.as_deref(), Some("role"));
    assert_eq!(
        validate_teammate_role("  Frontend lead  ").unwrap(),
        "Frontend lead"
    );
}

async fn seed_review_assignment_with_stranded_replies(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group:review', 'Review', 'a1');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project:review', 'group:review', 'Review', 'a1');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path)
         VALUES ('folder:review', 'project:review', 'Review', 'managed', 'projects/review');
         INSERT INTO chat_conversations
             (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ('conversation:review', 'project:review', 'channel', '2026-08-04T17:00:00.000Z',
                 '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z');
         INSERT INTO chat_channels
             (id, project_id, conversation_id, name, is_default, created_at, updated_at)
         VALUES ('channel:review', 'project:review', 'conversation:review', 'general', 1,
                 '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z');
         INSERT INTO chat_conversation_memberships
             (conversation_id, participant_id, sync_key, membership_role, created_at,
              updated_at)
         VALUES ('conversation:review', 'participant:local-owner', 'membership:review-owner',
                 'owner', '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z');",
    )
    .execute(pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ganbaru_chat::project_defaults::ensure_project_chat_defaults(
        &mut connection,
        "project:review",
        &UtcTimestamp::new("2026-08-04T17:00:00.000Z").unwrap(),
    )
    .await
    .unwrap();
    drop(connection);
    sqlx::raw_sql(
        "INSERT INTO chat_participants
             (id, participant_kind, display_name, created_at, updated_at)
         VALUES (
             'participant:review-agent', 'ai_teammate', 'Review agent',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_teammates
             (participant_id, role, instructions, created_at, updated_at)
         VALUES (
             'participant:review-agent', 'Test', 'Test',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_teammate_policy_revisions
             (id, teammate_id, provider_instance_id,
              model_selection_data, created_at)
         VALUES (
             'policy:review', 'participant:review-agent', 'provider:review',
             '{\"providerManagedModel\":true,\"modelId\":null,\"modelOptions\":[]}',
             '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_conversation_memberships
             (conversation_id, participant_id, sync_key, membership_role, created_at,
              updated_at)
         VALUES (
             'conversation:review', 'participant:review-agent', 'membership:review', 'member',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_teammate_channel_memberships
             (conversation_id, teammate_id, sync_key, access_profile_id,
              read_history, read_history_inherits_profile,
              participate, participate_inherits_profile,
              history_boundary, history_boundary_inherits_profile,
              created_at, updated_at)
         VALUES (
             'conversation:review', 'participant:review-agent', 'channel-membership:review',
             'access-profile:build-and-test', 1, 1, 1, 1, 'entire', 1,
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_teammate_working_folder_grants
             (conversation_id, teammate_id, project_id, working_folder_id, sync_key,
              capability, capability_inherits_profile, is_default, created_at)
         VALUES (
             'conversation:review', 'participant:review-agent', 'project:review',
             'folder:review', 'grant:review', 'execute', 1, 1, '2026-08-04T17:00:00.000Z'
         );
         UPDATE chat_teammate_access_state
         SET access_revision = 1, updated_at = '2026-08-04T17:00:00.000Z'
         WHERE teammate_id = 'participant:review-agent';
         INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES (
             'item:review-root', 'conversation:review', 'message', 'a1',
             '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (
             'item:review-root', 'participant:local-owner', 'You',
             '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES (
             'revision:review-root', 'item:review-root', 'Create hello.py',
             '2026-08-04T17:00:00.000Z'
         );
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-root'
         WHERE item_id = 'item:review-root';
         INSERT INTO chat_reply_threads
             (id, conversation_id, root_item_id, last_activity_at,
              created_at, updated_at)
         VALUES (
             'reply-thread:review', 'conversation:review', 'item:review-root',
             '2026-08-04T17:02:00.000Z', '2026-08-04T17:00:00.000Z',
             '2026-08-04T17:02:00.000Z'
         );
         INSERT INTO chat_conversation_items
             (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES
             ('item:review-reply-1', 'conversation:review', 'reply-thread:review',
              'message', 'a1', '2026-08-04T17:01:00.000Z'),
             ('item:review-reply-2', 'conversation:review', 'reply-thread:review',
              'message', 'a2', '2026-08-04T17:02:00.000Z');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES
             ('item:review-reply-1', 'participant:local-owner', 'You',
              '2026-08-04T17:01:00.000Z'),
             ('item:review-reply-2', 'participant:local-owner', 'You',
              '2026-08-04T17:02:00.000Z');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES
             ('revision:review-reply-1', 'item:review-reply-1',
              'Ok, remove it now.', '2026-08-04T17:01:00.000Z'),
             ('revision:review-reply-2', 'item:review-reply-2',
              '@review-agent Ok, remove it now.', '2026-08-04T17:02:00.000Z');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-reply-1'
         WHERE item_id = 'item:review-reply-1';
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-reply-2'
         WHERE item_id = 'item:review-reply-2';
         INSERT INTO chat_work_assignments
             (id, reply_thread_id, teammate_id, triggering_message_item_id,
              executing_device_id, state, created_at, updated_at)
         VALUES (
             'assignment:review', 'reply-thread:review', 'participant:review-agent',
             'item:review-root', 'device:desk', 'ready_for_review',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:02:00.000Z'
         );
         INSERT INTO chat_work_assignment_inputs
             (id, assignment_id, message_item_id, ordinal, routing_kind,
              delivery_state, created_at, delivered_at)
         VALUES
             ('input:review-root', 'assignment:review', 'item:review-root', 1,
              'trigger', 'delivered', '2026-08-04T17:00:00.000Z',
              '2026-08-04T17:00:00.000Z'),
             ('input:review-reply-1', 'assignment:review', 'item:review-reply-1', 2,
              'queued_continuation', 'pending', '2026-08-04T17:01:00.000Z', NULL),
             ('input:review-reply-2', 'assignment:review', 'item:review-reply-2', 3,
              'queued_continuation', 'pending', '2026-08-04T17:02:00.000Z', NULL);",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn unused_archived_teammates_can_be_restored() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let teammate_id = ChatParticipantId::new("participant:unused-agent").unwrap();
        seed_unused_teammate(&pool, teammate_id.as_str(), "policy:unused-agent").await;

        let archived =
            super::teammate_lifecycle::set_teammate_archived(&pool, &teammate_id, 1, true)
                .await
                .unwrap();
        assert!(archived.participant.archived_at.is_some());
        assert_eq!(archived.active_assignment_count, 0);
        let archived_access = access::read_teammate_access(&pool, &teammate_id)
            .await
            .expect("archived agents stay inspectable in Settings");
        assert!(archived_access.channels.is_empty());

        let restored = super::teammate_lifecycle::set_teammate_archived(
            &pool,
            &teammate_id,
            archived.participant.revision,
            false,
        )
        .await
        .unwrap();
        assert!(restored.participant.archived_at.is_none());
    });
}

#[test]
fn active_work_blocks_teammate_archiving() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let teammate_id = ChatParticipantId::new("participant:review-agent").unwrap();

        let error = super::teammate_lifecycle::set_teammate_archived(&pool, &teammate_id, 1, true)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict);
        let archived_at: Option<String> =
            sqlx::query_scalar("SELECT archived_at FROM chat_participants WHERE id = ?")
                .bind(teammate_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(archived_at.is_none());
    });
}

#[test]
fn scheduled_delivery_requires_a_near_future_time_within_one_year() {
    let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();

    assert!(
        validate_scheduled_for(
            &UtcTimestamp::new("2026-08-03T00:00:30.000Z").unwrap(),
            &now,
        )
        .is_ok()
    );
    assert_eq!(
        validate_scheduled_for(
            &UtcTimestamp::new("2026-08-03T00:00:29.999Z").unwrap(),
            &now,
        )
        .unwrap_err()
        .field
        .as_deref(),
        Some("scheduledFor"),
    );
    assert_eq!(
        validate_scheduled_for(
            &UtcTimestamp::new("2027-08-05T00:00:00.000Z").unwrap(),
            &now,
        )
        .unwrap_err()
        .field
        .as_deref(),
        Some("scheduledFor"),
    );
}

/// Device that scheduled the test messages.
const SCHEDULING_DEVICE: &str = "device:scheduling";

/// Stores `request_data` as the scheduled message `scheduled:test` executed by `executing`,
/// with the local dispatch row the scheduling device keeps when `dispatches_here`.
async fn insert_test_scheduled_message(
    pool: &SqlitePool,
    request_data: &str,
    executing: Option<&str>,
    dispatches_here: bool,
    now: &UtcTimestamp,
) {
    sqlx::query(
        "INSERT INTO chat_scheduled_messages
            (id, client_command_id, channel_id, executing_device_id, request_data,
             scheduled_for, created_at, updated_at)
         VALUES ('scheduled:test', 'command:test', 'channel:test', ?, ?, ?, ?, ?)",
    )
    .bind(executing)
    .bind(request_data)
    .bind("2026-08-04T00:00:00.000Z")
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .unwrap();
    if dispatches_here {
        sqlx::query(
            "INSERT INTO chat_scheduled_message_dispatch
                (scheduled_message_id, attempt_count, available_at, updated_at)
             VALUES ('scheduled:test', 0, '2026-08-04T00:00:00.000Z', ?)",
        )
        .bind(now.as_str())
        .execute(pool)
        .await
        .unwrap();
    }
}

#[test]
fn immediate_scheduled_delivery_claim_is_atomic() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Send this now")).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        let claimed = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:first",
        )
        .await
        .unwrap();
        assert_eq!(claimed, request_data);
        let row = sqlx::query(
            "SELECT attempt_count, claim_token
             FROM chat_scheduled_message_dispatch WHERE scheduled_message_id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<i64, _>("attempt_count"), 1);
        assert_eq!(row.get::<String, _>("claim_token"), "claim:first");
        let duplicate = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:second",
        )
        .await
        .unwrap_err();
        assert_eq!(duplicate.code, ChatErrorCode::Busy);
    });
}

#[test]
fn only_the_executing_device_sends_a_scheduled_message() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Send from the desk")).unwrap();
        // The scheduled message arrived through sync: this device keeps no dispatch row.
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), false, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();

        let refused = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            "device:phone",
            &now,
            "claim:phone",
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
        scheduling::prepare_scheduled_dispatch(&pool, "device:phone", &now)
            .await
            .unwrap();
        let dispatch_rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_scheduled_message_dispatch")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dispatch_rows, 0);

        // The executing device adopts the message even without its dispatch row.
        scheduling::prepare_scheduled_dispatch(&pool, SCHEDULING_DEVICE, &now)
            .await
            .unwrap();
        scheduling::require_scheduled_executor(&pool, &scheduled_message_id, SCHEDULING_DEVICE)
            .await
            .unwrap();
        let available_at: String = sqlx::query_scalar(
            "SELECT available_at FROM chat_scheduled_message_dispatch
             WHERE scheduled_message_id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(available_at, "2026-08-04T00:00:00.000Z");
    });
}

#[test]
fn a_scheduled_message_without_an_executing_device_is_sent_where_it_dispatches() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Older schedule")).unwrap();
        insert_test_scheduled_message(&pool, &request_data, None, false, &now).await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();

        let refused =
            scheduling::require_scheduled_executor(&pool, &scheduled_message_id, "device:any")
                .await
                .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
        sqlx::query(
            "INSERT INTO chat_scheduled_message_dispatch
                (scheduled_message_id, attempt_count, available_at, updated_at)
             VALUES ('scheduled:test', 0, ?1, ?1)",
        )
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();
        scheduling::require_scheduled_executor(&pool, &scheduled_message_id, "device:any")
            .await
            .unwrap();
    });
}

#[test]
fn scheduled_content_must_fit_one_sync_operation() {
    let request = message("Short");
    let serialized = serde_json::to_string(&request).unwrap();
    scheduling::validate_scheduled_content(&serialized, &request.references).unwrap();

    let oversized = "x".repeat(ganbaru_sync::domains::chat::MAX_SCHEDULED_REQUEST_BYTES + 1);
    let error = scheduling::validate_scheduled_content(&oversized, &[]).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("message"));

    let mention = participant_reference(
        "reference:atlas",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiTeammate,
        "Atlas",
        "@Atlas",
        0,
        6,
    );
    let mentions = vec![mention; ganbaru_sync::domains::chat::MAX_SCHEDULED_REFERENCES + 1];
    let error = scheduling::validate_scheduled_content(&serialized, &mentions).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("references"));
}

#[test]
fn scheduled_send_posts_stored_attachment_values_under_derived_ids() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let mut request = message("Send the screenshot");
        request.attachment_ids = vec![ChatAttachmentId::new("attachment:gone").unwrap()];
        let request_data = serde_json::to_string(&request).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        // The device row of the attachment is absent, as after its working folder was deleted
        // or on a device that received the scheduled message through sync.
        let digest = "a".repeat(64);
        sqlx::query(
            "INSERT INTO chat_scheduled_message_attachment_references
                (scheduled_message_id, ordinal, attachment_id, working_folder_id, kind,
                 original_display_name, mime_type, byte_size, sha256)
             VALUES ('scheduled:test', 0, 'attachment:gone', 'working-folder:gone', 'image',
                     'screen.png', 'image/png', 42, ?)",
        )
        .bind(&digest)
        .execute(&pool)
        .await
        .unwrap();
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:a",
        )
        .await
        .unwrap();
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
        };
        let device = PostingDevice::this_device(SCHEDULING_DEVICE);

        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();

        let expected = super::common::scheduled_post_ids(
            "scheduled:test",
            super::common::ScheduledPostTarget::Message,
        )
        .unwrap();
        assert_eq!(posted.item_id, expected.item_id);
        let reference: (String, String, String, String, i64, String) = sqlx::query_as(
            "SELECT message_revision_id, attachment_id, working_folder_id, original_display_name,
                    byte_size, sha256
             FROM chat_communication_attachment_references",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            reference,
            (
                expected.revision_id.as_str().to_owned(),
                "attachment:gone".to_owned(),
                "working-folder:gone".to_owned(),
                "screen.png".to_owned(),
                42,
                digest,
            )
        );
        let scheduled_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(scheduled_rows, 0);

        let repeated = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();
        assert_eq!(repeated.item_id, expected.item_id);
        let items: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_conversation_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(items, 1);
    });
}

#[test]
fn scheduled_send_rejects_a_claim_that_changed() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request = message("Send this once");
        let request_data = serde_json::to_string(&request).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:a",
        )
        .await
        .unwrap();
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:stale",
        };

        let Err(error) = persist_post(
            &pool,
            &request,
            Some(send),
            PostingDevice::this_device(SCHEDULING_DEVICE),
        )
        .await
        else {
            panic!("a stale claim posted");
        };

        assert_eq!(error.code, ChatErrorCode::Conflict);
        let items: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_conversation_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(items, 0);
    });
}

async fn seed_test_channel(pool: &SqlitePool, now: &UtcTimestamp) {
    sqlx::query(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group:test', 'Test', 'a1')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project:test', 'group:test', 'Test', 'a1')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_conversations
                (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
             VALUES ('conversation:test', 'project:test', 'channel', ?, ?, ?)",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_channels
                (id, project_id, conversation_id, name, created_at, updated_at)
             VALUES ('channel:test', 'project:test', 'conversation:test', 'general', ?, ?)",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn restart_rehomes_review_replies_into_one_ordered_follow_up_assignment() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let mut input_ids = Vec::new();
        for message in [
            "item:review-root",
            "item:review-reply-1",
            "item:review-reply-2",
        ] {
            // Input ids are immutable keys, so the seeded input is written again under the id
            // that derives from its message.
            let id = super::common::assignment_input_id(message).unwrap();
            let seeded = sqlx::query(
                "SELECT assignment_id, ordinal, routing_kind, delivery_state, created_at,
                        delivered_at
                 FROM chat_work_assignment_inputs WHERE message_item_id = ?",
            )
            .bind(message)
            .fetch_one(&pool)
            .await
            .unwrap();
            sqlx::query("DELETE FROM chat_work_assignment_inputs WHERE message_item_id = ?")
                .bind(message)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO chat_work_assignment_inputs
                    (id, assignment_id, message_item_id, ordinal, routing_kind,
                     delivery_state, created_at, delivered_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(seeded.get::<String, _>("assignment_id"))
            .bind(message)
            .bind(seeded.get::<i64, _>("ordinal"))
            .bind(seeded.get::<String, _>("routing_kind"))
            .bind(seeded.get::<String, _>("delivery_state"))
            .bind(seeded.get::<String, _>("created_at"))
            .bind(seeded.get::<Option<String>, _>("delivered_at"))
            .execute(&pool)
            .await
            .unwrap();
            input_ids.push(id);
        }

        let elsewhere =
            assignments::recover_stranded_follow_up_assignments(&pool, "device:phone", &now)
                .await
                .unwrap();
        assert_eq!(elsewhere, 0, "only the executing device rehomes replies");
        let recovered =
            assignments::recover_stranded_follow_up_assignments(&pool, "device:desk", &now)
                .await
                .unwrap();

        assert_eq!(recovered, 1);
        let all_inputs: Vec<String> =
            sqlx::query_scalar("SELECT id FROM chat_work_assignment_inputs ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let mut expected_ids = input_ids.clone();
        expected_ids.sort();
        assert_eq!(
            all_inputs, expected_ids,
            "inputs move instead of being replaced"
        );
        let old_state: String = sqlx::query_scalar(
            "SELECT state FROM chat_work_assignments WHERE id = 'assignment:review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_state, "completed");
        let follow_up = sqlx::query(
            "SELECT id, triggering_message_item_id, previous_assignment_id, state,
                    executing_device_id
             FROM chat_work_assignments WHERE previous_assignment_id = 'assignment:review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let follow_up_id = follow_up.get::<String, _>("id");
        assert_eq!(
            follow_up.get::<String, _>("executing_device_id"),
            "device:desk"
        );
        assert_eq!(
            follow_up.get::<String, _>("triggering_message_item_id"),
            "item:review-reply-1"
        );
        assert_eq!(follow_up.get::<String, _>("state"), "queued");
        let inputs = sqlx::query(
            "SELECT message_item_id, ordinal, routing_kind, delivery_state
             FROM chat_work_assignment_inputs WHERE assignment_id = ? ORDER BY ordinal",
        )
        .bind(&follow_up_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(
            inputs[0].get::<String, _>("message_item_id"),
            "item:review-reply-1"
        );
        assert_eq!(inputs[0].get::<String, _>("routing_kind"), "follow_up");
        assert_eq!(
            inputs[1].get::<String, _>("message_item_id"),
            "item:review-reply-2"
        );
        assert_eq!(
            inputs[1].get::<String, _>("routing_kind"),
            "queued_continuation"
        );
        assert!(
            inputs
                .iter()
                .all(|input| input.get::<String, _>("delivery_state") == "pending")
        );
        let queued_job: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_assignment_dispatch_jobs
             WHERE assignment_id = ? AND state = 'queued'",
        )
        .bind(follow_up_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(queued_job, 1);
    });
}

#[test]
fn unmentioned_replies_continue_only_with_a_teammate_that_still_participates() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let conversation_id = ChatConversationId::new("conversation:review").unwrap();
        let reply_thread_id = ChatReplyThreadId::new("reply-thread:review").unwrap();

        let invocation =
            resolve_invoked_teammate(&pool, &conversation_id, Some(&reply_thread_id), &[])
                .await
                .unwrap()
                .expect("a reply in an assignment thread continues with its teammate");
        assert_eq!(invocation.teammate_id.as_str(), "participant:review-agent");

        sqlx::query(
            "UPDATE chat_conversation_memberships SET removed_at = '2026-08-04T17:03:00.000Z'
             WHERE participant_id = 'participant:review-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let error = resolve_invoked_teammate(&pool, &conversation_id, Some(&reply_thread_id), &[])
            .await
            .expect_err("a removed teammate is not re-invoked by an unmentioned reply");
        assert_eq!(error.code, ChatErrorCode::Conflict);
    });
}

#[test]
fn plain_at_text_is_valid_but_does_not_invoke_a_teammate() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let request = message("Please ask @atlas to review this.");
        validate_message_request(&request).unwrap();
        let invocation = resolve_invoked_teammate(
            &pool,
            &ChatConversationId::new("conversation:test").unwrap(),
            None,
            &request.references,
        )
        .await
        .unwrap();
        assert!(invocation.is_none());
    });
}

#[test]
fn only_structured_participant_references_make_top_level_messages_thread_eligible() {
    let mut request = message("Ordinary channel message");
    assert!(!has_thread_eligible_mention(&request));

    request.normalized_markdown = "@Atlas Please review this.".to_string();
    request.references = vec![participant_reference(
        "reference:atlas",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiTeammate,
        "Atlas",
        "@Atlas",
        0,
        6,
    )];

    assert!(has_thread_eligible_mention(&request));
}

#[test]
fn active_continuations_reject_new_authority_bearing_references() {
    let participant = participant_reference(
        "reference:atlas",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiTeammate,
        "Atlas",
        "@Atlas",
        0,
        6,
    );
    assert!(!has_authority_bearing_references(std::slice::from_ref(
        &participant
    )));
    require_continuation_scope_is_unchanged(true, &[participant]).unwrap();

    let channel = ChatMessageReference::Channel {
        metadata: ChatReferenceMetadata {
            reference_id: ChatMessageReferenceId::new("reference:source-channel").unwrap(),
            label_snapshot: "source".to_string(),
            start_offset: 0,
            end_offset: 7,
            plain_text_projection: "#source".to_string(),
        },
        channel_id: ChatChannelId::new("channel:source").unwrap(),
    };
    assert!(has_authority_bearing_references(std::slice::from_ref(
        &channel
    )));
    let error =
        require_continuation_scope_is_unchanged(true, std::slice::from_ref(&channel)).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Conflict);
    require_continuation_scope_is_unchanged(false, &[channel]).unwrap();
}

#[test]
fn channel_unread_count_excludes_local_messages() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group:unread', 'Unread', 'a1');
             INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project:unread', 'group:unread', 'Unread', 'a1');
             INSERT INTO chat_conversations
                 (id, project_id, conversation_kind, last_activity_at,
                  created_at, updated_at)
             VALUES (
                 'conversation:unread', 'project:unread', 'channel',
                 '2026-08-04T19:01:00.000Z', '2026-08-04T19:00:00.000Z',
                 '2026-08-04T19:01:00.000Z'
             );
             INSERT INTO chat_channels
                 (id, project_id, conversation_id, name, created_at, updated_at)
             VALUES (
                 'channel:unread', 'project:unread', 'conversation:unread',
                 'general', '2026-08-04T19:00:00.000Z',
                 '2026-08-04T19:01:00.000Z'
             );
             INSERT INTO chat_participants
                 (id, participant_kind, display_name, created_at, updated_at)
             VALUES (
                 'participant:collaborator', 'human', 'Collaborator',
                 '2026-08-04T19:00:00.000Z', '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_conversation_items
                 (id, conversation_id, item_kind, order_key, created_at)
             VALUES
                 ('item:local-unread', 'conversation:unread', 'message', 'a1',
                  '2026-08-04T19:00:00.000Z'),
                 ('item:incoming-unread', 'conversation:unread', 'message', 'a2',
                  '2026-08-04T19:01:00.000Z');
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES
                 ('item:local-unread', 'participant:local-owner', 'You',
                  '2026-08-04T19:00:00.000Z'),
                 ('item:incoming-unread', 'participant:collaborator', 'Collaborator',
                  '2026-08-04T19:01:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let channel = super::super::channels::read_channel(
            &pool,
            &ChatChannelId::new("channel:unread").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(channel.message_count, 2);
        assert_eq!(channel.unread_count, 1);
    });
}

#[test]
fn reply_thread_unread_excludes_local_messages_and_tracks_item_positions() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key)
             VALUES ('group:thread-unread', 'Thread unread', 'a1');
             INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project:thread-unread', 'group:thread-unread', 'Thread unread', 'a1');
             INSERT INTO chat_conversations
                 (id, project_id, conversation_kind, last_activity_at,
                  created_at, updated_at)
             VALUES (
                 'conversation:thread-unread', 'project:thread-unread', 'channel',
                 '2026-08-04T19:03:00.000Z', '2026-08-04T19:00:00.000Z',
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_participants
                 (id, participant_kind, display_name, created_at, updated_at)
             VALUES (
                 'participant:thread-collaborator', 'human', 'Collaborator',
                 '2026-08-04T19:00:00.000Z', '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_conversation_items
                 (id, conversation_id, item_kind, order_key, created_at)
             VALUES (
                 'item:thread-root', 'conversation:thread-unread', 'message', 'a1',
                 '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-root', 'participant:local-owner', 'You',
                 '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-root', 'item:thread-root', 'Root',
                 '2026-08-04T19:00:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-root'
             WHERE item_id = 'item:thread-root';
             INSERT INTO chat_reply_threads
                 (id, conversation_id, root_item_id, last_activity_at,
                  created_at, updated_at)
             VALUES (
                 'reply-thread:unread', 'conversation:thread-unread',
                 'item:thread-root', '2026-08-04T19:02:00.000Z',
                 '2026-08-04T19:00:00.000Z', '2026-08-04T19:02:00.000Z'
             );
             INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES
                 ('item:thread-work', 'conversation:thread-unread',
                  'reply-thread:unread', 'work_update', 'a1',
                  '2026-08-04T19:01:00.000Z'),
                 ('item:thread-local', 'conversation:thread-unread',
                  'reply-thread:unread', 'message', 'a2',
                  '2026-08-04T19:02:00.000Z');
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-local', 'participant:local-owner', 'You',
                 '2026-08-04T19:02:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-local', 'item:thread-local', 'Local reply',
                 '2026-08-04T19:02:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-local'
             WHERE item_id = 'item:thread-local';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let thread_id = ChatReplyThreadId::new("reply-thread:unread").unwrap();

        let local_only = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(!local_only.unread);

        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES (
                 'item:thread-incoming', 'conversation:thread-unread',
                 'reply-thread:unread', 'message', 'a3',
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-incoming', 'participant:thread-collaborator', 'Collaborator',
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-incoming', 'item:thread-incoming',
                 'Incoming reply', '2026-08-04T19:03:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-incoming'
             WHERE item_id = 'item:thread-incoming';
             UPDATE chat_reply_threads
             SET last_activity_at = '2026-08-04T19:03:00.000Z',
                 updated_at = '2026-08-04T19:03:00.000Z'
             WHERE id = 'reply-thread:unread';",
        )
        .execute(&pool)
        .await
        .unwrap();

        let incoming = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(incoming.unread);

        super::reads::read_reply_thread_page(&pool, &thread_id, None, 50)
            .await
            .unwrap();
        let read_position: (String, String) = sqlx::query_as(
            "SELECT last_read_order_key, last_read_item_id FROM chat_reply_thread_read_cursors
             WHERE reply_thread_id = ? AND participant_id = 'participant:local-owner'",
        )
        .bind(thread_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            read_position,
            ("a3".to_owned(), "item:thread-incoming".to_owned())
        );
        let read = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(!read.unread);

        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES (
                 'item:thread-local-after-read', 'conversation:thread-unread',
                 'reply-thread:unread', 'message', 'a4',
                 '2026-08-04T19:04:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-local-after-read', 'participant:local-owner', 'You',
                 '2026-08-04T19:04:00.000Z'
             );",
        )
        .execute(&pool)
        .await
        .unwrap();
        let local_after_read = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(!local_after_read.unread);
    });
}

#[test]
fn two_structured_ai_mentions_are_rejected_before_dispatch() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        for index in 1..=2 {
            sqlx::query(
                "INSERT INTO chat_participants
                    (id, participant_kind, display_name, created_at, updated_at)
                 VALUES (?, 'ai_teammate', ?, ?, ?)",
            )
            .bind(format!("participant:agent-{index}"))
            .bind(format!("Agent {index}"))
            .bind("2026-08-01T00:00:00.000Z")
            .bind("2026-08-01T00:00:00.000Z")
            .execute(&pool)
            .await
            .unwrap();
        }
        let references = (1..=2)
            .map(|index| {
                participant_reference(
                    &format!("reference:agent-{index}"),
                    ChatParticipantId::new(format!("participant:agent-{index}")).unwrap(),
                    ChatParticipantKind::AiTeammate,
                    &format!("Agent {index}"),
                    &format!("@Agent {index}"),
                    u64::try_from((index - 1) * 9).unwrap(),
                    u64::try_from(index * 9).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let error = resolve_invoked_teammate(
            &pool,
            &ChatConversationId::new("conversation:test").unwrap(),
            None,
            &references,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("references"));
    });
}

#[test]
fn message_validation_rejects_duplicate_atomic_mention_ranges() {
    let mut request = message("@Atlas");
    let reference = participant_reference(
        "reference:duplicate",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiTeammate,
        "Atlas",
        "@Atlas",
        0,
        6,
    );
    request.references = vec![reference.clone(), reference];
    let error = validate_message_request(&request).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("references"));
}

#[test]
fn fts_queries_are_bounded_and_quote_each_term() {
    assert_eq!(
        normalized_fts_query("review exact-folder").unwrap(),
        "\"review\" AND \"exact-folder\""
    );
    assert!(normalized_fts_query("   ").is_err());
    assert!(normalized_fts_query(&"x".repeat(501)).is_err());
}

#[test]
fn history_access_admits_positions_from_its_lower_bound() {
    let entire = super::common::TeammateHistoryAccess::resolve(true, false, None);
    assert_eq!(entire.visible_from(), Some(""));
    assert!(entire.admits_position("c000"));

    let from_grant =
        super::common::TeammateHistoryAccess::resolve(true, true, Some("c005".to_owned()));
    assert!(!from_grant.admits_position("c004"));
    assert!(from_grant.admits_position("c005"));
    assert!(from_grant.admits_position("c0050"));

    let unreadable = super::common::TeammateHistoryAccess::resolve(false, false, None);
    assert_eq!(unreadable.visible_from(), None);
    assert!(!unreadable.admits_position("zzzz"));

    let unstamped = super::common::TeammateHistoryAccess::resolve(true, true, None);
    assert_eq!(unstamped.visible_from(), None);
}

/// Freezes the seeded review assignment for a reply that mentions the agent,
/// with the given channel history settings, and returns the source kinds and
/// the excluded thread reply count.
async fn freeze_review_reply_context(
    read_history: bool,
    history_from_order_key: Option<&str>,
) -> (Vec<String>, i64) {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    sqlx::raw_sql(
        "INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:review-later', 'conversation:review', 'message', 'a2',
                 '2026-08-04T17:03:00.000Z');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES ('item:review-later', 'participant:local-owner', 'You',
                 '2026-08-04T17:03:00.000Z');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:review-later', 'item:review-later', 'Later update',
                 '2026-08-04T17:03:00.000Z');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-later'
         WHERE item_id = 'item:review-later';",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE chat_teammate_channel_memberships
         SET read_history = ?, read_history_inherits_profile = 0,
             history_boundary = ?, history_boundary_inherits_profile = 0,
             history_from_order_key = ?
         WHERE teammate_id = 'participant:review-agent'",
    )
    .bind(read_history)
    .bind(if history_from_order_key.is_some() {
        "from_grant"
    } else {
        "entire"
    })
    .bind(history_from_order_key)
    .execute(&pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    super::context::freeze_context_package(
        &mut transaction,
        &ChatWorkAssignmentId::new("assignment:review").unwrap(),
        &ChatReplyThreadId::new("reply-thread:review").unwrap(),
        &ChatConversationItemId::new("item:review-reply-2").unwrap(),
        &ChatParticipantId::new("participant:review-agent").unwrap(),
        None,
        &UtcTimestamp::new("2026-08-04T17:04:00.000Z").unwrap(),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let kinds = sqlx::query_scalar(
        "SELECT source.source_kind || ':' || source.source_id
         FROM chat_assignment_context_sources source
         JOIN chat_assignment_context_packages package ON package.id = source.context_package_id
         WHERE package.assignment_id = 'assignment:review'
         ORDER BY source.ordinal",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let excluded = sqlx::query_scalar(
        "SELECT excluded_thread_reply_count FROM chat_assignment_context_packages
         WHERE assignment_id = 'assignment:review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    (kinds, excluded)
}

#[test]
fn context_packages_include_threads_within_the_history_boundary() {
    tauri::async_runtime::block_on(async {
        let (kinds, excluded) = freeze_review_reply_context(true, None).await;
        assert_eq!(
            kinds,
            [
                "thread_root:item:review-root",
                "thread_reply:item:review-reply-1",
                "trigger:item:review-reply-2",
                "channel_message:item:review-later",
            ]
            .map(String::from)
        );
        assert_eq!(excluded, 0);

        let (kinds, _) = freeze_review_reply_context(true, Some("a1")).await;
        assert!(kinds.contains(&"thread_root:item:review-root".to_string()));
    });
}

#[test]
fn context_packages_exclude_threads_rooted_before_the_history_boundary() {
    tauri::async_runtime::block_on(async {
        let (kinds, excluded) = freeze_review_reply_context(true, Some("a2")).await;
        assert_eq!(
            kinds,
            [
                "trigger:item:review-reply-2",
                "channel_message:item:review-later",
            ]
            .map(String::from)
        );
        assert_eq!(excluded, 0);
    });
}

#[test]
fn context_packages_without_history_access_hold_only_the_trigger() {
    tauri::async_runtime::block_on(async {
        let (kinds, excluded) = freeze_review_reply_context(false, None).await;
        assert_eq!(kinds, ["trigger:item:review-reply-2".to_string()]);
        assert_eq!(excluded, 0);
    });
}

#[test]
fn removing_a_teammate_from_a_channel_keeps_the_rows_and_hides_them_from_readers() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let teammate_id = ChatParticipantId::new("participant:design-agent").unwrap();
        seed_unused_teammate(&pool, teammate_id.as_str(), "policy:design-agent").await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, icon, order_key)
             VALUES ('group-design', 'Design', 'lucide:folder', 'a0');
             INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-design', 'group-design', 'Design', 'lucide:folder', 'a0');
             INSERT INTO chat_conversations
                 (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
             VALUES ('conversation-design', 'project-design', 'channel',
                     '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z',
                     '2026-08-14T17:30:00.000Z');
             INSERT INTO chat_channels
                 (id, project_id, conversation_id, name, is_default, created_at, updated_at)
             VALUES ('channel-design', 'project-design', 'conversation-design', 'design', 0,
                     '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z');
             INSERT INTO chat_conversation_memberships
                 (conversation_id, participant_id, sync_key, membership_role, created_at,
                  updated_at)
             VALUES ('conversation-design', 'participant:design-agent', 'membership:design',
                     'member', '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z');
             INSERT INTO chat_teammate_channel_memberships
                 (conversation_id, teammate_id, sync_key, access_profile_id, created_at,
                  updated_at)
             VALUES ('conversation-design', 'participant:design-agent',
                     'channel-membership:design', 'access-profile:edit-files',
                     '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let conversation_id = ChatConversationId::new("conversation-design").unwrap();
        let before = access::read_teammate_access(&pool, &teammate_id)
            .await
            .unwrap();
        assert_eq!(before.channels.len(), 1);

        let removed_at = UtcTimestamp::new("2026-08-15T09:00:00.000Z").unwrap();
        let mut transaction = pool.begin().await.unwrap();
        access::remove_teammate_from_conversation(
            &mut transaction,
            &teammate_id,
            conversation_id.as_str(),
            &removed_at,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let kept: Option<String> = sqlx::query_scalar(
            "SELECT removed_at FROM chat_teammate_channel_memberships
             WHERE conversation_id = 'conversation-design'
               AND teammate_id = 'participant:design-agent'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kept.as_deref(), Some(removed_at.as_str()));
        let after = access::read_teammate_access(&pool, &teammate_id)
            .await
            .unwrap();
        assert!(after.channels.is_empty());
        let roster = reads::read_memberships_for_conversation(&pool, &conversation_id, false)
            .await
            .unwrap();
        assert!(roster.is_empty());
        let history = reads::read_memberships_for_conversation(&pool, &conversation_id, true)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].removed_at.is_some());
        assert!(
            history[0].ai_access.is_none(),
            "a removed membership carries no channel access"
        );
    });
}

/// Runs the save-time profile revision check for one channel draft made against `revision_id`.
async fn check_draft_profile_revision(
    pool: &SqlitePool,
    revision_id: &str,
) -> Result<(), ChatErrorCode> {
    let draft = ChatTeammateChannelAccessInput {
        channel_id: ChatChannelId::new("channel:draft").unwrap(),
        access_profile_id: ChatAccessProfileId::new("access-profile:draft").unwrap(),
        access_profile_revision_id: ChatAccessProfileRevisionId::new(revision_id).unwrap(),
        capabilities: ChatChannelCapabilities {
            read_history: true,
            participate: true,
        },
        history_boundary: ChatHistoryBoundary::Entire,
        runtime_approval_override: None,
        scratch_runtime_approval_override: None,
        folder_grants: Vec::new(),
    };
    let mut transaction = pool.begin().await.unwrap();
    access::require_expected_profile_revisions_in_transaction(&mut transaction, &[draft])
        .await
        .map_err(|error| error.code)
}

#[test]
fn saved_access_drafts_compare_effective_profile_revision_ids_not_numbers() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        sqlx::raw_sql(
            "INSERT INTO chat_access_profiles (id, display_name, created_at, updated_at)
             VALUES ('access-profile:draft', 'Draft profile', '2026-10-24T09:00:00.000Z',
                     '2026-10-24T09:00:00.000Z');
             INSERT INTO chat_access_profile_revisions
                (id, access_profile_id, default_read_history, default_participate,
                 default_history_boundary, maximum_folder_capability, created_at)
             VALUES ('revision:first', 'access-profile:draft', 1, 1, 'entire', 'edit',
                     '2026-10-24T09:01:00.000Z'),
                    ('revision:seen', 'access-profile:draft', 1, 1, 'entire', 'publish',
                     '2026-10-24T09:03:00.000Z');
             UPDATE chat_access_profiles
             SET current_revision_id = 'revision:seen',
                 current_maximum_folder_capability = 'publish'
             WHERE id = 'access-profile:draft';",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            check_draft_profile_revision(&pool, "revision:seen").await,
            Ok(())
        );

        // A synced revision created between the two shifts the numbers, so the stricter one the
        // pointer now names carries number 2, the number the draft saw.
        sqlx::raw_sql(
            "INSERT INTO chat_access_profile_revisions
                (id, access_profile_id, default_read_history, default_participate,
                 default_history_boundary, maximum_folder_capability, created_at)
             VALUES ('revision:synced', 'access-profile:draft', 1, 1, 'entire', 'read',
                     '2026-10-24T09:02:00.000Z');
             UPDATE chat_access_profiles
             SET current_revision_id = 'revision:synced',
                 current_maximum_folder_capability = 'read'
             WHERE id = 'access-profile:draft';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let synced_number: i64 = sqlx::query_scalar(
            "SELECT revision FROM chat_access_profile_effective_revisions
             WHERE access_profile_id = 'access-profile:draft'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(synced_number, 2);
        assert_eq!(
            check_draft_profile_revision(&pool, "revision:seen").await,
            Err(ChatErrorCode::StaleRevision)
        );
        assert_eq!(
            check_draft_profile_revision(&pool, "revision:synced").await,
            Ok(())
        );
    });
}

/// Seeds root messages of the review channel as `(item id, order key, deleted)`.
async fn seed_review_root_messages(pool: &SqlitePool, messages: &[(&str, &str, bool)]) {
    for (item_id, order_key, deleted) in messages {
        let revision_id = format!("revision:{item_id}");
        sqlx::query(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, item_kind, order_key, created_at)
             VALUES (?, 'conversation:review', 'message', ?, '2026-08-04T17:05:00.000Z')",
        )
        .bind(item_id)
        .bind(order_key)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, deleted_at, created_at)
             VALUES (?, 'participant:local-owner', 'You', ?, '2026-08-04T17:05:00.000Z')",
        )
        .bind(item_id)
        .bind(deleted.then_some("2026-08-04T17:06:00.000Z"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (?, ?, ?, '2026-08-04T17:05:00.000Z')",
        )
        .bind(&revision_id)
        .bind(item_id)
        .bind(format!("Text of {item_id}"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?",
        )
        .bind(&revision_id)
        .bind(item_id)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[test]
fn channel_pages_break_equal_order_keys_by_item_id_without_gaps() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_review_root_messages(
            &pool,
            &[("item:tie-b", "a2", false), ("item:tie-a", "a2", false)],
        )
        .await;
        let channel_id = ChatChannelId::new("channel:review").unwrap();
        let ids = |page: &ChatChannelPageRead| {
            page.messages
                .iter()
                .map(|message| message.item_id.as_str().to_owned())
                .collect::<Vec<_>>()
        };

        let newest = read_channel_page(&pool, channel_id.clone(), None, Some(1))
            .await
            .unwrap();
        assert_eq!(ids(&newest), ["item:tie-b"]);
        assert_eq!(newest.messages[0].order_key, "a2");
        let middle = read_channel_page(
            &pool,
            channel_id.clone(),
            newest.previous_cursor.as_deref(),
            Some(1),
        )
        .await
        .unwrap();
        assert_eq!(ids(&middle), ["item:tie-a"]);
        let oldest = read_channel_page(
            &pool,
            channel_id.clone(),
            middle.previous_cursor.as_deref(),
            Some(2),
        )
        .await
        .unwrap();
        assert_eq!(ids(&oldest), ["item:review-root"]);
        assert_eq!(oldest.previous_cursor, None);

        let through = super::common::PageCursor::through("a2", "item:tie-a");
        let anchored = read_channel_page(&pool, channel_id, Some(&through), Some(5))
            .await
            .unwrap();
        assert_eq!(ids(&anchored), ["item:review-root", "item:tie-a"]);
    });
}

#[test]
fn reply_thread_summaries_count_only_live_reply_messages() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let thread_id = ChatReplyThreadId::new("reply-thread:review").unwrap();
        let summary = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert_eq!(summary.reply_count, 2);

        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES ('item:review-update', 'conversation:review', 'reply-thread:review',
                     'work_update', 'a3', '2026-08-04T17:03:00.000Z');
             UPDATE chat_communication_messages SET deleted_at = '2026-08-04T17:04:00.000Z'
             WHERE item_id = 'item:review-reply-1';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let summary = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert_eq!(summary.reply_count, 1);
    });
}

#[test]
fn channel_reference_ranges_break_equal_order_keys_at_both_ends() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_review_root_messages(
            &pool,
            &[
                ("item:tie-a", "a2", false),
                ("item:tie-b", "a2", false),
                ("item:tie-c", "a2", true),
            ],
        )
        .await;
        sqlx::raw_sql(
            "UPDATE chat_teammate_channel_memberships
             SET history_boundary = 'from_grant', history_boundary_inherits_profile = 0,
                 history_from_order_key = 'a2'
             WHERE teammate_id = 'participant:review-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut request = message("See general");
        request.channel_id = ChatChannelId::new("channel:review").unwrap();
        request.reply_thread_id = Some(ChatReplyThreadId::new("reply-thread:review").unwrap());
        request.references = vec![ChatMessageReference::Channel {
            metadata: ChatReferenceMetadata {
                reference_id: ChatMessageReferenceId::new("reference:general").unwrap(),
                label_snapshot: "general".to_owned(),
                start_offset: 4,
                end_offset: 11,
                plain_text_projection: "#general".to_owned(),
            },
            channel_id: ChatChannelId::new("channel:review").unwrap(),
        }];
        let teammate_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let mut transaction = pool.begin().await.unwrap();
        super::workflow::insert_communication_message(
            &mut transaction,
            super::workflow::CommunicationMessageWrite {
                item_id: &ChatConversationItemId::new("item:review-reference").unwrap(),
                revision_id: &ChatMessageRevisionId::new("revision:review-reference").unwrap(),
                conversation_id: &ChatConversationId::new("conversation:review").unwrap(),
                reply_thread_id: request.reply_thread_id.as_ref(),
                order_key: "a3",
                request: &request,
                attachments: super::common::AttachmentSource::Device(&request.attachment_ids),
                invoked_teammate_id: Some(&teammate_id),
                now: &UtcTimestamp::new("2026-08-04T17:07:00.000Z").unwrap(),
            },
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let bounds: (String, String, String, String) = sqlx::query_as(
            "SELECT source_lower_order_key, source_lower_item_id,
                    source_high_order_key, source_high_item_id
             FROM chat_channel_reference_targets WHERE reference_id = 'reference:general'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            bounds,
            (
                "a2".to_owned(),
                String::new(),
                "a2".to_owned(),
                "item:tie-b".to_owned()
            )
        );

        seed_review_root_messages(&pool, &[("item:tie-z", "a2", false)]).await;
        let in_range: Vec<String> = sqlx::query_scalar(
            "SELECT item.id FROM chat_conversation_items item
             JOIN chat_channel_reference_targets target
               ON target.reference_id = 'reference:general'
             WHERE item.conversation_id = target.source_conversation_id
               AND item.reply_thread_id IS NULL
               AND (item.order_key, item.id)
                   >= (target.source_lower_order_key, target.source_lower_item_id)
               AND (item.order_key, item.id)
                   <= (target.source_high_order_key, target.source_high_item_id)
             ORDER BY item.order_key, item.id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(in_range, ["item:tie-a", "item:tie-b"]);
    });
}

#[test]
fn message_search_keeps_messages_whose_author_identity_is_gone() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_review_root_messages(
            &pool,
            &[
                ("item:attributed", "a5", false),
                ("item:unattributed", "a6", false),
            ],
        )
        .await;
        sqlx::raw_sql(
            "UPDATE chat_communication_messages SET author_participant_id = NULL
             WHERE item_id = 'item:unattributed';
             UPDATE chat_participants SET display_name = 'Renamed owner'
             WHERE id = 'participant:local-owner';",
        )
        .execute(&pool)
        .await
        .unwrap();

        let results = search_messages(
            &pool,
            normalized_fts_query("Text of item").unwrap(),
            Some("project:review"),
            None,
        )
        .await
        .unwrap();
        let result = |item_id: &str| {
            results
                .iter()
                .find(|result| result.message_item_id.as_str() == item_id)
                .unwrap_or_else(|| panic!("search dropped {item_id}"))
        };

        let attributed = result("item:attributed");
        assert_eq!(
            attributed
                .author_participant_id
                .as_ref()
                .map(ChatParticipantId::as_str),
            Some("participant:local-owner")
        );
        assert_eq!(attributed.author_kind, Some(ChatParticipantKind::LocalUser));
        assert_eq!(attributed.author_display_name, "Renamed owner");
        let unattributed = result("item:unattributed");
        assert_eq!(unattributed.author_participant_id, None);
        assert_eq!(unattributed.author_kind, None);
        assert_eq!(unattributed.author_display_name, "You");
    });
}

/// Removes the seeded review assignment so the review thread holds plain replies only.
async fn clear_review_assignment(pool: &SqlitePool) {
    sqlx::raw_sql(
        "DELETE FROM chat_work_assignment_inputs;
         DELETE FROM chat_work_assignments;",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn set_review_agent_home_device(pool: &SqlitePool, home_device_id: Option<&str>) {
    sqlx::query(
        "UPDATE chat_teammates SET home_device_id = ?
         WHERE participant_id = 'participant:review-agent'",
    )
    .bind(home_device_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Routes the review reply `item:review-reply-1` to the review agent as `device` posts it.
async fn route_review_reply(
    pool: &SqlitePool,
    device: super::workflow::PostingDevice<'_>,
) -> super::workflow::AssignmentWrite {
    let invocation = super::workflow::ResolvedInvocation {
        teammate_id: ChatParticipantId::new("participant:review-agent").unwrap(),
        active_assignment: None,
        latest_assignment: None,
    };
    let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
    let mut transaction = pool.begin().await.unwrap();
    let write = super::workflow::persist_assignment_routing(
        &mut transaction,
        &ChatReplyThreadId::new("reply-thread:review").unwrap(),
        &ChatConversationItemId::new("item:review-reply-1").unwrap(),
        Some(&invocation),
        None,
        device,
        &now,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    write
}

/// The executing device and dispatch job count of the assignment the review reply triggers.
async fn routed_assignment(pool: &SqlitePool) -> Option<(Option<String>, i64)> {
    sqlx::query(
        "SELECT assignment.executing_device_id,
                (SELECT count(*) FROM chat_assignment_dispatch_jobs job
                 WHERE job.assignment_id = assignment.id) AS dispatch_jobs
         FROM chat_work_assignments assignment
         WHERE assignment.triggering_message_item_id = 'item:review-reply-1'",
    )
    .fetch_optional(pool)
    .await
    .unwrap()
    .map(|row| {
        (
            row.get::<Option<String>, _>("executing_device_id"),
            row.get::<i64, _>("dispatch_jobs"),
        )
    })
}

async fn review_thread_without_assignment() -> SqlitePool {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    clear_review_assignment(&pool).await;
    pool
}

#[test]
fn a_new_assignment_runs_on_the_home_device_else_the_posting_desktop_else_nowhere() {
    tauri::async_runtime::block_on(async {
        let phone = super::workflow::PostingDevice {
            id: "device:phone",
            can_execute: false,
        };
        let desk = super::workflow::PostingDevice {
            id: "device:desk",
            can_execute: true,
        };

        let pool = review_thread_without_assignment().await;
        let write = route_review_reply(&pool, phone).await;
        assert!(
            write.assignment_id.is_none(),
            "the mention is recorded only"
        );
        assert!(!write.executes_here);
        assert_eq!(routed_assignment(&pool).await, None);

        let pool = review_thread_without_assignment().await;
        set_review_agent_home_device(&pool, Some("device:desk")).await;
        let write = route_review_reply(&pool, phone).await;
        assert!(write.assignment_id.is_some());
        assert!(
            !write.executes_here,
            "the home device dispatches on arrival"
        );
        assert_eq!(
            routed_assignment(&pool).await,
            Some((Some("device:desk".to_owned()), 0))
        );

        let pool = review_thread_without_assignment().await;
        let write = route_review_reply(&pool, desk).await;
        assert!(write.executes_here);
        assert_eq!(
            routed_assignment(&pool).await,
            Some((Some("device:desk".to_owned()), 1))
        );

        let pool = review_thread_without_assignment().await;
        set_review_agent_home_device(&pool, Some("device:laptop")).await;
        let write = route_review_reply(&pool, desk).await;
        assert!(
            !write.executes_here,
            "the home device outranks the posting desktop"
        );
        assert_eq!(
            routed_assignment(&pool).await,
            Some((Some("device:laptop".to_owned()), 0))
        );
    });
}

#[test]
fn only_the_executing_device_claims_dispatch_and_writes_active_states() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let assignment_id = ChatWorkAssignmentId::new("assignment:review").unwrap();
        sqlx::query(
            "INSERT INTO chat_assignment_dispatch_jobs
                (id, assignment_id, state, available_at, created_at, updated_at)
             VALUES ('dispatch:review', 'assignment:review', 'queued', ?1, ?1, ?1)",
        )
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();

        let refused = assignments::require_executing_device(&pool, &assignment_id, "device:phone")
            .await
            .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
        assignments::require_executing_device(&pool, &assignment_id, "device:desk")
            .await
            .unwrap();
        let claim =
            super::dispatch::claim_dispatch_job(&pool, &assignment_id, "device:phone", &now)
                .await
                .unwrap();
        assert!(claim.is_none(), "another device never claims the dispatch");
        let claim = super::dispatch::claim_dispatch_job(&pool, &assignment_id, "device:desk", &now)
            .await
            .unwrap();
        assert!(claim.is_some());

        // An assignment written before executing devices existed names none.
        sqlx::raw_sql(
            "DELETE FROM chat_assignment_dispatch_jobs;
             DELETE FROM chat_work_assignment_inputs;
             DELETE FROM chat_work_assignments;
             INSERT INTO chat_work_assignments
                 (id, reply_thread_id, teammate_id, triggering_message_item_id,
                  executing_device_id, state, created_at, updated_at)
             VALUES ('assignment:review', 'reply-thread:review', 'participant:review-agent',
                     'item:review-root', NULL, 'queued', '2026-08-04T17:00:00.000Z',
                     '2026-08-04T17:00:00.000Z');
             INSERT INTO chat_assignment_dispatch_jobs
                 (id, assignment_id, state, available_at, created_at, updated_at)
             VALUES ('dispatch:review', 'assignment:review', 'queued',
                     '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z',
                     '2026-08-04T17:00:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        assignments::require_executing_device(&pool, &assignment_id, "device:phone")
            .await
            .expect("an assignment without an executing device runs where its job is");
        sqlx::query("DELETE FROM chat_assignment_dispatch_jobs")
            .execute(&pool)
            .await
            .unwrap();
        let refused = assignments::require_executing_device(&pool, &assignment_id, "device:phone")
            .await
            .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
    });
}

#[test]
fn start_creates_the_assignment_of_an_unrouted_mention_on_this_device() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        sqlx::raw_sql(
            "INSERT INTO chat_message_references
                 (id, message_revision_id, reference_kind, label_snapshot,
                  plain_text_projection, start_offset, end_offset, ordinal, created_at)
             VALUES ('reference:review-agent', 'revision:review-reply-2', 'participant',
                     'review-agent', '@review-agent', 0, 13, 0, '2026-08-04T17:02:00.000Z');
             INSERT INTO chat_participant_reference_targets (reference_id, participant_id)
             VALUES ('reference:review-agent', 'participant:review-agent');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let teammate_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();

        let unmentioned = assignments::persist_started_assignment(
            &pool,
            &ChatConversationItemId::new("item:review-reply-1").unwrap(),
            &teammate_id,
            "device:desk",
            &now,
        )
        .await
        .unwrap_err();
        assert_eq!(unmentioned.code, ChatErrorCode::Validation);

        let mention = ChatConversationItemId::new("item:review-reply-2").unwrap();
        let assignment_id = assignments::persist_started_assignment(
            &pool,
            &mention,
            &teammate_id,
            "device:desk",
            &now,
        )
        .await
        .unwrap();
        let started = sqlx::query(
            "SELECT assignment.executing_device_id, assignment.triggering_message_item_id,
                    input.id AS input_id,
                    (SELECT count(*) FROM chat_assignment_dispatch_jobs job
                     WHERE job.assignment_id = assignment.id AND job.state = 'queued')
                        AS queued_jobs
             FROM chat_work_assignments assignment
             JOIN chat_work_assignment_inputs input ON input.assignment_id = assignment.id
             WHERE assignment.id = ?",
        )
        .bind(assignment_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            started.get::<String, _>("executing_device_id"),
            "device:desk"
        );
        assert_eq!(
            started.get::<String, _>("triggering_message_item_id"),
            "item:review-reply-2"
        );
        assert_eq!(
            started.get::<String, _>("input_id"),
            super::common::assignment_input_id("item:review-reply-2").unwrap()
        );
        assert_eq!(started.get::<i64, _>("queued_jobs"), 1);

        let again = assignments::persist_started_assignment(
            &pool,
            &mention,
            &teammate_id,
            "device:laptop",
            &now,
        )
        .await
        .unwrap_err();
        assert_eq!(again.code, ChatErrorCode::Conflict);
    });
}
