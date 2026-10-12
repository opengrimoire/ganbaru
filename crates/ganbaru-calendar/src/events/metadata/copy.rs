//! Copy Calendar metadata with independent native and imported identities.

use super::rows::*;
use super::{Metadata, revision};
#[cfg(test)]
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
macro_rules! insert_row {
    ($connection:expr, $table:literal, $row:ident, { $($field:ident),+ }) => {{
        let columns = &[$(stringify!($field)),+];
        let sql = format!("INSERT INTO {} ({}) VALUES ({})", $table, columns.join(", "), vec!["?"; columns.len()].join(", "));
        sqlx::query(&sql)$(.bind(&$row.$field))+.execute(&mut *$connection).await
            .map_err(|error| format!("copy Calendar {}: {error}", $table))?;
    }};
}

pub(super) fn copied_id(target: &str, kind: &str, source: &str) -> Result<String, String> {
    Ok(format!(
        "calendar-copy-{}",
        revision(&(target, kind, source))?
    ))
}

fn mapped(
    ids: &BTreeMap<String, String>,
    value: &Option<String>,
) -> Result<Option<String>, String> {
    value
        .as_ref()
        .map(|id| {
            ids.get(id)
                .cloned()
                .ok_or_else(|| format!("Calendar copy has an uncaptured reference: {id}"))
        })
        .transpose()
}

/// Exact rows prepared before the write phase. Parent rows precede their children.
#[derive(Default)]
pub(super) struct PreparedCopy {
    component_id: Option<String>,
    objects: Vec<Object>,
    diagnostics: Vec<Diagnostic>,
    components: Vec<Component>,
    properties: Vec<ComponentProperty>,
    parameters: Vec<Parameter>,
    nodes: Vec<Node>,
    warnings: Vec<Warning>,
    alarms: Vec<Alarm>,
    attendees: Vec<Attendee>,
    categories: Vec<Category>,
    extensions: Vec<Property>,
    organizers: Vec<Organizer>,
    task_links: Vec<TaskLink>,
}

impl Metadata {
    /// Allocate and validate the complete imported copy on the admitted worker.
    #[cfg(test)]
    pub(super) fn prepare_base_copy(&self, target_row: &Event) -> Result<PreparedCopy, String> {
        self.prepare_copy(target_row, false)
    }

