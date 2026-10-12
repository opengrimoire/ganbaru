use crate::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, InteractionMode,
    ProjectWorkingFolderId, ProviderInstanceId, SafetyMode, UtcTimestamp, VersionedJson,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDraftWrite {
    pub id: String,
    pub working_folder_id: ProjectWorkingFolderId,
    pub thread_id: Option<ChatThreadId>,
    pub text: String,
    pub rich_content: Option<VersionedJson>,
    pub attachment_ids: Vec<ChatAttachmentId>,
    pub mentions: VersionedJson,
    pub provider_instance_id: Option<ProviderInstanceId>,
    pub model_selection: Option<VersionedJson>,
    pub safety_mode: Option<SafetyMode>,
    pub interaction_mode: Option<InteractionMode>,
    pub sent_snapshot: Option<VersionedJson>,
    pub updated_at: UtcTimestamp,
}

pub type ChatDraftRead = ChatDraftWrite;

pub async fn save_draft(pool: &SqlitePool, draft: &ChatDraftWrite) -> ChatResult<ChatDraftRead> {
    validate_draft(draft)?;
    let mentions = json_text(&draft.mentions.value)?;
    let rich_content = versioned_parts(draft.rich_content.as_ref())?;
    let model = versioned_parts(draft.model_selection.as_ref())?;
    let sent = versioned_parts(draft.sent_snapshot.as_ref())?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    validate_draft_owner(&mut transaction, draft).await?;
    sqlx::query(
        "INSERT INTO chat_drafts
            (id, working_folder_id, thread_id, text, rich_content_schema_version,
             rich_content_data, mentions_schema_version, mentions_data,
             provider_instance_id, model_selection_schema_version, model_selection_data,
             safety_mode, interaction_mode, sent_snapshot_schema_version,
             sent_snapshot_data, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
             working_folder_id = excluded.working_folder_id, thread_id = excluded.thread_id,
             text = excluded.text,
             rich_content_schema_version = excluded.rich_content_schema_version,
             rich_content_data = excluded.rich_content_data,
             mentions_schema_version = excluded.mentions_schema_version,
             mentions_data = excluded.mentions_data,
             provider_instance_id = excluded.provider_instance_id,
             model_selection_schema_version = excluded.model_selection_schema_version,
             model_selection_data = excluded.model_selection_data,
             safety_mode = excluded.safety_mode, interaction_mode = excluded.interaction_mode,
             sent_snapshot_schema_version = excluded.sent_snapshot_schema_version,
             sent_snapshot_data = excluded.sent_snapshot_data, updated_at = excluded.updated_at",
    )
    .bind(&draft.id)
    .bind(draft.working_folder_id.as_str())
    .bind(draft.thread_id.as_ref().map(ChatThreadId::as_str))
    .bind(&draft.text)
    .bind(rich_content.0)
    .bind(rich_content.1)
    .bind(i64::from(draft.mentions.schema_version))
    .bind(mentions)
    .bind(
        draft
            .provider_instance_id
            .as_ref()
            .map(ProviderInstanceId::as_str),
    )
    .bind(model.0)
    .bind(model.1)
    .bind(draft.safety_mode.map(wire_safety))
    .bind(draft.interaction_mode.map(wire_interaction))
    .bind(sent.0)
    .bind(sent.1)
    .bind(draft.updated_at.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query("DELETE FROM chat_attachment_references WHERE draft_id = ?")
        .bind(&draft.id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    for (index, attachment_id) in draft.attachment_ids.iter().enumerate() {
        let workspace: Option<String> = sqlx::query_scalar(
            "SELECT working_folder_id FROM chat_attachments WHERE id = ? AND deletion_state = 'active'",
        )
        .bind(attachment_id.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        if workspace.as_deref() != Some(draft.working_folder_id.as_str()) {
            return Err(ChatError::validation(
                "attachmentIds",
                "Draft attachment does not belong to the active project working folder",
            ));
        }
        sqlx::query(
            "INSERT INTO chat_attachment_references
                (id, attachment_id, draft_id, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(format!("draft:{}:{index}", draft.id))
        .bind(attachment_id.as_str())
        .bind(&draft.id)
        .bind(draft.updated_at.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    read_draft(pool, &draft.id)
        .await?
        .ok_or_else(corrupt_data_error)
}

pub async fn read_draft(pool: &SqlitePool, id: &str) -> ChatResult<Option<ChatDraftRead>> {
    let row = sqlx::query(
        "SELECT id, working_folder_id, thread_id, text, rich_content_schema_version,
                rich_content_data, mentions_schema_version, mentions_data,
                provider_instance_id, model_selection_schema_version, model_selection_data,
                safety_mode, interaction_mode, sent_snapshot_schema_version,
                sent_snapshot_data, updated_at
         FROM chat_drafts WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else { return Ok(None) };
    let attachment_rows = sqlx::query(
        "SELECT attachment_id FROM chat_attachment_references
         WHERE draft_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    Ok(Some(ChatDraftRead {
        id: row.try_get("id").map_err(persistence_error)?,
        working_folder_id: ProjectWorkingFolderId::new(
            row.try_get::<String, _>("working_folder_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        thread_id: row
            .try_get::<Option<String>, _>("thread_id")
            .map_err(persistence_error)?
            .map(ChatThreadId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        text: row.try_get("text").map_err(persistence_error)?,
        rich_content: read_versioned(&row, "rich_content_schema_version", "rich_content_data")?,
        attachment_ids: attachment_rows
            .into_iter()
            .map(|row| {
                ChatAttachmentId::new(
                    row.try_get::<String, _>("attachment_id")
                        .map_err(persistence_error)?,
                )
                .map_err(|_| corrupt_data_error())
            })
            .collect::<ChatResult<_>>()?,
        mentions: read_required_versioned(&row, "mentions_schema_version", "mentions_data")?,
        provider_instance_id: row
            .try_get::<Option<String>, _>("provider_instance_id")
            .map_err(persistence_error)?
            .map(ProviderInstanceId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        model_selection: read_versioned(
            &row,
            "model_selection_schema_version",
            "model_selection_data",
        )?,
        safety_mode: row
            .try_get::<Option<String>, _>("safety_mode")
            .map_err(persistence_error)?
            .map(|value| parse_safety(&value))
            .transpose()?,
        interaction_mode: row
            .try_get::<Option<String>, _>("interaction_mode")
            .map_err(persistence_error)?
            .map(|value| parse_interaction(&value))
            .transpose()?,
        sent_snapshot: read_versioned(&row, "sent_snapshot_schema_version", "sent_snapshot_data")?,
        updated_at: UtcTimestamp::new(
            row.try_get::<String, _>("updated_at")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
    }))
}

pub async fn delete_draft(
    pool: &SqlitePool,
    id: &str,
    unreferenced_at: &UtcTimestamp,
) -> ChatResult<bool> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let attachment_ids: Vec<String> = sqlx::query_scalar(
        "SELECT attachment_id FROM chat_attachment_references WHERE draft_id = ?",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let deleted = sqlx::query("DELETE FROM chat_drafts WHERE id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?
        .rows_affected()
        == 1;
    for attachment_id in attachment_ids {
        sqlx::query(
            "UPDATE chat_attachments SET unreferenced_at = ?
             WHERE id = ?
               AND NOT EXISTS (SELECT 1 FROM chat_attachment_references WHERE attachment_id = ?)
               AND NOT EXISTS (SELECT 1 FROM chat_queued_attachment_references WHERE attachment_id = ?)
               AND NOT EXISTS (SELECT 1 FROM chat_communication_attachment_references WHERE attachment_id = ?)
               AND NOT EXISTS (SELECT 1 FROM chat_scheduled_message_attachment_references WHERE attachment_id = ?)",
        )
        .bind(unreferenced_at.as_str())
        .bind(&attachment_id)
        .bind(&attachment_id)
        .bind(&attachment_id)
        .bind(&attachment_id)
        .bind(&attachment_id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(deleted)
}

async fn validate_draft_owner(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    draft: &ChatDraftWrite,
) -> ChatResult<()> {
    if let Some(thread_id) = &draft.thread_id {
        let workspace: Option<String> =
            sqlx::query_scalar("SELECT working_folder_id FROM chat_threads WHERE id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&mut **transaction)
                .await
                .map_err(persistence_error)?;
        if workspace.as_deref() != Some(draft.working_folder_id.as_str()) {
            return Err(ChatError::validation(
                "threadId",
                "Draft thread does not belong to its workspace",
            ));
        }
    }
    Ok(())
}

fn validate_draft(draft: &ChatDraftWrite) -> ChatResult<()> {
    if draft.id.is_empty() || draft.id.len() > 1024 || draft.id.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "draft.id",
            "Chat draft ID is invalid",
        ));
    }
    if draft.text.len() > 16_777_216 || draft.attachment_ids.len() > 20 {
        return Err(ChatError::validation(
            "draft",
            "Chat draft exceeds supported limits",
        ));
    }
    if draft.rich_content.as_ref().is_some_and(|content| {
        !content.value.is_object() || content.value.to_string().len() > 67_108_864
    }) {
        return Err(ChatError::validation(
            "richContent",
            "Chat draft rich content is invalid or exceeds supported limits",
        ));
    }
    if !draft.mentions.value.is_array() {
        return Err(ChatError::validation(
            "mentions",
            "Chat draft mentions must be an array",
        ));
    }
    Ok(())
}

fn versioned_parts(value: Option<&VersionedJson>) -> ChatResult<(Option<i64>, Option<String>)> {
    value
        .map(|value| {
            Ok((
                Some(i64::from(value.schema_version)),
                Some(json_text(&value.value)?),
            ))
        })
        .unwrap_or(Ok((None, None)))
}
fn read_required_versioned(
    row: &sqlx::sqlite::SqliteRow,
    version: &str,
    data: &str,
) -> ChatResult<VersionedJson> {
    read_versioned(row, version, data)?.ok_or_else(corrupt_data_error)
}
fn read_versioned(
    row: &sqlx::sqlite::SqliteRow,
    version: &str,
    data: &str,
) -> ChatResult<Option<VersionedJson>> {
    match (
        row.try_get::<Option<i64>, _>(version)
            .map_err(persistence_error)?,
        row.try_get::<Option<String>, _>(data)
            .map_err(persistence_error)?,
    ) {
        (None, None) => Ok(None),
        (Some(version), Some(data)) => Ok(Some(VersionedJson {
            schema_version: u32::try_from(version).map_err(|_| corrupt_data_error())?,
            value: serde_json::from_str(&data).map_err(serialization_error)?,
        })),
        _ => Err(corrupt_data_error()),
    }
}
fn json_text(value: &serde_json::Value) -> ChatResult<String> {
    serde_json::to_string(value).map_err(serialization_error)
}
fn wire_safety(value: SafetyMode) -> &'static str {
    match value {
        SafetyMode::AskForApproval => "ask_for_approval",
        SafetyMode::ApproveForMe => "approve_for_me",
        SafetyMode::FullAccess => "full_access",
        SafetyMode::Custom => "custom",
    }
}
fn wire_interaction(value: InteractionMode) -> &'static str {
    match value {
        InteractionMode::Build => "build",
        InteractionMode::Plan => "plan",
    }
}
fn parse_safety(value: &str) -> ChatResult<SafetyMode> {
    match value {
        "ask_for_approval" => Ok(SafetyMode::AskForApproval),
        "approve_for_me" => Ok(SafetyMode::ApproveForMe),
        "full_access" => Ok(SafetyMode::FullAccess),
        "custom" => Ok(SafetyMode::Custom),
        _ => Err(corrupt_data_error()),
    }
}
fn parse_interaction(value: &str) -> ChatResult<InteractionMode> {
    match value {
        "build" => Ok(InteractionMode::Build),
        "plan" => Ok(InteractionMode::Plan),
        _ => Err(corrupt_data_error()),
    }
}
fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat draft persistence failed",
        true,
    )
}
fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat draft JSON is invalid",
        false,
    )
}
fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat draft is invalid",
        false,
    )
}
