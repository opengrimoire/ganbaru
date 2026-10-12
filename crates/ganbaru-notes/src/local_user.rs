use crate::models::{NoteLocalUserDto, NoteLocalUserRow, NoteLocalUserUpdate};
use crate::validation::require_uuid;
use serde_json::json;
use sqlx::{Sqlite, SqlitePool, Transaction};

const DEFAULT_LOCAL_USER_DISPLAY_NAME: &str = "You";
const LOCAL_USER_DISPLAY_NAME_MAX_CHARS: usize = 80;

pub async fn load_local_user(pool: &SqlitePool) -> Result<NoteLocalUserDto, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes local user load: {e}"))?;
    let row = ensure_local_user_tx(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes local user load: {e}"))?;
    Ok(NoteLocalUserDto::new(row))
}

pub async fn update_local_user(
    pool: &SqlitePool,
    update: NoteLocalUserUpdate,
) -> Result<NoteLocalUserDto, String> {
    let display_name = normalize_display_name(&update.display_name)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes local user update: {e}"))?;
    let existing = ensure_local_user_tx(&mut tx).await?;
    sqlx::query(
        "UPDATE notes_local_users
         SET display_name = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(&display_name)
    .bind(&existing.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update notes local user: {e}"))?;

    sqlx::query(
        "UPDATE notes_comments
         SET display_name = ?
         WHERE created_by = ?
           AND json_extract(display_name, '$.type') = 'user'",
    )
    .bind(comment_display_name_json(&display_name))
    .bind(&existing.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update notes local comment display names: {e}"))?;

    sqlx::query(
        "UPDATE notes_suggestions
         SET display_name = ?
         WHERE created_by = ?
           AND json_extract(display_name, '$.type') = 'user'",
    )
    .bind(comment_display_name_json(&display_name))
    .bind(&existing.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update notes local suggestion display names: {e}"))?;

    let row = load_local_user_tx(&mut tx, &existing.id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes local user update: {e}"))?;
    Ok(NoteLocalUserDto::new(row))
}

pub async fn current_local_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<NoteLocalUserRow, String> {
    ensure_local_user_tx(tx).await
}

pub fn comment_display_name_json(display_name: &str) -> String {
    let resolved_name = display_name.trim();
    let resolved_name = if resolved_name.is_empty() {
        DEFAULT_LOCAL_USER_DISPLAY_NAME
    } else {
        resolved_name
    };
    json!({
        "type": "user",
        "resolved_name": resolved_name,
    })
    .to_string()
}

/// Reads the name captured by [`comment_display_name_json`], falling back to the default for malformed values.
pub fn comment_display_name_text(display_name_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(display_name_json)
        .ok()
        .and_then(|value| {
            value
                .get("resolved_name")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| DEFAULT_LOCAL_USER_DISPLAY_NAME.to_string())
}

async fn ensure_local_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<NoteLocalUserRow, String> {
    if let Some(row) = load_first_local_user_tx(tx).await? {
        return Ok(row);
    }
    let id = generate_local_user_id_tx(tx).await?;
    sqlx::query(
        "INSERT INTO notes_local_users (id, display_name)
         VALUES (?, ?)",
    )
    .bind(&id)
    .bind(DEFAULT_LOCAL_USER_DISPLAY_NAME)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create notes local user: {e}"))?;
    load_local_user_tx(tx, &id).await
}

async fn load_first_local_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<Option<NoteLocalUserRow>, String> {
    sqlx::query_as::<_, NoteLocalUserRow>(
        "SELECT *
         FROM notes_local_users
         ORDER BY created_time ASC, id ASC
         LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes local user: {e}"))
}

async fn load_local_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<NoteLocalUserRow, String> {
    sqlx::query_as::<_, NoteLocalUserRow>(
        "SELECT *
         FROM notes_local_users
         WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes local user: {e}"))?
    .ok_or_else(|| "notes local user not found".to_string())
}

async fn generate_local_user_id_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<String, String> {
    for _ in 0..32 {
        let id: String = sqlx::query_scalar(
            "SELECT lower(hex(randomblob(4))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(6)))",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("generate notes local user id: {e}"))?;
        require_uuid(&id, "generated_local_user_id")?;
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM notes_local_users WHERE id = ?")
                .bind(&id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|e| format!("check notes local user id: {e}"))?;
        if exists.is_none() {
            return Ok(id);
        }
    }
    Err("could not generate a unique notes local user id".to_string())
}

fn normalize_display_name(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("display_name is required".to_string());
    }
    if trimmed.chars().count() > LOCAL_USER_DISPLAY_NAME_MAX_CHARS {
        return Err("display_name is too long".to_string());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("display_name must not contain control characters".to_string());
    }
    Ok(trimmed.to_string())
}
