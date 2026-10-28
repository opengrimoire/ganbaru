//! Conflicts on a Notes page: its title, block text, property values, discussion status, and
//! comment bodies.
//!
//! Concurrent edits from linked devices whose values differ leave one displayed value on every
//! replica and the others as versions to choose from. Keeping the displayed version seals the
//! group; using another version writes it through the same path as a local edit, then seals the
//! group, so the conflict closes on every device.

use std::collections::{BTreeSet, HashMap};

use ganbaru_sync::manifest::vault::notes::{
    BLOCKS_TABLE, COMMENTS_TABLE, PAGES_TABLE, THREADS_TABLE, VALUES_TABLE, block_group,
    comment_group, page_group, thread_group, value_group,
};
use ganbaru_sync::{ConflictDetail, ConflictVersion, DeviceNames, DeviceRef, Engine, local};
use ganbaru_sync_contracts::{Field, GroupId, SpaceId, TableId, Value as SyncValue, WriterId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::block_storage::compose_block_content;
use crate::comments::{resolve_comment_thread_tx, update_comment_tx};
use crate::models::{NoteBlockUpdate, NoteCommentUpdate, NotePageUpdate};
use crate::page_property_storage::write_page_property_value;
use crate::project_history::{ensure_page_baseline_for_mutation, mark_page_dirty_tx};
use crate::validation::{require_uuid, validate_page_update};
use crate::writes::{rich_text, update_block_tx, update_page_tx};

/// Stable conflict outcomes, independent of diagnostic wording.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum NotesConflictError {
    /// The conflict or the chosen version no longer exists; reload and show the current state.
    Resolved(String),
    /// The chosen block version cannot be written because the block's type no longer stores
    /// text it can hold; keep the shown version or edit the block.
    Unfit(String),
    Failed(String),
}

impl From<String> for NotesConflictError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// A field of a page that can hold a conflict.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotesConflictTarget {
    /// The page title; the id is the page id.
    PageTitle,
    /// A block's text; the id is the block id.
    BlockText,
    /// A property value of the page; the id is the property id.
    PropertyValue,
    /// Whether a discussion is open or resolved; the id is the thread id.
    ThreadStatus,
    /// A comment's body; the id is the comment id.
    CommentBody,
}

impl NotesConflictTarget {
    const ALL: [Self; 5] = [
        Self::PageTitle,
        Self::BlockText,
        Self::PropertyValue,
        Self::ThreadStatus,
        Self::CommentBody,
    ];

    fn table(self) -> TableId {
        match self {
            Self::PageTitle => PAGES_TABLE,
            Self::BlockText => BLOCKS_TABLE,
            Self::PropertyValue => VALUES_TABLE,
            Self::ThreadStatus => THREADS_TABLE,
            Self::CommentBody => COMMENTS_TABLE,
        }
    }

    fn group(self) -> GroupId {
        match self {
            Self::PageTitle => page_group::TITLE,
            Self::BlockText => block_group::TEXT,
            Self::PropertyValue => value_group::VALUE,
            Self::ThreadStatus => thread_group::STATUS,
            Self::CommentBody => comment_group::BODY,
        }
    }

    /// Rows of the page this target covers, from sync row key to the id the views use.
    fn scope_sql(self) -> &'static str {
        match self {
            Self::PageTitle => "SELECT id, id FROM notes_pages WHERE id = ?",
            Self::BlockText => "SELECT id, id FROM notes_blocks WHERE page_id = ?",
            Self::PropertyValue => {
                "SELECT sync_key, property_id FROM notes_page_property_values WHERE page_id = ?"
            }
            Self::ThreadStatus => "SELECT id, id FROM notes_comment_threads WHERE page_id = ?",
            Self::CommentBody => {
                "SELECT c.id, c.id FROM notes_comments c
                 JOIN notes_comment_threads t ON t.id = c.thread_id WHERE t.page_id = ?"
            }
        }
    }

    /// The sync row key of one field, bound to the page id and the field id, found only when
    /// the field belongs to that page.
    fn key_sql(self) -> &'static str {
        match self {
            Self::PageTitle => "SELECT id FROM notes_pages WHERE id = ? AND id = ?",
            Self::BlockText => "SELECT id FROM notes_blocks WHERE page_id = ? AND id = ?",
            Self::PropertyValue => {
                "SELECT sync_key FROM notes_page_property_values
                 WHERE page_id = ? AND property_id = ?"
            }
            Self::ThreadStatus => {
                "SELECT id FROM notes_comment_threads WHERE page_id = ? AND id = ?"
            }
            Self::CommentBody => {
                "SELECT c.id FROM notes_comments c
                 JOIN notes_comment_threads t ON t.id = c.thread_id
                 WHERE t.page_id = ? AND c.id = ?"
            }
        }
    }
}

