use crate::models::{NoteLocalUserDto, NoteLocalUserRow};
use crate::validation::require_uuid;
use serde_json::json;
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};

const DEFAULT_LOCAL_USER_DISPLAY_NAME: &str = "You";

/// Portable preference that names the person using this vault.
///
/// The app's preference registry owns the key; Notes reads it so local comment, suggestion, and
/// reaction authors store no copy of the name and a rename writes one preference.
pub const PROFILE_DISPLAY_NAME_PREFERENCE_KEY: &str = "profile.displayName";

pub async fn load_local_user(pool: &SqlitePool) -> Result<NoteLocalUserDto, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes local user load: {e}"))?;
    let row = ensure_local_user_tx(&mut tx).await?;
    let display_name = local_author_name(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes local user load: {e}"))?;
    Ok(NoteLocalUserDto::new(row, display_name))
}

pub async fn current_local_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<NoteLocalUserRow, String> {
    ensure_local_user_tx(tx).await
}

/// Reads the local author's name from the profile preference, falling back to the default name
/// when the preference is unset, blank, or not a JSON string.
pub async fn local_author_name(connection: &mut SqliteConnection) -> Result<String, String> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM vault_preferences WHERE key = ?")
            .bind(PROFILE_DISPLAY_NAME_PREFERENCE_KEY)
            .fetch_optional(&mut *connection)
            .await
            .map_err(|e| format!("load profile display name: {e}"))?;
    Ok(stored
        .and_then(|value| serde_json::from_str::<String>(&value).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| DEFAULT_LOCAL_USER_DISPLAY_NAME.to_string()))
}

/// Reads the local author's name through a pool connection.
pub async fn local_author_name_from_pool(pool: &SqlitePool) -> Result<String, String> {
    let mut connection = pool
        .acquire()
        .await
        .map_err(|e| format!("acquire profile display name connection: {e}"))?;
    local_author_name(&mut connection).await
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

/// Returns a stored author name, or the local author's name when the row stores none.
pub fn resolved_display_name_json(stored: Option<&str>, local_author_name: &str) -> String {
    stored.map_or_else(
        || comment_display_name_json(local_author_name),
        str::to_string,
    )
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
    sqlx::query("INSERT INTO notes_local_users (id) VALUES (?)")
        .bind(&id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create notes local user: {e}"))?;
    load_local_user_tx(tx, &id).await
}

async fn load_first_local_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<Option<NoteLocalUserRow>, String> {
    sqlx::query_as::<_, NoteLocalUserRow>(
        "SELECT id, created_time
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
        "SELECT id, created_time
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
