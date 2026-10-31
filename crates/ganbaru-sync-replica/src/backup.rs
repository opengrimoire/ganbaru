//! Whole-vault archives: the layout a bundle carries, consistent database snapshots, and the
//! zip archives that backups and the bootstrap snapshot of a vault handoff are written as.
//!
//! Archive creation and the managed asset sweep share an [`ArchiveGate`], so an archive never
//! holds a database snapshot whose asset files the sweep removed while it was written.
//!
//! User backups are snapshot archives sealed into the `GBBK` format of
//! [`ganbaru_sync_contracts::backup`]: the plaintext archive lives only in a private staging
//! directory, and the encrypted file is streamed in fixed chunks, so memory stays constant.

mod extract;
pub mod seal_keys;

pub use extract::{
    DIFFERENT_VAULT_ERROR, MAX_VAULT_MANIFEST_BYTES, SkippedEntries, VAULT_APP_MARKER,
    VAULT_SCHEMA_VERSION, VaultManifest, extract_backup_archive, extract_backup_archive_within,
    parse_vault_manifest, safe_archive_path,
};

use ganbaru_atomic_files::{FileAccess, StagedFile};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::manifest::vault::VAULT_MANIFEST;
use ganbaru_sync_contracts::backup::{
    BACKUP_CHUNK_BYTES, BACKUP_MAGIC, BACKUP_NONCE_BYTES, BackupError, BackupHeader, ChunkOpener,
    ChunkSealer, FileKey, HEADER_PREFIX_BYTES, MAX_CHUNK_CIPHERTEXT_BYTES,
};
use ganbaru_sync_contracts::{EncryptionPublicKey, SpaceId, SpaceKey};
use sqlx::{Connection, SqliteConnection, SqlitePool};
use std::fmt;
use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

/// File name of the vault marker.
pub const VAULT_MANIFEST_FILE: &str = "vault.json";
/// File name of the vault database.
pub const DATABASE_FILE: &str = "ganbaru.sqlite";
/// Most entries one archive holds.
pub const BACKUP_MAX_ENTRIES: usize = 100_000;
/// Most uncompressed bytes one archive holds.
pub const BACKUP_MAX_BYTES: u64 = 100 * 1024 * 1024 * 1024;
/// Deepest relative path one archive holds.
pub const BACKUP_MAX_DEPTH: usize = 64;
/// Bytes copied at a time between an archive and a file.
pub const COPY_BUFFER_BYTES: usize = 64 * 1024;
/// Top-level directories a vault bundle carries with everything below them.
/// Every vault skeleton directory lives below one of these.
pub const BUNDLE_DIRECTORIES: &[&str] = &[
    "notes",
    "diary",
    "projects",
    "reports",
    "assets",
    "templates",
];
/// Directory whose direct children are project working folders.
const BUNDLE_PROJECTS_DIRECTORY: &str = "projects";

/// Whether `relative` belongs to the vault layout a bundle carries: the vault marker and
/// database files, the skeleton directories with their contents, and only folders directly
/// below `projects`. Anything else, such as a `.git`, `.claude`, `.codex`, or `.vscode` folder
/// or a stray `config.json` at the vault root, stays out of bundles in both directions.
pub fn is_bundle_layout_entry(relative: &Path, directory: bool) -> bool {
    let mut components = relative.components().map(|component| match component {
        Component::Normal(name) => name.to_str(),
        _ => None,
    });
    let Some(Some(top)) = components.next() else {
        return false;
    };
    match components.next() {
        None if directory => BUNDLE_DIRECTORIES.contains(&top),
        None => [VAULT_MANIFEST_FILE, DATABASE_FILE].contains(&top),
        Some(None) => false,
        Some(Some(_)) if top == BUNDLE_PROJECTS_DIRECTORY => {
            directory || components.next().is_some()
        }
        Some(Some(_)) => BUNDLE_DIRECTORIES.contains(&top),
    }
}

