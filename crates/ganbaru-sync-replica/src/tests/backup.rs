use super::{TestDirectory, acquire, add_note, block_on, context, create_vault};
use crate::backup::{
    ArchiveGate, BackupFileError, BackupSealKeys, DATABASE_FILE, VAULT_MANIFEST_FILE,
    create_backup_archive, create_encrypted_backup, create_snapshot_archive, decrypt_backup,
    is_bundle_layout_entry, is_database_sidecar, read_backup_header, seal_archive,
    vacuum_database,
};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync_contracts::backup::{BACKUP_CHUNK_BYTES, BackupError};
use ganbaru_sync_contracts::{EncryptionKeyPair, SpaceId, SpaceKey};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zeroize::Zeroizing;
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
        ("config.json", false, false),
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
        context(&mut *acquire(&pool).await).await;
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
fn encrypted_backup_holds_the_asset_sweep_gate_until_it_finishes() {
    block_on(async {
        let directory = TestDirectory::new("backup-gate");
        let root = directory.path().join("vault");
        std::fs::create_dir_all(&root).expect("create vault root");
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &root.join(DATABASE_FILE)).await;
        write_layout(&root);
        registry.close_all().await.expect("close vault");
        drop(pool);
        let backup = directory.path().join("vault.ganbaru-backup");
        let staging = directory.path().join("staging");
        let keys = seal_keys(None);
        let gate = ArchiveGate::default();
        let create = |destination: PathBuf| {
            let (gate, root, staging, keys) = (&gate, &root, &staging, &keys);
            async move { create_encrypted_backup(gate, root, staging, keys, &destination).await }
        };

        let sweep = gate.lock().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(200), create(backup.clone()))
                .await
                .is_err(),
            "the backup waits while the sweep holds the gate"
        );
        assert!(!backup.exists());
        drop(sweep);

        let failed = create(directory.path().join("missing/vault.ganbaru-backup")).await;
        assert!(matches!(failed, Err(BackupFileError::Io(_))), "{failed:?}");
        assert!(
            tokio::time::timeout(Duration::from_millis(200), gate.lock())
                .await
                .is_ok(),
            "a failed backup releases the gate"
        );
        assert_eq!(staged_entries(&staging), 0, "a failed backup leaves no staging");

        create(backup.clone()).await.expect("create backup");
        assert!(backup.exists());
        assert_eq!(staged_entries(&staging), 0, "the plaintext archive is removed");
    });
}

/// The space every encrypted backup test seals to.
fn test_space() -> SpaceId {
    SpaceId::personal("vault-backup-test")
}

/// The fixed recovery key pair standing in for a recovery kit.
fn kit() -> EncryptionKeyPair {
    EncryptionKeyPair::from_secret(Zeroizing::new([7u8; 32])).expect("kit key")
}

/// Seal keys for the test space, with `epoch_key` when the vault is linked.
fn seal_keys(epoch_key: Option<SpaceKey>) -> BackupSealKeys {
    BackupSealKeys {
        space: test_space(),
        epoch_key,
        recovery: kit().public_key(),
    }
}

/// Entries left in the staging directory `path`, zero when it does not exist.
fn staged_entries(path: &Path) -> usize {
    std::fs::read_dir(path).map_or(0, |entries| entries.count())
}

/// Bytes deflate cannot shrink, so an archive of them spans several chunks.
fn noise(length: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_be_bytes()[0]
        })
        .collect()
}

/// Seals `plaintext` as an archive with `keys` and returns the backup bytes.
fn sealed(directory: &TestDirectory, plaintext: &[u8], keys: &BackupSealKeys) -> Vec<u8> {
    let archive = directory.path().join("plain.zip");
    let backup = directory.path().join("sealed.ganbaru-backup");
    std::fs::write(&archive, plaintext).expect("write plaintext");
    seal_archive(&archive, keys, &backup).expect("seal archive");
    std::fs::read(backup).expect("read backup")
}

