use super::*;
use ganbaru_chat::coordination::reports::{HostCall, HostCallOutcome};
use ganbaru_chat::coordination::schedule_proposals::record_host_schedule_proposal;
use ganbaru_chat::project_defaults::{default_channel_ids, ensure_project_chat_defaults};
use ganbaru_chat::scheduling::{QuietPolicy, RecurrenceRule, RecurrenceSpec, WeekdaySet};

const NOW: &str = "2026-11-12T09:00:00.000Z";
const FIRST_AT: &str = "2026-11-13T08:00:00.000Z";
const PROJECT_ID: &str = "project-schedule-proposals";
const TEAMMATE: &str = "participant:proposals-helper";
const OWNER: &str = "participant:local-owner";
const ASSIGNMENT: &str = "assignment:proposals";
const RUN: &str = "run:proposals-1";
const THREAD: &str = "reply:proposals";

/// A migrated pool with foreign keys off, so the run needs none of the policy and
/// authorization rows its dispatch would create.
async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

/// Seeds a working assignment of a teammate named `teammate_name` and returns the id of the
/// schedule proposal its run posted.
async fn seed_proposal(pool: &SqlitePool, teammate_name: &str) -> ChatConversationItemId {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-proposals', 'P', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{PROJECT_ID}', 'group-proposals', 'Proposals', 'a0');"
    )))
    .execute(pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ensure_project_chat_defaults(
        &mut connection,
        PROJECT_ID,
        &UtcTimestamp::new(NOW).unwrap(),
    )
    .await
    .unwrap();
    drop(connection);
    let conversation = default_channel_ids(PROJECT_ID).unwrap().0;
    let digest = "a".repeat(64);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{TEAMMATE}', 'ai_teammate', '{teammate_name}', '{NOW}', '{NOW}');
         INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
         VALUES ('{TEAMMATE}', 'Helper', '{NOW}', '{NOW}');
         INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:root', '{conversation}', 'message', 'c000', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:root', '{OWNER}', '{NOW}');
         INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('{THREAD}', '{conversation}', 'item:root', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, state,
             created_at, updated_at)
         VALUES ('{ASSIGNMENT}', '{THREAD}', '{TEAMMATE}', 'item:root', 'working', '{NOW}', '{NOW}');
         UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         INSERT INTO chat_agent_runs
            (id, assignment_id, project_id, teammate_policy_revision_id,
             authorization_revision_id, authorization_scope_digest, provider_turn_id,
             provider_thread_id, state, run_ordinal, created_at, updated_at)
         VALUES ('{RUN}', '{ASSIGNMENT}', '{PROJECT_ID}', 'policy:proposals',
                 'authorization:proposals', '{digest}', 'turn:proposals', 'thread:proposals',
                 'working', 1, '{NOW}', '{NOW}');
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(pool)
    .await
    .unwrap();
    let proposal = ScheduleProposal {
        message: "Post the standup summary".to_string(),
        first_at: FIRST_AT.to_string(),
        recurrence: None,
        reason: "You asked for a daily nudge".to_string(),
    };
    let call = HostCall {
        run_id: RUN,
        assignment_id: ASSIGNMENT,
        client_request_id: "request-1",
    };
    let mut connection = pool.acquire().await.unwrap();
    let outcome = record_host_schedule_proposal(&mut connection, call, &proposal, |_| Ok(()), NOW)
        .await
        .unwrap();
    assert_eq!(outcome, HostCallOutcome::Posted);
    let item_id: String = sqlx::query_scalar(
        "SELECT item_id FROM chat_work_semantic_updates WHERE update_kind = 'schedule_proposal'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    ChatConversationItemId::new(item_id).unwrap()
}

fn request(
    proposal_item_id: &ChatConversationItemId,
    normalized_markdown: Option<&str>,
) -> ConfirmScheduleProposalCommand {
    ConfirmScheduleProposalCommand {
        proposal_item_id: proposal_item_id.clone(),
        scheduled_for: UtcTimestamp::new(FIRST_AT).unwrap(),
        recurrence: Some(RecurrenceSpec {
            rule: RecurrenceRule::FixedTime {
                local_minute: 480,
                weekdays: WeekdaySet::ALL,
            },
            time_zone: "Europe/Madrid".to_string(),
            quiet_policy: QuietPolicy::Defer,
        }),
        normalized_markdown: normalized_markdown.map(str::to_string),
    }
}

async fn confirm(
    pool: &SqlitePool,
    request: ConfirmScheduleProposalCommand,
) -> ChatResult<ScheduleChatMessageCommand> {
    let mut connection = pool.acquire().await.unwrap();
    confirmation_command(&mut connection, request).await
}

