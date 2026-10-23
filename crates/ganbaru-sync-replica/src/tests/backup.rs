use super::{TestDirectory, acquire, add_note, block_on, context, create_vault};
use crate::backup::{
    ArchiveGate, DATABASE_FILE, VAULT_MANIFEST_FILE, create_backup_archive,
    create_snapshot_archive, is_bundle_layout_entry, is_database_sidecar, vacuum_database,
};
use ganbaru_db::DatabasePoolRegistry;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use zip::ZipArchive;

/// The device-local lock file a running vault holds.
const VAULT_LOCK_FILE: &str = "vault.lock";

/// Names of the entries of the archive at `path`.
fn archive_names(path: &Path) -> Vec<String> {
    let archive =
        ZipArchive::new(std::fs::File::open(path).expect("open archive")).expect("read archive");
    let mut names: Vec<String> = archive
        .file_names()
        .map(|name| name.expect("archive entry name").into_owned())
        .collect();
    names.sort();
    names
}

/// Extracts the database of the archive at `path` to `destination`.
fn extract_database(path: &Path, destination: &Path) {
    let mut archive =
        ZipArchive::new(std::fs::File::open(path).expect("open archive")).expect("read archive");
    let mut bytes = Vec::new();
    archive
        .by_name(DATABASE_FILE)
        .expect("archived database")
        .read_to_end(&mut bytes)
        .expect("read archived database");
    std::fs::write(destination, bytes).expect("write extracted database");
}

/// Note ids the database at `path` holds.
async fn note_ids(path: &Path) -> Vec<String> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .expect("open extracted database");
    let ids = sqlx::query_scalar("SELECT id FROM quick_notes ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("read notes");
    pool.close().await;
    ids
}

/// Writes a vault layout around the database at `root`: the marker, a note document, a
/// database sidecar, the vault lock, and a folder outside the bundle layout.
fn write_layout(root: &Path) {
    std::fs::create_dir_all(root.join("notes")).expect("create notes");
    std::fs::create_dir_all(root.join(".git")).expect("create foreign folder");
    std::fs::write(root.join(VAULT_MANIFEST_FILE), b"{}").expect("write marker");
    std::fs::write(root.join("notes/page.md"), b"note").expect("write note");
    std::fs::write(root.join(".git/HEAD"), b"ref").expect("write foreign file");
    std::fs::write(root.join(VAULT_LOCK_FILE), b"").expect("write lock");
}

#[test]
fn backup_layout_recognizes_database_sidecars_and_bundle_entries() {
    for sidecar in ["-wal", "-shm", "-journal"] {
        assert!(is_database_sidecar(Path::new(&format!(
            "{DATABASE_FILE}{sidecar}"
        ))));
    }
    assert!(!is_database_sidecar(Path::new(DATABASE_FILE)));
    assert!(!is_database_sidecar(Path::new("ganbaru.sqlite.bak")));

    for (entry, directory, expected) in [
        (DATABASE_FILE, false, true),
        (VAULT_MANIFEST_FILE, false, true),
        ("assets", true, true),
        ("assets/profile/a.png", false, true),
        ("projects/one", true, true),
        ("projects/stray.txt", false, false),
        (VAULT_LOCK_FILE, false, false),
        ("ganbaru.sqlite-wal", false, false),
        (".git", true, false),
    ] {
        assert_eq!(
            is_bundle_layout_entry(Path::new(entry), directory),
            expected,
            "{entry}"
        );
    }
}

#[test]
fn backup_vacuum_writes_a_readable_consistent_snapshot() {
    block_on(async {
        let directory = TestDirectory::new("backup-vacuum");
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &directory.path().join(DATABASE_FILE)).await;
        add_note(&mut *acquire(&pool).await, "kept").await;
        let snapshot = directory.path().join("snapshot.sqlite");

        vacuum_database(&pool, &snapshot).await.expect("vacuum");
        add_note(&mut *acquire(&pool).await, "later").await;

        assert_eq!(note_ids(&snapshot).await, vec!["kept".to_string()]);
    });
}

