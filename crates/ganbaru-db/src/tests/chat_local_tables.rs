use super::helpers::{memory_pool_migrated_through, migrate_through, migrated_memory_pool};
use crate::run_migrations;
use sqlx::{AssertSqlSafe, Row, SqlitePool};
use std::collections::BTreeMap;

const BEFORE_CHAT_LOCAL_TABLES: i64 = 20261024045821;
const CHAT_LOCAL_TABLES: i64 = 20261024142304;

/// One project with a Chat graph of teammates, assignments, authorizations, runs, and scratch.
const PROJECT_CHAT_GRAPH: &str = include_str!("project_chat_graph.sql");

/// A queued access revocation job for the fixture authorization.
const REVOCATION_JOB: &str = "INSERT INTO chat_access_revocation_jobs
    (id, teammate_id, conversation_id, authorization_revision_id, reason,
     available_at, created_at, updated_at)
VALUES ('revocation-1', 'participant-teammate', 'conversation-general', 'authorization-1',
        'Access was reduced', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z',
        '2026-10-23T00:00:00.000Z');";

/// Tables the migration rebuilds.
const REBUILT_TABLES: [&str; 8] = [
    "chat_access_revocation_jobs",
    "chat_assignment_authorization_revisions",
    "chat_agent_runs",
    "chat_assignment_authorized_channel_sources",
    "chat_assignment_context_packages",
    "chat_host_tool_returned_message_revisions",
    "chat_scratch_generation_sources",
    "chat_scratch_scopes",
];

/// Triggers that stand down while the sync engine applies remote rows.
const GATED_TRIGGERS: [&str; 16] = [
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
    "chat_communication_search_delete",
];

const APPLY_GATE: &str = "(SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0";

async fn load_seeded_graph(pool: &SqlitePool) {
    sqlx::raw_sql(PROJECT_CHAT_GRAPH)
        .execute(pool)
        .await
        .unwrap();
    sqlx::raw_sql(REVOCATION_JOB).execute(pool).await.unwrap();
}

async fn row_counts(pool: &SqlitePool) -> BTreeMap<String, i64> {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_schema
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations'
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    let mut counts = BTreeMap::new();
    for table in tables {
        let count: i64 =
            sqlx::query_scalar(AssertSqlSafe(format!("SELECT count(*) FROM \"{table}\"")))
                .fetch_one(pool)
                .await
                .unwrap();
        counts.insert(table, count);
    }
    counts
}

/// Index and trigger names on the rebuilt tables, as (type, name).
async fn rebuilt_table_objects(pool: &SqlitePool) -> Vec<(String, String)> {
    let mut objects = Vec::new();
    for table in REBUILT_TABLES {
        let rows = sqlx::query(
            "SELECT type, name FROM sqlite_schema
             WHERE tbl_name = ? AND type IN ('index', 'trigger')",
        )
        .bind(table)
        .fetch_all(pool)
        .await
        .unwrap();
        objects.extend(rows.iter().map(|row| (row.get("type"), row.get("name"))));
    }
    objects.sort();
    objects
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
fn chat_local_tables_migration_keeps_rows_and_objects_and_cascades_from_organizational_rows() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_CHAT_LOCAL_TABLES).await;
        load_seeded_graph(&pool).await;
        let counts_before = row_counts(&pool).await;
        let objects_before = rebuilt_table_objects(&pool).await;
        let run_before: (String, String, String) = sqlx::query_as(
            "SELECT assignment_id, authorization_revision_id, provider_turn_id
             FROM chat_agent_runs",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut foreign_keys_before = BTreeMap::new();
        for table in REBUILT_TABLES {
            foreign_keys_before.insert(table, foreign_keys(&pool, table).await);
        }

        migrate_through(&pool, CHAT_LOCAL_TABLES).await.unwrap();

        let mut counts_after = row_counts(&pool).await;
        assert_eq!(
            counts_after.remove("chat_scheduled_message_dispatch"),
            Some(0),
            "the dispatch table starts empty"
        );
        assert_eq!(counts_after, counts_before);
        assert_eq!(foreign_key_violations(&pool).await, 0);
        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(enabled, 1);
        assert_eq!(rebuilt_table_objects(&pool).await, objects_before);
        let run_after: (String, String, String) = sqlx::query_as(
            "SELECT assignment_id, authorization_revision_id, provider_turn_id
             FROM chat_agent_runs",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(run_after, run_before);

        for table in REBUILT_TABLES {
            let before = &foreign_keys_before[table];
            let after = foreign_keys(&pool, table).await;
            assert_eq!(after.len(), before.len(), "{table} keeps its foreign keys");
            for ((parent, column, old_action), (new_parent, new_column, action)) in
                before.iter().zip(&after)
            {
                assert_eq!((parent, column), (new_parent, new_column));
                let kept = parent == "chat_execution_environments"
                    || parent == "chat_scratch_generations"
                    || parent == "chat_threads"
                    || (table == "chat_access_revocation_jobs"
                        && parent == "chat_assignment_authorization_revisions");
                if kept {
                    assert_eq!(action, old_action, "{table}.{column} -> {parent}");
                } else {
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
        assert_eq!(foreign_key_violations(&pool).await, 0);
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

#[test]
fn chat_local_tables_migration_rolls_back_when_a_foreign_key_is_violated() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_CHAT_LOCAL_TABLES).await;
        load_seeded_graph(&pool).await;
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO chat_access_revocation_jobs
                 (id, teammate_id, reason, available_at, created_at, updated_at)
             VALUES ('revocation-orphan', 'participant-missing', 'Access was reduced',
                     '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z',
                     '2026-10-23T00:00:00.000Z');
             PRAGMA foreign_keys = ON;",
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        drop(connection);
        let counts_before = row_counts(&pool).await;
        let runs_before = foreign_keys(&pool, "chat_agent_runs").await;

        let error = run_migrations(&pool).await.unwrap_err();
        assert!(error.contains("CHECK constraint failed"), "{error}");
        // The pool's only connection still holds the open migration transaction.
        sqlx::raw_sql("ROLLBACK; PRAGMA legacy_alter_table = OFF; PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(row_counts(&pool).await, counts_before);
        assert_eq!(foreign_keys(&pool, "chat_agent_runs").await, runs_before);
        let applied: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version = ?")
                .bind(CHAT_LOCAL_TABLES)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(applied, 0);
        let leftovers: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema
             WHERE name LIKE '%\\_next' ESCAPE '\\' OR name = 'chat_scheduled_message_dispatch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(leftovers, 0);
    });
}