    fn prepare_copy(
        &self,
        target_row: &Event,
        include_overrides: bool,
    ) -> Result<PreparedCopy, String> {
        let target = target_row.id.as_str();
        let source = self
            .events
            .first()
            .ok_or("Calendar metadata source is missing")?;
        if source.id == target && !include_overrides {
            return Err("Calendar metadata requires a distinct copy target".into());
        }
        self.preservation.validate()?;
        let mut result = PreparedCopy::default();
        let mut included: BTreeSet<String> = source
            .icalendar_component_id
            .iter()
            .cloned()
            .chain(
                self.attendees
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .chain(
                self.alarms
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .collect();
        if include_overrides {
            included.extend(
                self.overrides
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            );
            // Capture contains only this source's roots and their descendants.
            // Retain cropped and promoted override provenance independently of
            // the original import, without projecting those roots as events.
            included.extend(
                self.preservation
                    .components
                    .iter()
                    .filter(|row| row.component_type == "vevent")
                    .map(|row| row.id.clone()),
            );
        }
        let objects: BTreeSet<&str> = self
            .preservation
            .components
            .iter()
            .filter(|row| included.contains(&row.id))
            .map(|row| row.object_id.as_str())
            .collect();
        let parents: BTreeMap<_, _> = self
            .preservation
            .components
            .iter()
            .map(|row| (row.id.as_str(), row.component_type.as_str()))
            .collect();
        for row in &self.preservation.components {
            if objects.contains(row.object_id.as_str())
                && matches!(row.component_type.as_str(), "vcalendar" | "vtimezone")
            {
                included.insert(row.id.clone());
            }
        }
        loop {
            let before = included.len();
            for row in &self.preservation.components {
                if row.parent_component_id.as_ref().is_some_and(|parent| {
                    included.contains(parent) && parents.get(parent.as_str()) != Some(&"vcalendar")
                }) {
                    included.insert(row.id.clone());
                }
            }
            if included.len() == before {
                break;
            }
        }
        let component_ids: BTreeMap<_, _> = included
            .iter()
            .map(|id| Ok((id.clone(), copied_id(target, "component", id)?)))
            .collect::<Result<_, String>>()?;
        let property_ids: BTreeMap<_, _> = self
            .preservation
            .properties
            .iter()
            .filter(|row| included.contains(&row.component_id))
            .map(|row| Ok((row.id.clone(), copied_id(target, "property", &row.id)?)))
            .collect::<Result<_, String>>()?;
        let parameter_ids: BTreeMap<_, _> = self
            .preservation
            .parameters
            .iter()
            .filter(|row| property_ids.contains_key(&row.property_id))
            .map(|row| Ok((row.id.clone(), copied_id(target, "parameter", &row.id)?)))
            .collect::<Result<_, String>>()?;
        let nodes: Vec<_> = self
            .preservation
            .nodes
            .iter()
            .filter(|row| {
                row.property_id
                    .as_ref()
                    .is_some_and(|id| property_ids.contains_key(id))
                    || row
                        .parameter_id
                        .as_ref()
                        .is_some_and(|id| parameter_ids.contains_key(id))
            })
            .collect();
        let node_ids: BTreeMap<_, _> = nodes
            .iter()
            .map(|row| Ok((row.id.clone(), copied_id(target, "node", &row.id)?)))
            .collect::<Result<_, String>>()?;
        for source in &self.preservation.objects {
            if !objects.contains(source.id.as_str()) {
                continue;
            }
            let mut row = source.clone();
            row.id = copied_id(target, "object", &row.id)?;
            row.calendar_id.clone_from(&target_row.calendar_id);
            result.objects.push(row);
        }
        for source in &self.preservation.diagnostics {
            if !objects.contains(source.object_id.as_str()) {
                continue;
            }
            let mut row = source.clone();
            row.id = copied_id(target, "diagnostic", &row.id)?;
            row.object_id = copied_id(target, "object", &row.object_id)?;
            result.diagnostics.push(row);
        }
        let mut pending: Vec<_> = self
            .preservation
            .components
            .iter()
            .filter(|row| included.contains(&row.id))
            .collect();
        let mut inserted = BTreeSet::new();
        while !pending.is_empty() {
            let before = inserted.len();
            let mut deferred = Vec::new();
            for source_component in pending {
                if source_component
                    .parent_component_id
                    .as_ref()
                    .is_some_and(|id| !inserted.contains(id))
                {
                    deferred.push(source_component);
                    continue;
                }
                let mut row = source_component.clone();
                row.id = component_ids[&row.id].clone();
                row.object_id = copied_id(target, "object", &row.object_id)?;
                row.parent_component_id = mapped(&component_ids, &row.parent_component_id)?;
                row.calendar_id.clone_from(&target_row.calendar_id);
                if Some(&source_component.id) == source.icalendar_component_id.as_ref() {
                    row.projected_kind = Some("event".into());
                    row.projected_id = Some(target.into());
                    row.uid = Some(
                        target_row
                            .source_uid
                            .clone()
                            .unwrap_or_else(|| target.into()),
                    );
                    row.recurrence_id = None;
                    row.recurrence_id_value_type = None;
                    row.sequence = Some(target_row.sequence);
                    row.dtstart_key = Some(target_row.start_time.clone());
                } else if let Some(override_row) = self
                    .overrides
                    .iter()
                    .find(|row| row.icalendar_component_id.as_ref() == Some(&source_component.id))
                    .filter(|_| include_overrides)
                {
                    row.projected_kind = Some("override".into());
                    row.projected_id = Some(copied_id(target, "override", &override_row.id)?);
                    row.uid = Some(
                        target_row
                            .source_uid
                            .clone()
                            .unwrap_or_else(|| target.into()),
                    );
                    row.recurrence_id = Some(override_row.recurrence_id.clone());
                } else {
                    row.projected_kind = None;
                    row.projected_id = None;
                }
                result.components.push(row);
                inserted.insert(source_component.id.clone());
            }
            if inserted.len() == before {
                return Err("Calendar component copy made no progress".into());
            }
            pending = deferred;
        }
        for source in &self.preservation.properties {
            let Some(id) = property_ids.get(&source.id) else {
                continue;
            };
            let mut row = source.clone();
            row.id.clone_from(id);
            row.component_id = component_ids[&row.component_id].clone();
            result.properties.push(row);
        }
        for source in &self.preservation.parameters {
            let Some(id) = parameter_ids.get(&source.id) else {
                continue;
            };
            let mut row = source.clone();
            row.id.clone_from(id);
            row.property_id = property_ids[&row.property_id].clone();
            result.parameters.push(row);
        }
        let mut pending = nodes;
        let mut inserted = BTreeSet::new();
        while !pending.is_empty() {
            let before = inserted.len();
            let mut deferred = Vec::new();
            for source in pending {
                if source
                    .parent_node_id
                    .as_ref()
                    .is_some_and(|id| !inserted.contains(id))
                {
                    deferred.push(source);
                    continue;
                }
                let mut row = source.clone();
                row.id = node_ids[&row.id].clone();
                row.property_id = mapped(&property_ids, &row.property_id)?;
                row.parameter_id = mapped(&parameter_ids, &row.parameter_id)?;
                row.parent_node_id = mapped(&node_ids, &row.parent_node_id)?;
                result.nodes.push(row);
                inserted.insert(source.id.clone());
            }
            if inserted.len() == before {
                return Err("Calendar value copy made no progress".into());
            }
            pending = deferred;
        }
        for source in &self.preservation.warnings {
            if !included.contains(&source.component_id) {
                continue;
            }
            let mut row = source.clone();
            row.id = copied_id(target, "warning", &row.id)?;
            row.component_id = component_ids[&row.component_id].clone();
            result.warnings.push(row);
        }
        result.component_id = mapped(&component_ids, &source.icalendar_component_id)?;
        for source in &self.alarms {
            let mut row = source.clone();
            row.event_id = target.into();
            row.id = copied_id(target, "alarm", &row.id)?;
            row.icalendar_component_id = mapped(&component_ids, &row.icalendar_component_id)?;
            result.alarms.push(row);
        }
        for source in &self.attendees {
            let mut row = source.clone();
            row.event_id = target.into();
            row.id = copied_id(target, "attendee", &row.id)?;
            row.icalendar_component_id = mapped(&component_ids, &row.icalendar_component_id)?;
            result.attendees.push(row);
        }
        for source in &self.categories {
            let mut row = source.clone();
            row.event_id = target.into();
            row.id = copied_id(target, "category", &row.id)?;
            result.categories.push(row);
        }
        for source in &self.properties {
            let mut row = source.clone();
            row.event_id = target.into();
            row.id = copied_id(target, "extended-property", &row.id)?;
            result.extensions.push(row);
        }
        for source in &self.organizers {
            let mut row = source.clone();
            row.event_id = target.into();
            result.organizers.push(row);
        }
        for source in &self.task_links {
            let mut row = source.clone();
            row.event_id = target.into();
            result.task_links.push(row);
        }
        Ok(result)
    }

    /// Retarget every captured projection and its independent import closure.
    /// The caller reserves aggregate output space before cloning source rows.
    pub(super) fn copy_to(mut self, target_row: Event) -> Result<Self, String> {
        let copy = self.prepare_copy(&target_row, true)?;
        let target = &target_row.id;
        let mut overrides = std::collections::BTreeMap::new();
        for row in &mut self.overrides {
            let id = copied_id(target, "override", &row.id)?;
            overrides.insert(row.id.clone(), id.clone());
            row.id = id;
            row.parent_event_id.clone_from(target);
            row.icalendar_component_id = row
                .icalendar_component_id
                .as_deref()
                .map(|id| copied_id(target, "component", id))
                .transpose()?;
        }
        for row in &mut self.override_properties {
            row.id = copied_id(target, "override-property", &row.id)?;
            row.override_id = overrides
                .get(&row.override_id)
                .cloned()
                .ok_or("Calendar override property lost its owner")?;
        }
        macro_rules! retarget {
            ($rows:ident, $kind:literal) => {
                for row in &mut self.$rows {
                    row.id = copied_id(target, $kind, &row.id)?;
                    row.event_id.clone_from(target);
                }
            };
        }
        retarget!(exdates, "exdate");
        retarget!(rdates, "rdate");
        retarget!(notifications, "notification");
        for row in &mut self.focus_configs {
            row.event_id.clone_from(target);
        }
        for row in &mut self.count_rhythms {
            row.event_id.clone_from(target);
        }
        for row in &mut self.sequence_steps {
            row.event_id.clone_from(target);
        }
        for row in &mut self.music_assignments {
            row.owner_id.clone_from(target);
        }
        self.events = vec![Event {
            icalendar_component_id: copy.component_id,
            ..target_row
        }];
        self.alarms = copy.alarms;
        self.attendees = copy.attendees;
        self.categories = copy.categories;
        self.properties = copy.extensions;
        self.organizers = copy.organizers;
        self.task_links = copy.task_links;
        self.preservation = super::preservation::Preservation {
            objects: copy.objects,
            diagnostics: copy.diagnostics,
            components: copy.components,
            properties: copy.properties,
            parameters: copy.parameters,
            nodes: copy.nodes,
            warnings: copy.warnings,
        };
        Ok(self)
    }
}

impl PreparedCopy {
    /// The caller owns the transaction and rolls back on any failure.
    #[cfg(test)]
    pub(super) async fn write(
        self,
        connection: &mut SqliteConnection,
        target: &str,
    ) -> Result<(), String> {
        for row in self.objects {
            insert_row!(connection, "icalendar_objects", row, { id, calendar_id, source_kind, source_name, source_fingerprint, prodid, version, method, calendar_scale, created_at, updated_at });
        }
        for row in self.diagnostics {
            insert_row!(connection, "icalendar_object_diagnostics", row, { id, object_id, message, sort_order });
        }
        for row in self.components {
            insert_row!(connection, "icalendar_components", row, { id, object_id, parent_component_id, calendar_id, component_type, uid, recurrence_id, recurrence_id_value_type, sequence, dtstart_key, projected_kind, projected_id, preservation_status, sort_order, created_at, updated_at });
        }
        for row in self.properties {
            insert_row!(connection, "icalendar_component_properties", row, { id, component_id, name, value_type, sort_order });
        }
        for row in self.parameters {
            insert_row!(connection, "icalendar_property_parameters", row, { id, property_id, name, sort_order });
        }
        for row in self.nodes {
            insert_row!(connection, "icalendar_value_nodes", row, { id, property_id, parameter_id, parent_node_id, sort_order, value_kind, object_key, text_value, number_value, boolean_value });
        }
        for row in self.warnings {
            insert_row!(connection, "icalendar_component_projection_warnings", row, { id, component_id, message, sort_order });
        }
        for row in self.alarms {
            insert_row!(connection, "calendar_event_alarms", row, { id, event_id, action, trigger_type, trigger_value, description, sort_order, icalendar_component_id });
        }
        for row in self.attendees {
            insert_row!(connection, "calendar_event_attendees", row, { id, event_id, name, email, role, status, rsvp, sort_order, icalendar_component_id, icalendar_property_index });
        }
        for row in self.categories {
            insert_row!(connection, "calendar_event_categories", row, { id, event_id, category, sort_order });
        }
        for row in self.extensions {
            insert_row!(connection, "calendar_event_extended_properties", row, { id, event_id, property_key, property_value, sort_order });
        }
        for row in self.organizers {
            insert_row!(connection, "calendar_event_organizers", row, { event_id, name, email });
        }
        for row in self.task_links {
            insert_row!(connection, "project_task_event_links", row, { task_id, event_id, link_kind, created_at });
        }
        sqlx::query("UPDATE calendar_events SET icalendar_component_id = ? WHERE id = ?")
            .bind(self.component_id)
            .bind(target)
            .execute(connection)
            .await
            .map_err(|error| format!("link copied Calendar preservation: {error}"))?;
        Ok(())
    }
}
