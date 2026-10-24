//! Focus across replicas: open runs and heartbeats capture nothing, closing a run publishes it
//! with its segments, pauses, and run events in one seal that a crash never splits, Calendar writes to a closed run's link
//! replicate as Focus operations, runs closed against a concurrently split or deleted event
//! converge, and each device's activity row replicates as one operation per transition.

use sqlx::Connection;

use super::block_on;
use super::calendar::{calendar_sql, event_sql};
use super::replica::{Replica, converge, device_id, quoted};
use crate::error::failpoints;
use crate::{SyncError, SyncResult};
use crate::hold::{self, MAX_HOLD_MS};
use crate::manifest::vault::calendar::EVENTS_TABLE;
use crate::manifest::vault::focus::{
    DEVICE_ACTIVITY_TABLE, RUN_EVENTS_TABLE, RUNS_TABLE, SEGMENTS_TABLE,
};

const EVENT: &str = "event-a";
const RUN: &str = "run-a";
const ARCHIVE: &str = "archive-a";
const STARTED: &str = "2027-01-05T09:00:00.000Z";
const ENDED: &str = "2027-01-05T09:50:00.000Z";
const HEARTBEAT: &str = "2027-01-05T09:20:00.000Z";
const ARCHIVED: &str = "2027-01-06T00:00:00.000Z";
/// Segments and run events of the run the scenarios close.
const SEGMENTS: u32 = 3;
const RUN_EVENTS: u32 = 8;

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 113).await);
    }
    replicas
}

/// Every replica holding the shared calendar and `EVENT`, with operations exchanged.
async fn shared(count: usize) -> Vec<Replica> {
    let mut replicas = replicas(count).await;
    replicas[0]
        .exec(&format!(
            "{}{}",
            calendar_sql(),
            event_sql(EVENT, "uid-a", "Deep work")
        ))
        .await;
    converge(&mut replicas).await;
    replicas
}

fn segment_id(run: &str, index: u32) -> String {
    format!("{run}-segment-{index}")
}

/// SQL starting `run` on `event` with `segments` segments, the last one active with an open
/// pause, and `events` run events spread across the segments.
pub(super) fn open_run_sql(run: &str, event: &str, segments: u32, events: u32) -> String {
    let started = quoted(STARTED);
    let mut sql = format!(
        "INSERT INTO pomodoro_runs
             (id, event_id, original_event_id, event_date, planned_start, planned_end,
              started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
              event_title_snapshot, current_occurrence_id, current_event_date, created_at)
         VALUES ({run}, {event}, {event}, '2027-01-05', {started}, '2027-01-05T10:00:00.000Z',
                 {started}, 'count', 'preset', 'balanced', {started}, 'Deep work',
                 {occurrence}, '2027-01-05', {started});
         INSERT INTO pomodoro_run_count_rhythms
             (run_id, ordinal, focus_duration_minutes, short_break_minutes, long_break_minutes,
              long_break_after_focus_count)
         VALUES ({run}, 0, 25, 5, 15, 4);",
        run = quoted(run),
        event = quoted(event),
        occurrence = quoted(&format!("{event}::2027-01-05")),
    );
    for index in 0..segments {
        let active = index + 1 == segments;
        let phase = if index % 2 == 0 {
            "focus"
        } else {
            "short_break"
        };
        let (status, actual_end, end_reason) = if active {
            ("active", "NULL".to_owned(), "NULL")
        } else {
            ("completed", started.clone(), "'completed'")
        };
        sql.push_str(&format!(
            "INSERT INTO pomodoro_segments
                 (id, event_id, event_date, run_id, rhythm_position, phase, planned_start,
                  planned_end, actual_start, actual_end, status, end_reason, created_at)
             VALUES ({segment}, {event}, '2027-01-05', {run}, {position}, '{phase}', {started},
                     {started}, {started}, {actual_end}, '{status}', {end_reason}, {started});",
            segment = quoted(&segment_id(run, index)),
            event = quoted(event),
            run = quoted(run),
            position = index / 2 + 1,
        ));
    }
    sql.push_str(&format!(
        "INSERT INTO pomodoro_pauses (segment_id, ordinal, started_at, reason, created_at)
         VALUES ({segment}, 0, {started}, 'manual', {started});",
        segment = quoted(&segment_id(run, segments - 1)),
    ));
    for index in 0..events {
        sql.push_str(&format!(
            "INSERT INTO pomodoro_run_events
                 (id, run_id, segment_id, event_type, occurred_at, phase, created_at)
             VALUES ({id}, {run}, {segment}, 'phase_start', {started}, 'focus', {started});",
            id = quoted(&format!("{run}-event-{index}")),
            run = quoted(run),
            segment = quoted(&segment_id(run, index % segments)),
        ));
    }
    sql
}

