pub(super) use super::super::{
    CalendarEventCreate, CalendarEventMutationTarget, CalendarEventUpdateField,
    CalendarPomodoroConfig, CalendarPomodoroRhythm, CalendarPomodoroSequenceStep,
    apply_update_field, archive_calendar_event_tx, delete_calendar_event_tx,
    insert_calendar_event_row, insert_pomodoro_config, replace_pomodoro_config,
    restore_archived_calendar_event_tx, sanitize_stored_event_description, validate_color,
    validate_event_create, validate_non_negative, validate_positive, validate_priority,
    validate_update_field,
};

pub(super) fn event_create() -> CalendarEventCreate {
    CalendarEventCreate {
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
        description: String::new(),
        rrule: None,
        notifications: None,
        exceptions: None,
        repeat_until: None,
        all_day: false,
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
        rdate: None,
        extended_properties: None,
        organizer: None,
        meeting_enabled: false,
        local_rsvp_status: None,
        guest_can_modify: false,
        guest_can_invite_others: true,
        guest_can_see_other_guests: true,
        created_at: "2026-05-09 10:00:00".to_string(),
        updated_at: "2026-05-09 10:00:00".to_string(),
        pomodoro_config: None,
        attendees: Vec::new(),
    }
}

pub(super) fn pomodoro_config() -> CalendarPomodoroConfig {
    CalendarPomodoroConfig {
        rhythm: CalendarPomodoroRhythm::Count {
            focus_duration_minutes: 40,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        },
        rhythm_source: "preset".to_string(),
        preset_key: Some("adaptive".to_string()),
        idle_timeout_minutes: Some(3),
    }
}

pub(super) fn sequence_pomodoro_config() -> CalendarPomodoroConfig {
    CalendarPomodoroConfig {
        rhythm: CalendarPomodoroRhythm::Sequence {
            steps: vec![
                CalendarPomodoroSequenceStep {
                    focus_duration_minutes: 25,
                    break_phase: "short_break".to_string(),
                    break_duration_minutes: 5,
                },
                CalendarPomodoroSequenceStep {
                    focus_duration_minutes: 35,
                    break_phase: "long_break".to_string(),
                    break_duration_minutes: 12,
                },
            ],
        },
        rhythm_source: "custom".to_string(),
        preset_key: None,
        idle_timeout_minutes: None,
    }
}

pub(super) fn invalid_sequence_pomodoro_config() -> CalendarPomodoroConfig {
    CalendarPomodoroConfig {
        rhythm: CalendarPomodoroRhythm::Sequence { steps: Vec::new() },
        rhythm_source: "custom".to_string(),
        preset_key: None,
        idle_timeout_minutes: None,
    }
}

pub(super) async fn in_memory_pool() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    pool
}

pub(super) async fn insert_test_event(pool: &sqlx::SqlitePool, id: &str, description: &str) {
    insert_test_event_at(
        pool,
        id,
        "2026-05-09T10:00:00Z",
        "2026-05-09T11:00:00Z",
        None,
    )
    .await;
    sqlx::query("UPDATE calendar_events SET description = ? WHERE id = ?")
        .bind(description)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

pub(super) async fn insert_test_event_at(
    pool: &sqlx::SqlitePool,
    id: &str,
    start_time: &str,
    end_time: &str,
    rrule: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO calendar_events
           (id, title, start_time, end_time, timezone, calendar_id,
            color, description, rrule, repeat_until, all_day, location, url,
            transparency, status, source_uid, visibility, priority, geo_lat, geo_lng,
            sequence,
            guest_can_modify, guest_can_invite_others, guest_can_see_other_guests,
            created_at, updated_at)
         VALUES (?, '', ?, ?,
            'America/Monterrey', 'local', NULL, '', ?, NULL,
            0, '', '', 'opaque', 'confirmed',
            NULL, 'public', NULL, NULL, NULL, 0,
            0, 1, 1, '2026-05-09 10:00:00', '2026-05-09 10:00:00')",
    )
    .bind(id)
    .bind(start_time)
    .bind(end_time)
    .bind(rrule)
    .execute(pool)
    .await
    .unwrap();
}

pub(super) async fn insert_test_open_pomodoro_run(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO pomodoro_runs
            (id, event_id, original_event_id, event_date, planned_start, planned_end,
             started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
             start_trigger)
         VALUES ('run-1', 'event-1', 'event-1', '2026-05-09',
                 '2026-05-09T10:00:00Z', '2026-05-09T11:00:00Z',
                 '2026-05-09T10:00:00Z', 'count', 'preset', 'adaptive',
                 '2026-05-09T10:05:00Z', 'manual')",
    )
    .execute(pool)
    .await
    .unwrap();
}

pub(super) async fn insert_test_active_pomodoro_segment(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO pomodoro_segments
            (id, event_id, event_date, run_id, rhythm_position, phase,
             planned_start, planned_end, actual_start, status)
         VALUES ('segment-1', 'event-1', '2026-05-09', 'run-1', 1, 'focus',
                 '2026-05-09T10:00:00Z', '2026-05-09T10:40:00Z',
                 '2026-05-09T10:00:00Z', 'active')",
    )
    .execute(pool)
    .await
    .unwrap();
}

pub(super) async fn insert_test_completed_pomodoro_history(
    pool: &sqlx::SqlitePool,
    event_id: &str,
    original_event_id: &str,
    event_date: &str,
) {
    insert_completed_history_with_ids(
        pool,
        event_id,
        original_event_id,
        event_date,
        "run-1",
        "segment-1",
    )
    .await;
}

pub(super) async fn insert_completed_history_with_ids(
    pool: &sqlx::SqlitePool,
    event_id: &str,
    original_event_id: &str,
    event_date: &str,
    run_id: &str,
    segment_id: &str,
) {
    let planned_start = format!("{event_date}T10:00:00Z");
    let segment_end = format!("{event_date}T10:40:00Z");
    let planned_end = format!("{event_date}T11:00:00Z");
    sqlx::query(
        "INSERT INTO pomodoro_runs
            (id, event_id, original_event_id, event_date, planned_start, planned_end,
             started_at, ended_at, end_reason, rhythm_kind, rhythm_source, preset_key,
             last_heartbeat,
             start_trigger)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'completed',
                 'count', 'preset', 'adaptive', ?, 'manual')",
    )
    .bind(run_id)
    .bind(event_id)
    .bind(original_event_id)
    .bind(event_date)
    .bind(&planned_start)
    .bind(&planned_end)
    .bind(&planned_start)
    .bind(&segment_end)
    .bind(&segment_end)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO pomodoro_segments
            (id, event_id, event_date, run_id, rhythm_position, phase,
             planned_start, planned_end, actual_start, actual_end, status, end_reason)
         VALUES (?, ?, ?, ?, 1, 'focus',
                 ?, ?, ?, ?, 'completed', 'completed')",
    )
    .bind(segment_id)
    .bind(event_id)
    .bind(event_date)
    .bind(run_id)
    .bind(&planned_start)
    .bind(&segment_end)
    .bind(&planned_start)
    .bind(&segment_end)
    .execute(pool)
    .await
    .unwrap();
}
