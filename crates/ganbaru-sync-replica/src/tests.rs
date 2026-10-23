use super::access::vault_connection_hook;
use super::carry_forward;
use super::writer::{
    FileKeyStore, RESERVATION_BLOCK, SuccessorPlan, SyncFiles, WriterCheck, WriterKeyStore,
    WriterRecord, check_writer, create_writer, read_record, read_stored_writer, record_matches,
};
use ganbaru_contacts::PersonKeyPair;
use ganbaru_db::{DatabaseAccessMode, DatabasePoolRegistry};
use ganbaru_sync::local::{self, LocalWriterHead};
use ganbaru_sync::{Engine, SpaceContext, WriterState};
use sqlx::SqliteConnection;
use sqlx::pool::PoolConnection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Notify;

mod assets;
mod backup;
mod exchange;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("sync replica test runtime should initialize")
        .block_on(future)
}

const VAULT_ID: &str = "vault-sync-test";
const DEVICE_ID: &str = "device-test";
const NOW_MS: u64 = 1_800_000_000_000;

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ganbaru-sync-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn files(&self) -> SyncFiles {
        SyncFiles::new(self.0.join("sync"), VAULT_ID).expect("valid vault id")
    }

    fn keys(&self) -> FileKeyStore {
        FileKeyStore {
            directory: self.0.join("keys"),
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&[7; 32]).expect("test person seed")
}

/// Writes a copy of the process-wide migrated template database to `path`, so connecting to it
/// only validates the applied migrations instead of running the whole chain.
///
/// The image is serialized and written directly, since an in-memory connection opens every
/// `VACUUM INTO` target in memory too.
async fn write_migrated_database(path: &Path) {
    let template = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open template copy");
    ganbaru_db::test_support::apply_migrated_template(&template)
        .await
        .expect("load migrated template");
    let image = acquire(&template)
        .await
        .serialize(None)
        .await
        .expect("serialize migrated template")
        .to_vec();
    template.close().await;
    std::fs::write(path, image).expect("write migrated database");
}

/// Creates a migrated vault database at `path` holding the test person identity.
async fn create_vault(registry: &DatabasePoolRegistry, path: &Path) -> sqlx::SqlitePool {
    write_migrated_database(path).await;
    let pool = registry
        .connect_path(path)
        .await
        .expect("open migrated vault");
    sqlx::query(
        "INSERT INTO contacts_local_identity
            (singleton, public_key, created_at)
         VALUES (1, ?, '2026-01-01T00:00:00Z')",
    )
    .bind(person().public_key().to_text())
    .execute(&pool)
    .await
    .expect("insert person identity");
    pool
}

/// Initializes the test space, coordinated by the device `coordinator`.
async fn context(conn: &mut SqliteConnection, coordinator: &str) -> SpaceContext {
    Engine::vault()
        .init_space(conn, VAULT_ID, NOW_MS)
        .await
        .expect("initialize space");
    local::space_context(conn, VAULT_ID, coordinator)
        .await
        .expect("read space")
        .expect("space context")
}

async fn add_note(conn: &mut SqliteConnection, id: &str) {
    sqlx::query("INSERT INTO quick_notes (id, title, order_key) VALUES (?, 'Note', 'a0')")
        .bind(id)
        .execute(&mut *conn)
        .await
        .expect("insert quick note");
}

async fn acquire(pool: &sqlx::SqlitePool) -> PoolConnection<sqlx::Sqlite> {
    pool.acquire().await.expect("acquire connection")
}

async fn check(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    directory: &TestDirectory,
) -> WriterCheck {
    let files = directory.files();
    let stored = read_stored_writer(&files, &directory.keys()).expect("read stored writer");
    check_writer(engine, conn, ctx, &files, stored)
        .await
        .expect("check writer")
}

fn record(last_committed_seq: u64, reserved_through: u64) -> WriterRecord {
    WriterRecord {
        schema_version: 1,
        writer_id: "00".repeat(16),
        certificate: String::new(),
        reserved_through,
        last_committed_seq,
        paused: false,
        retired_keys: Vec::new(),
    }
}

