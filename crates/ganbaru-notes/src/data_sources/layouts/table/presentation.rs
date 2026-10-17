//! Saved table presentation and complete-source calculations.

use crate::data_sources;
use crate::data_sources::layouts::table::{TableProperty, normalized_row_for_schema};
use crate::models::{NoteDataSourceTableFilter, NoteDataSourceViewWindowRequest, NotePageRow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use std::collections::{BTreeMap, HashMap, HashSet};

/// View-owned presentation for each stable property identity.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TablePresentation {
    #[serde(default)]
    pub frozen_property_id: Option<String>,
    #[serde(default)]
    pub columns: BTreeMap<String, ColumnPresentation>,
    #[serde(default)]
    pub color_rules: Vec<ConditionalColorRule>,
}

/// Ordered view-owned colors evaluated using the saved query predicate contract.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalColorRule {
    pub id: String,
    pub property_id: Option<String>,
    pub color: String,
    pub filters: Vec<NoteDataSourceTableFilter>,
}

/// Column behavior independent of canonical source schema.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnPresentation {
    #[serde(default)]
    pub wrap: bool,
    #[serde(default)]
    pub date_format: DateFormat,
    #[serde(default)]
    pub time_format: TimeFormat,
    #[serde(default)]
    pub calculation: Option<Calculation>,
}

/// Supported date display formats.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    #[default]
    Locale,
    Iso,
    Relative,
}

/// Supported time display formats, including hiding the time.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeFormat {
    #[default]
    Locale,
    #[serde(rename = "12_hour")]
    TwelveHour,
    #[serde(rename = "24_hour")]
    TwentyFourHour,
    Hidden,
}

/// Calculations whose inputs and empty behavior are explicit.
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Calculation {
    CountAll,
    CountValues,
    Empty,
    Unique,
    Sum,
    Average,
    Min,
    Max,
    PercentChecked,
}

impl TablePresentation {
    /// Canonicalize predicate values with the same rules as the saved view query.
    pub(crate) fn canonical(
        &self,
        schema: &[TableProperty],
        hidden: &[String],
    ) -> Result<Self, String> {
        self.validate(schema, hidden)?;
        let mut presentation = self.clone();
        let types: HashMap<_, _> = schema
            .iter()
            .map(|property| (property.id.as_str(), property.property_type.as_str()))
            .collect();
        for rule in &mut presentation.color_rules {
            let canonical = crate::data_sources::views::canonical_filter(
                &rule.filters,
                &types,
                "conditional color",
            )?
            .ok_or_else(|| "conditional color requires a predicate".to_string())?;
            rule.filters = serde_json::from_value(canonical["filters"].clone())
                .map_err(|error| format!("canonicalize conditional color: {error}"))?;
        }
        Ok(presentation)
    }

