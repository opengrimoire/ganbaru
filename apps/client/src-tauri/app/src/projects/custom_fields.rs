use ganbaru_projects::custom_fields;
use ganbaru_projects::{
    ProjectCustomFieldCreate, ProjectCustomFieldOptionCreate, ProjectCustomFieldOptionUpdate,
    ProjectCustomFieldUpdate, ProjectCustomFieldValueUpdate, ProjectsMutationRows,
};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;

#[tauri::command]
pub async fn projects_create_custom_field<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    field: ProjectCustomFieldCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    custom_fields::create_custom_field(&pool, field).await
}

#[tauri::command]
pub async fn projects_update_custom_field<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    field: ProjectCustomFieldUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    custom_fields::update_custom_field(&pool, field).await
}

#[tauri::command]
pub async fn projects_delete_custom_field<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    field_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    custom_fields::delete_custom_field(&pool, field_id).await
}

#[tauri::command]
pub async fn projects_create_custom_field_option<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    option: ProjectCustomFieldOptionCreate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    custom_fields::create_custom_field_option(&pool, option).await
}

#[tauri::command]
pub async fn projects_update_custom_field_option<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    option: ProjectCustomFieldOptionUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    custom_fields::update_custom_field_option(&pool, option).await
}

#[tauri::command]
pub async fn projects_delete_custom_field_option<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    option_id: String,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    custom_fields::delete_custom_field_option(&pool, option_id).await
}

#[tauri::command]
pub async fn projects_update_custom_field_value<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    value: ProjectCustomFieldValueUpdate,
) -> Result<ProjectsMutationRows, String> {
    let pool = connect_sqlite(app, db_url).await?;
    custom_fields::update_custom_field_value(&pool, value).await
}
