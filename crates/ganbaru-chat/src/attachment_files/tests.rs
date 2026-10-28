use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

const NOW: &str = "2026-10-27T08:00:00.000Z";
const FOLDER_ID: &str = "folder:attachments";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nattachment";

struct TestVault(PathBuf);

impl TestVault {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-attachment-files-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test vault should be created");
        Self(fs::canonicalize(path).expect("test vault should canonicalize"))
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn exists(&self, relative: &str) -> bool {
        self.0.join(relative).exists()
    }

    fn files(&self) -> Vec<String> {
        let directory = self.0.join(ATTACHMENT_DIRECTORY);
        let mut names: Vec<String> = fs::read_dir(directory)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
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
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-files', 'F', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project-files', 'group-files', 'Files', 'a0');
         INSERT INTO project_working_folders (id, project_id, display_name, kind)
         VALUES ('{FOLDER_ID}', 'project-files', 'Code', 'external');"
    )))
    .execute(&pool)
    .await
    .unwrap();
    pool
}

/// Inserts an attachment row naming `path`, as an older vault wrote it.
async fn insert_attachment(pool: &SqlitePool, id: &str, bytes: &[u8], mime: &str, path: &str) {
    let kind = if mime == "text/plain" {
        "text_snippet"
    } else {
        "image"
    };
    sqlx::query(
        "INSERT INTO chat_attachments
            (id, working_folder_id, kind, original_display_name, mime_type, byte_size, sha256,
             managed_relative_path, signature_kind, created_at)
         VALUES (?, ?, ?, 'file', ?, ?, ?, ?, 'test', ?)",
    )
    .bind(id)
    .bind(FOLDER_ID)
    .bind(kind)
    .bind(mime)
    .bind(i64::try_from(bytes.len()).unwrap())
    .bind(digest(bytes))
    .bind(path)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

/// Every attachment and resource path, as (table, id, path).
async fn stored_paths(pool: &SqlitePool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT 'attachment', id, managed_relative_path FROM chat_attachments
         UNION ALL
         SELECT 'resource', attachment_id, managed_relative_path FROM chat_resources
         WHERE attachment_id IS NOT NULL
         ORDER BY 1, 2",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[test]
fn file_names_follow_the_sync_asset_naming() {
    let sha = digest(PNG);
    assert_eq!(
        relative_path(&sha, "image/png"),
        format!("assets/chat/attachments/{sha}.png")
    );
    assert_eq!(extension("image/jpeg"), "jpg");
    assert_eq!(extension("text/plain"), "txt");
    assert_eq!(extension("application/x-unknown"), "bin");
    assert!(is_sha256_hex(&sha));
    assert!(!is_sha256_hex(&sha.to_uppercase()));
    assert!(!is_sha256_hex(&sha[1..]));
}

#[test]
fn resolve_refuses_paths_outside_the_attachment_directory() {
    let vault = TestVault::new();
    for path in [
        "assets/chat/browser-artifacts/a.png",
        "assets/chat/attachments/../profile/a.png",
        "/assets/chat/attachments/a.png",
        "assets/chat/attachments",
    ] {
        assert!(resolve(&vault.0, path).is_err(), "{path} should be refused");
    }
    assert!(resolve(&vault.0, "assets/chat/attachments/a.png").is_ok());
}

#[test]
fn store_writes_one_digest_named_file_and_keeps_a_matching_one() {
    let vault = TestVault::new();
    let sha = digest(PNG);
    let first = store(&vault.0, &sha, "image/png", PNG).unwrap();
    assert_eq!(first, relative_path(&sha, "image/png"));
    let second = store(&vault.0, &sha, "image/png", PNG).unwrap();
    assert_eq!(second, first);
    assert_eq!(vault.files(), vec![format!("{sha}.png")]);
    assert_eq!(fs::read(vault.0.join(&first)).unwrap(), PNG);
}

#[test]
fn store_replaces_a_damaged_file_and_refuses_a_wrong_digest() {
    let vault = TestVault::new();
    let sha = digest(PNG);
    let relative = relative_path(&sha, "image/png");
    vault.write(&relative, b"damaged");
    store(&vault.0, &sha, "image/png", PNG).unwrap();
    assert_eq!(fs::read(vault.0.join(&relative)).unwrap(), PNG);

    let error = store(&vault.0, &digest(b"other"), "image/png", PNG).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn a_rename_that_lost_a_race_succeeds_only_when_the_target_holds_the_digest() {
    let vault = TestVault::new();
    let sha = digest(PNG);
    let attachment = Unconverted {
        id: "attachment-raced".into(),
        sha256: sha.clone(),
        mime_type: "image/png".into(),
        byte_size: PNG.len() as u64,
        managed_relative_path: "assets/chat/attachments/old/attachment-raced.png".into(),
    };
    let old = vault.0.join(&attachment.managed_relative_path);
    let target = vault.0.join(relative_path(&sha, "image/png"));

    let error = rename_to_digest(&old, &target, &attachment).unwrap_err();
    assert_eq!(
        error.kind(),
        io::ErrorKind::NotFound,
        "nothing moved the file"
    );

    store(&vault.0, &sha, "image/png", PNG).unwrap();
    rename_to_digest(&old, &target, &attachment).unwrap();
    assert_eq!(fs::read(&target).unwrap(), PNG);
}

#[test]
fn store_writes_nothing_for_empty_bytes() {
    let vault = TestVault::new();
    let sha = digest(b"");
    let relative = store(&vault.0, &sha, "text/plain", b"").unwrap();
    assert_eq!(relative, relative_path(&sha, "text/plain"));
    assert!(!vault.exists(&relative));
}

#[tokio::test]
async fn convert_collapses_equal_files_and_rewrites_every_path() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let sha = digest(PNG);
    let text = b"notes";
    let text_sha = digest(text);
    vault.write("assets/chat/attachments/one.png", PNG);
    vault.write("assets/chat/attachments/two.png", PNG);
    vault.write("assets/chat/attachments/three.txt", text);
    insert_attachment(
        &pool,
        "a1",
        PNG,
        "image/png",
        "assets/chat/attachments/one.png",
    )
    .await;
    insert_attachment(
        &pool,
        "a2",
        PNG,
        "image/png",
        "assets/chat/attachments/two.png",
    )
    .await;
    insert_attachment(
        &pool,
        "a3",
        text,
        "text/plain",
        "assets/chat/attachments/three.txt",
    )
    .await;
    assert!(assert_converted(&pool).await.is_err());

    assert_eq!(convert(&pool, &vault.0).await.unwrap(), 3);

    let image = relative_path(&sha, "image/png");
    let snippet = relative_path(&text_sha, "text/plain");
    assert_eq!(
        stored_paths(&pool).await,
        vec![
            ("attachment".into(), "a1".into(), image.clone()),
            ("attachment".into(), "a2".into(), image.clone()),
            ("attachment".into(), "a3".into(), snippet.clone()),
            ("resource".into(), "a1".into(), image.clone()),
            ("resource".into(), "a2".into(), image.clone()),
            ("resource".into(), "a3".into(), snippet.clone()),
        ]
    );
    let mut expected = vec![format!("{sha}.png"), format!("{text_sha}.txt")];
    expected.sort();
    assert_eq!(vault.files(), expected);
    assert_eq!(fs::read(vault.0.join(&image)).unwrap(), PNG);
    assert_converted(&pool).await.unwrap();

    assert_eq!(convert(&pool, &vault.0).await.unwrap(), 0);
    let mut expected = vec![format!("{sha}.png"), format!("{text_sha}.txt")];
    expected.sort();
    assert_eq!(vault.files(), expected);
}

#[tokio::test]
async fn convert_finishes_after_a_crash_between_the_file_move_and_the_row_update() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let sha = digest(PNG);
    let image = relative_path(&sha, "image/png");
    insert_attachment(
        &pool,
        "a1",
        PNG,
        "image/png",
        "assets/chat/attachments/one.png",
    )
    .await;
    insert_attachment(
        &pool,
        "a2",
        PNG,
        "image/png",
        "assets/chat/attachments/two.png",
    )
    .await;
    // The first run moved one.png to the digest name and crashed before any row changed;
    // two.png still holds its copy.
    vault.write(&image, PNG);
    vault.write("assets/chat/attachments/two.png", PNG);

    assert_eq!(convert(&pool, &vault.0).await.unwrap(), 2);

    assert_eq!(vault.files(), vec![format!("{sha}.png")]);
    assert!(
        stored_paths(&pool)
            .await
            .iter()
            .all(|(_, _, path)| *path == image)
    );
}

#[tokio::test]
async fn convert_moves_rows_whose_file_is_missing_or_damaged_and_keeps_damaged_files() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let empty = b"";
    insert_attachment(
        &pool,
        "missing",
        PNG,
        "image/png",
        "assets/chat/attachments/gone.png",
    )
    .await;
    insert_attachment(
        &pool,
        "damaged",
        b"\x89PNG\r\n\x1a\nother",
        "image/png",
        "assets/chat/attachments/damaged.png",
    )
    .await;
    insert_attachment(
        &pool,
        "empty",
        empty,
        "text/plain",
        "assets/chat/attachments/empty.txt",
    )
    .await;
    vault.write("assets/chat/attachments/damaged.png", b"changed on disk");

    assert_eq!(convert(&pool, &vault.0).await.unwrap(), 3);

    assert_converted(&pool).await.unwrap();
    assert_eq!(vault.files(), vec!["damaged.png".to_string()]);
}

