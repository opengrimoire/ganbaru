use crate::{db::connect_sqlite, vault};
#[cfg(desktop)]
use std::path::PathBuf;
#[cfg(desktop)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, FilePath};

#[cfg(desktop)]
pub use ganbaru_notes::assets::files::NotesFileAssetDto;
#[cfg(desktop)]
pub use ganbaru_notes::assets::files::{
    NotesImportFileReferenceDto, NotesImportFileReferenceRequest,
};

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_pick_file_asset<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    block_type: String,
) -> Result<Option<NotesFileAssetDto>, String> {
    let mut picker = app.dialog().file().set_title("Attach local file");
    if let Some((filter_name, extensions)) =
        ganbaru_notes::assets::files::picker_extensions_for_block_type(&block_type)?
    {
        picker = picker.add_filter(filter_name, extensions);
    }
    if let Some(directory) = app.path().document_dir().ok().filter(|path| path.is_dir()) {
        picker = picker.set_directory(directory);
    }
    let Some(path) = picker.blocking_pick_file().map(dialog_path).transpose()? else {
        return Ok(None);
    };
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    ganbaru_notes::assets::files::save_selected_file(&pool, &vault_root, block_type, &path)
        .await
        .map(Some)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_prepare_import_file_reference<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NotesImportFileReferenceRequest,
) -> Result<NotesImportFileReferenceDto, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    ganbaru_notes::assets::files::prepare_import_file_reference(&pool, &vault_root, request).await
}

#[tauri::command]
pub async fn notes_file_asset_data_url<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    relative_path: String,
) -> Result<String, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_vault_path(&app)?;
    ganbaru_notes::assets::files::file_asset_data_url(&pool, &vault_root, relative_path).await
}

#[cfg(desktop)]
fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|error| format!("selected path is not a local file: {error}"))
}