/// One concurrent version of a field.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesConflictVersion {
    /// Opaque version id for resolving.
    version: String,
    device: DeviceRef,
    /// When the version was written, in Unix milliseconds.
    edited_at_ms: u64,
    /// Whether this version is the one every device shows.
    displayed: bool,
    /// Plain text of the version: the title, block or comment text, value text, or thread status.
    text: String,
}

/// The versions of one field in conflict.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesFieldConflict {
    target: NotesConflictTarget,
    id: String,
    versions: Vec<NotesConflictVersion>,
}

/// The conflicts of a page; empty when it has none.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesConflictRead {
    page_id: String,
    conflicts: Vec<NotesFieldConflict>,
}

/// How to resolve a field in conflict.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotesConflictChoice {
    /// Keep the displayed version.
    Displayed,
    /// Use another version.
    Version { version: String },
}

/// A conflict resolution request. The field must belong to `page_id`, which also keys property
/// values with the property id.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesConflictResolution {
    pub page_id: String,
    pub target: NotesConflictTarget,
    pub id: String,
    pub choice: NotesConflictChoice,
}

fn text_field(value: &SyncValue, index: usize) -> Option<&str> {
    match value.fields().get(index) {
        Some(Field::Text(text)) => Some(text),
        _ => None,
    }
}

/// Plain text of rich text JSON: the `plain_text` of each run, in order, with nested arrays such
/// as table cells separated by tabs.
fn rich_text_plain_text(value: &Value) -> String {
    match value {
        Value::Array(items) if items.iter().all(Value::is_array) => items
            .iter()
            .map(rich_text_plain_text)
            .collect::<Vec<_>>()
            .join("\t"),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("plain_text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

/// Plain text of a block's stored text group, a JSON object of rich text arrays.
pub(crate) fn block_text_plain_text(text: &str) -> String {
    let Ok(Value::Object(parts)) = serde_json::from_str::<Value>(text) else {
        return String::new();
    };
    parts
        .values()
        .map(rich_text_plain_text)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plain text of a stored property value: strings, numbers, and booleans it holds, and the
/// names or plain text of the objects it holds, separated by commas.
pub(crate) fn property_value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Array(items) => items
            .iter()
            .map(property_value_text)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(object) => ["plain_text", "name", "start", "url", "email", "id"]
            .iter()
            .find_map(|key| object.get(*key).filter(|inner| !inner.is_null()))
            .map(property_value_text)
            .unwrap_or_default(),
        Value::Null => String::new(),
    }
}

/// Plain text of a version value for the given target.
fn version_text(target: NotesConflictTarget, value: &SyncValue) -> Result<String, String> {
    let invalid = || "notes conflict value is invalid".to_string();
    Ok(match target {
        NotesConflictTarget::PageTitle | NotesConflictTarget::ThreadStatus => {
            text_field(value, 0).ok_or_else(invalid)?.to_string()
        }
        NotesConflictTarget::BlockText => {
            block_text_plain_text(text_field(value, 0).ok_or_else(invalid)?)
        }
        NotesConflictTarget::PropertyValue => {
            let stored = text_field(value, 1).ok_or_else(invalid)?;
            serde_json::from_str::<Value>(stored)
                .map(|parsed| property_value_text(&parsed))
                .map_err(|_| invalid())?
        }
        NotesConflictTarget::CommentBody => text_field(value, 1).ok_or_else(invalid)?.to_string(),
    })
}

fn version_read(
    target: NotesConflictTarget,
    version: &ConflictVersion,
    devices: &DeviceNames,
) -> Result<NotesConflictVersion, String> {
    Ok(NotesConflictVersion {
        version: version.writer.to_hex(),
        device: devices.describe(&version.device_id),
        edited_at_ms: version.clock.physical_ms(),
        displayed: version.displayed,
        text: version_text(target, &version.value)?,
    })
}

/// The sync space of the vault, or `None` for a vault without a person identity, which does
/// not sync.
async fn linked_sync_space(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
) -> Result<Option<SpaceId>, String> {
    Ok(local::space_anchor(tx)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
        .map(|_| SpaceId::personal(vault_id)))
}

async fn scope(
    tx: &mut Transaction<'_, Sqlite>,
    target: NotesConflictTarget,
    page_id: &str,
) -> Result<HashMap<String, String>, String> {
    let rows: Vec<(String, String)> = sqlx::query_as(target.scope_sql())
        .bind(page_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| format!("read notes conflict rows: {error}"))?;
    Ok(rows.into_iter().collect())
}

/// Reads the conflicts of a page. A vault that does not sync has none, so views can read them
/// unconditionally.
pub async fn conflicts_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    devices: &DeviceNames,
    page_id: &str,
) -> Result<NotesConflictRead, NotesConflictError> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin notes conflict read: {error}"))?;
    let mut conflicts = Vec::new();
    if let Some(space) = linked_sync_space(&mut tx, vault_id).await? {
        let engine = Engine::vault();
        for target in NotesConflictTarget::ALL {
            let open = engine
                .open_conflicts(&mut tx, space, target.table())
                .await
                .map_err(|error| format!("read notes conflicts: {error}"))?;
            if open.is_empty() {
                continue;
            }
            let rows = scope(&mut tx, target, page_id).await?;
            for row in open {
                let Some(id) = rows.get(&row.row_key) else {
                    continue;
                };
                for detail in row
                    .groups
                    .iter()
                    .filter(|detail| detail.group == target.group())
                {
                    let versions = detail
                        .versions
                        .iter()
                        .map(|version| version_read(target, version, devices))
                        .collect::<Result<Vec<_>, _>>()?;
                    conflicts.push(NotesFieldConflict {
                        target,
                        id: id.clone(),
                        versions,
                    });
                }
            }
        }
    }
    tx.rollback()
        .await
        .map_err(|error| format!("end notes conflict read: {error}"))?;
    Ok(NotesConflictRead {
        page_id: page_id.to_string(),
        conflicts,
    })
}

