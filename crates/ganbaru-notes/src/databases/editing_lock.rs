//! A device-local editing preference that protects database structure from accidental changes.
//!
//! The lock lives in `notes_block_view_state`, so it is not page content: setting it records no
//! page history and leaves the block's edit time unchanged.

use crate::block_storage::{BlockViewState, write_block_view_state};
use crate::databases;
use crate::models::NoteDatabaseReferenceDto;
use crate::validation::require_uuid;
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Read the shell editing lock, treating an absent flag as unlocked.
pub(crate) fn is_editing_locked(payload: &Value) -> Result<bool, String> {
    match payload.get("editing_locked") {
        None => Ok(false),
        Some(Value::Bool(locked)) => Ok(*locked),
        _ => Err("database editing_locked must be a boolean".to_string()),
    }
}

/// Guard a structural write in its requesting shell, independently of source ownership.
pub(crate) async fn ensure_unlocked_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
) -> Result<(), String> {
    if shell_locked_tx(tx, database_id).await? {
        return Err("database layout is locked; unlock it before changing structure".to_string());
    }
    Ok(())
}

/// Read whether a database shell's structure is locked.
pub(crate) async fn shell_locked_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
) -> Result<bool, String> {
    Ok(load_shell_view_tx(tx, database_id)
        .await?
        .editing_locked
        .unwrap_or(false))
}

async fn load_shell_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
) -> Result<BlockViewState, String> {
    require_uuid(database_id, "database_id")?;
    let row: Option<(Option<bool>, Option<bool>)> = sqlx::query_as(
        "SELECT view_state.open, view_state.editing_locked
         FROM notes_blocks AS block
         JOIN notes_databases AS database ON database.id = block.id
         LEFT JOIN notes_block_view_state AS view_state ON view_state.block_id = block.id
         WHERE block.id = ? AND block.type = 'child_database'
           AND block.in_trash = 0 AND database.in_trash = 0",
    )
    .bind(database_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| format!("load database editing preference: {error}"))?;
    let (open, editing_locked) = row.ok_or_else(|| "database shell not found".to_string())?;
    Ok(BlockViewState {
        open,
        editing_locked,
    })
}

/// Persist the local editing preference without changing data-source or row authority.
pub async fn set_database_editing_lock(
    pool: &SqlitePool,
    database_id: &str,
    locked: bool,
) -> Result<NoteDatabaseReferenceDto, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin database editing lock update: {error}"))?;
    let view = load_shell_view_tx(&mut tx, database_id).await?;
    if view.editing_locked.unwrap_or(false) != locked {
        let view = BlockViewState {
            editing_locked: Some(locked),
            ..view
        };
        write_block_view_state(&mut tx, database_id, view)
            .await
            .map_err(|error| format!("save database editing preference: {error}"))?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit database editing preference: {error}"))?;
    databases::database_reference(pool, database_id).await
}
