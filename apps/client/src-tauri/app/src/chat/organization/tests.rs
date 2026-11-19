use super::common::{has_thread_eligible_mention, normalized_fts_query, validate_message_request};
use super::scheduling::{
    SCHEDULED_REQUEST_MISMATCH_REASON, ScheduledRowBinding,
    claim_scheduled_message_for_immediate_send, insert_scheduled_reference,
    refuse_mismatched_request, scheduled_consent_in, scheduled_consents_without_a_row,
    scheduled_reference_target, scheduled_request_mismatch, validate_scheduled_for,
};
use super::workflow::{has_authority_bearing_references, require_continuation_scope_is_unchanged};
use super::*;
use crate::chat::device_state::{
    ChatDeviceScope, forget_scheduled_message_consent, record_assignment_acceptance,
    record_scheduled_message_consent,
};
use ganbaru_chat::coordination::acceptance::scheduled_consent_digest;
use serde_json::json;
use sqlx::{Row, SqlitePool};

mod acceptance;
mod agents;
mod channel_reads;
mod context_packages;
mod follow_up_recovery;
mod messages;
mod review;
mod routing;
mod scheduled;
mod takeover;
mod targetless_dispatch;
mod unattended_scheduled;

pub(super) async fn migrated_pool() -> SqlitePool {
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

pub(super) fn message(markdown: &str) -> PostChatMessageCommand {
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
        followup_routing: None,
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

/// A device scope that accepted what `assignment_id` would run.
async fn scope_accepting(pool: &SqlitePool, assignment_id: &str) -> ChatDeviceScope {
    let digest = assignments::assignment_acceptance_digest(
        pool,
        &ChatWorkAssignmentId::new(assignment_id).unwrap(),
        &test_providers(),
    )
    .await
    .unwrap();
    let mut scope = ChatDeviceScope::default();
    record_assignment_acceptance(
        &mut scope,
        digest,
        UtcTimestamp::new("2026-08-04T17:00:00.000Z").unwrap(),
    );
    scope
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
             'participant:review-agent', 'ai_agent', 'Review agent',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_agents
             (participant_id, provider_family_id, created_at, updated_at)
         VALUES (
             'participant:review-agent', 'codex',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_agent_policy_revisions
             (id, agent_id, model_selection_data, created_at)
         VALUES (
             'policy:review', 'participant:review-agent',
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
         INSERT INTO chat_agent_channel_memberships
             (conversation_id, agent_id, sync_key,
              created_at, updated_at)
         VALUES (
             'conversation:review', 'participant:review-agent', 'channel-membership:review',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         UPDATE chat_agents SET current_policy_revision_id = 'policy:review'
         WHERE participant_id = 'participant:review-agent';
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
             (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
              executing_device_id, state, created_at, updated_at)
         VALUES (
             'assignment:review', 'reply-thread:review', 'participant:review-agent',
             'item:review-root', 1, json_object('providerManagedModel', json('true')), 'device:desk', 'ready_for_review',
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

/// The random id of an agent a person named in the removal tests.
const NAMED_AGENT_ID: &str = "agent:0123456789abcdef0123456789abcdef";

/// Device that scheduled the test messages.
pub(super) const SCHEDULING_DEVICE: &str = "device:scheduling";

/// The Chat writers `device_ids`.
pub(super) fn chat_writers_of(device_ids: &[&str]) -> super::workflow::ChatWriters {
    super::workflow::ChatWriters::new(device_ids.iter().map(|id| (*id).to_owned()))
        .with_providers(test_providers())
}

/// Provider family of the test agents.
pub(super) const TEST_FAMILY: &str = "codex";

/// Provider instance that runs the test agents.
pub(super) const TEST_INSTANCE: &str = "provider:review";

/// Support that enforces every boundary.
pub(super) fn full_support() -> ProviderAuthoritySupport {
    ProviderAuthoritySupport {
        isolated_conversation: true,
        internal_host_tools: true,
        deny_shell: true,
        read_only_root: true,
        writable_root: true,
        confined_commands: true,
        network_boundary: true,
        classified_publish: true,
        person_approved_commands: true,
    }
}

/// The providers of a test device: the test family and `claude`, ready, enforcing every boundary.
pub(super) fn test_providers() -> super::provider_agents::DeviceProviders {
    test_providers_with(full_support())
}

/// The providers of a test device whose ready test family enforces `support`, beside a ready
/// `claude` provider that enforces every boundary.
pub(super) fn test_providers_with(
    support: ProviderAuthoritySupport,
) -> super::provider_agents::DeviceProviders {
    super::provider_agents::DeviceProviders::from_entries([
        (
            TEST_FAMILY.to_string(),
            super::provider_agents::DeviceProvider {
                instance_id: ProviderInstanceId::new(TEST_INSTANCE).unwrap(),
                support,
                ready: true,
            },
        ),
        (
            "claude".to_string(),
            super::provider_agents::DeviceProvider {
                instance_id: ProviderInstanceId::new("provider:other").unwrap(),
                support: full_support(),
                ready: true,
            },
        ),
    ])
}

/// Stores `request_data` as the scheduled message `scheduled:test` executed by `executing`,
/// with the local dispatch row the scheduling device keeps when `dispatches_here`.
pub(super) async fn insert_test_scheduled_message(
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

/// The dispatch job count of the assignment of `assignment_id`.
async fn dispatch_job_count(pool: &SqlitePool, assignment_id: &ChatWorkAssignmentId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?")
        .bind(assignment_id.as_str())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Stores and claims the scheduled message `scheduled:test` as `claim:a` on `device:desk`, a
/// mention of the review agent, returning its request.
async fn claim_scheduled_mention(
    pool: &SqlitePool,
    now: &UtcTimestamp,
) -> (PostChatMessageCommand, ChatScheduledMessageId) {
    claim_scheduled_mention_on(pool, now, None).await
}

/// Like `claim_scheduled_mention`, with the request choosing `execution_target`.
async fn claim_scheduled_mention_on(
    pool: &SqlitePool,
    now: &UtcTimestamp,
    execution_target: Option<ChatExecutionTarget>,
) -> (PostChatMessageCommand, ChatScheduledMessageId) {
    let mut request = message("@Review agent check this");
    request.execution_target = execution_target;
    request.client_command_id = ChatCommandId::new("command:test").unwrap();
    request.channel_id = ChatChannelId::new("channel:review").unwrap();
    request.references = vec![participant_reference(
        "reference:scheduled",
        ChatParticipantId::new("participant:review-agent").unwrap(),
        ChatParticipantKind::AiAgent,
        "Review agent",
        "@Review agent",
        0,
        13,
    )];
    sqlx::query(
        "INSERT INTO chat_scheduled_messages
            (id, client_command_id, channel_id, executing_device_id, request_data,
             scheduled_for, created_at, updated_at)
         VALUES ('scheduled:test', 'command:test', 'channel:review', 'device:desk', ?1, ?2, ?2,
                 ?2)",
    )
    .bind(serde_json::to_string(&request).unwrap())
    .bind(now.as_str())
    .execute(pool)
    .await
    .unwrap();
    let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
    claim_scheduled_message_for_immediate_send(
        pool,
        &scheduled_message_id,
        "device:desk",
        now,
        "claim:a",
    )
    .await
    .unwrap();
    (request, scheduled_message_id)
}

/// Seeds the review thread with `assignment:review` in `state` on `device:desk`, without
/// pending replies, and stores and claims as `claim:a` a scheduled reply in that thread that
/// names no agent, so its routing infers the review agent. Returns the pool, the request,
/// and the scheduled message.
async fn claim_scheduled_review_reply(
    state: &str,
    now: &UtcTimestamp,
) -> (SqlitePool, PostChatMessageCommand, ChatScheduledMessageId) {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    sqlx::query(
        "DELETE FROM chat_work_assignment_inputs
         WHERE assignment_id = 'assignment:review' AND routing_kind <> 'trigger'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE chat_work_assignments SET state = ? WHERE id = 'assignment:review'")
        .bind(state)
        .execute(&pool)
        .await
        .unwrap();
    let mut request = message("Also cover the edge cases");
    request.client_command_id = ChatCommandId::new("command:scheduled-reply").unwrap();
    request.channel_id = ChatChannelId::new("channel:review").unwrap();
    request.reply_thread_id = Some(ChatReplyThreadId::new("reply-thread:review").unwrap());
    sqlx::query(
        "INSERT INTO chat_scheduled_messages
            (id, client_command_id, channel_id, reply_thread_id, executing_device_id,
             request_data, scheduled_for, created_at, updated_at)
         VALUES ('scheduled:reply', 'command:scheduled-reply', 'channel:review',
                 'reply-thread:review', 'device:desk', ?1, ?2, ?2, ?2)",
    )
    .bind(serde_json::to_string(&request).unwrap())
    .bind(now.as_str())
    .execute(&pool)
    .await
    .unwrap();
    let scheduled_message_id = ChatScheduledMessageId::new("scheduled:reply").unwrap();
    claim_scheduled_message_for_immediate_send(
        &pool,
        &scheduled_message_id,
        "device:desk",
        now,
        "claim:a",
    )
    .await
    .unwrap();
    (pool, request, scheduled_message_id)
}

pub(super) async fn seed_test_channel(pool: &SqlitePool, now: &UtcTimestamp) {
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

async fn review_thread_without_assignment() -> SqlitePool {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    clear_review_assignment(&pool).await;
    pool
}

/// The only Chat writer after `device:old` stopped writing, as after an unlink.
const NEW_OWNER: &str = "device:new";

/// Writes the assignment `assignment:old` of the review reply `item:review-reply-1` in
/// `state`, executed by `device:old`, with a pending input for `item:review-reply-2`.
async fn seed_assignment_on_old_owner(pool: &SqlitePool, state: &str) {
    sqlx::query(
        "INSERT INTO chat_work_assignments
             (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
              executing_device_id, state, created_at, updated_at)
         VALUES ('assignment:old', 'reply-thread:review', 'participant:review-agent',
                 'item:review-reply-1', 1, json_object('providerManagedModel', json('true')), 'device:old', ?, '2026-08-04T17:01:00.000Z',
                 '2026-08-04T17:01:00.000Z')",
    )
    .bind(state)
    .execute(pool)
    .await
    .unwrap();
    for (message, ordinal, kind) in [
        ("item:review-reply-1", 1, "trigger"),
        ("item:review-reply-2", 2, "queued_continuation"),
    ] {
        sqlx::query(
            "INSERT INTO chat_work_assignment_inputs
                 (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
             VALUES (?, 'assignment:old', ?, ?, ?, '2026-08-04T17:01:00.000Z')",
        )
        .bind(super::common::assignment_input_id(message).unwrap())
        .bind(message)
        .bind(ordinal)
        .bind(kind)
        .execute(pool)
        .await
        .unwrap();
    }
}

/// State, reason, executing device, and successor of the assignment `assignment_id`.
async fn assignment_takeover(
    pool: &SqlitePool,
    assignment_id: &str,
) -> (String, Option<String>, Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT assignment.state, assignment.state_reason, assignment.executing_device_id,
                (SELECT successor.id FROM chat_work_assignments successor
                 WHERE successor.previous_assignment_id = assignment.id) AS successor_id
         FROM chat_work_assignments assignment WHERE assignment.id = ?",
    )
    .bind(assignment_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// A local reply in `reply-thread:review` that mentions the review agent.
fn review_reply_mention(client_command_id: &str) -> PostChatMessageCommand {
    let mut request = message("@Review agent continue");
    request.client_command_id = ChatCommandId::new(client_command_id).unwrap();
    request.channel_id = ChatChannelId::new("channel:review").unwrap();
    request.reply_thread_id = Some(ChatReplyThreadId::new("reply-thread:review").unwrap());
    request.references = vec![participant_reference(
        "reference:reply",
        ChatParticipantId::new("participant:review-agent").unwrap(),
        ChatParticipantKind::AiAgent,
        "Review agent",
        "@Review agent",
        0,
        13,
    )];
    request
}

/// Number of follow-up assignments of `assignment:review`.
async fn review_follow_ups(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM chat_work_assignments
         WHERE previous_assignment_id = 'assignment:review'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// An agent of another provider family in the review channel, so its work has another
/// acceptance digest.
async fn seed_other_review_agent(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO chat_participants
             (id, participant_kind, display_name, created_at, updated_at)
         VALUES (
             'participant:other-agent', 'ai_agent', 'Other agent',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_agents
             (participant_id, provider_family_id, created_at, updated_at)
         VALUES (
             'participant:other-agent', 'claude',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_agent_policy_revisions
             (id, agent_id, model_selection_data, created_at)
         VALUES (
             'policy:other', 'participant:other-agent',
             '{\"providerManagedModel\":true,\"modelId\":null,\"modelOptions\":[]}',
             '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_conversation_memberships
             (conversation_id, participant_id, sync_key, membership_role, created_at,
              updated_at)
         VALUES (
             'conversation:review', 'participant:other-agent', 'membership:other', 'member',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         INSERT INTO chat_agent_channel_memberships
             (conversation_id, agent_id, sync_key,
              created_at, updated_at)
         VALUES (
             'conversation:review', 'participant:other-agent', 'channel-membership:other',
             '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z'
         );
         UPDATE chat_agents SET current_policy_revision_id = 'policy:other'
         WHERE participant_id = 'participant:other-agent';",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Acceptance digest of `assignment_id`.
async fn digest_of(pool: &SqlitePool, assignment_id: &str) -> String {
    assignments::assignment_acceptance_digest(
        pool,
        &ChatWorkAssignmentId::new(assignment_id).unwrap(),
        &test_providers(),
    )
    .await
    .unwrap()
}

#[test]
fn waiting_replies_leave_the_list_once_their_subject_is_accepted() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:desk"]);
        let device = super::workflow::PostingDevice::this_device("device:desk", &writers);
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;

        let waiting = assignments::waiting_assignments(&pool, device, &ChatDeviceScope::default())
            .await
            .unwrap();
        assert_eq!(waiting.items.len(), 1);
        assert_eq!(waiting.total, 1);
        assert_eq!(waiting.items[0].assignment_id.as_str(), "assignment:review");

        let accepted = scope_accepting(&pool, "assignment:review").await;
        assert!(
            assignments::waiting_assignments(&pool, device, &accepted)
                .await
                .unwrap()
                .items
                .is_empty(),
            "recovery continues accepted replies unasked"
        );
    });
}

/// A review thread whose second reply mentions the review agent without an assignment.
async fn review_thread_with_unrouted_mention() -> SqlitePool {
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
    pool
}
