//! Carry-forward of locally stored operations across a vault database replacement.
//!
//! Before a snapshot replaces the vault, every operation the current database stores and the
//! replacement lacks is written to a device-local bundle. The next service start imports the
//! bundle into the replacement and deletes it, so a crash between the two steps loses nothing.

use super::writer::SyncFiles;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::local::{
    self, MAX_BUNDLE_BYTES, decode_bundle, encode_bundle, replacement_vector,
};
use ganbaru_sync::{CarryReport, Engine, SpaceContext};
use sqlx::SqliteConnection;
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
    let count = result?;
    closed?;
    Ok(count)
}

async fn export_with(
    engine: &Engine,
    registry: &DatabasePoolRegistry,
    current: &Path,
    replacement: &Path,
    files: &SyncFiles,
    coordinator_device_id: &str,
) -> Result<usize, String> {
    let current_pool = registry.connect_path_read_only(current).await?;
    let mut current_conn = acquire(&current_pool).await?;
    let Some(ctx) = local::space_context(&mut current_conn, &files.vault_id, coordinator_device_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(0);
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
        return Ok(0);
    }
    let bytes = encode_bundle(ctx.space, &ops);
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err("local sync operations exceed the carry-forward limit".to_string());
    }
    std::fs::create_dir_all(&files.directory)
        .map_err(|error| format!("create sync directory: {error}"))?;
    ganbaru_handoff::pairing::write_private_file_atomically(&files.carry_path(), &bytes)?;
    Ok(ops.len())
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
