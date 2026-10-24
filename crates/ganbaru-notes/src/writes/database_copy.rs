//! Transactional database graph copying shared by database, block, and page commands.

use super::copy_budget::CopyContext;
use super::ids::new_note_id;
use super::page_duplicates::{DuplicatePageGraph, insert_child_page_copy, plan_child_page_copy};
use crate::models::{NoteBlockRow, NoteDataSourceRow, NoteParent};
use crate::{assets, data_sources};
use serde_json::Value;
use sqlx::{AssertSqlSafe, Sqlite, Transaction};
use std::collections::{HashMap, HashSet};

pub(super) use super::copy_budget::MAX_COPY_OBJECTS;
const MAX_DATABASE_DEPTH: usize = 64;
pub(crate) const TEMPLATE_PAGE_REFERENCE: &str = "__ganbaru_template_page_id";

/// Load the canonical block named by a template's database payload inside its write transaction.
pub(crate) async fn load_reference_source(
    tx: &mut Transaction<'_, Sqlite>,
    payload: &Value,
) -> Result<NoteBlockRow, String> {
    let id = payload
        .get("database_id")
        .and_then(Value::as_str)
        .ok_or("template database has no canonical block id")?;
    crate::validation::require_uuid(id, "template database id")?;
    let block = super::block_tree::load_block_row_tx(tx, id, false).await?;
    if block.block_type != "child_database" {
        return Err("template database source not found".to_string());
    }
    Ok(block)
}

pub(crate) struct DatabaseCopy {
    source_id: String,
    pub(crate) id: String,
    pub(crate) payload: Value,
    sources: Vec<(String, String)>,
    views: Vec<(String, String)>,
    rows: Vec<DuplicatePageGraph>,
    templates: Vec<(String, String, String)>,
    identities: HashMap<String, String>,
    schema_ids: HashMap<String, HashMap<String, String>>,
    template_block_ids: HashMap<String, HashMap<String, String>>,
}

