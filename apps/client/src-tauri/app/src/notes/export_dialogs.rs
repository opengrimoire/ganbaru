//! Desktop save dialogs for Notes exports. Export content comes from `ganbaru-notes`; this
//! module only picks the destination and hands it to the core writer.

use super::{
    NoteAgentBridgeExportRequest, NoteAgentBridgeExportSaveDto, NoteDataSourceCsvExportRequest,
    NoteDataSourceCsvExportSaveDto, NoteHtmlArchiveSaveDto, NoteHtmlExportRequest,
    NoteJsonGraphExportRequest, NoteJsonGraphExportSaveDto,
};
use crate::vault;
use ganbaru_notes::{data_sources::csv_export, transfers};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt, FilePath};

/// Save file dialog settings for one export format.
struct SaveTarget<'a> {
    title: &'a str,
    file_name: &'a str,
    filter_name: &'a str,
    extension: &'a str,
}

/// Ask for a destination file; `None` means the user canceled the dialog.
fn pick_save_path<R: Runtime>(
    app: &AppHandle<R>,
    target: SaveTarget<'_>,
) -> Result<Option<PathBuf>, String> {
    app.dialog()
        .file()
        .set_title(target.title)
        .set_file_name(target.file_name)
        .add_filter(target.filter_name, &[target.extension])
        .blocking_save_file()
        .map(dialog_path)
        .transpose()
}

fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|error| format!("selected path is not a local file: {error}"))
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
    let Some(path) = pick_save_path(app, target)? else {
        return Ok(NoteAgentBridgeExportSaveDto::canceled());
    };
    transfers::agent_bridge_export::write_bridge(&path, export)
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
    let Some(path) = pick_save_path(app, target)? else {
        return Ok(NoteDataSourceCsvExportSaveDto::canceled());
    };
    csv_export::write_csv(&path, export)
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
    let Some(path) = pick_save_path(app, target)? else {
        return Ok(NoteJsonGraphExportSaveDto::canceled());
    };
    transfers::json_graph_export::write_graph(&path, export)
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
    let Some(path) = pick_save_path(app, target)? else {
        return Ok(NoteHtmlArchiveSaveDto::canceled());
    };
    let asset_root = vault::active_vault_path(app)?.join("assets");
    transfers::html_export::archive::write_archive(&asset_root, &path, &mut archive)
}
