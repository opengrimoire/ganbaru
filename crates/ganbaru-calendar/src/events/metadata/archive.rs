//! Complete archives with independent imported graphs and original identities.

use std::collections::BTreeMap;

use sqlx::{Sqlite, Transaction};

use super::super::types::CalendarEventMutationContext;
use super::Metadata;
use super::copy::copied_id;

/// Prepared on the admitted worker. Writing never rereads a mutable live source.
pub(in crate::events) struct PreparedArchive {
    metadata: Metadata,
    source_id: String,
    original_id: String,
    recurrence_date: Option<String>,
    original_children: BTreeMap<String, String>,
}

impl Metadata {
    /// Inherit moved occurrence metadata before cropping and copying provenance.
    /// The archive key can be opaque; original identity remains a separate fact.
    pub(in crate::events) fn prepare_archive(
        mut self,
        archive_id: &str,
        context: &CalendarEventMutationContext,
    ) -> Result<PreparedArchive, String> {
        let source_id = self
            .events
            .first()
            .ok_or("Calendar archive source is missing")?
            .id
            .clone();
        if source_id != context.source_event_id {
            return Err("Calendar archive geometry belongs to another source".into());
        }
        if context.synthetic {
            self.inherit_occurrence(
                context
                    .occurrence_date
                    .as_deref()
                    .ok_or("Calendar archive occurrence date is missing")?,
            )?;
            let event = &mut self.events[0];
            event.start_time = context.start_time.clone();
            event.end_time = context.end_time.clone();
            if event.all_day != 0 {
                event.start_time = super::super::ids::date_part(&event.start_time)
                    .ok_or("Calendar archive floating start is invalid")?;
                event.end_time = super::super::ids::date_part(&event.end_time)
                    .ok_or("Calendar archive floating end is invalid")?;
            }
            event.rrule = None;
            event.repeat_until = None;
            self.overrides.clear();
            self.override_properties.clear();
            self.exdates.clear();
            self.rdates.clear();
        }
        self.reserve_archive_copy()?;
        let original_children = self
            .alarms
            .iter()
            .map(|row| ("alarm", &row.id))
            .chain(self.attendees.iter().map(|row| ("attendee", &row.id)))
            .chain(self.overrides.iter().map(|row| ("override", &row.id)))
            .map(|(kind, id)| Ok((copied_id(archive_id, kind, id)?, id.clone())))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let mut event = self.events[0].clone();
        event.id = archive_id.into();
        let mut metadata = self.copy_to(event)?;
        for component in &mut metadata.preservation.components {
            if component.projected_kind.as_deref() == Some("event")
                && component.projected_id.as_deref() == Some(archive_id)
            {
                component.projected_id = Some(context.canonical_id.clone());
                component.uid = metadata.events[0]
                    .source_uid
                    .clone()
                    .or_else(|| Some(source_id.clone()));
            } else if component.projected_kind.as_deref() == Some("override") {
                component.projected_id = component
                    .projected_id
                    .as_ref()
                    .map(|id| {
                        original_children
                            .get(id)
                            .cloned()
                            .ok_or("Calendar archive override lost its original identity")
                    })
                    .transpose()?;
            }
        }
        Ok(PreparedArchive {
            metadata,
            source_id,
            original_id: context.canonical_id.clone(),
            recurrence_date: context
                .synthetic
                .then(|| context.occurrence_date.clone())
                .flatten(),
            original_children,
        })
    }
}

// Table and column identifiers are literals owned by the native service.
// Every value is bound from a captured typed row, never supplied as SQL by IPC.
macro_rules! archive_row {
    ($tx:expr, $table:literal, {$($field:ident: $value:expr),+ $(,)?}) => {{
        let columns = &[$(stringify!($field)),+];
        let sql = format!("INSERT INTO {} ({}) VALUES ({})", $table, columns.join(", "), vec!["?"; columns.len()].join(", "));
        sqlx::query(&sql)$(.bind($value))+.execute(&mut **$tx).await
            .map_err(|error| format!("write Calendar archive {}: {error}", $table))?;
    }};
}

impl PreparedArchive {
    pub(in crate::events) fn id(&self) -> &str {
        &self.metadata.events[0].id
    }

    fn original_child(&self, id: &str) -> Result<&str, String> {
        self.original_children
            .get(id)
            .map(String::as_str)
            .ok_or("Calendar archive lost a child identity".into())
    }

