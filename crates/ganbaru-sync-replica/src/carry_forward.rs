//! Carry-forward of locally stored operations and device state across a vault database
//! replacement.
//!
//! Before a snapshot replaces the vault, every operation the current database stores and the
//! replacement lacks is written to a device-local bundle, stored and applied in the staged
//! replacement, and the bundle is deleted. The receiver's `Local` rows and columns are then
//! copied from the current database into the replacement. The current database stays the vault
//! until the replacement is renamed into place, so a failure at any step loses nothing. A bundle
//! that a failure leaves behind is prepended to the next replacement's bundle, and a service
//! start imports it into the open vault, where its operations are already stored.

use super::writer::SyncFiles;
use ganbaru_atomic_files::{FileAccess, StagedFile, create_directories_within};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::assets::{AssetClass, CONTENT_TYPE_EXTENSIONS, UNKNOWN_EXTENSION, is_sha256_hex};
use ganbaru_sync::local::{
    self, MAX_BUNDLE_BYTES, decode_bundle, encode_bundle, replacement_vector,
};
use ganbaru_sync::manifest::Manifest;
use ganbaru_sync::{CarryReport, Engine, LocalRestoreReport, SpaceContext};
use sha2::{Digest as _, Sha256};
use sqlx::{Connection as _, SqliteConnection};
use std::io::Read as _;
use std::path::Path;

/// Error for a carry bundle that is not a regular file within the bundle limit.
const BUNDLE_TOO_LARGE: &str = "carry-forward bundle is not a file within the size limit";
/// Bytes read at a time while an asset file is carried into a replacement.
const CARRY_BUFFER_BYTES: usize = 64 * 1024;

/// Exports the operations of `current` that `replacement` lacks into the carry bundle of the
/// vault, after any bundle an interrupted replacement left. Returns how many operations the
/// bundle holds. Pending captures refuse the export, because they would be lost.
pub async fn export(
    engine: &Engine,
    current: &Path,
    replacement: &Path,
    files: &SyncFiles,
) -> Result<usize, String> {
    Ok(export_space(engine, current, replacement, files)
        .await?
        .map_or(0, |(_, count)| count))
}

/// Carries the operations of `current` that the staged `replacement` lacks into it: exports
/// them into the carry bundle, stores and applies the bundle in the replacement, and deletes
/// the bundle. Returns `None` when `current` has no sync space or nothing was pending.
pub async fn carry_into_replacement(
    engine: &Engine,
    current: &Path,
    replacement: &Path,
    files: &SyncFiles,
    now_ms: u64,
) -> Result<Option<CarryReport>, String> {
    let Some((ctx, _)) = export_space(engine, current, replacement, files).await? else {
        return Ok(None);
    };
    let mut conn = open_staged(replacement).await?;
    let imported = import_pending(engine, &mut conn, &ctx, files, now_ms).await;
    close_staged(conn, imported).await
}

/// Copies the `Local` rows and columns of `current` into the staged `replacement` and resolves
/// the references the copy leaves dangling. See [`local::restore_local_state`].
pub async fn restore_into_replacement(
    engine: &Engine,
    current: &Path,
    replacement: &Path,
) -> Result<LocalRestoreReport, String> {
    let mut conn = open_staged(replacement).await?;
    let restored = local::restore_local_state(&mut conn, engine.manifest(), current)
        .await
        .map_err(|error| format!("restore device state: {error}"));
    close_staged(conn, restored).await
}

/// Opens a staged replacement database outside any pool registry, so it never shares a pool
/// with the vault it replaces.
async fn open_staged(path: &Path) -> Result<SqliteConnection, String> {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(true);
    SqliteConnection::connect_with(&options)
        .await
        .map_err(|error| format!("open replacement database: {error}"))
}

/// Closes a staged replacement connection, keeping the first error.
async fn close_staged<T>(conn: SqliteConnection, result: Result<T, String>) -> Result<T, String> {
    let closed = conn
        .close()
        .await
        .map_err(|error| format!("close replacement database: {error}"));
    let value = result?;
    closed?;
    Ok(value)
}

/// Exports the carry bundle and returns the space of `current` with how many operations the
/// bundle holds, or `None` when `current` has no sync space.
async fn export_space(
    engine: &Engine,
    current: &Path,
    replacement: &Path,
    files: &SyncFiles,
) -> Result<Option<(SpaceContext, usize)>, String> {
    let registry = DatabasePoolRegistry::default();
    let result = export_with(engine, &registry, current, replacement, files).await;
    let closed = registry.close_all().await;
    let exported = result?;
    closed?;
    Ok(exported)
}

async fn export_with(
    engine: &Engine,
    registry: &DatabasePoolRegistry,
    current: &Path,
    replacement: &Path,
    files: &SyncFiles,
) -> Result<Option<(SpaceContext, usize)>, String> {
    let current_pool = registry.connect_path_read_only(current).await?;
    let mut current_conn = acquire(&current_pool).await?;
    let Some(ctx) = local::space_context(&mut current_conn, &files.vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(None);
    };
    if local::has_pending_captures(&mut current_conn)
        .await
        .map_err(|error| format!("read pending sync changes: {error}"))?
    {
        return Err("local synced changes are not sealed yet".to_string());
    }
    let replacement_pool = registry.connect_path_read_only(replacement).await?;
    let mut replacement_conn = acquire(&replacement_pool).await?;
    let staged = replacement_vector(&mut replacement_conn, ctx.space)
        .await
        .map_err(|error| format!("read replacement sync state: {error}"))?;
    drop(replacement_conn);
    let mut ops = read_bundle(files, &ctx)?.unwrap_or_default();
    ops.extend(
        engine
            .carry_forward_ops(&mut current_conn, &ctx, &staged)
            .await
            .map_err(|error| format!("collect local sync operations: {error}"))?,
    );
    if ops.is_empty() {
        return Ok(Some((ctx, 0)));
    }
    let bytes = encode_bundle(ctx.space, &ops);
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err("local sync operations exceed the carry-forward limit".to_string());
    }
    std::fs::create_dir_all(&files.directory)
        .map_err(|error| format!("create sync directory: {error}"))?;
    ganbaru_handoff::pairing::write_private_file_atomically(&files.carry_path(), &bytes)?;
    Ok(Some((ctx, ops.len())))
}

