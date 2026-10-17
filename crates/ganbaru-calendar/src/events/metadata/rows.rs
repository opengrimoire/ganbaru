//! Exact persisted Calendar rows. Projection, sanitization and defaults belong after capture.

use super::super::occurrence::ReadBudget;
use ganbaru_db::impl_sqlite_from_row;
use serde::Serialize;
use sqlx::SqliteConnection;

impl Event {
    /// Update the existing row without deleting its dependent durable history.
    pub(super) async fn update(&self, connection: &mut SqliteConnection) -> Result<(), String> {
        let result = sqlx::query("UPDATE calendar_events SET title = ?, start_time = ?, end_time = ?, timezone = ?, calendar_id = ?, project_id = ?, color = ?, description = ?, rrule = ?, repeat_until = ?, environment_id = ?, playlist_id = ?, all_day = ?, location = ?, url = ?, transparency = ?, status = ?, source_uid = ?, visibility = ?, priority = ?, geo_lat = ?, geo_lng = ?, sequence = ?, guest_can_modify = ?, guest_can_invite_others = ?, guest_can_see_other_guests = ?, created_at = ?, updated_at = ?, icalendar_component_id = ?, local_rsvp_status = ?, meeting_enabled = ? WHERE id = ?")
            .bind(&self.title)
            .bind(&self.start_time)
            .bind(&self.end_time)
            .bind(&self.timezone)
            .bind(&self.calendar_id)
            .bind(&self.project_id)
            .bind(self.color)
            .bind(&self.description)
            .bind(&self.rrule)
            .bind(&self.repeat_until)
            .bind(&self.environment_id)
            .bind(&self.playlist_id)
            .bind(self.all_day)
            .bind(&self.location)
            .bind(&self.url)
            .bind(&self.transparency)
            .bind(&self.status)
            .bind(&self.source_uid)
            .bind(&self.visibility)
            .bind(self.priority)
            .bind(self.geo_lat)
            .bind(self.geo_lng)
            .bind(self.sequence)
            .bind(self.guest_can_modify)
            .bind(self.guest_can_invite_others)
            .bind(self.guest_can_see_other_guests)
            .bind(&self.created_at)
            .bind(&self.updated_at)
            .bind(&self.icalendar_component_id)
            .bind(&self.local_rsvp_status)
            .bind(self.meeting_enabled)
            .bind(&self.id)
            .execute(connection).await.map_err(|error| format!("update prepared Calendar row: {error}"))?;
        if result.rows_affected() != 1 {
            return Err("Calendar source disappeared before its prepared write".into());
        }
        Ok(())
    }
}

// Each declaration fixes its table, selection and complete column set in source.
// No caller can supply SQL, table names or column names.
macro_rules! stored_row {
    ($name:ident, $table:literal, $selection:literal, ($($key:ident),+), { $($field:ident: $kind:ty),+ $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, Serialize)]
        pub(super) struct $name { $(pub(super) $field: $kind,)+ }
        impl_sqlite_from_row!($name { $($field),+ });
        impl $name {
            /// Insert an exact prepared row in the caller's transaction.
            pub(super) async fn insert(&self, connection: &mut SqliteConnection) -> Result<(), String> {
                let columns = &[$(stringify!($field)),+];
                let sql = format!("INSERT INTO {} ({}) VALUES ({})", $table, columns.join(", "), vec!["?"; columns.len()].join(", "));
                sqlx::query(&sql)$(.bind(&self.$field))+.execute(connection).await
                    .map_err(|error| format!("write Calendar {}: {error}", $table))?;
                Ok(())
            }
            pub(super) async fn read(connection: &mut SqliteConnection, id: &str, budget: &mut ReadBudget) -> Result<Vec<Self>, String> {
                let columns = &[$(stringify!($field)),+];
                let sql = format!("SELECT {} FROM {} WHERE {}", columns.join(", "), $table, $selection);
                let mut rows = budget.read::<Self>(connection, id, &sql, columns).await?;
                // Sorting follows admission so an oversized source is never sorted in SQL.
                rows.sort_by(|a,b| ($(&a.$key,)+).cmp(&($(&b.$key,)+)));
                Ok(rows)
            }
        }
    };
}

stored_row!(Event, "calendar_events", "id = ?1", (id), {
    id: String,
    title: String,
    start_time: String,
    end_time: String,
    timezone: String,
    calendar_id: String,
    project_id: Option<String>,
    color: Option<i64>,
    description: String,
    rrule: Option<String>,
    repeat_until: Option<String>,
    environment_id: Option<String>,
    playlist_id: Option<String>,
    all_day: i64,
    location: String,
    url: String,
    transparency: String,
    status: String,
    source_uid: Option<String>,
    visibility: String,
    priority: Option<i64>,
    geo_lat: Option<f64>,
    geo_lng: Option<f64>,
    sequence: i64,
    guest_can_modify: i64,
    guest_can_invite_others: i64,
    guest_can_see_other_guests: i64,
    created_at: String,
    updated_at: String,
    icalendar_component_id: Option<String>,
    local_rsvp_status: Option<String>,
    meeting_enabled: i64,
});

stored_row!(Alarm, "calendar_event_alarms", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    action: String,
    trigger_type: String,
    trigger_value: String,
    description: Option<String>,
    sort_order: i64,
    icalendar_component_id: Option<String>,
});

