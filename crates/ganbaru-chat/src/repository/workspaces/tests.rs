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

#[tokio::test]
async fn list_workspaces_skips_a_row_that_does_not_parse() {
    let pool = migrated_pool().await;
    insert_project_with_external_folder(&pool).await;
    sqlx::raw_sql(
        "INSERT INTO project_working_folders
             (id, project_id, display_name, kind, sort_order, archived_at)
         VALUES ('working-folder:unreadable', 'project-removal', 'Unreadable', 'external', 20,
                 'not a timestamp at all');",
    )
    .execute(&pool)
    .await
    .unwrap();

    let folders = list_workspaces(&pool)
        .await
        .expect("an unreadable row must not fail the list");

    let ids: Vec<&str> = folders
        .iter()
        .filter(|folder| folder.project_id == "project-removal")
        .map(|folder| folder.id.as_str())
        .collect();
    assert_eq!(ids, ["working-folder:project-removal", EXTERNAL_FOLDER_ID]);
}

#[tokio::test]
async fn create_workspace_refuses_a_managed_folder_id() {
    let pool = migrated_pool().await;
    insert_project_with_external_folder(&pool).await;
    for reserved in [
        "working-folder:project-other",
        "working-folder-routine-other",
    ] {
        let request = CreateProjectWorkingFolderRequest {
            id: ProjectWorkingFolderId::new(reserved.to_string()).unwrap(),
            project_id: "project-removal".to_string(),
            display_name: "Repository".to_string(),
        };
        let error = create_workspace(&pool, &request, &now()).await.unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation, "{reserved}");
    }

    let request = CreateProjectWorkingFolderRequest {
        id: ProjectWorkingFolderId::new(
            "working-folder:external:0f8e2c4a-6b1d-4e3f-9a7c-5d2b8e1f0a63".to_string(),
        )
        .unwrap(),
        project_id: "project-removal".to_string(),
        display_name: "Repository".to_string(),
    };
    let created = create_workspace(&pool, &request, &now()).await.unwrap();
    assert_eq!(created.id, request.id);
}

#[tokio::test]
async fn list_workspaces_keeps_query_errors_fatal() {
    let pool = migrated_pool().await;
    pool.close().await;

    let error = list_workspaces(&pool).await.unwrap_err();

    assert_eq!(error.code, ChatErrorCode::Persistence);
}

const MANAGED_FOLDER_ID: &str = "working-folder:project-removal";

/// Stores `working_folder_id` as the primary folder of the test project.
async fn store_primary(pool: &SqlitePool, working_folder_id: &str) {
    sqlx::query(
        "INSERT INTO project_primary_working_folders
            (project_id, working_folder_id, created_at, updated_at)
         VALUES ('project-removal', ?1, ?2, ?2)",
    )
    .bind(working_folder_id)
    .bind(now().as_str())
    .execute(pool)
    .await
    .unwrap();
}

/// Gives the folder a timestamp that the schema accepts and the reader refuses.
async fn make_unreadable(pool: &SqlitePool, working_folder_id: &str) {
    sqlx::query("UPDATE project_working_folders SET updated_at = 'not a timestamp' WHERE id = ?")
        .bind(working_folder_id)
        .execute(pool)
        .await
        .unwrap();
}

async fn resolved_primary_id(pool: &SqlitePool) -> ChatResult<Option<String>> {
    resolve_primary_workspace(pool, "project-removal")
        .await
        .map(|folder| folder.map(|folder| folder.id.as_str().to_string()))
}

#[tokio::test]
async fn resolve_primary_workspace_reads_the_stored_folder_and_then_the_managed_folder() {
    let pool = migrated_pool().await;
    insert_project_with_external_folder(&pool).await;
    assert_eq!(
        resolved_primary_id(&pool).await.unwrap().as_deref(),
        Some(MANAGED_FOLDER_ID)
    );

    store_primary(&pool, EXTERNAL_FOLDER_ID).await;

    assert_eq!(
        resolved_primary_id(&pool).await.unwrap().as_deref(),
        Some(EXTERNAL_FOLDER_ID)
    );
}

#[tokio::test]
async fn resolve_primary_workspace_skips_a_stored_primary_that_does_not_parse() {
    let pool = migrated_pool().await;
    insert_project_with_external_folder(&pool).await;
    store_primary(&pool, EXTERNAL_FOLDER_ID).await;
    make_unreadable(&pool, EXTERNAL_FOLDER_ID).await;

    let resolved = resolved_primary_id(&pool).await;

    assert!(
        matches!(
            resolved.as_ref().map(Option::as_deref),
            Ok(Some(MANAGED_FOLDER_ID))
        ),
        "an unreadable stored primary must fall back to the managed folder, got {resolved:?}"
    );
}

#[tokio::test]
async fn resolve_primary_workspace_reads_no_folder_when_no_candidate_parses() {
    let pool = migrated_pool().await;
    insert_project_with_external_folder(&pool).await;
    make_unreadable(&pool, MANAGED_FOLDER_ID).await;

    let resolved = resolved_primary_id(&pool).await;
    assert!(
        matches!(resolved, Ok(None)),
        "an unreadable managed folder must read as no primary, got {resolved:?}"
    );

    store_primary(&pool, EXTERNAL_FOLDER_ID).await;
    assert_eq!(
        resolved_primary_id(&pool).await.unwrap().as_deref(),
        Some(EXTERNAL_FOLDER_ID),
        "a readable stored primary still wins over an unreadable managed folder"
    );
}
