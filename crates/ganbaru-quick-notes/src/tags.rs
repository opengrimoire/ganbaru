//! Quick notes tags: an ordered, uniquely named set that filters the active notes.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

use super::order::{OrderedRow, placement, take_key};
use super::validate_id;

/// Local create rule; synchronized devices can hold more tags.
const MAX_TAGS: usize = 9;
const MAX_TAG_NAME_CHARS: usize = 40;

/// Stable tag write outcomes, independent of diagnostic wording.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum QuickNoteTagError {
    DuplicateName(String),
    LimitReached(String),
    NotFound(String),
    Failed(String),
}

impl From<String> for QuickNoteTagError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteTagWrite {
    pub(super) id: String,
    pub(super) name: String,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteTagRead {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) order_key: String,
    created_at: String,
    updated_at: String,
}

/// Every tag in display order.
pub async fn list_from_pool(pool: &SqlitePool) -> Result<Vec<QuickNoteTagRead>, String> {
    sqlx::query_as::<_, QuickNoteTagRead>(
        "SELECT id, name, order_key, created_at, updated_at
         FROM quick_note_tags ORDER BY order_key, id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("list quick note tags: {error}"))
}

async fn load(pool: &SqlitePool, id: &str) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    sqlx::query_as::<_, QuickNoteTagRead>(
        "SELECT id, name, order_key, created_at, updated_at FROM quick_note_tags WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("load quick note tag: {error}"))?
    .ok_or_else(|| QuickNoteTagError::NotFound("quick note tag not found".to_string()))
}

async fn ensure_unique_name(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    name: &str,
) -> Result<(), QuickNoteTagError> {
    // The name column compares without case, matching its UNIQUE constraint.
    let duplicate = sqlx::query_scalar::<_, String>(
        "SELECT id FROM quick_note_tags WHERE name = ? AND id <> ? LIMIT 1",
    )
    .bind(name)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| format!("check quick note tag name: {error}"))?;
    match duplicate {
        Some(_) => Err(QuickNoteTagError::DuplicateName(
            "a quick note tag with this name already exists".to_string(),
        )),
        None => Ok(()),
    }
}

/// Creates a tag after the last one.
pub async fn create_from_pool(
    pool: &SqlitePool,
    tag: QuickNoteTagWrite,
) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    validate_id(&tag.id)?;
    let name = validate_tag_name(&tag.name)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note tag create: {error}"))?;
    let group = sqlx::query_as::<_, OrderedRow>(
        "SELECT id, order_key FROM quick_note_tags ORDER BY order_key, id",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|error| format!("load quick note tag order: {error}"))?;
    if group.len() >= MAX_TAGS {
        return Err(QuickNoteTagError::LimitReached(format!(
            "quick notes cannot have more than {MAX_TAGS} tags"
        )));
    }
    ensure_unique_name(&mut tx, &tag.id, name).await?;
    let (order_key, others) = take_key(placement(&group, group.len(), &tag.id)?, &tag.id)?;
    for (id, key) in others {
        sqlx::query("UPDATE quick_note_tags SET order_key = ? WHERE id = ?")
            .bind(key.as_str())
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|error| format!("re-key quick note tags: {error}"))?;
    }
    sqlx::query("INSERT INTO quick_note_tags (id, name, order_key) VALUES (?, ?, ?)")
        .bind(&tag.id)
        .bind(name)
        .bind(order_key.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("create quick note tag: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note tag create: {error}"))?;
    load(pool, &tag.id).await
}

/// Renames a tag, keeping names unique without regard to case.
pub async fn rename_from_pool(
    pool: &SqlitePool,
    tag: QuickNoteTagWrite,
) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    validate_id(&tag.id)?;
    let name = validate_tag_name(&tag.name)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note tag rename: {error}"))?;
    ensure_unique_name(&mut tx, &tag.id, name).await?;
    let result = sqlx::query(
        "UPDATE quick_note_tags
         SET name = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(name)
    .bind(&tag.id)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("rename quick note tag: {error}"))?;
    if result.rows_affected() != 1 {
        return Err(QuickNoteTagError::NotFound(
            "quick note tag not found".to_string(),
        ));
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note tag rename: {error}"))?;
    load(pool, &tag.id).await
}

/// Deletes a tag and untags its notes. Deleting a missing tag succeeds, because another device
/// may have deleted it first.
pub async fn delete_from_pool(pool: &SqlitePool, id: &str) -> Result<(), QuickNoteTagError> {
    validate_id(id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note tag delete: {error}"))?;
    // Open editors hold the old tag, so its notes take a new revision before the foreign key
    // clears the reference.
    sqlx::query("UPDATE quick_notes SET revision = revision + 1 WHERE tag_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("untag quick notes: {error}"))?;
    sqlx::query("DELETE FROM quick_note_tags WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("delete quick note tag: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note tag delete: {error}").into())
}

fn validate_tag_name(name: &str) -> Result<&str, String> {
    let trimmed = name.trim();
    let count = trimmed.chars().count();
    if count == 0 || count > MAX_TAG_NAME_CHARS {
        return Err(format!(
            "quick note tag name must be between 1 and {MAX_TAG_NAME_CHARS} characters"
        ));
    }
    Ok(trimmed)
}
