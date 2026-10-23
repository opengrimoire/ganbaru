//! Planned rows of one preserved component's property graph and of one object's diagnostics,
//! bounded to one Calendar sync value each before they are written.
//!
//! A component's projection warnings, properties, parameters, and value nodes replicate as one
//! value, and so do an object's diagnostics. A property whose rows would push the graph over
//! [`MAX_VALUE_BYTES`](crate::value_bound::MAX_VALUE_BYTES) is dropped from preservation whole,
//! and a warning row reserved before any property is admitted counts the dropped properties.
//! Diagnostics past the bound are omitted and counted the same way.

use serde_json::Value;

use crate::value_bound::{VALUE_TOO_LARGE_CODE, ValueSize};

/// One `icalendar_component_properties` row.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PropertyRow {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) value_type: String,
    pub(super) sort_order: i64,
}

/// One `icalendar_property_parameters` row.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ParameterRow {
    pub(super) id: String,
    pub(super) property_id: String,
    pub(super) name: String,
    pub(super) sort_order: i64,
}

/// One `icalendar_value_nodes` row.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ValueNodeRow {
    pub(super) id: String,
    pub(super) property_id: String,
    pub(super) parameter_id: Option<String>,
    pub(super) parent_node_id: Option<String>,
    pub(super) sort_order: i64,
    pub(super) value_kind: &'static str,
    pub(super) object_key: Option<String>,
    pub(super) text_value: Option<String>,
    pub(super) number_value: Option<String>,
    pub(super) boolean_value: Option<i64>,
}

/// One property with every parameter and value node that belongs to it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlannedProperty {
    pub(super) property: PropertyRow,
    pub(super) parameters: Vec<ParameterRow>,
    pub(super) values: Vec<ValueNodeRow>,
}

/// The rows of one component's graph that fit its sync value.
#[derive(Debug, Default, PartialEq)]
pub(super) struct BoundedGraph {
    pub(super) warnings: Vec<String>,
    pub(super) properties: Vec<PlannedProperty>,
}

/// Longest message the reserved drop warning or omission diagnostic can have.
const OMISSION_MESSAGE_ALLOWANCE_BYTES: usize = 160;

/// Plans the property graph rows of a jCal component array. Properties that are not arrays of
/// at least a name, parameters, and a value type are skipped, as export cannot rebuild them.
pub(super) fn plan_graph(
    component_id: &str,
    raw_jcal: &str,
) -> Result<Vec<PlannedProperty>, String> {
    let value = serde_json::from_str::<Value>(raw_jcal)
        .map_err(|e| format!("parse preserved iCalendar component: {e}"))?;
    let component = value
        .as_array()
        .ok_or_else(|| "preserved iCalendar component must be an array".to_string())?;
    let properties = component
        .get(1)
        .and_then(Value::as_array)
        .ok_or_else(|| "preserved iCalendar component properties must be an array".to_string())?;
    let mut planned = Vec::new();
    for (property_index, property) in properties.iter().enumerate() {
        let Some(property_array) = property.as_array() else {
            continue;
        };
        if property_array.len() < 3 {
            continue;
        }
        let Some(name) = property_array.first().and_then(Value::as_str) else {
            continue;
        };
        let Some(value_type) = property_array.get(2).and_then(Value::as_str) else {
            continue;
        };
        let property_id = format!("{component_id}:property:{property_index}");
        let mut parameters = Vec::new();
        let mut values = Vec::new();
        if let Some(params) = property_array.get(1).and_then(Value::as_object) {
            for (param_index, (param_name, param_value)) in params.iter().enumerate() {
                let parameter_id = format!("{property_id}:parameter:{param_index}");
                plan_value_node(
                    &mut values,
                    param_value,
                    NodePlace {
                        property_id: &property_id,
                        parameter_id: Some(&parameter_id),
                        parent_node_id: None,
                        sort_order: 0,
                        node_id: format!("{parameter_id}:value"),
                        object_key: None,
                    },
                );
                parameters.push(ParameterRow {
                    id: parameter_id,
                    property_id: property_id.clone(),
                    name: param_name.clone(),
                    sort_order: param_index as i64,
                });
            }
        }
        for (value_index, property_value) in property_array.iter().skip(3).enumerate() {
            plan_value_node(
                &mut values,
                property_value,
                NodePlace {
                    property_id: &property_id,
                    parameter_id: None,
                    parent_node_id: None,
                    sort_order: value_index as i64,
                    node_id: format!("{property_id}:value:{value_index}"),
                    object_key: None,
                },
            );
        }
        planned.push(PlannedProperty {
            property: PropertyRow {
                id: property_id,
                name: name.to_string(),
                value_type: value_type.to_string(),
                sort_order: property_index as i64,
            },
            parameters,
            values,
        });
    }
    Ok(planned)
}

/// Where a value node sits in its property.
struct NodePlace<'a> {
    property_id: &'a str,
    parameter_id: Option<&'a str>,
    parent_node_id: Option<String>,
    sort_order: i64,
    node_id: String,
    object_key: Option<String>,
}

