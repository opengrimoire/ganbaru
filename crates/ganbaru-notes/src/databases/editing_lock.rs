//! A shell-local editing preference that protects database structure from accidental changes.

use crate::models::{NoteBlockRow, NoteDatabaseReferenceDto};
use crate::validation::{require_uuid, validate_block_payload};
use crate::{databases, page_history, project_history};
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
    let block = load_shell_tx(tx, database_id).await?;
    let payload: Value = serde_json::from_str(&block.payload)
        .map_err(|error| format!("read database editing lock: {error}"))?;
    if is_editing_locked(&payload)? {
        return Err("database layout is locked; unlock it before changing structure".to_string());
    }
    Ok(())
}

/// Read whether a database shell's structure is locked.
pub(crate) async fn shell_locked_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
) -> Result<bool, String> {
    let block = load_shell_tx(tx, database_id).await?;
    let payload: Value = serde_json::from_str(&block.payload)
        .map_err(|error| format!("read database editing lock: {error}"))?;
    is_editing_locked(&payload)
}

async fn load_shell_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
) -> Result<NoteBlockRow, String> {
    require_uuid(database_id, "database_id")?;
    sqlx::query_as("SELECT block.*, block.type AS block_type FROM notes_blocks AS block JOIN notes_databases AS database ON database.id = block.id WHERE block.id = ? AND block.type = 'child_database' AND block.in_trash = 0 AND database.in_trash = 0")
        .bind(database_id).fetch_optional(&mut **tx).await
        .map_err(|error| format!("load database editing preference: {error}"))?
        .ok_or_else(|| "database shell not found".to_string())
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
    let block = load_shell_tx(&mut tx, database_id).await?;
    let mut payload: Value = serde_json::from_str(&block.payload)
        .map_err(|error| format!("read database editing preference: {error}"))?;
    if is_editing_locked(&payload)? != locked {
        page_history::record_page_snapshot_tx(&mut tx, &block.page_id, "database_editing_lock")
            .await?;
        project_history::mark_page_dirty_tx(
            &mut tx,
            &block.page_id,
            "Database editing lock",
            false,
        )
        .await?;
        payload["editing_locked"] = Value::Bool(locked);
        validate_block_payload("child_database", &payload)?;
        sqlx::query("UPDATE notes_blocks SET payload = ?, last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
            .bind(payload.to_string()).bind(database_id).execute(&mut *tx).await
            .map_err(|error| format!("save database editing preference: {error}"))?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit database editing preference: {error}"))?;
    databases::database_reference(pool, database_id).await
}
