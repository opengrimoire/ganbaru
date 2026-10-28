//! Chat sync conflicts: participant names, teammate roles and instructions, channel names and
//! topics, and message edits.
//!
//! Concurrent edits from linked devices whose values differ leave one displayed value on every
//! device and the others as versions to choose from. Resolving writes the chosen version's
//! columns as a local edit and seals the group even when nothing changed, so the conflict
//! closes on every device.

use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use ganbaru_sync::manifest::vault::chat::{
    self as chat_manifest, channel_group, message_group, participant_group, teammate_group,
};
use ganbaru_sync::manifest::{GroupStorage, TableSpec};
use ganbaru_sync::{ConflictVersion, DeviceNames, DeviceRef, Engine, local};
use ganbaru_sync_contracts::{Field, GroupId, SpaceId, Value, WriterId};
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Sqlite, SqlitePool, Transaction};

use crate::chat::channels::{chat_pool, now_timestamp};

/// A Chat row kind whose edits can conflict.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatConflictTarget {
    /// A participant's display name and avatar.
    Participant,
    /// An AI teammate's role and instructions.
    Teammate,
    /// A channel's name and topic.
    Channel,
    /// A message's current revision.
    Message,
}

impl ChatConflictTarget {
    const ALL: [Self; 4] = [
        Self::Participant,
        Self::Teammate,
        Self::Channel,
        Self::Message,
    ];

    fn table(self) -> &'static TableSpec {
        match self {
            Self::Participant => &chat_manifest::PARTICIPANTS,
            Self::Teammate => &chat_manifest::TEAMMATES,
            Self::Channel => &chat_manifest::CHANNELS,
            Self::Message => &chat_manifest::MESSAGES,
        }
    }

    /// The surfaced group of the row kind.
    fn group(self) -> GroupId {
        match self {
            Self::Participant => participant_group::PROFILE,
            Self::Teammate => teammate_group::PROFILE,
            Self::Channel => channel_group::DETAILS,
            Self::Message => message_group::CONTENT,
        }
    }
}

/// One concurrent version of a conflicted Chat row.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConflictVersion {
    /// Opaque version id for resolving.
    version: String,
    device: DeviceRef,
    /// When the version was written, in Unix milliseconds.
    edited_at_ms: u64,
    /// Whether this version is the one every device shows.
    displayed: bool,
    /// Main text: a name, a role, or message Markdown.
    text: String,
    /// Secondary text: a teammate's instructions or a channel's topic.
    detail: Option<String>,
}

/// The versions of one conflicted Chat row, newest first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConflict {
    target: ChatConflictTarget,
    id: String,
    /// Where the row is, for a message its channel name, or `None` when the row names itself.
    context: Option<String>,
    versions: Vec<ChatConflictVersion>,
}

/// How to resolve a conflicted Chat row.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatConflictChoice {
    /// Keep the displayed version.
    Displayed,
    /// Use another version.
    Version { version: String },
}

/// A Chat conflict resolution request.
#[derive(Debug, Deserialize)]
pub struct ChatConflictResolution {
    target: ChatConflictTarget,
    id: String,
    choice: ChatConflictChoice,
}

fn sync_error(context: &'static str) -> impl Fn(ganbaru_sync::SyncError) -> ChatError {
    move |error| {
        eprintln!("{context}: {error}");
        ChatError::new(ChatErrorCode::Persistence, context, true)
    }
}

fn persistence(context: &'static str) -> impl Fn(sqlx::Error) -> ChatError {
    move |error| {
        eprintln!("{context}: {error}");
        ChatError::new(ChatErrorCode::Persistence, context, true)
    }
}

fn resolved_error(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, message, true)
}

/// The sync space of the vault, or `None` for a vault without a person identity, which does
/// not sync.
async fn linked_sync_space(
    tx: &mut Transaction<'_, Sqlite>,
    vault_id: &str,
) -> ChatResult<Option<SpaceId>> {
    Ok(local::space_anchor(tx)
        .await
        .map_err(sync_error("Read the Chat sync space"))?
        .map(|_| SpaceId::personal(vault_id)))
}

/// Text of field `index` of `value`, or `None` when it is null or not text.
fn text_field(value: &Value, index: usize) -> Option<String> {
    match value.fields().get(index) {
        Some(Field::Text(text)) => Some(text.clone()),
        _ => None,
    }
}

