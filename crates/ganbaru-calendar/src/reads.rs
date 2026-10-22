//! Calendar reads: render, Focus, and notification windows, full event rows, and export snapshots.

use crate::description::sanitize_calendar_description_html;
use ganbaru_db::impl_sqlite_from_row;
use serde::Serialize;
use sqlx::{AssertSqlSafe, SqlitePool};

pub(crate) mod event_music;
mod export;
mod export_preservation;
pub mod focus_context;
mod hydration;
mod icalendar;
pub mod native_window;
pub mod project_events;

pub use export::CalendarExportSnapshot;

/// Read and expand canonical Calendar data in one native request.
pub async fn load_native_window(
    pool: &SqlitePool,
    request: native_window::NativeWindowRequest,
) -> Result<native_window::NativeCalendarWindow, String> {
    native_window::load(pool, request, native_window::WindowPurpose::Render).await
}

/// Return native occurrences only for commitments with a persisted Focus configuration.
pub async fn load_native_focus_window(
    pool: &SqlitePool,
    request: native_window::NativeWindowRequest,
) -> Result<native_window::NativeCalendarWindow, String> {
    native_window::load(pool, request, native_window::WindowPurpose::Focus).await
}

/// Read only reminder-bearing canonical events and expand their home-zone occurrences.
pub async fn load_native_notification_window(
    pool: &SqlitePool,
    request: native_window::NativeWindowRequest,
) -> Result<native_window::NativeCalendarWindow, String> {
    native_window::load(pool, request, native_window::WindowPurpose::Notifications).await
}

/// Return one bounded, consistent Calendar export snapshot from the active vault.
pub async fn load_export_snapshot(
    pool: &SqlitePool,
    calendar_id: String,
) -> Result<export::CalendarExportSnapshot, String> {
    export::load_export_snapshot(pool, &calendar_id).await
}

use hydration::{hydrate_full_event_row, hydrate_full_override_rows, hydrate_window_event_rows};

#[derive(Serialize)]
pub struct DbCalendarEventRow {
    pub id: String,
    pub title: String,
    pub(crate) start_time: String,
    pub(crate) end_time: String,
    pub(crate) timezone: String,
    pub(crate) calendar_id: String,
    pub(crate) project_id: Option<String>,
    pub(crate) environment_id: Option<String>,
    pub(crate) playlist_id: Option<String>,
    pub(crate) color: Option<i64>,
    pub(crate) rrule: Option<String>,
    pub(crate) notifications: Option<String>,
    pub(crate) exceptions: Option<String>,
    pub(crate) repeat_until: Option<String>,
    pub all_day: i64,
    pub(crate) location: String,
    pub(crate) has_call_link: i64,
    pub(crate) meeting_enabled: i64,
    pub(crate) has_music_playlist: i64,
    pub(crate) has_soundscape: i64,
    pub(crate) transparency: String,
    pub status: String,
    pub(crate) local_rsvp_status: Option<String>,
    pub created_at: String,
    pub(crate) rdate: Option<String>,
    pub rhythm_kind: Option<String>,
    pub(crate) rhythm_source: Option<String>,
    pub(crate) preset_key: Option<String>,
    pub(crate) count_focus_duration_minutes: Option<i64>,
    pub(crate) count_short_break_minutes: Option<i64>,
    pub(crate) count_long_break_minutes: Option<i64>,
    pub(crate) count_long_break_after_focus_count: Option<i64>,
    pub(crate) sequence_steps: Option<String>,
    pub(crate) idle_timeout_minutes: Option<i64>,
}
impl_sqlite_from_row!(DbCalendarEventRow {
    id,
    title,
    start_time,
    end_time,
    timezone,
    calendar_id,
    project_id,
    environment_id,
    playlist_id,
    color,
    rrule,
    notifications,
    exceptions,
    repeat_until,
    all_day,
    location,
    has_call_link,
    meeting_enabled,
    has_music_playlist,
    has_soundscape,
    transparency,
    status,
    local_rsvp_status,
    created_at,
    rdate,
    rhythm_kind,
    rhythm_source,
    preset_key,
    count_focus_duration_minutes,
    count_short_break_minutes,
    count_long_break_minutes,
    count_long_break_after_focus_count,
    sequence_steps,
    idle_timeout_minutes,
});