/// Capture all row graphs before inserting into a destination that may be inside the source.
pub(crate) async fn plan_database_copy(
    tx: &mut Transaction<'_, Sqlite>,
    block: &NoteBlockRow,
    id: &str,
    context: &mut CopyContext<'_>,
    destination_project_id: Option<&str>,
    include_trashed: bool,
) -> Result<DatabaseCopy, String> {
    context.budget.database(tx, &block.id).await?;
    let marker = format!("database:{}", block.id);
    if context
        .reserved_ids
        .iter()
        .filter(|id| id.starts_with("database:"))
        .count()
        >= MAX_DATABASE_DEPTH
        || !context.reserved_ids.insert(marker.clone())
    {
        return Err("database copy contains a cycle or exceeds the nesting limit".to_string());
    }
    let mut payload: Value = serde_json::from_str(&block.payload)
        .map_err(|e| format!("parse copied database payload: {e}"))?;
    let selected_source = payload
        .get("data_source_id")
        .and_then(Value::as_str)
        .ok_or("database block has no data source")?
        .to_string();
    let selected_view = payload
        .get("view_id")
        .and_then(Value::as_str)
        .ok_or("database block has no view")?
        .to_string();
    let source_sizes = sqlx::query_as::<_, (String, i64)>(concat!(
        "SELECT DISTINCT source.id,
            length(CAST(source.title AS BLOB)) + length(CAST(source.title_rich_text AS BLOB))
                + length(CAST(source.description AS BLOB)) + coalesce(length(CAST(source.icon AS BLOB)), 0)
                + length(CAST(",
        crate::note_data_source_properties_json!("source"),
        " AS BLOB))
                + coalesce(length(CAST(source.source_provider AS BLOB)), 0)
                + coalesce(length(CAST(source.source_object_id AS BLOB)), 0)
                + coalesce(length(CAST(source.source_workspace_id AS BLOB)), 0)
                + coalesce(length(CAST(source.source_last_edited_time AS BLOB)), 0)
         FROM notes_data_sources AS source
         JOIN notes_databases AS owner ON owner.id = source.database_id
         JOIN notes_blocks AS owner_block ON owner_block.id = owner.id
         WHERE ((source.in_trash = 0 AND owner.in_trash = 0) OR
             (? AND source.in_trash = 1 AND owner.in_trash = 1
              AND owner_block.trash_owner = (SELECT trash_owner FROM notes_blocks WHERE id = ?)))
         AND (source.database_id = ? OR source.id IN (
             SELECT data_source_id FROM notes_database_views WHERE database_id = ?))
         ORDER BY source.id LIMIT ?",
    ))
    .bind(include_trashed)
    .bind(&block.id)
    .bind(&block.id)
    .bind(&block.id)
    .bind((MAX_COPY_OBJECTS + 1) as i64)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load copied database sources: {e}"))?;
    if source_sizes.is_empty() || source_sizes.len() > MAX_COPY_OBJECTS {
        return Err("database sources are missing or exceed the copy limit".to_string());
    }
    let source_bytes = source_sizes.iter().try_fold(0_i64, |total, (_, bytes)| {
        total
            .checked_add(*bytes)
            .ok_or("Notes copy byte count overflow")
    })?;
    context
        .budget
        .charge(source_sizes.len() as i64, source_bytes)?;
    let source_keys =
        serde_json::to_string(&source_sizes.iter().map(|(id, _)| id).collect::<Vec<_>>())
            .map_err(|error| format!("encode copied source identities: {error}"))?;
    let sources = sqlx::query_as::<_, NoteDataSourceRow>(concat!(
        "SELECT ",
        crate::note_data_source_row_columns!("source"),
        " FROM notes_data_sources AS source
         JOIN json_each(?) AS selected ON source.id = selected.value ORDER BY source.id",
    ))
    .bind(source_keys)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("load admitted database sources: {error}"))?;
    let mut identities = HashMap::from([(block.id.clone(), id.to_string())]);
    let mut schema_ids = HashMap::new();
    let mut template_block_ids = HashMap::new();
    let mut source_ids = Vec::new();
    for source in &sources {
        let new_id = new_note_id(tx, context.reserved_ids).await?;
        identities.insert(source.id.clone(), new_id.clone());
        source_ids.push((source.id.clone(), new_id));
        let schema: Value = serde_json::from_str(&source.properties)
            .map_err(|e| format!("parse copied database schema: {e}"))?;
        let mut property_ids = HashMap::new();
        for property in schema
            .as_object()
            .ok_or("database schema must be an object")?
            .values()
        {
            if let Some(property_id) = property.get("id").and_then(Value::as_str) {
                if property_id != "title" {
                    property_ids.insert(
                        property_id.to_string(),
                        new_note_id(tx, context.reserved_ids).await?,
                    );
                }
            }
            for kind in ["select", "multi_select", "status"] {
                if let Some(options) = property
                    .get(kind)
                    .and_then(|config| config.get("options"))
                    .and_then(Value::as_array)
                {
                    for option in options {
                        if let Some(option_id) = option.get("id").and_then(Value::as_str) {
                            property_ids.insert(
                                option_id.to_string(),
                                new_note_id(tx, context.reserved_ids).await?,
                            );
                        }
                    }
                }
            }
        }
        schema_ids.insert(source.id.clone(), property_ids);
    }
    let source_views: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM notes_database_views WHERE database_id = ? ORDER BY sort_order, id LIMIT ?",
    )
    .bind(&block.id)
    .bind((MAX_COPY_OBJECTS + 1) as i64)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load copied database views: {e}"))?;
    let mut views = Vec::new();
    for source in source_views {
        let new_id = new_note_id(tx, context.reserved_ids).await?;
        identities.insert(source.clone(), new_id.clone());
        views.push((source, new_id));
    }
    let mut rows = Vec::new();
    let mut templates = Vec::new();
    for (source, new_source) in &source_ids {
        let page_ids: Vec<String> = sqlx::query_scalar(
            "SELECT page.id FROM notes_pages AS page
             JOIN notes_data_sources AS source ON source.id = page.parent_data_source_id
             JOIN notes_blocks AS owner ON owner.id = source.database_id
             WHERE page.parent_data_source_id = ? AND (page.in_trash = 0 OR
                 (? AND source.in_trash = 1 AND page.in_trash = 1
                  AND page.trash_owner = owner.trash_owner))
             ORDER BY page.id LIMIT ?",
        )
        .bind(source)
        .bind(include_trashed)
        .bind((MAX_COPY_OBJECTS + 1) as i64)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| format!("load copied database rows: {e}"))?;
        for page_id in page_ids {
            let new_page = new_note_id(tx, context.reserved_ids).await?;
            let graph = Box::pin(plan_child_page_copy(
                tx,
                &page_id,
                &new_page,
                NoteParent::DataSourceId {
                    data_source_id: new_source.clone(),
                },
                include_trashed,
                context,
                destination_project_id,
            ))
            .await?;
            graph.extend_identities(&mut identities);
            rows.push(graph);
        }
        let template_ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM notes_data_source_templates WHERE data_source_id = ? ORDER BY id LIMIT ?",
        ).bind(source).bind((MAX_COPY_OBJECTS + 1) as i64).fetch_all(&mut **tx).await
            .map_err(|e| format!("load copied database templates: {e}"))?;
        for template in template_ids {
            context.budget.template(tx, &template).await?;
            let new_template = new_note_id(tx, context.reserved_ids).await?;
            identities.insert(template.clone(), new_template.clone());
            let blocks: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM notes_data_source_template_blocks WHERE template_id = ? LIMIT ?",
            )
            .bind(&template)
            .bind((MAX_COPY_OBJECTS + 1) as i64)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| format!("load copied template blocks: {e}"))?;
            let mut block_ids = HashMap::new();
            for block_id in blocks {
                block_ids.insert(block_id, new_note_id(tx, context.reserved_ids).await?);
            }
            template_block_ids.insert(template.clone(), block_ids);
            templates.push((template, new_template, new_source.clone()));
        }
    }
    payload["database_id"] = id.into();
    payload["data_source_id"] = identities
        .get(&selected_source)
        .ok_or("selected source was not copied")?
        .clone()
        .into();
    payload["view_id"] = identities
        .get(&selected_view)
        .ok_or("selected view was not copied")?
        .clone()
        .into();
    strip_trash_metadata(&mut payload);
    context.reserved_ids.remove(&marker);
    Ok(DatabaseCopy {
        source_id: block.id.clone(),
        id: id.to_string(),
        payload,
        sources: source_ids,
        views,
        rows,
        templates,
        identities,
        schema_ids,
        template_block_ids,
    })
}

