use super::*;

async fn fixture() -> (SqlitePool, Window) {
    let pool = super::super::test_pool().await;
    let window = Window::new(
        "2026-10-04",
        "2026-10-04",
        &civil_time::zone("UTC").unwrap(),
    )
    .unwrap();
    (pool, window)
}

async fn event(pool: &SqlitePool, id: &str, start: &str, end: &str) {
    sqlx::query("INSERT INTO calendar_events(id, start_time, end_time, created_at) VALUES (?, ?, ?, '2026-10-01T00:00:00Z')")
        .bind(id).bind(start).bind(end).execute(pool).await.unwrap();
}

async fn project(pool: &SqlitePool, window: Window) -> NativeCalendarWindow {
    let mut connection = pool.acquire().await.unwrap();
    let source = native_window::read(&mut connection, &window, WindowPurpose::Music, false)
        .await
        .unwrap();
    source.expand(&window).unwrap()
}

fn instant(label: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(label)
        .unwrap()
        .timestamp_millis()
}

#[tokio::test]
async fn native_music_calendar_selects_earliest_end_then_stable_identity_and_keeps_exact_boundaries()
 {
    let (pool, window) = fixture().await;
    event(
        &pool,
        "late",
        "2026-10-04T10:00:00Z",
        "2026-10-04T12:00:00Z",
    )
    .await;
    for id in ["b", "a"] {
        event(&pool, id, "2026-10-04T10:00:00Z", "2026-10-04T11:00:00Z").await;
    }
    let window = project(&pool, window).await;
    assert!(
        select(&window, instant("2026-10-04T09:59:59Z"))
            .unwrap()
            .0
            .is_none()
    );
    let (winner, boundary) = select(&window, instant("2026-10-04T10:00:00Z")).unwrap();
    assert_eq!(winner.unwrap().event_id, "a");
    assert_eq!(boundary, Some(instant("2026-10-04T11:00:00Z")));
    assert_eq!(
        select(&window, instant("2026-10-04T11:00:00Z"))
            .unwrap()
            .0
            .unwrap()
            .event_id,
        "late"
    );
    assert!(
        select(&window, instant("2026-10-04T12:00:00Z"))
            .unwrap()
            .0
            .is_none()
    );
}

#[tokio::test]
async fn native_music_calendar_never_infers_a_focus_phase_from_calendar_configuration() {
    let (pool, window) = fixture().await;
    for id in ["focus", "all-day", "cancelled", "timed"] {
        event(&pool, id, "2026-10-04T10:00:00Z", "2026-10-04T11:00:00Z").await;
    }
    sqlx::query("INSERT INTO calendar_event_pomodoro_configs(event_id, rhythm_kind, rhythm_source) VALUES ('focus', 'count', 'custom')")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE calendar_events SET all_day = 1 WHERE id = 'all-day'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE calendar_events SET status = 'cancelled' WHERE id = 'cancelled'")
        .execute(&pool)
        .await
        .unwrap();
    let window = project(&pool, window).await;
    assert_eq!(window.events.len(), 1);
    let selected = select(&window, instant("2026-10-04T10:30:00Z"))
        .unwrap()
        .0
        .unwrap();
    assert_eq!(selected.event_id, "timed");
    assert_eq!(selected.owner(), SessionOwner::CalendarEvent);
}

#[tokio::test]
async fn native_music_calendar_consumes_canonical_recurrence_without_reintroducing_excluded_instances()
 {
    let (pool, window) = fixture().await;
    event(
        &pool,
        "series",
        "2026-10-02T10:00:00Z",
        "2026-10-02T11:00:00Z",
    )
    .await;
    sqlx::query("UPDATE calendar_events SET rrule = 'FREQ=DAILY;COUNT=3' WHERE id = 'series'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO calendar_event_exdates(id, event_id, sort_order, occurrence_date) VALUES ('excluded', 'series', 0, '2026-10-04')")
        .execute(&pool).await.unwrap();
    let projected = project(&pool, window).await;
    assert!(
        select(&projected, instant("2026-10-04T10:30:00Z"))
            .unwrap()
            .0
            .is_none()
    );
}