#[derive(Serialize)]
pub struct DbOverrideRow {
    pub id: String,
    pub(crate) parent_event_id: String,
    pub(crate) recurrence_id: String,
    pub(crate) recurrence_range: Option<String>,
    pub title: Option<String>,
    pub(crate) start_time: Option<String>,
    pub(crate) end_time: Option<String>,
    pub(crate) color: Option<i64>,
    pub(crate) status: Option<String>,
    pub(crate) transparency: Option<String>,
}
impl_sqlite_from_row!(DbOverrideRow {
    id,
    parent_event_id,
    recurrence_id,
    recurrence_range,
    title,
    start_time,
    end_time,
    color,
    status,
    transparency,
});

#[derive(Serialize)]
pub struct DbAttendeeRow {
    id: String,
    event_id: String,
    icalendar_component_id: Option<String>,
    icalendar_property_index: Option<i64>,
    name: Option<String>,
    email: String,
    role: String,
    status: String,
    rsvp: i64,
    sort_order: i64,
}
impl_sqlite_from_row!(DbAttendeeRow {
    id,
    event_id,
    icalendar_component_id,
    icalendar_property_index,
    name,
    email,
    role,
    status,
    rsvp,
    sort_order,
});

#[derive(Serialize)]
pub struct DbWindowAttendeeRow {
    pub(crate) event_id: String,
    pub(crate) email: String,
    pub(crate) status: String,
}
impl_sqlite_from_row!(DbWindowAttendeeRow {
    event_id,
    email,
    status,
});

#[derive(Serialize)]
pub struct DbAlarmRow {
    id: String,
    event_id: String,
    icalendar_component_id: Option<String>,
    action: String,
    trigger_type: String,
    trigger_value: String,
    description: Option<String>,
    sort_order: i64,
}
impl_sqlite_from_row!(DbAlarmRow {
    id,
    event_id,
    icalendar_component_id,
    action,
    trigger_type,
    trigger_value,
    description,
    sort_order,
});

#[derive(Serialize)]
pub struct DbFullEventRow {
    id: String,
    title: String,
    start_time: String,
    end_time: String,
    timezone: String,
    calendar_id: String,
    project_id: Option<String>,
    environment_id: Option<String>,
    playlist_id: Option<String>,
    color: Option<i64>,
    description: String,
    rrule: Option<String>,
    notifications: Option<String>,
    exceptions: Option<String>,
    repeat_until: Option<String>,
    all_day: i64,
    location: String,
    url: String,
    transparency: String,
    status: String,
    source_uid: Option<String>,
    visibility: String,
    priority: Option<i64>,
    categories: Option<String>,
    geo: Option<String>,
    sequence: i64,
    created_at: String,
    rdate: Option<String>,
    extended_properties: Option<String>,
    organizer: Option<String>,
    meeting_enabled: i64,
    guest_can_modify: i64,
    guest_can_invite_others: i64,
    guest_can_see_other_guests: i64,
    local_rsvp_status: Option<String>,
    icalendar_component_id: Option<String>,
    icalendar_preservation_status: Option<String>,
    icalendar_projection_warnings: Option<String>,
    icalendar_raw_jcal: Option<String>,
    rhythm_kind: Option<String>,
    rhythm_source: Option<String>,
    preset_key: Option<String>,
    count_focus_duration_minutes: Option<i64>,
    count_short_break_minutes: Option<i64>,
    count_long_break_minutes: Option<i64>,
    count_long_break_after_focus_count: Option<i64>,
    sequence_steps: Option<String>,
    idle_timeout_minutes: Option<i64>,
}
impl_sqlite_from_row!(DbFullEventRow {
    id,
    title,
    start_time,
    end_time,
    timezone,
    calendar_id,
    project_id,
    environment_id,
    playlist_id,
    color,
    description,
    rrule,
    notifications,
    exceptions,
    repeat_until,
    all_day,
    location,
    url,
    transparency,
    status,
    source_uid,
    visibility,
    priority,
    categories,
    geo,
    sequence,
    created_at,
    rdate,
    extended_properties,
    organizer,
    meeting_enabled,
    guest_can_modify,
    guest_can_invite_others,
    guest_can_see_other_guests,
    local_rsvp_status,
    icalendar_component_id,
    icalendar_preservation_status,
    icalendar_projection_warnings,
    icalendar_raw_jcal,
    rhythm_kind,
    rhythm_source,
    preset_key,
    count_focus_duration_minutes,
    count_short_break_minutes,
    count_long_break_minutes,
    count_long_break_after_focus_count,
    sequence_steps,
    idle_timeout_minutes,
});

