//! Row values for a database property whose type changes or which is deleted.
//!
//! A type change reads each row's old value as text items, then writes them through the same rules as a table
//! cell edit. Values the new type cannot represent become that type's empty value instead of failing the change.

use crate::data_sources;
use crate::data_sources::layouts::table::{self, TableProperty};
use crate::validation::rich_text_items_plain_text;
use serde_json::{Map, Value, json};
use sqlx::{Sqlite, Transaction};
use std::collections::HashSet;

/// Types whose values come from a fixed option list.
const OPTION_TYPES: &[&str] = &["select", "multi_select", "status"];

/// Types computed on read and never stored on rows.
const COMPUTED_TYPES: &[&str] = &["rollup", "formula", "button"];

/// Free text types whose commas separate several option names.
const TEXT_TYPES: &[&str] = &[
    "title",
    "rich_text",
    "url",
    "email",
    "phone_number",
    "place",
];

/// Text a checkbox accepts as checked, compared without ASCII case.
const CHECKED_TEXT: &[&str] = &["true", "yes", "1", "checked", "x"];

/// One row page's stored properties with the metadata a conversion can read.
pub(crate) struct ConversionRow {
    id: String,
    properties: Map<String, Value>,
    created_time: String,
    last_edited_time: String,
}

/// Load every row page of a source, including trashed and archived rows, so restored rows match the schema.
pub(crate) async fn load_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<Vec<ConversionRow>, String> {
    let rows = sqlx::query_as::<_, (String, String, String, String)>(sqlx::AssertSqlSafe(format!(
        "SELECT page.id, {} AS properties, page.created_time, page.last_edited_time
         FROM notes_pages AS page
         WHERE page.parent_type = 'data_source_id' AND page.parent_data_source_id = ?
         ORDER BY page.created_time ASC, page.id ASC",
        crate::page_property_storage::page_properties_sql("page")
    )))
    .bind(data_source_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load rows for property conversion: {e}"))?;
    rows.into_iter()
        .map(|(id, properties, created_time, last_edited_time)| {
            let properties = match serde_json::from_str::<Value>(&properties) {
                Ok(Value::Object(object)) => object,
                Ok(_) => return Err("row page properties must be an object".to_string()),
                Err(error) => return Err(format!("parse row page properties: {error}")),
            };
            Ok(ConversionRow {
                id,
                properties,
                created_time,
                last_edited_time,
            })
        })
        .collect()
}

/// Save each row's value of `property_id` without changing its edit time, since the schema changed, not
/// the row. A value the property no longer stores as a value row, such as an empty relation, is deleted;
/// relation links follow the schema write.
pub(crate) async fn save_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[ConversionRow],
    property_id: &str,
) -> Result<(), String> {
    for row in rows {
        let stored = row
            .properties
            .values()
            .find(|value| value.get("id").and_then(Value::as_str) == Some(property_id))
            .and_then(|value| {
                let value_type = value.get("type").and_then(Value::as_str)?;
                let inner = value.get(value_type)?;
                (!crate::page_property_storage::UNSTORED_VALUE_TYPES.contains(&value_type))
                    .then_some((value_type, inner))
            });
        match stored {
            Some((value_type, value)) => {
                crate::page_property_storage::write_page_property_value(
                    &mut **tx,
                    &row.id,
                    property_id,
                    value_type,
                    value,
                )
                .await?
            }
            None => {
                crate::page_property_storage::delete_page_property_value(
                    &mut **tx,
                    &row.id,
                    property_id,
                )
                .await?
            }
        }
    }
    Ok(())
}

/// A property's stored value, whether it is stored under its current name or an older one.
pub(crate) fn stored_value<'a>(
    row: &'a ConversionRow,
    key: &str,
    property_id: &str,
) -> Option<&'a Value> {
    let matches_id = |value: &&Value| value.get("id").and_then(Value::as_str) == Some(property_id);
    row.properties
        .get(key)
        .filter(matches_id)
        .or_else(|| row.properties.values().find(matches_id))
}

pub(crate) fn insert_value(row: &mut ConversionRow, key: &str, value: Value) {
    row.properties.insert(key.to_string(), value);
}