#[tokio::test]
async fn convert_leaves_a_converted_vault_unchanged() {
    let vault = TestVault::new();
    let pool = migrated_pool().await;
    let sha = digest(PNG);
    let image = store(&vault.0, &sha, "image/png", PNG).unwrap();
    insert_attachment(&pool, "a1", PNG, "image/png", &image).await;

    assert_eq!(convert(&pool, &vault.0).await.unwrap(), 0);
    assert_converted(&pool).await.unwrap();
    assert_eq!(vault.files(), vec![format!("{sha}.png")]);
}

#[test]
fn received_files_must_carry_the_signature_of_their_content_type() {
    let vault = TestVault::new();
    vault.write("staged/image", PNG);
    vault.write("staged/text", b"note");
    vault.write("staged/binary", b"no\0text");
    let staged = |name: &str| vault.0.join("staged").join(name);

    assert_eq!(check_received_file("image/png", &staged("image")), Ok(()));
    assert!(check_received_file("image/gif", &staged("image")).is_err());
    assert_eq!(check_received_file("text/plain", &staged("text")), Ok(()));
    assert!(check_received_file("text/plain", &staged("binary")).is_err());
    assert!(check_received_file("application/pdf", &staged("text")).is_err());
    assert!(check_received_file("image/png", &staged("missing")).is_err());
}

#[test]
fn every_attachment_type_has_an_extension_and_a_signature_kind() {
    for content_type in IMAGE_CONTENT_TYPES.iter().chain([&TEXT_CONTENT_TYPE]) {
        assert_ne!(extension(content_type), UNKNOWN_EXTENSION);
        assert_ne!(signature_kind(content_type), "unknown");
    }
    assert_eq!(ATTACHMENT_EXTENSIONS.len(), IMAGE_CONTENT_TYPES.len() + 1);
    assert!(content_type_matches(
        "image/jpeg",
        &[0xff, 0xd8, 0xff, 0xe0]
    ));
    assert!(content_type_matches("image/webp", b"RIFF\0\0\0\0WEBPVP8 "));
    assert!(!content_type_matches("image/webp", b"RIFF\0\0\0\0WAVE"));
}
