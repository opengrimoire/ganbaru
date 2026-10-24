//! Atomic contextual property actions within one requesting table: insert, duplicate, change type, and delete.

use crate::data_sources::property_conversion::{self, ConversionRow};
use crate::models::{
    NoteDataSourcePropertyAction, NoteDataSourcePropertyInsertionSide, NoteDataSourceSchemaDto,
    NoteDataSourceSchemaUpdate,
};
use crate::validation::require_uuid;
use crate::{data_sources, page_history};
use serde::Serialize;
use serde_json::{Map, Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct NoteDataSourcePropertyActionDto {
    pub property_id: String,
    pub schema: NoteDataSourceSchemaDto,
}

/// Row work an action applies once the next schema is canonical.
enum RowChange {
    None,
    /// Rows rewrite the property's values for its new type.
    Convert {
        key: String,
        property_id: String,
        rows: Vec<ConversionRow>,
        items: Vec<Vec<String>>,
    },
    /// Rows drop the deleted property's stored values.
    Remove {
        key: String,
        property_id: String,
    },
}

/// Apply one property action, its row values, and its requesting view position without exposing an intermediate state.
pub async fn apply_property_action(
    pool: &SqlitePool,
    data_source_id: &str,
    database_id: &str,
    view_id: &str,
    action: NoteDataSourcePropertyAction,
) -> Result<NoteDataSourcePropertyActionDto, String> {
    data_sources::views::validate_view_scope(data_source_id, Some(database_id), Some(view_id))?;
    let data_source_id = data_source_id.trim();
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin database property action: {error}"))?;
    let view = data_sources::views::load_scoped_view_row_tx(
        &mut tx,
        data_source_id,
        "table",
        Some(database_id),
        Some(view_id),
    )
    .await?
    .ok_or_else(|| "requesting table view not found".to_string())?;
    data_sources::views::prepare_view_mutation_tx(&mut tx, &view, "Database property").await?;
    let mut properties =
        crate::property_storage::load_active_data_source_properties(&mut *tx, data_source_id)
            .await?
            .ok_or_else(|| "data source not found".to_string())?;
    let object = properties
        .as_object_mut()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let target_id = match &action {
        NoteDataSourcePropertyAction::Insert { property_id, .. }
        | NoteDataSourcePropertyAction::Duplicate { property_id, .. }
        | NoteDataSourcePropertyAction::ChangeType { property_id, .. }
        | NoteDataSourcePropertyAction::Delete { property_id } => property_id.clone(),
    };
    let (target_key, target) = object
        .iter()
        .find(|(_, property)| {
            property.get("id").and_then(Value::as_str) == Some(target_id.as_str())
        })
        .map(|(key, property)| (key.clone(), property.clone()))
        .ok_or_else(|| "property action target not found".to_string())?;
    let target_is_title = target.get("type").and_then(Value::as_str) == Some("title");
    let mut placement = None;
    let mut row_change = RowChange::None;
    let result_id = match action {
        NoteDataSourcePropertyAction::Insert { property, side, .. } => {
            if target_is_title && matches!(side, NoteDataSourcePropertyInsertionSide::Left) {
                return Err("the required title must remain first".to_string());
            }
            let new_id = insert_property(object, property)?;
            placement = Some(side);
            new_id
        }
        NoteDataSourcePropertyAction::Duplicate { name, .. } => {
            if target_is_title {
                return Err("the required title cannot be duplicated".to_string());
            }
            let property = duplicate_property_tx(&mut tx, target.clone(), &name).await?;
            let new_id = insert_property(object, property)?;
            placement = Some(NoteDataSourcePropertyInsertionSide::Right);
            new_id
        }
        NoteDataSourcePropertyAction::ChangeType {
            property_type,
            config,
            ..
        } => {
            if target_is_title {
                return Err("the required title cannot change type".to_string());
            }
            let (property, change) = changed_type_property_tx(
                &mut tx,
                data_source_id,
                &target_key,
                &target,
                &property_type,
                config,
            )
            .await?;
            detach_inverse_relation_tx(&mut tx, data_source_id, object, &target).await?;
            object.insert(target_key, property);
            row_change = change;
            target_id.clone()
        }
        NoteDataSourcePropertyAction::Delete { .. } => {
            if target_is_title {
                return Err("the required title cannot be deleted".to_string());
            }
            object.remove(&target_key);
            detach_inverse_relation_tx(&mut tx, data_source_id, object, &target).await?;
            row_change = RowChange::Remove {
                key: target_key,
                property_id: target_id.clone(),
            };
            target_id.clone()
        }
    };
    if let Some(side) = placement {
        place_page_position(object, &target, &result_id, side);
    }
    let prepared =
        data_sources::schema::prepare_schema_update(&NoteDataSourceSchemaUpdate { properties })?;
    let page_id: String =
        sqlx::query_scalar("SELECT page_id FROM notes_blocks WHERE id = ? AND in_trash = 0")
            .bind(database_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| format!("load property action parent page: {error}"))?;
    page_history::record_page_snapshot_tx(&mut tx, &page_id, "data_source_property_action").await?;
    apply_row_change_tx(&mut tx, data_source_id, row_change, &prepared).await?;
    data_sources::schema::update_data_source_schema_tx(
        &mut tx,
        data_source_id,
        Some(database_id),
        Some(&view.id),
        prepared,
    )
    .await?;
    if let Some(side) = placement {
        place_property_tx(&mut tx, &view.id, &target_id, &result_id, side).await?;
    }
    let schema = data_sources::schema::load_data_source_schema_tx(
        &mut tx,
        data_source_id,
        Some(database_id),
        Some(&view.id),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit database property action: {error}"))?;
    Ok(NoteDataSourcePropertyActionDto {
        property_id: result_id,
        schema,
    })
}

/// Place a new property beside its target on row pages when the page has an explicit order.
///
/// Positioned properties are renumbered from zero with the new one beside its target. An unpositioned target leaves
/// the new property unpositioned, so it follows the fallback order like its target.
fn place_page_position(
    object: &mut Map<String, Value>,
    target: &Value,
    new_id: &str,
    side: NoteDataSourcePropertyInsertionSide,
) {
    let property_id = |property: &Value| {
        property
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let target_id = property_id(target);
    let targeted = target
        .get("page_position")
        .and_then(Value::as_u64)
        .is_some();
    let mut positioned: Vec<(u64, String)> = object
        .values()
        .filter_map(|property| {
            let id = property_id(property)?;
            let position = property.get("page_position").and_then(Value::as_u64)?;
            (id != new_id).then_some((position, id))
        })
        .collect();
    positioned.sort();
    let mut order: Vec<String> = positioned.into_iter().map(|(_, id)| id).collect();
    if targeted {
        let index = order
            .iter()
            .position(|id| Some(id) == target_id.as_ref())
            .map_or(order.len(), |index| {
                index + usize::from(matches!(side, NoteDataSourcePropertyInsertionSide::Right))
            });
        order.insert(index, new_id.to_string());
    }
    for property in object.values_mut() {
        let Some(id) = property_id(property) else {
            continue;
        };
        let Some(property) = property.as_object_mut() else {
            continue;
        };
        match order.iter().position(|candidate| *candidate == id) {
            Some(index) => property.insert("page_position".to_string(), json!(index)),
            None => property.remove("page_position"),
        };
    }
}

/// Add a new non-title property with a unique id and name, returning its id.
fn insert_property(object: &mut Map<String, Value>, property: Value) -> Result<String, String> {
    if property.get("type").and_then(Value::as_str) == Some("title") {
        return Err("a source has exactly one title property".to_string());
    }
    let new_id = property
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "new property id is required".to_string())?
        .to_string();
    if object
        .values()
        .any(|existing| existing.get("id").and_then(Value::as_str) == Some(&new_id))
    {
        return Err("new property id must be unique".to_string());
    }
    let name = property
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "new property name is required".to_string())?
        .to_string();
    if object.contains_key(&name) {
        return Err("new property name must be unique".to_string());
    }
    object.insert(name, property);
    Ok(new_id)
}

/// Build the property under its new type and read the row values it will convert.
///
/// Select-like targets keep the old property's options and gain one option per distinct row value, so values carry
/// over by name. Other targets use the requested configuration, or the type's defaults.
async fn changed_type_property_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    key: &str,
    current: &Value,
    property_type: &str,
    config: Option<Value>,
) -> Result<(Value, RowChange), String> {
    let property_id = current
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "property id is missing".to_string())?
        .to_string();
    let current_type = current
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "property type is missing".to_string())?;
    if property_type == "title" {
        return Err("a source has exactly one title property".to_string());
    }
    if property_type == current_type {
        return Err("the property already has this type".to_string());
    }
    let rows = property_conversion::load_rows_tx(tx, data_source_id).await?;
    let items: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            let value = property_conversion::stored_value(row, key, &property_id);
            let items = property_conversion::value_items(current_type, value, row);
            property_conversion::target_items(current_type, property_type, items)
        })
        .collect();
    let config = match property_type {
        "select" | "multi_select" | "status" => {
            options_config_tx(tx, current, property_type, &items).await?
        }
        "relation" => one_way_relation_config(config)?,
        _ => config.unwrap_or_else(|| json!({})),
    };
    let mut property = Map::new();
    for field in [
        "id",
        "name",
        "description",
        "icon",
        "page_visibility",
        "page_position",
    ] {
        if let Some(value) = current.get(field) {
            property.insert(field.to_string(), value.clone());
        }
    }
    property.insert("type".to_string(), json!(property_type));
    property.insert(property_type.to_string(), config);
    Ok((
        Value::Object(property),
        RowChange::Convert {
            key: key.to_string(),
            property_id,
            rows,
            items,
        },
    ))
}

