use super::common::{
    has_thread_eligible_mention, is_valid_teammate_effort, map_teammate_write_error,
    normalized_fts_query, validate_message_request, validate_teammate_role,
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
    ganbaru_db::run_migrations(&pool).await.unwrap();
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
            (participant_id, role, instructions, latest_policy_revision, created_at, updated_at)
         VALUES (?, 'General support', '', 1, ?, ?)",
    )
    .bind(teammate_id)
    .bind("2026-08-14T17:30:00.000Z")
    .bind("2026-08-14T17:30:00.000Z")
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_teammate_policy_revisions
            (id, teammate_id, revision, provider_instance_id,
             model_selection_data, created_at)
         VALUES (?, ?, 1, 'provider:test',
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

        let error = sqlx::query(
            "INSERT INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES (?, 'ai_teammate', ?, ?, ?)",
        )
        .bind("participant:active-reviewer")
        .bind(" review lead ")
        .bind("2026-08-14T17:31:00.000Z")
        .bind("2026-08-14T17:31:00.000Z")
        .execute(&pool)
        .await
        .unwrap_err();

        let mapped = map_teammate_write_error(error);
        assert_eq!(mapped.code, ChatErrorCode::Validation);
        assert_eq!(mapped.field.as_deref(), Some("displayName"));
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
        "INSERT INTO project_groups (id, name) VALUES ('group:review', 'Review');
         INSERT INTO projects (id, group_id, name)
         VALUES ('project:review', 'group:review', 'Review');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path)
         VALUES ('folder:review', 'project:review', 'Review', 'managed', 'projects/review');
         UPDATE chat_conversations SET id = 'conversation:review'
         WHERE project_id = 'project:review' AND conversation_kind = 'channel';
         UPDATE chat_channels SET id = 'channel:review'
         WHERE project_id = 'project:review' AND is_default = 1;
         INSERT INTO chat_participants
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
             (id, teammate_id, revision, provider_instance_id,
              model_selection_data, created_at)
         VALUES (
             'policy:review', 'participant:review-agent', 1, 'provider:review',
             '{\"providerManagedModel\":true,\"modelId\":null,\"modelOptions\":[]}',
             '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_conversation_memberships
             (conversation_id, participant_id, membership_role, created_at, updated_at)
         VALUES (
             'conversation:review', 'participant:review-agent', 'member',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_teammate_channel_memberships
             (conversation_id, teammate_id, access_profile_id,
              read_history, read_history_inherits_profile,
              participate, participate_inherits_profile,
              history_boundary, history_boundary_inherits_profile,
              created_at, updated_at)
         VALUES (
             'conversation:review', 'participant:review-agent',
             'access-profile:build-and-test', 1, 1, 1, 1, 'entire', 1,
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_teammate_working_folder_grants
             (conversation_id, teammate_id, project_id, working_folder_id,
              capability, capability_inherits_profile, is_default, created_at)
         VALUES (
             'conversation:review', 'participant:review-agent', 'project:review',
             'folder:review', 'execute', 1, 1, '2026-08-04T17:00:00.000Z'
         );
         UPDATE chat_teammate_access_state
         SET access_revision = 1, updated_at = '2026-08-04T17:00:00.000Z'
         WHERE teammate_id = 'participant:review-agent';
         INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, ordinal, created_at)
         VALUES (
             'item:review-root', 'conversation:review', 'message', 1,
             '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (
             'item:review-root', 'participant:local-owner', 'You',
             '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, revision, normalized_markdown, created_at)
         VALUES (
             'revision:review-root', 'item:review-root', 1, 'Create hello.py',
             '2026-08-04T17:00:00.000Z'
         );
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-root'
         WHERE item_id = 'item:review-root';
         INSERT INTO chat_reply_threads
             (id, conversation_id, root_item_id, reply_count, last_activity_at,
              created_at, updated_at)
         VALUES (
             'reply-thread:review', 'conversation:review', 'item:review-root', 2,
             '2026-08-04T17:02:00.000Z', '2026-08-04T17:00:00.000Z',
             '2026-08-04T17:02:00.000Z'
         );
         INSERT INTO chat_conversation_items
             (id, conversation_id, reply_thread_id, item_kind, ordinal, created_at)
         VALUES
             ('item:review-reply-1', 'conversation:review', 'reply-thread:review',
              'message', 1, '2026-08-04T17:01:00.000Z'),
             ('item:review-reply-2', 'conversation:review', 'reply-thread:review',
              'message', 2, '2026-08-04T17:02:00.000Z');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES
             ('item:review-reply-1', 'participant:local-owner', 'You',
              '2026-08-04T17:01:00.000Z'),
             ('item:review-reply-2', 'participant:local-owner', 'You',
              '2026-08-04T17:02:00.000Z');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, revision, normalized_markdown, created_at)
         VALUES
             ('revision:review-reply-1', 'item:review-reply-1', 1,
              'Ok, remove it now.', '2026-08-04T17:01:00.000Z'),
             ('revision:review-reply-2', 'item:review-reply-2', 1,
              '@review-agent Ok, remove it now.', '2026-08-04T17:02:00.000Z');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-reply-1'
         WHERE item_id = 'item:review-reply-1';
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-reply-2'
         WHERE item_id = 'item:review-reply-2';
         INSERT INTO chat_work_assignments
             (id, reply_thread_id, teammate_id, triggering_message_item_id,
              state, created_at, updated_at)
         VALUES (
             'assignment:review', 'reply-thread:review', 'participant:review-agent',
             'item:review-root', 'ready_for_review',
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
fn unused_archived_teammates_can_be_restored_or_permanently_deleted() {
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
        assert!(!archived.has_durable_history);
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

        let archived_again = super::teammate_lifecycle::set_teammate_archived(
            &pool,
            &teammate_id,
            restored.participant.revision,
            true,
        )
        .await
        .unwrap();
        super::teammate_lifecycle::delete_unused_teammate(
            &pool,
            &teammate_id,
            archived_again.participant.revision,
        )
        .await
        .unwrap();

        let participant_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_participants WHERE id = ?")
                .bind(teammate_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        let policy_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_teammate_policy_revisions WHERE teammate_id = ?",
        )
        .bind(teammate_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(participant_count, 0);
        assert_eq!(policy_count, 0);
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
fn durable_history_blocks_permanent_teammate_deletion() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        sqlx::query(
            "UPDATE chat_work_assignments SET state = 'completed' WHERE id = 'assignment:review'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let teammate_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let archived =
            super::teammate_lifecycle::set_teammate_archived(&pool, &teammate_id, 1, true)
                .await
                .unwrap();
        assert!(archived.has_durable_history);
        let error = super::teammate_lifecycle::delete_unused_teammate(
            &pool,
            &teammate_id,
            archived.participant.revision,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict);
        let participant_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_participants WHERE id = ?")
                .bind(teammate_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(participant_count, 1);
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

#[test]
fn immediate_scheduled_delivery_claim_is_atomic() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        sqlx::query("INSERT INTO project_groups (id, name) VALUES ('group:test', 'Test')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name)
             VALUES ('project:test', 'group:test', 'Test')",
        )
        .execute(&pool)
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
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_channels
                (id, project_id, conversation_id, name, created_at, updated_at)
             VALUES ('channel:test', 'project:test', 'conversation:test', 'general', ?, ?)",
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();
        let request_data = serde_json::to_string(&message("Send this now")).unwrap();
        sqlx::query(
            "INSERT INTO chat_scheduled_messages
                (id, client_command_id, channel_id, request_data, scheduled_for,
                 available_at, created_at, updated_at)
             VALUES ('scheduled:test', 'command:test', 'channel:test', ?, ?, ?, ?, ?)",
        )
        .bind(&request_data)
        .bind("2026-08-04T00:00:00.000Z")
        .bind("2026-08-04T00:00:00.000Z")
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        let claimed = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            &now,
            "claim:first",
        )
        .await
        .unwrap();
        assert_eq!(claimed, request_data);
        let row = sqlx::query(
            "SELECT state, attempt_count, claim_token
             FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("state"), "dispatching");
        assert_eq!(row.get::<i64, _>("attempt_count"), 1);
        assert_eq!(row.get::<String, _>("claim_token"), "claim:first");
        let duplicate = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            &now,
            "claim:second",
        )
        .await
        .unwrap_err();
        assert_eq!(duplicate.code, ChatErrorCode::Busy);
    });
}

#[test]
fn restart_rehomes_review_replies_into_one_ordered_follow_up_assignment() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();

        let recovered = assignments::recover_stranded_follow_up_assignments(&pool, &now)
            .await
            .unwrap();

        assert_eq!(recovered, 1);
        let old_state: String = sqlx::query_scalar(
            "SELECT state FROM chat_work_assignments WHERE id = 'assignment:review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_state, "completed");
        let follow_up = sqlx::query(
            "SELECT id, triggering_message_item_id, previous_assignment_id, state
             FROM chat_work_assignments WHERE previous_assignment_id = 'assignment:review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let follow_up_id = follow_up.get::<String, _>("id");
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
            "INSERT INTO project_groups (id, name) VALUES ('group:unread', 'Unread');
             INSERT INTO projects (id, group_id, name)
             VALUES ('project:unread', 'group:unread', 'Unread');
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
                 (id, conversation_id, item_kind, ordinal, created_at)
             VALUES
                 ('item:local-unread', 'conversation:unread', 'message', 1,
                  '2026-08-04T19:00:00.000Z'),
                 ('item:incoming-unread', 'conversation:unread', 'message', 2,
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
fn reply_thread_unread_excludes_local_messages_and_tracks_item_ordinals() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name)
             VALUES ('group:thread-unread', 'Thread unread');
             INSERT INTO projects (id, group_id, name)
             VALUES ('project:thread-unread', 'group:thread-unread', 'Thread unread');
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
                 (id, conversation_id, item_kind, ordinal, created_at)
             VALUES (
                 'item:thread-root', 'conversation:thread-unread', 'message', 1,
                 '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-root', 'participant:local-owner', 'You',
                 '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, revision, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-root', 'item:thread-root', 1, 'Root',
                 '2026-08-04T19:00:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-root'
             WHERE item_id = 'item:thread-root';
             INSERT INTO chat_reply_threads
                 (id, conversation_id, root_item_id, reply_count, last_activity_at,
                  created_at, updated_at)
             VALUES (
                 'reply-thread:unread', 'conversation:thread-unread',
                 'item:thread-root', 1, '2026-08-04T19:02:00.000Z',
                 '2026-08-04T19:00:00.000Z', '2026-08-04T19:02:00.000Z'
             );
             INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, ordinal, created_at)
             VALUES
                 ('item:thread-work', 'conversation:thread-unread',
                  'reply-thread:unread', 'work_update', 1,
                  '2026-08-04T19:01:00.000Z'),
                 ('item:thread-local', 'conversation:thread-unread',
                  'reply-thread:unread', 'message', 2,
                  '2026-08-04T19:02:00.000Z');
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-local', 'participant:local-owner', 'You',
                 '2026-08-04T19:02:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, revision, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-local', 'item:thread-local', 1, 'Local reply',
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
                 (id, conversation_id, reply_thread_id, item_kind, ordinal, created_at)
             VALUES (
                 'item:thread-incoming', 'conversation:thread-unread',
                 'reply-thread:unread', 'message', 3,
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-incoming', 'participant:thread-collaborator', 'Collaborator',
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, revision, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-incoming', 'item:thread-incoming', 1,
                 'Incoming reply', '2026-08-04T19:03:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-incoming'
             WHERE item_id = 'item:thread-incoming';
             UPDATE chat_reply_threads
             SET reply_count = 2, last_activity_at = '2026-08-04T19:03:00.000Z',
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
        let read_ordinal: i64 = sqlx::query_scalar(
            "SELECT last_read_reply_ordinal FROM chat_reply_thread_read_cursors
             WHERE reply_thread_id = ? AND participant_id = 'participant:local-owner'",
        )
        .bind(thread_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(read_ordinal, 3);
        let read = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(!read.unread);

        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, ordinal, created_at)
             VALUES (
                 'item:thread-local-after-read', 'conversation:thread-unread',
                 'reply-thread:unread', 'message', 4,
                 '2026-08-04T19:04:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-local-after-read', 'participant:local-owner', 'You',
                 '2026-08-04T19:04:00.000Z'
             );
             UPDATE chat_reply_threads SET reply_count = 3
             WHERE id = 'reply-thread:unread';",
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
