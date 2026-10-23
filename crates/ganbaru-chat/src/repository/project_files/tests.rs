use super::*;
use crate::models::{ChatAttachmentId, ProjectWorkingFolderId, UtcTimestamp};
use crate::repository::attachments::{
    AttachmentBytesImport, ChatAttachmentKind, import_attachment_bytes,
};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestVault(PathBuf);

impl TestVault {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-project-files-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test vault should be created");
        Self(fs::canonicalize(path).expect("test vault should canonicalize"))
    }
}

impl Drop for TestVault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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

/// Inserts a group with one project and its managed folder, then imports one attachment there.
async fn insert_project_with_attachment(pool: &SqlitePool, vault: &Path, name: &str) -> String {
    sqlx::query(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-' || ?1, ?1, 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project-' || ?1, 'group-' || ?1, ?1, 'a0');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path)
         VALUES ('working-folder:project-' || ?1, 'project-' || ?1, ?1, 'managed',
                 'projects/project-' || ?1);",
    )
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    let folder = ProjectWorkingFolderId::new(format!("working-folder:project-{name}")).unwrap();
    let now = now();
    import_attachment_bytes(
        pool,
        vault,
        AttachmentBytesImport {
            working_folder_id: &folder,
            attachment_id: ChatAttachmentId::new(format!("attachment:{name}")).unwrap(),
            display_name: format!("{name}.txt"),
            bytes: name.as_bytes(),
            requested_kind: ChatAttachmentKind::TextSnippet,
            now: &now,
        },
    )
    .await
    .unwrap()
    .managed_relative_path
}

#[tokio::test]
async fn deleted_project_files_are_removed_and_other_projects_keep_theirs() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let deleted = insert_project_with_attachment(&pool, &vault.0, "deleted").await;
    let kept = insert_project_with_attachment(&pool, &vault.0, "kept").await;

    let paths = managed_file_paths_of_projects(&pool, &["project-deleted".to_string()])
        .await
        .unwrap();
    assert!(paths.contains(&deleted));
    assert!(!paths.contains(&kept));

    sqlx::query("DELETE FROM project_groups WHERE id = 'group-deleted'")
        .execute(&pool)
        .await
        .unwrap();
    let mut swept = paths.clone();
    swept.push(kept.clone());
    let failures = remove_unreferenced_managed_files(&pool, &vault.0, &swept)
        .await
        .unwrap();

    assert_eq!(failures, Vec::new());
    for path in &paths {
        assert!(!vault.0.join(path).exists(), "{path} is removed");
    }
    assert!(
        vault.0.join(&kept).is_file(),
        "a path still named by a row keeps its file"
    );
}

#[tokio::test]
async fn files_stay_when_the_project_rows_were_not_deleted() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let path = insert_project_with_attachment(&pool, &vault.0, "rolled-back").await;
    let paths = managed_file_paths_of_projects(&pool, &["project-rolled-back".to_string()])
        .await
        .unwrap();

    let failures = remove_unreferenced_managed_files(&pool, &vault.0, &paths)
        .await
        .unwrap();

    assert_eq!(failures, Vec::new());
    assert!(vault.0.join(&path).is_file());
}