/// Remove a property's stored value from a row, whether it is stored under its current name or an older one.
pub(crate) fn remove_value(row: &mut ConversionRow, key: &str, property_id: &str) -> Option<Value> {
    let stored = row.properties.remove(key);
    let matches_id = |value: &Value| value.get("id").and_then(Value::as_str) == Some(property_id);
    if stored.as_ref().is_some_and(matches_id) {
        return stored;
    }
    if let Some(value) = stored {
        row.properties.insert(key.to_string(), value);
    }
    let other_key = row
        .properties
        .iter()
        .find(|(_, value)| matches_id(value))
        .map(|(key, _)| key.clone())?;
    row.properties.remove(&other_key)
}

/// Read a stored value as the text items it would contribute to another type.
pub(crate) fn value_items(
    property_type: &str,
    value: Option<&Value>,
    row: &ConversionRow,
) -> Vec<String> {
    let payload = value.and_then(|value| value.get(property_type));
    let items = match property_type {
        "title" | "rich_text" => payload
            .and_then(Value::as_array)
            .map(|items| vec![rich_text_items_plain_text(items)])
            .unwrap_or_default(),
        "number" => payload
            .and_then(Value::as_f64)
            .map(|number| vec![number_text(number)])
            .unwrap_or_default(),
        "checkbox" => match payload.and_then(Value::as_bool) {
            Some(true) => vec!["true".to_string()],
            _ => Vec::new(),
        },
        "select" | "status" | "place" | "created_by" | "last_edited_by" => {
            payload.and_then(named_text).into_iter().collect()
        }
        "multi_select" | "files" | "people" => payload
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(named_text).collect())
            .unwrap_or_default(),
        "date" => payload.and_then(date_text).into_iter().collect(),
        "url" | "email" | "phone_number" => payload
            .and_then(Value::as_str)
            .map(str::to_string)
            .into_iter()
            .collect(),
        "created_time" => vec![row.created_time.clone()],
        "last_edited_time" => vec![row.last_edited_time.clone()],
        "unique_id" => payload.and_then(unique_id_text).into_iter().collect(),
        _ => Vec::new(),
    };
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Shape text items for the target type: option types split free text on commas, single options keep the first.
pub(crate) fn target_items(
    source_type: &str,
    target_type: &str,
    items: Vec<String>,
) -> Vec<String> {
    if !OPTION_TYPES.contains(&target_type) {
        return items;
    }
    let mut names: Vec<String> = if TEXT_TYPES.contains(&source_type) {
        items
            .iter()
            .flat_map(|item| item.split(','))
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        items
    };
    if target_type != "multi_select" {
        names.truncate(1);
    }
    names
}

/// Option definitions a select-like target starts with: the old property's options, then each new name in row order.
///
/// New options have no id yet; the caller assigns one. Names that cannot label an option are skipped, so their
/// rows convert to an empty value.
pub(crate) fn seeded_options<'a>(
    old_property: &Value,
    target_type: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Vec<Value> {
    let mut options = existing_options(old_property, target_type == "status");
    let mut seen: HashSet<String> = options
        .iter()
        .filter_map(|option| option.get("name").and_then(Value::as_str))
        .map(str::to_lowercase)
        .collect();
    for name in names {
        if options.len() >= data_sources::schema::MAX_OPTIONS {
            break;
        }
        if !data_sources::schema::is_valid_option_name(name) || !seen.insert(name.to_lowercase()) {
            continue;
        }
        let mut option = json!({ "name": name });
        if target_type == "status" {
            option["group"] = json!("To-do");
        }
        options.push(option);
    }
    options
}