/// Whether `name` is a SQLite journal, WAL, or shared-memory file of the vault database. These
/// belong to one database file and never move with another.
pub fn is_database_sidecar(name: &Path) -> bool {
    name.file_name()
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_prefix(DATABASE_FILE))
        .is_some_and(|suffix| suffix.starts_with('-'))
}

/// Lets one archive creation or managed asset sweep of a process run at a time.
#[derive(Clone, Debug, Default)]
pub struct ArchiveGate(Arc<tokio::sync::Mutex<()>>);

/// Held while an archive is created or the asset sweep runs.
#[derive(Debug)]
pub struct ArchiveGuard {
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

impl ArchiveGate {
    /// The gate every archive and sweep of this process shares.
    pub fn process() -> Self {
        static PROCESS: LazyLock<ArchiveGate> = LazyLock::new(ArchiveGate::default);
        PROCESS.clone()
    }

    /// Waits until no archive or sweep runs, and holds the gate until the guard drops.
    pub async fn lock(&self) -> ArchiveGuard {
        ArchiveGuard {
            _guard: Arc::clone(&self.0).lock_owned().await,
        }
    }
}

/// Writes a consistent copy of the database `pool` serves to `destination`.
pub async fn vacuum_database(pool: &SqlitePool, destination: &Path) -> Result<(), String> {
    let destination = destination
        .to_str()
        .ok_or_else(|| "database snapshot path contains non-UTF-8 characters".to_string())?;
    sqlx::query("VACUUM INTO ?")
        .bind(destination)
        .execute(pool)
        .await
        .map_err(|error| format!("create consistent database snapshot: {error}"))?;
    Ok(())
}

/// Archives the vault at `vault_root` to `destination` with a consistent snapshot of the
/// database `pool` serves, written to `database_snapshot` first. The archive keeps every
/// device-local row and sync state, so it restores this installation as it was.
pub async fn create_backup_archive(
    gate: &ArchiveGate,
    pool: &SqlitePool,
    vault_root: &Path,
    database_snapshot: &Path,
    destination: &Path,
) -> Result<(), String> {
    let _gate = gate.lock().await;
    vacuum_database(pool, database_snapshot).await?;
    write_archive(vault_root, database_snapshot, destination)
}

/// Checks an extracted vault database against the current schema and for damage, then closes it.
pub async fn validate_vault_database(database: &Path) -> Result<(), String> {
    let metadata =
        fs::metadata(database).map_err(|error| format!("vault has no database: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err("vault database is empty".to_string());
    }
    let registry = DatabasePoolRegistry::default();
    let pool = registry.connect_path_read_only(database).await?;
    let validation = async {
        // The catalog check runs first so that integrity checking never reaches virtual table
        // modules, such as FTS5, that the file defines or reshapes.
        ganbaru_db::validate_current_schema(&pool).await?;
        let (result,): (String,) = sqlx::query_as("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .map_err(|error| format!("check vault database: {error}"))?;
        if result != "ok" {
            return Err(format!(
                "vault database failed its integrity check: {result}"
            ));
        }
        Ok(())
    }
    .await;
    let closed = registry.close_all().await;
    validation.and(closed)
}

/// Archives the vault at `vault_root` to `archive_path` as the snapshot another installation
/// starts from: the database copy at `database_snapshot` leaves out installation-local sync
/// state and device-local rows. The caller excludes vault writers first.
pub async fn create_snapshot_archive(
    gate: &ArchiveGate,
    vault_root: &Path,
    database_snapshot: &Path,
    archive_path: &Path,
) -> Result<(), String> {
    let _gate = gate.lock().await;
    write_snapshot_archive(vault_root, database_snapshot, archive_path).await
}

/// Writes the snapshot archive of [`create_snapshot_archive`] while the caller holds the gate.
async fn write_snapshot_archive(
    vault_root: &Path,
    database_snapshot: &Path,
    archive_path: &Path,
) -> Result<(), String> {
    for path in [database_snapshot, archive_path] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove stale vault snapshot: {error}")),
        }
    }
    let registry = DatabasePoolRegistry::default();
    let pool = registry
        .connect_path(vault_root.join(DATABASE_FILE))
        .await?;
    let snapshot_result = vacuum_database(&pool, database_snapshot).await;
    registry.close_all().await?;
    snapshot_result?;
    prepare_snapshot_copy(database_snapshot).await?;
    write_archive(vault_root, database_snapshot, archive_path)
}

/// Clears installation-local sync state and device-local rows from a snapshot copy before it
/// leaves this device.
async fn prepare_snapshot_copy(snapshot: &Path) -> Result<(), String> {
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(snapshot);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| format!("open vault snapshot: {error}"))?;
    let result = ganbaru_sync::local::prepare_snapshot_copy(&mut conn, &VAULT_MANIFEST)
        .await
        .map_err(|error| format!("prepare vault snapshot: {error}"));
    let closed = conn
        .close()
        .await
        .map_err(|error| format!("close vault snapshot: {error}"));
    result.and(closed)
}

