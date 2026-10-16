//! Quick notes tags: an ordered, uniquely named set that filters the active notes.

use ganbaru_sync_contracts::OrderKey;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};

use super::order::{OrderedRow, placement, take_key};
use super::{validate_color, validate_id};

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
    /// Palette slot that tagged notes take.
    pub(super) color: i64,
}

/// Moves a tag between two adjacent neighbors; a missing anchor means the start or the end.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteTagReorder {
    pub(super) id: String,
    pub(super) previous_id: Option<String>,
    pub(super) next_id: Option<String>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteTagRead {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) color: i64,
    pub(super) order_key: String,
    created_at: String,
    updated_at: String,
}

/// Every tag in display order.
pub async fn list_from_pool(pool: &SqlitePool) -> Result<Vec<QuickNoteTagRead>, String> {
    sqlx::query_as::<_, QuickNoteTagRead>(
        "SELECT id, name, color, order_key, created_at, updated_at
         FROM quick_note_tags ORDER BY order_key, id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("list quick note tags: {error}"))
}

async fn load(pool: &SqlitePool, id: &str) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    sqlx::query_as::<_, QuickNoteTagRead>(
        "SELECT id, name, color, order_key, created_at, updated_at
         FROM quick_note_tags WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("load quick note tag: {error}"))?
    .ok_or_else(not_found)
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
    validate_color(tag.color)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note tag create: {error}"))?;
    let group = ordered_group(&mut tx, &tag.id).await?;
    if group.len() >= MAX_TAGS {
        return Err(QuickNoteTagError::LimitReached(format!(
            "quick notes cannot have more than {MAX_TAGS} tags"
        )));
    }
    ensure_unique_name(&mut tx, &tag.id, name).await?;
    let (order_key, others) = take_key(placement(&group, group.len(), &tag.id)?, &tag.id)?;
    write_order_keys(&mut tx, &others).await?;
    sqlx::query("INSERT INTO quick_note_tags (id, name, color, order_key) VALUES (?, ?, ?, ?)")
        .bind(&tag.id)
        .bind(name)
        .bind(tag.color)
        .bind(order_key.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("create quick note tag: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note tag create: {error}"))?;
    load(pool, &tag.id).await
}

/// Renames and recolors a tag, keeping names unique without regard to case. Notes keep the
/// color they already have.
pub async fn update_from_pool(
    pool: &SqlitePool,
    tag: QuickNoteTagWrite,
) -> Result<QuickNoteTagRead, QuickNoteTagError> {
    validate_id(&tag.id)?;
    let name = validate_tag_name(&tag.name)?;
    validate_color(tag.color)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note tag update: {error}"))?;
    ensure_unique_name(&mut tx, &tag.id, name).await?;
    let result = sqlx::query(
        "UPDATE quick_note_tags
         SET name = ?, color = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(name)
    .bind(tag.color)
    .bind(&tag.id)
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("update quick note tag: {error}"))?;
    if result.rows_affected() != 1 {
        return Err(not_found());
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note tag update: {error}"))?;
    load(pool, &tag.id).await
}

/// Moves a tag between two adjacent neighbors in display order.
pub async fn reorder_from_pool(
    pool: &SqlitePool,
    request: QuickNoteTagReorder,
) -> Result<(), QuickNoteTagError> {
    validate_id(&request.id)?;
    for anchor in [&request.previous_id, &request.next_id]
        .into_iter()
        .flatten()
    {
        validate_id(anchor)?;
    }
    if request.previous_id.as_deref() == Some(request.id.as_str())
        || request.next_id.as_deref() == Some(request.id.as_str())
        || (request.previous_id.is_some() && request.previous_id == request.next_id)
    {
        return Err("quick note tag reorder anchors must be distinct"
            .to_string()
            .into());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note tag reorder: {error}"))?;
    let exists = sqlx::query_scalar::<_, i64>("SELECT 1 FROM quick_note_tags WHERE id = ?")
        .bind(&request.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| format!("load quick note tag reorder source: {error}"))?;
    if exists.is_none() {
        return Err(not_found());
    }
    let group = ordered_group(&mut tx, &request.id).await?;
    let position = |anchor: Option<&str>, label: &str| -> Result<Option<usize>, String> {
        anchor
            .map(|id| {
                group
                    .iter()
                    .position(|row| row.id == id)
                    .ok_or_else(|| format!("quick note tag reorder {label} anchor is invalid"))
            })
            .transpose()
    };
    let previous = position(request.previous_id.as_deref(), "previous")?;
    let next = position(request.next_id.as_deref(), "next")?;
    let index = match (previous, next) {
        (Some(previous), Some(next)) if previous + 1 != next => {
            return Err("quick note tag reorder anchors are not adjacent"
                .to_string()
                .into());
        }
        (_, Some(next)) => next,
        (Some(previous), None) => previous + 1,
        (None, None) => 0,
    };
    write_order_keys(&mut tx, &placement(&group, index, &request.id)?).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note tag reorder: {error}").into())
}

fn not_found() -> QuickNoteTagError {
    QuickNoteTagError::NotFound("quick note tag not found".to_string())
}

/// Every tag except `excluding`, in display order.
async fn ordered_group(
    tx: &mut Transaction<'_, Sqlite>,
    excluding: &str,
) -> Result<Vec<OrderedRow>, String> {
    sqlx::query_as::<_, OrderedRow>(
        "SELECT id, order_key FROM quick_note_tags WHERE id <> ? ORDER BY order_key, id",
    )
    .bind(excluding)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("load quick note tag order: {error}"))
}

async fn write_order_keys(
    tx: &mut Transaction<'_, Sqlite>,
    updates: &[(String, OrderKey)],
) -> Result<(), String> {
    for (id, key) in updates {
        sqlx::query("UPDATE quick_note_tags SET order_key = ? WHERE id = ?")
            .bind(key.as_str())
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|error| format!("write quick note tag order: {error}"))?;
    }
    Ok(())
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
