use super::helpers::{load_project_chat_graph, migrated_memory_pool};
use sqlx::{AssertSqlSafe, SqlitePool};

/// A queued access revocation job for the fixture authorization.
const REVOCATION_JOB: &str = "INSERT INTO chat_access_revocation_jobs
    (id, teammate_id, conversation_id, authorization_revision_id, reason,
     available_at, created_at, updated_at)
VALUES ('revocation-1', 'participant-teammate', 'conversation-general', 'authorization-1',
        'Access was reduced', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z',
        '2026-10-23T00:00:00.000Z');";

/// Tables whose foreign keys follow their organizational parents.
const REBUILT_TABLES: [&str; 11] = [
    "chat_access_revocation_jobs",
    "chat_assignment_authorization_revisions",
    "chat_agent_runs",
    "chat_assignment_authorized_channel_sources",
    "chat_assignment_context_packages",
    "chat_host_tool_returned_message_revisions",
    "chat_scratch_generation_sources",
    "chat_scratch_scopes",
    "chat_threads",
    "chat_execution_environment_reference_targets",
    "chat_scheduled_message_references",
];

/// Columns that carry no foreign key, as (table, column), so the id stays unchecked.
const UNCHECKED_COLUMNS: [(&str, &str); 2] = [
    (
        "chat_execution_environment_reference_targets",
        "execution_environment_id",
    ),
    (
        "chat_scheduled_message_references",
        "execution_environment_id",
    ),
];

/// Foreign keys that do not cascade, as (table, parent).
const NON_CASCADING_FOREIGN_KEYS: [(&str, &str); 3] = [
    (
        "chat_access_revocation_jobs",
        "chat_assignment_authorization_revisions",
    ),
    ("chat_scheduled_message_references", "chat_participants"),
    ("chat_scheduled_message_references", "chat_channels"),
];

/// Triggers that stand down while the sync engine applies remote rows.
const GATED_TRIGGERS: [&str; 15] = [
    "chat_reply_threads_root_item_insert",
    "chat_communication_message_current_revision_update",
    "chat_teammate_folder_grant_identity_insert",
    "chat_channels_default_name_insert",
    "chat_channels_default_protected_update",
    "chat_teammate_create_access_state",
    "chat_conversation_create_audience_state",
    "chat_access_profile_audience_publish",
    "chat_generic_membership_audience_insert",
    "chat_generic_membership_audience_update",
    "chat_generic_membership_audience_delete",
    "chat_teammate_channel_membership_audience_insert",
    "chat_teammate_channel_membership_audience_update",
    "chat_teammate_channel_membership_audience_delete",
    "chat_communication_search_insert",
];

const APPLY_GATE: &str = "(SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0";

/// Chat tables that replicate; every other Chat table is device-local.
const REPLICATED_CHAT_TABLES: [&str; 29] = [
    "chat_participants",
    "chat_teammates",
    "chat_teammate_policy_revisions",
    "chat_access_profiles",
    "chat_access_profile_revisions",
    "chat_conversations",
    "chat_conversation_memberships",
    "chat_teammate_channel_memberships",
    "chat_teammate_working_folder_grants",
    "chat_channels",
    "chat_reply_threads",
    "chat_conversation_items",
    "chat_communication_messages",
    "chat_communication_message_revisions",
    "chat_message_references",
    "chat_channel_reference_targets",
    "chat_execution_environment_reference_targets",
    "chat_participant_reference_targets",
    "chat_working_folder_reference_targets",
    "chat_workspace_path_reference_targets",
    "chat_conversation_read_cursors",
    "chat_reply_thread_read_cursors",
    "chat_communication_attachment_references",
    "chat_scheduled_messages",
    "chat_scheduled_message_attachment_references",
    "chat_scheduled_message_references",
    "chat_work_assignments",
    "chat_work_assignment_inputs",
    "chat_work_semantic_updates",
];

async fn load_seeded_graph(pool: &SqlitePool) {
    load_project_chat_graph(pool).await;
    sqlx::raw_sql(REVOCATION_JOB).execute(pool).await.unwrap();
}

/// Every foreign key of a table as (parent, child column, delete action).
async fn foreign_keys(pool: &SqlitePool, table: &str) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT \"table\", \"from\", on_delete FROM pragma_foreign_key_list(?)
         ORDER BY id, seq",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn foreign_key_violations(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM pragma_foreign_key_check")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count_where(pool: &SqlitePool, table: &str, filter: &str) -> i64 {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT count(*) FROM {table} WHERE {filter}"
    )))
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn set_applying(pool: &SqlitePool, applying: bool) {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(i64::from(applying))
        .execute(pool)
        .await
        .unwrap();
}

