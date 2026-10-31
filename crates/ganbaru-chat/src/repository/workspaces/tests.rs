use super::*;
use crate::project_defaults::ensure_project_chat_defaults;

const EXTERNAL_FOLDER_ID: &str = "working-folder:external-removal";

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

fn now() -> UtcTimestamp {
    UtcTimestamp::new("2026-10-23T08:00:00.000Z".to_string()).unwrap()
}

/// Inserts a project with its managed folder and one external folder, plus the project's Chat
/// defaults, which give both folders execution environments.
async fn insert_project_with_external_folder(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-removal', 'Removal', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project-removal', 'group-removal', 'Removal', 'a0');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path)
         VALUES ('working-folder:project-removal', 'project-removal', 'Removal', 'managed',
                 'projects/project-removal');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, repository_kind, repository_identity, sort_order)
         VALUES ('working-folder:external-removal', 'project-removal', 'Repository', 'external',
                 'git', 'repository-identity', 10);",
    )
    .execute(pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ensure_project_chat_defaults(&mut connection, "project-removal", &now())
        .await
        .unwrap();
}

async fn folder_exists(pool: &SqlitePool) -> bool {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_working_folders WHERE id = ?)")
        .bind(EXTERNAL_FOLDER_ID)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn external_folder_removal_refuses_while_chat_references_the_folder() {
    let pool = migrated_pool().await;
    insert_project_with_external_folder(&pool).await;
    sqlx::query(
        "INSERT INTO chat_cleanup_jobs
             (id, working_folder_id, cleanup_kind, exact_target, available_at, created_at,
              updated_at)
         VALUES ('cleanup-removal', ?1, 'diagnostic_event', 'event-removal', ?2, ?2, ?2)",
    )
    .bind(EXTERNAL_FOLDER_ID)
    .bind(now().as_str())
    .execute(&pool)
    .await
    .unwrap();
    let folder = ProjectWorkingFolderId::new(EXTERNAL_FOLDER_ID.to_string()).unwrap();

    let error = remove_external_working_folder(&pool, &folder)
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Conflict);
    assert!(folder_exists(&pool).await);
    let cleanup_jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_cleanup_jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(cleanup_jobs, 1, "the refusal keeps the referencing rows");

    sqlx::query("DELETE FROM chat_cleanup_jobs WHERE id = 'cleanup-removal'")
        .execute(&pool)
        .await
        .unwrap();
    remove_external_working_folder(&pool, &folder)
        .await
        .unwrap();
    assert!(!folder_exists(&pool).await);
}

#[tokio::test]
async fn external_folder_removal_ignores_the_folder_execution_environment() {
    let pool = migrated_pool().await;
    insert_project_with_external_folder(&pool).await;
    let environments = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM chat_execution_environments WHERE working_folder_id = ?",
        )
        .bind(EXTERNAL_FOLDER_ID)
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    assert_eq!(environments().await, 1);
    let folder = ProjectWorkingFolderId::new(EXTERNAL_FOLDER_ID.to_string()).unwrap();

    remove_external_working_folder(&pool, &folder)
        .await
        .unwrap();

    assert!(!folder_exists(&pool).await);
    assert_eq!(environments().await, 0);
}