/// Imports a pending carry bundle into the vault database and deletes it. Returns `None` when
/// no bundle is pending.
pub async fn import_pending(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    files: &SyncFiles,
    now_ms: u64,
) -> Result<Option<CarryReport>, String> {
    let Some(ops) = read_bundle(files, ctx)? else {
        return Ok(None);
    };
    let report = engine
        .import_carried(conn, ctx, &files.vault_id, &ops, now_ms)
        .await
        .map_err(|error| format!("import carried sync operations: {error}"))?;
    remove_bundle(files)?;
    Ok(Some(report))
}

/// Reads the carry bundle of the vault. A bundle of another space cannot apply and is an error.
/// The size is checked on the opened file before reading, and the read stops one byte past the
/// limit, so an oversized or growing file is refused without loading it.
fn read_bundle(files: &SyncFiles, ctx: &SpaceContext) -> Result<Option<Vec<Vec<u8>>>, String> {
    let file = match std::fs::File::open(files.carry_path()) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read carry-forward bundle: {error}")),
    };
    let limit = MAX_BUNDLE_BYTES as u64;
    let metadata = file
        .metadata()
        .map_err(|error| format!("read carry-forward bundle: {error}"))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(BUNDLE_TOO_LARGE.to_string());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read carry-forward bundle: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(BUNDLE_TOO_LARGE.to_string());
    }
    let (space, ops) =
        decode_bundle(&bytes).map_err(|error| format!("decode carry-forward bundle: {error}"))?;
    if space != ctx.space {
        return Err("carry-forward bundle belongs to another sync space".to_string());
    }
    Ok(Some(ops))
}

/// Copies the managed asset files of `current` that `replacement` lacks into `replacement`,
/// for every registered class, before a whole-vault replacement, so assets only this device
/// holds survive until sync sends them. Only regular `{sha256}.{ext}` files within their class
/// bound whose bytes match their digest are copied; others are left behind. Returns how many
/// files were copied.
pub fn carry_assets(
    manifest: &Manifest,
    current: &Path,
    replacement: &Path,
) -> Result<usize, String> {
    let mut copied = 0;
    for class in manifest.asset_classes() {
        copied += carry_class(class, current, replacement)?;
    }
    Ok(copied)
}

fn carry_class(class: &AssetClass, current: &Path, replacement: &Path) -> Result<usize, String> {
    let entries = match std::fs::read_dir(class.directory(current)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("list {} assets: {error}", class.name)),
    };
    let target = class.directory(replacement);
    let mut copied = 0;
    for entry in entries {
        let entry = entry.map_err(|error| format!("list {} assets: {error}", class.name))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(digest) = carried_digest(&name) else {
            continue;
        };
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| format!("inspect {} asset: {error}", class.name))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > u64::from(class.max_bytes)
        {
            continue;
        }
        match std::fs::symlink_metadata(target.join(&name)) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("inspect carried {} asset: {error}", class.name)),
        }
        create_directories_within(replacement, &target)
            .map_err(|error| format!("create {} asset directory: {error}", class.name))?;
        if copy_verified(&entry.path(), &target.join(&name), digest, class.max_bytes)
            .map_err(|error| format!("carry {} asset: {error}", class.name))?
        {
            copied += 1;
        }
    }
    Ok(copied)
}

/// Digest of an asset file name, when the name is `{sha256}.{ext}` with an extension the
/// engine names files with.
fn carried_digest(name: &str) -> Option<&str> {
    let (digest, extension) = name.split_once('.')?;
    let known = extension == UNKNOWN_EXTENSION
        || CONTENT_TYPE_EXTENSIONS
            .iter()
            .any(|(_, known)| *known == extension);
    (is_sha256_hex(digest) && known).then_some(digest)
}

/// Streams `source` into a temporary file beside `target` while hashing it, and moves it into
/// place only when the digest matches and the bytes stay within `max_bytes`. Returns whether
/// the file was copied.
fn copy_verified(
    source: &Path,
    target: &Path,
    digest: &str,
    max_bytes: u32,
) -> std::io::Result<bool> {
    let mut reader = std::fs::File::open(source)?.take(u64::from(max_bytes) + 1);
    let mut staged = StagedFile::create(target, FileAccess::Default)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CARRY_BUFFER_BYTES];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        staged.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        if staged.written() > u64::from(max_bytes) {
            return Ok(false);
        }
    }
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != digest {
        return Ok(false);
    }
    staged.commit()?;
    Ok(true)
}

fn remove_bundle(files: &SyncFiles) -> Result<(), String> {
    match std::fs::remove_file(files.carry_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove carry-forward bundle: {error}")),
    }
}

async fn acquire(
    pool: &sqlx::SqlitePool,
) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, String> {
    pool.acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))
}
