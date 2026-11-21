//! Chat image import from the desktop file picker.

use super::support::{chat_pool, now_timestamp, require_workspace};
use crate::chat::attachment_commands::{
    MAX_IMAGE_BYTES, MAX_IMAGE_COUNT, attachment_io_error, vault_error,
};
use crate::chat::interaction_commands::PickChatImagesRequest;
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use crate::vault;
use ganbaru_chat::repository::attachments;
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use std::fs;
use std::path::PathBuf;
use tauri_plugin_dialog::{DialogExt, FilePath};

/// The largest total of the images one pick imports.
const MAX_PICKED_IMAGE_TOTAL_BYTES: u64 = 50 * 1024 * 1024;

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
        if total_bytes > MAX_PICKED_IMAGE_TOTAL_BYTES {
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

fn dialog_path(value: FilePath) -> ChatResult<PathBuf> {
    value
        .into_path()
        .map_err(|_| ChatError::validation("image", "Selected image is not local"))
}