#[tokio::test]
async fn a_confirmation_mentions_the_teammate_in_the_reply_thread_under_derived_ids() {
    let pool = migrated_pool().await;
    let item_id = seed_proposal(&pool, "Helper").await;
    let command = confirm(&pool, request(&item_id, None)).await.unwrap();
    assert_eq!(
        command.scheduled_message_id.as_str(),
        confirmed_scheduled_message_id(item_id.as_str()).unwrap()
    );
    assert_eq!(
        command
            .message
            .reply_thread_id
            .as_ref()
            .map(|id| id.as_str()),
        Some(THREAD)
    );
    assert_eq!(
        command.message.channel_id.as_str(),
        default_channel_ids(PROJECT_ID).unwrap().1
    );
    assert_eq!(
        command.message.normalized_markdown,
        "@Helper Post the standup summary"
    );
    assert_eq!(command.scheduled_for.as_str(), FIRST_AT);
    assert!(command.recurrence.is_some());
    let [
        ChatMessageReference::Participant {
            metadata,
            participant_id,
            participant_kind,
        },
    ] = command.message.references.as_slice()
    else {
        panic!("one participant reference");
    };
    assert_eq!(participant_id.as_str(), TEAMMATE);
    assert_eq!(*participant_kind, ChatParticipantKind::AiTeammate);
    assert_eq!((metadata.start_offset, metadata.end_offset), (0, 7));
    assert_eq!(metadata.plain_text_projection, "@Helper");
    // A second device confirming the same proposal derives the same ids.
    let again = confirm(&pool, request(&item_id, Some("Edited text")))
        .await
        .unwrap();
    assert_eq!(again.scheduled_message_id, command.scheduled_message_id);
    assert_eq!(
        again.message.client_command_id,
        command.message.client_command_id
    );
    assert_eq!(again.message.normalized_markdown, "@Helper Edited text");
}

#[tokio::test]
async fn a_non_ascii_teammate_name_gets_byte_offsets() {
    let pool = migrated_pool().await;
    let item_id = seed_proposal(&pool, "Asistenté").await;
    let command = confirm(&pool, request(&item_id, None)).await.unwrap();
    let ChatMessageReference::Participant { metadata, .. } = &command.message.references[0] else {
        panic!("participant reference");
    };
    let mention = "@Asistenté";
    assert_eq!(metadata.end_offset, mention.len() as u64);
    assert_eq!(
        &command.message.normalized_markdown[..mention.len()],
        mention
    );
}

#[tokio::test]
async fn confirmations_refuse_blank_or_oversized_text_and_removed_teammates() {
    let pool = migrated_pool().await;
    let item_id = seed_proposal(&pool, "Helper").await;
    let blank = confirm(&pool, request(&item_id, Some("   ")))
        .await
        .unwrap_err();
    assert_eq!(blank.code, ChatErrorCode::Validation);
    let long = "é".repeat(MAX_PROPOSED_MESSAGE_CHARS + 1);
    let oversized = confirm(&pool, request(&item_id, Some(&long)))
        .await
        .unwrap_err();
    assert_eq!(oversized.code, ChatErrorCode::Validation);
    sqlx::query("UPDATE chat_participants SET archived_at = ? WHERE id = ?")
        .bind(NOW)
        .bind(TEAMMATE)
        .execute(&pool)
        .await
        .unwrap();
    let removed = confirm(&pool, request(&item_id, None)).await.unwrap_err();
    assert_eq!(removed.code, ChatErrorCode::Conflict);
    let unknown = ChatConversationItemId::new("item:unknown").unwrap();
    let missing = confirm(&pool, request(&unknown, None)).await.unwrap_err();
    assert_eq!(missing.code, ChatErrorCode::NotFound);
}

#[tokio::test]
async fn states_are_pending_dismissed_or_confirmed_and_confirmation_wins() {
    let pool = migrated_pool().await;
    let item_id = seed_proposal(&pool, "Helper").await;
    let other = ChatConversationItemId::new("item:other-proposal").unwrap();
    let mut dismissed = BTreeMap::new();
    dismissed.insert(other.as_str().to_string(), UtcTimestamp::new(NOW).unwrap());
    let states = proposal_states(&pool, vec![item_id.clone(), other.clone()], &dismissed)
        .await
        .unwrap();
    assert_eq!(
        states.iter().map(|read| read.state).collect::<Vec<_>>(),
        [
            ChatScheduleProposalState::Pending,
            ChatScheduleProposalState::Dismissed
        ]
    );
    // A one-time post that was already sent counts as confirmed even after its row is gone.
    let scheduled_id = confirmed_scheduled_message_id(item_id.as_str()).unwrap();
    let posted = scheduled_post_ids(&scheduled_id, ScheduledPostTarget::Message, None)
        .unwrap()
        .item_id;
    let conversation = default_channel_ids(PROJECT_ID).unwrap().0;
    sqlx::query(
        "INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES (?, ?, 'message', 'c900', ?)",
    )
    .bind(posted.as_str())
    .bind(&conversation)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    dismissed.insert(
        item_id.as_str().to_string(),
        UtcTimestamp::new(NOW).unwrap(),
    );
    let states = proposal_states(&pool, vec![item_id], &dismissed)
        .await
        .unwrap();
    assert_eq!(states[0].state, ChatScheduleProposalState::Confirmed);
}