    /// Validate stable references and type-compatible calculations before saving.
    pub(crate) fn validate(
        &self,
        schema: &[TableProperty],
        hidden: &[String],
    ) -> Result<(), String> {
        if self.color_rules.len() > 16 {
            return Err("table color rules are limited to 16".to_string());
        }
        let property_types: HashMap<_, _> = schema
            .iter()
            .map(|property| (property.id.as_str(), property.property_type.as_str()))
            .collect();
        let mut rule_ids = HashSet::new();
        for rule in &self.color_rules {
            if rule.id.is_empty()
                || rule.id.trim() != rule.id
                || rule.id.len() > 64
                || !rule_ids.insert(&rule.id)
            {
                return Err("table color rules require unique bounded identities".to_string());
            }
            if rule
                .property_id
                .as_deref()
                .is_some_and(|id| !property_types.contains_key(id))
            {
                return Err("table color target references an unknown property".to_string());
            }
            if ![
                "gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red",
            ]
            .contains(&rule.color.as_str())
            {
                return Err("unsupported table conditional color".to_string());
            }
            if rule.filters.is_empty() {
                return Err("table conditional color requires a predicate".to_string());
            }
            crate::data_sources::views::canonical_filter(
                &rule.filters,
                &property_types,
                "conditional color",
            )?;
        }
        if let Some(id) = &self.frozen_property_id {
            if !schema.iter().any(|property| &property.id == id) || hidden.contains(id) {
                return Err("frozen column must reference a visible property".to_string());
            }
        }
        for (id, presentation) in &self.columns {
            let property = schema
                .iter()
                .find(|property| &property.id == id)
                .ok_or_else(|| "column presentation references an unknown property".to_string())?;
            if let Some(calculation) = presentation.calculation {
                let compatible = match calculation {
                    Calculation::Sum
                    | Calculation::Average
                    | Calculation::Min
                    | Calculation::Max => matches!(
                        property.property_type.as_str(),
                        "number" | "formula" | "rollup"
                    ),
                    Calculation::PercentChecked => property.property_type == "checkbox",
                    _ => true,
                };
                if !compatible {
                    return Err(
                        "column calculation is incompatible with the property type".to_string()
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Aggregate {
    rows: i64,
    values: i64,
    numbers: i64,
    checked: i64,
    sum: f64,
    min: Option<f64>,
    max: Option<f64>,
    unique: HashSet<String>,
}

impl Aggregate {
    fn add(&mut self, payload: &Value, unique: bool) {
        self.rows += 1;
        let populated = match payload {
            Value::Null => false,
            Value::String(value) => !value.trim().is_empty(),
            Value::Array(value) => !value.is_empty(),
            _ => true,
        };
        if populated {
            self.values += 1;
            if unique {
                self.unique.insert(payload.to_string());
            }
        }
        if payload == &Value::Bool(true) {
            self.checked += 1;
        }
        if let Some(number) = payload.as_f64().filter(|value| value.is_finite()) {
            self.numbers += 1;
            self.sum += number;
            self.min = Some(self.min.map_or(number, |value| value.min(number)));
            self.max = Some(self.max.map_or(number, |value| value.max(number)));
        }
    }

    fn result(&self, calculation: Calculation) -> Value {
        match calculation {
            Calculation::CountAll => json!(self.rows),
            Calculation::CountValues => json!(self.values),
            Calculation::Empty => json!(self.rows - self.values),
            Calculation::Unique => json!(self.unique.len()),
            Calculation::Sum => json!(self.sum),
            Calculation::Average => {
                if self.numbers == 0 {
                    Value::Null
                } else {
                    json!(self.sum / self.numbers as f64)
                }
            }
            Calculation::Min => json!(self.min),
            Calculation::Max => json!(self.max),
            Calculation::PercentChecked => {
                if self.rows == 0 {
                    Value::Null
                } else {
                    json!(self.checked as f64 / self.rows as f64)
                }
            }
        }
    }
}

/// Stream the complete filtered source, hydrate computed values, and aggregate each group.
pub(crate) async fn calculations_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    schema: &[TableProperty],
    schema_properties: &Value,
    filters: &[NoteDataSourceTableFilter],
    presentation: &TablePresentation,
    group_property: Option<&TableProperty>,
) -> Result<Value, String> {
    let selected: Vec<_> = schema
        .iter()
        .filter_map(|property| {
            presentation
                .columns
                .get(&property.id)?
                .calculation
                .map(|calculation| (property, calculation))
        })
        .collect();
    if selected.is_empty() {
        return Ok(json!({ "overall": {}, "groups": {} }));
    }
    let mut overall: BTreeMap<String, Aggregate> = selected
        .iter()
        .map(|(property, _)| (property.id.clone(), Aggregate::default()))
        .collect();
    let mut groups: BTreeMap<String, BTreeMap<String, Aggregate>> = BTreeMap::new();
    let needs_computed = selected.iter().any(|(property, calculation)| {
        matches!(property.property_type.as_str(), "formula" | "rollup")
            && !matches!(calculation, Calculation::CountAll)
    });
    let mut cursor = None;
    loop {
        let request = NoteDataSourceViewWindowRequest {
            start_cursor: cursor,
            page_size: Some(data_sources::window::MAX_WINDOW_SIZE),
            ..Default::default()
        };
        let mut window = data_sources::window::load_row_page_tx(
            tx,
            data_source_id,
            data_sources::window::RowWindowQuery {
                schema,
                filters,
                sorts: &[],
                request: &request,
                date_property: None,
                group_property: None,
            },
        )
        .await?;
        window.rows = window
            .rows
            .into_iter()
            .map(|row| normalized_row_for_schema(row, schema))
            .collect::<Result<Vec<_>, _>>()?;
        if needs_computed {
            data_sources::rollups::hydrate_rollups_tx(
                tx,
                data_source_id,
                schema_properties,
                &mut window.rows,
            )
            .await?;
            data_sources::formulas::hydrate_formulas(schema_properties, &mut window.rows)?;
        }
        for row in &window.rows {
            let properties: Value = serde_json::from_str(&row.properties)
                .map_err(|error| format!("parse calculation row: {error}"))?;
            add_row_aggregates(&mut overall, &properties, row, &selected);
            if let Some(property) = group_property {
                for group in row_group_ids(row, property)? {
                    add_row_aggregates(
                        groups.entry(group).or_default(),
                        &properties,
                        row,
                        &selected,
                    );
                }
            }
        }
        if !window.has_more {
            break;
        }
        cursor = window.next_cursor;
        if cursor.is_none() {
            return Err("calculation window has no continuation cursor".to_string());
        }
    }
    let results = |columns: BTreeMap<String, Aggregate>| -> BTreeMap<String, Value> {
        selected
            .iter()
            .map(|(property, calculation)| {
                (
                    property.id.clone(),
                    columns
                        .get(&property.id)
                        .map_or(Value::Null, |aggregate| aggregate.result(*calculation)),
                )
            })
            .collect()
    };
    let group_results: BTreeMap<_, _> = groups
        .into_iter()
        .map(|(group, columns)| (group, results(columns)))
        .collect();
    Ok(json!({ "overall": results(overall), "groups": group_results }))
}

fn add_row_aggregates(
    aggregates: &mut BTreeMap<String, Aggregate>,
    properties: &Value,
    row: &NotePageRow,
    selected: &[(&TableProperty, Calculation)],
) {
    for (property, calculation) in selected {
        let payload = calculation_payload(properties, row, property);
        aggregates
            .entry(property.id.clone())
            .or_default()
            .add(&payload, matches!(calculation, Calculation::Unique));
    }
}

fn calculation_payload(properties: &Value, row: &NotePageRow, property: &TableProperty) -> Value {
    if property.property_type == "created_time" {
        return json!(row.created_time);
    }
    if property.property_type == "last_edited_time" {
        return json!(row.last_edited_time);
    }
    let payload = properties
        .get(&property.key)
        .and_then(|value| value.get(&property.property_type))
        .unwrap_or(&Value::Null);
    if matches!(property.property_type.as_str(), "formula" | "rollup") {
        let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");
        payload.get(kind).cloned().unwrap_or(Value::Null)
    } else if matches!(property.property_type.as_str(), "title" | "rich_text") {
        let text = payload
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("plain_text").and_then(Value::as_str))
                    .collect::<String>()
            })
            .unwrap_or_default();
        json!(text)
    } else if matches!(
        property.property_type.as_str(),
        "select" | "status" | "created_by" | "last_edited_by"
    ) {
        payload
            .get("id")
            .or_else(|| payload.get("name"))
            .cloned()
            .unwrap_or(Value::Null)
    } else if matches!(
        property.property_type.as_str(),
        "multi_select" | "people" | "relation"
    ) {
        let mut identities: Vec<String> = payload
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        item.get("id")
                            .or_else(|| item.get("name"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default();
        identities.sort();
        identities.dedup();
        json!(identities)
    } else {
        payload.clone()
    }
}

fn row_group_ids(row: &NotePageRow, property: &TableProperty) -> Result<Vec<String>, String> {
    let properties: Value = serde_json::from_str(&row.properties)
        .map_err(|error| format!("parse group calculation row: {error}"))?;
    let payload = properties
        .get(&property.key)
        .and_then(|value| value.get(&property.property_type))
        .unwrap_or(&Value::Null);
    let ids = match property.property_type.as_str() {
        "select" | "status" => payload
            .get("id")
            .and_then(Value::as_str)
            .map(|id| vec![id.to_string()])
            .unwrap_or_default(),
        "multi_select" | "people" | "relation" => payload
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        "checkbox" => vec![payload.as_bool().unwrap_or(false).to_string()],
        "date" => payload
            .get("start")
            .and_then(Value::as_str)
            .map(|id| vec![id.to_string()])
            .unwrap_or_default(),
        _ => vec!["__ungrouped__".to_string()],
    };
    let mut seen = HashSet::new();
    let mut ids: Vec<_> = ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect();
    if ids.is_empty() {
        ids.push("__empty__".to_string());
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculations_distinguish_empty_values_zero_false_and_non_numeric_values() {
        let mut aggregate = Aggregate::default();
        for value in [
            Value::Null,
            json!(" "),
            json!([]),
            json!(0),
            json!(false),
            json!(true),
            json!(3),
            json!("invalid number"),
            json!(3),
        ] {
            aggregate.add(&value, true);
        }
        assert_eq!(aggregate.result(Calculation::CountAll), json!(9));
        assert_eq!(aggregate.result(Calculation::CountValues), json!(6));
        assert_eq!(aggregate.result(Calculation::Empty), json!(3));
        assert_eq!(aggregate.result(Calculation::Unique), json!(5));
        assert_eq!(aggregate.result(Calculation::Sum), json!(6.0));
        assert_eq!(aggregate.result(Calculation::Average), json!(2.0));
        assert_eq!(aggregate.result(Calculation::Min), json!(0.0));
        assert_eq!(aggregate.result(Calculation::Max), json!(3.0));
        assert_eq!(
            aggregate.result(Calculation::PercentChecked),
            json!(1.0 / 9.0)
        );
        let empty = Aggregate::default();
        assert_eq!(empty.result(Calculation::Sum), json!(0.0));
        assert_eq!(empty.result(Calculation::Average), Value::Null);
        assert_eq!(empty.result(Calculation::PercentChecked), Value::Null);
    }
}