/// The current Markdown of message revision `revision_id`, or empty when it is gone.
async fn revision_markdown(
    tx: &mut Transaction<'_, Sqlite>,
    revision_id: Option<String>,
) -> ChatResult<String> {
    let Some(revision_id) = revision_id else {
        return Ok(String::new());
    };
    let markdown: Option<String> = sqlx::query_scalar(
        "SELECT normalized_markdown FROM chat_communication_message_revisions WHERE id = ?",
    )
    .bind(revision_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(persistence("Read a conflicted Chat message"))?;
    Ok(markdown.unwrap_or_default())
}

async fn version_read(
    tx: &mut Transaction<'_, Sqlite>,
    target: ChatConflictTarget,
    version: &ConflictVersion,
    devices: &DeviceNames,
) -> ChatResult<ChatConflictVersion> {
    let (text, detail) = match target {
        ChatConflictTarget::Participant => {
            (text_field(&version.value, 0).unwrap_or_default(), None)
        }
        ChatConflictTarget::Teammate | ChatConflictTarget::Channel => (
            text_field(&version.value, 0).unwrap_or_default(),
            text_field(&version.value, 1),
        ),
        ChatConflictTarget::Message => (
            revision_markdown(tx, text_field(&version.value, 0)).await?,
            None,
        ),
    };
    Ok(ChatConflictVersion {
        version: version.writer.to_hex(),
        device: devices.describe(&version.device_id),
        edited_at_ms: version.clock.physical_ms(),
        displayed: version.displayed,
        text,
        detail,
    })
}

/// The name of the channel holding message `item_id`, when it is in a channel.
async fn message_channel_name(
    tx: &mut Transaction<'_, Sqlite>,
    item_id: &str,
) -> ChatResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT c.name FROM chat_conversation_items i
         JOIN chat_channels c ON c.conversation_id = i.conversation_id
         WHERE i.id = ?",
    )
    .bind(item_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(persistence("Read a conflicted Chat message channel"))
}

/// Every open Chat conflict of the vault. A vault that does not sync has none, so views can
/// read them unconditionally.
pub(crate) async fn conflicts_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    devices: &DeviceNames,
) -> ChatResult<Vec<ChatConflict>> {
    let mut tx = pool
        .begin()
        .await
        .map_err(persistence("Begin the Chat conflict read"))?;
    let Some(space) = linked_sync_space(&mut tx, vault_id).await? else {
        return Ok(Vec::new());
    };
    let engine = Engine::vault();
    let mut conflicts = Vec::new();
    for target in ChatConflictTarget::ALL {
        let open = engine
            .open_conflicts(&mut tx, space, target.table().id)
            .await
            .map_err(sync_error("Read Chat conflicts"))?;
        for row in open {
            let Some(detail) = row
                .groups
                .into_iter()
                .find(|detail| detail.group == target.group())
            else {
                continue;
            };
            let mut versions = Vec::with_capacity(detail.versions.len());
            for version in &detail.versions {
                versions.push(version_read(&mut tx, target, version, devices).await?);
            }
            let context = match target {
                ChatConflictTarget::Message => message_channel_name(&mut tx, &row.row_key).await?,
                _ => None,
            };
            conflicts.push(ChatConflict {
                target,
                id: row.row_key,
                context,
                versions,
            });
        }
    }
    tx.rollback()
        .await
        .map_err(persistence("End the Chat conflict read"))?;
    Ok(conflicts)
}

/// Whether a group of `spec` stores `updated_at`.
fn has_updated_at(spec: &TableSpec) -> bool {
    spec.groups.iter().any(|group| match group.storage {
        GroupStorage::Columns(columns) => columns.iter().any(|column| column.name == UPDATED_AT),
        GroupStorage::Owned { .. } => false,
    })
}

const UPDATED_AT: &str = "updated_at";

