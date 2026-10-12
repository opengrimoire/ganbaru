use super::{
    NoteDataSourceAttach, NoteDataSourceCreate, NoteDataSourcePropertyAction,
    NoteDataSourceSchemaDto, NoteDatabaseReferenceDto, project_history,
};
use crate::db::connect_sqlite;
use tauri::{AppHandle, Runtime};

#[tauri::command]
pub async fn notes_set_database_editing_lock<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    database_id: String,
    locked: bool,
) -> Result<project_history::NotesMutationResultDto<NoteDatabaseReferenceDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = ganbaru_notes::databases::editing_lock::set_database_editing_lock(
        &pool,
        &database_id,
        locked,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_create_data_source<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteDataSourceCreate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceSchemaDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = ganbaru_notes::data_sources::management::create_data_source(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_attach_data_source<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteDataSourceAttach,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceSchemaDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = ganbaru_notes::data_sources::management::attach_data_source(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_apply_data_source_property_action<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: String,
    view_id: String,
    action: NoteDataSourcePropertyAction,
) -> Result<
    project_history::NotesMutationResultDto<
        ganbaru_notes::data_sources::property_actions::NoteDataSourcePropertyActionDto,
    >,
    String,
> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = ganbaru_notes::data_sources::property_actions::apply_property_action(
        &pool,
        &data_source_id,
        &database_id,
        &view_id,
        action,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}
