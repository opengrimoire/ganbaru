use crate::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId,
    UtcTimestamp,
};
use crate::repository::attachments;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::fs;
use std::io::Write;
use std::path::Path;

const MAX_BROWSER_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

pub struct StoreBrowserArtifact<'a> {
    pub resource_id: &'a str,
    pub thread_id: &'a ChatThreadId,
    pub working_folder_id: &'a ProjectWorkingFolderId,
    pub preview_tab_id: &'a str,
    pub kind: ChatResourceKind,
    pub display_name: &'a str,
    pub mime_type: &'a str,
    pub source_url: &'a str,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub duration_milliseconds: Option<u64>,
    pub frame_count: Option<u64>,
    pub created_at: &'a UtcTimestamp,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatResourceKind {
    Image,
    TextSnippet,
    BrowserScreenshot,
    BrowserRecording,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResourceRead {
    pub id: String,
    pub working_folder_id: ProjectWorkingFolderId,
    pub kind: ChatResourceKind,
    pub display_name: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub resource_uri: String,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug)]
struct StoredChatResource {
    read: ChatResourceRead,
    attachment_id: Option<ChatAttachmentId>,
    managed_relative_path: String,
    integrity_state: String,
}

pub async fn list_thread_resources(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<Vec<ChatResourceRead>> {
    let rows = sqlx::query(
        "SELECT resource.id, resource.working_folder_id, resource.resource_kind,
                resource.display_name, resource.mime_type, resource.byte_size,
                resource.sha256, resource.resource_uri, resource.created_at
         FROM chat_resource_thread_references reference
         JOIN chat_resources resource ON resource.id = reference.resource_id
         WHERE reference.thread_id = ? AND resource.integrity_state != 'deleted'
         ORDER BY resource.created_at, resource.id
         LIMIT 500",
    )
    .bind(thread_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter().map(parse_resource_read).collect()
}

pub async fn read_thread_resource_bytes(
    pool: &SqlitePool,
    vault_root: &Path,
    thread_id: &ChatThreadId,
    resource_id: &str,
) -> ChatResult<(ChatResourceRead, Vec<u8>)> {
    validate_resource_id(resource_id)?;
    let row = sqlx::query(
        "SELECT resource.id, resource.working_folder_id, resource.attachment_id,
                resource.resource_kind, resource.display_name, resource.mime_type,
                resource.byte_size, resource.sha256, resource.managed_relative_path,
                resource.resource_uri, resource.integrity_state, resource.created_at
         FROM chat_resource_thread_references reference
         JOIN chat_resources resource ON resource.id = reference.resource_id
         WHERE reference.thread_id = ? AND reference.resource_id = ?",
    )
    .bind(thread_id.as_str())
    .bind(resource_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Chat resource is not visible to this thread",
            false,
        )
    })?;
    let stored = parse_stored_resource(row)?;
    if stored.integrity_state != "verified" {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Chat resource integrity must be restored before it can be read",
            true,
        ));
    }
    let bytes = if let Some(attachment_id) = &stored.attachment_id {
        let attachment = attachments::read_attachment(pool, attachment_id)
            .await?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Managed Chat attachment was not found",
                    true,
                )
            })?;
        attachments::read_managed_attachment_bytes(vault_root, &attachment)?.1
    } else {
        read_browser_artifact_bytes(vault_root, &stored)?
    };
    if bytes.len() as u64 != stored.read.byte_size
        || format!("{:x}", Sha256::digest(&bytes)) != stored.read.sha256
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Managed Chat resource changed after import",
            true,
        ));
    }
    Ok((stored.read, bytes))
}