#[test]
fn record_matches_only_the_recorded_chain() {
    let active = |stored| LocalWriterHead::Genuine {
        stored,
        state: WriterState::Active,
    };
    let cases = [
        (record(0, 0), LocalWriterHead::Absent, true),
        (record(3, 256), LocalWriterHead::Absent, false),
        (record(3, 256), active(3), true),
        (record(3, 256), active(200), true),
        (record(3, 256), active(256), true),
        (record(3, 256), active(2), false),
        (record(3, 256), active(257), false),
        (record(3, 256), LocalWriterHead::Mismatch, false),
        (
            record(3, 256),
            LocalWriterHead::Genuine {
                stored: 3,
                state: WriterState::Retired,
            },
            false,
        ),
    ];
    for (record, head, expected) in cases {
        assert_eq!(
            record_matches(&record, head),
            expected,
            "{head:?} with committed {} and reserved {}",
            record.last_committed_seq,
            record.reserved_through
        );
    }
}

#[test]
fn reservations_grow_in_blocks() {
    let directory = TestDirectory::new("reservation");
    let files = directory.files();
    let keys = directory.keys();
    let mut writer = create_writer(
        &files,
        &keys,
        SuccessorPlan::default(),
        &person(),
        DEVICE_ID,
        NOW_MS,
    )
    .expect("create writer");
    let mut reserve = |seq| {
        let local = writer.local();
        local.reservation.ensure_reserved(seq).expect("reserve");
        read_record(&files.record_path())
            .expect("read record")
            .expect("record")
            .reserved_through
    };
    assert_eq!(reserve(1), RESERVATION_BLOCK);
    assert_eq!(reserve(RESERVATION_BLOCK), RESERVATION_BLOCK);
    assert_eq!(reserve(RESERVATION_BLOCK + 1), 2 * RESERVATION_BLOCK);
}

#[test]
fn records_naming_a_non_canonical_writer_id_are_refused() {
    let directory = TestDirectory::new("record-ids");
    let files = directory.files();
    let keys = directory.keys();
    std::fs::create_dir_all(&files.directory).expect("create sync directory");
    let outside = directory.path().join("outside.key");
    std::fs::write(&outside, b"not a writer key").expect("write outside file");
    let canonical = "ab".repeat(16);
    let cases = [
        ("../outside".to_string(), Vec::new()),
        ("AB".repeat(16), Vec::new()),
        (canonical.clone(), vec!["../../outside".to_string()]),
        (canonical.clone(), vec!["ab".repeat(15)]),
    ];
    for (writer_id, retired_keys) in cases {
        let mut stored = record(0, 0);
        stored.writer_id = writer_id.clone();
        stored.retired_keys = retired_keys.clone();
        std::fs::write(
            files.record_path(),
            serde_json::to_vec(&stored).expect("encode record"),
        )
        .expect("write record");
        let error = read_stored_writer(&files, &keys)
            .err()
            .unwrap_or_else(|| panic!("{writer_id} with {retired_keys:?} was accepted"));
        assert_eq!(error, "sync writer record names an invalid writer id");
    }
    let mut stored = record(0, 0);
    stored.writer_id = canonical;
    std::fs::write(
        files.record_path(),
        serde_json::to_vec(&stored).expect("encode record"),
    )
    .expect("write record");
    assert!(read_record(&files.record_path()).expect("read").is_some());
    assert!(outside.exists());
}

