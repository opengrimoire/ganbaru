//! Fixtures for reads of assignment attention: channels, reply threads, agents, and
//! assignments in any state, written directly so a test controls every timestamp.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use std::sync::atomic::{AtomicU64, Ordering};

/// Creation time of every fixture row other than assignment timestamps.
pub(crate) const SEEDED_AT: &str = "2026-10-20T12:00:00.000Z";

static ORDER_KEY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// A fresh conversation item order key. Keys keep the `a0` integer part and a fraction that
/// never ends in `0`, so every key passes the replicated order key rule.
fn next_order_key() -> String {
    let sequence = ORDER_KEY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("a0{sequence}1")
}

/// The stored timestamp `days` days before now, for rows a time window must include or skip.
pub(crate) fn days_ago(days: i64) -> String {
    (Utc::now() - Duration::days(days)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The stored timestamp `minutes` minutes after [`SEEDED_AT`], for ordering by recency.
pub(crate) fn minutes_after_seed(minutes: i64) -> String {
    let seeded = DateTime::parse_from_rfc3339(SEEDED_AT)
        .unwrap()
        .with_timezone(&Utc);
    (seeded + Duration::minutes(minutes)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Writes an AI agent named `display_name`.
pub(crate) async fn seed_agent(pool: &sqlx::SqlitePool, id: &str, display_name: &str) {
    sqlx::query(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, created_at, updated_at)
         VALUES (?, 'ai_agent', ?, ?, ?)",
    )
    .bind(id)
    .bind(display_name)
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_agents (participant_id, role, created_at, updated_at)
         VALUES (?, 'Reviewer', ?, ?)",
    )
    .bind(id)
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
}

/// Writes a project in its own group, archived when `archived` is set.
pub(crate) async fn seed_project(pool: &sqlx::SqlitePool, id: &str, name: &str, archived: bool) {
    sqlx::query("INSERT INTO project_groups (id, name, order_key) VALUES (?, ?, 'a1')")
        .bind(format!("group:{id}"))
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects (id, group_id, name, order_key) VALUES (?, ?, ?, 'a1')")
        .bind(id)
        .bind(format!("group:{id}"))
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
    if archived {
        sqlx::query("UPDATE projects SET status = 'archived' WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
}

/// Writes a channel of `project_id` with its conversation and returns the conversation id.
pub(crate) async fn seed_channel(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    channel_id: &str,
    name: &str,
    archived: bool,
) -> String {
    let conversation_id = format!("conversation:{channel_id}");
    let archived_at = archived.then_some(SEEDED_AT);
    sqlx::query(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES (?, ?, 'channel', ?, ?, ?)",
    )
    .bind(&conversation_id)
    .bind(project_id)
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_channels
            (id, project_id, conversation_id, name, archived_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(channel_id)
    .bind(project_id)
    .bind(&conversation_id)
    .bind(name)
    .bind(archived_at)
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
    conversation_id
}

/// Writes a reply thread `thread_id` in `conversation_id` under a new root message, which
/// fixture assignments also use as their trigger.
pub(crate) async fn seed_reply_thread(
    pool: &sqlx::SqlitePool,
    conversation_id: &str,
    thread_id: &str,
) {
    let root_item_id = root_item_id(thread_id);
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, item_kind, order_key, created_at)
         VALUES (?, ?, 'message', ?, ?)",
    )
    .bind(&root_item_id)
    .bind(conversation_id)
    .bind(next_order_key())
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES (?, 'participant:local-owner', ?)",
    )
    .bind(&root_item_id)
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(thread_id)
    .bind(conversation_id)
    .bind(&root_item_id)
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
}

fn root_item_id(thread_id: &str) -> String {
    format!("item:root:{thread_id}")
}

/// One fixture assignment. Settled states take `settled_at`, else `updated_at`.
pub(crate) struct SeedAssignment<'a> {
    pub id: &'a str,
    pub reply_thread_id: &'a str,
    pub agent_id: &'a str,
    pub state: &'a str,
    pub updated_at: String,
    pub settled_at: Option<String>,
    pub previous_assignment_id: Option<&'a str>,
    pub parent_assignment_id: Option<&'a str>,
    pub executing_device_id: Option<&'a str>,
    pub state_reason: Option<&'a str>,
}

impl<'a> SeedAssignment<'a> {
    /// An undelegated assignment with no predecessor, updated `minutes` after the seed time.
    pub(crate) fn new(
        id: &'a str,
        reply_thread_id: &'a str,
        agent_id: &'a str,
        state: &'a str,
        minutes: i64,
    ) -> Self {
        let updated_at = minutes_after_seed(minutes);
        let settled_at =
            matches!(state, "completed" | "failed" | "cancelled").then(|| updated_at.clone());
        Self {
            id,
            reply_thread_id,
            agent_id,
            state,
            updated_at,
            settled_at,
            previous_assignment_id: None,
            parent_assignment_id: None,
            executing_device_id: None,
            state_reason: None,
        }
    }
}

/// Writes `assignment`, with delegation fields one level below its parent when it has one.
pub(crate) async fn seed_assignment(pool: &sqlx::SqlitePool, assignment: &SeedAssignment<'_>) {
    let delegated = assignment.parent_assignment_id.is_some();
    sqlx::query(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id,
             executing_device_id, previous_assignment_id, state, state_reason, settled_at,
             parent_assignment_id, delegation_depth, delegation_objective,
             delegation_expected_result, delegation_folder_ceiling, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(assignment.id)
    .bind(assignment.reply_thread_id)
    .bind(assignment.agent_id)
    .bind(root_item_id(assignment.reply_thread_id))
    .bind(assignment.executing_device_id)
    .bind(assignment.previous_assignment_id)
    .bind(assignment.state)
    .bind(assignment.state_reason)
    .bind(assignment.settled_at.as_deref())
    .bind(assignment.parent_assignment_id)
    .bind(i64::from(delegated))
    .bind(delegated.then_some("Check the parser"))
    .bind(delegated.then_some("A list of failures"))
    .bind(delegated.then_some("[]"))
    .bind(SEEDED_AT)
    .bind(&assignment.updated_at)
    .execute(pool)
    .await
    .unwrap();
}

/// Writes a `review` semantic update of `assignment_id` in `reply_thread_id` that lists
/// `paths` as changed files.
pub(crate) async fn seed_review_update(
    pool: &sqlx::SqlitePool,
    reply_thread_id: &str,
    assignment_id: &str,
    item_id: &str,
    paths: &[&str],
) {
    let changed_files = paths
        .iter()
        .map(|path| {
            serde_json::json!({
                "relativePath": path,
                "previousRelativePath": null,
                "additions": 1,
                "deletions": 0,
                "binary": false,
                "status": "modified",
            })
        })
        .collect::<Vec<_>>();
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         SELECT ?, conversation_id, id, 'message', ?, ?
         FROM chat_reply_threads WHERE id = ?",
    )
    .bind(item_id)
    .bind(next_order_key())
    .bind(SEEDED_AT)
    .bind(reply_thread_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES (?, ?, 'review', ?, ?)",
    )
    .bind(item_id)
    .bind(assignment_id)
    .bind(serde_json::json!({ "changedFiles": changed_files }).to_string())
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
}

/// Writes a reply `item_id` by `author_id` in `reply_thread_id` at `order_key`, created at
/// `created_at`.
pub(crate) async fn seed_reply(
    pool: &sqlx::SqlitePool,
    reply_thread_id: &str,
    item_id: &str,
    author_id: &str,
    order_key: &str,
    created_at: &str,
) {
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         SELECT ?, conversation_id, id, 'message', ?, ?
         FROM chat_reply_threads WHERE id = ?",
    )
    .bind(item_id)
    .bind(order_key)
    .bind(created_at)
    .bind(reply_thread_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES (?, ?, ?)",
    )
    .bind(item_id)
    .bind(author_id)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

/// Writes the failure message `item_id` of `assignment_id` as a reply by `agent_id`, the way
/// a failed run posts it.
pub(crate) async fn seed_failure_update(
    pool: &sqlx::SqlitePool,
    reply_thread_id: &str,
    assignment_id: &str,
    agent_id: &str,
    item_id: &str,
    order_key: &str,
    created_at: &str,
) {
    seed_reply(
        pool,
        reply_thread_id,
        item_id,
        agent_id,
        order_key,
        created_at,
    )
    .await;
    sqlx::query(
        "INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES (?, ?, 'failure', ?, ?)",
    )
    .bind(item_id)
    .bind(assignment_id)
    .bind(serde_json::json!({ "code": "timeout" }).to_string())
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

/// Moves the local read cursor of `reply_thread_id` to `order_key` and `item_id`.
pub(crate) async fn seed_reply_thread_cursor(
    pool: &sqlx::SqlitePool,
    reply_thread_id: &str,
    order_key: &str,
    item_id: &str,
) {
    sqlx::query(
        "INSERT INTO chat_reply_thread_read_cursors
            (reply_thread_id, participant_id, sync_key, last_read_order_key,
             last_read_item_id, updated_at)
         VALUES (?, 'participant:local-owner', ?, ?, ?, ?)
         ON CONFLICT (reply_thread_id, participant_id) DO UPDATE SET
            last_read_order_key = excluded.last_read_order_key,
            last_read_item_id = excluded.last_read_item_id",
    )
    .bind(reply_thread_id)
    .bind(format!("cursor:{reply_thread_id}"))
    .bind(order_key)
    .bind(item_id)
    .bind(SEEDED_AT)
    .execute(pool)
    .await
    .unwrap();
}