/// Decrypts the backup `bytes` with the kit, returning the plaintext or the refusal. The
/// restored archive must not survive a refusal.
fn open_with_kit(directory: &TestDirectory, bytes: &[u8]) -> Result<Vec<u8>, BackupFileError> {
    let source = directory.path().join("opened.ganbaru-backup");
    let archive = directory.path().join("opened.zip");
    let _ = std::fs::remove_file(&archive);
    std::fs::write(&source, bytes).expect("write backup");
    let result = decrypt_backup(
        &source,
        |header| header.open_with_recovery(&kit()),
        &archive,
    );
    match result {
        Ok(_) => Ok(std::fs::read(&archive).expect("read restored archive")),
        Err(error) => {
            assert!(!archive.exists(), "a refused backup leaves no plaintext");
            Err(error)
        }
    }
}

#[test]
fn encrypted_backup_files_round_trip_at_chunk_boundaries() {
    let directory = TestDirectory::new("backup-boundaries");
    let keys = seal_keys(Some(SpaceKey::generate(3).expect("epoch key")));
    for length in [
        0,
        1,
        BACKUP_CHUNK_BYTES - 1,
        BACKUP_CHUNK_BYTES,
        BACKUP_CHUNK_BYTES + 1,
        3 * BACKUP_CHUNK_BYTES,
    ] {
        let plaintext = noise(length);
        let bytes = sealed(&directory, &plaintext, &keys);
        let chunks = length.div_ceil(BACKUP_CHUNK_BYTES).max(1);
        let header = 25 + 72 + 96;
        assert_eq!(
            bytes.len(),
            header + chunks * (4 + 24 + 16) + length,
            "{length}"
        );
        assert_eq!(open_with_kit(&directory, &bytes).expect("open"), plaintext);
    }
}

#[test]
fn encrypted_backup_files_refuse_damage_at_the_file_level() {
    let directory = TestDirectory::new("backup-damage");
    let keys = seal_keys(Some(SpaceKey::generate(1).expect("epoch key")));
    let plaintext = noise(2 * BACKUP_CHUNK_BYTES + 10);
    let bytes = sealed(&directory, &plaintext, &keys);
    let header = 25 + 72 + 96;
    let frame = 4 + 24 + BACKUP_CHUNK_BYTES + 16;
    let refusal = |bytes: &[u8]| open_with_kit(&directory, bytes).expect_err("refused");

    assert_eq!(
        refusal(b"PK\x03\x04 a plaintext zip"),
        BackupFileError::Format(BackupError::NotBackup)
    );
    assert_eq!(
        refusal(b"GB"),
        BackupFileError::Format(BackupError::Truncated)
    );
    for cut in [10, header - 1, header, header + 2, header + 30, header + frame - 1] {
        assert_eq!(
            refusal(&bytes[..cut]),
            BackupFileError::Format(BackupError::Truncated),
            "{cut}"
        );
    }
    for cut in [header + frame, header + 2 * frame] {
        assert_eq!(
            refusal(&bytes[..cut]),
            BackupFileError::Format(BackupError::Authentication),
            "{cut}"
        );
    }
    let mut extended = bytes.clone();
    extended.extend_from_slice(&bytes[header..header + frame]);
    assert!(matches!(refusal(&extended), BackupFileError::Format(_)));
    let mut swapped = bytes[..header].to_vec();
    swapped.extend_from_slice(&bytes[header + frame..header + 2 * frame]);
    swapped.extend_from_slice(&bytes[header..header + frame]);
    swapped.extend_from_slice(&bytes[header + 2 * frame..]);
    assert_eq!(
        refusal(&swapped),
        BackupFileError::Format(BackupError::Authentication)
    );
    let mut flipped = bytes.clone();
    flipped[header + frame + 100] ^= 1;
    assert_eq!(
        refusal(&flipped),
        BackupFileError::Format(BackupError::Authentication)
    );
    let mut oversized = bytes.clone();
    oversized[header..header + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        refusal(&oversized),
        BackupFileError::Format(BackupError::Malformed)
    );
    let other = sealed(&directory, &plaintext, &keys);
    let mut foreign = other[..header].to_vec();
    foreign.extend_from_slice(&bytes[header..]);
    assert_eq!(
        refusal(&foreign),
        BackupFileError::Format(BackupError::Authentication)
    );
}

