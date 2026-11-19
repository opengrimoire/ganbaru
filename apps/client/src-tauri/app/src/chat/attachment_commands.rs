//! Managed Chat attachment import and read commands shared by desktop and Android.
//!
//! Desktop authorizes the attachment folder through the device's folder binding. A phone binds no
//! folders and runs no agents, so it imports only into the effective primary working folder of a
//! project; an organizational run on a desktop accepts attachments of any folder of the project
//! whose channel posted them.

use super::channels::{chat_pool, now_timestamp};
use crate::vault;
use base64::{Engine as _, engine::general_purpose};
use ganbaru_chat::repository::attachments;
use ganbaru_chat_contracts::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use serde::Deserialize;
use sqlx::SqlitePool;
use std::path::{Component, Path, PathBuf};

/// The largest Chat image in bytes.
pub(crate) const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
/// The most images or text attachments one provider turn carries.
#[cfg(desktop)]
pub(crate) const MAX_IMAGE_COUNT: usize = 8;
/// The longest base64 text an image import accepts, checked before decoding.
const MAX_IMAGE_BASE64_CHARS: usize = MAX_IMAGE_BYTES.div_ceil(3) * 4;
/// The largest text attachment in bytes, so a pasted log fits while staying far below
/// `MAX_ATTACHMENT_BYTES`. The composer mirrors it as `MAX_TEXT_SNIPPET_BYTES` in `composer/paste.ts`.
const MAX_TEXT_SNIPPET_BYTES: usize = 1024 * 1024;
/// The most attachments one read names.
const MAX_ATTACHMENT_READS: usize = 20;

/// An image to store as a Chat attachment of a working folder, with its bytes as base64 text so
/// Android IPC carries it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportChatImageRequest {
    pub attachment_id: ChatAttachmentId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub display_name: String,
    pub base64: String,
}

/// Text too large to send inline, to store as a Chat text attachment of a working folder.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportChatTextSnippetRequest {
    pub working_folder_id: ProjectWorkingFolderId,
    pub attachment_id: ChatAttachmentId,
    pub display_name: String,
    pub text: String,
}

/// Stores an image as a Chat attachment of the request's working folder.
#[tauri::command]
pub async fn chat_import_image(
    app: tauri::AppHandle,
    db_url: String,
    request: ImportChatImageRequest,
) -> ChatResult<attachments::ChatAttachmentRead> {
    let bytes = decode_image(&request.base64)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_attachment_folder(&app, &pool, &request.working_folder_id).await?;
    let vault_root = vault::active_writable_vault_path(&app).map_err(vault_error)?;
    let now = now_timestamp()?;
    attachments::import_attachment_bytes(
        &pool,
        &vault_root,
        attachments::AttachmentBytesImport {
            working_folder_id: &request.working_folder_id,
            attachment_id: request.attachment_id,
            display_name: request.display_name,
            bytes: &bytes,
            requested_kind: attachments::ChatAttachmentKind::Image,
            now: &now,
        },
    )
    .await
}

/// Stores pasted text as a Chat text attachment of the request's working folder.
#[tauri::command]
pub async fn chat_import_text_snippet(
    app: tauri::AppHandle,
    db_url: String,
    request: ImportChatTextSnippetRequest,
) -> ChatResult<attachments::ChatAttachmentRead> {
    validate_text_snippet(&request.text)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    authorize_attachment_folder(&app, &pool, &request.working_folder_id).await?;
    let vault_root = vault::active_writable_vault_path(&app).map_err(vault_error)?;
    let now = now_timestamp()?;
    attachments::import_attachment_bytes(
        &pool,
        &vault_root,
        attachments::AttachmentBytesImport {
            working_folder_id: &request.working_folder_id,
            attachment_id: request.attachment_id,
            display_name: request.display_name,
            bytes: request.text.as_bytes(),
            requested_kind: attachments::ChatAttachmentKind::TextSnippet,
            now: &now,
        },
    )
    .await
}

/// Returns a stored Chat image as a data URL.
#[tauri::command]
pub async fn chat_attachment_data_url(
    app: tauri::AppHandle,
    db_url: String,
    attachment_id: ChatAttachmentId,
) -> ChatResult<String> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let attachment = attachments::read_attachment(&pool, &attachment_id)
        .await?
        .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Chat image was not found", true))?;
    if attachment.kind != attachments::ChatAttachmentKind::Image {
        return Err(ChatError::validation(
            "attachmentId",
            "Attachment is not an image",
        ));
    }
    let path = managed_attachment_path(&app, &attachment.managed_relative_path)?;
    let bytes = ganbaru_atomic_files::read_regular_file_capped(
        &path,
        attachment.byte_size.min(MAX_IMAGE_BYTES as u64),
        ganbaru_atomic_files::FinalSymlink::Refuse,
    )
    .map_err(attachment_io_error)?;
    if bytes.len() as u64 != attachment.byte_size {
        return Err(attachment_io_error(()));
    }
    Ok(format!(
        "data:{};base64,{}",
        attachment.mime_type,
        general_purpose::STANDARD.encode(bytes)
    ))
}