    /// All archive records and their independent import graph share the caller's
    /// transaction. An existing archive is a conflict, never an implicit overwrite.
    pub(in crate::events) async fn write(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        archived_at: &str,
    ) -> Result<(), String> {
        self.metadata.write_preservation(tx).await?;
        let row = &self.metadata.events[0];
        archive_row!(tx, "calendar_event_archives", {
            id: &row.id, source_event_id: &self.source_id, original_occurrence_id: &self.original_id, recurrence_date: &self.recurrence_date, archived_at: archived_at,
            title: &row.title, start_time: &row.start_time, end_time: &row.end_time, timezone: &row.timezone,
            calendar_id: &row.calendar_id, project_id: &row.project_id, color: row.color, description: &row.description,
            rrule: &row.rrule, repeat_until: &row.repeat_until, environment_id: &row.environment_id, playlist_id: &row.playlist_id,
            all_day: row.all_day, location: &row.location, url: &row.url, transparency: &row.transparency, status: &row.status,
            source_uid: &row.source_uid, visibility: &row.visibility, priority: row.priority, geo_lat: row.geo_lat, geo_lng: row.geo_lng,
            sequence: row.sequence, guest_can_modify: row.guest_can_modify, guest_can_invite_others: row.guest_can_invite_others,
            guest_can_see_other_guests: row.guest_can_see_other_guests, created_at: &row.created_at, updated_at: &row.updated_at,
            icalendar_component_id: &row.icalendar_component_id, local_rsvp_status: &row.local_rsvp_status, meeting_enabled: row.meeting_enabled
        });
        let id = &row.id;
        for row in &self.metadata.preservation.objects {
            archive_row!(tx, "calendar_event_archive_import_objects", {archive_event_id: id, object_id: &row.id});
        }
        for row in &self.metadata.alarms {
            archive_row!(tx, "calendar_event_archive_alarms", {id: &row.id, archive_event_id: id, source_alarm_id: self.original_child(&row.id)?, action: &row.action, trigger_type: &row.trigger_type, trigger_value: &row.trigger_value, description: &row.description, sort_order: row.sort_order, icalendar_component_id: &row.icalendar_component_id});
        }
        for row in &self.metadata.attendees {
            archive_row!(tx, "calendar_event_archive_attendees", {id: &row.id, archive_event_id: id, source_attendee_id: self.original_child(&row.id)?, name: &row.name, email: &row.email, role: &row.role, status: &row.status, rsvp: row.rsvp, sort_order: row.sort_order, icalendar_component_id: &row.icalendar_component_id, icalendar_property_index: row.icalendar_property_index});
        }
        for row in &self.metadata.categories {
            archive_row!(tx, "calendar_event_archive_categories", {id: &row.id, archive_event_id: id, category: &row.category, sort_order: row.sort_order});
        }
        for row in &self.metadata.exdates {
            archive_row!(tx, "calendar_event_archive_exdates", {id: &row.id, archive_event_id: id, occurrence_date: &row.occurrence_date, sort_order: row.sort_order});
        }
        for row in &self.metadata.rdates {
            archive_row!(tx, "calendar_event_archive_rdates", {id: &row.id, archive_event_id: id, occurrence_start: &row.occurrence_start, sort_order: row.sort_order});
        }
        for row in &self.metadata.properties {
            archive_row!(tx, "calendar_event_archive_extended_properties", {id: &row.id, archive_event_id: id, property_key: &row.property_key, property_value: &row.property_value, sort_order: row.sort_order});
        }
        for row in &self.metadata.notifications {
            archive_row!(tx, "calendar_event_archive_notifications", {id: &row.id, archive_event_id: id, offset_minutes: row.offset_minutes, sort_order: row.sort_order});
        }
        for row in &self.metadata.organizers {
            archive_row!(tx, "calendar_event_archive_organizers", {archive_event_id: id, name: &row.name, email: &row.email});
        }
        for row in &self.metadata.overrides {
            archive_row!(tx, "calendar_event_archive_overrides", {id: &row.id, archive_event_id: id, source_override_id: self.original_child(&row.id)?, recurrence_id: &row.recurrence_id, title: &row.title, start_time: &row.start_time, end_time: &row.end_time, description: &row.description, location: &row.location, url: &row.url, color: row.color, status: &row.status, transparency: &row.transparency, visibility: &row.visibility, created_at: &row.created_at, updated_at: &row.updated_at, icalendar_component_id: &row.icalendar_component_id, recurrence_range: &row.recurrence_range});
        }
        for row in &self.metadata.override_properties {
            archive_row!(tx, "calendar_event_archive_override_extended_properties", {id: &row.id, archive_override_id: &row.override_id, property_key: &row.property_key, property_value: &row.property_value, sort_order: row.sort_order});
        }
        for row in &self.metadata.focus_configs {
            archive_row!(tx, "calendar_event_archive_pomodoro_configs", {archive_event_id: id, rhythm_kind: &row.rhythm_kind, rhythm_source: &row.rhythm_source, preset_key: &row.preset_key, idle_timeout_minutes: row.idle_timeout_minutes});
        }
        for row in &self.metadata.count_rhythms {
            archive_row!(tx, "calendar_event_archive_pomodoro_config_count_rhythms", {archive_event_id: id, focus_duration_minutes: row.focus_duration_minutes, short_break_minutes: row.short_break_minutes, long_break_minutes: row.long_break_minutes, long_break_after_focus_count: row.long_break_after_focus_count});
        }
        for row in &self.metadata.sequence_steps {
            archive_row!(tx, "calendar_event_archive_pomodoro_config_sequence_steps", {archive_event_id: id, step_index: row.step_index, focus_duration_minutes: row.focus_duration_minutes, break_phase: &row.break_phase, break_duration_minutes: row.break_duration_minutes});
        }
        for row in &self.metadata.music_assignments {
            archive_row!(tx, "calendar_event_archive_music_assignments", {archive_event_id: id, owner_kind: &row.owner_kind, phase: &row.phase, behavior: &row.behavior, playlist_id: &row.playlist_id, soundscape_id: &row.soundscape_id, provenance_kind: &row.provenance_kind, provenance_id: &row.provenance_id, updated_at_ms: row.updated_at_ms, version: row.version, soundscape_behavior: &row.soundscape_behavior});
        }
        for row in &self.metadata.task_links {
            archive_row!(tx, "calendar_event_archive_task_links", {archive_event_id: id, task_id: &row.task_id, link_kind: &row.link_kind, created_at: &row.created_at});
        }
        Ok(())
    }
}