#[derive(Serialize)]
pub struct DbFullOverrideRow {
    id: String,
    parent_event_id: String,
    recurrence_id: String,
    recurrence_range: Option<String>,
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
    extended_properties: Option<String>,
    icalendar_component_id: Option<String>,
    icalendar_raw_jcal: Option<String>,
}
impl_sqlite_from_row!(DbFullOverrideRow {
    id,
    parent_event_id,
    recurrence_id,
    recurrence_range,
    title,
    start_time,
    end_time,
    description,
    location,
    url,
    color,
    status,
    transparency,
    visibility,
    extended_properties,
    icalendar_component_id,
    icalendar_raw_jcal,
});

#[derive(Serialize)]
pub struct CalendarWindowRows {
    events: Vec<DbCalendarEventRow>,
    overrides: Vec<DbOverrideRow>,
    attendees: Vec<DbWindowAttendeeRow>,
    total_event_count: Option<i64>,
}

#[derive(Serialize)]
pub struct CalendarNotificationSchedulerRows {
    events: Vec<DbCalendarEventRow>,
    overrides: Vec<DbOverrideRow>,
}

#[derive(Serialize)]
pub struct CalendarPanelEventRows {
    event: Option<DbFullEventRow>,
    attendees: Vec<DbAttendeeRow>,
}

#[derive(Serialize)]
pub struct CalendarFullEventRows {
    event: Option<DbFullEventRow>,
    attendees: Vec<DbAttendeeRow>,
    alarms: Vec<DbAlarmRow>,
    overrides: Vec<DbFullOverrideRow>,
}

#[derive(Serialize)]
pub struct CalendarIcalendarExportMetadata {
    method: Option<String>,
    mixed_methods: bool,
}

const WINDOW_EVENTS_SQL: &str = r#"
    SELECT ce.id, ce.title, ce.start_time, ce.end_time, ce.timezone,
           ce.calendar_id, ce.project_id, ce.environment_id, ce.playlist_id, ce.color, ce.rrule,
           NULL AS notifications, NULL AS exceptions, ce.repeat_until,
           ce.all_day, ce.location, ce.transparency, ce.status,
           CASE WHEN ce.url <> '' THEN 1 ELSE 0 END AS has_call_link,
           ce.meeting_enabled, 0 AS has_music_playlist, 0 AS has_soundscape,
           ce.local_rsvp_status, ce.created_at,
           NULL AS rdate,
           pc.rhythm_kind, pc.rhythm_source, pc.preset_key,
           pcc.focus_duration_minutes AS count_focus_duration_minutes,
           pcc.short_break_minutes AS count_short_break_minutes,
           pcc.long_break_minutes AS count_long_break_minutes,
           pcc.long_break_after_focus_count AS count_long_break_after_focus_count,
           (
             SELECT '[' || group_concat(step_json) || ']'
             FROM (
               SELECT json_object(
                 'focusDurationMinutes', pcss.focus_duration_minutes,
                 'breakPhase', pcss.break_phase,
                 'breakDurationMinutes', pcss.break_duration_minutes
               ) AS step_json
               FROM calendar_event_pomodoro_config_sequence_steps pcss
               WHERE pcss.event_id = ce.id
               ORDER BY pcss.step_index ASC
             )
           ) AS sequence_steps,
           pc.idle_timeout_minutes
    FROM calendar_events ce
    LEFT JOIN calendar_event_pomodoro_configs pc ON pc.event_id = ce.id
    LEFT JOIN calendar_event_pomodoro_config_count_rhythms pcc ON pcc.event_id = ce.id
    WHERE
      (ce.rrule IS NOT NULL AND ce.rrule <> '')
      OR EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
      OR (
        (ce.rrule IS NULL OR ce.rrule = '')
        AND NOT EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
        AND (
          (ce.all_day = 1 AND substr(ce.end_time, 1, 10) >= ? AND substr(ce.start_time, 1, 10) <= ?)
          OR (ce.all_day <> 1 AND ce.end_time >= ? AND ce.start_time < ?)
        )
      )
    ORDER BY ce.start_time ASC
"#;