#[derive(Clone)]
struct ArchiveEntry {
    source: PathBuf,
    relative: PathBuf,
    directory: bool,
}

fn collect_archive_entries(
    root: &Path,
    current: &Path,
    entries: &mut Vec<ArchiveEntry>,
    total_bytes: &mut u64,
) -> Result<(), String> {
    let mut children = fs::read_dir(current)
        .map_err(|error| format!("read backup source '{}': {error}", current.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read backup source entry: {error}"))?;
    children.sort_by_key(fs::DirEntry::file_name);

    for child in children {
        if entries.len() >= BACKUP_MAX_ENTRIES {
            return Err("Ganbaru folder contains too many backup entries".to_string());
        }
        let source = child.path();
        let relative = source
            .strip_prefix(root)
            .map_err(|_| "backup entry escaped the Ganbaru folder".to_string())?
            .to_path_buf();
        let depth = relative.components().count();
        if depth > BACKUP_MAX_DEPTH {
            return Err("Ganbaru folder is nested too deeply to back up".to_string());
        }
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("inspect backup entry '{}': {error}", source.display()))?;
        if !is_bundle_layout_entry(&relative, metadata.is_dir()) {
            continue;
        }
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "backup does not support symbolic link '{}'",
                relative.display()
            ));
        }
        if metadata.is_dir() {
            entries.push(ArchiveEntry {
                source: source.clone(),
                relative,
                directory: true,
            });
            collect_archive_entries(root, &source, entries, total_bytes)?;
        } else if metadata.is_file() {
            if is_database_sidecar(&relative) {
                continue;
            }
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > BACKUP_MAX_BYTES {
                return Err("Ganbaru folder exceeds the backup size limit".to_string());
            }
            entries.push(ArchiveEntry {
                source,
                relative,
                directory: false,
            });
        } else {
            return Err(format!(
                "backup contains unsupported entry '{}'",
                relative.display()
            ));
        }
    }
    Ok(())
}

fn archive_name(path: &Path, directory: bool) -> Result<String, String> {
    let mut name = path
        .to_str()
        .ok_or_else(|| "backup entry path contains non-UTF-8 characters".to_string())?
        .replace('\\', "/");
    if directory && !name.ends_with('/') {
        name.push('/');
    }
    Ok(name)
}

