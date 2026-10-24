//! Stored shape of data source schemas.
//!
//! The API schema is a JSON object keyed by property name. Storage keeps one
//! `notes_data_source_properties` row per property, holding its name, type, and every other
//! field in `config`, and one `notes_data_source_property_options` row per select,
//! multi-select, or status option, holding the option and its status group. Reads compose the
//! schema object back in SQL; writes diff the composed object against the stored rows and touch
//! only the rows that change, so concurrent edits to different properties or options never
//! overwrite each other.

use crate::page_property_storage;
use crate::sync_keys::{PROPERTIES, PROPERTY_OPTIONS};
use ganbaru_sync_contracts::OrderKey;
use ganbaru_sync_contracts::order_placement::{self, OrderedRow};
use serde_json::{Map, Value};
use sqlx::SqliteConnection;
use std::collections::{HashMap, HashSet};

/// Property types whose options are stored as option rows.
pub const OPTION_PROPERTY_TYPES: &[&str] = &["select", "multi_select", "status"];

/// Status group name of an option that no group lists.
const STATUS_OPTION_TYPE: &str = "status";

/// SQL expression that composes the schema object of the `notes_data_sources` row aliased as
/// `$alias` from its property and option rows.
///
/// Properties follow their order keys and options their positions. Status groups list their
/// option ids in option order.
#[macro_export]
#[doc(hidden)]
macro_rules! note_data_source_properties_json {
    ($alias:literal) => {
        concat!(
            "(SELECT json_group_object(stored_property.name, json(CASE ",
            "WHEN stored_property.type IN ('select', 'multi_select') THEN json_set(",
            $crate::note_data_source_property_base_json!(),
            ", '$.' || stored_property.type || '.options', json(",
            $crate::note_data_source_property_options_json!(),
            ")) ",
            "WHEN stored_property.type = 'status' THEN json_set(",
            $crate::note_data_source_property_base_json!(),
            ", '$.status.options', json(",
            $crate::note_data_source_property_options_json!(),
            "), '$.status.groups', json((SELECT json_group_array(json(json_set(json(status_group.value), '$.option_ids', json((",
            "SELECT json_group_array(group_member.option_id ORDER BY group_member.position, group_member.option_id) ",
            "FROM notes_data_source_property_options AS group_member ",
            "WHERE group_member.data_source_id = stored_property.data_source_id ",
            "AND group_member.property_id = stored_property.property_id ",
            "AND group_member.status_group = json_extract(status_group.value, '$.name'))))) ORDER BY status_group.id) ",
            "FROM json_each(stored_property.config, '$.status.groups') AS status_group))) ",
            "ELSE ",
            $crate::note_data_source_property_base_json!(),
            " END) ORDER BY stored_property.order_key, stored_property.property_id) ",
            "FROM notes_data_source_properties AS stored_property ",
            "WHERE stored_property.data_source_id = ",
            $alias,
            ".id)"
        )
    };
}

/// A stored property's config with its id, name, and type restored.
#[macro_export]
#[doc(hidden)]
macro_rules! note_data_source_property_base_json {
    () => {
        "json_set(stored_property.config, '$.id', stored_property.property_id, '$.name', stored_property.name, '$.type', stored_property.type)"
    };
}

/// A stored property's options as an API option array.
#[macro_export]
#[doc(hidden)]
macro_rules! note_data_source_property_options_json {
    () => {
        concat!(
            "(SELECT json_group_array(json_object('id', stored_option.option_id, 'name', stored_option.name, 'color', stored_option.color) ",
            "ORDER BY stored_option.position, stored_option.option_id) ",
            "FROM notes_data_source_property_options AS stored_option ",
            "WHERE stored_option.data_source_id = stored_property.data_source_id ",
            "AND stored_option.property_id = stored_property.property_id)"
        )
    };
}

