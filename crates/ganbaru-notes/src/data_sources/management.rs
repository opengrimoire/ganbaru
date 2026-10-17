//! Reversible source creation and attachment within an existing database shell.

use crate::models::{NoteDataSourceAttach, NoteDataSourceCreate, NoteDataSourceSchemaDto};
use crate::validation::require_uuid;
use crate::{data_sources, databases, page_history, project_history};
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};

fn validate_name(value: &str) -> Result<&str, String> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
        return Err(
            "data source and view names must contain 1 to 200 printable characters".to_string(),
        );
    }
    Ok(name)
}

async fn prepare_shell_write_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
    action: &str,
) -> Result<(), String> {
    databases::editing_lock::ensure_unlocked_tx(tx, database_id).await?;
    let page_id: String =
        sqlx::query_scalar("SELECT page_id FROM notes_blocks WHERE id = ? AND in_trash = 0")
            .bind(database_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| format!("load source management parent: {error}"))?;
    page_history::record_page_snapshot_tx(tx, &page_id, action).await?;
    project_history::mark_page_dirty_tx(tx, &page_id, "Database data sources", false).await?;
    Ok(())
}

/// Create one owned source and its initial table view in an existing unlocked shell.
pub async fn create_data_source(
    pool: &SqlitePool,
    request: NoteDataSourceCreate,
) -> Result<NoteDataSourceSchemaDto, String> {
    require_uuid(&request.id, "id")?;
    require_uuid(&request.database_id, "database_id")?;
    require_uuid(&request.view_id, "view_id")?;
    if request.id == request.database_id
        || request.id == request.view_id
        || request.view_id == request.database_id
    {
        return Err("data source, database, and view identities must differ".to_string());
    }
    let title = validate_name(&request.title)?;
    let view_name = validate_name(&request.view_name)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin data source creation: {error}"))?;
    prepare_shell_write_tx(&mut tx, &request.database_id, "create_data_source").await?;
    let properties = databases::default_data_source_properties();
    sqlx::query("INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description, properties) VALUES (?, ?, ?, ?, '[]', ?)")
        .bind(&request.id).bind(&request.database_id).bind(title).bind(databases::rich_text_array(title).to_string()).bind(properties.to_string())
        .execute(&mut *tx).await.map_err(|error| format!("create database data source: {error}"))?;
    insert_table_tx(
        &mut tx,
        &request.database_id,
        &request.id,
        &request.view_id,
        view_name,
        &properties,
    )
    .await?;
    let dto = data_sources::schema::load_data_source_schema_tx(
        &mut tx,
        &request.id,
        Some(&request.database_id),
        Some(&request.view_id),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit data source creation: {error}"))?;
    Ok(dto)
}

/// Attach a shared source through an independent table view without copying its rows.
pub async fn attach_data_source(
    pool: &SqlitePool,
    request: NoteDataSourceAttach,
) -> Result<NoteDataSourceSchemaDto, String> {
    data_sources::views::validate_view_scope(
        &request.data_source_id,
        Some(&request.database_id),
        Some(&request.view_id),
    )?;
    if request.data_source_id == request.database_id
        || request.data_source_id == request.view_id
        || request.view_id == request.database_id
    {
        return Err("data source, database, and view identities must differ".to_string());
    }
    let view_name = validate_name(&request.view_name)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin data source attachment: {error}"))?;
    prepare_shell_write_tx(&mut tx, &request.database_id, "attach_data_source").await?;
    let already_attached: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notes_database_views WHERE database_id = ? AND data_source_id = ?",
    )
    .bind(&request.database_id)
    .bind(&request.data_source_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| format!("read attached data sources: {error}"))?;
    if already_attached != 0 {
        return Err("data source is already attached to this database".to_string());
    }
    let (source, _) = data_sources::views::load_active_data_source_and_database_tx(
        &mut tx,
        &request.data_source_id,
        "source attachment",
    )
    .await?;
    let properties: Value = serde_json::from_str(&source.properties)
        .map_err(|error| format!("read attached source schema: {error}"))?;
    insert_table_tx(
        &mut tx,
        &request.database_id,
        &source.id,
        &request.view_id,
        view_name,
        &properties,
    )
    .await?;
    let dto = data_sources::schema::load_data_source_schema_tx(
        &mut tx,
        &source.id,
        Some(&request.database_id),
        Some(&request.view_id),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit data source attachment: {error}"))?;
    Ok(dto)
}

async fn insert_table_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
    source_id: &str,
    view_id: &str,
    name: &str,
    properties: &Value,
) -> Result<(), String> {
    let sort_order = data_sources::views::next_view_sort_order_tx(tx, database_id).await?;
    sqlx::query("INSERT INTO notes_database_views (id, database_id, data_source_id, name, type, sorts, configuration, sort_order) VALUES (?, ?, ?, ?, 'table', '[]', ?, ?)")
        .bind(view_id).bind(database_id).bind(source_id).bind(name).bind(data_sources::layouts::table::default_table_configuration(properties)?.to_string()).bind(sort_order)
        .execute(&mut **tx).await.map_err(|error| format!("create source table view: {error}"))?;
    Ok(())
}