/// Writes the layout entries of `vault_root` to a zip archive at `destination`, taking the
/// database bytes from `database_snapshot`.
fn write_archive(
    vault_root: &Path,
    database_snapshot: &Path,
    destination: &Path,
) -> Result<(), String> {
    let mut entries = Vec::new();
    let mut total_bytes = 0;
    collect_archive_entries(vault_root, vault_root, &mut entries, &mut total_bytes)?;

    let file =
        fs::File::create(destination).map_err(|error| format!("create backup archive: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let directory_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o700);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];

    for entry in entries {
        let name = archive_name(&entry.relative, entry.directory)?;
        if entry.directory {
            writer
                .add_directory(name, directory_options)
                .map_err(|error| format!("write backup directory: {error}"))?;
            continue;
        }
        writer
            .start_file(name, options)
            .map_err(|error| format!("write backup entry: {error}"))?;
        let source = if entry.relative == Path::new(DATABASE_FILE) {
            database_snapshot
        } else {
            &entry.source
        };
        let mut input = fs::File::open(source).map_err(|error| {
            format!("open backup entry '{}': {error}", entry.relative.display())
        })?;
        loop {
            let count = input.read(&mut buffer).map_err(|error| {
                format!("read backup entry '{}': {error}", entry.relative.display())
            })?;
            if count == 0 {
                break;
            }
            writer.write_all(&buffer[..count]).map_err(|error| {
                format!(
                    "compress backup entry '{}': {error}",
                    entry.relative.display()
                )
            })?;
        }
    }
    writer
        .finish()
        .map_err(|error| format!("finish backup archive: {error}"))?
        .sync_all()
        .map_err(|error| format!("sync backup archive: {error}"))?;
    Ok(())
}

/// Prefix of the private staging directory one encrypted backup is assembled in.
const BACKUP_STAGING_PREFIX: &str = ".ganbaru-backup-";
/// File name of the snapshot database inside a backup staging directory.
const STAGED_DATABASE_FILE: &str = "snapshot.sqlite";
/// File name of the plaintext archive inside a backup staging directory.
const STAGED_ARCHIVE_FILE: &str = "archive.zip";

/// The keys a new encrypted backup is sealed with.
#[derive(Debug)]
pub struct BackupSealKeys {
    /// The space the vault belongs to.
    pub space: SpaceId,
    /// The newest epoch key this installation holds, or `None` for a vault never linked.
    pub epoch_key: Option<SpaceKey>,
    /// The recovery encryption key the recovery kit opens.
    pub recovery: EncryptionPublicKey,
}

/// An encrypted backup that could not be written or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupFileError {
    /// The backup bytes break the format, or no offered key opens them.
    Format(BackupError),
    /// Reading, writing, or archiving failed, with context.
    Io(String),
}

impl fmt::Display for BackupFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => error.fmt(formatter),
            Self::Io(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for BackupFileError {}

impl From<BackupError> for BackupFileError {
    fn from(error: BackupError) -> Self {
        Self::Format(error)
    }
}

/// Maps a read error, treating an early end of file as a truncated backup.
fn read_error(context: &str, error: io::Error) -> BackupFileError {
    if error.kind() == ErrorKind::UnexpectedEof {
        BackupFileError::Format(BackupError::Truncated)
    } else {
        BackupFileError::Io(format!("{context}: {error}"))
    }
}

/// A private staging directory removed with everything in it when dropped.
struct StagingDirectory(PathBuf);

impl StagingDirectory {
    fn create(parent: &Path) -> Result<Self, BackupFileError> {
        ganbaru_atomic_files::create_private_directories(parent)
            .map_err(|error| BackupFileError::Io(format!("create backup staging: {error}")))?;
        ganbaru_atomic_files::create_private_temporary_directory(parent, BACKUP_STAGING_PREFIX)
            .map(Self)
            .map_err(|error| BackupFileError::Io(format!("create backup staging: {error}")))
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        // A leftover private directory holds nothing the vault does not, and the next backup
        // uses a fresh name, so a failed removal is not an error.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Writes an encrypted backup of the vault at `vault_root` to `destination`.
///
/// The archive is the snapshot another installation starts from, so a restore never carries
/// installation-local sync state or device-local rows. It is assembled in a fresh private
/// directory below `staging`, which is removed on every path. The gate stays held from the
/// database snapshot until the encrypted file is synced and renamed over `destination`, so
/// the asset sweep never removes a file the archive references while it is written. The
/// caller excludes vault writers first. Returns the bytes written.
pub async fn create_encrypted_backup(
    gate: &ArchiveGate,
    vault_root: &Path,
    staging: &Path,
    keys: &BackupSealKeys,
    destination: &Path,
) -> Result<u64, BackupFileError> {
    let _gate = gate.lock().await;
    let directory = StagingDirectory::create(staging)?;
    let archive = directory.0.join(STAGED_ARCHIVE_FILE);
    write_snapshot_archive(
        vault_root,
        &directory.0.join(STAGED_DATABASE_FILE),
        &archive,
    )
    .await
    .map_err(BackupFileError::Io)?;
    seal_archive(&archive, keys, destination)
}

/// Streams the archive at `archive` into an encrypted backup at `destination`.
pub(crate) fn seal_archive(
    archive: &Path,
    keys: &BackupSealKeys,
    destination: &Path,
) -> Result<u64, BackupFileError> {
    let (header, file_key) =
        BackupHeader::seal(&keys.space, keys.epoch_key.as_ref(), &keys.recovery)?;
    let mut sealer = ChunkSealer::new(&header, file_key);
    let mut input = io::BufReader::new(
        fs::File::open(archive)
            .map_err(|error| BackupFileError::Io(format!("open backup archive: {error}")))?,
    );
    let mut output = StagedFile::create(destination, FileAccess::OwnerOnly)
        .map_err(|error| BackupFileError::Io(format!("create backup file: {error}")))?;
    let write = |output: &mut StagedFile, bytes: &[u8]| {
        output
            .write_all(bytes)
            .map_err(|error| BackupFileError::Io(format!("write backup file: {error}")))
    };
    write(&mut output, &header.to_bytes())?;

    let mut current = Vec::with_capacity(MAX_CHUNK_CIPHERTEXT_BYTES);
    let mut next = Vec::with_capacity(MAX_CHUNK_CIPHERTEXT_BYTES);
    fill_chunk(&mut input, &mut current)?;
    loop {
        // A chunk is last when the archive has no byte after it, so a full chunk at the end of
        // the archive is last and no empty chunk follows it.
        let last = if current.len() < BACKUP_CHUNK_BYTES {
            true
        } else {
            fill_chunk(&mut input, &mut next)?;
            next.is_empty()
        };
        let nonce = sealer.seal_in_place(&mut current, last)?;
        let length = u32::try_from(current.len()).map_err(|_| BackupError::Malformed)?;
        write(&mut output, &length.to_be_bytes())?;
        write(&mut output, &nonce)?;
        write(&mut output, &current)?;
        if last {
            break;
        }
        std::mem::swap(&mut current, &mut next);
    }
    let written = output.written();
    output
        .commit()
        .map_err(|error| BackupFileError::Io(format!("save backup file: {error}")))?;
    Ok(written)
}

/// Replaces `buffer` with up to one chunk of plaintext from `input`.
fn fill_chunk(input: &mut impl Read, buffer: &mut Vec<u8>) -> Result<(), BackupFileError> {
    buffer.clear();
    input
        .take(BACKUP_CHUNK_BYTES as u64)
        .read_to_end(buffer)
        .map_err(|error| BackupFileError::Io(format!("read backup archive: {error}")))?;
    Ok(())
}

/// Opens the backup at `source`, refusing anything that is not a regular file.
fn open_backup(source: &Path) -> Result<io::BufReader<fs::File>, BackupFileError> {
    let file = fs::File::open(source)
        .map_err(|error| BackupFileError::Io(format!("open backup file: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| BackupFileError::Io(format!("inspect backup file: {error}")))?;
    if !metadata.is_file() {
        return Err(BackupFileError::Format(BackupError::NotBackup));
    }
    Ok(io::BufReader::with_capacity(COPY_BUFFER_BYTES, file))
}

/// Reads and decodes the header at the start of `input`.
fn read_header(input: &mut impl Read) -> Result<BackupHeader, BackupFileError> {
    let mut prefix = [0u8; HEADER_PREFIX_BYTES];
    let available = read_up_to(input, &mut prefix)?;
    if available < HEADER_PREFIX_BYTES {
        let magic = available.min(BACKUP_MAGIC.len());
        return Err(BackupFileError::Format(
            if prefix[..magic] == BACKUP_MAGIC[..magic] {
                BackupError::Truncated
            } else {
                BackupError::NotBackup
            },
        ));
    }
    let length = BackupHeader::length(&prefix)?;
    let mut bytes = vec![0u8; length];
    bytes[..HEADER_PREFIX_BYTES].copy_from_slice(&prefix);
    input
        .read_exact(&mut bytes[HEADER_PREFIX_BYTES..])
        .map_err(|error| read_error("read backup header", error))?;
    Ok(BackupHeader::decode(&bytes)?)
}

/// Fills as much of `buffer` as `input` holds, returning the bytes read.
fn read_up_to(input: &mut impl Read, buffer: &mut [u8]) -> Result<usize, BackupFileError> {
    let mut filled = 0;
    while filled < buffer.len() {
        match input.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(BackupFileError::Io(format!("read backup file: {error}"))),
        }
    }
    Ok(filled)
}

/// Reads the next frame length, or `None` at the end of the file.
fn read_frame_length(input: &mut impl Read) -> Result<Option<u32>, BackupFileError> {
    let mut prefix = [0u8; 4];
    match read_up_to(input, &mut prefix)? {
        0 => Ok(None),
        4 => Ok(Some(u32::from_be_bytes(prefix))),
        _ => Err(BackupFileError::Format(BackupError::Truncated)),
    }
}

/// Reads the clear header of the backup at `source` without opening it, so a caller can tell
/// which space and epoch it belongs to and whether the recovery kit is needed.
pub fn read_backup_header(source: &Path) -> Result<BackupHeader, BackupFileError> {
    read_header(&mut open_backup(source)?)
}

/// Decrypts the backup at `source` into the new private file `archive`, with the file key
/// `open` returns for its header. Every chunk is authenticated before its plaintext is
/// written, and `archive` is removed when any check fails. Returns the header.
pub fn decrypt_backup(
    source: &Path,
    open: impl FnOnce(&BackupHeader) -> Result<FileKey, BackupError>,
    archive: &Path,
) -> Result<BackupHeader, BackupFileError> {
    let mut input = open_backup(source)?;
    let header = read_header(&mut input)?;
    let mut opener = ChunkOpener::new(&header, open(&header)?);
    let file = ganbaru_atomic_files::create_private_file(archive)
        .map_err(|error| BackupFileError::Io(format!("create restored archive: {error}")))?;
    let result = decrypt_chunks(&mut input, &mut opener, file);
    if result.is_err() {
        // The partial plaintext is private and discarded either way.
        let _ = fs::remove_file(archive);
    }
    result.map(|()| header)
}

/// Opens every chunk after the header and writes the plaintext to `file`.
fn decrypt_chunks(
    input: &mut impl Read,
    opener: &mut ChunkOpener,
    file: fs::File,
) -> Result<(), BackupFileError> {
    let mut output = io::BufWriter::with_capacity(COPY_BUFFER_BYTES, file);
    let mut buffer = Vec::with_capacity(MAX_CHUNK_CIPHERTEXT_BYTES);
    let mut length = read_frame_length(input)?.ok_or(BackupError::Truncated)?;
    loop {
        let size = opener.check_length(length)?;
        let mut nonce = [0u8; BACKUP_NONCE_BYTES];
        input
            .read_exact(&mut nonce)
            .map_err(|error| read_error("read backup chunk", error))?;
        buffer.resize(size, 0);
        input
            .read_exact(&mut buffer)
            .map_err(|error| read_error("read backup chunk", error))?;
        let next = read_frame_length(input)?;
        opener.open_in_place(&nonce, &mut buffer, next.is_none())?;
        output
            .write_all(&buffer)
            .map_err(|error| BackupFileError::Io(format!("write restored archive: {error}")))?;
        match next {
            Some(value) => length = value,
            None => break,
        }
    }
    output
        .into_inner()
        .map_err(|error| BackupFileError::Io(format!("write restored archive: {error}")))?
        .sync_all()
        .map_err(|error| BackupFileError::Io(format!("sync restored archive: {error}")))?;
    Ok(())
}