/// Reads Chat attachments of `working_folder_id`, refusing one of another folder.
#[tauri::command]
pub async fn chat_read_attachments(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    attachment_ids: Vec<ChatAttachmentId>,
) -> ChatResult<Vec<attachments::ChatAttachmentRead>> {
    if attachment_ids.len() > MAX_ATTACHMENT_READS {
        return Err(ChatError::validation(
            "attachmentIds",
            "Too many Chat attachments were requested",
        ));
    }
    let pool = chat_pool(app, db_url).await?;
    let mut result = Vec::with_capacity(attachment_ids.len());
    for id in attachment_ids {
        let attachment = attachments::read_attachment(&pool, &id)
            .await?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Chat attachment was not found",
                    true,
                )
            })?;
        if attachment.working_folder_id != working_folder_id {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Chat attachment belongs to another workspace",
                false,
            ));
        }
        result.push(attachment);
    }
    Ok(result)
}

/// Requires the device to hold the working folder binding, for reading files into the folder.
#[cfg(desktop)]
async fn authorize_attachment_folder(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    crate::chat::workspace::commands::authorize_working_folder(
        app,
        pool,
        working_folder_id,
        crate::chat::workspace::WorkingFolderAuthorizationOperation::FileRead,
    )
    .await
    .map(|_| ())
}

/// Requires `working_folder_id` to be the effective primary working folder of its project.
#[cfg(mobile)]
async fn authorize_attachment_folder(
    _app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    if is_effective_primary_folder(pool, working_folder_id).await? {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "Attach files to the project's primary working folder on this device",
            false,
        ))
    }
}

/// Returns whether `working_folder_id` is an active folder and the effective primary working
/// folder of its project.
#[cfg_attr(desktop, allow(dead_code))]
async fn is_effective_primary_folder(
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<bool> {
    let project_id: Option<String> = sqlx::query_scalar(
        "SELECT project_id FROM project_working_folders WHERE id = ? AND archived_at IS NULL",
    )
    .bind(working_folder_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "read the attachment working folder",
            true,
        )
    })?;
    let Some(project_id) = project_id else {
        return Ok(false);
    };
    let primary = ganbaru_chat::repository::primary_folder::resolve_primary_working_folder_id(
        pool,
        &project_id,
    )
    .await?;
    Ok(primary.as_ref() == Some(working_folder_id))
}

/// Decodes the base64 text of an image import, refusing text whose image would exceed
/// `MAX_IMAGE_BYTES` before decoding it.
fn decode_image(encoded: &str) -> ChatResult<Vec<u8>> {
    let too_large = || ChatError::validation("image", "Chat images must be 20 MiB or smaller");
    if encoded.len() > MAX_IMAGE_BASE64_CHARS {
        return Err(too_large());
    }
    let bytes = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| ChatError::validation("image", "Chat image bytes are invalid"))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Refuses text attachment content that is empty, larger than `MAX_TEXT_SNIPPET_BYTES`, or holds a
/// NUL character. Oversized text is refused whole, never truncated.
fn validate_text_snippet(text: &str) -> ChatResult<()> {
    if text.is_empty() || text.len() > MAX_TEXT_SNIPPET_BYTES || text.contains('\0') {
        return Err(ChatError::validation(
            "text",
            "Chat text context must be between 1 byte and 1 MiB",
        ));
    }
    Ok(())
}

fn managed_attachment_path(app: &tauri::AppHandle, relative_path: &str) -> ChatResult<PathBuf> {
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || !relative_path.starts_with("assets/chat/attachments/")
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(ChatError::validation(
            "attachmentPath",
            "Managed Chat attachment path is invalid",
        ));
    }
    Ok(vault::active_vault_path(app)
        .map_err(vault_error)?
        .join(relative))
}

pub(crate) fn vault_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat attachment folder is unavailable",
        true,
    )
}

pub(crate) fn attachment_io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat image could not be read",
        true,
    )
}

#[cfg(test)]
mod tests;
