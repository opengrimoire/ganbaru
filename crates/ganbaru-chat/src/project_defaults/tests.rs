use super::*;
use sqlx::SqlitePool;

const PROJECT_ID: &str = "project-defaults";
const MANAGED_FOLDER_ID: &str = "working-folder:project-defaults";
const EXTERNAL_FOLDER_ID: &str = "working-folder:external-defaults";

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

async fn insert_project_without_chat_rows(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-defaults', 'Defaults', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project-defaults', 'group-defaults', 'Defaults', 'a0');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path)
         VALUES ('working-folder:project-defaults', 'project-defaults', 'Defaults', 'managed',
                 'projects/project-defaults');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, repository_kind, repository_identity, sort_order)
         VALUES ('working-folder:external-defaults', 'project-defaults', 'Repository', 'external',
                 'git', 'repository-identity', 10);",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn ensure(pool: &SqlitePool, project_id: &str) -> ChatResult<()> {
    let mut connection = pool.acquire().await.unwrap();
    ensure_project_chat_defaults(&mut connection, project_id, &now()).await
}

async fn chat_row_counts(pool: &SqlitePool) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM chat_conversations),
                (SELECT count(*) FROM chat_channels),
                (SELECT count(*) FROM chat_conversation_memberships),
                (SELECT count(*) FROM chat_execution_environments),
                (SELECT coalesce(sum(revision), 0) FROM chat_conversation_audience_state)",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn default_channel(pool: &SqlitePool, project_id: &str) -> (String, String, String) {
    sqlx::query_as(
        "SELECT id, conversation_id, name FROM chat_channels
         WHERE project_id = ? AND is_default = 1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn default_channel_uses_derived_ids_with_owner_membership_and_folder_environments() {
    let pool = migrated_pool().await;
    insert_project_without_chat_rows(&pool).await;
    let empty: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_channels WHERE project_id = ?")
        .bind(PROJECT_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(empty, 0);

    ensure(&pool, PROJECT_ID).await.unwrap();

    let (conversation_id, channel_id) = default_channel_ids(PROJECT_ID).unwrap();
    assert!(conversation_id.starts_with("conversation-default:"));
    assert!(channel_id.starts_with("channel-default:"));
    assert_eq!(
        default_channel(&pool, PROJECT_ID).await,
        (
            channel_id,
            conversation_id.clone(),
            DEFAULT_CHANNEL_NAME.to_string()
        )
    );
    let owner: String = sqlx::query_scalar(
        "SELECT membership_role FROM chat_conversation_memberships
         WHERE conversation_id = ? AND participant_id = ?",
    )
    .bind(&conversation_id)
    .bind(LOCAL_OWNER_PARTICIPANT_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner, "owner");
    let environments: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, working_folder_id, display_name, repository_identity
         FROM chat_execution_environments
         WHERE working_folder_id IN (?, ?) ORDER BY id",
    )
    .bind(MANAGED_FOLDER_ID)
    .bind(EXTERNAL_FOLDER_ID)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        environments,
        vec![
            (
                current_folder_environment_id(EXTERNAL_FOLDER_ID),
                EXTERNAL_FOLDER_ID.to_string(),
                "Repository".to_string(),
                Some("repository-identity".to_string()),
            ),
            (
                current_folder_environment_id(MANAGED_FOLDER_ID),
                MANAGED_FOLDER_ID.to_string(),
                "Defaults".to_string(),
                None,
            ),
        ]
    );
}

#[tokio::test]
async fn default_chat_rows_are_created_once() {
    let pool = migrated_pool().await;
    insert_project_without_chat_rows(&pool).await;
    ensure_everything(&pool).await;
    let first = chat_row_counts(&pool).await;
    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(first.1, projects, "every project has one default channel");

    ensure_everything(&pool).await;

    assert_eq!(chat_row_counts(&pool).await, first);
}

/// Runs every entry point that creates default Chat rows once.
async fn ensure_everything(pool: &SqlitePool) {
    ensure(pool, PROJECT_ID).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ensure_all_project_chat_defaults(&mut connection, &now())
        .await
        .unwrap();
    ensure_working_folder_chat_defaults(&mut connection, EXTERNAL_FOLDER_ID, &now())
        .await
        .unwrap();
}