/// SQL ending the open pause and active segment of `run` and stopping it, as Focus does in one
/// transaction.
pub(super) fn close_run_sql(run: &str, segments: u32) -> String {
    let ended = quoted(ENDED);
    format!(
        "UPDATE pomodoro_pauses SET ended_at = {ended} WHERE segment_id = {segment};
         UPDATE pomodoro_segments SET status = 'completed', actual_end = {ended},
             end_reason = 'stopped' WHERE id = {segment};
         INSERT INTO pomodoro_run_events
             (id, run_id, segment_id, event_type, occurred_at, created_at)
         VALUES ({stop}, {run}, {segment}, 'stop', {ended}, {ended});
         UPDATE pomodoro_runs SET ended_at = {ended}, end_reason = 'stopped',
             last_heartbeat = {ended} WHERE id = {run};",
        run = quoted(run),
        segment = quoted(&segment_id(run, segments - 1)),
        stop = quoted(&format!("{run}-stop")),
    )
}

/// Captured Focus history rows as `table key`, in table and key order.
async fn focus_captures(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT table_id || ' ' || row_key FROM sync_capture
         WHERE table_id BETWEEN ? AND ? ORDER BY table_id, row_key",
    )
    .bind(i64::from(RUNS_TABLE.0))
    .bind(i64::from(RUN_EVENTS_TABLE.0))
    .fetch_all(&mut replica.conn)
    .await
    .unwrap()
}

