use super::*;

const OTHER_DEVICE: &str = "device-other";

type Row = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
);

async fn activity(pool: &SqlitePool) -> Option<Row> {
    sqlx::query_as(
        "SELECT state, run_id, occurrence_id, event_title, phase_started_at_ms,
                phase_ends_at_ms, expires_at_ms, updated_at_ms
         FROM pomodoro_device_activity WHERE device_id = ?",
    )
    .bind(LOCAL_DEVICE)
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn other(pool: &SqlitePool, device_id: &str, state: &str, expires_at_ms: i64) {
    let run = (state != "idle").then_some("remote-run");
    sqlx::query(
        "INSERT INTO pomodoro_device_activity
             (device_id, state, run_id, occurrence_id, event_title, expires_at_ms, updated_at_ms)
         VALUES (?, ?, ?, 'focus-event', 'Focus', ?, ?)",
    )
    .bind(device_id)
    .bind(state)
    .bind(run)
    .bind(expires_at_ms)
    .bind(START)
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn activity_is_written_on_each_transition_and_expires_as_specified() {
    block_on(async {
        let pool = pool().await;
        let started = start(&pool).await;
        let run_id = started.run.as_ref().unwrap().id.clone();
        let deadline = started.phase_deadline_ms.unwrap();
        let running = activity(&pool).await.unwrap();
        assert_eq!(
            running,
            (
                "focus".to_owned(),
                Some(run_id.clone()),
                Some("focus-event".to_owned()),
                Some("Focus".to_owned()),
                Some(START),
                Some(deadline),
                deadline + FOCUS_ACTIVITY_RUNNING_GRACE_MS,
                START,
            )
        );

        let heartbeat = observe(&pool, FocusObservation::Heartbeat, &context(START + 10_000)).await;
        assert_eq!(heartbeat.mode, FocusMode::Running);
        assert_eq!(
            activity(&pool).await.unwrap(),
            running,
            "a heartbeat writes nothing"
        );

        let paused_at = START + 30_000;
        execute(
            &pool,
            &command("pause", started.revision, FocusIntent::Pause),
            &context(paused_at),
        )
        .await
        .unwrap();
        let paused = activity(&pool).await.unwrap();
        assert_eq!(paused.0, "paused");
        assert_eq!(paused.1.as_deref(), Some(run_id.as_str()));
        assert_eq!(paused.5, None);
        assert_eq!(paused.6, paused_at + FOCUS_ACTIVITY_PAUSED_GRACE_MS);
        assert_eq!(paused.7, paused_at);

        let resumed = execute(
            &pool,
            &command("resume", started.revision + 1, FocusIntent::Resume),
            &context(START + 90_000),
        )
        .await
        .unwrap();
        let resumed_deadline = resumed.phase_deadline_ms.unwrap();
        let running = activity(&pool).await.unwrap();
        assert_eq!(running.0, "focus");
        assert_eq!(running.5, Some(resumed_deadline));
        assert_eq!(
            running.6,
            resumed_deadline + FOCUS_ACTIVITY_RUNNING_GRACE_MS
        );

        let stopped_at = START + 100_000;
        execute(
            &pool,
            &command("stop", resumed.revision, FocusIntent::Stop),
            &context(stopped_at),
        )
        .await
        .unwrap();
        assert_eq!(
            activity(&pool).await.unwrap(),
            (
                "idle".to_owned(),
                None,
                None,
                None,
                None,
                None,
                stopped_at,
                stopped_at
            ),
            "a stopped device is idle and already expired"
        );
    });
}

#[test]
fn activity_phase_change_moves_the_row_to_the_break() {
    block_on(async {
        let pool = pool().await;
        let started = start(&pool).await;
        let advanced = execute(
            &pool,
            &command("advance", started.revision, FocusIntent::Advance),
            &context(START + 60_000),
        )
        .await
        .unwrap();
        let row = activity(&pool).await.unwrap();
        assert_eq!(row.0, "short_break");
        assert_eq!(row.4, Some(START + 60_000));
        assert_eq!(row.5, advanced.phase_deadline_ms);
    });
}

#[test]
fn activity_is_not_written_without_a_device_or_for_a_device_that_never_focused() {
    block_on(async {
        let pool = pool().await;
        let anonymous = FocusExecutionContext {
            device_id: None,
            ..context(START)
        };
        execute(
            &pool,
            &command(
                "start",
                0,
                FocusIntent::StartScheduled {
                    occurrence_id: None,
                },
            ),
            &anonymous,
        )
        .await
        .unwrap();
        assert_eq!(activity(&pool).await, None);

        let pool = super::pool().await;
        execute(
            &pool,
            &command("stop", 0, FocusIntent::Stop),
            &context(START),
        )
        .await
        .unwrap();
        assert_eq!(
            activity(&pool).await,
            None,
            "an idle device publishes nothing"
        );
    });
}

#[test]
fn activity_reads_return_only_fresh_rows_of_other_devices() {
    block_on(async {
        let pool = pool().await;
        start(&pool).await;
        let now = START + MINUTE;
        other(&pool, OTHER_DEVICE, "focus", now + 1).await;
        other(&pool, "device-expired", "paused", now).await;
        other(&pool, "device-idle", "idle", now + MINUTE).await;
        let mut conn = pool.acquire().await.unwrap();
        let fresh = focus_read_other_device_activity(&mut conn, LOCAL_DEVICE, now)
            .await
            .unwrap();
        assert_eq!(
            fresh,
            [FocusDeviceActivity {
                device_id: OTHER_DEVICE.to_owned(),
                state: FocusActivityState::Focus,
                occurrence_id: Some("focus-event".to_owned()),
                event_title: Some("Focus".to_owned()),
                phase_started_at_ms: None,
                phase_ends_at_ms: None,
                expires_at_ms: now + 1,
                updated_at_ms: START,
            }],
            "the local row, expired rows, and idle rows are left out"
        );
    });
}