#[test]
fn writer_survives_restarts_and_rotates_after_a_restore() {
    block_on(async {
        let directory = TestDirectory::new("writer");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let keys = directory.keys();

        let first = create_vault(&registry, &directory.path().join("first.sqlite")).await;
        let mut conn = acquire(&first).await;
        let ctx = context(&mut conn, DEVICE_ID).await;
        let WriterCheck::Successor(plan) = check(&engine, &mut conn, &ctx, &directory).await else {
            panic!("a vault without a record needs a writer");
        };
        assert_eq!(plan, SuccessorPlan::default());
        let mut writer =
            create_writer(&files, &keys, plan, &person(), DEVICE_ID, NOW_MS).expect("create");
        let original = writer.id();

        add_note(&mut conn, "note-1").await;
        let report = engine
            .seal(&mut conn, &ctx, &mut writer.local(), NOW_MS)
            .await
            .expect("seal");
        let last = report.last_seq.expect("sealed operations");
        writer.committed(last).expect("record commit");
        drop(writer);

        let WriterCheck::Ready(writer) = check(&engine, &mut conn, &ctx, &directory).await else {
            panic!("the recorded writer still matches the database");
        };
        assert_eq!(writer.id(), original);
        assert_eq!(writer.record().last_committed_seq, last);
        drop(writer);
        drop(conn);

        // A restored database lacks the committed chain of the recorded writer.
        let restored = create_vault(&registry, &directory.path().join("restored.sqlite")).await;
        let mut conn = acquire(&restored).await;
        context(&mut conn, DEVICE_ID).await;
        let WriterCheck::Successor(plan) = check(&engine, &mut conn, &ctx, &directory).await else {
            panic!("a restored database needs a successor");
        };
        let successor =
            create_writer(&files, &keys, plan, &person(), DEVICE_ID, NOW_MS).expect("successor");
        assert_ne!(successor.id(), original);
        assert!(successor.record().retired_keys.is_empty());
        let stored = read_record(&files.record_path())
            .expect("read record")
            .expect("record");
        assert_eq!(stored.writer_id, successor.id().to_hex());
        assert!(
            keys.read(VAULT_ID, &original.to_hex())
                .expect("read retired key")
                .is_none()
        );
        assert!(
            keys.read(VAULT_ID, &successor.id().to_hex())
                .expect("read successor key")
                .is_some()
        );
        drop(conn);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn carried_operations_reach_the_replacement_once() {
    block_on(async {
        let directory = TestDirectory::new("carry");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let keys = directory.keys();
        let current_path = directory.path().join("current.sqlite");
        let replacement_path = directory.path().join("replacement.sqlite");

        let current = create_vault(&registry, &current_path).await;
        create_vault(&registry, &replacement_path).await;
        let mut conn = acquire(&current).await;
        let ctx = context(&mut conn, DEVICE_ID).await;
        let mut writer = create_writer(
            &files,
            &keys,
            SuccessorPlan::default(),
            &person(),
            DEVICE_ID,
            NOW_MS,
        )
        .expect("create writer");

        add_note(&mut conn, "note-unsealed").await;
        drop(conn);
        registry.close_all().await.expect("close pools");
        let refused =
            carry_forward::export(&engine, &current_path, &replacement_path, &files, DEVICE_ID)
                .await
                .expect_err("pending captures refuse the export");
        assert!(refused.contains("not sealed"));

        let current = registry.connect_path(&current_path).await.expect("reopen");
        let mut conn = acquire(&current).await;
        let last = engine
            .seal(&mut conn, &ctx, &mut writer.local(), NOW_MS)
            .await
            .expect("seal")
            .last_seq
            .expect("sealed operations");
        drop(conn);
        registry.close_all().await.expect("close pools");

        let exported =
            carry_forward::export(&engine, &current_path, &replacement_path, &files, DEVICE_ID)
                .await
                .expect("export");
        assert_eq!(u64::try_from(exported).expect("count"), last);
        assert!(files.carry_path().exists());

        let replacement = registry
            .connect_path(&replacement_path)
            .await
            .expect("open replacement");
        let mut conn = acquire(&replacement).await;
        let report = carry_forward::import_pending(&engine, &mut conn, &ctx, &files, NOW_MS)
            .await
            .expect("import")
            .expect("a pending bundle");
        assert_eq!(report.stored, exported);
        assert_eq!(report.refused, 0);
        assert!(!files.carry_path().exists());
        let title: String = sqlx::query_scalar("SELECT title FROM quick_notes WHERE id = ?")
            .bind("note-unsealed")
            .fetch_one(&mut *conn)
            .await
            .expect("carried note");
        assert_eq!(title, "Note");
        let head = engine
            .local_writer_head(&mut conn, ctx.space, &writer.local().key.public_key())
            .await
            .expect("writer head");
        assert_eq!(
            head,
            LocalWriterHead::Genuine {
                stored: last,
                state: WriterState::Active
            }
        );
        assert!(
            carry_forward::import_pending(&engine, &mut conn, &ctx, &files, NOW_MS)
                .await
                .expect("second import")
                .is_none()
        );
        drop(conn);
        drop(replacement);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn carry_forward_aborts_when_the_replacement_lacks_compacted_operations() {
    block_on(async {
        let directory = TestDirectory::new("carry-compacted");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let current_path = directory.path().join("current.sqlite");
        let behind_path = directory.path().join("behind.sqlite");
        let caught_up_path = directory.path().join("caught-up.sqlite");

        let current = create_vault(&registry, &current_path).await;
        create_vault(&registry, &behind_path).await;
        let mut conn = acquire(&current).await;
        let ctx = context(&mut conn, DEVICE_ID).await;
        let mut writer = create_writer(
            &files,
            &directory.keys(),
            SuccessorPlan::default(),
            &person(),
            DEVICE_ID,
            NOW_MS,
        )
        .expect("create writer");
        for id in ["note-1", "note-2", "note-3"] {
            add_note(&mut conn, id).await;
            engine
                .seal(&mut conn, &ctx, &mut writer.local(), NOW_MS)
                .await
                .expect("seal");
        }
        sqlx::query("VACUUM INTO ?")
            .bind(caught_up_path.to_string_lossy().into_owned())
            .execute(&mut *conn)
            .await
            .expect("copy the caught-up replacement");
        add_note(&mut conn, "note-4").await;
        let last = engine
            .seal(&mut conn, &ctx, &mut writer.local(), NOW_MS)
            .await
            .expect("seal")
            .last_seq
            .expect("sealed operations");
        let stored = engine.stored_vector(&mut conn, &ctx).await.expect("stored");
        let report = engine
            .compact(&mut conn, &ctx, &stored)
            .await
            .expect("compact");
        assert_eq!(report.compacted, usize::try_from(last - 2).expect("count"));
        drop(conn);
        registry.close_all().await.expect("close pools");

        let refused =
            carry_forward::export(&engine, &current_path, &behind_path, &files, DEVICE_ID)
                .await
                .expect_err("compacted operations cannot be carried");
        assert!(refused.contains("compacted"), "{refused}");
        assert!(!files.carry_path().exists());

        let exported =
            carry_forward::export(&engine, &current_path, &caught_up_path, &files, DEVICE_ID)
                .await
                .expect("a replacement holding the compacted operations takes the rest");
        assert_eq!(exported, 1);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn oversized_carry_bundles_are_refused_before_reading() {
    block_on(async {
        let directory = TestDirectory::new("carry-oversized");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
        let mut conn = acquire(&pool).await;
        let ctx = context(&mut conn, DEVICE_ID).await;
        std::fs::create_dir_all(&files.directory).expect("create sync directory");
        let bundle = std::fs::File::create(files.carry_path()).expect("create bundle");
        bundle
            .set_len(ganbaru_sync::local::MAX_BUNDLE_BYTES as u64 + 1)
            .expect("size bundle");
        drop(bundle);

        let error = carry_forward::import_pending(&engine, &mut conn, &ctx, &files, NOW_MS)
            .await
            .expect_err("an oversized bundle is refused");
        assert_eq!(
            error,
            "carry-forward bundle is not a file within the size limit"
        );
        assert!(files.carry_path().exists());
        drop(conn);
        drop(pool);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn guarded_connections_refuse_local_tables_and_signal_commits() {
    block_on(async {
        let directory = TestDirectory::new("guarded");
        let path = directory.path().join("vault.sqlite");
        let setup = DatabasePoolRegistry::default();
        create_vault(&setup, &path).await;
        setup.close_all().await.expect("close setup pools");

        let commits = Arc::new(Notify::new());
        let registry = DatabasePoolRegistry::default();
        registry.set_connection_hook(Some(vault_connection_hook(commits.clone())));
        let (pool, access) = registry
            .connect_path_guarded(&path)
            .await
            .expect("open guarded");
        assert_eq!(access, DatabaseAccessMode::Guarded);

        let refused = sqlx::query(
            "INSERT INTO projects (id, group_id, name, order_key) VALUES ('project-1', 'group-1', 'Project', 'a1')",
        )
        .execute(&pool)
        .await
        .expect_err("guarded replicas refuse local-only tables");
        assert!(
            refused
                .to_string()
                .contains(ganbaru_sync::guards::READ_ONLY_MESSAGE),
            "{refused}"
        );

        let mut conn = acquire(&pool).await;
        add_note(&mut conn, "note-guarded").await;
        drop(conn);
        tokio::time::timeout(Duration::from_secs(5), commits.notified())
            .await
            .expect("a commit signals the sync service");
        drop(pool);
        registry.close_all().await.expect("close pools");
    });
}