const NOTIFICATION_SCHEDULER_EVENTS_SQL: &str = r#"
    SELECT ce.id, ce.title, ce.start_time, ce.end_time, ce.timezone,
           ce.calendar_id, ce.project_id, ce.environment_id, ce.playlist_id, ce.color, ce.rrule,
           NULL AS notifications, NULL AS exceptions, ce.repeat_until,
           ce.all_day, ce.location, ce.transparency, ce.status,
           CASE WHEN ce.url <> '' THEN 1 ELSE 0 END AS has_call_link,
           ce.meeting_enabled, 0 AS has_music_playlist, 0 AS has_soundscape,
           ce.local_rsvp_status, ce.created_at,
           NULL AS rdate,
           pc.rhythm_kind, pc.rhythm_source, pc.preset_key,
           pcc.focus_duration_minutes AS count_focus_duration_minutes,
           pcc.short_break_minutes AS count_short_break_minutes,
           pcc.long_break_minutes AS count_long_break_minutes,
           pcc.long_break_after_focus_count AS count_long_break_after_focus_count,
           (
             SELECT '[' || group_concat(step_json) || ']'
             FROM (
               SELECT json_object(
                 'focusDurationMinutes', pcss.focus_duration_minutes,
                 'breakPhase', pcss.break_phase,
                 'breakDurationMinutes', pcss.break_duration_minutes
               ) AS step_json
               FROM calendar_event_pomodoro_config_sequence_steps pcss
               WHERE pcss.event_id = ce.id
               ORDER BY pcss.step_index ASC
             )
           ) AS sequence_steps,
           pc.idle_timeout_minutes
    FROM calendar_events ce
    LEFT JOIN calendar_event_pomodoro_configs pc ON pc.event_id = ce.id
    LEFT JOIN calendar_event_pomodoro_config_count_rhythms pcc ON pcc.event_id = ce.id
    WHERE EXISTS (
      SELECT 1 FROM calendar_event_notifications n WHERE n.event_id = ce.id
    ) AND (
      (ce.rrule IS NOT NULL AND ce.rrule <> '')
      OR EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
      OR (
        (ce.rrule IS NULL OR ce.rrule = '')
        AND NOT EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
        AND (
          (ce.all_day = 1 AND substr(ce.end_time, 1, 10) >= ? AND substr(ce.start_time, 1, 10) <= ?)
          OR (ce.all_day <> 1 AND ce.end_time >= ? AND ce.start_time < ?)
        )
      )
    )
    ORDER BY ce.start_time ASC, ce.created_at ASC
"#;

const NOTIFICATION_SCHEDULER_OVERRIDES_SQL: &str = r#"
    SELECT o.id, o.parent_event_id, o.recurrence_id, o.recurrence_range, o.title, o.start_time,
           o.end_time, o.color, o.status, o.transparency
    FROM calendar_event_overrides o
    JOIN calendar_events ce ON ce.id = o.parent_event_id
    WHERE EXISTS (
      SELECT 1 FROM calendar_event_notifications n WHERE n.event_id = ce.id
    ) AND (
      (ce.rrule IS NOT NULL AND ce.rrule <> '')
      OR EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
      OR (
        (ce.rrule IS NULL OR ce.rrule = '')
        AND NOT EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
        AND (
          (ce.all_day = 1 AND substr(ce.end_time, 1, 10) >= ? AND substr(ce.start_time, 1, 10) <= ?)
          OR (ce.all_day <> 1 AND ce.end_time >= ? AND ce.start_time < ?)
        )
      )
    )
"#;

const WINDOW_OVERRIDES_SQL: &str = r#"
    SELECT o.id, o.parent_event_id, o.recurrence_id, o.recurrence_range, o.title, o.start_time,
           o.end_time, o.color, o.status, o.transparency
    FROM calendar_event_overrides o
    JOIN calendar_events ce ON ce.id = o.parent_event_id
    WHERE
      (ce.rrule IS NOT NULL AND ce.rrule <> '')
      OR EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
      OR (
        (ce.rrule IS NULL OR ce.rrule = '')
        AND NOT EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
        AND (
          (ce.all_day = 1 AND substr(ce.end_time, 1, 10) >= ? AND substr(ce.start_time, 1, 10) <= ?)
          OR (ce.all_day <> 1 AND ce.end_time >= ? AND ce.start_time < ?)
        )
      )
"#;