/// Select list that reads every column a [`crate::models::NoteDataSourceRow`] needs from a
/// `notes_data_sources` row aliased as `$alias`, composing `properties` from the property rows.
#[macro_export]
#[doc(hidden)]
macro_rules! note_data_source_row_columns {
    ($alias:literal) => {
        concat!(
            $alias,
            ".id, ",
            $alias,
            ".database_id, ",
            $alias,
            ".title, ",
            $alias,
            ".title_rich_text, ",
            $alias,
            ".description, ",
            $alias,
            ".icon, ",
            $crate::note_data_source_properties_json!($alias),
            " AS properties, ",
            $alias,
            ".in_trash, ",
            $alias,
            ".source_provider, ",
            $alias,
            ".source_object_id, ",
            $alias,
            ".source_workspace_id, ",
            $alias,
            ".source_last_edited_time, ",
            $alias,
            ".created_time, ",
            $alias,
            ".last_edited_time"
        )
    };
}

/// One property of a schema as it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredProperty {
    pub property_id: String,
    pub name: String,
    pub property_type: String,
    pub config: Value,
    pub options: Vec<StoredOption>,
}

/// One option of a select, multi-select, or status property as it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOption {
    pub option_id: String,
    pub name: String,
    pub color: String,
    pub status_group: Option<String>,
}

/// Splits an API schema object into stored properties, in object order.
///
/// Options move out of the type config, and status group membership moves from the groups'
/// `option_ids` onto the options; a status option no group lists has no group. Property ids
/// and names must be unique, because reads key the composed schema by name and rows by id, so
/// a duplicate would hide one property and let the next diff delete it.
pub fn decompose_schema(schema: &Value) -> Result<Vec<StoredProperty>, String> {
    let object = schema
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let properties = object
        .iter()
        .map(|(key, value)| decompose_property(key, value))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for property in &properties {
        if !ids.insert(property.property_id.as_str()) {
            return Err("property ids must be unique".to_string());
        }
        if !names.insert(property.name.as_str()) {
            return Err("property names must be unique".to_string());
        }
    }
    Ok(properties)
}

fn decompose_property(key: &str, value: &Value) -> Result<StoredProperty, String> {
    let mut config = value
        .as_object()
        .cloned()
        .ok_or_else(|| "data source property must be an object".to_string())?;
    let property_type = take_text(&mut config, "type")
        .ok_or_else(|| "data source property type is required".to_string())?;
    let property_id = take_text(&mut config, "id")
        .or_else(|| (property_type == "title").then(|| "title".to_string()));
    let property_id = property_id
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| "data source property id is required".to_string())?;
    let name = take_text(&mut config, "name").unwrap_or_else(|| key.to_string());
    let options = if OPTION_PROPERTY_TYPES.contains(&property_type.as_str()) {
        take_options(&mut config, &property_type)?
    } else {
        Vec::new()
    };
    Ok(StoredProperty {
        property_id,
        name,
        property_type,
        config: Value::Object(config),
        options,
    })
}

fn take_text(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    match object.remove(key) {
        Some(Value::String(text)) => Some(text),
        _ => None,
    }
}

fn take_options(
    config: &mut Map<String, Value>,
    property_type: &str,
) -> Result<Vec<StoredOption>, String> {
    let Some(type_config) = config.get_mut(property_type).and_then(Value::as_object_mut) else {
        return Ok(Vec::new());
    };
    let mut groups = HashMap::<String, String>::new();
    if property_type == STATUS_OPTION_TYPE {
        if let Some(items) = type_config.get_mut("groups").and_then(Value::as_array_mut) {
            for group in items.iter_mut().filter_map(Value::as_object_mut) {
                let name = group
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let members = group.remove("option_ids");
                if let (Some(name), Some(Value::Array(members))) = (name, members) {
                    for member in members.iter().filter_map(Value::as_str) {
                        groups
                            .entry(member.to_string())
                            .or_insert_with(|| name.clone());
                    }
                }
            }
        }
    }
    let items = match type_config.remove("options") {
        Some(Value::Array(items)) => items,
        Some(_) => return Err("property options must be an array".to_string()),
        None => Vec::new(),
    };
    let mut seen = HashSet::new();
    items
        .iter()
        .map(|item| {
            let option_id = item
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| "property option id is required".to_string())?;
            if !seen.insert(option_id) {
                return Err("property option ids must be unique".to_string());
            }
            Ok(StoredOption {
                option_id: option_id.to_string(),
                name: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                color: item
                    .get("color")
                    .and_then(Value::as_str)
                    .unwrap_or("default")
                    .to_string(),
                status_group: groups.get(option_id).cloned(),
            })
        })
        .collect()
}