/// Insert independent sources, views, rows, and templates after the destination block exists.
pub(crate) async fn insert_database_copy(
    tx: &mut Transaction<'_, Sqlite>,
    copy: &DatabaseCopy,
) -> Result<(), String> {
    sqlx::query("INSERT INTO notes_databases (id, block_id, parent_type, parent_page_id, parent_block_id, title, title_rich_text, description, icon, cover, is_inline)
        SELECT block.id, block.id, block.parent_type, block.parent_page_id, block.parent_block_id, source.title, source.title_rich_text, source.description, source.icon, source.cover, source.is_inline
        FROM notes_databases AS source JOIN notes_blocks AS block ON block.id = ? WHERE source.id = ?")
        .bind(&copy.id).bind(&copy.source_id).execute(&mut **tx).await
        .map_err(|e| format!("copy Notes database: {e}"))?;
    for (source, id) in &copy.sources {
        sqlx::query("INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description, icon)
            SELECT ?, ?, title, title_rich_text, description, icon FROM notes_data_sources WHERE id = ?")
            .bind(id).bind(&copy.id).bind(source).execute(&mut **tx).await
            .map_err(|e| format!("copy Notes database source: {e}"))?;
        crate::property_storage::copy_data_source_properties(tx, source, id).await?;
    }
    for (source, id) in &copy.views {
        sqlx::query("INSERT INTO notes_database_views (id, database_id, data_source_id, name, type, filter, sorts, configuration, sort_order)
            SELECT ?, ?, ?, name, type, filter, sorts, configuration, sort_order FROM notes_database_views WHERE id = ?")
            .bind(id).bind(&copy.id)
            .bind(sqlx::query_scalar::<_, String>("SELECT data_source_id FROM notes_database_views WHERE id = ?")
                .bind(source).fetch_one(&mut **tx).await.map_err(|e| format!("resolve copied view source: {e}"))?
                .pipe_source(&copy.identities)?)
            .bind(source).execute(&mut **tx).await.map_err(|e| format!("copy Notes database view: {e}"))?;
    }
    for graph in &copy.rows {
        Box::pin(insert_child_page_copy(tx, graph)).await?;
    }
    for (source, id, data_source) in &copy.templates {
        sqlx::query("INSERT INTO notes_data_source_templates (id, data_source_id, source_page_id, name, properties)
            SELECT ?, ?, NULL, name, properties FROM notes_data_source_templates WHERE id = ?")
            .bind(id).bind(data_source).bind(source).execute(&mut **tx).await
            .map_err(|e| format!("copy Notes database template: {e}"))?;
        sqlx::query(
            "UPDATE notes_data_sources SET default_template_id = ?1
             WHERE id = ?2
               AND EXISTS (SELECT 1 FROM notes_data_sources WHERE default_template_id = ?3)",
        )
        .bind(id)
        .bind(data_source)
        .bind(source)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("copy Notes database default template: {e}"))?;
        let source_page: Option<String> = sqlx::query_scalar(
            "SELECT source_page_id FROM notes_data_source_templates WHERE id = ?",
        )
        .bind(source)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("load template source page: {e}"))?;
        if let Some(page) = source_page
            .as_ref()
            .and_then(|page| copy.identities.get(page))
        {
            sqlx::query("UPDATE notes_data_source_templates SET source_page_id = ? WHERE id = ?")
                .bind(page)
                .bind(id)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("copy template row reference: {e}"))?;
        }
        let blocks = sqlx::query_as::<_, crate::models::NoteDataSourceTemplateBlockRow>(
            "SELECT template_id, id, parent_type, parent_block_id, has_children, type AS block_type, payload, plain_text, ordinal, created_time, last_edited_time
             FROM notes_data_source_template_blocks WHERE template_id = ? ORDER BY ordinal")
            .bind(source).fetch_all(&mut **tx).await.map_err(|e| format!("read template copy: {e}"))?;
        for block in blocks {
            let payload: Value = serde_json::from_str(&block.payload)
                .map_err(|e| format!("parse copied template: {e}"))?;
            let block_ids = &copy.template_block_ids[source];
            sqlx::query("INSERT INTO notes_data_source_template_blocks (template_id, id, parent_type, parent_block_id, has_children, type, payload, plain_text, ordinal) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(id).bind(&block_ids[&block.id]).bind(block.parent_type)
                .bind(block.parent_block_id.as_ref().and_then(|id| block_ids.get(id)))
                .bind(block.has_children).bind(block.block_type).bind(payload.to_string()).bind(block.plain_text).bind(block.ordinal)
                .execute(&mut **tx).await.map_err(|e| format!("copy template body: {e}"))?;
        }
    }
    Ok(())
}

impl DatabaseCopy {
    pub(super) fn collect_identities(
        &self,
        identities: &mut HashMap<String, String>,
        schemas: &mut HashMap<String, HashMap<String, String>>,
    ) {
        identities.extend(self.identities.clone());
        schemas.extend(self.schema_ids.clone());
        for graph in &self.rows {
            graph.collect_identities(identities, schemas);
        }
    }

    pub(super) async fn finalize(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        inherited: &HashMap<String, String>,
        schemas: &HashMap<String, HashMap<String, String>>,
        copied: &mut CopiedRelations,
    ) -> Result<(), String> {
        let mut identities = inherited.clone();
        let mut schemas = schemas.clone();
        self.collect_identities(&mut identities, &mut schemas);
        for (source, id) in &self.sources {
            let mut schema = crate::property_storage::load_data_source_properties(tx, id)
                .await?
                .ok_or_else(|| "copied data source not found".to_string())?;
            remap_schema(&mut schema, &identities, &schemas, source);
            crate::property_storage::write_data_source_properties(tx, id, &schema).await?;
            copied.sources.insert(id.clone());
        }
        for (source, id) in &self.views {
            let source_id: String =
                sqlx::query_scalar("SELECT data_source_id FROM notes_database_views WHERE id = ?")
                    .bind(source)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(|e| format!("read copied view scope: {e}"))?;
            let mut scoped = identities.clone();
            if let Some(schema) = schemas.get(&source_id) {
                scoped.extend(schema.clone());
            }
            for column in ["filter", "sorts", "configuration"] {
                remap_column(tx, "notes_database_views", column, id, &scoped).await?;
            }
        }
        for (source, id, _) in &self.templates {
            let source_id: String = sqlx::query_scalar(
                "SELECT data_source_id FROM notes_data_source_templates WHERE id = ?",
            )
            .bind(source)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("read copied template scope: {e}"))?;
            let mut scoped = identities.clone();
            if let Some(schema) = schemas.get(&source_id) {
                scoped.extend(schema.clone());
            }
            remap_column(tx, "notes_data_source_templates", "properties", id, &scoped).await?;
            scoped.extend(self.template_block_ids[source].clone());
            for (source_block_id, block_id) in &self.template_block_ids[source] {
                let (raw, block_type): (String, String) = sqlx::query_as("SELECT payload, type FROM notes_data_source_template_blocks WHERE template_id = ? AND id = ?")
                    .bind(id).bind(block_id).fetch_one(&mut **tx).await.map_err(|e| format!("read copied template body: {e}"))?;
                let mut payload: Value = serde_json::from_str(&raw)
                    .map_err(|e| format!("parse copied template body: {e}"))?;
                let database_id = payload
                    .get("database_id")
                    .and_then(Value::as_str)
                    .and_then(|id| identities.get(id))
                    .cloned();
                if block_type == "child_page" {
                    if let Some(page_id) = identities.get(source_block_id) {
                        payload[TEMPLATE_PAGE_REFERENCE] = page_id.clone().into();
                    }
                }
                remap_json_ids(&mut payload, &scoped);
                if let Some(id) = database_id {
                    payload["database_id"] = id.into();
                }
                sqlx::query("UPDATE notes_data_source_template_blocks SET payload = ? WHERE template_id = ? AND id = ?")
                    .bind(payload.to_string()).bind(id).bind(block_id).execute(&mut **tx).await
                    .map_err(|e| format!("remap copied template body: {e}"))?;
            }
        }
        for graph in &self.rows {
            Box::pin(graph.finalize(tx, &identities, &schemas, copied)).await?;
        }
        Ok(())
    }
}

/// One copied row page with its properties remapped onto the copy's identities.
pub(crate) struct CopiedRow {
    pub data_source_id: String,
    pub page_id: String,
    pub properties: Value,
}

/// What finalizing copies leaves for the closing pass: the copied data sources, whose property
/// assets are reindexed, and the copied rows, whose relation links are written once every copy
/// exists.
#[derive(Default)]
pub(crate) struct CopiedRelations {
    pub sources: HashSet<String>,
    pub rows: Vec<CopiedRow>,
}

/// Remap the entire captured closure after insertion, then rebuild relation and asset indexes.
pub(crate) async fn finalize_copies(
    tx: &mut Transaction<'_, Sqlite>,
    databases: &[DatabaseCopy],
    pages: &[DuplicatePageGraph],
    base: &HashMap<String, String>,
) -> Result<(), String> {
    let mut identities = base.clone();
    let mut schemas = HashMap::new();
    for database in databases {
        database.collect_identities(&mut identities, &mut schemas);
    }
    for page in pages {
        page.collect_identities(&mut identities, &mut schemas);
    }
    let mut copied = CopiedRelations::default();
    for database in databases {
        database
            .finalize(tx, &identities, &schemas, &mut copied)
            .await?;
    }
    for page in pages {
        page.finalize(tx, &identities, &schemas, &mut copied)
            .await?;
    }
    crate::data_sources::row_hierarchy::copy_edges_tx(tx, &identities).await?;
    write_copied_relation_links(tx, &copied.rows).await?;
    for source in &copied.sources {
        let schema = crate::property_storage::load_data_source_properties(tx, source)
            .await?
            .ok_or_else(|| "copied data source not found".to_string())?;
        assets::sync_data_source_property_asset_references_tx(tx, source, &schema).await?;
    }
    Ok(())
}

/// Links each active copied row to the related pages its source row showed that are still rows
/// of the relation's target data source. Relations to an unavailable data source are
/// left empty.
async fn write_copied_relation_links(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[CopiedRow],
) -> Result<(), String> {
    let mut schemas = HashMap::<String, Value>::new();
    for row in rows {
        let active_row: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0)",
        )
        .bind(&row.page_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("check copied relation row: {e}"))?;
        if !active_row {
            continue;
        }
        if !schemas.contains_key(&row.data_source_id) {
            let schema = live_relation_schema(tx, &row.data_source_id).await?;
            schemas.insert(row.data_source_id.clone(), schema);
        }
        let schema = &schemas[&row.data_source_id];
        let properties = related_row_values(tx, schema, &row.properties).await?;
        data_sources::relations::replace_row_relation_links_tx(
            tx,
            &row.data_source_id,
            &row.page_id,
            schema,
            &properties,
        )
        .await?;
    }
    Ok(())
}

