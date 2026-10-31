use super::*;
use ganbaru_chat::repository::workspaces::{
    remove_external_working_folder, set_workspace_archived,
};

const PROJECT_ID: &str = "project-primary";
const MANAGED_FOLDER_ID: &str = "working-folder:project-primary";
const EXTERNAL_FOLDER_ID: &str = "working-folder:primary-external";
const OTHER_PROJECT_FOLDER_ID: &str = "working-folder:project-other";
const CREATED_AT: &str = "2026-10-23T08:00:00.000Z";

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
    sqlx::raw_sql(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-primary', 'Primary', 'a0');
         INSERT INTO projects (id, group_id, name, order_key) VALUES
             ('project-primary', 'group-primary', 'Primary', 'a0'),
             ('project-other', 'group-primary', 'Other', 'a1');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path) VALUES
             ('working-folder:project-primary', 'project-primary', 'Primary', 'managed',
              'projects/project-primary'),
             ('working-folder:project-other', 'project-other', 'Other', 'managed',
              'projects/project-other');
         INSERT INTO project_working_folders (id, project_id, display_name, kind, sort_order)
         VALUES ('working-folder:primary-external', 'project-primary', 'External', 'external', 10);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn now() -> UtcTimestamp {
    UtcTimestamp::new(CREATED_AT.to_string()).unwrap()
}

fn folder_id(value: &str) -> ProjectWorkingFolderId {
    ProjectWorkingFolderId::new(value.to_string()).unwrap()
}

async fn store_primary(pool: &SqlitePool, working_folder_id: &str) {
    sqlx::query(
        "INSERT INTO project_primary_working_folders
            (project_id, working_folder_id, created_at, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (project_id) DO UPDATE SET working_folder_id = excluded.working_folder_id",
    )
    .bind(PROJECT_ID)
    .bind(working_folder_id)
    .bind(CREATED_AT)
    .bind(CREATED_AT)
    .execute(pool)
    .await
    .unwrap();
}

async fn effective(pool: &SqlitePool) -> ChatProjectPrimaryWorkingFolderRead {
    read_effective_primary_working_folder(pool, PROJECT_ID)
        .await
        .unwrap()
}

async fn folder_revision(pool: &SqlitePool, id: &str) -> u64 {
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM project_working_folders WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
    u64::try_from(revision).unwrap()
}

#[tokio::test]
async fn effective_primary_working_folder_is_the_stored_active_folder_of_the_project() {
    let pool = migrated_pool().await;
    store_primary(&pool, EXTERNAL_FOLDER_ID).await;

    let read = effective(&pool).await;

    assert_eq!(read.working_folder_id.as_str(), EXTERNAL_FOLDER_ID);
    assert_eq!(read.revision, 1);
}

#[tokio::test]
async fn effective_primary_working_folder_falls_back_to_the_managed_folder() {
    let pool = migrated_pool().await;
    for stored in [
        "working-folder:not-yet-arrived",
        OTHER_PROJECT_FOLDER_ID,
        EXTERNAL_FOLDER_ID,
    ] {
        store_primary(&pool, stored).await;
        if stored == EXTERNAL_FOLDER_ID {
            sqlx::query("UPDATE project_working_folders SET archived_at = ? WHERE id = ?")
                .bind(CREATED_AT)
                .bind(EXTERNAL_FOLDER_ID)
                .execute(&pool)
                .await
                .unwrap();
        }
        assert_eq!(
            effective(&pool).await.working_folder_id.as_str(),
            MANAGED_FOLDER_ID,
            "stored {stored}"
        );
    }
}

#[tokio::test]
async fn primary_working_folder_without_a_stored_row_reads_the_managed_folder_at_revision_zero() {
    let pool = migrated_pool().await;

    let read = effective(&pool).await;
    assert_eq!(read.working_folder_id.as_str(), MANAGED_FOLDER_ID);
    assert_eq!(read.revision, 0);

    let set =
        set_primary_working_folder(&pool, PROJECT_ID, &folder_id(EXTERNAL_FOLDER_ID), 0, &now())
            .await
            .unwrap();
    assert_eq!(set.working_folder_id.as_str(), EXTERNAL_FOLDER_ID);
    assert_eq!(set.revision, 1);

    let stale =
        set_primary_working_folder(&pool, PROJECT_ID, &folder_id(MANAGED_FOLDER_ID), 0, &now())
            .await
            .unwrap_err();
    assert_eq!(stale.code, ChatErrorCode::StaleRevision);
}

#[tokio::test]
async fn primary_working_folder_set_refuses_a_foreign_missing_or_archived_folder() {
    let pool = migrated_pool().await;
    store_primary(&pool, MANAGED_FOLDER_ID).await;
    sqlx::query("UPDATE project_working_folders SET archived_at = ? WHERE id = ?")
        .bind(CREATED_AT)
        .bind(EXTERNAL_FOLDER_ID)
        .execute(&pool)
        .await
        .unwrap();

    for candidate in [
        OTHER_PROJECT_FOLDER_ID,
        "working-folder:missing",
        EXTERNAL_FOLDER_ID,
    ] {
        let error = set_primary_working_folder(&pool, PROJECT_ID, &folder_id(candidate), 1, &now())
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            ChatErrorCode::Validation,
            "candidate {candidate}"
        );
    }
    assert_eq!(
        effective(&pool).await.working_folder_id.as_str(),
        MANAGED_FOLDER_ID
    );
}

#[tokio::test]
async fn stored_primary_working_folder_cannot_be_archived_or_removed() {
    let pool = migrated_pool().await;
    store_primary(&pool, EXTERNAL_FOLDER_ID).await;
    let external = folder_id(EXTERNAL_FOLDER_ID);
    let revision = folder_revision(&pool, EXTERNAL_FOLDER_ID).await;

    let archive = set_workspace_archived(&pool, &external, true, revision, &now())
        .await
        .unwrap_err();
    assert_eq!(archive.code, ChatErrorCode::Conflict);
    let remove = remove_external_working_folder(&pool, &external)
        .await
        .unwrap_err();
    assert_eq!(remove.code, ChatErrorCode::Conflict);

    set_primary_working_folder(&pool, PROJECT_ID, &folder_id(MANAGED_FOLDER_ID), 1, &now())
        .await
        .unwrap();
    let archived = set_workspace_archived(&pool, &external, true, revision, &now())
        .await
        .unwrap();
    assert!(archived.archived_at.is_some());
    remove_external_working_folder(&pool, &external)
        .await
        .unwrap();
}