async fn texts(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

async fn assert_converged(replicas: &mut [Replica]) {
    let first = replicas[0].domain_dump().await;
    for replica in replicas.iter_mut().skip(1) {
        assert_eq!(
            replica.domain_dump().await,
            first,
            "replica {} diverged",
            replica.index
        );
    }
}

/// History rows of `RUN` on `replica` as `runs segments pauses events`.
async fn history(replica: &mut Replica) -> [i64; 4] {
    [
        replica
            .count("SELECT count(*) FROM pomodoro_runs WHERE id = 'run-a'")
            .await,
        replica
            .count("SELECT count(*) FROM pomodoro_segments WHERE run_id = 'run-a'")
            .await,
        replica
            .count(
                "SELECT count(*) FROM pomodoro_pauses p
                 JOIN pomodoro_segments s ON s.id = p.segment_id WHERE s.run_id = 'run-a'",
            )
            .await,
        replica
            .count("SELECT count(*) FROM pomodoro_run_events WHERE run_id = 'run-a'")
            .await,
    ]
}

const CLOSED_HISTORY: [i64; 4] = [1, SEGMENTS as i64, 1, RUN_EVENTS as i64 + 1];

/// Replicas sharing `EVENT` and the closed `RUN` written on replica 0.
async fn closed(count: usize) -> Vec<Replica> {
    let mut replicas = shared(count).await;
    replicas[0]
        .exec(&format!(
            "{}{}",
            open_run_sql(RUN, EVENT, SEGMENTS, RUN_EVENTS),
            close_run_sql(RUN, SEGMENTS)
        ))
        .await;
    converge(&mut replicas).await;
    replicas
}

#[test]
fn focus_open_runs_and_heartbeats_capture_nothing() {
    block_on(async {
        let mut replicas = shared(1).await;
        let author = &mut replicas[0];
        author
            .exec(&open_run_sql(RUN, EVENT, SEGMENTS, RUN_EVENTS))
            .await;
        for _ in 0..3 {
            author
                .exec(&format!(
                    "UPDATE pomodoro_runs SET last_heartbeat = {} WHERE id = 'run-a'",
                    quoted(HEARTBEAT)
                ))
                .await;
        }
        author
            .exec(
                "UPDATE pomodoro_pauses SET ended_at = '2027-01-05T09:21:00.000Z'
                     WHERE segment_id = 'run-a-segment-2';
                 INSERT INTO pomodoro_pauses (segment_id, ordinal, started_at, reason)
                     VALUES ('run-a-segment-2', 1, '2027-01-05T09:22:00.000Z', 'idle');",
            )
            .await;
        assert_eq!(focus_captures(author).await, Vec::<String>::new());
        let report = author.seal().await.unwrap();
        assert_eq!(report.sealed_ops, 0, "an open run seals nothing");
    });
}

#[test]
fn focus_closing_a_run_publishes_it_with_its_children_in_one_seal() {
    block_on(async {
        let mut replicas = shared(2).await;
        replicas[0]
            .exec(&open_run_sql(RUN, EVENT, SEGMENTS, RUN_EVENTS))
            .await;
        replicas[0].exec(&close_run_sql(RUN, SEGMENTS)).await;
        let captured = focus_captures(&mut replicas[0]).await;
        let in_table = |table: u16| {
            captured
                .iter()
                .filter(|row| row.starts_with(&format!("{table} ")))
                .count()
        };
        assert_eq!(in_table(RUNS_TABLE.0), 1);
        assert_eq!(in_table(SEGMENTS_TABLE.0), SEGMENTS as usize);
        assert_eq!(in_table(RUN_EVENTS_TABLE.0), RUN_EVENTS as usize + 1);
        let report = replicas[0].seal().await.unwrap();
        assert!(report.invalid_rows.is_empty(), "{:?}", report.invalid_rows);
        assert_eq!(
            report.sealed_ops, 1,
            "the run and its children seal together"
        );
        assert!(!report.more);
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(history(&mut replicas[1]).await, CLOSED_HISTORY);
        assert_eq!(
            texts(
                &mut replicas[1],
                "SELECT ordinal || ':' || reason || ':' || ended_at FROM pomodoro_pauses"
            )
            .await,
            [format!("0:manual:{ENDED}")]
        );
    });
}

/// Arms `name`, checks that `call` fails there leaving `replica` exactly as it was, then runs
/// the call again disarmed.
async fn after_rollback<T: std::fmt::Debug>(
    replica: &mut Replica,
    name: &'static str,
    mut call: impl AsyncFnMut(&mut Replica) -> SyncResult<T>,
) -> T {
    let before = replica.snapshot().await;
    failpoints::arm(name);
    let result = call(replica).await;
    failpoints::clear();
    match result {
        Err(SyncError::Failpoint(hit)) => assert_eq!(hit, name),
        other => panic!("{name} did not fail: {other:?}"),
    }
    assert!(replica.snapshot().await == before, "{name} left changes");
    replica.check_invariants().await;
    call(replica).await.unwrap()
}

#[test]
fn focus_a_crash_while_sealing_or_applying_a_closed_run_leaves_no_partial_history() {
    block_on(async {
        let mut replicas = shared(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}",
                open_run_sql(RUN, EVENT, SEGMENTS, RUN_EVENTS),
                close_run_sql(RUN, SEGMENTS)
            ))
            .await;
        for name in ["seal.reserve", "seal.before_commit"] {
            let captured = focus_captures(&mut replicas[0]).await;
            assert!(!captured.is_empty());
            let before = replicas[0].snapshot().await;
            failpoints::arm(name);
            let result = replicas[0].seal().await;
            failpoints::clear();
            assert!(
                matches!(result, Err(SyncError::Failpoint(hit)) if hit == name),
                "{name} did not fail"
            );
            assert!(replicas[0].snapshot().await == before, "{name} left changes");
            assert_eq!(focus_captures(&mut replicas[0]).await, captured);
        }
        let report = replicas[0].seal().await.unwrap();
        assert_eq!(report.sealed_ops, 1, "the run and its children seal together");

        let (author, rest) = replicas.split_at_mut(1);
        let reader = &mut rest[0];
        let known = reader.stored_vector().await;
        let ops = author[0].ops_since(&known).await;
        reader.receive(&ops).await;
        assert_eq!(history(reader).await, [0; 4]);
        after_rollback(reader, "apply.before_commit", async |r: &mut Replica| {
            r.apply_once().await
        })
        .await;
        assert_eq!(history(reader).await, CLOSED_HISTORY);
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn focus_a_calendar_relink_of_a_closed_run_replicates_as_a_focus_operation() {
    block_on(async {
        let mut replicas = closed(2).await;
        let author = &mut replicas[0];
        author
            .exec(&format!(
                "{}UPDATE pomodoro_runs SET event_id = 'event-b', current_occurrence_id = 'event-b'
                 WHERE id = 'run-a' AND ended_at IS NOT NULL;",
                event_sql("event-b", "uid-b", "Moved")
            ))
            .await;
        assert_eq!(
            focus_captures(author).await,
            [format!("{} {RUN}", RUNS_TABLE.0)]
        );
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            texts(
                &mut replicas[1],
                "SELECT event_id || ':' || current_occurrence_id FROM pomodoro_runs"
            )
            .await,
            ["event-b:event-b"]
        );
    });
}

/// Archives `EVENT`, links the closed run to the archive, and deletes the event under a hold,
/// in one transaction like Calendar deletion.
async fn delete_held(replica: &mut Replica) {
    let now = replica.now_ms;
    let mut tx = replica.conn.begin().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO calendar_event_archives
             (id, source_event_id, archived_at, title, start_time, end_time, calendar_id,
              source_uid, created_at, updated_at)
         SELECT {archive}, id, {archived}, title, start_time, end_time, calendar_id, source_uid,
                created_at, updated_at
         FROM calendar_events WHERE id = 'event-a';
         UPDATE pomodoro_runs SET event_id = NULL, calendar_archive_id = {archive}
             WHERE id = 'run-a' AND event_id = 'event-a' AND ended_at IS NOT NULL;
         UPDATE pomodoro_segments SET event_id = NULL WHERE run_id = 'run-a';
         DELETE FROM calendar_events WHERE id = 'event-a';",
        archive = quoted(ARCHIVE),
        archived = quoted(ARCHIVED),
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    hold::hold_deletes(
        &replica.engine,
        &mut tx,
        EVENTS_TABLE,
        &[EVENT],
        now,
        now + MAX_HOLD_MS / 2,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Restores the held event, relinks the run, and removes the archive, like Calendar Undo.
async fn undo(replica: &mut Replica) {
    let now = replica.now_ms;
    let mut tx = replica.conn.begin().await.unwrap();
    assert!(
        hold::release_hold(&mut tx, EVENTS_TABLE, &[EVENT], now)
            .await
            .unwrap()
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "{}UPDATE pomodoro_runs SET event_id = 'event-a', calendar_archive_id = NULL
             WHERE id = 'run-a' AND event_id IS NULL AND calendar_archive_id = {archive}
                 AND ended_at IS NOT NULL;
         DELETE FROM calendar_event_archives WHERE id = {archive};",
        event_sql(EVENT, "uid-a", "Deep work"),
        archive = quoted(ARCHIVE),
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

#[test]
fn focus_a_calendar_deletion_and_undo_relink_resolve_on_a_second_replica() {
    block_on(async {
        let mut replicas = closed(2).await;
        delete_held(&mut replicas[0]).await;
        assert!(
            focus_captures(&mut replicas[0])
                .await
                .contains(&format!("{} {RUN}", RUNS_TABLE.0)),
            "the archive link is captured as Focus"
        );
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;
        assert_eq!(
            texts(
                &mut replicas[1],
                "SELECT coalesce(event_id, '-') || ':' || calendar_archive_id FROM pomodoro_runs"
            )
            .await,
            ["-:archive-a"]
        );

        replicas[0].now_ms += MAX_HOLD_MS / 4;
        undo(&mut replicas[0]).await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT event_id || ':' || coalesce(calendar_archive_id, '-')
                     FROM pomodoro_runs"
                )
                .await,
                ["event-a:-"]
            );
            assert_eq!(history(replica).await, CLOSED_HISTORY);
        }
    });
}

#[test]
fn focus_a_run_closed_on_a_split_occurrence_converges_with_the_split() {
    block_on(async {
        let mut replicas = shared(2).await;
        replicas[0]
            .exec(
                "UPDATE calendar_events SET rrule = 'FREQ=WEEKLY'
                 WHERE id = 'event-a';",
            )
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec(&format!(
                "UPDATE calendar_events SET rrule = 'FREQ=WEEKLY;UNTIL=20270111T000000Z'
                 WHERE id = 'event-a';
                 {}UPDATE calendar_events SET rrule = 'FREQ=WEEKLY' WHERE id = 'event-b';",
                event_sql("event-b", "uid-b", "Deep work"),
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "{}{}",
                open_run_sql(RUN, EVENT, SEGMENTS, RUN_EVENTS),
                close_run_sql(RUN, SEGMENTS)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(history(replica).await, CLOSED_HISTORY);
            assert_eq!(
                texts(
                    replica,
                    "SELECT event_id || ':' || current_occurrence_id FROM pomodoro_runs"
                )
                .await,
                ["event-a:event-a::2027-01-05"],
                "the run keeps the occurrence it was closed on"
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM calendar_events WHERE id = 'event-b'")
                    .await,
                1
            );
        }
    });
}

#[test]
fn focus_a_run_closed_for_an_event_deleted_elsewhere_arrives_without_its_event() {
    block_on(async {
        let mut replicas = shared(2).await;
        replicas[0]
            .exec("DELETE FROM calendar_events WHERE id = 'event-a'")
            .await;
        replicas[1]
            .exec(&format!(
                "{}{}",
                open_run_sql(RUN, EVENT, SEGMENTS, RUN_EVENTS),
                close_run_sql(RUN, SEGMENTS)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(history(replica).await, CLOSED_HISTORY);
            assert_eq!(
                texts(
                    replica,
                    "SELECT coalesce(event_id, '-') || ':' || coalesce(calendar_archive_id, '-')
                         || ':' || event_title_snapshot
                     FROM pomodoro_runs"
                )
                .await,
                ["-:-:Deep work"],
                "the run reads from its title snapshot"
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM pomodoro_segments WHERE event_id IS NOT NULL")
                    .await,
                0
            );
        }
    });
}

fn activity_sql(device: &str, state: &str, run: Option<&str>, expires_at_ms: i64) -> String {
    format!(
        "INSERT INTO pomodoro_device_activity
             (device_id, state, run_id, occurrence_id, event_title, phase_started_at_ms,
              phase_ends_at_ms, expires_at_ms, updated_at_ms)
         VALUES ({device}, '{state}', {run}, 'event-a::2027-01-05', 'Deep work', 1000, 2000,
                 {expires_at_ms}, 1000)
         ON CONFLICT (device_id) DO UPDATE SET state = excluded.state, run_id = excluded.run_id,
             expires_at_ms = excluded.expires_at_ms, updated_at_ms = excluded.updated_at_ms;",
        device = quoted(device),
        run = run.map_or_else(|| "NULL".to_owned(), quoted),
    )
}

#[test]
fn focus_device_activity_seals_one_operation_per_transition_and_converges() {
    block_on(async {
        let mut replicas = shared(2).await;
        for (index, replica) in replicas.iter_mut().enumerate() {
            replica
                .exec(&activity_sql(&device_id(index), "focus", Some(RUN), 5000))
                .await;
            let report = replica.seal().await.unwrap();
            assert!(report.invalid_rows.is_empty(), "{:?}", report.invalid_rows);
            assert_eq!((report.sealed_ops, report.changes), (1, 1));
        }
        replicas[0]
            .exec(&activity_sql(&device_id(0), "paused", Some(RUN), 9000))
            .await;
        let report = replicas[0].seal().await.unwrap();
        assert_eq!((report.sealed_ops, report.changes), (1, 1));
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT device_id || ':' || state || ':' || expires_at_ms
                     FROM pomodoro_device_activity ORDER BY device_id"
                )
                .await,
                [
                    format!("{}:paused:9000", device_id(0)),
                    format!("{}:focus:5000", device_id(1)),
                ]
            );
        }
        assert_eq!(
            replicas[0]
                .count(&format!(
                    "SELECT count(*) FROM sync_rows WHERE table_id = {}",
                    DEVICE_ACTIVITY_TABLE.0
                ))
                .await,
            2
        );
    });
}

#[test]
fn focus_device_activity_with_a_running_state_and_no_run_stays_captured() {
    block_on(async {
        let mut replicas = shared(1).await;
        replicas[0]
            .exec(&activity_sql(&device_id(0), "focus", None, 5000))
            .await;
        let report = replicas[0].seal().await.unwrap();
        assert_eq!(report.sealed_ops, 0);
        assert_eq!(report.invalid_rows.len(), 1);
    });
}