#[test]
fn encrypted_backup_files_open_with_the_epoch_key_or_the_kit_only() {
    let directory = TestDirectory::new("backup-keys");
    let epoch_key = SpaceKey::generate(2).expect("epoch key");
    let plaintext = noise(4096);
    let linked = sealed(&directory, &plaintext, &seal_keys(Some(epoch_key.clone())));
    let source = directory.path().join("linked.ganbaru-backup");
    std::fs::write(&source, &linked).expect("write backup");
    let header = read_backup_header(&source).expect("read header");
    assert_eq!((header.space, header.epoch), (test_space(), 2));

    let archive = directory.path().join("epoch.zip");
    decrypt_backup(&source, |header| header.open_with_epoch_key(&epoch_key), &archive)
        .expect("open with the epoch key");
    assert_eq!(std::fs::read(&archive).expect("read archive"), plaintext);
    let other = SpaceKey::generate(2).expect("other key");
    let refused = directory.path().join("refused.zip");
    assert_eq!(
        decrypt_backup(&source, |header| header.open_with_epoch_key(&other), &refused),
        Err(BackupFileError::Format(BackupError::Key))
    );
    let wrong_kit = EncryptionKeyPair::generate().expect("other kit");
    assert_eq!(
        decrypt_backup(&source, |header| header.open_with_recovery(&wrong_kit), &refused),
        Err(BackupFileError::Format(BackupError::Key))
    );
    assert!(!refused.exists());

    let unlinked = sealed(&directory, &plaintext, &seal_keys(None));
    std::fs::write(&source, &unlinked).expect("write backup");
    assert_eq!(read_backup_header(&source).expect("header").epoch, 0);
    assert_eq!(
        decrypt_backup(&source, |header| header.open_with_epoch_key(&epoch_key), &refused),
        Err(BackupFileError::Format(BackupError::Epoch))
    );
    assert_eq!(open_with_kit(&directory, &unlinked).expect("open"), plaintext);
}

#[test]
fn encrypted_backup_carries_the_snapshot_archive_of_the_vault() {
    block_on(async {
        let directory = TestDirectory::new("backup-encrypted");
        let root = directory.path().join("vault");
        std::fs::create_dir_all(&root).expect("create vault root");
        let registry = DatabasePoolRegistry::default();
        let pool = create_vault(&registry, &root.join(DATABASE_FILE)).await;
        context(&mut *acquire(&pool).await).await;
        add_note(&mut *acquire(&pool).await, "sealed").await;
        registry.close_all().await.expect("close vault");
        write_layout(&root);
        std::fs::create_dir_all(root.join("assets/notes/files")).expect("create assets");
        std::fs::write(
            root.join("assets/notes/files/large.bin"),
            noise(2 * BACKUP_CHUNK_BYTES),
        )
        .expect("write asset");
        let backup = directory.path().join("vault.ganbaru-backup");
        let staging = directory.path().join("staging");

        let written = create_encrypted_backup(
            &ArchiveGate::default(),
            &root,
            &staging,
            &seal_keys(None),
            &backup,
        )
        .await
        .expect("create encrypted backup");

        assert_eq!(
            written,
            std::fs::metadata(&backup).expect("backup metadata").len()
        );
        assert_eq!(staged_entries(&staging), 0);
        let archive = directory.path().join("restored.zip");
        decrypt_backup(&backup, |header| header.open_with_recovery(&kit()), &archive)
            .expect("decrypt backup");
        let names = archive_names(&archive);
        for expected in [DATABASE_FILE, "notes/page.md", "assets/notes/files/large.bin"] {
            assert!(names.iter().any(|name| name == expected), "{expected}");
        }
        assert!(!names.iter().any(|name| name == VAULT_LOCK_FILE));
        let extracted = directory.path().join("extracted.sqlite");
        extract_database(&archive, &extracted);
        assert_eq!(note_ids(&extracted).await, vec!["sealed".to_string()]);
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
        assert_eq!(captured, 0, "the backup holds no local capture queue");
    });
}
