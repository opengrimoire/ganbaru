use super::helpers::{
    load_project_chat_graph, memory_pool_migrated_through, migrate_through, migrated_memory_pool,
};
use crate::run_migrations;
use sqlx::{AssertSqlSafe, Row, SqlitePool};
use std::collections::BTreeMap;

const BEFORE_FK_ACTIONS: i64 = 20261023045510;
const FK_ACTIONS: i64 = 20261023071334;

/// Tables the migration rebuilds.
const REBUILT_TABLES: [&str; 14] = [
    "project_tasks",
    "chat_threads",
    "chat_agent_runs",
    "chat_assignment_authorization_revisions",
    "chat_assignment_authorized_folder_sources",
    "chat_attachments",
    "chat_cleanup_jobs",
    "chat_resources",
    "chat_scheduled_message_references",
    "chat_scratch_promotions",
    "chat_scratch_cleanup_jobs",
    "chat_teammate_working_folder_grants",
    "chat_working_folder_reference_targets",
    "chat_workspace_path_reference_targets",
];

/// Delete triggers that deny deletion only while the row's parents exist.
const PARENT_GUARDED_TRIGGERS: [&str; 5] = [
    "chat_assignment_authorization_delete_denied",
    "chat_authorized_channel_source_immutable_delete",
    "chat_authorized_folder_source_immutable_delete",
    "chat_scratch_generation_source_delete_denied",
    "chat_scratch_promotion_audit_delete_denied",
];

/// Tables that hold rows of the fixture project.
const PROJECT_ROW_TABLES: [&str; 18] = [
    "projects",
    "project_working_folders",
    "project_sections",
    "project_statuses",
    "project_priorities",
    "chat_conversations",
    "chat_execution_environments",
    "chat_work_assignments",
    "chat_assignment_authorized_channel_sources",
    "chat_scratch_generations",
    "chat_scratch_generation_sources",
    "chat_threads",
    "chat_agent_runs",
    "chat_attachments",
    "chat_resources",
    "chat_scheduled_message_references",
    "chat_scratch_promotions",
    "chat_scratch_cleanup_jobs",
];

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

/// Index and trigger SQL keyed by (type, name) for every object on the rebuilt tables.
async fn rebuilt_table_objects(pool: &SqlitePool) -> BTreeMap<(String, String), Option<String>> {
    let mut objects = BTreeMap::new();
    for table in REBUILT_TABLES {
        let rows = sqlx::query(
            "SELECT type, name, sql FROM sqlite_schema
             WHERE tbl_name = ? AND type IN ('index', 'trigger')",
        )
        .bind(table)
        .fetch_all(pool)
        .await
        .unwrap();
        for row in rows {
            objects.insert((row.get("type"), row.get("name")), row.get("sql"));
        }
    }
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

#[test]
fn fk_actions_migration_keeps_rows_values_and_schema_objects() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_FK_ACTIONS).await;
        load_project_chat_graph(&pool).await;
        let counts_before = row_counts(&pool).await;
        let objects_before = rebuilt_table_objects(&pool).await;
        let thread_before: (String, String, String, String) = sqlx::query_as(
            "SELECT title, title_search, execution_environment_id, state
             FROM chat_threads WHERE id = 'thread-archived'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut foreign_keys_before = BTreeMap::new();
        for table in REBUILT_TABLES {
            foreign_keys_before.insert(table, foreign_keys(&pool, table).await);
        }

        migrate_through(&pool, FK_ACTIONS).await.unwrap();

        assert_eq!(row_counts(&pool).await, counts_before);
        assert_eq!(foreign_key_violations(&pool).await, 0);
        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(enabled, 1);

        let objects_after = rebuilt_table_objects(&pool).await;
        assert_eq!(
            objects_after.keys().collect::<Vec<_>>(),
            objects_before.keys().collect::<Vec<_>>(),
            "every index and trigger on a rebuilt table survives"
        );
        for (key, sql) in &objects_before {
            if !PARENT_GUARDED_TRIGGERS.contains(&key.1.as_str()) {
                assert_eq!(&objects_after[key], sql, "{} {} is unchanged", key.0, key.1);
            }
        }

        for table in REBUILT_TABLES {
            let before = &foreign_keys_before[table];
            let after = foreign_keys(&pool, table).await;
            assert_eq!(after.len(), before.len(), "{table} keeps its foreign keys");
            for ((parent, column, old_action), (new_parent, new_column, action)) in
                before.iter().zip(&after)
            {
                assert_eq!((parent, column), (new_parent, new_column));
                let cascades = parent == "projects"
                    || parent == "project_working_folders"
                    || parent == "project_sections"
                    || parent == "project_statuses"
                    || (table == "chat_scratch_promotions"
                        && (parent == "chat_channels" || parent == "chat_scratch_generations"))
                    || table == "chat_scratch_cleanup_jobs";
                let expected = if cascades { "CASCADE" } else { old_action };
                assert_eq!(action, expected, "{table}.{column} -> {parent}");
            }
        }

        let thread_after: (String, String, String, String) = sqlx::query_as(
            "SELECT title, title_search, execution_environment_id, state
             FROM chat_threads WHERE id = 'thread-archived'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(thread_after, thread_before);
        assert_eq!(
            count_where(
                &pool,
                "chat_assignment_authorized_folder_sources",
                "working_folder_id = 'folder-archived'"
            )
            .await,
            1
        );
        assert_eq!(
            count_where(
                &pool,
                "project_tasks",
                "id = 'task-retired' AND status_id = 'status-retired'
                 AND priority = 'priority-retired'"
            )
            .await,
            1
        );
        let retired: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM project_statuses WHERE deleted_at IS NOT NULL
             UNION ALL
             SELECT id FROM project_priorities WHERE deleted_at IS NOT NULL",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(retired, vec!["status-retired", "priority-retired"]);
    });
}

