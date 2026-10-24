//! Row page context: one database row hydrated as its table cells show it, with its source schema and sub-items.

use crate::data_sources::layouts::table::{hydrate_rows_tx, load_active_row_page_tx, table_schema};
use crate::data_sources::row_hierarchy::{NoteDataSourceRowHierarchyMetadata, row_metadata_tx};
use crate::data_sources::views::{load_active_data_source_and_database_tx, parse_json};
use crate::models::{NoteDataSourceSchemaDto, NotePageDto, NotePageRow};
use crate::validation::require_uuid;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Everything a row page needs to show and edit its properties and sub-items.
#[derive(Serialize)]
pub struct NoteDataSourceRowPageContextDto {
    /// Source schema with the default table view, whose property order is the row page fallback order.
    pub schema: NoteDataSourceSchemaDto,
    pub row: NotePageDto,
    pub hierarchy: NoteDataSourceRowHierarchyMetadata,
    pub parent: Option<NotePageDto>,
    pub children: Vec<NotePageDto>,
    /// Whether the source's database shell protects its structure from edits.
    pub editing_locked: bool,
}

/// Load one active row page with computed values, its parent row, and its direct sub-items.
pub async fn row_page_context(
    pool: &SqlitePool,
    data_source_id: &str,
    page_id: &str,
) -> Result<NoteDataSourceRowPageContextDto, String> {
    let data_source_id = data_source_id.trim();
    let page_id = page_id.trim();
    require_uuid(data_source_id, "data_source_id")?;
    require_uuid(page_id, "page_id")?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Notes row page context: {error}"))?;
    let context = row_page_context_tx(&mut tx, data_source_id, page_id).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit Notes row page context: {error}"))?;
    Ok(context)
}

async fn row_page_context_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    page_id: &str,
) -> Result<NoteDataSourceRowPageContextDto, String> {
    let (data_source, database) =
        load_active_data_source_and_database_tx(tx, data_source_id, "row page").await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let schema = table_schema(&schema_properties)?;
    let row = load_active_row_page_tx(tx, data_source_id, page_id).await?;
    let row = hydrate_rows_tx(tx, data_source_id, &schema_properties, &schema, vec![row])
        .await?
        .pop()
        .ok_or_else(|| "row page not found".to_string())?;
    let hierarchy = row_metadata_tx(tx, data_source_id, &[page_id.to_string()])
        .await?
        .remove(page_id)
        .ok_or_else(|| "row page hierarchy not found".to_string())?;
    let parent = match hierarchy.parent_row_page_id.as_deref() {
        Some(parent_id) => Some(NotePageDto::new(
            load_active_row_page_tx(tx, data_source_id, parent_id).await?,
        )?),
        None => None,
    };
    let children = load_child_rows_tx(tx, data_source_id, page_id)
        .await?
        .into_iter()
        .map(NotePageDto::new)
        .collect::<Result<Vec<_>, _>>()?;
    let editing_locked = crate::databases::editing_lock::shell_locked_tx(tx, &database.id).await?;
    let schema =
        crate::data_sources::schema::load_data_source_schema_tx(tx, data_source_id, None, None)
            .await?;
    Ok(NoteDataSourceRowPageContextDto {
        schema,
        row: NotePageDto::new(row)?,
        hierarchy,
        parent,
        children,
        editing_locked,
    })
}

/// Read a row's direct, active sub-items in creation order.
async fn load_child_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    parent_id: &str,
) -> Result<Vec<NotePageRow>, String> {
    sqlx::query_as::<_, NotePageRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {}
         FROM notes_data_source_row_hierarchy AS hierarchy
         JOIN notes_pages AS page ON page.id = hierarchy.row_page_id
         WHERE hierarchy.data_source_id = ?
           AND hierarchy.parent_row_page_id = ?
           AND page.parent_type = 'data_source_id'
           AND page.parent_data_source_id = hierarchy.data_source_id
           AND page.in_trash = 0
           AND page.archived = 0
         ORDER BY page.created_time ASC, page.id ASC",
        crate::structure::page_row_columns("page")
    )))
    .bind(data_source_id)
    .bind(parent_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("load Notes sub-items: {error}"))
}
