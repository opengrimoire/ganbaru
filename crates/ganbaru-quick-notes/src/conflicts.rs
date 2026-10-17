//! Conflicts on Quick notes titles and bodies: concurrent edits from linked devices whose
//! values differ. Every replica shows the same winner; resolving seals a write that covers
//! every version, so the conflict closes on every device.

use ganbaru_sync::domains::quick_notes::decode_runs;
use ganbaru_sync::manifest::vault::quick_notes::{NOTES_TABLE, note_group};
use ganbaru_sync::{
    ConflictDetail, ConflictVersion, DeviceNames, DeviceRef, Engine, SpaceContext, local,
};
use ganbaru_sync_contracts::{Field, GroupId, Value, WriterId};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::{
    QuickNoteRead, QuickNoteTextRun, QuickNoteWrite, body_plain_text, insert_note,
    load_note_from_pool, new_note_id, normalized_runs, replace_runs, validate_id,
};

/// Stable conflict outcomes, independent of diagnostic wording.
#[derive(Debug, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum QuickNoteConflictError {
    /// The conflict or the chosen version no longer exists; reload and show the current state.
    Resolved(String),
    Failed(String),
}

impl From<String> for QuickNoteConflictError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// A note field that can hold a conflict.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuickNoteConflictField {
    Title,
    Body,
}

impl QuickNoteConflictField {
    fn from_group(group: GroupId) -> Option<Self> {
        if group == note_group::TITLE {
            Some(Self::Title)
        } else if group == note_group::BODY {
            Some(Self::Body)
        } else {
            None
        }
    }

    fn group(self) -> GroupId {
        match self {
            Self::Title => note_group::TITLE,
            Self::Body => note_group::BODY,
        }
    }
}

/// One concurrent version of a field.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteConflictVersion {
    /// Opaque version id for resolving.
    version: String,
    device: DeviceRef,
    /// When the version was written, in Unix milliseconds.
    edited_at_ms: u64,
    /// Whether this version is the one every device shows.
    displayed: bool,
    /// Title text, for a title version.
    title: Option<String>,
    /// Body runs, for a body version.
    runs: Option<Vec<QuickNoteTextRun>>,
}

/// The versions of one field in conflict, newest first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteConflictGroup {
    field: QuickNoteConflictField,
    versions: Vec<QuickNoteConflictVersion>,
}

/// The conflicts of a note; empty when it has none.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteConflictRead {
    id: String,
    groups: Vec<QuickNoteConflictGroup>,
}

/// How to resolve a field in conflict.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuickNoteConflictChoice {
    /// Keep the displayed version.
    Displayed,
    /// Use another version.
    Version { version: String },
    /// Keep the displayed version and copy another version into a new note.
    KeepBoth { version: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteConflictResolution {
    id: String,
    field: QuickNoteConflictField,
    choice: QuickNoteConflictChoice,
}

/// The note after a resolution, and the note created to keep both versions.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteConflictResolved {
    note: QuickNoteRead,
    copy: Option<QuickNoteRead>,
}

/// Body runs of a body group value.
pub(super) fn runs_from_value(value: &Value) -> Result<Vec<QuickNoteTextRun>, String> {
    let [Field::Blob(bytes)] = value.fields() else {
        return Err("quick note body value is invalid".to_string());
    };
    let runs = decode_runs(bytes).map_err(|error| error.to_string())?;
    Ok(runs
        .into_iter()
        .map(|run| QuickNoteTextRun {
            content: run.content,
            bold: run.bold,
            italic: run.italic,
            underline: run.underline,
        })
        .collect())
}

fn title_from_value(value: &Value) -> Result<String, String> {
    match value.fields() {
        [Field::Text(title)] => Ok(title.clone()),
        _ => Err("quick note title value is invalid".to_string()),
    }
}

fn version_read(
    field: QuickNoteConflictField,
    version: &ConflictVersion,
    devices: &DeviceNames,
) -> Result<QuickNoteConflictVersion, String> {
    let (title, runs) = match field {
        QuickNoteConflictField::Title => (Some(title_from_value(&version.value)?), None),
        QuickNoteConflictField::Body => (None, Some(runs_from_value(&version.value)?)),
    };
    Ok(QuickNoteConflictVersion {
        version: version.writer.to_hex(),
        device: devices.describe(&version.device_id),
        edited_at_ms: version.clock.physical_ms(),
        displayed: version.displayed,
        title,
        runs,
    })
}

