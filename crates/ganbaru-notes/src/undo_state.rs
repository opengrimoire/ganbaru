use crate::validation::require_uuid;
use serde_json::Value;
use sqlx::SqlitePool;

const MAX_UNDO_STATE_BYTES: usize = 512 * 1024;

fn validate_undo_state_json(state_json: &str) -> Result<(), String> {
    if state_json.trim().is_empty() {
        return Err("notes undo state must not be empty".to_string());
    }
    if state_json.len() > MAX_UNDO_STATE_BYTES {
        return Err("notes undo state is too large".to_string());
    }
    serde_json::from_str::<Value>(state_json)
        .map(|_| ())
        .map_err(|e| format!("parse notes undo state: {e}"))
}

async fn require_active_page(pool: &SqlitePool, page_id: &str) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check notes undo state page: {e}"))?;
    if exists.is_none() {
        return Err("notes page not found".to_string());
    }
    Ok(())
}

pub async fn load_undo_state(pool: &SqlitePool, page_id: &str) -> Result<Option<String>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    require_active_page(pool, page_id).await?;
    sqlx::query_scalar("SELECT state_payload FROM notes_undo_state WHERE page_id = ?")
        .bind(page_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load notes undo state: {e}"))
}

pub async fn save_undo_state(
    pool: &SqlitePool,
    page_id: &str,
    state_json: &str,
) -> Result<(), String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    require_active_page(pool, page_id).await?;
    validate_undo_state_json(state_json)?;
    sqlx::query(
        "INSERT INTO notes_undo_state (page_id, state_payload, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(page_id) DO UPDATE SET
             state_payload = excluded.state_payload,
             updated_at = excluded.updated_at",
    )
    .bind(page_id)
    .bind(state_json)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| format!("save notes undo state: {e}"))
}

pub async fn clear_undo_state(pool: &SqlitePool, page_id: &str) -> Result<(), String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    sqlx::query("DELETE FROM notes_undo_state WHERE page_id = ?")
        .bind(page_id)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| format!("clear notes undo state: {e}"))
}