/// The relation properties of `data_source_id` whose target data source is active.
async fn live_relation_schema(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<Value, String> {
    let schema = crate::property_storage::load_data_source_properties(tx, data_source_id)
        .await?
        .ok_or_else(|| "copied data source not found".to_string())?;
    let mut live = serde_json::Map::new();
    for (key, property) in schema
        .as_object()
        .ok_or("copied schema must be an object")?
    {
        let Some(relation) = data_sources::relations::relation_config_from_schema(property)? else {
            continue;
        };
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM notes_data_sources AS source
            JOIN notes_databases AS database ON database.id = source.database_id
            WHERE source.id = ? AND source.in_trash = 0 AND database.in_trash = 0)",
        )
        .bind(&relation.target_data_source_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("check copied relation source: {e}"))?;
        if active {
            live.insert(key.clone(), property.clone());
        }
    }
    Ok(Value::Object(live))
}

/// The relation values of `properties` under `schema`, keeping only related pages that are still
/// rows of each relation's target data source. Trashed and archived rows stay linked, as they
/// do on the source row, so restoring them restores the link on both rows.
async fn related_row_values(
    tx: &mut Transaction<'_, Sqlite>,
    schema: &Value,
    properties: &Value,
) -> Result<Value, String> {
    let mut live_values = serde_json::Map::new();
    for (key, property) in schema
        .as_object()
        .ok_or("copied schema must be an object")?
    {
        let Some(relation) = data_sources::relations::relation_config_from_schema(property)? else {
            continue;
        };
        let property_id = property
            .get("id")
            .and_then(Value::as_str)
            .ok_or("copied relation has no property id")?;
        let mut live = Vec::new();
        if let Some(value) = properties.get(key) {
            let references = data_sources::relations::canonical_relation_payload(value)?;
            for reference in references
                .as_array()
                .ok_or("copied relation must be an array")?
            {
                let page_id = reference
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("copied relation has no page id")?;
                let active: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM notes_pages WHERE id = ? AND parent_data_source_id = ?)",
                )
                .bind(page_id)
                .bind(&relation.target_data_source_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(|e| format!("check copied relation target: {e}"))?;
                if active {
                    live.push(reference.clone());
                }
            }
        }
        live_values.insert(
            key.clone(),
            data_sources::relations::relation_property_value(property_id, Value::Array(live)),
        );
    }
    Ok(Value::Object(live_values))
}

