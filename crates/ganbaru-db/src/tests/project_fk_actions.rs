use super::helpers::{load_project_chat_graph, migrated_memory_pool};
use sqlx::{AssertSqlSafe, SqlitePool};
use std::collections::BTreeMap;

/// Tables with foreign keys into the project graph.
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
fn project_owned_foreign_keys_cascade() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;

        for table in REBUILT_TABLES {
            for (parent, column, action) in foreign_keys(&pool, table).await {
                let cascades = parent == "projects"
                    || parent == "project_working_folders"
                    || parent == "project_sections"
                    || parent == "project_statuses"
                    || (table == "chat_scratch_promotions"
                        && (parent == "chat_channels" || parent == "chat_scratch_generations"))
                    || table == "chat_scratch_cleanup_jobs";
                if cascades {
                    assert_eq!(action, "CASCADE", "{table}.{column} -> {parent}");
                }
            }
        }
        assert_eq!(foreign_key_violations(&pool).await, 0);
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
        assert_eq!(
            count_where(
                &pool,
                "chat_working_folder_reference_targets",
                "working_folder_id = 'folder-archived'"
            )
            .await,
            1,
            "a mention target keeps its unchecked folder id and renders the label snapshot"
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
