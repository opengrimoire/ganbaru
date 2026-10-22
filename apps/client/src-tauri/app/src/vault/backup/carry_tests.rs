//! Replacement of a vault by a newer snapshot of itself: carried operations reach the staged
//! database, device state follows them, and a failure at any step leaves the vault whole.

use super::*;
use ganbaru_contacts::PersonKeyPair;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::local::{self, LocalWriterHead};
use ganbaru_sync::{Engine, SpaceContext, WriterState};
use ganbaru_sync_contracts::WriterPublicKey;
use ganbaru_sync_replica::writer::{SuccessorPlan, SyncFiles, WriterKeyStore, create_writer};
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::sync::Mutex;

const OWNER_DEVICE: &str = "owner";
const RECEIVING_DEVICE: &str = "receiver";

/// Writer keys kept in memory for the length of a test.
#[derive(Default)]
struct MemoryKeys(Mutex<HashMap<String, Vec<u8>>>);

impl WriterKeyStore for MemoryKeys {
    fn read(&self, vault_id: &str, writer_id: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&format!("{vault_id}:{writer_id}"))
            .cloned())
    }

    fn write(&self, vault_id: &str, writer_id: &str, pkcs8: &[u8]) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(format!("{vault_id}:{writer_id}"), pkcs8.to_vec());
        Ok(())
    }

    fn remove(&self, vault_id: &str, writer_id: &str) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .remove(&format!("{vault_id}:{writer_id}"));
        Ok(())
    }
}

fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&[7; 32]).unwrap()
}

