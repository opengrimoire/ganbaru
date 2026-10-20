//! Desktop dialogs for Notes exports and folder imports. Content comes from `ganbaru-notes`; this
//! module only picks the native path and hands it to the core reader or writer.

use super::{
    NoteAgentBridgeExportRequest, NoteAgentBridgeExportSaveDto, NoteDataSourceCsvExportRequest,
    NoteDataSourceCsvExportSaveDto, NoteHtmlArchiveSaveDto, NoteHtmlExportRequest,
    NoteJsonGraphExportRequest, NoteJsonGraphExportSaveDto, NoteNotionExportImportDto,
    NoteNotionExportImportRequest, on_dialog_thread,
};
use crate::vault;
use ganbaru_notes::{blocking::run_blocking, data_sources::csv_export, transfers};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, FilePath};

/// Save file dialog settings for one export format.
struct SaveTarget<'a> {
    title: &'a str,
    file_name: &'a str,
    filter_name: &'a str,
    extension: &'a str,
}

/// Ask for a destination file; `None` means the user canceled the dialog.
async fn pick_save_path<R: Runtime>(
    app: &AppHandle<R>,
    target: SaveTarget<'_>,
) -> Result<Option<PathBuf>, String> {
    let dialog = app
        .dialog()
        .file()
        .set_title(target.title)
        .set_file_name(target.file_name)
        .add_filter(target.filter_name, &[target.extension]);
    on_dialog_thread(move || dialog.blocking_save_file())
        .await?
        .map(dialog_path)
        .transpose()
}
fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|error| format!("selected path is not a local file: {error}"))
}

/// Resolve a folder picker result; `None` means the user canceled.
fn picked_folder(selection: Option<FilePath>) -> Result<Option<PathBuf>, String> {
    selection.map(dialog_path).transpose()
}

/// Ask for a Notion export folder and import it; `None` means the user canceled the picker.
///
/// The folder comes only from the native picker, so the webview cannot name a path to read.
pub(super) async fn pick_and_import_notion_export<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    request: NoteNotionExportImportRequest,
) -> Result<Option<NoteNotionExportImportDto>, String> {
    let dialog = app.dialog().file().set_title("Import Notion export folder");
    let selection = on_dialog_thread(move || dialog.blocking_pick_folder()).await?;
    let Some(export_root) = picked_folder(selection)? else {
        return Ok(None);
    };
    let vault_root = vault::active_writable_vault_path(app)?;
    let home = app.path().home_dir().ok();
    transfers::notion_export_import::import_folder(
        pool,
        &vault_root,
        &export_root,
        home.as_deref(),
        request,
    )
    .await
    .map(Some)
}

/// Export the agent bridge Markdown and write it to a user-selected file.
pub(super) async fn pick_and_write_bridge<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    request: NoteAgentBridgeExportRequest,
) -> Result<NoteAgentBridgeExportSaveDto, String> {
    let export = transfers::agent_bridge_export::export_bridge(pool, request).await?;
    let target = SaveTarget {
        title: "Export agent bridge markdown",
        file_name: &export.file_name,
        filter_name: "Markdown",
        extension: "md",
    };
    let Some(path) = pick_save_path(app, target).await? else {
        return Ok(NoteAgentBridgeExportSaveDto::canceled());
    };
    run_blocking(move || transfers::agent_bridge_export::write_bridge(&path, export)).await
}

/// Export one data source as CSV and write it to a user-selected file.
pub(super) async fn pick_and_write_csv<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    data_source_id: &str,
    request: NoteDataSourceCsvExportRequest,
) -> Result<NoteDataSourceCsvExportSaveDto, String> {
    let export = csv_export::export_csv(pool, data_source_id, request).await?;
    let target = SaveTarget {
        title: "Export Notes database CSV",
        file_name: &export.file_name,
        filter_name: "CSV",
        extension: "csv",
    };
    let Some(path) = pick_save_path(app, target).await? else {
        return Ok(NoteDataSourceCsvExportSaveDto::canceled());
    };
    run_blocking(move || csv_export::write_csv(&path, export)).await
}

/// Export the Notes JSON graph and write it to a user-selected file.
pub(super) async fn pick_and_write_graph<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    request: NoteJsonGraphExportRequest,
) -> Result<NoteJsonGraphExportSaveDto, String> {
    let export = transfers::json_graph_export::export_graph(pool, request).await?;
    let target = SaveTarget {
        title: "Export Notes JSON graph",
        file_name: &export.file_name,
        filter_name: "JSON graph",
        extension: "json",
    };
    let Some(path) = pick_save_path(app, target).await? else {
        return Ok(NoteJsonGraphExportSaveDto::canceled());
    };
    run_blocking(move || transfers::json_graph_export::write_graph(&path, export)).await
}

/// Export the HTML archive and write it, with referenced vault assets, to a user-selected file.
pub(super) async fn pick_and_write_archive<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    request: NoteHtmlExportRequest,
) -> Result<NoteHtmlArchiveSaveDto, String> {
    let mut archive = transfers::html_export::export_archive(pool, request).await?;
    let target = SaveTarget {
        title: "Export Notes HTML archive",
        file_name: &archive.default_file_name,
        filter_name: "Zip archive",
        extension: "zip",
    };
    let Some(path) = pick_save_path(app, target).await? else {
        return Ok(NoteHtmlArchiveSaveDto::canceled());
    };
    let asset_root = vault::active_vault_path(app)?.join("assets");
    run_blocking(move || {
        transfers::html_export::archive::write_archive(&asset_root, &path, &mut archive)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canceled_folder_picker_is_a_canceled_outcome() {
        assert_eq!(picked_folder(None), Ok(None));
        let chosen = std::env::temp_dir().join("Notion export");
        assert_eq!(
            picked_folder(Some(FilePath::Path(chosen.clone()))),
            Ok(Some(chosen))
        );
    }
}