fn remap_schema(
    schema: &mut Value,
    identities: &HashMap<String, String>,
    schemas: &HashMap<String, HashMap<String, String>>,
    source: &str,
) {
    let mut scoped = identities.clone();
    if let Some(ids) = schemas.get(source) {
        scoped.extend(ids.clone());
    }
    let relation_targets: HashMap<String, String> = schema
        .as_object()
        .into_iter()
        .flat_map(|properties| properties.values())
        .filter_map(|property| {
            Some((
                property.get("id")?.as_str()?.to_string(),
                property
                    .get("relation")?
                    .get("data_source_id")?
                    .as_str()?
                    .to_string(),
            ))
        })
        .collect();
    if let Some(properties) = schema.as_object_mut() {
        for property in properties.values_mut() {
            // Target property IDs use the target schema, which can reuse a local property ID.
            let inverse = property
                .pointer("/relation/dual_property/synced_property_id")
                .and_then(Value::as_str)
                .map(|id| {
                    remapped_target_property(
                        id,
                        property
                            .pointer("/relation/data_source_id")
                            .and_then(Value::as_str),
                        schemas,
                    )
                });
            let rollup = property
                .pointer("/rollup/rollup_property_id")
                .and_then(Value::as_str)
                .map(|id| {
                    let target = property
                        .pointer("/rollup/relation_property_id")
                        .and_then(Value::as_str)
                        .and_then(|id| relation_targets.get(id));
                    remapped_target_property(id, target.map(String::as_str), schemas)
                });
            remap_json_ids(property, &scoped);
            if let Some(id) = inverse {
                property["relation"]["dual_property"]["synced_property_id"] = id.into();
            }
            if let Some(id) = rollup {
                property["rollup"]["rollup_property_id"] = id.into();
            }
        }
    }
}