async fn space_context(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
) -> Result<SpaceContext, QuickNoteConflictError> {
    local::space_context(tx, vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
        .ok_or_else(|| QuickNoteConflictError::Resolved("this vault does not sync".to_string()))
}

async fn details(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
    id: &str,
) -> Result<Vec<ConflictDetail>, QuickNoteConflictError> {
    let ctx = space_context(tx, vault_id).await?;
    Ok(Engine::vault()
        .conflict_details(tx, &ctx, NOTES_TABLE, id)
        .await
        .map_err(|error| format!("read quick note conflicts: {error}"))?)
}

/// Reads the conflicts of a note.
pub async fn conflict_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    devices: &DeviceNames,
    id: &str,
) -> Result<QuickNoteConflictRead, QuickNoteConflictError> {
    validate_id(id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note conflict read: {error}"))?;
    let details = details(&mut tx, vault_id, id).await?;
    tx.rollback()
        .await
        .map_err(|error| format!("end quick note conflict read: {error}"))?;
    let mut groups = Vec::new();
    for detail in details {
        let Some(field) = QuickNoteConflictField::from_group(detail.group) else {
            continue;
        };
        let versions = detail
            .versions
            .iter()
            .map(|version| version_read(field, version, devices))
            .collect::<Result<Vec<_>, _>>()?;
        groups.push(QuickNoteConflictGroup { field, versions });
    }
    Ok(QuickNoteConflictRead {
        id: id.to_string(),
        groups,
    })
}

/// Writes a field value to a live note as a local edit.
async fn write_field(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    field: QuickNoteConflictField,
    value: &Value,
) -> Result<(), String> {
    let result = match field {
        QuickNoteConflictField::Title => {
            sqlx::query(
                "UPDATE quick_notes
             SET title = ?, revision = revision + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
            )
            .bind(title_from_value(value)?)
            .bind(id)
            .execute(&mut **tx)
            .await
        }
        QuickNoteConflictField::Body => {
            let runs = normalized_runs(&runs_from_value(value)?)?;
            replace_runs(tx, id, &runs).await?;
            sqlx::query(
                "UPDATE quick_notes
                 SET body_plain_text = ?, revision = revision + 1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?",
            )
            .bind(body_plain_text(&runs))
            .bind(id)
            .execute(&mut **tx)
            .await
        }
    };
    let result = result.map_err(|error| format!("write quick note conflict choice: {error}"))?;
    if result.rows_affected() != 1 {
        return Err("quick note not found".to_string());
    }
    Ok(())
}

/// Creates a note that copies `id` with one field taken from another version.
async fn insert_copy(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    field: QuickNoteConflictField,
    value: &Value,
) -> Result<String, String> {
    let (title, color, tag_id): (String, i64, Option<String>) =
        sqlx::query_as("SELECT title, color, tag_id FROM quick_notes WHERE id = ?")
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| format!("read quick note for copy: {error}"))?;
    let runs = sqlx::query_as::<_, (String, bool, bool, bool)>(
        "SELECT content, bold, italic, underline FROM quick_note_text_runs
         WHERE note_id = ? ORDER BY sort_order",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("read quick note runs for copy: {error}"))?
    .into_iter()
    .map(|(content, bold, italic, underline)| QuickNoteTextRun {
        content,
        bold,
        italic,
        underline,
    })
    .collect();
    let mut copy = QuickNoteWrite {
        id: new_note_id()?,
        title,
        runs,
        color,
        tag_id,
        pinned: false,
    };
    match field {
        QuickNoteConflictField::Title => copy.title = title_from_value(value)?,
        QuickNoteConflictField::Body => copy.runs = runs_from_value(value)?,
    }
    insert_note(tx, &copy).await?;
    Ok(copy.id)
}

/// Resolves a field in conflict. The chosen value is written as a local edit and the field is
/// sealed even when its value did not change, so the write covers every version it saw.
pub async fn resolve_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    request: QuickNoteConflictResolution,
    now_ms: u64,
) -> Result<QuickNoteConflictResolved, QuickNoteConflictError> {
    validate_id(&request.id)?;
    let group = request.field.group();
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin quick note conflict resolution: {error}"))?;
    let detail = details(&mut tx, vault_id, &request.id)
        .await?
        .into_iter()
        .find(|detail| detail.group == group)
        .ok_or_else(|| QuickNoteConflictError::Resolved("the conflict is resolved".to_string()))?;
    let chosen = |version: &str| {
        let writer = WriterId::from_hex(version);
        detail
            .versions
            .iter()
            .find(|candidate| Some(candidate.writer) == writer)
            .map(|candidate| candidate.value.clone())
            .ok_or_else(|| QuickNoteConflictError::Resolved("the version is gone".to_string()))
    };
    let mut copy_id = None;
    match &request.choice {
        QuickNoteConflictChoice::Displayed => {}
        QuickNoteConflictChoice::Version { version } => {
            write_field(&mut tx, &request.id, request.field, &chosen(version)?).await?;
        }
        QuickNoteConflictChoice::KeepBoth { version } => {
            let value = chosen(version)?;
            copy_id = Some(insert_copy(&mut tx, &request.id, request.field, &value).await?);
        }
    }
    Engine::vault()
        .force_group(&mut tx, NOTES_TABLE, &request.id, group, now_ms)
        .await
        .map_err(|error| format!("mark quick note conflict resolved: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit quick note conflict resolution: {error}"))?;
    let note = load_note_from_pool(pool, &request.id).await?;
    let copy = match copy_id {
        Some(id) => Some(load_note_from_pool(pool, &id).await?),
        None => None,
    };
    Ok(QuickNoteConflictResolved { note, copy })
}
