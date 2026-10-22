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
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::local::{
    self, MAX_BUNDLE_BYTES, decode_bundle, encode_bundle, replacement_vector,
};
use ganbaru_sync::{CarryReport, Engine, LocalRestoreReport, SpaceContext};
use sqlx::{Connection as _, SqliteConnection};
use std::io::Read as _;
use std::path::Path;

/// Error for a carry bundle that is not a regular file within the bundle limit.
const BUNDLE_TOO_LARGE: &str = "carry-forward bundle is not a file within the size limit";

/// Exports the operations of `current` that `replacement` lacks into the carry bundle of the
/// vault, after any bundle an interrupted replacement left. Returns how many operations the
/// bundle holds. Pending captures refuse the export, because they would be lost.
/// `coordinator_device_id` identifies the device whose writers may revoke in the vault.
pub async fn export(
    engine: &Engine,
    current: &Path,
    replacement: &Path,
    files: &SyncFiles,
    coordinator_device_id: &str,
) -> Result<usize, String> {
    Ok(
        export_space(engine, current, replacement, files, coordinator_device_id)
            .await?
            .map_or(0, |(_, count)| count),
    )
}

/// Carries the operations of `current` that the staged `replacement` lacks into it: exports
/// them into the carry bundle, stores and applies the bundle in the replacement, and deletes
/// the bundle. Returns `None` when `current` has no sync space or nothing was pending.
pub async fn carry_into_replacement(
    engine: &Engine,
    current: &Path,
    replacement: &Path,
    files: &SyncFiles,
    coordinator_device_id: &str,
    now_ms: u64,
) -> Result<Option<CarryReport>, String> {
    let Some((ctx, _)) =
        export_space(engine, current, replacement, files, coordinator_device_id).await?
    else {
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
    coordinator_device_id: &str,
) -> Result<Option<(SpaceContext, usize)>, String> {
    let registry = DatabasePoolRegistry::default();
    let result = export_with(
        engine,
        &registry,
        current,
        replacement,
        files,
        coordinator_device_id,
    )
    .await;
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
    coordinator_device_id: &str,
) -> Result<Option<(SpaceContext, usize)>, String> {
    let current_pool = registry.connect_path_read_only(current).await?;
    let mut current_conn = acquire(&current_pool).await?;
    let Some(ctx) = local::space_context(&mut current_conn, &files.vault_id, coordinator_device_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(None);
    };
    if local::has_pending_captures(&mut current_conn)
        .await
        .map_err(|error| format!("read pending sync changes: {error}"))?
    {
        return Err("local Quick notes changes are not sealed yet".to_string());
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
