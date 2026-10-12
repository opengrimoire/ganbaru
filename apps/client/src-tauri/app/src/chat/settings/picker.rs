use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use std::path::PathBuf;
use tauri_plugin_dialog::{DialogExt, FilePath};

pub(crate) async fn pick_local_path(
    app: &tauri::AppHandle,
    directory: bool,
    title: &str,
) -> ChatResult<Option<String>> {
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    let picker = app.dialog().file().set_title(title);
    let callback = move |selection: Option<FilePath>| {
        let result = selection.map(dialog_path).transpose();
        let _ = sender.try_send(result);
    };
    if directory {
        picker.pick_folder(callback);
    } else {
        picker.pick_file(callback);
    }
    let selected = receiver.recv().await.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Native picker did not respond",
            true,
        )
    })??;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let metadata = std::fs::metadata(&selected).map_err(|_| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Selected provider path is missing",
            true,
        )
    })?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(ChatError::validation(
            "providerPath",
            "Selected provider path has the wrong type",
        ));
    }
    let canonical = std::fs::canonicalize(selected).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Permission,
            "Selected provider path could not be resolved",
            true,
        )
    })?;
    canonical
        .to_str()
        .map(|value| Some(value.to_string()))
        .ok_or_else(|| ChatError::validation("providerPath", "Selected path is not supported"))
}

pub(crate) fn validate_picker_title(title: &str) -> ChatResult<&str> {
    let title = title.trim();
    if title.is_empty() || title.len() > 160 || title.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "title",
            "Native picker title is invalid",
        ));
    }
    Ok(title)
}

pub(crate) fn dialog_path(path: FilePath) -> ChatResult<PathBuf> {
    path.into_path()
        .map_err(|_| ChatError::validation("providerPath", "Selected path is not local"))
}
