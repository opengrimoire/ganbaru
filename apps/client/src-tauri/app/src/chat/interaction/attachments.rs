//! Managed Chat attachment import and read operations.

use super::support::{chat_pool, now_timestamp, require_workspace};
use crate::chat::interaction_commands::{
    ImportChatImageRequest, ImportChatTextSnippetRequest, PickChatImagesRequest,
};
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use crate::vault;
use base64::{Engine as _, engine::general_purpose};
use ganbaru_chat::repository::attachments;
use ganbaru_chat_contracts::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use std::fs;
use std::path::{Component, Path, PathBuf};
use tauri_plugin_dialog::{DialogExt, FilePath};

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
pub(crate) const MAX_IMAGE_COUNT: usize = 8;

pub(crate) async fn import_image(
    app: tauri::AppHandle,
    db_url: String,
    request: ImportChatImageRequest,
) -> ChatResult<attachments::ChatAttachmentRead> {
    if request.bytes.len() > MAX_IMAGE_BYTES {
        return Err(ChatError::validation(
            "image",
            "Chat images must be 20 MiB or smaller",
        ));
    }
    let pool = chat_pool(app.clone(), db_url).await?;
    require_workspace(
        &app,
        &pool,
        &request.working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let vault_root = vault::active_writable_vault_path(&app).map_err(vault_error)?;
    let now = now_timestamp()?;
    attachments::import_attachment_bytes(
        &pool,
        &vault_root,
        attachments::AttachmentBytesImport {
            working_folder_id: &request.working_folder_id,
            attachment_id: request.attachment_id,
            display_name: request.display_name,
            bytes: &request.bytes,
            requested_kind: attachments::ChatAttachmentKind::Image,
            now: &now,
        },
    )
    .await
}

pub(crate) async fn pick_images(
    app: tauri::AppHandle,
    db_url: String,
    request: PickChatImagesRequest,
) -> ChatResult<Vec<attachments::ChatAttachmentRead>> {
    if request.attachment_ids.is_empty() || request.attachment_ids.len() > MAX_IMAGE_COUNT {
        return Err(ChatError::validation(
            "attachmentIds",
            "Choose between one and eight Chat images",
        ));
    }
    let title = request.title.trim();
    if title.is_empty() || title.len() > 160 || title.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "title",
            "Image picker title is invalid",
        ));
    }
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    app.dialog()
        .file()
        .set_title(title)
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
        .pick_files(move |selection| {
            let result = selection
                .unwrap_or_default()
                .into_iter()
                .map(dialog_path)
                .collect::<ChatResult<Vec<_>>>();
            let _ = sender.try_send(result);
        });
    let selected = receiver.recv().await.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Image picker did not respond",
            true,
        )
    })??;
    if selected.len() > request.attachment_ids.len() {
        return Err(ChatError::validation(
            "images",
            "More images were selected than the available attachment slots",
        ));
    }
    let pool = chat_pool(app.clone(), db_url).await?;
    require_workspace(
        &app,
        &pool,
        &request.working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let vault_root = vault::active_writable_vault_path(&app).map_err(vault_error)?;
    let mut total_bytes = 0_u64;
    for path in &selected {
        let metadata = fs::symlink_metadata(path).map_err(attachment_io_error)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_IMAGE_BYTES as u64
        {
            return Err(ChatError::validation(
                "image",
                "Chat images must be 20 MiB or smaller",
            ));
        }
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| ChatError::validation("images", "Selected images are too large"))?;
        if total_bytes > 50 * 1024 * 1024 {
            return Err(ChatError::validation(
                "images",
                "Selected Chat images must total 50 MiB or less",
            ));
        }
    }
    let mut imported = Vec::with_capacity(selected.len());
    for (path, attachment_id) in selected.into_iter().zip(request.attachment_ids) {
        imported.push(
            attachments::import_attachment(
                &pool,
                &vault_root,
                &request.working_folder_id,
                attachment_id,
                &path,
                attachments::ChatAttachmentKind::Image,
                &now_timestamp()?,
            )
            .await?,
        );
    }
    Ok(imported)
}

pub(crate) async fn attachment_data_url(
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
    let metadata = fs::symlink_metadata(&path).map_err(attachment_io_error)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != attachment.byte_size
        || metadata.len() > MAX_IMAGE_BYTES as u64
    {
        return Err(attachment_io_error(()));
    }
    let bytes = fs::read(path).map_err(attachment_io_error)?;
    Ok(format!(
        "data:{};base64,{}",
        attachment.mime_type,
        general_purpose::STANDARD.encode(bytes)
    ))
}

pub(crate) async fn read_attachments(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    attachment_ids: Vec<ChatAttachmentId>,
) -> ChatResult<Vec<attachments::ChatAttachmentRead>> {
    if attachment_ids.len() > 20 {
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

pub(crate) async fn import_text_snippet(
    app: tauri::AppHandle,
    db_url: String,
    request: ImportChatTextSnippetRequest,
) -> ChatResult<attachments::ChatAttachmentRead> {
    if request.text.is_empty() || request.text.len() > 128 * 1024 || request.text.contains('\0') {
        return Err(ChatError::validation(
            "text",
            "Chat text context must be between 1 byte and 128 KiB",
        ));
    }
    let pool = chat_pool(app.clone(), db_url).await?;
    require_workspace(
        &app,
        &pool,
        &request.working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let now = now_timestamp()?;
    attachments::import_attachment_bytes(
        &pool,
        &vault::active_writable_vault_path(&app).map_err(vault_error)?,
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

fn dialog_path(value: FilePath) -> ChatResult<PathBuf> {
    value
        .into_path()
        .map_err(|_| ChatError::validation("image", "Selected image is not local"))
}

fn vault_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat attachment folder is unavailable",
        true,
    )
}

fn attachment_io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat image could not be read",
        true,
    )
}