#[test]
fn chat_local_tables_cascade_from_organizational_rows_and_gated_triggers_stand_down() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_seeded_graph(&pool).await;

        for table in REBUILT_TABLES {
            for (parent, column, action) in foreign_keys(&pool, table).await {
                assert!(
                    !UNCHECKED_COLUMNS.contains(&(table, column.as_str())),
                    "{table}.{column} carries no foreign key"
                );
                if !NON_CASCADING_FOREIGN_KEYS.contains(&(table, parent.as_str())) {
                    assert_eq!(action, "CASCADE", "{table}.{column} -> {parent}");
                }
            }
        }

        for trigger in GATED_TRIGGERS {
            let sql: String = sqlx::query_scalar(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?",
            )
            .bind(trigger)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(
                sql.contains(APPLY_GATE),
                "{trigger} stands down during apply"
            );
        }
        let removal_columns: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pragma_table_info('chat_teammate_channel_memberships')
             WHERE name = 'removed_at'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(removal_columns, 1);

        sqlx::query("DELETE FROM chat_conversations WHERE id = 'conversation-general'")
            .execute(&pool)
            .await
            .unwrap();
        for table in [
            "chat_access_revocation_jobs",
            "chat_assignment_authorization_revisions",
            "chat_agent_runs",
            "chat_assignment_authorized_channel_sources",
            "chat_scratch_generation_sources",
            "chat_scratch_scopes",
        ] {
            assert_eq!(
                count_where(&pool, table, "1 = 1").await,
                0,
                "{table} rows follow the deleted conversation"
            );
        }
        assert_scratch_chain_removed(&pool).await;
        assert_eq!(foreign_key_violations(&pool).await, 0);
    });
}

/// Asserts that the fixture's scratch generation, environment, thread, turn, and run are gone
/// while the folder-bound thread stays.
async fn assert_scratch_chain_removed(pool: &SqlitePool) {
    for (table, filter) in [
        ("chat_scratch_generations", "id = 'scratch-generation'"),
        ("chat_execution_environments", "id = 'scratch-environment'"),
        ("chat_threads", "id = 'thread-scratch'"),
        ("chat_turns", "id = 'turn-scratch'"),
        ("chat_agent_runs", "id = 'run-scratch'"),
    ] {
        assert_eq!(
            count_where(pool, table, filter).await,
            0,
            "{table} follows its scratch scope"
        );
    }
    assert_eq!(
        count_where(pool, "chat_threads", "id = 'thread-managed'").await,
        1
    );
}

#[test]
fn deleting_a_reply_thread_removes_its_scratch_work_and_keeps_the_conversation() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_seeded_graph(&pool).await;

        sqlx::query("DELETE FROM chat_reply_threads WHERE id = 'reply-thread'")
            .execute(&pool)
            .await
            .unwrap();

        for table in [
            "chat_scratch_scopes",
            "chat_assignment_authorization_revisions",
        ] {
            assert_eq!(count_where(&pool, table, "1 = 1").await, 0, "{table}");
        }
        assert_scratch_chain_removed(&pool).await;
        assert_eq!(
            count_where(&pool, "chat_conversations", "id = 'conversation-general'").await,
            1
        );
        assert_eq!(
            count_where(
                &pool,
                "chat_execution_environment_reference_targets",
                "execution_environment_id = 'scratch-environment'"
            )
            .await,
            1,
            "an organizational mention keeps the unchecked environment id"
        );
        assert_eq!(
            count_where(
                &pool,
                "chat_scheduled_message_references",
                "execution_environment_id = 'scratch-environment'"
            )
            .await,
            1,
            "a scheduled mention keeps the unchecked environment id"
        );
        assert_eq!(foreign_key_violations(&pool).await, 0);
    });
}

