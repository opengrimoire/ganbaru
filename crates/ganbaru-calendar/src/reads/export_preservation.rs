//! Bounded batch reconstruction of the preservation selected for one Calendar export.

use ganbaru_db::impl_sqlite_from_row;
use std::cell::Cell;
use std::collections::BTreeMap;

use serde_json::{Map, Value};
use sqlx::SqliteConnection;

use super::export::{ExportBudget, MAX_EXPORT_BYTES};

const MAX_TREE_DEPTH: usize = 32;

/// Reserve UTF-8 bytes after the component and its enclosing IPC string are escaped.
fn string_reservation(value: &str) -> usize {
    value.bytes().fold(16usize, |total, byte| {
        total.saturating_add(match byte {
            b'"' | b'\\' => 4,
            b'\n' | b'\r' | b'\t' | 8 | 12 => 3,
            0..=31 => 7,
            _ => 1,
        })
    })
}

// A retained archive envelope is storage custody, not an active import. Include
// shared envelopes while a live projection uses them and standalone imports
// with no archive owner. Native archival records every retained object owner.
const EXPORT_OBJECT_SCOPE: &str = r#"
WITH RECURSIVE export_objects(id) AS (
    SELECT object.id FROM icalendar_objects object WHERE object.calendar_id = ?1 AND (
        EXISTS(SELECT 1 FROM icalendar_components component
            JOIN calendar_events event ON event.icalendar_component_id = component.id
            WHERE component.object_id = object.id AND event.calendar_id = ?1)
        OR EXISTS(SELECT 1 FROM icalendar_components component
            JOIN calendar_event_overrides override ON override.icalendar_component_id = component.id
            JOIN calendar_events event ON event.id = override.parent_event_id
            WHERE component.object_id = object.id AND event.calendar_id = ?1)
        OR (
            NOT EXISTS(SELECT 1 FROM calendar_event_archive_import_objects owner WHERE owner.object_id = object.id)
        )
    )
)
"#;

// Select live event/override roots and preserved top-level non-event components.
// An unprojected or archived VEVENT must never reappear merely because it was preserved.
const COMPONENT_SCOPE: &str = r#"
, selected(id) AS (
    SELECT c.id FROM icalendar_components c
    JOIN export_objects object ON object.id = c.object_id
    LEFT JOIN icalendar_components parent ON parent.id = c.parent_component_id
    WHERE c.calendar_id = ?1 AND (
        c.id IN (SELECT icalendar_component_id FROM calendar_events WHERE calendar_id = ?1)
        OR c.id IN (SELECT o.icalendar_component_id FROM calendar_event_overrides o
            JOIN calendar_events e ON e.id = o.parent_event_id WHERE e.calendar_id = ?1)
        OR c.component_type = 'vtimezone'
        OR (parent.component_type = 'vcalendar' AND c.component_type NOT IN ('vevent', 'vtimezone'))
    )
    UNION
    SELECT child.id FROM icalendar_components child JOIN selected ON child.parent_component_id = selected.id
    WHERE child.calendar_id = ?1
)
"#;

struct Component {
    id: String,
    parent_component_id: Option<String>,
    component_type: String,
    parent_type: Option<String>,
}
impl_sqlite_from_row!(Component {
    id,
    parent_component_id,
    component_type,
    parent_type
});

struct Property {
    id: String,
    component_id: String,
    name: String,
    value_type: String,
}
impl_sqlite_from_row!(Property {
    id,
    component_id,
    name,
    value_type
});

struct Parameter {
    id: String,
    property_id: String,
    name: String,
}
impl_sqlite_from_row!(Parameter {
    id,
    property_id,
    name
});

struct Node {
    id: String,
    property_id: Option<String>,
    parameter_id: Option<String>,
    parent_node_id: Option<String>,
    value_kind: String,
    object_key: Option<String>,
    text_value: Option<String>,
    number_value: Option<f64>,
    boolean_value: Option<i64>,
}
impl_sqlite_from_row!(Node {
    id,
    property_id,
    parameter_id,
    parent_node_id,
    value_kind,
    object_key,
    text_value,
    number_value,
    boolean_value
});