const WINDOW_ATTENDEES_SQL: &str = r#"
    SELECT a.event_id, a.email, a.status
    FROM calendar_event_attendees a
    JOIN calendar_events ce ON ce.id = a.event_id
    WHERE
      (ce.rrule IS NOT NULL AND ce.rrule <> '')
      OR EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
      OR (
        (ce.rrule IS NULL OR ce.rrule = '')
        AND NOT EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
        AND (
          (ce.all_day = 1 AND substr(ce.end_time, 1, 10) >= ? AND substr(ce.start_time, 1, 10) <= ?)
          OR (ce.all_day <> 1 AND ce.end_time >= ? AND ce.start_time < ?)
        )
      )
    ORDER BY a.event_id ASC, a.sort_order ASC
"#;

const FULL_EVENT_SELECT_SQL: &str = r#"
    SELECT ce.id, ce.title, ce.start_time, ce.end_time, ce.timezone, ce.calendar_id,
           ce.project_id, ce.environment_id, ce.playlist_id, ce.color, ce.description, ce.rrule,
           NULL AS notifications, NULL AS exceptions, ce.repeat_until,
           ce.all_day, ce.location, ce.url, ce.transparency, ce.status,
           ce.source_uid, ce.visibility, ce.priority,
           NULL AS categories,
           CASE
             WHEN ce.geo_lat IS NULL OR ce.geo_lng IS NULL THEN NULL
             ELSE NULL
           END AS geo,
           ce.sequence,
           ce.created_at,
           NULL AS rdate,
           NULL AS extended_properties,
           NULL AS organizer,
           ce.meeting_enabled, ce.guest_can_modify, ce.guest_can_invite_others,
           ce.guest_can_see_other_guests, ce.local_rsvp_status, ce.icalendar_component_id,
           ic.preservation_status AS icalendar_preservation_status,
           NULL AS icalendar_projection_warnings,
           NULL AS icalendar_raw_jcal,
           pc.rhythm_kind, pc.rhythm_source, pc.preset_key,
           pcc.focus_duration_minutes AS count_focus_duration_minutes,
           pcc.short_break_minutes AS count_short_break_minutes,
           pcc.long_break_minutes AS count_long_break_minutes,
           pcc.long_break_after_focus_count AS count_long_break_after_focus_count,
           (
             SELECT '[' || group_concat(step_json) || ']'
             FROM (
               SELECT json_object(
                 'focusDurationMinutes', pcss.focus_duration_minutes,
                 'breakPhase', pcss.break_phase,
                 'breakDurationMinutes', pcss.break_duration_minutes
               ) AS step_json
               FROM calendar_event_pomodoro_config_sequence_steps pcss
               WHERE pcss.event_id = ce.id
               ORDER BY pcss.step_index ASC
             )
           ) AS sequence_steps,
           pc.idle_timeout_minutes
    FROM calendar_events ce
    LEFT JOIN icalendar_components ic ON ic.id = ce.icalendar_component_id
    LEFT JOIN calendar_event_pomodoro_configs pc ON pc.event_id = ce.id
    LEFT JOIN calendar_event_pomodoro_config_count_rhythms pcc ON pcc.event_id = ce.id
"#;

/// Load the stored events and overrides that intersect one render window.
pub async fn load_window(
    pool: &SqlitePool,
    window_start_date: String,
    window_end_date: String,
    window_start_utc: String,
    window_end_exclusive_utc: String,
    include_total_event_count: Option<bool>,
) -> Result<CalendarWindowRows, String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|e| format!("begin calendar window read: {e}"))?;
    let mut events = sqlx::query_as::<_, DbCalendarEventRow>(WINDOW_EVENTS_SQL)
        .bind(&window_start_date)
        .bind(&window_end_date)
        .bind(&window_start_utc)
        .bind(&window_end_exclusive_utc)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|e| format!("load calendar window events: {e}"))?;
    hydrate_window_event_rows(&mut transaction, &mut events).await?;
    let overrides = if events.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, DbOverrideRow>(WINDOW_OVERRIDES_SQL)
            .bind(&window_start_date)
            .bind(&window_end_date)
            .bind(&window_start_utc)
            .bind(&window_end_exclusive_utc)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|e| format!("load calendar window overrides: {e}"))?
    };
    let attendees = if events.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, DbWindowAttendeeRow>(WINDOW_ATTENDEES_SQL)
            .bind(&window_start_date)
            .bind(&window_end_date)
            .bind(&window_start_utc)
            .bind(&window_end_exclusive_utc)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|e| format!("load calendar window attendees: {e}"))?
    };
    let total_event_count = if include_total_event_count.unwrap_or(false) {
        Some(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM calendar_events")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|e| format!("count calendar events: {e}"))?,
        )
    } else {
        None
    };
    transaction
        .commit()
        .await
        .map_err(|e| format!("commit calendar window read: {e}"))?;

    Ok(CalendarWindowRows {
        events,
        overrides,
        attendees,
        total_event_count,
    })
}

