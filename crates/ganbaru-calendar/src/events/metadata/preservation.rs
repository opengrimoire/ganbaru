//! Bounded imported component closure, without traversing unrelated calendar events.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use sqlx::SqliteConnection;

use super::super::occurrence::ReadBudget;
use super::rows::*;

const MAX_COMPONENT_DEPTH: usize = 32;

#[derive(Clone, Default, Serialize)]
pub(super) struct Preservation {
    pub(super) components: Vec<Component>,
    pub(super) properties: Vec<ComponentProperty>,
    pub(super) parameters: Vec<Parameter>,
    pub(super) nodes: Vec<Node>,
    pub(super) warnings: Vec<Warning>,
    pub(super) objects: Vec<Object>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) fn ids_json<'a>(ids: impl IntoIterator<Item = &'a str>) -> Result<String, String> {
    serde_json::to_string(&ids.into_iter().collect::<BTreeSet<_>>())
        .map_err(|error| format!("encode Calendar preservation selection: {error}"))
}

impl Preservation {
    /// Descendants are read one bounded layer at a time. A recursive SQL CTE
    /// could traverse an oversized or cyclic import before its outer LIMIT.
    pub(super) async fn read(
        connection: &mut SqliteConnection,
        roots: BTreeSet<String>,
        budget: &mut ReadBudget,
    ) -> Result<Self, String> {
        let root_ids = ids_json(roots.iter().map(String::as_str))?;
        let mut components = Component::read(connection, &root_ids, budget).await?;
        if components.len() != roots.len() {
            return Err("Calendar preservation references a missing component".into());
        }
        let mut visited = roots;
        // Retain the import envelope and timezone definitions, but never its
        // unrelated VEVENT siblings. Copies can then own an independent object.
        let object_ids = ids_json(components.iter().map(|row| row.object_id.as_str()))?;
        let context = budget.read::<(String,)>(connection, &object_ids,
            "SELECT id FROM icalendar_components WHERE object_id IN (SELECT value FROM json_each(?1)) AND component_type IN ('vcalendar', 'vtimezone')",
            &["id"]).await?;
        let context: Vec<_> = context
            .into_iter()
            .map(|row| row.0)
            .filter(|id| visited.insert(id.clone()))
            .collect();
        let context_ids = ids_json(context.iter().map(String::as_str))?;
        components.extend(Component::read(connection, &context_ids, budget).await?);
        let mut frontier = ids_json(
            components
                .iter()
                .filter(|row| row.component_type != "vcalendar")
                .map(|row| row.id.as_str()),
        )?;
        for depth in 0..=MAX_COMPONENT_DEPTH {
            let next = budget.read::<(String,)>(connection, &frontier,
                "SELECT id FROM icalendar_components WHERE parent_component_id IN (SELECT value FROM json_each(?1))",
                &["id"]).await?;
            if next.is_empty() {
                break;
            }
            if depth == MAX_COMPONENT_DEPTH {
                return Err("Calendar preservation exceeds its component depth budget".into());
            }
            // A referenced alarm can also be a descendant of the event root.
            // Already selected nodes are checked for cycles below, not reread.
            let next: Vec<String> = next
                .into_iter()
                .map(|row| row.0)
                .filter(|id| visited.insert(id.clone()))
                .collect();
            if next.is_empty() {
                break;
            }
            frontier = ids_json(next.iter().map(String::as_str))?;
            components.extend(Component::read(connection, &frontier, budget).await?);
        }
        components.sort_by(|a, b| a.id.cmp(&b.id));
        let component_ids = ids_json(components.iter().map(|row| row.id.as_str()))?;
        let properties = ComponentProperty::read(connection, &component_ids, budget).await?;
        let property_ids = ids_json(properties.iter().map(|row| row.id.as_str()))?;
        let parameters = Parameter::read(connection, &property_ids, budget).await?;
        let nodes = Node::read(connection, &property_ids, budget).await?;
        // Every stored descendant must carry the same property/parameter owner.
        // Without this check, malformed ownerless children would disappear from
        // an owner-based selection and be silently omitted by a copy.
        let node_ids = ids_json(nodes.iter().map(|row| row.id.as_str()))?;
        let unselected_children = budget.read::<(String,)>(connection, &node_ids,
            "SELECT id FROM icalendar_value_nodes WHERE parent_node_id IN (SELECT value FROM json_each(?1)) AND id NOT IN (SELECT value FROM json_each(?1))",
            &["id"]).await?;
        if !unselected_children.is_empty() {
            return Err("Calendar preservation value has an uncaptured child owner".into());
        }
        let warnings = Warning::read(connection, &component_ids, budget).await?;
        let object_ids = ids_json(components.iter().map(|row| row.object_id.as_str()))?;
        let objects = Object::read(connection, &object_ids, budget).await?;
        let diagnostics = Diagnostic::read(connection, &object_ids, budget).await?;
        Ok(Self {
            components,
            properties,
            parameters,
            nodes,
            warnings,
            objects,
            diagnostics,
        })
    }

    /// Run graph checks on the blocking preparation worker, after bounded reads.
    pub(super) fn validate(&self) -> Result<(), String> {
        let components: BTreeMap<_, _> = self
            .components
            .iter()
            .map(|row| (row.id.as_str(), row.parent_component_id.as_deref()))
            .collect();
        validate_parents(&components, true, "component")?;
        let nodes: BTreeMap<_, _> = self
            .nodes
            .iter()
            .map(|row| (row.id.as_str(), row.parent_node_id.as_deref()))
            .collect();
        validate_parents(&nodes, true, "value")?;
        let properties: BTreeSet<_> = self.properties.iter().map(|row| row.id.as_str()).collect();
        let parameters: BTreeMap<_, _> = self
            .parameters
            .iter()
            .map(|row| (row.id.as_str(), row.property_id.as_str()))
            .collect();
        let by_id: BTreeMap<_, _> = self
            .nodes
            .iter()
            .map(|row| (row.id.as_str(), row))
            .collect();
        for row in &self.nodes {
            if row.number_value.is_some_and(|value| !value.is_finite()) {
                return Err("Calendar preservation contains a non-finite numeric value".into());
            }
            let property = row.property_id.as_deref();
            let parameter = row.parameter_id.as_deref();
            if property.is_some() == parameter.is_some()
                || property.is_some_and(|id| !properties.contains(id))
                || parameter.is_some_and(|id| !parameters.contains_key(id))
            {
                return Err("Calendar preservation value has an invalid owner".into());
            }
            if let Some(parent) = row.parent_node_id.as_deref() {
                let parent = by_id
                    .get(parent)
                    .ok_or("Calendar preservation value has a missing parent")?;
                if parent.property_id != row.property_id || parent.parameter_id != row.parameter_id
                {
                    return Err("Calendar preservation value crosses property owners".into());
                }
            }
        }
        Ok(())
    }
}

fn validate_parents(
    parents: &BTreeMap<&str, Option<&str>>,
    require_parent: bool,
    kind: &str,
) -> Result<(), String> {
    for id in parents.keys() {
        let mut visited = BTreeSet::new();
        let mut next = Some(*id);
        while let Some(id) = next {
            if !visited.insert(id) {
                return Err(format!("Calendar preservation {kind} contains a cycle"));
            }
            if visited.len() > MAX_COMPONENT_DEPTH + 1 {
                return Err(format!(
                    "Calendar preservation {kind} exceeds its depth budget"
                ));
            }
            match parents.get(id) {
                Some(parent) => next = *parent,
                None if !require_parent => break,
                None => return Err(format!("Calendar preservation {kind} has a missing parent")),
            }
        }
    }
    Ok(())
}