struct Warning {
    component_id: String,
    message: String,
}
impl_sqlite_from_row!(Warning {
    component_id,
    message
});

struct Method {
    method: String,
}
impl_sqlite_from_row!(Method { method });

pub(super) struct ExportPreservation {
    components: BTreeMap<String, Component>,
    children: BTreeMap<String, Vec<String>>,
    properties: BTreeMap<String, Vec<Property>>,
    parameters: BTreeMap<String, Vec<Parameter>>,
    nodes: BTreeMap<String, Node>,
    node_children: BTreeMap<String, Vec<String>>,
    property_roots: BTreeMap<String, Vec<String>>,
    parameter_roots: BTreeMap<String, Vec<String>>,
    warnings: BTreeMap<String, Vec<String>>,
    timezone_ids: Vec<String>,
    passthrough_ids: Vec<String>,
    pub(super) methods: Vec<String>,
    assembly_bytes_left: Cell<usize>,
}

pub(super) struct ExportPreservationRows {
    components: Vec<Component>,
    properties: Vec<Property>,
    parameters: Vec<Parameter>,
    nodes: Vec<Node>,
    warnings: Vec<Warning>,
    methods: Vec<Method>,
}

impl ExportPreservationRows {
    pub(super) async fn read(
        connection: &mut SqliteConnection,
        calendar_id: &str,
        budget: &mut ExportBudget,
    ) -> Result<Self, String> {
        let components: Vec<Component> = budget.read(connection, calendar_id,
            &format!("{EXPORT_OBJECT_SCOPE}{COMPONENT_SCOPE} SELECT c.id, c.parent_component_id, c.component_type, parent.component_type AS parent_type
                FROM icalendar_components c JOIN selected ON selected.id = c.id
                LEFT JOIN icalendar_components parent ON parent.id = c.parent_component_id
                ORDER BY c.object_id, c.sort_order, c.id"),
            &["id", "parent_component_id", "component_type", "parent_type"],
        ).await?;
        let properties: Vec<Property> = budget.read(connection, calendar_id,
            &format!("{EXPORT_OBJECT_SCOPE}{COMPONENT_SCOPE} SELECT p.id, p.component_id, p.name, p.value_type FROM icalendar_component_properties p
                JOIN selected ON selected.id = p.component_id ORDER BY p.component_id, p.sort_order, p.id"),
            &["id", "component_id", "name", "value_type"],
        ).await?;
        let parameters: Vec<Parameter> = budget.read(connection, calendar_id,
            &format!("{EXPORT_OBJECT_SCOPE}{COMPONENT_SCOPE} SELECT p.id, p.property_id, p.name FROM icalendar_property_parameters p
                JOIN icalendar_component_properties property ON property.id = p.property_id
                JOIN selected ON selected.id = property.component_id ORDER BY p.property_id, p.sort_order, p.id"),
            &["id", "property_id", "name"],
        ).await?;
        let nodes: Vec<Node> = budget.read(connection, calendar_id,
            &format!("{EXPORT_OBJECT_SCOPE}{COMPONENT_SCOPE} SELECT n.* FROM icalendar_value_nodes n
                WHERE n.property_id IN (SELECT p.id FROM icalendar_component_properties p JOIN selected ON selected.id = p.component_id)
                OR n.parameter_id IN (SELECT param.id FROM icalendar_property_parameters param
                    JOIN icalendar_component_properties p ON p.id = param.property_id JOIN selected ON selected.id = p.component_id)
                ORDER BY n.sort_order, n.id"),
            &["id", "property_id", "parameter_id", "parent_node_id", "value_kind", "object_key", "text_value"],
        ).await?;
        let warnings: Vec<Warning> = budget.read(connection, calendar_id,
            &format!("{EXPORT_OBJECT_SCOPE}{COMPONENT_SCOPE} SELECT w.component_id, w.message FROM icalendar_component_projection_warnings w
                JOIN selected ON selected.id = w.component_id ORDER BY w.component_id, w.sort_order, w.id"),
            &["component_id", "message"],
        ).await?;
        let methods: Vec<Method> = budget.read(connection, calendar_id,
            &format!("{EXPORT_OBJECT_SCOPE} SELECT object.method FROM icalendar_objects object
                JOIN export_objects selected ON selected.id = object.id WHERE object.method IS NOT NULL ORDER BY object.id"),
            &["method"],
        ).await?;
        Ok(Self {
            components,
            properties,
            parameters,
            nodes,
            warnings,
            methods,
        })
    }

    /// Build indexes and check graph structure only after the read transaction is released.
    pub(super) fn assemble(self) -> Result<ExportPreservation, String> {
        let Self {
            components,
            properties,
            parameters,
            nodes,
            warnings,
            methods,
        } = self;
        let mut result = ExportPreservation {
            components: BTreeMap::new(),
            children: BTreeMap::new(),
            properties: BTreeMap::new(),
            parameters: BTreeMap::new(),
            nodes: BTreeMap::new(),
            node_children: BTreeMap::new(),
            property_roots: BTreeMap::new(),
            parameter_roots: BTreeMap::new(),
            warnings: BTreeMap::new(),
            timezone_ids: Vec::new(),
            passthrough_ids: Vec::new(),
            methods: methods.into_iter().map(|row| row.method).collect(),
            assembly_bytes_left: Cell::new(MAX_EXPORT_BYTES),
        };
        for row in components {
            if row.component_type == "vtimezone" {
                result.timezone_ids.push(row.id.clone());
            }
            if row.parent_type.as_deref() == Some("vcalendar")
                && !matches!(row.component_type.as_str(), "vevent" | "vtimezone")
            {
                result.passthrough_ids.push(row.id.clone());
            }
            if let Some(parent) = &row.parent_component_id {
                result
                    .children
                    .entry(parent.clone())
                    .or_default()
                    .push(row.id.clone());
            }
            result.components.insert(row.id.clone(), row);
        }
        for row in properties {
            result
                .properties
                .entry(row.component_id.clone())
                .or_default()
                .push(row);
        }
        for row in parameters {
            result
                .parameters
                .entry(row.property_id.clone())
                .or_default()
                .push(row);
        }
        for row in warnings {
            result
                .warnings
                .entry(row.component_id)
                .or_default()
                .push(row.message);
        }
        for row in nodes {
            if let Some(parent) = &row.parent_node_id {
                result
                    .node_children
                    .entry(parent.clone())
                    .or_default()
                    .push(row.id.clone());
            } else if let (Some(property), None) = (&row.property_id, &row.parameter_id) {
                result
                    .property_roots
                    .entry(property.clone())
                    .or_default()
                    .push(row.id.clone());
            } else if let (None, Some(parameter)) = (&row.property_id, &row.parameter_id) {
                result
                    .parameter_roots
                    .entry(parameter.clone())
                    .or_default()
                    .push(row.id.clone());
            } else {
                return Err("Invalid iCalendar value owner in Calendar export".to_string());
            }
            result.nodes.insert(row.id.clone(), row);
        }
        result.validate_nodes()?;
        Ok(result)
    }
}

impl ExportPreservation {
    fn validate_nodes(&self) -> Result<(), String> {
        for node in self.nodes.values() {
            let mut cursor = node;
            let mut depth = 0;
            while let Some(parent_id) = &cursor.parent_node_id {
                depth += 1;
                if depth >= MAX_TREE_DEPTH {
                    return Err(
                        "Calendar export preservation exceeds its tree depth limit".to_string()
                    );
                }
                let parent = self
                    .nodes
                    .get(parent_id)
                    .ok_or("Missing iCalendar value parent in Calendar export")?;
                if parent.property_id != node.property_id
                    || parent.parameter_id != node.parameter_id
                    || !matches!(parent.value_kind.as_str(), "array" | "object")
                {
                    return Err("Invalid iCalendar value parent in Calendar export".to_string());
                }
                cursor = parent;
            }
        }
        Ok(())
    }

    fn charge(&self, bytes: usize) -> Result<(), String> {
        let remaining = self
            .assembly_bytes_left
            .get()
            .checked_sub(bytes)
            .ok_or("Calendar export preservation exceeds its byte limit")?;
        self.assembly_bytes_left.set(remaining);
        Ok(())
    }

    fn string(&self, value: &str) -> Result<Value, String> {
        // Reserve before cloning for JSON control escapes and the second escaping
        // layer when a component JSON string is placed in the IPC snapshot.
        self.charge(string_reservation(value))?;
        Ok(Value::String(value.to_string()))
    }

    fn values(&self, ids: Option<&Vec<String>>, depth: usize) -> Result<Vec<Value>, String> {
        ids.into_iter()
            .flatten()
            .map(|id| self.node(id, depth))
            .collect()
    }

    fn node(&self, id: &str, depth: usize) -> Result<Value, String> {
        if depth >= MAX_TREE_DEPTH {
            return Err("Calendar export preservation exceeds its tree depth limit".to_string());
        }
        self.charge(32)?;
        let node = self
            .nodes
            .get(id)
            .ok_or("Missing iCalendar value in Calendar export")?;
        match node.value_kind.as_str() {
            "array" => Ok(Value::Array(
                self.values(self.node_children.get(id), depth + 1)?,
            )),
            "object" => {
                let mut object = Map::new();
                for child_id in self.node_children.get(id).into_iter().flatten() {
                    let child = self
                        .nodes
                        .get(child_id)
                        .ok_or("Missing iCalendar object child")?;
                    let key = child
                        .object_key
                        .as_ref()
                        .ok_or("Missing iCalendar object key")?;
                    self.charge(string_reservation(key))?;
                    object.insert(key.clone(), self.node(child_id, depth + 1)?);
                }
                Ok(Value::Object(object))
            }
            "text" => self.string(node.text_value.as_deref().unwrap_or_default()),
            "number" => serde_json::Number::from_f64(node.number_value.unwrap_or(0.0))
                .map(Value::Number)
                .ok_or_else(|| "Invalid iCalendar number in Calendar export".to_string()),
            "boolean" => Ok(Value::Bool(node.boolean_value.unwrap_or(0) != 0)),
            "null" => Ok(Value::Null),
            kind => Err(format!("Unsupported Calendar export value kind: {kind}")),
        }
    }

    fn component(&self, id: &str, depth: usize) -> Result<Value, String> {
        if depth >= MAX_TREE_DEPTH {
            return Err("Calendar export preservation exceeds its tree depth limit".to_string());
        }
        self.charge(64)?;
        let component = self
            .components
            .get(id)
            .ok_or("Missing live iCalendar component in Calendar export")?;
        let mut properties = Vec::new();
        for property in self.properties.get(id).into_iter().flatten() {
            let mut parameters = Map::new();
            for parameter in self.parameters.get(&property.id).into_iter().flatten() {
                let mut values = self.values(self.parameter_roots.get(&parameter.id), 0)?;
                let value = if values.len() == 1 {
                    values.remove(0)
                } else {
                    Value::Array(values)
                };
                self.charge(string_reservation(&parameter.name))?;
                parameters.insert(parameter.name.clone(), value);
            }
            let mut value = vec![
                self.string(&property.name)?,
                Value::Object(parameters),
                self.string(&property.value_type)?,
            ];
            value.extend(self.values(self.property_roots.get(&property.id), 0)?);
            properties.push(Value::Array(value));
        }
        let children = self
            .children
            .get(id)
            .into_iter()
            .flatten()
            .map(|child| self.component(child, depth + 1))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::Array(vec![
            self.string(&component.component_type)?,
            Value::Array(properties),
            Value::Array(children),
        ]))
    }

    pub(super) fn component_json(&self, id: Option<&str>) -> Result<Option<String>, String> {
        id.map(|id| {
            self.component(id, 0)
                .and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()))
        })
        .transpose()
    }

    pub(super) fn warnings_json(&self, id: Option<&str>) -> Result<Option<String>, String> {
        id.and_then(|id| self.warnings.get(id))
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| error.to_string())
    }

    pub(super) fn timezones(&self) -> Result<Vec<Value>, String> {
        self.timezone_ids
            .iter()
            .map(|id| self.component(id, 0))
            .collect()
    }

    pub(super) fn passthrough(&self) -> Result<Vec<Value>, String> {
        self.passthrough_ids
            .iter()
            .map(|id| self.component(id, 0))
            .collect()
    }
}