/// The sync space of the vault on `conn`, created with the test person as its anchor.
async fn sync_context(conn: &mut SqliteConnection, vault_id: &str, now_ms: u64) -> SpaceContext {
    sqlx::query(
        "INSERT OR IGNORE INTO contacts_local_identity
            (singleton, public_key, card_nonce, created_at, updated_at)
         VALUES (1, ?, 'AAAAAAAAAAAAAAAAAAAAAA', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(person().public_key().to_text())
    .execute(&mut *conn)
    .await
    .unwrap();
    Engine::vault()
        .init_space(conn, vault_id, now_ms)
        .await
        .unwrap();
    local::space_context(conn, vault_id, OWNER_DEVICE)
        .await
        .unwrap()
        .unwrap()
}

async fn execute(conn: &mut SqliteConnection, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut *conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

/// Snapshots `source` into a handoff archive and stages it at `staging`.
async fn hand_off(source: &Path, staging: &Path, parent: &Path, label: &str, vault_id: &str) {
    let snapshot = parent.join(format!("{label}.sqlite"));
    let archive = parent.join(format!("{label}.zip"));
    create_handoff_archive(source, &snapshot, &archive)
        .await
        .unwrap();
    stage_handoff_archive(&archive, staging, vault_id)
        .await
        .unwrap();
}

/// Quick notes of the vault at `root` as id, title, and revision.
async fn notes(root: &Path) -> Vec<(String, String, i64)> {
    let registry = DatabasePoolRegistry::default();
    let pool = registry
        .connect_path_read_only(database_path(root))
        .await
        .unwrap();
    let notes = sqlx::query_as("SELECT id, title, revision FROM quick_notes ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    registry.close_all().await.unwrap();
    notes
}

/// The stored head of the writer with `key` in the vault at `root`.
async fn writer_head(root: &Path, ctx: &SpaceContext, key: &WriterPublicKey) -> LocalWriterHead {
    let registry = DatabasePoolRegistry::default();
    let pool = registry
        .connect_path_read_only(database_path(root))
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let head = Engine::vault()
        .local_writer_head(&mut conn, ctx.space, key)
        .await
        .unwrap();
    drop(conn);
    registry.close_all().await.unwrap();
    head
}

fn note(id: &str, title: &str, revision: i64) -> (String, String, i64) {
    (id.to_owned(), title.to_owned(), revision)
}

#[tokio::test]
async fn replacement_carries_operations_and_device_state_through_failed_attempts() {
    let parent = crate::vault::tests::unique_path("carry-replacement");
    let source = parent.join("owner");
    let target = parent.join("receiver");
    let staging = parent.join("staging");
    fs::create_dir_all(&source).unwrap();
    let vault_id = crate::vault::initialize_vault(&source).unwrap().vault_id;
    let engine = Engine::vault();
    let keys = MemoryKeys::default();
    let owner_files = SyncFiles::new(parent.join("owner-sync"), &vault_id).unwrap();
    let files = SyncFiles::new(parent.join("receiver-sync"), &vault_id).unwrap();
    let now = crate::sync::now_ms().unwrap();
    let registry = DatabasePoolRegistry::default();

    let pool = registry.connect_path(database_path(&source)).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let ctx = sync_context(&mut conn, &vault_id, now).await;
    let mut owner = create_writer(
        &owner_files,
        &keys,
        SuccessorPlan::default(),
        &person(),
        OWNER_DEVICE,
        now,
    )
    .unwrap();
    execute(
        &mut conn,
        "INSERT INTO quick_notes (id, title, order_key) VALUES ('note-owner', 'Owner', 'a0')",
    )
    .await;
    engine
        .seal(&mut conn, &ctx, &mut owner.local(), now)
        .await
        .unwrap();
    drop(conn);
    registry.close_all().await.unwrap();

    let initial = parent.join("initial-staging");
    hand_off(&source, &initial, &parent, "initial", &vault_id).await;
    replace_vault(&initial, &target, &parent.join("initial-rollback")).unwrap();

    // The receiver seals a note the owner never sees and raises both revisions.
    let pool = registry.connect_path(database_path(&target)).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    sync_context(&mut conn, &vault_id, now).await;
    let mut receiver = create_writer(
        &files,
        &keys,
        SuccessorPlan::default(),
        &person(),
        RECEIVING_DEVICE,
        now,
    )
    .unwrap();
    let receiver_key = receiver.local().key.public_key();
    execute(
        &mut conn,
        "INSERT INTO quick_notes (id, title, order_key) VALUES ('note-carried', 'Carried', 'a1')",
    )
    .await;
    let last = engine
        .seal(&mut conn, &ctx, &mut receiver.local(), now)
        .await
        .unwrap()
        .last_seq
        .unwrap();
    execute(
        &mut conn,
        "UPDATE quick_notes SET revision = 4 WHERE id = 'note-carried';
         UPDATE quick_notes SET revision = 6 WHERE id = 'note-owner';",
    )
    .await;
    assert!(!local::has_pending_captures(&mut conn).await.unwrap());
    drop(conn);
    registry.close_all().await.unwrap();

    // The owner edits its note and hands off a newer snapshot that lacks the receiver's note.
    let pool = registry.connect_path(database_path(&source)).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    execute(
        &mut conn,
        "UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-owner'",
    )
    .await;
    engine
        .seal(&mut conn, &ctx, &mut owner.local(), now)
        .await
        .unwrap();
    drop(conn);
    registry.close_all().await.unwrap();
    hand_off(&source, &staging, &parent, "newer", &vault_id).await;
    let live = vec![
        note("note-carried", "Carried", 4),
        note("note-owner", "Owner", 6),
    ];

    // The staged apply ran, the bundle is gone, and nothing is restored yet.
    failpoints::arm("replace.before_restore");
    let error = carry_into_replacement(&engine, &staging, &target, &files, OWNER_DEVICE, now)
        .await
        .unwrap_err();
    assert_eq!(error, "failpoint replace.before_restore");
    assert!(!files.carry_path().exists());
    assert_eq!(
        notes(&staging).await,
        [
            note("note-carried", "Carried", 1),
            note("note-owner", "Edited", 1)
        ]
    );
    assert_eq!(
        writer_head(&staging, &ctx, &receiver_key).await,
        LocalWriterHead::Genuine {
            stored: last,
            state: WriterState::Active
        }
    );
    assert_eq!(notes(&target).await, live);

    // A retry finds the operations stored and restores device state.
    failpoints::arm("replace.before_rename");
    let error = carry_into_replacement(&engine, &staging, &target, &files, OWNER_DEVICE, now)
        .await
        .unwrap_err();
    assert_eq!(error, "failpoint replace.before_rename");
    assert_eq!(
        notes(&staging).await,
        [
            note("note-carried", "Carried", 5),
            note("note-owner", "Edited", 7)
        ]
    );
    assert_eq!(notes(&target).await, live);

    failpoints::clear();
    carry_into_replacement(&engine, &staging, &target, &files, OWNER_DEVICE, now)
        .await
        .unwrap();
    replace_vault(&staging, &target, &parent.join("newer-rollback")).unwrap();
    assert_eq!(
        notes(&target).await,
        [
            note("note-carried", "Carried", 6),
            note("note-owner", "Edited", 8)
        ]
    );
    assert_eq!(
        writer_head(&target, &ctx, &receiver_key).await,
        LocalWriterHead::Genuine {
            stored: last,
            state: WriterState::Active
        }
    );
    assert!(!files.carry_path().exists());
    fs::remove_dir_all(parent).unwrap();
}