#[test]
fn a_message_deleted_while_sync_applies_leaves_the_search_index() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_seeded_graph(&pool).await;
        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
             VALUES ('item-reply', 'conversation-general', 'message', 'c001', '2026-10-24T00:00:00.000Z');
             INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
             VALUES ('item-reply', 'participant:local-owner', '2026-10-24T00:00:00.000Z');
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES ('message-revision-reply', 'item-reply', 'Searchable reply',
                     '2026-10-24T00:00:00.000Z');
             UPDATE chat_communication_messages SET current_revision_id = 'message-revision-reply'
             WHERE item_id = 'item-reply';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let indexed = "message_item_id = 'item-reply'";
        assert_eq!(
            count_where(&pool, "chat_communication_search_fts", indexed).await,
            1
        );

        set_applying(&pool, true).await;
        sqlx::query("DELETE FROM chat_communication_messages WHERE item_id = 'item-reply'")
            .execute(&pool)
            .await
            .unwrap();
        set_applying(&pool, false).await;

        assert_eq!(
            count_where(&pool, "chat_communication_search_fts", indexed).await,
            0,
            "the device-local search index drops a message deleted by sync"
        );
    });
}

#[test]
fn derived_state_triggers_run_for_commands_and_stand_down_while_sync_applies() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_seeded_graph(&pool).await;
        assert_eq!(
            count_where(
                &pool,
                "chat_teammate_access_state",
                "teammate_id = 'participant-teammate'"
            )
            .await,
            1,
            "a teammate created by a command gets its access state"
        );

        set_applying(&pool, true).await;
        sqlx::raw_sql(
            "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant-synced', 'ai_teammate', 'Synced',
                     '2026-10-24T00:00:00.000Z', '2026-10-24T00:00:00.000Z');
             INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
             VALUES ('participant-synced', 'Arrived by sync',
                     '2026-10-24T00:00:00.000Z', '2026-10-24T00:00:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            count_where(
                &pool,
                "chat_teammate_access_state",
                "teammate_id = 'participant-synced'"
            )
            .await,
            0,
            "the materialize hook derives state for applied rows"
        );
        set_applying(&pool, false).await;

        sqlx::raw_sql(
            "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant-local', 'ai_teammate', 'Local',
                     '2026-10-24T00:00:00.000Z', '2026-10-24T00:00:00.000Z');
             INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
             VALUES ('participant-local', 'Made here',
                     '2026-10-24T00:00:00.000Z', '2026-10-24T00:00:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            count_where(
                &pool,
                "chat_teammate_access_state",
                "teammate_id = 'participant-local'"
            )
            .await,
            1
        );
    });
}

/// Device-local `NO ACTION` edges, as (child, column), whose child row always cascades from the same
/// root as its parent. SQLite checks `NO ACTION` at the end of the statement, so these never stop a
/// delete: a restore operation and its checkpoint both cascade from their thread.
const SAME_ROOT_NO_ACTION_EDGES: [(&str, &str); 1] = [("chat_restore_operations", "checkpoint_id")];

/// Walks every foreign key chain that a replicated Chat delete can start and returns the edges
/// that would stop it or that would let a device-local delete remove a replicated row.
async fn blocking_foreign_key_edges(pool: &SqlitePool) -> Vec<String> {
    let edges: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT child.name, foreign_key.\"table\", foreign_key.\"from\", foreign_key.on_delete
         FROM sqlite_schema child
         JOIN pragma_foreign_key_list(child.name) foreign_key
         WHERE child.type = 'table'
         ORDER BY child.name, foreign_key.id, foreign_key.seq",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut reached: Vec<String> = REPLICATED_CHAT_TABLES.map(String::from).to_vec();
    let mut next = 0;
    let mut blocking = Vec::new();
    while next < reached.len() {
        let parent = reached[next].clone();
        next += 1;
        let parent_is_replicated = REPLICATED_CHAT_TABLES.contains(&parent.as_str());
        for (child, _, column, action) in edges.iter().filter(|edge| edge.1 == parent) {
            let child_is_replicated = REPLICATED_CHAT_TABLES.contains(&child.as_str());
            let edge = format!("{child}.{column} -> {parent} ON DELETE {action}");
            match action.as_str() {
                "CASCADE" if !parent_is_replicated && child_is_replicated => blocking.push(edge),
                "CASCADE" => {
                    if !reached.contains(child) {
                        reached.push(child.clone());
                    }
                }
                "SET NULL" | "SET DEFAULT" => {}
                _ if parent_is_replicated && child_is_replicated => {}
                _ if SAME_ROOT_NO_ACTION_EDGES.contains(&(child.as_str(), column.as_str())) => {}
                _ => blocking.push(edge),
            }
        }
    }
    blocking
}

#[test]
fn replicated_chat_deletes_never_stop_on_a_device_local_foreign_key() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        assert_eq!(
            blocking_foreign_key_edges(&pool).await,
            Vec::<String>::new()
        );
    });
}