#[tokio::test]
async fn existing_default_channel_keeps_its_id() {
    let pool = migrated_pool().await;
    insert_project_without_chat_rows(&pool).await;
    let conversation_id = "conversation:used-default";
    let channel_id = "channel:used-default";
    let owner_key =
        crate::conversion::membership_key(conversation_id, LOCAL_OWNER_PARTICIPANT_ID).unwrap();
    let created = now();
    let created = created.as_str();
    sqlx::query(
        "INSERT INTO chat_conversations
             (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES (?, ?, 'channel', ?, ?, ?)",
    )
    .bind(conversation_id)
    .bind(PROJECT_ID)
    .bind(created)
    .bind(created)
    .bind(created)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_channels
             (id, project_id, conversation_id, name, is_default, created_at, updated_at)
         VALUES (?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(channel_id)
    .bind(PROJECT_ID)
    .bind(conversation_id)
    .bind(DEFAULT_CHANNEL_NAME)
    .bind(created)
    .bind(created)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_conversation_memberships
             (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES (?, ?, ?, 'owner', ?, ?)",
    )
    .bind(conversation_id)
    .bind(LOCAL_OWNER_PARTICIPANT_ID)
    .bind(owner_key)
    .bind(created)
    .bind(created)
    .execute(&pool)
    .await
    .unwrap();

    ensure(&pool, PROJECT_ID).await.unwrap();
    let before = chat_row_counts(&pool).await;
    ensure(&pool, PROJECT_ID).await.unwrap();

    let channels: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_channels WHERE project_id = ?")
            .bind(PROJECT_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(channels, 1);
    assert_eq!(default_channel(&pool, PROJECT_ID).await.0, channel_id);
    assert_ne!(channel_id, default_channel_ids(PROJECT_ID).unwrap().1);
    assert_eq!(chat_row_counts(&pool).await, before);
}

#[tokio::test]
async fn default_environment_is_not_recreated_beside_an_archived_current_folder_row() {
    let pool = migrated_pool().await;
    insert_project_without_chat_rows(&pool).await;
    ensure(&pool, PROJECT_ID).await.unwrap();
    sqlx::query("UPDATE chat_execution_environments SET archived_at = ? WHERE id = ?")
        .bind(now().as_str())
        .bind(current_folder_environment_id(EXTERNAL_FOLDER_ID))
        .execute(&pool)
        .await
        .unwrap();
    let before = chat_row_counts(&pool).await;

    ensure(&pool, PROJECT_ID).await.unwrap();

    assert_eq!(chat_row_counts(&pool).await, before);
}

#[tokio::test]
async fn archived_working_folders_get_no_current_folder_environment() {
    let pool = migrated_pool().await;
    insert_project_without_chat_rows(&pool).await;
    sqlx::query("UPDATE project_working_folders SET archived_at = ? WHERE id = ?")
        .bind(now().as_str())
        .bind(EXTERNAL_FOLDER_ID)
        .execute(&pool)
        .await
        .unwrap();

    ensure(&pool, PROJECT_ID).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ensure_all_project_chat_defaults(&mut connection, &now())
        .await
        .unwrap();
    drop(connection);

    let environments: Vec<String> = sqlx::query_scalar(
        "SELECT environment.id FROM chat_execution_environments environment
         JOIN project_working_folders folder ON folder.id = environment.working_folder_id
         WHERE folder.project_id = ?
         ORDER BY environment.id",
    )
    .bind(PROJECT_ID)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        environments,
        vec![current_folder_environment_id(MANAGED_FOLDER_ID)]
    );
}

#[tokio::test]
async fn all_project_defaults_cover_every_project_missing_chat_rows() {
    let pool = migrated_pool().await;
    insert_project_without_chat_rows(&pool).await;
    sqlx::raw_sql(
        "INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project-second', 'group-defaults', 'Second', 'a1');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path)
         VALUES ('working-folder:project-second', 'project-second', 'Second', 'managed',
                 'projects/project-second');",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut connection = pool.acquire().await.unwrap();
    ensure_all_project_chat_defaults(&mut connection, &now())
        .await
        .unwrap();
    drop(connection);

    let missing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM projects project
         WHERE NOT EXISTS (
             SELECT 1 FROM chat_channels WHERE project_id = project.id AND is_default = 1
         )",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(missing, 0);
    let second_environment: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_execution_environments
         WHERE id = 'current-folder:working-folder:project-second'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(second_environment, 1);
}

#[tokio::test]
async fn default_chat_rows_refuse_a_missing_project_or_folder() {
    let pool = migrated_pool().await;
    let error = ensure(&pool, "project-missing").await.unwrap_err();
    assert_eq!(error.code, ChatErrorCode::NotFound);
    let mut connection = pool.acquire().await.unwrap();
    let error =
        ensure_working_folder_chat_defaults(&mut connection, "working-folder:missing", &now())
            .await
            .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::NotFound);
}
