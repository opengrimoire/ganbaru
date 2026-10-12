use crate::data_sources;
use crate::data_sources::views::{
    ViewProperty, canonical_filter, canonical_sorts, generate_uuid_tx,
    load_active_data_source_and_database_tx, normalized_row_for_schema, parse_json, stored_filters,
    stored_sorts, view_schema,
};
use crate::models::{
    NoteDataSourceGalleryConfigurationUpdate, NoteDataSourceGalleryViewDto,
    NoteDataSourceGalleryViewUpdate, NoteDataSourceRow, NoteDataSourceViewWindowRequest,
    NoteDatabaseViewRow,
};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::HashSet;

const DEFAULT_GALLERY_VIEW_NAME: &str = "Gallery";
const MAX_GALLERY_CONFIGURATION_BYTES: usize = 50 * 1024;
const GALLERY_COVER_SOURCES: &[&str] = &["page_cover", "files_property", "none"];
const GALLERY_CARD_SIZES: &[&str] = &["small", "medium", "large"];
const GALLERY_ROW_OPEN_MODES: &[&str] = &["full_page", "side_panel"];

#[cfg(test)]
pub async fn data_source_gallery_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<NoteDataSourceGalleryViewDto, String> {
    data_source_gallery_view_window(
        pool,
        data_source_id,
        database_id,
        view_id,
        NoteDataSourceViewWindowRequest::default(),
    )
    .await
}

