#[cfg(test)]
use crate::description::sanitize_calendar_description_html;

#[cfg(test)]
use super::types::{
    CalendarEventCreate, CalendarEventUpdateField, CalendarPomodoroConfig, CalendarPomodoroRhythm,
};
use super::types::{CalendarGeoPayload, CalendarOrganizerPayload};
use super::validation::require_non_empty;
#[cfg(test)]
use super::validation::validate_pomodoro_config;

#[cfg(test)]
pub(super) async fn insert_calendar_event_row(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event: &CalendarEventCreate,
) -> Result<(), String> {
    let description = sanitize_calendar_description_html(&event.description);
    let geo = parse_geo(&event.geo)?;
    sqlx::query(
        "INSERT INTO calendar_events
           (id, title, start_time, end_time, timezone, calendar_id,
            project_id, environment_id, playlist_id, color, description, rrule, repeat_until,
            all_day, location, url, transparency, status,
            source_uid, visibility, priority, geo_lat, geo_lng,
            sequence,
            meeting_enabled, local_rsvp_status,
            guest_can_modify, guest_can_invite_others, guest_can_see_other_guests,
            created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&event.id)
    .bind(&event.title)
    .bind(&event.start_time)
    .bind(&event.end_time)
    .bind(&event.timezone)
    .bind(&event.calendar_id)
    .bind(&event.project_id)
    .bind(&event.environment_id)
    .bind(&event.playlist_id)
    .bind(event.color)
    .bind(&description)
    .bind(&event.rrule)
    .bind(&event.repeat_until)
    .bind(if event.all_day { 1_i64 } else { 0_i64 })
    .bind(&event.location)
    .bind(&event.url)
    .bind(&event.transparency)
    .bind(&event.status)
    .bind(&event.source_uid)
    .bind(&event.visibility)
    .bind(event.priority)
    .bind(geo.map(|value| value.0))
    .bind(geo.map(|value| value.1))
    .bind(event.sequence)
    .bind(if event.meeting_enabled { 1_i64 } else { 0_i64 })
    .bind(&event.local_rsvp_status)
    .bind(if event.guest_can_modify { 1_i64 } else { 0_i64 })
    .bind(if event.guest_can_invite_others {
        1_i64
    } else {
        0_i64
    })
    .bind(if event.guest_can_see_other_guests {
        1_i64
    } else {
        0_i64
    })
    .bind(&event.created_at)
    .bind(&event.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert calendar event: {e}"))?;
    Ok(())
}
#[cfg(test)]
pub(super) async fn replace_pomodoro_config(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_id: &str,
    config: &CalendarPomodoroConfig,
) -> Result<(), String> {
    validate_pomodoro_config(config)?;
    sqlx::query("DELETE FROM calendar_event_pomodoro_configs WHERE event_id = ?")
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete previous pomodoro config: {e}"))?;
    insert_pomodoro_config(tx, event_id, config).await
}
#[cfg(test)]
pub(super) async fn insert_pomodoro_config(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_id: &str,
    config: &CalendarPomodoroConfig,
) -> Result<(), String> {
    validate_pomodoro_config(config)?;
    let rhythm_kind = match &config.rhythm {
        CalendarPomodoroRhythm::Count { .. } => "count",
        CalendarPomodoroRhythm::Sequence { .. } => "sequence",
    };
    sqlx::query(
        "INSERT INTO calendar_event_pomodoro_configs
           (event_id, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(event_id)
    .bind(rhythm_kind)
    .bind(&config.rhythm_source)
    .bind(&config.preset_key)
    .bind(config.idle_timeout_minutes)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro config: {e}"))?;

    match &config.rhythm {
        CalendarPomodoroRhythm::Count {
            focus_duration_minutes,
            short_break_minutes,
            long_break_minutes,
            long_break_after_focus_count,
        } => {
            sqlx::query(
                "INSERT INTO calendar_event_pomodoro_config_count_rhythms
                   (event_id, focus_duration_minutes, short_break_minutes, long_break_minutes,
                    long_break_after_focus_count)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(event_id)
            .bind(focus_duration_minutes)
            .bind(short_break_minutes)
            .bind(long_break_minutes)
            .bind(long_break_after_focus_count)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert count pomodoro rhythm: {e}"))?;
        }
        CalendarPomodoroRhythm::Sequence { steps } => {
            for (step_index, step) in steps.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO calendar_event_pomodoro_config_sequence_steps
                       (event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(event_id)
                .bind(step_index as i64)
                .bind(step.focus_duration_minutes)
                .bind(&step.break_phase)
                .bind(step.break_duration_minutes)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("insert sequence pomodoro rhythm step: {e}"))?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
pub(super) async fn apply_update_field(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    field: &CalendarEventUpdateField,
) -> Result<(), String> {
    match field {
        CalendarEventUpdateField::Title(value) => update_text(tx, id, "title", value).await,
        CalendarEventUpdateField::StartTime(value) => {
            update_text(tx, id, "start_time", value).await
        }
        CalendarEventUpdateField::EndTime(value) => update_text(tx, id, "end_time", value).await,
        CalendarEventUpdateField::Timezone(value) => update_text(tx, id, "timezone", value).await,
        CalendarEventUpdateField::CalendarId(value) => {
            update_text(tx, id, "calendar_id", value).await
        }
        CalendarEventUpdateField::ProjectId(value) => {
            update_optional_text(tx, id, "project_id", value.as_deref()).await
        }
        CalendarEventUpdateField::EnvironmentId(value) => {
            update_optional_text(tx, id, "environment_id", value.as_deref()).await
        }
        CalendarEventUpdateField::PlaylistId(value) => {
            update_optional_text(tx, id, "playlist_id", value.as_deref()).await
        }
        CalendarEventUpdateField::Color(value) => {
            update_optional_i64(tx, id, "color", *value).await
        }
        CalendarEventUpdateField::Description(value) => {
            let description = sanitize_calendar_description_html(value);
            update_text(tx, id, "description", &description).await
        }
        CalendarEventUpdateField::Rrule(value) => {
            update_optional_text(tx, id, "rrule", value.as_deref()).await
        }
        CalendarEventUpdateField::RepeatUntil(value) => {
            update_optional_text(tx, id, "repeat_until", value.as_deref()).await
        }
        CalendarEventUpdateField::Notifications(value) => {
            replace_i64_list(
                tx,
                id,
                "calendar_event_notifications",
                "offset_minutes",
                parse_i64_list(value, "notifications")?,
            )
            .await
        }
        CalendarEventUpdateField::Exceptions(value) => {
            replace_string_list(
                tx,
                id,
                "calendar_event_exdates",
                "occurrence_date",
                parse_string_list(value, "exceptions")?,
            )
            .await
        }
        CalendarEventUpdateField::AllDay(value) => {
            update_i64(tx, id, "all_day", if *value { 1 } else { 0 }).await
        }
        CalendarEventUpdateField::Location(value) => update_text(tx, id, "location", value).await,
        CalendarEventUpdateField::Url(value) => update_text(tx, id, "url", value).await,
        CalendarEventUpdateField::Transparency(value) => {
            update_text(tx, id, "transparency", value).await
        }
        CalendarEventUpdateField::Status(value) => update_text(tx, id, "status", value).await,
        CalendarEventUpdateField::SourceUid(value) => {
            update_optional_text(tx, id, "source_uid", value.as_deref()).await
        }
        CalendarEventUpdateField::Visibility(value) => {
            update_text(tx, id, "visibility", value).await
        }
        CalendarEventUpdateField::Priority(value) => {
            update_optional_i64(tx, id, "priority", *value).await
        }
        CalendarEventUpdateField::Categories(value) => {
            replace_string_list(
                tx,
                id,
                "calendar_event_categories",
                "category",
                parse_string_list(value, "categories")?,
            )
            .await
        }
        CalendarEventUpdateField::Geo(value) => {
            let geo = parse_geo(value)?;
            sqlx::query("UPDATE calendar_events SET geo_lat = ?, geo_lng = ? WHERE id = ?")
                .bind(geo.map(|value| value.0))
                .bind(geo.map(|value| value.1))
                .bind(id)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("update calendar geo: {e}"))?;
            Ok(())
        }
        CalendarEventUpdateField::Sequence(value) => update_i64(tx, id, "sequence", *value).await,
        CalendarEventUpdateField::Rdate(value) => {
            replace_string_list(
                tx,
                id,
                "calendar_event_rdates",
                "occurrence_start",
                parse_string_list(value, "rdate")?,
            )
            .await
        }
        CalendarEventUpdateField::ExtendedProperties(value) => {
            replace_extended_properties(
                tx,
                id,
                "calendar_event_extended_properties",
                "event_id",
                parse_string_map(value, "extended_properties")?,
            )
            .await
        }
        CalendarEventUpdateField::Organizer(value) => {
            replace_organizer(tx, id, parse_organizer(value)?).await
        }
        CalendarEventUpdateField::MeetingEnabled(value) => {
            update_i64(tx, id, "meeting_enabled", if *value { 1 } else { 0 }).await
        }
        CalendarEventUpdateField::LocalRsvpStatus(value) => {
            update_optional_text(tx, id, "local_rsvp_status", value.as_deref()).await
        }
        CalendarEventUpdateField::GuestPermissions(value) => {
            sqlx::query(
                "UPDATE calendar_events
                    SET guest_can_modify = ?,
                        guest_can_invite_others = ?,
                        guest_can_see_other_guests = ?
                  WHERE id = ?",
            )
            .bind(if value.guest_can_modify { 1_i64 } else { 0_i64 })
            .bind(if value.guest_can_invite_others {
                1_i64
            } else {
                0_i64
            })
            .bind(if value.guest_can_see_other_guests {
                1_i64
            } else {
                0_i64
            })
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("update calendar guest permissions: {e}"))?;
            Ok(())
        }
        CalendarEventUpdateField::MusicSnapshotAssignments(_)
        | CalendarEventUpdateField::MusicOverrideAssignments(_) => {
            Err("music assignment fields require the contextual update path".to_string())
        }
    }
}
pub(super) fn parse_i64_list(value: &Option<String>, field: &str) -> Result<Vec<i64>, String> {
    match value {
        Some(json) => serde_json::from_str::<Vec<i64>>(json)
            .map_err(|e| format!("{field} is not a valid integer list: {e}")),
        None => Ok(Vec::new()),
    }
}
pub(super) fn parse_string_list(
    value: &Option<String>,
    field: &str,
) -> Result<Vec<String>, String> {
    match value {
        Some(json) => serde_json::from_str::<Vec<String>>(json)
            .map_err(|e| format!("{field} is not a valid string list: {e}")),
        None => Ok(Vec::new()),
    }
}
pub(super) fn parse_string_map(
    value: &Option<String>,
    field: &str,
) -> Result<Vec<(String, String)>, String> {
    let Some(json) = value else {
        return Ok(Vec::new());
    };
    let map = serde_json::from_str::<std::collections::BTreeMap<String, String>>(json)
        .map_err(|e| format!("{field} is not a valid string map: {e}"))?;
    Ok(map.into_iter().collect())
}
pub(super) fn parse_geo(value: &Option<String>) -> Result<Option<(f64, f64)>, String> {
    let Some(json) = value else {
        return Ok(None);
    };
    let geo = serde_json::from_str::<CalendarGeoPayload>(json)
        .map_err(|e| format!("geo is not valid coordinates: {e}"))?;
    if !geo.lat.is_finite() || !geo.lng.is_finite() {
        return Err("geo coordinates must be finite".to_string());
    }
    Ok(Some((geo.lat, geo.lng)))
}
pub(super) fn parse_organizer(
    value: &Option<String>,
) -> Result<Option<CalendarOrganizerPayload>, String> {
    let Some(json) = value else {
        return Ok(None);
    };
    let organizer = serde_json::from_str::<CalendarOrganizerPayload>(json)
        .map_err(|e| format!("organizer is not valid: {e}"))?;
    require_non_empty(&organizer.email, "organizer.email")?;
    Ok(Some(organizer))
}
#[cfg(test)]
pub(super) async fn replace_i64_list(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_id: &str,
    table: &'static str,
    column: &'static str,
    values: Vec<i64>,
) -> Result<(), String> {
    let delete = format!("DELETE FROM {table} WHERE event_id = ?");
    sqlx::query(&delete)
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar {table}: {e}"))?;
    let insert = format!(
        "INSERT INTO {table} (id, event_id, {column}, sort_order)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?)"
    );
    for (sort_order, value) in values.into_iter().enumerate() {
        sqlx::query(&insert)
            .bind(event_id)
            .bind(value)
            .bind(sort_order as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert calendar {table}: {e}"))?;
    }
    Ok(())
}
#[cfg(test)]
pub(super) async fn replace_string_list(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_id: &str,
    table: &'static str,
    column: &'static str,
    values: Vec<String>,
) -> Result<(), String> {
    let delete = format!("DELETE FROM {table} WHERE event_id = ?");
    sqlx::query(&delete)
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar {table}: {e}"))?;
    let insert = format!(
        "INSERT INTO {table} (id, event_id, {column}, sort_order)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?)"
    );
    for (sort_order, value) in values.into_iter().enumerate() {
        sqlx::query(&insert)
            .bind(event_id)
            .bind(value)
            .bind(sort_order as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert calendar {table}: {e}"))?;
    }
    Ok(())
}
#[cfg(test)]
pub(super) async fn replace_extended_properties(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    owner_id: &str,
    table: &'static str,
    owner_column: &'static str,
    values: Vec<(String, String)>,
) -> Result<(), String> {
    let delete = format!("DELETE FROM {table} WHERE {owner_column} = ?");
    sqlx::query(&delete)
        .bind(owner_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar {table}: {e}"))?;
    let insert = format!(
        "INSERT INTO {table} (id, {owner_column}, property_key, property_value, sort_order)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?)"
    );
    for (sort_order, (key, value)) in values.into_iter().enumerate() {
        sqlx::query(&insert)
            .bind(owner_id)
            .bind(key)
            .bind(value)
            .bind(sort_order as i64)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert calendar {table}: {e}"))?;
    }
    Ok(())
}
#[cfg(test)]
pub(super) async fn replace_organizer(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_id: &str,
    organizer: Option<CalendarOrganizerPayload>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM calendar_event_organizers WHERE event_id = ?")
        .bind(event_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("clear calendar organizer: {e}"))?;
    if let Some(organizer) = organizer {
        sqlx::query(
            "INSERT INTO calendar_event_organizers (event_id, name, email)
             VALUES (?, ?, ?)",
        )
        .bind(event_id)
        .bind(organizer.name)
        .bind(organizer.email)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert calendar organizer: {e}"))?;
    }
    Ok(())
}
#[cfg(test)]
pub(super) async fn update_text(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    column: &'static str,
    value: &str,
) -> Result<(), String> {
    let query = format!("UPDATE calendar_events SET {column} = ? WHERE id = ?");
    sqlx::query(&query)
        .bind(value)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("update calendar column {column}: {e}"))?;
    Ok(())
}
#[cfg(test)]
pub(super) async fn update_optional_text(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    column: &'static str,
    value: Option<&str>,
) -> Result<(), String> {
    let query = format!("UPDATE calendar_events SET {column} = ? WHERE id = ?");
    sqlx::query(&query)
        .bind(value)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("update calendar column {column}: {e}"))?;
    Ok(())
}
#[cfg(test)]
pub(super) async fn update_i64(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    column: &'static str,
    value: i64,
) -> Result<(), String> {
    let query = format!("UPDATE calendar_events SET {column} = ? WHERE id = ?");
    sqlx::query(&query)
        .bind(value)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("update calendar column {column}: {e}"))?;
    Ok(())
}
#[cfg(test)]
pub(super) async fn update_optional_i64(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    column: &'static str,
    value: Option<i64>,
) -> Result<(), String> {
    let query = format!("UPDATE calendar_events SET {column} = ? WHERE id = ?");
    sqlx::query(&query)
        .bind(value)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("update calendar column {column}: {e}"))?;
    Ok(())
}
#[cfg(test)]
pub(super) async fn sanitize_stored_event_description(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
) -> Result<(), String> {
    let description =
        sqlx::query_scalar::<_, String>("SELECT description FROM calendar_events WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load copied calendar description: {e}"))?;
    let Some(description) = description else {
        return Ok(());
    };
    let sanitized = sanitize_calendar_description_html(&description);
    if sanitized != description {
        update_text(tx, id, "description", &sanitized).await?;
    }
    Ok(())
}