/// The sync row key of a field on the page: the property value's sync key, or the id itself.
/// A field that is gone or belongs to another page reads as resolved.
async fn row_key(
    tx: &mut Transaction<'_, Sqlite>,
    target: NotesConflictTarget,
    page_id: &str,
    id: &str,
) -> Result<String, NotesConflictError> {
    let found: Option<String> = sqlx::query_scalar(target.key_sql())
        .bind(page_id)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|error| format!("read notes conflict row key: {error}"))?;
    found.ok_or_else(|| NotesConflictError::Resolved("the field is gone".to_string()))
}

async fn open_detail(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
    target: NotesConflictTarget,
    key: &str,
) -> Result<ConflictDetail, NotesConflictError> {
    let space = linked_sync_space(tx, vault_id)
        .await?
        .ok_or_else(|| NotesConflictError::Resolved("this vault does not sync".to_string()))?;
    let details = Engine::vault()
        .conflict_details(tx, space, target.table(), key)
        .await
        .map_err(|error| format!("read notes conflicts: {error}"))?;
    details
        .into_iter()
        .find(|detail| detail.group == target.group())
        .ok_or_else(|| NotesConflictError::Resolved("the conflict is resolved".to_string()))
}

/// The stored text group a block version writes: the version itself when its parts are the
/// ones the block's current type stores, or else its plain text as the block's rich text, so a
/// version written before the block changed type stays usable.
fn fitted_block_text(current: &str, version: &str) -> Result<String, NotesConflictError> {
    let invalid = || NotesConflictError::Failed("notes conflict value is invalid".to_string());
    let Ok(Value::Object(mut stored)) = serde_json::from_str::<Value>(current) else {
        return Err(invalid());
    };
    let Ok(Value::Object(chosen)) = serde_json::from_str::<Value>(version) else {
        return Err(invalid());
    };
    if stored.keys().collect::<BTreeSet<_>>() == chosen.keys().collect::<BTreeSet<_>>() {
        return Ok(version.to_string());
    }
    if !stored.contains_key("rich_text") {
        return Err(NotesConflictError::Unfit(
            "the version does not fit the block's type".to_string(),
        ));
    }
    let plain = block_text_plain_text(version);
    let runs = if plain.is_empty() {
        Vec::new()
    } else {
        vec![rich_text(&plain)]
    };
    stored.insert("rich_text".to_string(), Value::Array(runs));
    Ok(Value::Object(stored).to_string())
}

