use crate::db::connect_sqlite;
use ganbaru_notes::data_sources::row_hierarchy::{self, NoteDataSourceRowParentUpdate};
use ganbaru_notes::data_sources::row_page;
use ganbaru_notes::models::{NoteDataSourceRowPageCreate, NoteLoadedPage};
use tauri::{AppHandle, Runtime};

/// Create a sub-item without changing its canonical data-source ownership.
#[tauri::command]
pub async fn notes_create_data_source_subitem<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    parent_row_page_id: String,
    request: NoteDataSourceRowPageCreate,
) -> Result<super::project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        row_hierarchy::create_subitem(&pool, &data_source_id, &parent_row_page_id, request).await?;
    super::project_history::mutation_result(&pool, value).await
}

/// Persist one source-scoped parent relationship after native cycle and depth validation.
#[tauri::command]
pub async fn notes_update_data_source_row_parent<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    row_page_id: String,
    update: NoteDataSourceRowParentUpdate,
) -> Result<super::project_history::NotesMutationResultDto<()>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    row_hierarchy::set_row_parent(&pool, &data_source_id, &row_page_id, update).await?;
    super::project_history::mutation_result(&pool, ()).await
}

/// Load one row page with computed property values, its source schema, parent, and sub-items.
#[tauri::command]
pub async fn notes_get_data_source_row_page_context<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    page_id: String,
) -> Result<row_page::NoteDataSourceRowPageContextDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    row_page::row_page_context(&pool, &data_source_id, &page_id).await
}