async fn options_config_tx(
    tx: &mut Transaction<'_, Sqlite>,
    current: &Value,
    property_type: &str,
    items: &[Vec<String>],
) -> Result<Value, String> {
    let names = items.iter().flatten().map(String::as_str);
    let mut options = property_conversion::seeded_options(current, property_type, names);
    for option in &mut options {
        if option.get("id").is_none() {
            option["id"] = json!(
                data_sources::views::generate_uuid_tx(
                    tx,
                    "generate converted option id",
                    "option_id"
                )
                .await?
            );
        }
    }
    if property_type == "status" && options.is_empty() {
        return Ok(json!({}));
    }
    Ok(json!({ "options": options }))
}

/// A converted relation starts one-way; a reciprocal property needs the target's own schema edit.
fn one_way_relation_config(config: Option<Value>) -> Result<Value, String> {
    let data_source_id = config
        .as_ref()
        .and_then(|config| config.get("data_source_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "a relation needs a target data source".to_string())?;
    Ok(json!({ "data_source_id": data_source_id }))
}

/// Keep a reciprocal relation valid when its counterpart stops being a relation by turning it one-way.
async fn detach_inverse_relation_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    properties: &mut Map<String, Value>,
    removed: &Value,
) -> Result<(), String> {
    let Some(config) = data_sources::relations::relation_config_from_schema(removed)? else {
        return Ok(());
    };
    let Some(synced_id) = config.synced_property_id().map(str::to_string) else {
        return Ok(());
    };
    if config.target_data_source_id == data_source_id {
        detach_dual_property(properties, &synced_id);
        return Ok(());
    }
    let Some(mut target) = crate::property_storage::load_active_data_source_properties(
        &mut **tx,
        &config.target_data_source_id,
    )
    .await?
    else {
        return Ok(());
    };
    let Some(target_object) = target.as_object_mut() else {
        return Err("data source properties must be an object".to_string());
    };
    if !detach_dual_property(target_object, &synced_id) {
        return Ok(());
    }
    crate::project_history::mark_data_source_dirty_tx(
        tx,
        &config.target_data_source_id,
        "Database schema",
        false,
    )
    .await?;
    crate::property_storage::write_data_source_properties(
        &mut **tx,
        &config.target_data_source_id,
        &target,
    )
    .await?;
    sqlx::query(
        "UPDATE notes_data_sources
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(&config.target_data_source_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("detach reciprocal relation: {error}"))?;
    Ok(())
}

/// Remove the reciprocal link from one relation property, reporting whether anything changed.
fn detach_dual_property(properties: &mut Map<String, Value>, property_id: &str) -> bool {
    properties
        .values_mut()
        .find(|property| {
            property.get("id").and_then(Value::as_str) == Some(property_id)
                && property.get("type").and_then(Value::as_str) == Some("relation")
        })
        .and_then(|property| property.get_mut("relation"))
        .and_then(Value::as_object_mut)
        .is_some_and(|relation| relation.remove("dual_property").is_some())
}

async fn apply_row_change_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    change: RowChange,
    prepared: &Value,
) -> Result<(), String> {
    match change {
        RowChange::None => Ok(()),
        RowChange::Convert {
            key,
            property_id,
            mut rows,
            items,
        } => {
            let property = data_sources::layouts::table::table_schema(prepared)?
                .into_iter()
                .find(|property| property.id == property_id)
                .ok_or_else(|| "converted property is missing".to_string())?;
            for (row, items) in rows.iter_mut().zip(items) {
                property_conversion::remove_value(row, &key, &property_id);
                if let Some(value) = property_conversion::converted_value(&property, &items) {
                    property_conversion::insert_value(row, &property.key, value);
                }
            }
            property_conversion::save_rows_tx(tx, &rows, &property_id).await
        }
        RowChange::Remove { key, property_id } => {
            let mut rows = property_conversion::load_rows_tx(tx, data_source_id).await?;
            rows.retain_mut(|row| {
                property_conversion::remove_value(row, &key, &property_id).is_some()
            });
            property_conversion::save_rows_tx(tx, &rows, &property_id).await
        }
    }
}

async fn duplicate_property_tx(
    tx: &mut Transaction<'_, Sqlite>,
    mut property: Value,
    name: &str,
) -> Result<Value, String> {
    property["id"] = Value::String(
        data_sources::views::generate_uuid_tx(tx, "generate duplicated property id", "property_id")
            .await?,
    );
    property["name"] = Value::String(name.to_string());
    let property_type = property
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "property type is missing".to_string())?
        .to_string();
    let configuration = property
        .get_mut(&property_type)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "property configuration is missing".to_string())?;
    if property_type == "relation" {
        configuration.insert("type".to_string(), json!("single_property"));
        configuration.remove("dual_property");
    }
    let mut replacements = HashMap::new();
    if let Some(options) = configuration
        .get_mut("options")
        .and_then(Value::as_array_mut)
    {
        for option in options {
            let old = option
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "property option id is missing".to_string())?
                .to_string();
            let id = data_sources::views::generate_uuid_tx(
                tx,
                "generate duplicated option id",
                "option_id",
            )
            .await?;
            option["id"] = json!(id);
            replacements.insert(old, id);
        }
    }
    if let Some(groups) = configuration
        .get_mut("groups")
        .and_then(Value::as_array_mut)
    {
        for group in groups {
            if let Some(ids) = group.get_mut("option_ids").and_then(Value::as_array_mut) {
                for id in ids {
                    if let Some(next) = id.as_str().and_then(|old| replacements.get(old)) {
                        *id = json!(next);
                    }
                }
            }
        }
    }
    Ok(property)
}