stored_row!(Attendee, "calendar_event_attendees", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    name: Option<String>,
    email: String,
    role: String,
    status: String,
    rsvp: i64,
    sort_order: i64,
    icalendar_component_id: Option<String>,
    icalendar_property_index: Option<i64>,
});

stored_row!(Category, "calendar_event_categories", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    category: String,
    sort_order: i64,
});

stored_row!(Exdate, "calendar_event_exdates", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    occurrence_date: String,
    sort_order: i64,
});

stored_row!(Property, "calendar_event_extended_properties", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    property_key: String,
    property_value: String,
    sort_order: i64,
});

stored_row!(Notification, "calendar_event_notifications", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    offset_minutes: i64,
    sort_order: i64,
});

stored_row!(Organizer, "calendar_event_organizers", "event_id = ?1", (event_id), {
    event_id: String,
    name: Option<String>,
    email: String,
});

stored_row!(Override, "calendar_event_overrides", "parent_event_id = ?1", (id), {
    id: String,
    parent_event_id: String,
    recurrence_id: String,
    title: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    description: Option<String>,
    location: Option<String>,
    url: Option<String>,
    color: Option<i64>,
    status: Option<String>,
    transparency: Option<String>,
    visibility: Option<String>,
    created_at: String,
    updated_at: String,
    icalendar_component_id: Option<String>,
    recurrence_range: Option<String>,
});

stored_row!(OverrideProperty, "calendar_event_override_extended_properties", "override_id IN (SELECT id FROM calendar_event_overrides WHERE parent_event_id = ?1)", (id), {
    id: String,
    override_id: String,
    property_key: String,
    property_value: String,
    sort_order: i64,
});

stored_row!(Rdate, "calendar_event_rdates", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    occurrence_start: String,
    sort_order: i64,
});

stored_row!(FocusConfig, "calendar_event_pomodoro_configs", "event_id = ?1", (event_id), {
    event_id: String,
    rhythm_kind: String,
    rhythm_source: String,
    preset_key: Option<String>,
    idle_timeout_minutes: Option<i64>,
});

stored_row!(CountRhythm, "calendar_event_pomodoro_config_count_rhythms", "event_id = ?1", (event_id), {
    event_id: String,
    focus_duration_minutes: i64,
    short_break_minutes: i64,
    long_break_minutes: i64,
    long_break_after_focus_count: i64,
});

stored_row!(SequenceStep, "calendar_event_pomodoro_config_sequence_steps", "event_id = ?1", (step_index), {
    event_id: String,
    step_index: i64,
    focus_duration_minutes: i64,
    break_phase: String,
    break_duration_minutes: i64,
});

stored_row!(MusicAssignment, "music_context_assignments", "owner_id = ?1 AND owner_kind IN ('event-snapshot', 'event-override')", (owner_kind, phase), {
    owner_kind: String,
    owner_id: String,
    phase: String,
    behavior: String,
    playlist_id: Option<String>,
    soundscape_id: Option<String>,
    provenance_kind: String,
    provenance_id: Option<String>,
    updated_at_ms: i64,
    version: i64,
    soundscape_behavior: String,
});

stored_row!(TaskLink, "project_task_event_links", "event_id = ?1", (task_id), {
    task_id: String,
    event_id: String,
    link_kind: String,
    created_at: String,
});

stored_row!(Component, "icalendar_components", "id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    object_id: String,
    parent_component_id: Option<String>,
    calendar_id: String,
    component_type: String,
    uid: Option<String>,
    recurrence_id: Option<String>,
    recurrence_id_value_type: Option<String>,
    sequence: Option<i64>,
    dtstart_key: Option<String>,
    projected_kind: Option<String>,
    projected_id: Option<String>,
    preservation_status: String,
    sort_order: i64,
    created_at: String,
    updated_at: String,
});

stored_row!(ComponentProperty, "icalendar_component_properties", "component_id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    component_id: String,
    name: String,
    value_type: String,
    sort_order: i64,
});

stored_row!(Parameter, "icalendar_property_parameters", "property_id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    property_id: String,
    name: String,
    sort_order: i64,
});

stored_row!(Node, "icalendar_value_nodes", "property_id IN (SELECT value FROM json_each(?1)) OR parameter_id IN (SELECT id FROM icalendar_property_parameters WHERE property_id IN (SELECT value FROM json_each(?1)))", (id), {
    id: String,
    property_id: Option<String>,
    parameter_id: Option<String>,
    parent_node_id: Option<String>,
    sort_order: i64,
    value_kind: String,
    object_key: Option<String>,
    text_value: Option<String>,
    number_value: Option<f64>,
    boolean_value: Option<i64>,
});

stored_row!(Warning, "icalendar_component_projection_warnings", "component_id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    component_id: String,
    message: String,
    sort_order: i64,
});

stored_row!(Object, "icalendar_objects", "id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    calendar_id: String,
    source_kind: String,
    source_name: String,
    source_fingerprint: String,
    prodid: Option<String>,
    version: Option<String>,
    method: Option<String>,
    calendar_scale: Option<String>,
    created_at: String,
    updated_at: String,
});

stored_row!(Diagnostic, "icalendar_object_diagnostics", "object_id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    object_id: String,
    message: String,
    sort_order: i64,
});