/// Order keys that make `desired` sort in its order, reusing the keys of `existing` rows that
/// already sort correctly.
///
/// `existing` holds the stored `(id, order_key)` rows, in any order; ids absent from `desired`
/// are ignored. The result maps every desired id to its key.
pub fn plan_order_keys(
    existing: &[(String, String)],
    desired: &[&str],
) -> Result<HashMap<String, String>, String> {
    let wanted = desired.iter().copied().collect::<HashSet<_>>();
    let mut kept = existing
        .iter()
        .filter(|row| wanted.contains(row.0.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    kept.sort_by(|left, right| (&left.1, &left.0).cmp(&(&right.1, &right.0)));
    let kept_ids = kept.iter().map(|row| row.0.clone()).collect::<HashSet<_>>();
    let existing_order = desired
        .iter()
        .copied()
        .filter(|id| kept_ids.contains(*id))
        .collect::<Vec<_>>();
    let mut keys = kept
        .iter()
        .map(|row| (row.0.clone(), row.1.clone()))
        .collect::<HashMap<_, _>>();
    for (id, key) in order_placement::reorder(&kept, &existing_order)
        .map_err(|error| format!("order data source schema rows: {error}"))?
    {
        keys.insert(id, key.into_string());
    }
    let mut group = existing_order
        .iter()
        .map(|id| ((*id).to_string(), keys[*id].clone()))
        .collect::<Vec<_>>();
    let mut position = 0;
    for id in desired {
        if kept_ids.contains(*id) {
            position += 1;
            continue;
        }
        let updates = order_placement::placement(&group, position, id)
            .map_err(|error| format!("place data source schema row: {error}"))?;
        let (key, others) = order_placement::take_key(updates, id)
            .map_err(|error| format!("place data source schema row: {error}"))?;
        for (row_id, row_key) in others {
            if let Some(row) = group.iter_mut().find(|row| row.0 == row_id) {
                row.1 = row_key.into_string();
            }
        }
        group.insert(position, ((*id).to_string(), key.into_string()));
        position += 1;
    }
    for row in &group {
        debug_assert!(OrderKey::parse(row.order_key()).is_ok());
    }
    Ok(group.into_iter().collect())
}

/// Replaces the stored schema of `data_source_id` with `schema`, writing only changed rows.
///
/// Properties and options the schema omits are deleted, with the row values stored under a
/// deleted property. New rows get their derived sync keys; updated rows keep theirs. When a
/// relation property is added, removed, retargeted, or paired differently, the edges under it
/// and its synced properties are reconciled so every link keeps its meaning (see
/// [`page_property_storage::reconcile_relation_edges`]).
pub async fn write_data_source_properties(
    connection: &mut SqliteConnection,
    data_source_id: &str,
    schema: &Value,
) -> Result<(), String> {
    let properties = decompose_schema(schema)?;
    let stored = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT property_id, name, type, config, order_key
         FROM notes_data_source_properties WHERE data_source_id = ?",
    )
    .bind(data_source_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load notes data source properties: {e}"))?;
    let mut relation_scope = changed_relation_scope(data_source_id, &stored, &properties)?;
    page_property_storage::extend_scope_with_namers(connection, &mut relation_scope).await?;
    let relation_links = if relation_scope.is_empty() {
        Vec::new()
    } else {
        page_property_storage::relation_links_in_scope(connection, &relation_scope).await?
    };
    let wanted = properties
        .iter()
        .map(|property| property.property_id.as_str())
        .collect::<HashSet<_>>();
    for (property_id, ..) in &stored {
        if !wanted.contains(property_id.as_str()) {
            sqlx::query(
                "DELETE FROM notes_page_property_values
                 WHERE property_id = ?
                   AND page_id IN (SELECT id FROM notes_pages WHERE parent_data_source_id = ?)",
            )
            .bind(property_id)
            .bind(data_source_id)
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("delete notes page property values: {e}"))?;
            sqlx::query(
                "DELETE FROM notes_data_source_properties
                 WHERE data_source_id = ? AND property_id = ?",
            )
            .bind(data_source_id)
            .bind(property_id)
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("delete notes data source property: {e}"))?;
        }
    }
    let existing_keys = stored
        .iter()
        .map(|row| (row.0.clone(), row.4.clone()))
        .collect::<Vec<_>>();
    let desired = properties
        .iter()
        .map(|property| property.property_id.as_str())
        .collect::<Vec<_>>();
    let order_keys = plan_order_keys(&existing_keys, &desired)?;
    let stored_by_id = stored
        .iter()
        .map(|row| (row.0.as_str(), row))
        .collect::<HashMap<_, _>>();
    for property in &properties {
        let order_key = &order_keys[property.property_id.as_str()];
        let unchanged = stored_by_id
            .get(property.property_id.as_str())
            .is_some_and(|row| {
                row.1 == property.name
                    && row.2 == property.property_type
                    && row.4 == *order_key
                    && serde_json::from_str::<Value>(&row.3).ok().as_ref() == Some(&property.config)
            });
        if !unchanged {
            sqlx::query(
                "INSERT INTO notes_data_source_properties
                   (data_source_id, property_id, sync_key, name, type, config, order_key)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (data_source_id, property_id) DO UPDATE SET
                   name = excluded.name,
                   type = excluded.type,
                   config = excluded.config,
                   order_key = excluded.order_key",
            )
            .bind(data_source_id)
            .bind(&property.property_id)
            .bind(PROPERTIES.key(&[data_source_id, &property.property_id])?)
            .bind(&property.name)
            .bind(&property.property_type)
            .bind(property.config.to_string())
            .bind(order_key)
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("write notes data source property: {e}"))?;
        }
        write_property_options(connection, data_source_id, property).await?;
    }
    if !relation_scope.is_empty() {
        page_property_storage::reconcile_relation_edges(
            connection,
            &relation_scope,
            &relation_links,
        )
        .await?;
    }
    Ok(())
}

