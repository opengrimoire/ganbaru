//! Commands for the table, board, gallery, list, calendar, and timeline layouts of a data source,
//! and the row property edits and button clicks made from them.

use super::{
    NoteDataSourceBoardRowMove, NoteDataSourceBoardViewDto, NoteDataSourceBoardViewUpdate,
    NoteDataSourceButtonClick, NoteDataSourceCalendarViewDto, NoteDataSourceCalendarViewUpdate,
    NoteDataSourceGalleryViewDto, NoteDataSourceGalleryViewUpdate, NoteDataSourceListViewDto,
    NoteDataSourceListViewUpdate, NoteDataSourceRowPropertyUpdate, NoteDataSourceTableViewDto,
    NoteDataSourceTableViewUpdate, NoteDataSourceTimelineViewDto, NoteDataSourceTimelineViewUpdate,
    NoteDataSourceViewWindowRequest, NotePageDto, project_history,
};
use crate::db::connect_sqlite;
use ganbaru_notes::data_sources;
use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn notes_data_source_table_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceTableViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::table::data_source_table_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_table_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceTableViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceTableViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::table::update_data_source_table_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_data_source_row_property<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    page_id: String,
    update: NoteDataSourceRowPropertyUpdate,
) -> Result<project_history::NotesMutationResultDto<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::table::update_data_source_row_property(
        &pool,
        &data_source_id,
        &page_id,
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_data_source_rows_property<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    page_ids: Vec<String>,
    update: NoteDataSourceRowPropertyUpdate,
) -> Result<project_history::NotesMutationResultDto<Vec<String>>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::table::update_data_source_rows_property(
        &pool,
        &data_source_id,
        &page_ids,
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_click_data_source_button<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    page_id: String,
    request: NoteDataSourceButtonClick,
) -> Result<project_history::NotesMutationResultDto<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        data_sources::buttons::click_data_source_button(&pool, &data_source_id, &page_id, request)
            .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_board_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceBoardViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::board::data_source_board_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_board_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceBoardViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceBoardViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::board::update_data_source_board_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_move_data_source_board_row<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    request: NoteDataSourceBoardRowMove,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceBoardViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::board::move_data_source_board_row(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        request,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_gallery_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceGalleryViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::gallery::data_source_gallery_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_gallery_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceGalleryViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceGalleryViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::gallery::update_data_source_gallery_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_list_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceListViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::list::data_source_list_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_list_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceListViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceListViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::list::update_data_source_list_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_calendar_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceCalendarViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::calendar::data_source_calendar_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_calendar_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceCalendarViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceCalendarViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::calendar::update_data_source_calendar_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_timeline_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceTimelineViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::timeline::data_source_timeline_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_timeline_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceTimelineViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceTimelineViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::timeline::update_data_source_timeline_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}
