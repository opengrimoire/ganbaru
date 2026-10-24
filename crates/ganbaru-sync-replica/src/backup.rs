//! Whole-vault archives: the layout a bundle carries, consistent database snapshots, and the
//! zip archives that backups and the bootstrap snapshot of a vault handoff are written as.
//!
//! Archive creation and the managed asset sweep share an [`ArchiveGate`], so an archive never
//! holds a database snapshot whose asset files the sweep removed while it was written.

use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::manifest::vault::VAULT_MANIFEST;
use sqlx::{Connection, SqliteConnection, SqlitePool};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

/// File name of the vault marker.
pub const VAULT_MANIFEST_FILE: &str = "vault.json";
/// File name of the vault configuration.
pub const CONFIG_FILE: &str = "config.json";
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

/// Whether `relative` belongs to the vault layout a bundle carries: the vault marker,
/// configuration, and database files, the skeleton directories with their contents, and only
/// folders directly below `projects`. Anything else, such as a `.git`, `.claude`, `.codex`, or
/// `.vscode` folder at the vault root, stays out of bundles in both directions.
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
        None => [VAULT_MANIFEST_FILE, CONFIG_FILE, DATABASE_FILE].contains(&top),
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
/// device-local row and sync state, so it restores this installation as it was. `config_lock`
/// is held while the files are read, so the configuration is read whole.
pub async fn create_backup_archive(
    gate: &ArchiveGate,
    pool: &SqlitePool,
    vault_root: &Path,
    database_snapshot: &Path,
    destination: &Path,
    config_lock: &Mutex<()>,
) -> Result<(), String> {
    let _gate = gate.lock().await;
    vacuum_database(pool, database_snapshot).await?;
    write_archive(vault_root, database_snapshot, destination, config_lock)
}

/// Archives the vault at `vault_root` to `archive_path` as the snapshot another installation
/// starts from: the database copy at `database_snapshot` leaves out installation-local sync
/// state and device-local rows. The caller excludes vault writers first.
pub async fn create_snapshot_archive(
    gate: &ArchiveGate,
    vault_root: &Path,
    database_snapshot: &Path,
    archive_path: &Path,
    config_lock: &Mutex<()>,
) -> Result<(), String> {
    let _gate = gate.lock().await;
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
    write_archive(vault_root, database_snapshot, archive_path, config_lock)
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
    config_lock: &Mutex<()>,
) -> Result<(), String> {
    let _config_guard = config_lock
        .lock()
        .map_err(|_| "vault config lock is unavailable".to_string())?;
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