/// Load the reminder-bearing rows the mobile notification scheduler expands.
pub async fn load_notification_scheduler_window(
    pool: &SqlitePool,
    window_start_date: String,
    window_end_date: String,
    window_start_utc: String,
    window_end_exclusive_utc: String,
) -> Result<CalendarNotificationSchedulerRows, String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|e| format!("begin notification calendar window read: {e}"))?;
    let mut events = sqlx::query_as::<_, DbCalendarEventRow>(NOTIFICATION_SCHEDULER_EVENTS_SQL)
        .bind(&window_start_date)
        .bind(&window_end_date)
        .bind(&window_start_utc)
        .bind(&window_end_exclusive_utc)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|e| format!("load notification scheduler events: {e}"))?;
    hydrate_window_event_rows(&mut transaction, &mut events).await?;

    let overrides = if events.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, DbOverrideRow>(NOTIFICATION_SCHEDULER_OVERRIDES_SQL)
            .bind(&window_start_date)
            .bind(&window_end_date)
            .bind(&window_start_utc)
            .bind(&window_end_exclusive_utc)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|e| format!("load notification scheduler overrides: {e}"))?
    };

    transaction
        .commit()
        .await
        .map_err(|e| format!("commit notification calendar window read: {e}"))?;
    Ok(CalendarNotificationSchedulerRows { events, overrides })
}