pub async fn data_source_gallery_view_window(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window: NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceGalleryViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    crate::project_history::ensure_data_source_baseline_for_mutation(pool, data_source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source gallery read: {e}"))?;
    let dto = load_gallery_view_tx(&mut tx, data_source_id, database_id, view_id, &window).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source gallery read: {e}"))?;
    Ok(dto)
}

pub async fn update_data_source_gallery_view(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    update: NoteDataSourceGalleryViewUpdate,
) -> Result<NoteDataSourceGalleryViewDto, String> {
    data_sources::views::validate_view_scope(data_source_id, database_id, view_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes data source gallery view update: {e}"))?;
    crate::project_history::mark_data_source_dirty_tx(
        &mut tx,
        data_source_id,
        "Gallery view",
        false,
    )
    .await?;
    let (data_source, _database) =
        load_active_data_source_and_database_tx(&mut tx, data_source_id, "board").await?;
    let schema = view_schema(&parse_json(
        &data_source.properties,
        "data source properties",
    )?)?;
    let property_ids: HashSet<String> = schema.iter().map(|property| property.id.clone()).collect();
    let property_types = schema
        .iter()
        .map(|property| (property.id.as_str(), property.property_type.as_str()))
        .collect();
    let filter = canonical_filter(&update.filter, &property_types, "gallery")?;
    let sorts = canonical_sorts(&update.sorts, &property_ids, "board")?;
    let configuration = canonical_gallery_configuration(&update.configuration, &schema)?;
    let view =
        ensure_gallery_view_row_tx(&mut tx, &data_source, database_id, view_id, &schema).await?;
    data_sources::views::prepare_view_mutation_tx(&mut tx, &view, "Gallery view").await?;
    sqlx::query(
        "UPDATE notes_database_views
         SET filter = ?,
             sorts = ?,
             configuration = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(filter.map(|value| value.to_string()))
    .bind(sorts.to_string())
    .bind(configuration.to_string())
    .bind(&view.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update notes data source gallery view: {e}"))?;
    let dto = load_gallery_view_tx(
        &mut tx,
        data_source_id,
        database_id,
        Some(&view.id),
        &NoteDataSourceViewWindowRequest::default(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes data source gallery view update: {e}"))?;
    Ok(dto)
}

async fn load_gallery_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
    window_request: &NoteDataSourceViewWindowRequest,
) -> Result<NoteDataSourceGalleryViewDto, String> {
    let (data_source, database) =
        load_active_data_source_and_database_tx(tx, data_source_id, "board").await?;
    let schema_properties = parse_json(&data_source.properties, "data source properties")?;
    let schema = view_schema(&schema_properties)?;
    let view = ensure_gallery_view_row_tx(tx, &data_source, database_id, view_id, &schema).await?;
    validate_gallery_configuration(view.configuration.as_deref(), &schema)?;
    let filters = stored_filters(view.filter.as_deref(), "database board filter", "board")?;
    let sorts = stored_sorts(&view.sorts, "database board sorts", "board")?;
    let window_schema = data_sources::window::table_properties_from_view(&schema);
    let mut window = data_sources::window::load_row_window_tx(
        tx,
        data_source_id,
        data_sources::window::RowWindowQuery {
            schema: &window_schema,
            filters: &filters,
            sorts: &sorts,
            request: window_request,
            date_property: None,
            group_property: None,
        },
    )
    .await?;
    window.rows = window
        .rows
        .into_iter()
        .map(|row| normalized_row_for_schema(row, &schema))
        .collect::<Result<Vec<_>, _>>()?;
    data_sources::relations::hydrate_relation_titles_tx(tx, &mut window.rows).await?;
    data_sources::rollups::hydrate_rollups_tx(
        tx,
        data_source_id,
        &schema_properties,
        &mut window.rows,
    )
    .await?;
    data_sources::formulas::hydrate_formulas(&schema_properties, &mut window.rows)?;
    data_sources::buttons::hydrate_buttons(&schema_properties, &mut window.rows)?;
    NoteDataSourceGalleryViewDto::new(data_source, database, view, window)
}

async fn ensure_gallery_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source: &NoteDataSourceRow,
    database_id: Option<&str>,
    view_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<NoteDatabaseViewRow, String> {
    if let Some(view) = load_gallery_view_row_tx(tx, &data_source.id, database_id, view_id).await? {
        return Ok(view);
    }
    let database_id = data_sources::views::scoped_database_id(data_source, database_id);
    crate::databases::editing_lock::ensure_unlocked_tx(tx, database_id).await?;
    let id = generate_uuid_tx(tx, "generate gallery view id", "generated_gallery_view_id").await?;
    let sort_order = data_sources::views::next_view_sort_order_tx(tx, database_id).await?;
    sqlx::query(
        "INSERT INTO notes_database_views (
            id,
            database_id,
            data_source_id,
            name,
            type,
            sorts,
            configuration,
            sort_order
         )
         VALUES (?, ?, ?, ?, 'gallery', '[]', ?, ?)",
    )
    .bind(&id)
    .bind(database_id)
    .bind(&data_source.id)
    .bind(DEFAULT_GALLERY_VIEW_NAME)
    .bind(default_gallery_configuration(schema).to_string())
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes gallery view: {e}"))?;
    load_gallery_view_row_tx(tx, &data_source.id, Some(database_id), Some(&id))
        .await?
        .ok_or_else(|| "inserted gallery view was not found".to_string())
}

async fn load_gallery_view_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    database_id: Option<&str>,
    view_id: Option<&str>,
) -> Result<Option<NoteDatabaseViewRow>, String> {
    data_sources::views::load_scoped_view_row_tx(
        tx,
        data_source_id,
        "gallery",
        database_id,
        view_id,
    )
    .await
}

fn default_gallery_configuration(schema: &[ViewProperty]) -> Value {
    json!({
        "type": "gallery",
        "gallery": {
            "cover_source": "page_cover",
            "cover_property_id": Value::Null,
            "visible_property_ids": visible_gallery_property_ids(schema),
            "card_size": "medium",
            "fit_image": false,
            "row_open_mode": "full_page"
        }
    })
}

fn canonical_gallery_configuration(
    update: &NoteDataSourceGalleryConfigurationUpdate,
    schema: &[ViewProperty],
) -> Result<Value, String> {
    let cover_source = update.cover_source.trim();
    if !GALLERY_COVER_SOURCES.contains(&cover_source) {
        return Err("gallery cover_source is not supported".to_string());
    }
    let cover_property_id =
        canonical_cover_property_id(cover_source, update.cover_property_id.as_deref(), schema)?;
    let visible_property_ids = canonical_visible_property_ids(&update.visible_property_ids, schema);
    let card_size = update.card_size.trim();
    if !GALLERY_CARD_SIZES.contains(&card_size) {
        return Err("gallery card_size is not supported".to_string());
    }
    let row_open_mode = update.row_open_mode.trim();
    if !GALLERY_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("gallery row_open_mode is not supported".to_string());
    }
    let value = json!({
        "type": "gallery",
        "gallery": {
            "cover_source": cover_source,
            "cover_property_id": cover_property_id,
            "visible_property_ids": visible_property_ids,
            "card_size": card_size,
            "fit_image": update.fit_image,
            "row_open_mode": row_open_mode
        }
    });
    if value.to_string().len() > MAX_GALLERY_CONFIGURATION_BYTES {
        return Err("gallery configuration must not exceed 50KB".to_string());
    }
    Ok(value)
}

fn validate_gallery_configuration(
    configuration: Option<&str>,
    schema: &[ViewProperty],
) -> Result<(), String> {
    let value = configuration
        .map(|configuration| parse_json(configuration, "gallery view configuration"))
        .transpose()?
        .unwrap_or_else(|| default_gallery_configuration(schema));
    let gallery = value
        .get("gallery")
        .and_then(Value::as_object)
        .ok_or_else(|| "gallery view configuration must contain gallery".to_string())?;
    let cover_source = gallery
        .get("cover_source")
        .and_then(Value::as_str)
        .unwrap_or("page_cover");
    if !GALLERY_COVER_SOURCES.contains(&cover_source) {
        return Err("gallery cover_source is not supported".to_string());
    }
    canonical_cover_property_id(
        cover_source,
        gallery.get("cover_property_id").and_then(Value::as_str),
        schema,
    )?;
    let card_size = gallery
        .get("card_size")
        .and_then(Value::as_str)
        .unwrap_or("medium");
    if !GALLERY_CARD_SIZES.contains(&card_size) {
        return Err("gallery card_size is not supported".to_string());
    }
    let row_open_mode = gallery
        .get("row_open_mode")
        .and_then(Value::as_str)
        .unwrap_or("full_page");
    if !GALLERY_ROW_OPEN_MODES.contains(&row_open_mode) {
        return Err("gallery row_open_mode is not supported".to_string());
    }
    Ok(())
}

fn canonical_cover_property_id(
    cover_source: &str,
    raw_property_id: Option<&str>,
    schema: &[ViewProperty],
) -> Result<Value, String> {
    if cover_source != "files_property" {
        return Ok(Value::Null);
    }
    let property_id = raw_property_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            "gallery files_property cover source requires a files property".to_string()
        })?;
    let property = schema
        .iter()
        .find(|property| property.id == property_id)
        .ok_or_else(|| "gallery cover property references an unknown property".to_string())?;
    if property.property_type != "files" {
        return Err("gallery cover property must be a files property".to_string());
    }
    Ok(Value::String(property_id.to_string()))
}

fn visible_gallery_property_ids(schema: &[ViewProperty]) -> Vec<String> {
    schema
        .iter()
        .filter(|property| property.property_type != "title")
        .take(4)
        .map(|property| property.id.clone())
        .collect()
}

fn canonical_visible_property_ids(property_ids: &[String], schema: &[ViewProperty]) -> Vec<String> {
    let known: HashSet<&str> = schema.iter().map(|property| property.id.as_str()).collect();
    let mut seen = HashSet::new();
    let mut visible = Vec::new();
    for property_id in property_ids {
        let property_id = property_id.trim();
        if property_id.is_empty() || property_id == "title" || !known.contains(property_id) {
            continue;
        }
        if seen.insert(property_id.to_string()) {
            visible.push(property_id.to_string());
        }
    }
    visible
}