async fn place_property_tx(
    tx: &mut Transaction<'_, Sqlite>,
    view_id: &str,
    target_id: &str,
    new_id: &str,
    side: NoteDataSourcePropertyInsertionSide,
) -> Result<(), String> {
    require_uuid(view_id, "view_id")?;
    let raw: String =
        sqlx::query_scalar("SELECT configuration FROM notes_database_views WHERE id = ?")
            .bind(view_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| format!("read requesting table position: {error}"))?;
    let mut configuration: Value = serde_json::from_str(&raw)
        .map_err(|error| format!("parse requesting table configuration: {error}"))?;
    let order = configuration
        .get_mut("table")
        .and_then(|table| table.get_mut("property_order"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "requesting table property order is missing".to_string())?;
    order.retain(|id| id.as_str() != Some(new_id));
    let index = order
        .iter()
        .position(|id| id.as_str() == Some(target_id))
        .ok_or_else(|| "target property is absent from table order".to_string())?;
    order.insert(
        index + usize::from(matches!(side, NoteDataSourcePropertyInsertionSide::Right)),
        json!(new_id),
    );
    sqlx::query("UPDATE notes_database_views SET configuration = ?, last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(configuration.to_string()).bind(view_id).execute(&mut **tx).await.map_err(|error| format!("save contextual property position: {error}"))?;
    Ok(())
}