pub async fn store_browser_artifact(
    pool: &SqlitePool,
    vault_root: &Path,
    input: StoreBrowserArtifact<'_>,
) -> ChatResult<ChatResourceRead> {
    validate_artifact_resource_id(input.resource_id)?;
    if input.bytes.is_empty() || input.bytes.len() as u64 > MAX_BROWSER_ARTIFACT_BYTES {
        return Err(ChatError::validation(
            "artifact",
            "Browser artifact size is invalid",
        ));
    }
    let extension = match input.kind {
        ChatResourceKind::BrowserScreenshot if input.mime_type == "image/png" => "png",
        ChatResourceKind::BrowserRecording if input.mime_type == "application/zip" => "zip",
        _ => return Err(invalid_resource_error()),
    };
    let relative_path = format!(
        "assets/chat/browser-artifacts/{}.{}",
        input.resource_id, extension
    );
    let destination = attachments::resolve_managed_chat_path(vault_root, &relative_path)?;
    let parent = destination.parent().ok_or_else(invalid_resource_error)?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let temporary = destination.with_extension(format!("{extension}.tmp"));
    let write_result = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(input.bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &destination)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(write_result.err()));
    }
    let sha256 = format!("{:x}", Sha256::digest(input.bytes));
    let resource_kind = match input.kind {
        ChatResourceKind::BrowserScreenshot => "browser_screenshot",
        ChatResourceKind::BrowserRecording => "browser_recording",
        _ => unreachable!(),
    };
    let artifact_kind = match input.kind {
        ChatResourceKind::BrowserScreenshot => "screenshot",
        ChatResourceKind::BrowserRecording => "recording",
        _ => unreachable!(),
    };
    let resource_uri = format!("ganbaru://chat/resource/{}", input.resource_id);
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let result = async {
        sqlx::query(
            "INSERT INTO chat_resources
                (id, working_folder_id, attachment_id, resource_kind, display_name, mime_type,
                 byte_size, sha256, managed_relative_path, resource_uri, integrity_state, created_at)
             VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, 'verified', ?)",
        )
        .bind(input.resource_id)
        .bind(input.working_folder_id.as_str())
        .bind(resource_kind)
        .bind(input.display_name)
        .bind(input.mime_type)
        .bind(i64::try_from(input.bytes.len()).map_err(|_| invalid_resource_error())?)
        .bind(&sha256)
        .bind(&relative_path)
        .bind(&resource_uri)
        .bind(input.created_at.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "INSERT INTO chat_resource_thread_references
                (resource_id, thread_id, first_message_id, created_at)
             VALUES (?, ?, NULL, ?)",
        )
        .bind(input.resource_id)
        .bind(input.thread_id.as_str())
        .bind(input.created_at.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "INSERT INTO chat_browser_artifacts
                (resource_id, thread_id, preview_tab_id, artifact_kind, source_url,
                 viewport_width, viewport_height, duration_milliseconds, frame_count, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(input.resource_id)
        .bind(input.thread_id.as_str())
        .bind(input.preview_tab_id)
        .bind(artifact_kind)
        .bind(input.source_url)
        .bind(i64::from(input.viewport_width))
        .bind(i64::from(input.viewport_height))
        .bind(input.duration_milliseconds.and_then(|value| i64::try_from(value).ok()))
        .bind(input.frame_count.and_then(|value| i64::try_from(value).ok()))
        .bind(input.created_at.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        Ok::<(), ChatError>(())
    }
    .await;
    if let Err(error) = result {
        let _ = transaction.rollback().await;
        let _ = fs::remove_file(&destination);
        return Err(error);
    }
    if let Err(error) = transaction.commit().await {
        let _ = fs::remove_file(&destination);
        return Err(persistence_error(error));
    }
    Ok(ChatResourceRead {
        id: input.resource_id.to_string(),
        working_folder_id: input.working_folder_id.clone(),
        kind: input.kind,
        display_name: input.display_name.to_string(),
        mime_type: input.mime_type.to_string(),
        byte_size: input.bytes.len() as u64,
        sha256,
        resource_uri,
        created_at: input.created_at.clone(),
    })
}

fn read_browser_artifact_bytes(
    vault_root: &Path,
    resource: &StoredChatResource,
) -> ChatResult<Vec<u8>> {
    if !resource
        .managed_relative_path
        .starts_with("assets/chat/browser-artifacts/")
        || resource.read.byte_size > MAX_BROWSER_ARTIFACT_BYTES
    {
        return Err(invalid_resource_error());
    }
    let path = attachments::resolve_managed_chat_path(vault_root, &resource.managed_relative_path)?;
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != resource.read.byte_size
    {
        return Err(invalid_resource_error());
    }
    fs::read(path).map_err(io_error)
}

fn parse_stored_resource(row: sqlx::sqlite::SqliteRow) -> ChatResult<StoredChatResource> {
    let attachment_id = row
        .try_get::<Option<String>, _>("attachment_id")
        .map_err(persistence_error)?
        .map(ChatAttachmentId::new)
        .transpose()
        .map_err(|_| corrupt_data_error())?;
    let managed_relative_path = row
        .try_get("managed_relative_path")
        .map_err(persistence_error)?;
    let integrity_state = row.try_get("integrity_state").map_err(persistence_error)?;
    Ok(StoredChatResource {
        read: parse_resource_read(row)?,
        attachment_id,
        managed_relative_path,
        integrity_state,
    })
}

fn parse_resource_read(row: sqlx::sqlite::SqliteRow) -> ChatResult<ChatResourceRead> {
    Ok(ChatResourceRead {
        id: row.try_get("id").map_err(persistence_error)?,
        working_folder_id: ProjectWorkingFolderId::new(
            row.try_get::<String, _>("working_folder_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        kind: parse_kind(
            &row.try_get::<String, _>("resource_kind")
                .map_err(persistence_error)?,
        )?,
        display_name: row.try_get("display_name").map_err(persistence_error)?,
        mime_type: row.try_get("mime_type").map_err(persistence_error)?,
        byte_size: u64::try_from(
            row.try_get::<i64, _>("byte_size")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        sha256: row.try_get("sha256").map_err(persistence_error)?,
        resource_uri: row.try_get("resource_uri").map_err(persistence_error)?,
        created_at: UtcTimestamp::new(
            row.try_get::<String, _>("created_at")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
    })
}

fn parse_kind(value: &str) -> ChatResult<ChatResourceKind> {
    match value {
        "image" => Ok(ChatResourceKind::Image),
        "text_snippet" => Ok(ChatResourceKind::TextSnippet),
        "browser_screenshot" => Ok(ChatResourceKind::BrowserScreenshot),
        "browser_recording" => Ok(ChatResourceKind::BrowserRecording),
        _ => Err(corrupt_data_error()),
    }
}

fn validate_resource_id(value: &str) -> ChatResult<()> {
    if value.is_empty() || value.len() > 1_024 || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "resourceId",
            "Chat resource ID is invalid",
        ));
    }
    Ok(())
}

fn validate_artifact_resource_id(value: &str) -> ChatResult<()> {
    validate_resource_id(value)?;
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(ChatError::validation(
            "resourceId",
            "Browser artifact resource ID is invalid",
        ));
    }
    Ok(())
}

fn invalid_resource_error() -> ChatError {
    ChatError::validation("resource", "Managed Chat resource is invalid")
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat resource is invalid",
        false,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat resources could not be read",
        true,
    )
}

fn io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Managed Chat resource could not be read safely",
        true,
    )
}
