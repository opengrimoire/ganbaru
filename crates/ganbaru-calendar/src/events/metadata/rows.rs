//! Exact persisted Calendar rows. Projection, sanitization and defaults belong after capture.

use super::super::occurrence::ReadBudget;
use crate::value_bound::ValueSize;
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
// No caller can supply SQL, table names or column names. A `keyed` declaration also writes a
// derived key column computed from the row at insert, so a row copied to another owner never
// carries its source's key.
macro_rules! stored_row {
    ($(#[$insert_attr:meta])* $name:ident, $table:literal, $selection:literal, ($($key:ident),+), { $($field:ident: $kind:ty),+ $(,)? } $(, keyed $key_column:expr => $key_fn:path)?) => {
        #[derive(Clone, Debug, PartialEq, Serialize)]
        pub(super) struct $name { $(pub(super) $field: $kind,)+ }
        impl_sqlite_from_row!($name { $($field),+ });
        impl $name {
            /// Insert an exact prepared row in the caller's transaction.
            $(#[$insert_attr])*
            pub(super) async fn insert(&self, connection: &mut SqliteConnection) -> Result<(), String> {
                #[allow(unused_mut)]
                let mut columns = vec![$(stringify!($field)),+];
                $(columns.push($key_column);)?
                let sql = format!("INSERT INTO {} ({}) VALUES ({})", $table, columns.join(", "), vec!["?"; columns.len()].join(", "));
                let query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))$(.bind(&self.$field))+;
                $(let query = query.bind($key_fn(self)?);)?
                query.execute(connection).await
                    .map_err(|error| format!("write Calendar {}: {error}", $table))?;
                Ok(())
            }
            /// Charges the row with every stored column to a sync value estimate. Columns the
            /// value does not carry, such as the owner, only raise the estimate.
            #[allow(dead_code, reason = "only rows of groups that own rows are charged")]
            pub(super) fn charge(&self, size: &mut ValueSize) {
                size.row();
                $(size.named_field(stringify!($field), &self.$field);)+
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
    geo_lat: Option<i64>,
    geo_lng: Option<i64>,
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

stored_row!(Exdate, "calendar_event_exdates", "event_id = ?1", (occurrence_date), {
    id: String,
    event_id: String,
    occurrence_date: String,
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

stored_row!(OverrideProperty, "calendar_event_override_extended_properties", "event_id = ?1", (id), {
    id: String,
    event_id: String,
    override_id: String,
    property_key: String,
    property_value: String,
    sort_order: i64,
});

stored_row!(Rdate, "calendar_event_rdates", "event_id = ?1", (occurrence_start), {
    id: String,
    event_id: String,
    occurrence_start: String,
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

stored_row!(#[expect(dead_code, reason = "edits write assignments through `upsert` so unchanged rows stay untouched")] MusicAssignment, "music_context_assignments", "owner_id = ?1 AND owner_kind IN ('event-snapshot', 'event-override')", (owner_kind, phase), {
    owner_kind: String,
    owner_id: String,
    phase: String,
    behavior: String,
    playlist_id: Option<String>,
    provenance_kind: String,
    provenance_id: Option<String>,
    updated_at_ms: i64,
    version: i64,
    soundscape_behavior: String,
    soundscape_preset_id: Option<String>,
}, keyed ganbaru_music::sync_keys::SYNC_KEY_COLUMN => music_assignment_key);

impl MusicAssignment {
    /// Writes the row in place of the stored row with the same owner and phase, leaving an
    /// unchanged row untouched so replication captures nothing for it.
    pub(super) async fn upsert(&self, connection: &mut SqliteConnection) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO music_context_assignments (owner_kind, owner_id, phase, behavior,
                playlist_id, provenance_kind, provenance_id, updated_at_ms, version,
                soundscape_behavior, soundscape_preset_id, sync_key)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (owner_kind, owner_id, phase) DO UPDATE SET
                behavior = excluded.behavior,
                playlist_id = excluded.playlist_id,
                provenance_kind = excluded.provenance_kind,
                provenance_id = excluded.provenance_id,
                updated_at_ms = excluded.updated_at_ms,
                version = excluded.version,
                soundscape_behavior = excluded.soundscape_behavior,
                soundscape_preset_id = excluded.soundscape_preset_id
             WHERE behavior IS NOT excluded.behavior
                OR playlist_id IS NOT excluded.playlist_id
                OR provenance_kind IS NOT excluded.provenance_kind
                OR provenance_id IS NOT excluded.provenance_id
                OR updated_at_ms IS NOT excluded.updated_at_ms
                OR version IS NOT excluded.version
                OR soundscape_behavior IS NOT excluded.soundscape_behavior
                OR soundscape_preset_id IS NOT excluded.soundscape_preset_id",
        )
        .bind(&self.owner_kind)
        .bind(&self.owner_id)
        .bind(&self.phase)
        .bind(&self.behavior)
        .bind(&self.playlist_id)
        .bind(&self.provenance_kind)
        .bind(&self.provenance_id)
        .bind(self.updated_at_ms)
        .bind(self.version)
        .bind(&self.soundscape_behavior)
        .bind(&self.soundscape_preset_id)
        .bind(music_assignment_key(self)?)
        .execute(connection)
        .await
        .map_err(|error| format!("write Calendar music assignment: {error}"))?;
        Ok(())
    }

    /// Removes the event's stored assignments whose owner kind and phase are not among `kept`.
    pub(super) async fn remove_stale(
        connection: &mut SqliteConnection,
        event_id: &str,
        kept: &[Self],
    ) -> Result<(), String> {
        let kept = kept
            .iter()
            .map(|row| [row.owner_kind.as_str(), row.phase.as_str()])
            .collect::<Vec<_>>();
        let kept = serde_json::to_string(&kept)
            .map_err(|error| format!("encode kept Calendar music assignments: {error}"))?;
        sqlx::query(
            "DELETE FROM music_context_assignments
             WHERE owner_id = ?1 AND owner_kind IN ('event-snapshot', 'event-override')
               AND NOT EXISTS (SELECT 1 FROM json_each(?2) AS kept
                   WHERE json_extract(kept.value, '$[0]') = owner_kind
                     AND json_extract(kept.value, '$[1]') = phase)",
        )
        .bind(event_id)
        .bind(kept)
        .execute(connection)
        .await
        .map_err(|error| format!("clear stale Calendar music assignments: {error}"))?;
        Ok(())
    }
}

/// Derived sync key of a Music assignment under its current owner.
fn music_assignment_key(row: &MusicAssignment) -> Result<String, String> {
    ganbaru_music::sync_keys::assignment_key(&row.owner_kind, &row.owner_id, &row.phase)
        .map_err(|error| format!("derive Calendar music assignment key: {error}"))
}

stored_row!(TaskLink, "project_task_event_links", "event_id = ?1", (task_id), {
    task_id: String,
    event_id: String,
    link_kind: String,
    created_at: String,
}, keyed "sync_key" => task_link_key);

/// Derived sync key of a task link under its current event.
fn task_link_key(row: &TaskLink) -> Result<String, String> {
    ganbaru_projects::keys::task_event_key(&row.task_id, &row.event_id)
}

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

stored_row!(Parameter, "icalendar_property_parameters", "component_id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    component_id: String,
    property_id: String,
    name: String,
    sort_order: i64,
});

stored_row!(Node, "icalendar_value_nodes", "component_id IN (SELECT value FROM json_each(?1))", (id), {
    id: String,
    component_id: String,
    property_id: String,
    parameter_id: Option<String>,
    parent_node_id: Option<String>,
    sort_order: i64,
    value_kind: String,
    object_key: Option<String>,
    text_value: Option<String>,
    number_value: Option<String>,
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
    import_id: String,
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

#[cfg(test)]
mod tests {
    use super::MusicAssignment;

    async fn pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    fn assignment(owner_kind: &str, phase: &str, behavior: &str) -> MusicAssignment {
        MusicAssignment {
            owner_kind: owner_kind.into(),
            owner_id: "source".into(),
            phase: phase.into(),
            behavior: behavior.into(),
            playlist_id: None,
            provenance_kind: "explicit".into(),
            provenance_id: None,
            updated_at_ms: 1,
            version: 1,
            soundscape_behavior: "inherit".into(),
            soundscape_preset_id: None,
        }
    }

    async fn stored(pool: &sqlx::SqlitePool) -> Vec<(String, String, String)> {
        sqlx::query_as(
            "SELECT owner_kind, phase, behavior FROM music_context_assignments
             WHERE owner_id = 'source' ORDER BY owner_kind, phase",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[test]
    fn removing_stale_assignments_keeps_listed_phases_and_other_owner_kinds() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let mut connection = pool.acquire().await.unwrap();
            let kept = [
                assignment("event-snapshot", "focus", "keep-current-music"),
                assignment("event-override", "short-break", "pause-music"),
            ];
            for row in kept.iter().chain(&[
                assignment("event-override", "focus", "pause-music"),
                assignment("event-snapshot", "short-break", "pause-music"),
                assignment("project-default", "long-break", "pause-music"),
            ]) {
                row.upsert(&mut connection).await.unwrap();
            }

            MusicAssignment::remove_stale(&mut connection, "source", &kept)
                .await
                .unwrap();
            drop(connection);
            let expected = |kind: &str, phase: &str, behavior: &str| {
                (kind.to_string(), phase.to_string(), behavior.to_string())
            };
            assert_eq!(
                stored(&pool).await,
                vec![
                    expected("event-override", "short-break", "pause-music"),
                    expected("event-snapshot", "focus", "keep-current-music"),
                    expected("project-default", "long-break", "pause-music"),
                ]
            );

            let mut connection = pool.acquire().await.unwrap();
            MusicAssignment::remove_stale(&mut connection, "source", &[])
                .await
                .unwrap();
            drop(connection);
            assert_eq!(
                stored(&pool).await,
                vec![expected("project-default", "long-break", "pause-music")]
            );
        });
    }

    #[test]
    fn upserting_a_changed_assignment_updates_it_in_place_and_keeps_its_key() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let mut connection = pool.acquire().await.unwrap();
            let mut row = assignment("event-snapshot", "focus", "keep-current-music");
            row.upsert(&mut connection).await.unwrap();
            let key: String = sqlx::query_scalar(
                "SELECT sync_key FROM music_context_assignments WHERE owner_id = 'source'",
            )
            .fetch_one(&mut *connection)
            .await
            .unwrap();
            row.behavior = "pause-music".into();
            row.upsert(&mut connection).await.unwrap();
            let after: (String, String) = sqlx::query_as(
                "SELECT behavior, sync_key FROM music_context_assignments WHERE owner_id = 'source'",
            )
            .fetch_one(&mut *connection)
            .await
            .unwrap();
            assert_eq!(after, ("pause-music".to_string(), key));
        });
    }
}