#[test]
fn backup_archive_carries_the_snapshot_database_and_the_bundle_layout_only() {
    block_on(async {
        let directory = TestDirectory::new("backup-archive");
        let root = directory.path().join("vault");
        std::fs::create_dir_all(&root).expect("create vault root");
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &root.join(DATABASE_FILE)).await;
        add_note(&mut *acquire(&pool).await, "backed-up").await;
        write_layout(&root);
        let archive = directory.path().join("vault.ganbaru-backup");

        create_backup_archive(
            &ArchiveGate::default(),
            &pool,
            &root,
            &directory.path().join("snapshot.sqlite"),
            &archive,
            &Mutex::new(()),
        )
        .await
        .expect("create backup archive");

        let names = archive_names(&archive);
        for expected in [DATABASE_FILE, VAULT_MANIFEST_FILE, "notes/page.md"] {
            assert!(names.iter().any(|name| name == expected), "{expected}");
        }
        for excluded in [VAULT_LOCK_FILE, ".git/HEAD", "ganbaru.sqlite-wal"] {
            assert!(!names.iter().any(|name| name == excluded), "{excluded}");
        }
        let extracted = directory.path().join("extracted.sqlite");
        extract_database(&archive, &extracted);
        assert_eq!(note_ids(&extracted).await, vec!["backed-up".to_string()]);
    });
}

#[test]
fn backup_snapshot_archive_leaves_installation_sync_state_behind() {
    block_on(async {
        let directory = TestDirectory::new("backup-snapshot");
        let root = directory.path().join("vault");
        std::fs::create_dir_all(&root).expect("create vault root");
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &root.join(DATABASE_FILE)).await;
        context(&mut *acquire(&pool).await, "device-hub").await;
        add_note(&mut *acquire(&pool).await, "handed-off").await;
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_capture")
            .fetch_one(&pool)
            .await
            .expect("count pending captures");
        assert!(pending > 0, "the live vault holds an unsealed capture");
        registry.close_all().await.expect("close vault");
        write_layout(&root);
        let archive = directory.path().join("handoff.ganbaru-backup");

        create_snapshot_archive(
            &ArchiveGate::default(),
            &root,
            &directory.path().join("snapshot.sqlite"),
            &archive,
            &Mutex::new(()),
        )
        .await
        .expect("create snapshot archive");

        let names = archive_names(&archive);
        assert!(names.iter().any(|name| name == DATABASE_FILE));
        assert!(!names.iter().any(|name| name == VAULT_LOCK_FILE));
        let extracted = directory.path().join("extracted.sqlite");
        extract_database(&archive, &extracted);
        assert_eq!(note_ids(&extracted).await, vec!["handed-off".to_string()]);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&extracted))
            .await
            .expect("open extracted database");
        let captured: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_capture")
            .fetch_one(&pool)
            .await
            .expect("count captured changes");
        pool.close().await;
        assert_eq!(captured, 0, "the snapshot holds no local capture queue");
    });
}

#[test]
fn backup_archive_waits_for_a_running_asset_sweep() {
    block_on(async {
        let directory = TestDirectory::new("backup-gate");
        let root = directory.path().join("vault");
        std::fs::create_dir_all(&root).expect("create vault root");
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &root.join(DATABASE_FILE)).await;
        write_layout(&root);
        let archive = directory.path().join("vault.ganbaru-backup");
        let snapshot = directory.path().join("snapshot.sqlite");
        let gate = ArchiveGate::default();
        let config_lock = Mutex::new(());
        let backup =
            || create_backup_archive(&gate, &pool, &root, &snapshot, &archive, &config_lock);

        let sweep = gate.lock().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(200), backup())
                .await
                .is_err(),
            "the archive waits while the sweep holds the gate"
        );
        assert!(!archive.exists());
        drop(sweep);
        backup().await.expect("create backup archive");
        assert!(archive.exists());
    });
}