/// The relation target and synced property a property's config names, when it is a relation.
fn relation_identity(property_type: &str, config: &Value) -> Option<(String, Option<String>)> {
    if property_type != "relation" {
        return None;
    }
    let relation = config.get("relation")?;
    let target = relation.get("data_source_id")?.as_str()?.to_string();
    let synced = relation
        .get("dual_property")
        .and_then(|dual| dual.get("synced_property_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((target, synced))
}

/// The relation properties whose links a write of `properties` over `stored` can change.
fn changed_relation_scope(
    data_source_id: &str,
    stored: &[(String, String, String, String, String)],
    properties: &[StoredProperty],
) -> Result<page_property_storage::RelationScope, String> {
    let mut before = HashMap::new();
    for (property_id, _, property_type, config, _) in stored {
        let config = serde_json::from_str::<Value>(config)
            .map_err(|e| format!("parse notes data source property config: {e}"))?;
        if let Some(identity) = relation_identity(property_type, &config) {
            before.insert(property_id.as_str(), identity);
        }
    }
    let after = properties
        .iter()
        .filter_map(|property| {
            relation_identity(&property.property_type, &property.config)
                .map(|identity| (property.property_id.as_str(), identity))
        })
        .collect::<HashMap<_, _>>();
    let mut scope = page_property_storage::RelationScope::new();
    let ids = before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<HashSet<_>>();
    for property_id in ids {
        let old = before.get(property_id);
        let new = after.get(property_id);
        if old == new {
            continue;
        }
        scope.insert((data_source_id.to_string(), property_id.to_string()));
        for (target, synced) in [old, new].into_iter().flatten() {
            if let Some(synced) = synced {
                scope.insert((target.clone(), synced.clone()));
            }
        }
    }
    Ok(scope)
}

type StoredOptionRow = (String, String, String, Option<String>, String);

async fn write_property_options(
    connection: &mut SqliteConnection,
    data_source_id: &str,
    property: &StoredProperty,
) -> Result<(), String> {
    let stored = sqlx::query_as::<_, StoredOptionRow>(
        "SELECT option_id, name, color, status_group, position
         FROM notes_data_source_property_options
         WHERE data_source_id = ? AND property_id = ?",
    )
    .bind(data_source_id)
    .bind(&property.property_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load notes data source property options: {e}"))?;
    let wanted = property
        .options
        .iter()
        .map(|option| option.option_id.as_str())
        .collect::<HashSet<_>>();
    for (option_id, ..) in &stored {
        if !wanted.contains(option_id.as_str()) {
            sqlx::query(
                "DELETE FROM notes_data_source_property_options
                 WHERE data_source_id = ? AND property_id = ? AND option_id = ?",
            )
            .bind(data_source_id)
            .bind(&property.property_id)
            .bind(option_id)
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("delete notes data source property option: {e}"))?;
        }
    }
    if property.options.is_empty() {
        return Ok(());
    }
    let existing_keys = stored
        .iter()
        .map(|row| (row.0.clone(), row.4.clone()))
        .collect::<Vec<_>>();
    let desired = property
        .options
        .iter()
        .map(|option| option.option_id.as_str())
        .collect::<Vec<_>>();
    let positions = plan_order_keys(&existing_keys, &desired)?;
    let stored_by_id = stored
        .iter()
        .map(|row| (row.0.as_str(), row))
        .collect::<HashMap<_, _>>();
    for option in &property.options {
        let position = &positions[option.option_id.as_str()];
        let unchanged = stored_by_id
            .get(option.option_id.as_str())
            .is_some_and(|row| {
                row.1 == option.name
                    && row.2 == option.color
                    && row.3 == option.status_group
                    && row.4 == *position
            });
        if unchanged {
            continue;
        }
        sqlx::query(
            "INSERT INTO notes_data_source_property_options
               (data_source_id, property_id, option_id, sync_key, name, color, status_group, position)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (data_source_id, property_id, option_id) DO UPDATE SET
               name = excluded.name,
               color = excluded.color,
               status_group = excluded.status_group,
               position = excluded.position",
        )
        .bind(data_source_id)
        .bind(&property.property_id)
        .bind(&option.option_id)
        .bind(PROPERTY_OPTIONS.key(&[
            data_source_id,
            &property.property_id,
            &option.option_id,
        ])?)
        .bind(&option.name)
        .bind(&option.color)
        .bind(&option.status_group)
        .bind(position)
        .execute(&mut *connection)
        .await
        .map_err(|e| format!("write notes data source property option: {e}"))?;
    }
    Ok(())
}

/// Reads the composed schema object of `data_source_id`, or `None` when the data source does
/// not exist.
pub async fn load_data_source_properties(
    connection: &mut SqliteConnection,
    data_source_id: &str,
) -> Result<Option<Value>, String> {
    let properties: Option<String> = sqlx::query_scalar(concat!(
        "SELECT ",
        crate::note_data_source_properties_json!("data_source"),
        " FROM notes_data_sources AS data_source WHERE data_source.id = ?"
    ))
    .bind(data_source_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|e| format!("load notes data source properties: {e}"))?;
    parse_properties(properties)
}

/// Reads the composed schema object of `data_source_id`, or `None` when the data source does
/// not exist or is in the trash.
pub async fn load_active_data_source_properties(
    connection: &mut SqliteConnection,
    data_source_id: &str,
) -> Result<Option<Value>, String> {
    let properties: Option<String> = sqlx::query_scalar(concat!(
        "SELECT ",
        crate::note_data_source_properties_json!("data_source"),
        " FROM notes_data_sources AS data_source WHERE data_source.id = ? AND data_source.in_trash = 0"
    ))
    .bind(data_source_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|e| format!("load notes data source properties: {e}"))?;
    parse_properties(properties)
}

fn parse_properties(properties: Option<String>) -> Result<Option<Value>, String> {
    properties
        .map(|properties| {
            serde_json::from_str(&properties)
                .map_err(|e| format!("parse notes data source properties: {e}"))
        })
        .transpose()
}

/// Copies the stored schema of `source_id` onto `target_id`, which has no properties yet.
pub async fn copy_data_source_properties(
    connection: &mut SqliteConnection,
    source_id: &str,
    target_id: &str,
) -> Result<(), String> {
    let properties = load_data_source_properties(connection, source_id)
        .await?
        .ok_or_else(|| "data source not found".to_string())?;
    write_data_source_properties(connection, target_id, &properties).await
}

#[cfg(test)]
mod tests {
    use super::{decompose_schema, plan_order_keys};
    use serde_json::json;

    #[test]
    fn decompose_moves_options_and_status_membership_out_of_the_config() {
        let schema = json!({
            "Stage": {
                "id": "stage",
                "name": "Stage",
                "type": "status",
                "description": "",
                "status": {
                    "options": [
                        {"id": "todo", "name": "Todo", "color": "default"},
                        {"id": "done", "name": "Done", "color": "green"}
                    ],
                    "groups": [
                        {"id": "To-do", "name": "To-do", "color": "gray", "option_ids": ["todo"]},
                        {"id": "Complete", "name": "Complete", "color": "green", "option_ids": ["done"]}
                    ],
                    "default_option_id": "todo"
                }
            }
        });
        let properties = decompose_schema(&schema).unwrap();
        assert_eq!(properties.len(), 1);
        let stage = &properties[0];
        assert_eq!(stage.property_id, "stage");
        assert_eq!(
            stage.config,
            json!({
                "description": "",
                "status": {
                    "groups": [
                        {"id": "To-do", "name": "To-do", "color": "gray"},
                        {"id": "Complete", "name": "Complete", "color": "green"}
                    ],
                    "default_option_id": "todo"
                }
            })
        );
        let groups = stage
            .options
            .iter()
            .map(|option| (option.option_id.as_str(), option.status_group.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            groups,
            vec![("todo", Some("To-do")), ("done", Some("Complete"))]
        );
    }

    #[test]
    fn decompose_refuses_options_without_ids() {
        let schema = json!({
            "Tag": {"id": "tag", "type": "select", "select": {"options": [{"name": "A"}]}}
        });
        assert!(decompose_schema(&schema).is_err());
    }

    #[test]
    fn decompose_refuses_duplicate_property_names_and_ids() {
        let same_name = json!({
            "Stage": {"id": "stage", "name": "Owner", "type": "rich_text"},
            "Owner": {"id": "owner", "type": "rich_text"}
        });
        assert_eq!(
            decompose_schema(&same_name).err().as_deref(),
            Some("property names must be unique")
        );
        let same_id = json!({
            "Stage": {"id": "shared", "type": "rich_text"},
            "Owner": {"id": "shared", "type": "number"}
        });
        assert_eq!(
            decompose_schema(&same_id).err().as_deref(),
            Some("property ids must be unique")
        );
    }

    #[test]
    fn plan_order_keys_keeps_sorted_rows_and_places_new_rows_between_them() {
        let existing = vec![
            ("a".to_string(), "a0".to_string()),
            ("b".to_string(), "a1".to_string()),
            ("c".to_string(), "a2".to_string()),
            ("gone".to_string(), "a3".to_string()),
        ];
        let keys = plan_order_keys(&existing, &["a", "new", "c", "b"]).unwrap();
        assert_eq!(keys["a"], "a0");
        assert!(keys["a"] < keys["new"]);
        assert!(keys["new"] < keys["c"]);
        assert!(keys["c"] < keys["b"]);
        assert!(!keys.contains_key("gone"));
        let unchanged = [("a", "a0"), ("b", "a1")];
        let kept = unchanged
            .iter()
            .filter(|(id, key)| keys[*id] == *key)
            .count();
        assert!(kept >= 1, "a sorted run keeps its keys");
    }

    #[test]
    fn plan_order_keys_numbers_an_empty_group() {
        let keys = plan_order_keys(&[], &["x", "y", "z"]).unwrap();
        assert!(keys["x"] < keys["y"] && keys["y"] < keys["z"]);
    }
}