/// Load one event with the attendees the side panel shows.
pub async fn load_panel_event(
    pool: &SqlitePool,
    id: String,
) -> Result<CalendarPanelEventRows, String> {
    let query = format!("{FULL_EVENT_SELECT_SQL} WHERE ce.id = ?");
    let mut event = sqlx::query_as::<_, DbFullEventRow>(AssertSqlSafe(query.as_str()))
        .bind(&id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load panel calendar event: {e}"))?;
    hydrate_full_event_row(pool, &mut event).await?;
    sanitize_full_event_row(&mut event);
    let attendees = sqlx::query_as::<_, DbAttendeeRow>(
        "SELECT * FROM calendar_event_attendees WHERE event_id = ? ORDER BY sort_order ASC",
    )
    .bind(&id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load panel calendar attendees: {e}"))?;

    Ok(CalendarPanelEventRows { event, attendees })
}

/// Load one event with every child row the full editor needs.
pub async fn load_full_event(
    pool: &SqlitePool,
    id: String,
) -> Result<CalendarFullEventRows, String> {
    let query = format!("{FULL_EVENT_SELECT_SQL} WHERE ce.id = ?");
    let mut event = sqlx::query_as::<_, DbFullEventRow>(AssertSqlSafe(query.as_str()))
        .bind(&id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load full calendar event: {e}"))?;
    hydrate_full_event_row(pool, &mut event).await?;
    let attendees = sqlx::query_as::<_, DbAttendeeRow>(
        "SELECT * FROM calendar_event_attendees WHERE event_id = ? ORDER BY sort_order ASC",
    )
    .bind(&id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load full calendar attendees: {e}"))?;
    let alarms = sqlx::query_as::<_, DbAlarmRow>(
        "SELECT * FROM calendar_event_alarms WHERE event_id = ? ORDER BY sort_order ASC",
    )
    .bind(&id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load full calendar alarms: {e}"))?;
    let mut overrides = sqlx::query_as::<_, DbFullOverrideRow>(
        "SELECT o.id, o.parent_event_id, o.recurrence_id, o.recurrence_range,
                o.title, o.start_time, o.end_time, o.description, o.location,
                o.url, o.color, o.status, o.transparency, o.visibility,
                NULL AS extended_properties, o.icalendar_component_id,
                NULL AS icalendar_raw_jcal
         FROM calendar_event_overrides o
         WHERE o.parent_event_id = ?",
    )
    .bind(&id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load full calendar overrides: {e}"))?;
    hydrate_full_override_rows(pool, &mut overrides).await?;
    sanitize_full_event_row(&mut event);
    sanitize_full_override_rows(&mut overrides);

    Ok(CalendarFullEventRows {
        event,
        attendees,
        alarms,
        overrides,
    })
}

fn sanitize_full_event_row(row: &mut Option<DbFullEventRow>) {
    if let Some(event) = row {
        event.description = sanitize_calendar_description_html(&event.description);
    }
}

fn sanitize_full_override_rows(rows: &mut [DbFullOverrideRow]) {
    for row in rows {
        if let Some(description) = &row.description {
            row.description = Some(sanitize_calendar_description_html(description));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::icalendar::{calendar_icalendar_export_metadata, load_component_value};
    use super::{
        DbFullEventRow, DbFullOverrideRow, sanitize_full_event_row, sanitize_full_override_rows,
    };
    use serde_json::json;

    #[test]
    fn sanitizes_full_event_description_before_returning_rows() {
        let mut row = Some(full_event_row(
            "<p onclick=\"alert(1)\">Safe <a href=\"javascript:alert(1)\">bad</a></p>",
        ));

        sanitize_full_event_row(&mut row);

        assert_eq!(row.unwrap().description, "<p>Safe <a>bad</a></p>");
    }

    #[test]
    fn sanitizes_full_override_description_before_returning_rows() {
        let mut rows = vec![DbFullOverrideRow {
            id: "override-1".to_string(),
            parent_event_id: "event-1".to_string(),
            recurrence_id: "2026-05-10".to_string(),
            recurrence_range: None,
            title: None,
            start_time: None,
            end_time: None,
            description: Some("<div><img src=\"x\"><strong>Safe</strong></div>".to_string()),
            location: None,
            url: None,
            color: None,
            status: None,
            transparency: None,
            visibility: None,
            extended_properties: None,
            icalendar_component_id: None,
            icalendar_raw_jcal: None,
        }];

        sanitize_full_override_rows(&mut rows);

        assert_eq!(
            rows[0].description.as_deref(),
            Some("<div><strong>Safe</strong></div>")
        );
    }

    #[test]
    fn uses_single_preserved_method_for_export_metadata() {
        let metadata = calendar_icalendar_export_metadata(vec![
            " request ".to_string(),
            "REQUEST".to_string(),
        ]);

        assert_eq!(metadata.method.as_deref(), Some("REQUEST"));
        assert!(!metadata.mixed_methods);
    }

    #[test]
    fn drops_mixed_preserved_methods_for_export_metadata() {
        let metadata =
            calendar_icalendar_export_metadata(vec!["REQUEST".to_string(), "CANCEL".to_string()]);

        assert_eq!(metadata.method, None);
        assert!(metadata.mixed_methods);
    }

    #[test]
    fn window_queries_do_not_load_icalendar_preservation_json() {
        for sql in [
            super::WINDOW_EVENTS_SQL,
            super::NOTIFICATION_SCHEDULER_EVENTS_SQL,
            super::NOTIFICATION_SCHEDULER_OVERRIDES_SQL,
            super::WINDOW_OVERRIDES_SQL,
        ] {
            let lower = sql.to_ascii_lowercase();
            assert!(!lower.contains("icalendar_components"));
            assert!(!lower.contains("raw_jcal"));
            assert!(!lower.contains("projection_warnings"));
        }
    }

    #[test]
    fn calendar_window_queries_use_existing_range_and_child_indexes() {
        crate::test_support::block_on(async {
            let pool = migrated_memory_pool().await;
            let bounds = [
                "2026-01-01",
                "2026-01-31",
                "2026-01-01T00:00:00Z",
                "2026-02-01T00:00:00Z",
            ];
            for (label, sql) in [
                ("events", super::WINDOW_EVENTS_SQL),
                ("overrides", super::WINDOW_OVERRIDES_SQL),
                ("attendees", super::WINDOW_ATTENDEES_SQL),
                (
                    "notification events",
                    super::NOTIFICATION_SCHEDULER_EVENTS_SQL,
                ),
                (
                    "notification overrides",
                    super::NOTIFICATION_SCHEDULER_OVERRIDES_SQL,
                ),
            ] {
                let plan_sql = format!("EXPLAIN QUERY PLAN {sql}");
                let rows = sqlx::query(sqlx::AssertSqlSafe(plan_sql.as_str()))
                    .bind(bounds[0])
                    .bind(bounds[1])
                    .bind(bounds[2])
                    .bind(bounds[3])
                    .fetch_all(&pool)
                    .await
                    .unwrap();
                let details = rows
                    .iter()
                    .map(|row| sqlx::Row::get::<String, _>(row, "detail"))
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(
                    details.contains("INDEX"),
                    "{label} plan did not use an index: {details}"
                );
            }
            for (table, index) in [
                (
                    "calendar_event_notifications",
                    "idx_calendar_event_notifications_event",
                ),
                (
                    "calendar_event_exdates",
                    "idx_calendar_event_exdates_event_date",
                ),
                (
                    "calendar_event_rdates",
                    "idx_calendar_event_rdates_event_start",
                ),
            ] {
                let sql = format!(
                    "EXPLAIN QUERY PLAN SELECT event_id FROM {table} WHERE event_id IN ('event-a', 'event-b') ORDER BY event_id, sort_order"
                );
                let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .fetch_all(&pool)
                    .await
                    .unwrap();
                let details = rows
                    .iter()
                    .map(|row| sqlx::Row::get::<String, _>(row, "detail"))
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(
                    details.contains(index),
                    "{table} plan did not use {index}: {details}"
                );
            }
        });
    }

    #[test]
    fn loads_icalendar_object_value_nodes() {
        crate::test_support::block_on(async {
            let pool = migrated_memory_pool().await;
            sqlx::query(
                "INSERT INTO icalendar_objects
                    (id, calendar_id, source_kind, source_name, source_fingerprint,
                     created_at, updated_at)
                 VALUES ('object-1', 'local', 'import-file', 'source.ics',
                         'fingerprint', '2026-05-09', '2026-05-09')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO icalendar_components
                    (id, object_id, calendar_id, component_type, preservation_status,
                     sort_order, created_at, updated_at)
                 VALUES ('component-1', 'object-1', 'local', 'vevent',
                         'partial', 0, '2026-05-09', '2026-05-09')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO icalendar_component_properties
                    (id, component_id, name, value_type, sort_order)
                 VALUES ('property-1', 'component-1', 'rrule', 'recur', 0)",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO icalendar_value_nodes
                    (id, property_id, sort_order, value_kind)
                 VALUES ('value-1', 'property-1', 0, 'object')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO icalendar_value_nodes
                    (id, property_id, parent_node_id, sort_order, value_kind,
                     object_key, text_value)
                 VALUES ('value-1-freq', 'property-1', 'value-1', 0, 'text',
                         'freq', 'DAILY')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO icalendar_value_nodes
                    (id, property_id, parent_node_id, sort_order, value_kind,
                     object_key, number_value)
                 VALUES ('value-1-count', 'property-1', 'value-1', 1, 'number',
                         'count', 3)",
            )
            .execute(&pool)
            .await
            .unwrap();

            let component = load_component_value(&pool, "component-1").await.unwrap();

            assert_eq!(
                component,
                json!([
                    "vevent",
                    [["rrule", {}, "recur", { "freq": "DAILY", "count": 3.0 }]],
                    []
                ]),
            );
        });
    }

    fn full_event_row(description: &str) -> DbFullEventRow {
        DbFullEventRow {
            id: "event-1".to_string(),
            title: "Focus".to_string(),
            start_time: "2026-05-09T10:00:00Z".to_string(),
            end_time: "2026-05-09T11:00:00Z".to_string(),
            timezone: "America/Monterrey".to_string(),
            calendar_id: "local".to_string(),
            project_id: None,
            environment_id: None,
            playlist_id: None,
            color: None,
            description: description.to_string(),
            rrule: None,
            notifications: None,
            exceptions: None,
            repeat_until: None,
            all_day: 0,
            location: String::new(),
            url: String::new(),
            transparency: "opaque".to_string(),
            status: "confirmed".to_string(),
            source_uid: None,
            visibility: "public".to_string(),
            priority: None,
            categories: None,
            geo: None,
            sequence: 0,
            created_at: "2026-05-09T09:00:00Z".to_string(),
            rdate: None,
            extended_properties: None,
            organizer: None,
            meeting_enabled: 0,
            guest_can_modify: 0,
            guest_can_invite_others: 1,
            guest_can_see_other_guests: 1,
            local_rsvp_status: None,
            icalendar_component_id: None,
            icalendar_preservation_status: None,
            icalendar_projection_warnings: None,
            icalendar_raw_jcal: None,
            rhythm_kind: None,
            rhythm_source: None,
            preset_key: None,
            count_focus_duration_minutes: None,
            count_short_break_minutes: None,
            count_long_break_minutes: None,
            count_long_break_after_focus_count: None,
            sequence_steps: None,
            idle_timeout_minutes: None,
        }
    }

    async fn migrated_memory_pool() -> sqlx::SqlitePool {
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
}