/// Writes `value` to the columns of the surfaced group of `target` on row `id` as the edit
/// commands do: the row's local revision is bumped where the group bumps it, and `updated_at`
/// is set where the table keeps it.
async fn write_group_value(
    tx: &mut Transaction<'_, Sqlite>,
    target: ChatConflictTarget,
    id: &str,
    value: &Value,
) -> ChatResult<()> {
    let spec = target.table();
    let group = spec.group(target.group());
    let columns = match group.map(|group| group.storage) {
        Some(GroupStorage::Columns(columns)) if columns.len() == value.fields().len() => columns,
        _ => {
            return Err(ChatError::new(
                ChatErrorCode::Internal,
                "Chat conflict value does not match its fields",
                false,
            ));
        }
    };
    let mut assignments: Vec<String> = columns
        .iter()
        .map(|column| format!("{} = ?", column.name))
        .collect();
    if let Some(revision) = spec
        .revision_column
        .filter(|_| group.is_some_and(|group| group.bumps_revision))
    {
        assignments.push(format!("{revision} = {revision} + 1"));
    }
    let updated_at = if has_updated_at(spec) {
        assignments.push(format!("{UPDATED_AT} = ?"));
        Some(now_timestamp()?)
    } else {
        None
    };
    let sql = format!(
        "UPDATE {} SET {} WHERE {} = ?",
        spec.name,
        assignments.join(", "),
        spec.key_column
    );
    let mut query = sqlx::query(AssertSqlSafe(sql));
    for field in value.fields() {
        query = match field {
            Field::Null => query.bind(None::<String>),
            Field::Integer(integer) => query.bind(*integer),
            Field::Text(text) => query.bind(text.clone()),
            Field::Blob(bytes) => query.bind(bytes.clone()),
        };
    }
    if let Some(updated_at) = &updated_at {
        query = query.bind(updated_at.as_str().to_string());
    }
    let result = query
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(persistence("Write the chosen Chat version"))?;
    if result.rows_affected() != 1 {
        return Err(resolved_error("The conflicted Chat row was not found"));
    }
    Ok(())
}

/// Resolves a Chat conflict. Another version's value is written as a local edit, and the group
/// is sealed even when the value did not change, so the write covers every version it saw.
pub(crate) async fn resolve_from_pool(
    pool: &SqlitePool,
    vault_id: &str,
    request: ChatConflictResolution,
    now_ms: u64,
) -> ChatResult<()> {
    let target = request.target;
    let mut tx = pool
        .begin()
        .await
        .map_err(persistence("Begin the Chat conflict resolution"))?;
    let space = linked_sync_space(&mut tx, vault_id)
        .await?
        .ok_or_else(|| resolved_error("This vault does not sync"))?;
    let engine = Engine::vault();
    let detail = engine
        .conflict_details(&mut tx, space, target.table().id, &request.id)
        .await
        .map_err(sync_error("Read the Chat conflict"))?
        .into_iter()
        .find(|detail| detail.group == target.group())
        .ok_or_else(|| resolved_error("The Chat conflict is resolved"))?;
    if let ChatConflictChoice::Version { version } = &request.choice {
        let writer = WriterId::from_hex(version);
        let chosen = detail
            .versions
            .iter()
            .find(|candidate| Some(candidate.writer) == writer)
            .ok_or_else(|| resolved_error("The Chat conflict version is gone"))?;
        write_group_value(&mut tx, target, &request.id, &chosen.value).await?;
    }
    engine
        .force_group(
            &mut tx,
            target.table().id,
            &request.id,
            target.group(),
            now_ms,
        )
        .await
        .map_err(sync_error("Mark the Chat conflict resolved"))?;
    tx.commit()
        .await
        .map_err(persistence("Commit the Chat conflict resolution"))
}

fn vault_error(error: String) -> ChatError {
    eprintln!("Chat conflicts could not read the vault: {error}");
    ChatError::new(ChatErrorCode::Persistence, "Vault is unavailable", true)
}

/// The open Chat conflicts of the vault, each with the devices that wrote the versions.
#[tauri::command]
pub async fn chat_conflicts(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<Vec<ChatConflict>> {
    let vault_id = crate::vault::active_vault_id(&app).map_err(vault_error)?;
    let devices = crate::sync::devices::read(&app).map_err(vault_error)?;
    let pool = chat_pool(app, db_url).await?;
    conflicts_from_pool(&pool, &vault_id, &devices).await
}

/// Resolves a Chat conflict by keeping the displayed version or using another one.
#[tauri::command]
pub async fn chat_resolve_conflict(
    app: tauri::AppHandle,
    db_url: String,
    resolution: ChatConflictResolution,
) -> ChatResult<()> {
    let vault_id = crate::vault::active_vault_id(&app).map_err(vault_error)?;
    let now_ms = crate::sync::now_ms().map_err(vault_error)?;
    let pool = chat_pool(app, db_url).await?;
    resolve_from_pool(&pool, &vault_id, resolution, now_ms).await
}

#[cfg(test)]
mod tests;
