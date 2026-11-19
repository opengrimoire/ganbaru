use super::*;

const PROJECT_ID: &str = "project-attach";
const MANAGED_FOLDER_ID: &str = "working-folder:project-attach";
const EXTERNAL_FOLDER_ID: &str = "working-folder:attach-external";
const OTHER_PROJECT_FOLDER_ID: &str = "working-folder:project-other";

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
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-attach', 'Attach', 'a0');
         INSERT INTO projects (id, group_id, name, order_key) VALUES
             ('project-attach', 'group-attach', 'Attach', 'a0'),
             ('project-other', 'group-attach', 'Other', 'a1');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path) VALUES
             ('working-folder:project-attach', 'project-attach', 'Attach', 'managed',
              'projects/project-attach'),
             ('working-folder:project-other', 'project-other', 'Other', 'managed',
              'projects/project-other');
         INSERT INTO project_working_folders (id, project_id, display_name, kind, sort_order)
         VALUES ('working-folder:attach-external', 'project-attach', 'External', 'external', 10);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn folder_id(value: &str) -> ProjectWorkingFolderId {
    ProjectWorkingFolderId::new(value.to_string()).unwrap()
}

async fn store_primary(pool: &SqlitePool, working_folder_id: &str) {
    sqlx::query(
        "INSERT INTO project_primary_working_folders
            (project_id, working_folder_id, created_at, updated_at)
         VALUES (?, ?, '2026-11-19T08:00:00.000Z', '2026-11-19T08:00:00.000Z')
         ON CONFLICT (project_id) DO UPDATE SET working_folder_id = excluded.working_folder_id",
    )
    .bind(PROJECT_ID)
    .bind(working_folder_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn primary(pool: &SqlitePool) -> bool {
    is_effective_primary_folder(pool, &folder_id(EXTERNAL_FOLDER_ID))
        .await
        .unwrap()
}

#[tokio::test]
async fn phone_attachments_go_to_the_managed_folder_without_a_stored_primary() {
    let pool = migrated_pool().await;

    assert!(
        is_effective_primary_folder(&pool, &folder_id(MANAGED_FOLDER_ID))
            .await
            .unwrap()
    );
    assert!(!primary(&pool).await);
}

#[tokio::test]
async fn phone_attachments_follow_the_stored_primary_until_it_is_archived() {
    let pool = migrated_pool().await;
    store_primary(&pool, EXTERNAL_FOLDER_ID).await;
    assert!(primary(&pool).await);
    assert!(
        !is_effective_primary_folder(&pool, &folder_id(MANAGED_FOLDER_ID))
            .await
            .unwrap()
    );

    sqlx::query(
        "UPDATE project_working_folders SET archived_at = '2026-11-19T09:00:00.000Z' WHERE id = ?",
    )
    .bind(EXTERNAL_FOLDER_ID)
    .execute(&pool)
    .await
    .unwrap();

    assert!(!primary(&pool).await);
    assert!(
        is_effective_primary_folder(&pool, &folder_id(MANAGED_FOLDER_ID))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn phone_attachments_refuse_a_missing_folder_and_another_projects_primary() {
    let pool = migrated_pool().await;
    store_primary(&pool, OTHER_PROJECT_FOLDER_ID).await;

    assert!(
        !is_effective_primary_folder(&pool, &folder_id("working-folder:missing"))
            .await
            .unwrap()
    );
    assert!(
        is_effective_primary_folder(&pool, &folder_id(MANAGED_FOLDER_ID))
            .await
            .unwrap(),
        "a stored primary of another project falls back to the managed folder"
    );
    assert!(
        is_effective_primary_folder(&pool, &folder_id(OTHER_PROJECT_FOLDER_ID))
            .await
            .unwrap(),
        "the other project's managed folder is its own primary"
    );
}

#[test]
fn image_imports_refuse_oversized_or_invalid_base64_before_storing() {
    let small = general_purpose::STANDARD.encode([0x89, b'P', b'N', b'G']);
    assert_eq!(decode_image(&small).unwrap(), vec![0x89, b'P', b'N', b'G']);
    let exact = general_purpose::STANDARD.encode(vec![0_u8; MAX_IMAGE_BYTES]);
    assert_eq!(exact.len(), MAX_IMAGE_BASE64_CHARS);
    assert_eq!(decode_image(&exact).unwrap().len(), MAX_IMAGE_BYTES);
    let over = general_purpose::STANDARD.encode(vec![0_u8; MAX_IMAGE_BYTES + 1]);
    assert_eq!(
        decode_image(&over).unwrap_err().code,
        ChatErrorCode::Validation
    );
    assert!(decode_image("not base64!").is_err());
}

#[test]
fn text_snippet_bound_is_named_and_refuses_over_limit() {
    assert!(validate_text_snippet(&"a".repeat(MAX_TEXT_SNIPPET_BYTES)).is_ok());
    let multibyte = "\u{E9}".repeat(MAX_TEXT_SNIPPET_BYTES / 2);
    assert!(validate_text_snippet(&multibyte).is_ok());
    let over = validate_text_snippet(&"a".repeat(MAX_TEXT_SNIPPET_BYTES + 1))
        .expect_err("text over the bound is refused");
    assert_eq!(over.code, ChatErrorCode::Validation);
    assert!(validate_text_snippet(&format!("{multibyte}a")).is_err());
    assert!(validate_text_snippet("").is_err());
    assert!(validate_text_snippet("log\0line").is_err());
}