fn remapped_target_property(
    id: &str,
    target: Option<&str>,
    schemas: &HashMap<String, HashMap<String, String>>,
) -> String {
    target
        .and_then(|source| schemas.get(source))
        .and_then(|ids| ids.get(id))
        .cloned()
        .unwrap_or_else(|| id.to_string())
}

/// Rewrite graph-local references while keeping references to unrelated data intact.
pub(super) fn remap_json_ids(value: &mut Value, identities: &HashMap<String, String>) {
    remap_identity_fields(value, identities, false, false);
}

fn remap_identity_fields(
    value: &mut Value,
    identities: &HashMap<String, String>,
    identity_values: bool,
    identity_keys: bool,
) {
    match value {
        Value::String(id) if identity_values => {
            if let Some(new_id) = identities.get(id) {
                *id = new_id.clone();
            }
        }
        Value::Array(values) => {
            for value in values {
                remap_identity_fields(value, identities, identity_values, false);
            }
        }
        Value::Object(values) => {
            let typed_action_value = values
                .get("property_type")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    matches!(kind, "select" | "multi_select" | "status" | "relation")
                });
            let old = std::mem::take(values);
            for (key, mut value) in old {
                let child_ids = key == "id"
                    || (key.ends_with("_id") && !key.starts_with("__ganbaru_"))
                    || key.ends_with("_ids")
                    || matches!(key.as_str(), "property_order" | "group_order" | "relation")
                    || (key == "value" && typed_action_value);
                if matches!(key.as_str(), "url" | "href") {
                    if let Some(url) = value.as_str() {
                        value = remap_local_notes_url(url, identities).into();
                    }
                } else {
                    remap_identity_fields(
                        &mut value,
                        identities,
                        child_ids,
                        matches!(key.as_str(), "column_widths" | "columns"),
                    );
                }
                let new_key = if identity_keys {
                    identities.get(&key).cloned().unwrap_or(key)
                } else {
                    key
                };
                values.insert(new_key, value);
            }
        }
        _ => {}
    }
}