/// Plans a value node and its descendants depth first, parents before children. Depth is
/// bounded by the JSON parser's recursion limit.
fn plan_value_node(rows: &mut Vec<ValueNodeRow>, value: &Value, place: NodePlace<'_>) {
    let (value_kind, text_value, number_value, boolean_value) = match value {
        Value::Array(_) => ("array", None, None, None),
        Value::Object(_) => ("object", None, None, None),
        Value::String(text) => ("text", Some(text.clone()), None, None),
        Value::Number(number) => (
            "number",
            None,
            Some(crate::stored_number::encode(number)),
            None,
        ),
        Value::Bool(boolean) => ("boolean", None, None, Some(i64::from(*boolean))),
        Value::Null => ("null", None, None, None),
    };
    let node_id = place.node_id;
    rows.push(ValueNodeRow {
        id: node_id.clone(),
        property_id: place.property_id.to_string(),
        parameter_id: place.parameter_id.map(str::to_string),
        parent_node_id: place.parent_node_id,
        sort_order: place.sort_order,
        value_kind,
        object_key: place.object_key,
        text_value,
        number_value,
        boolean_value,
    });
    let children: Vec<(String, Option<String>, &Value)> = match value {
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, child)| (format!("{node_id}:child:{index}"), None, child))
            .collect(),
        Value::Object(map) => map
            .iter()
            .enumerate()
            .map(|(index, (key, child))| {
                (
                    format!("{node_id}:member:{index}"),
                    Some(key.clone()),
                    child,
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    for (index, (child_id, object_key, child)) in children.into_iter().enumerate() {
        plan_value_node(
            rows,
            child,
            NodePlace {
                property_id: place.property_id,
                parameter_id: place.parameter_id,
                parent_node_id: Some(node_id.clone()),
                sort_order: index as i64,
                node_id: child_id,
                object_key,
            },
        );
    }
}

/// Charges one message row of a warning or diagnostic list under `owner_id`.
fn charge_message(size: &mut ValueSize, owner_id: &str, message: &str) {
    size.row();
    size.generated_id();
    size.text(owner_id);
    size.text(message);
    size.integer();
}

/// Charges a reserved message row at the omission message allowance.
fn charge_reserved_message(size: &mut ValueSize, owner_id: &str) {
    charge_message(
        size,
        owner_id,
        &"x".repeat(OMISSION_MESSAGE_ALLOWANCE_BYTES),
    );
}

fn charge_property(size: &mut ValueSize, component_id: &str, planned: &PlannedProperty) {
    let row = &planned.property;
    size.row();
    size.text(&row.id);
    size.text(component_id);
    size.text(&row.name);
    size.text(&row.value_type);
    size.integer();
    for parameter in &planned.parameters {
        size.row();
        size.text(&parameter.id);
        size.text(component_id);
        size.text(&parameter.property_id);
        size.text(&parameter.name);
        size.integer();
    }
    for node in &planned.values {
        size.row();
        size.text(&node.id);
        size.text(component_id);
        size.text(&node.property_id);
        size.optional_text(node.parameter_id.as_deref());
        size.optional_text(node.parent_node_id.as_deref());
        size.integer();
        size.text(node.value_kind);
        size.optional_text(node.object_key.as_deref());
        size.optional_text(node.text_value.as_deref());
        size.optional_text(node.number_value.as_deref());
        size.optional_integer(node.boolean_value);
    }
}

/// The warning row that counts what a bound left out.
fn omission_message(count: usize, what: &str) -> String {
    let message = format!(
        "{VALUE_TOO_LARGE_CODE}: {count} {what} left out to keep this value within the sync size limit"
    );
    debug_assert!(message.len() <= OMISSION_MESSAGE_ALLOWANCE_BYTES);
    message
}

/// Keeps the warnings and properties of one component that fit its sync value, in order.
/// Warnings are admitted first, then each property whole or not at all; one reserved warning
/// counts whatever was left out.
pub(super) fn bound_graph(
    component_id: &str,
    warnings: Vec<String>,
    properties: Vec<PlannedProperty>,
) -> BoundedGraph {
    let mut size = ValueSize::default();
    charge_reserved_message(&mut size, component_id);
    let mut omitted = 0_usize;
    let mut bounded = BoundedGraph::default();
    for warning in warnings {
        let mut next = size;
        charge_message(&mut next, component_id, &warning);
        if next.fits() {
            size = next;
            bounded.warnings.push(warning);
        } else {
            omitted += 1;
        }
    }
    for property in properties {
        let mut next = size;
        charge_property(&mut next, component_id, &property);
        if next.fits() {
            size = next;
            bounded.properties.push(property);
        } else {
            omitted += 1;
        }
    }
    if omitted > 0 {
        bounded.warnings.push(omission_message(
            omitted,
            "preserved iCalendar properties or warnings",
        ));
    }
    bounded
}

/// Keeps the diagnostics of one object that fit its sync value, in order, with one reserved
/// diagnostic counting any left out.
pub(super) fn bound_diagnostics(object_id: &str, diagnostics: Vec<String>) -> Vec<String> {
    let mut size = ValueSize::default();
    charge_reserved_message(&mut size, object_id);
    let mut omitted = 0_usize;
    let mut kept = Vec::new();
    for diagnostic in diagnostics {
        let mut next = size;
        charge_message(&mut next, object_id, &diagnostic);
        if next.fits() {
            size = next;
            kept.push(diagnostic);
        } else {
            omitted += 1;
        }
    }
    if omitted > 0 {
        kept.push(omission_message(omitted, "import diagnostics"));
    }
    kept
}

/// Upper bound of a bounded graph's sync value, charged as [`bound_graph`] charges it.
#[cfg(test)]
pub(super) fn graph_size(component_id: &str, graph: &BoundedGraph) -> ValueSize {
    let mut size = ValueSize::default();
    for warning in &graph.warnings {
        charge_message(&mut size, component_id, warning);
    }
    for property in &graph.properties {
        charge_property(&mut size, component_id, property);
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_bound::MAX_VALUE_BYTES;

    fn jcal(property_values: &[String]) -> String {
        let properties = property_values
            .iter()
            .map(|value| serde_json::json!(["x-big", {"cn": "a"}, "text", value]))
            .collect::<Vec<_>>();
        serde_json::json!(["vevent", properties, []]).to_string()
    }

    #[test]
    fn plan_links_every_parameter_and_nested_value_to_its_property() {
        let raw = r#"["vevent",[["attendee",{"role":["a","b"]},"cal-address","mailto:a@example.com"],["geo",{},"float",[1.5,2]]],[]]"#;
        let planned = plan_graph("c", raw).unwrap();
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].property.id, "c:property:0");
        assert_eq!(planned[0].parameters[0].id, "c:property:0:parameter:0");
        let kinds = planned[1]
            .values
            .iter()
            .map(|node| {
                (
                    node.id.as_str(),
                    node.value_kind,
                    node.number_value.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                ("c:property:1:value:0", "array", None),
                ("c:property:1:value:0:child:0", "number", Some("1.5")),
                ("c:property:1:value:0:child:1", "number", Some("2")),
            ]
        );
        assert!(
            planned
                .iter()
                .flat_map(|property| &property.values)
                .all(|node| node.property_id.starts_with("c:property:"))
        );
    }

    #[test]
    fn a_graph_within_the_bound_is_kept_whole() {
        let planned = plan_graph(
            "c",
            &jcal(&std::array::from_fn::<String, 3, _>(|_| {
                "small".to_string()
            })),
        )
        .unwrap();
        let bounded = bound_graph("c", vec!["note".to_string()], planned.clone());
        assert_eq!(bounded.properties, planned);
        assert_eq!(bounded.warnings, ["note"]);
    }

    #[test]
    fn an_oversized_property_is_dropped_whole_and_counted() {
        let values = vec![
            "first".to_string(),
            "x".repeat(MAX_VALUE_BYTES),
            "third".to_string(),
        ];
        let planned = plan_graph("c", &jcal(&values)).unwrap();
        let bounded = bound_graph("c", Vec::new(), planned);
        let kept = bounded
            .properties
            .iter()
            .map(|property| property.property.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(kept, ["c:property:0", "c:property:2"]);
        assert_eq!(bounded.warnings.len(), 1);
        assert!(bounded.warnings[0].starts_with(VALUE_TOO_LARGE_CODE));
        assert!(bounded.warnings[0].contains(" 1 "));
        assert!(graph_size("c", &bounded).fits());
    }

    #[test]
    fn many_properties_fill_the_bound_and_the_rest_is_counted() {
        let values = vec!["v".repeat(4_000); 100];
        let planned = plan_graph("c", &jcal(&values)).unwrap();
        let bounded = bound_graph("c", Vec::new(), planned);
        assert!(bounded.properties.len() < 100);
        assert!(bounded.properties.len() > 50);
        assert!(graph_size("c", &bounded).fits());
        let dropped = 100 - bounded.properties.len();
        assert!(bounded.warnings[0].contains(&format!(" {dropped} ")));
    }

    #[test]
    fn diagnostics_past_the_bound_are_omitted_and_counted() {
        let diagnostics = vec!["d".repeat(10_000); 40];
        let kept = bound_diagnostics("o", diagnostics);
        assert!(kept.len() < 40);
        let last = kept.last().unwrap();
        assert!(last.starts_with(VALUE_TOO_LARGE_CODE), "{last}");
        let mut size = ValueSize::default();
        for message in &kept {
            charge_message(&mut size, "o", message);
        }
        assert!(size.fits());
        assert_eq!(bound_diagnostics("o", vec!["a".to_string()]), ["a"]);
    }
}