/// Writes a chosen version through the same path as a local edit of that field, inside the
/// resolution's transaction. The caller ensures the page's project history baseline first.
pub(crate) async fn write_version_tx(
    tx: &mut Transaction<'_, Sqlite>,
    target: NotesConflictTarget,
    page_id: &str,
    id: &str,
    value: &SyncValue,
) -> Result<(), NotesConflictError> {
    let invalid = || "notes conflict value is invalid".to_string();
    match target {
        NotesConflictTarget::PageTitle => {
            let title = text_field(value, 0).ok_or_else(invalid)?;
            let update: NotePageUpdate = serde_json::from_value(json!({ "title": title }))
                .map_err(|error| format!("build notes page title update: {error}"))?;
            validate_page_update(&update)?;
            update_page_tx(tx, id, update).await?;
        }
        NotesConflictTarget::BlockText => {
            let version = text_field(value, 0).ok_or_else(invalid)?;
            let (block_type, attrs, current): (String, String, String) = sqlx::query_as(
                "SELECT type, attrs, text FROM notes_blocks WHERE id = ? AND in_trash = 0",
            )
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| format!("read notes block kind: {error}"))?
            .ok_or_else(|| NotesConflictError::Resolved("the block is gone".to_string()))?;
            let text = fitted_block_text(&current, version)?;
            let payload = compose_block_content(&block_type, &attrs, &text)?;
            let update: NoteBlockUpdate =
                serde_json::from_value(json!({ "type": block_type, block_type.as_str(): payload }))
                    .map_err(|error| format!("build notes block update: {error}"))?;
            update_block_tx(tx, id, update, true).await?;
        }
        NotesConflictTarget::PropertyValue => {
            let value_type = text_field(value, 0).ok_or_else(invalid)?;
            let stored = text_field(value, 1).ok_or_else(invalid)?;
            let stored: Value = serde_json::from_str(stored).map_err(|_| invalid())?;
            write_page_property_value(tx, page_id, id, value_type, &stored).await?;
            mark_page_dirty_tx(tx, page_id, "update_page", false).await?;
        }
        NotesConflictTarget::ThreadStatus => {
            let status = text_field(value, 0).ok_or_else(invalid)?;
            resolve_comment_thread_tx(tx, id, status == "resolved").await?;
        }
        NotesConflictTarget::CommentBody => {
            let rich_text = text_field(value, 0).ok_or_else(invalid)?;
            let attachments = text_field(value, 2).ok_or_else(invalid)?;
            let update: NoteCommentUpdate = serde_json::from_value(json!({
                "rich_text": serde_json::from_str::<Value>(rich_text).map_err(|_| invalid())?,
                "attachments": serde_json::from_str::<Value>(attachments).map_err(|_| invalid())?,
            }))
            .map_err(|error| format!("build notes comment update: {error}"))?;
            update_comment_tx(tx, id, update).await?;
        }
    }
    Ok(())
}

/// Resolves a field in conflict by keeping the displayed version or using another one.
///
/// Another version is written as a local edit, so derived state (plain text, mentions, asset
/// references, history) follows the same path as typing it. The write and the sealing of the
/// group share one transaction, so the field never holds the chosen value with its conflict
/// still open.
pub async fn resolve_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    request: NotesConflictResolution,
    now_ms: u64,
) -> Result<(), NotesConflictError> {
    let page_id = request.page_id.trim();
    require_uuid(page_id, "page_id")?;
    let id = request.id.trim();
    if id.is_empty() {
        return Err("id must not be empty".to_string().into());
    }
    let target = request.target;
    if matches!(request.choice, NotesConflictChoice::Version { .. }) {
        ensure_page_baseline_for_mutation(pool, page_id).await?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin notes conflict resolution: {error}"))?;
    let key = row_key(&mut tx, target, page_id, id).await?;
    let detail = open_detail(&mut tx, vault_id, target, &key).await?;
    if let NotesConflictChoice::Version { version } = &request.choice {
        let writer = WriterId::from_hex(version);
        let chosen = detail
            .versions
            .iter()
            .find(|candidate| Some(candidate.writer) == writer)
            .ok_or_else(|| NotesConflictError::Resolved("the version is gone".to_string()))?;
        write_version_tx(&mut tx, target, page_id, id, &chosen.value).await?;
    }
    Engine::vault()
        .force_group(&mut tx, target.table(), &key, target.group(), now_ms)
        .await
        .map_err(|error| format!("mark notes conflict resolved: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit notes conflict resolution: {error}"))?;
    Ok(())
}