fn remap_local_notes_url(url: &str, identities: &HashMap<String, String>) -> String {
    let Some((prefix, query)) = url.split_once("#notes?") else {
        return url.to_string();
    };
    let fields = query
        .split('&')
        .map(|field| {
            let Some((key, value)) = field.split_once('=') else {
                return field.to_string();
            };
            if matches!(key, "page" | "block" | "database") {
                if let Some(id) = identities.get(value) {
                    return format!("{key}={id}");
                }
            }
            field.to_string()
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{prefix}#notes?{fields}")
}

/// An independent copy never inherits its source's trash ownership or restore journal.
pub(super) fn strip_trash_metadata(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        for key in ["__ganbaru_database_trash", "__ganbaru_trash"] {
            object.remove(key);
        }
    }
}

/// Return whether a database block references a local graph; imported title-only placeholders do not.
pub(crate) fn has_database_graph(block: &NoteBlockRow) -> Result<bool, String> {
    let payload: Value = serde_json::from_str(&block.payload)
        .map_err(|e| format!("parse database copy reference: {e}"))?;
    Ok(["database_id", "data_source_id", "view_id"]
        .iter()
        .any(|key| payload.get(key).is_some_and(|value| !value.is_null())))
}

pub(super) async fn remap_column(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    column: &str,
    id: &str,
    identities: &HashMap<String, String>,
) -> Result<(), String> {
    let raw: Option<String> = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT {column} FROM {table} WHERE id = ?"
    )))
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("read copied references: {e}"))?;
    let Some(raw) = raw else {
        return Ok(());
    };
    let mut value: Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse copied references: {e}"))?;
    remap_json_ids(&mut value, identities);
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE {table} SET {column} = ? WHERE id = ?"
    )))
    .bind(value.to_string())
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("remap copied references: {e}"))?;
    Ok(())
}

trait CopiedSource {
    fn pipe_source(self, identities: &HashMap<String, String>) -> Result<String, String>;
}
impl CopiedSource for String {
    fn pipe_source(self, identities: &HashMap<String, String>) -> Result<String, String> {
        identities
            .get(&self)
            .cloned()
            .ok_or_else(|| "view source was not copied".to_string())
    }
}