#[test]
fn fk_actions_migration_rolls_back_when_a_foreign_key_is_violated() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_FK_ACTIONS).await;
        load_project_chat_graph(&pool).await;
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO chat_cleanup_jobs
                 (id, working_folder_id, cleanup_kind, exact_target, available_at,
                  created_at, updated_at)
             VALUES ('cleanup-orphan', 'folder-missing', 'diagnostic_event', 'event-orphan',
                     '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z',
                     '2026-10-23T00:00:00.000Z');
             PRAGMA foreign_keys = ON;",
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        drop(connection);
        let counts_before = row_counts(&pool).await;
        let threads_before = foreign_keys(&pool, "chat_threads").await;

        let error = run_migrations(&pool).await.unwrap_err();
        assert!(error.contains("CHECK constraint failed"), "{error}");
        // The pool's only connection still holds the open migration transaction.
        sqlx::raw_sql("ROLLBACK; PRAGMA legacy_alter_table = OFF; PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(row_counts(&pool).await, counts_before);
        assert_eq!(foreign_keys(&pool, "chat_threads").await, threads_before);
        let applied: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version = ?")
                .bind(FK_ACTIONS)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(applied, 0);
        let leftovers: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE '%\\_next' ESCAPE '\\'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(leftovers, 0);
    });
}

#[test]
fn sections_and_statuses_cascade_to_their_tasks() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;

        sqlx::query("DELETE FROM project_statuses WHERE id = 'status-retired'")
            .execute(&pool)
            .await
            .unwrap();
        let tasks: Vec<String> = sqlx::query_scalar("SELECT id FROM project_tasks ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(tasks, vec!["task-child", "task-now"]);

        sqlx::query("DELETE FROM project_sections WHERE id = 'section-now'")
            .execute(&pool)
            .await
            .unwrap();
        let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM project_tasks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(tasks, 0, "the child task follows its parent");
        assert_eq!(foreign_key_violations(&pool).await, 0);
    });
}

#[test]
fn deleting_a_working_folder_removes_only_its_chat_rows() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;

        sqlx::query("DELETE FROM project_working_folders WHERE id = 'folder-archived'")
            .execute(&pool)
            .await
            .unwrap();

        for (table, column) in [
            ("chat_threads", "working_folder_id"),
            ("chat_attachments", "working_folder_id"),
            ("chat_cleanup_jobs", "working_folder_id"),
            (
                "chat_assignment_authorized_folder_sources",
                "working_folder_id",
            ),
            ("chat_scheduled_message_references", "working_folder_id"),
            ("chat_teammate_working_folder_grants", "working_folder_id"),
            ("chat_working_folder_reference_targets", "working_folder_id"),
        ] {
            assert_eq!(
                count_where(&pool, table, &format!("{column} = 'folder-archived'")).await,
                0,
                "{table} rows of the folder are removed"
            );
            assert!(
                count_where(&pool, table, &format!("{column} = 'folder-managed'")).await > 0
                    || table == "chat_cleanup_jobs",
                "{table} rows of the other folder remain"
            );
        }
        assert_eq!(
            count_where(
                &pool,
                "chat_assignment_authorization_revisions",
                "id = 'authorization-1'"
            )
            .await,
            1
        );
        assert_eq!(foreign_key_violations(&pool).await, 0);
    });
}

#[test]
fn deleting_a_group_removes_its_projects_with_their_chat_rows() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let seeded = row_counts(&pool).await;
        load_project_chat_graph(&pool).await;

        sqlx::query("DELETE FROM project_groups WHERE id = 'group-chat'")
            .execute(&pool)
            .await
            .unwrap();

        let remaining = row_counts(&pool).await;
        for table in PROJECT_ROW_TABLES.iter().chain(REBUILT_TABLES.iter()) {
            assert_eq!(
                remaining[*table], seeded[*table],
                "{table} keeps only the seeded rows"
            );
        }
        assert_eq!(foreign_key_violations(&pool).await, 0);
    });
}

#[test]
fn audit_rows_stay_immutable_while_their_parents_exist() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;

        for table in [
            "chat_assignment_authorization_revisions",
            "chat_assignment_authorized_folder_sources",
            "chat_assignment_authorized_channel_sources",
            "chat_scratch_generation_sources",
            "chat_scratch_promotions",
        ] {
            let result = sqlx::query(AssertSqlSafe(format!("DELETE FROM {table}")))
                .execute(&pool)
                .await;
            assert!(result.is_err(), "{table} rows cannot be deleted directly");
        }
    });
}
