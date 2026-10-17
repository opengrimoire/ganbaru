use crate::{db::connect_sqlite, vault};
#[cfg(desktop)]
use std::path::PathBuf;
#[cfg(desktop)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, FilePath};

pub use ganbaru_notes::assets::page_icons::NotePageIconAssetDto;

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_pick_page_icon_file<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Option<NotePageIconAssetDto>, String> {
    let mut picker = app
        .dialog()
        .file()
        .set_title("Upload page icon")
        .add_filter(
            "Image",
            ganbaru_notes::assets::page_icons::PAGE_ICON_ALLOWED_EXTENSIONS,
        );
    if let Some(directory) = app.path().picture_dir().ok().filter(|path| path.is_dir()) {
        picker = picker.set_directory(directory);
    }
    let Some(path) = picker.blocking_pick_file().map(dialog_path).transpose()? else {
        return Ok(None);
    };
    let original_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned);
    let bytes = ganbaru_notes::assets::page_icons::read_file_capped(&path)?;
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    ganbaru_notes::assets::page_icons::save_page_icon_bytes(
        &pool,
        &vault_root,
        bytes,
        original_name,
    )
    .await
    .map(Some)
}

#[tauri::command]
pub async fn notes_save_page_icon_data_url<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_url: String,
    original_name: Option<String>,
) -> Result<NotePageIconAssetDto, String> {
    let bytes = ganbaru_notes::assets::page_icons::decode_page_icon_data_url(&data_url)?;
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_writable_vault_path(&app)?;
    ganbaru_notes::assets::page_icons::save_page_icon_bytes(
        &pool,
        &vault_root,
        bytes,
        original_name,
    )
    .await
}

#[tauri::command]
pub async fn notes_page_icon_asset_data_url<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    relative_path: String,
) -> Result<String, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_vault_path(&app)?;
    ganbaru_notes::assets::page_icons::page_icon_asset_data_url(&pool, &vault_root, relative_path)
        .await
}

#[cfg(desktop)]
fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|error| format!("selected path is not a local file: {error}"))
}
