use super::*;
use crate::attachment_files;
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

/// Project files created by [`insert_project_with_files`].
struct ProjectFiles {
    attachment: String,
    artifact: String,
}

/// Inserts a group with one project and its managed folder, then imports one attachment and
/// stores one browser artifact file with its resource row there.
async fn insert_project_with_files(pool: &SqlitePool, vault: &Path, name: &str) -> ProjectFiles {
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
    let attachment = import_attachment_bytes(
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
    .managed_relative_path;
    let artifact = format!("assets/chat/browser-artifacts/resource-{name}.png");
    let artifact_file = vault.join(&artifact);
    fs::create_dir_all(artifact_file.parent().unwrap()).unwrap();
    fs::write(&artifact_file, name.as_bytes()).unwrap();
    sqlx::query(
        "INSERT INTO chat_resources
             (id, working_folder_id, attachment_id, resource_kind, display_name, mime_type,
              byte_size, sha256, managed_relative_path, resource_uri, created_at)
         VALUES ('resource-' || ?1, 'working-folder:project-' || ?1, NULL, 'browser_screenshot',
                 ?1, 'image/png', ?2, ?3, ?4, 'ganbaru://chat/resource/resource-' || ?1, ?5)",
    )
    .bind(name)
    .bind(i64::try_from(name.len()).unwrap())
    .bind(attachment_files::digest(name.as_bytes()))
    .bind(&artifact)
    .bind(now.as_str())
    .execute(pool)
    .await
    .unwrap();
    ProjectFiles {
        attachment,
        artifact,
    }
}

#[tokio::test]
async fn deleted_project_artifacts_are_removed_and_other_projects_keep_theirs() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let deleted = insert_project_with_files(&pool, &vault.0, "deleted").await;
    let kept = insert_project_with_files(&pool, &vault.0, "kept").await;

    let paths = managed_file_paths_of_projects(&pool, &["project-deleted".to_string()])
        .await
        .unwrap();
    assert_eq!(paths, vec![deleted.artifact.clone()]);

    sqlx::query("DELETE FROM project_groups WHERE id = 'group-deleted'")
        .execute(&pool)
        .await
        .unwrap();
    let mut swept = paths.clone();
    swept.push(kept.artifact.clone());
    swept.push(deleted.attachment.clone());
    let failures = remove_unreferenced_managed_files(&pool, &vault.0, &swept)
        .await
        .unwrap();

    assert_eq!(failures, Vec::new());
    assert!(!vault.0.join(&deleted.artifact).exists());
    assert!(
        vault.0.join(&kept.artifact).is_file(),
        "a path still named by a row keeps its file"
    );
    assert!(
        vault.0.join(&deleted.attachment).is_file(),
        "attachment files are left to the asset sweep"
    );
}

#[tokio::test]
async fn files_stay_when_the_project_rows_were_not_deleted() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let files = insert_project_with_files(&pool, &vault.0, "rolled-back").await;
    let paths = managed_file_paths_of_projects(&pool, &["project-rolled-back".to_string()])
        .await
        .unwrap();

    let failures = remove_unreferenced_managed_files(&pool, &vault.0, &paths)
        .await
        .unwrap();

    assert_eq!(failures, Vec::new());
    assert!(vault.0.join(&files.artifact).is_file());
    assert!(vault.0.join(&files.attachment).is_file());
}
