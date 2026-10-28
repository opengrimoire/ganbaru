//! Replacement of a vault by a newer snapshot of itself: carried operations reach the staged
//! database, device state follows them, and a failure at any step leaves the vault whole.

use super::*;
use ganbaru_contacts::PersonKeyPair;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::local::{self, LocalWriterHead};
use ganbaru_sync::{Engine, SpaceContext, WriterState};
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKeyPair, DeviceKind, RecoverySeed, WriterPublicKey,
};
use ganbaru_sync_replica::authority::{self, AnchorKeys, Enrollment, Founding, ensure_charter};
use ganbaru_sync_replica::writer::{
    ActiveWriter, SuccessorPlan, SyncFiles, SyncKey, SyncKeyStore, create_writer,
};
use sqlx::SqliteConnection;
use std::collections::HashMap;
use std::sync::Mutex;

const OWNER_DEVICE: &str = "owner";
const RECEIVING_DEVICE: &str = "receiver";

/// Sync secrets kept in memory for the length of a test.
#[derive(Default)]
struct MemoryKeys(Mutex<HashMap<String, Vec<u8>>>);

impl MemoryKeys {
    fn reference(vault_id: &str, key: SyncKey<'_>) -> String {
        format!("{vault_id}:{key:?}")
    }
}

impl SyncKeyStore for MemoryKeys {
    fn read(&self, vault_id: &str, key: SyncKey<'_>) -> Result<Option<Vec<u8>>, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&Self::reference(vault_id, key))
            .cloned())
    }

    fn write(&self, vault_id: &str, key: SyncKey<'_>, secret: &[u8]) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .insert(Self::reference(vault_id, key), secret.to_vec());
        Ok(())
    }

    fn remove(&self, vault_id: &str, key: SyncKey<'_>) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .remove(&Self::reference(vault_id, key));
        Ok(())
    }
}

fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&[7; 32]).unwrap()
}

/// The anchor keys of [`OWNER_DEVICE`].
fn anchor_keys() -> AnchorKeys {
    AnchorKeys {
        device: DeviceKeyPair::from_seed(&[8; 32]).unwrap(),
        admin: AdminKeyPair::from_seed(&[9; 32]).unwrap(),
        recovery: RecoverySeed::from_bytes([10; 32]),
    }
}

/// The device key of [`RECEIVING_DEVICE`].
fn receiver_device() -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&[11; 32]).unwrap()
}

/// Founds the space of the vault on `conn` with [`OWNER_DEVICE`] as its anchor and enrolls
/// [`RECEIVING_DEVICE`] through the owner's writer.
async fn found_and_enroll(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    owner: &mut ActiveWriter,
    now_ms: u64,
) {
    let engine = Engine::vault();
    let anchor = anchor_keys();
    let founding = Founding {
        device_id: OWNER_DEVICE,
        tls_fingerprint: [12; 32],
        discovery_key: [13; 32],
        person: &person(),
        now_ms,
    };
    ensure_charter(&engine, conn, ctx, &anchor, &founding)
        .await
        .unwrap();
    let enrollment = Enrollment {
        device_id: RECEIVING_DEVICE.to_owned(),
        device_key: receiver_device().public_key(),
        tls_fingerprint: [14; 32],
        label: "Receiver".to_owned(),
        device_kind: DeviceKind::Desktop,
    };
    authority::enroll(
        &engine,
        conn,
        ctx,
        &anchor.admin,
        &mut owner.local(),
        enrollment,
        now_ms,
    )
    .await
    .unwrap();
}

/// The sync space of the vault on `conn`.
async fn sync_context(conn: &mut SqliteConnection, vault_id: &str, now_ms: u64) -> SpaceContext {
    sqlx::query(
        "INSERT OR IGNORE INTO contacts_local_identity
            (singleton, public_key, created_at)
         VALUES (1, ?, '2026-01-01T00:00:00Z')",
    )
    .bind(person().public_key().to_text())
    .execute(&mut *conn)
    .await
    .unwrap();
    Engine::vault()
        .init_space(conn, vault_id, now_ms)
        .await
        .unwrap();
    local::space_context(conn, vault_id).await.unwrap().unwrap()
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
    ganbaru_db::test_support::write_migrated_database(database_path(&source))
        .await
        .unwrap();
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
        &anchor_keys().device,
        OWNER_DEVICE,
        now,
    )
    .unwrap();
    found_and_enroll(&mut conn, &ctx, &mut owner, now).await;
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
        &receiver_device(),
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
    let error = carry_into_replacement(&engine, &staging, &target, &files, now)
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
    let error = carry_into_replacement(&engine, &staging, &target, &files, now)
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
    carry_into_replacement(&engine, &staging, &target, &files, now)
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

/// Authorization entries the anchor sequenced through its own writer and never sent travel
/// with its other operations, so a replacement that predates an enrollment keeps it.
#[tokio::test]
async fn replacement_carries_the_anchors_unsent_authorization_entries() {
    let parent = crate::vault::tests::unique_path("carry-authorization");
    let source = parent.join("owner");
    let staging = parent.join("staging");
    fs::create_dir_all(&source).unwrap();
    let vault_id = crate::vault::initialize_vault(&source).unwrap().vault_id;
    ganbaru_db::test_support::write_migrated_database(database_path(&source))
        .await
        .unwrap();
    let engine = Engine::vault();
    let keys = MemoryKeys::default();
    let owner_files = SyncFiles::new(parent.join("owner-sync"), &vault_id).unwrap();
    let now = crate::sync::now_ms().unwrap();
    let registry = DatabasePoolRegistry::default();

    let pool = registry.connect_path(database_path(&source)).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let ctx = sync_context(&mut conn, &vault_id, now).await;
    let anchor = anchor_keys();
    let founding = Founding {
        device_id: OWNER_DEVICE,
        tls_fingerprint: [12; 32],
        discovery_key: [13; 32],
        person: &person(),
        now_ms: now,
    };
    ensure_charter(&engine, &mut conn, &ctx, &anchor, &founding)
        .await
        .unwrap();
    let mut owner = create_writer(
        &owner_files,
        &keys,
        SuccessorPlan::default(),
        &anchor.device,
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
    hand_off(&source, &staging, &parent, "older", &vault_id).await;

    // The anchor enrolls a device after the snapshot and sends the entry nowhere.
    let pool = registry.connect_path(database_path(&source)).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let enrollment = Enrollment {
        device_id: RECEIVING_DEVICE.to_owned(),
        device_key: receiver_device().public_key(),
        tls_fingerprint: [14; 32],
        label: "Receiver".to_owned(),
        device_kind: DeviceKind::Desktop,
    };
    authority::enroll(
        &engine,
        &mut conn,
        &ctx,
        &anchor.admin,
        &mut owner.local(),
        enrollment,
        now,
    )
    .await
    .unwrap();
    drop(conn);
    registry.close_all().await.unwrap();

    carry_into_replacement(&engine, &staging, &source, &owner_files, now)
        .await
        .unwrap();
    replace_vault(&staging, &source, &parent.join("older-rollback")).unwrap();

    let pool = registry.connect_path(database_path(&source)).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let view = engine
        .authorization(&mut conn, &ctx)
        .await
        .unwrap()
        .expect("the replacement keeps the charter");
    assert_eq!(view.revision, 1);
    assert!(
        view.devices
            .iter()
            .any(|device| device.device_id == RECEIVING_DEVICE),
        "the unsent enrollment survives the replacement"
    );
    drop(conn);
    registry.close_all().await.unwrap();
    assert!(!owner_files.carry_path().exists());
    fs::remove_dir_all(parent).unwrap();
}