/// The stored value for one row under the converted property, or nothing for a computed type.
pub(crate) fn converted_value(property: &TableProperty, items: &[String]) -> Option<Value> {
    if COMPUTED_TYPES.contains(&property.property_type.as_str()) {
        return None;
    }
    let input = match property.property_type.as_str() {
        "checkbox" => Some(Value::Bool(items.iter().any(|item| {
            CHECKED_TEXT
                .iter()
                .any(|checked| item.eq_ignore_ascii_case(checked))
        }))),
        "multi_select" => Some(json!(items)),
        "number" | "select" | "status" | "date" | "place" => {
            items.first().cloned().map(Value::String)
        }
        "title" | "rich_text" | "url" | "email" | "phone_number" => {
            (!items.is_empty()).then(|| Value::String(items.join(", ")))
        }
        _ => None,
    };
    let converted =
        input.and_then(|input| table::property_value_from_edit(property, &input, None).ok());
    Some(converted.unwrap_or_else(|| table::default_property_value(property, "")))
}

fn existing_options(old_property: &Value, keep_groups: bool) -> Vec<Value> {
    let Some(old_type) = old_property
        .get("type")
        .and_then(Value::as_str)
        .filter(|old_type| OPTION_TYPES.contains(old_type))
    else {
        return Vec::new();
    };
    let config = old_property.get(old_type);
    let groups = config
        .and_then(|config| config.get("groups"))
        .and_then(Value::as_array);
    config
        .and_then(|config| config.get("options"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|option| {
            let mut next = json!({
                "id": option.get("id").cloned().unwrap_or(Value::Null),
                "name": option.get("name").cloned().unwrap_or(Value::Null),
                "color": option.get("color").cloned().unwrap_or(json!("default"))
            });
            if keep_groups {
                next["group"] = json!(option_group(option, groups).unwrap_or("To-do"));
            }
            next
        })
        .collect()
}

fn option_group<'a>(option: &Value, groups: Option<&'a Vec<Value>>) -> Option<&'a str> {
    let id = option.get("id")?;
    groups?
        .iter()
        .find(|group| {
            group
                .get("option_ids")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.contains(id))
        })
        .and_then(|group| group.get("name"))
        .and_then(Value::as_str)
}

fn named_text(value: &Value) -> Option<String> {
    ["name", "title", "url", "id"]
        .iter()
        .find_map(|field| value.get(field).and_then(Value::as_str))
        .map(str::to_string)
}

fn date_text(value: &Value) -> Option<String> {
    let start = value.get("start").and_then(Value::as_str)?;
    match value.get("end").and_then(Value::as_str) {
        Some(end) if !end.is_empty() && end != start => Some(format!("{start} to {end}")),
        _ => Some(start.to_string()),
    }
}

fn unique_id_text(value: &Value) -> Option<String> {
    let number = value.get("number").and_then(Value::as_i64)?;
    let prefix = value
        .get("prefix")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(format!("{prefix}{number}"))
}

fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(properties: Value) -> ConversionRow {
        ConversionRow {
            id: "row".to_string(),
            properties: properties.as_object().cloned().unwrap_or_default(),
            created_time: "2026-01-02T03:04:05.000Z".to_string(),
            last_edited_time: "2026-01-03T03:04:05.000Z".to_string(),
        }
    }

    fn property(property_type: &str, config: Value) -> TableProperty {
        TableProperty {
            key: "Field".to_string(),
            id: "field".to_string(),
            name: "Field".to_string(),
            property_type: property_type.to_string(),
            schema: json!({ "id": "field", "name": "Field", "type": property_type, property_type: config }),
        }
    }

    #[test]
    fn value_items_read_each_stored_shape_as_trimmed_text() {
        let empty = row(json!({}));
        assert_eq!(
            value_items(
                "rich_text",
                Some(&json!({"rich_text": [{"plain_text": " Plan "}]})),
                &empty
            ),
            vec!["Plan"]
        );
        assert_eq!(
            value_items("number", Some(&json!({"number": 3.0})), &empty),
            vec!["3"]
        );
        assert_eq!(
            value_items("number", Some(&json!({"number": 2.5})), &empty),
            vec!["2.5"]
        );
        assert_eq!(
            value_items("checkbox", Some(&json!({"checkbox": false})), &empty),
            Vec::<String>::new()
        );
        assert_eq!(
            value_items(
                "multi_select",
                Some(
                    &json!({"multi_select": [{"id": "a", "name": "A"}, {"id": "b", "name": "B"}]})
                ),
                &empty
            ),
            vec!["A", "B"]
        );
        assert_eq!(
            value_items(
                "date",
                Some(&json!({"date": {"start": "2026-01-01", "end": "2026-01-05"}})),
                &empty
            ),
            vec!["2026-01-01 to 2026-01-05"]
        );
        assert_eq!(
            value_items("created_time", None, &empty),
            vec!["2026-01-02T03:04:05.000Z"]
        );
        assert_eq!(
            value_items(
                "unique_id",
                Some(&json!({"unique_id": {"prefix": null, "number": null}})),
                &empty
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            value_items(
                "relation",
                Some(&json!({"relation": [{"id": "x"}]})),
                &empty
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn target_items_split_free_text_only_for_option_targets() {
        let items = vec!["Red, Blue".to_string()];
        assert_eq!(
            target_items("rich_text", "multi_select", items.clone()),
            vec!["Red", "Blue"]
        );
        assert_eq!(
            target_items("rich_text", "select", items.clone()),
            vec!["Red"]
        );
        assert_eq!(
            target_items("rich_text", "number", items.clone()),
            vec!["Red, Blue"]
        );
        let tags = vec!["A".to_string(), "B".to_string()];
        assert_eq!(target_items("multi_select", "status", tags), vec!["A"]);
    }

    #[test]
    fn seeded_options_keep_existing_identities_and_skip_invalid_or_repeated_names() {
        let old = json!({
            "type": "status",
            "status": {
                "options": [{"id": "doing", "name": "Doing", "color": "blue"}],
                "groups": [{"name": "In progress", "option_ids": ["doing"]}]
            }
        });
        let options = seeded_options(&old, "status", ["doing", "Later", "", "Later", "a,b"]);
        assert_eq!(
            options,
            vec![
                json!({"id": "doing", "name": "Doing", "color": "blue", "group": "In progress"}),
                json!({"name": "Later", "group": "To-do"}),
            ]
        );
        let from_text = seeded_options(
            &json!({"type": "rich_text", "rich_text": {}}),
            "select",
            ["One"],
        );
        assert_eq!(from_text, vec![json!({"name": "One"})]);
    }

    #[test]
    fn converted_values_follow_cell_edit_rules_and_fall_back_to_empty() {
        assert_eq!(
            converted_value(
                &property("number", json!({"format": "number"})),
                &["12.5".to_string()]
            ),
            Some(json!({"id": "field", "type": "number", "number": 12.5}))
        );
        assert_eq!(
            converted_value(
                &property("number", json!({"format": "number"})),
                &["soon".to_string()]
            ),
            Some(json!({"id": "field", "type": "number", "number": null}))
        );
        assert_eq!(
            converted_value(&property("checkbox", json!({})), &["Yes".to_string()]),
            Some(json!({"id": "field", "type": "checkbox", "checkbox": true}))
        );
        let select = property(
            "select",
            json!({"options": [{"id": "one", "name": "One", "color": "red"}]}),
        );
        assert_eq!(
            converted_value(&select, &["one".to_string()]),
            Some(
                json!({"id": "field", "type": "select", "select": {"id": "one", "name": "One", "color": "red"}})
            )
        );
        assert_eq!(
            converted_value(&property("date", json!({})), &["2026-02-03".to_string()]),
            Some(
                json!({"id": "field", "type": "date", "date": {"start": "2026-02-03", "end": null, "time_zone": null}})
            )
        );
        assert_eq!(
            converted_value(&property("formula", json!({"expression": "1"})), &[]),
            None
        );
    }

    #[test]
    fn remove_value_finds_a_value_stored_under_an_older_name() {
        let mut stored = row(json!({
            "Old": {"id": "field", "type": "number", "number": 1},
            "Field": {"id": "other", "type": "number", "number": 2}
        }));
        assert_eq!(
            remove_value(&mut stored, "Field", "field"),
            Some(json!({"id": "field", "type": "number", "number": 1}))
        );
        assert_eq!(stored.properties.len(), 1);
        assert!(stored.properties.contains_key("Field"));
    }
}
