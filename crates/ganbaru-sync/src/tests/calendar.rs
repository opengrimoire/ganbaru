//! Calendar across replicas: edits of different groups both survive, concurrent timing edits
//! surface one conflict everywhere, one file imported on two replicas keeps one event per UID,
//! duplicate repair moves Focus runs to the surviving event while the duplicate's task links
//! stay hidden and its Music assignments keep its id, a re-import after removal keeps the new
//! event, an unchanged re-import keeps a row another
//! replica added, removals that missed a concurrent add or edit
//! are offered for recovery, a deletion undone inside its hold publishes neither the event
//! tombstone nor its archive, and an expired hold publishes the archive once.

use ganbaru_sync_contracts::TableId;
use sqlx::Connection;

use super::block_on;
use super::replica::{Replica, connection_from, converge, device_id, quoted};
use crate::SyncError;
use crate::error::failpoints;
use crate::hold::{self, MAX_HOLD_MS};
use crate::manifest::vault::calendar::{
    ARCHIVES_TABLE, CALENDARS_TABLE, EVENTS_TABLE, TASK_EVENT_LINKS,
};
use crate::manifest::vault::music::ASSIGNMENTS;

const CREATED: &str = "2027-01-01T00:00:00.000Z";
const EDITED: &str = "2027-01-02T00:00:00.000Z";
const CALENDAR: &str = "calendar-shared";
const EVENT: &str = "event-a";
const ARCHIVE: &str = "archive-a";

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 97).await);
    }
    replicas
}

/// SQL adding the shared calendar.
pub(super) fn calendar_sql() -> String {
    format!(
        "INSERT INTO calendars (id, name, source, created_at, updated_at)
         VALUES ({}, 'Shared', 'local', {created}, {created});",
        quoted(CALENDAR),
        created = quoted(CREATED),
    )
}

/// SQL adding an event of the shared calendar imported with `uid`, with one alarm.
pub(super) fn event_sql(id: &str, uid: &str, title: &str) -> String {
    let created = quoted(CREATED);
    format!(
        "INSERT INTO calendar_events
             (id, title, start_time, end_time, calendar_id, source_uid, created_at, updated_at)
         VALUES ({id}, {title}, '2027-01-05T09:00:00Z', '2027-01-05T10:00:00Z', {calendar},
                 {uid}, {created}, {created});
         INSERT INTO calendar_event_alarms (event_id, id, trigger_value)
         VALUES ({id}, {alarm}, '-PT10M');",
        id = quoted(id),
        title = quoted(title),
        calendar = quoted(CALENDAR),
        uid = quoted(uid),
        alarm = quoted(&format!("{id}-alarm")),
    )
}

/// Every replica holding the shared calendar and `sql`, with operations exchanged.
async fn shared(count: usize, sql: &str) -> Vec<Replica> {
    let mut replicas = replicas(count).await;
    replicas[0].exec(&format!("{}{sql}", calendar_sql())).await;
    converge(&mut replicas).await;
    replicas
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

async fn conflicts(replica: &mut Replica) -> i64 {
    replica
        .count("SELECT count(*) FROM sync_rows WHERE conflict_mask <> 0")
        .await
}

#[test]
fn calendar_concurrent_title_and_color_edits_both_survive() {
    block_on(async {
        let mut replicas = shared(2, &event_sql(EVENT, "uid-a", "Draft")).await;
        replicas[0]
            .exec(&format!(
                "UPDATE calendar_events SET title = 'Final', updated_at = {} WHERE id = 'event-a'",
                quoted(EDITED)
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE calendar_events SET color = 7, updated_at = {} WHERE id = 'event-a'",
                quoted(EDITED)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT title || ':' || color FROM calendar_events WHERE id = 'event-a'"
                )
                .await,
                ["Final:7"]
            );
            assert_eq!(conflicts(replica).await, 0);
        }
    });
}

#[test]
fn calendar_concurrent_timing_edits_surface_one_conflict_on_every_replica() {
    block_on(async {
        let mut replicas = shared(3, &event_sql(EVENT, "uid-a", "Draft")).await;
        let moves = [
            ("2027-01-06T09:00:00Z", "2027-01-06T10:00:00Z"),
            ("2027-01-07T15:00:00Z", "2027-01-07T16:30:00Z"),
        ];
        for (replica, (start, end)) in replicas.iter_mut().zip(moves) {
            replica
                .exec(&format!(
                    "UPDATE calendar_events SET start_time = {}, end_time = {}, updated_at = {}
                     WHERE id = 'event-a';
                     INSERT INTO calendar_event_exdates (event_id, occurrence_date, id)
                     VALUES ('event-a', {}, {});",
                    quoted(start),
                    quoted(end),
                    quoted(EDITED),
                    quoted(&start[..10]),
                    quoted(&format!("exdate-{}", &start[..10])),
                ))
                .await;
            replica.seal().await.unwrap();
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let whole: Vec<String> = moves
            .iter()
            .map(|(start, end)| format!("{start}|{end}|{}", &start[..10]))
            .collect();
        for replica in &mut replicas {
            assert_eq!(conflicts(replica).await, 1);
            let timing = texts(
                replica,
                "SELECT start_time || '|' || end_time || '|'
                     || (SELECT group_concat(occurrence_date) FROM calendar_event_exdates
                         WHERE event_id = 'event-a')
                 FROM calendar_events WHERE id = 'event-a'",
            )
            .await;
            assert!(
                whole.contains(&timing[0]),
                "one timing should win whole, owned rows included: {timing:?}"
            );
        }
    });
}

#[test]
fn calendar_one_file_imported_on_two_replicas_keeps_one_event_per_uid() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        for (replica, id) in replicas.iter_mut().zip(["event-left", "event-right"]) {
            replica
                .exec(&format!(
                    "{}{}",
                    event_sql(id, "uid-shared", "Imported"),
                    event_sql(&format!("{id}-own"), &format!("uid-{id}"), "Only here"),
                ))
                .await;
            replica.seal().await.unwrap();
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT source_uid || ':' || count(*) FROM calendar_events
                     GROUP BY source_uid ORDER BY source_uid",
                )
                .await,
                ["uid-event-left:1", "uid-event-right:1", "uid-shared:1"]
            );
            let survivor = texts(
                replica,
                "SELECT id FROM calendar_events WHERE source_uid = 'uid-shared'",
            )
            .await;
            assert_eq!(
                texts(
                    replica,
                    "SELECT a.event_id || '>' || a.id FROM calendar_event_alarms a
                     JOIN calendar_events e ON e.id = a.event_id
                     WHERE e.source_uid = 'uid-shared'",
                )
                .await,
                [format!("{0}>{0}-alarm", survivor[0])],
                "the surviving event keeps its owned rows"
            );
        }
    });
}

/// SQL attaching a closed Focus run, a link from `task`, and a Music assignment for `phase` to
/// `event`, each named after the event.
fn attachments_sql(event: &str, task: &str, phase: &str) -> String {
    let run = format!("{event}-run");
    format!(
        "{open}{close}
         INSERT INTO project_task_event_links (task_id, event_id, link_kind, sync_key)
         VALUES ({task}, {event_id}, 'scheduled', {link});
         INSERT INTO music_context_assignments
             (owner_kind, owner_id, phase, behavior, provenance_kind, updated_at_ms, sync_key)
         VALUES ('event-override', {event_id}, {phase_value}, 'pause-music', 'explicit', 1,
                 {assignment});",
        open = super::focus::open_run_sql(&run, event, 1, 1),
        close = super::focus::close_run_sql(&run, 1),
        task = quoted(task),
        event_id = quoted(event),
        link = quoted(&super::projects::key(&TASK_EVENT_LINKS, &[task, event])),
        phase_value = quoted(phase),
        assignment = quoted(&super::projects::key(
            &ASSIGNMENTS,
            &["event-override", event, phase]
        )),
    )
}

#[test]
fn calendar_duplicate_repair_moves_runs_but_not_task_links_or_music_to_the_surviving_event() {
    block_on(async {
        let mut replicas = shared(
            2,
            &format!(
                "{}{}{}",
                super::projects::project_sql(),
                super::projects::task_sql("task-left", "Left", 1),
                super::projects::task_sql("task-right", "Right", 2),
            ),
        )
        .await;
        for (replica, (event, task, phase)) in replicas.iter_mut().zip([
            ("event-left", "task-left", "focus"),
            ("event-right", "task-right", "short-break"),
        ]) {
            replica
                .exec(&format!(
                    "{}{}",
                    event_sql(event, "uid-shared", "Imported"),
                    attachments_sql(event, task, phase),
                ))
                .await;
            replica.seal().await.unwrap();
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            let survivor = texts(
                replica,
                "SELECT id FROM calendar_events WHERE source_uid = 'uid-shared'",
            )
            .await;
            assert_eq!(
                survivor,
                ["event-left"],
                "duplicate repair keeps the lower key"
            );
            let survivor = &survivor[0];
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || '>' || coalesce(event_id, '') FROM pomodoro_runs ORDER BY id",
                )
                .await,
                [
                    format!("event-left-run>{survivor}"),
                    format!("event-right-run>{survivor}"),
                ]
            );
            assert_eq!(
                texts(
                    replica,
                    "SELECT task_id || '>' || event_id FROM project_task_event_links
                     ORDER BY task_id",
                )
                .await,
                [format!("task-left>{survivor}")],
                "a link names the event in its identity, so the duplicate's link stays hidden"
            );
            assert_eq!(
                texts(
                    replica,
                    "SELECT phase || '>' || owner_id FROM music_context_assignments
                     ORDER BY phase",
                )
                .await,
                ["focus>event-left", "short-break>event-right"],
                "assignment owners are opaque ids, so the duplicate's assignment keeps its id"
            );
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert!(entries.is_empty(), "{entries:?}");
        }
    });
}

#[test]
fn calendar_an_event_removed_and_imported_again_keeps_the_new_event() {
    block_on(async {
        let mut replicas = shared(2, &event_sql(EVENT, "uid-a", "First import")).await;
        replicas[0]
            .exec("DELETE FROM calendar_events WHERE id = 'event-a'")
            .await;
        converge(&mut replicas).await;
        replicas[1]
            .exec(&event_sql("event-again", "uid-a", "Second import"))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || ':' || title FROM calendar_events WHERE source_uid = 'uid-a'",
                )
                .await,
                ["event-again:Second import"]
            );
            assert_eq!(conflicts(replica).await, 0);
        }
    });
}

/// Imports one event with one alarm into the shared calendar through the Calendar import, with
/// payload ids ending in `suffix` as a fresh parse of the same file mints them.
async fn import_file(replica: &mut Replica, suffix: &str) {
    let payload = serde_json::from_value(serde_json::json!({
        "targetCalendarId": CALENDAR,
        "now": CREATED,
        "events": [{
            "candidateId": format!("event-{suffix}"),
            "title": "Imported",
            "startTime": "2027-01-05T09:00:00Z",
            "endTime": "2027-01-05T10:00:00Z",
            "timezone": "UTC",
            "description": "",
            "allDay": false,
            "location": "",
            "url": "",
            "transparency": "opaque",
            "status": "confirmed",
            "sourceUid": "uid-file",
            "visibility": "public",
            "sequence": 0,
            "notifications": "[10]",
            "meetingEnabled": false,
            "guestCanModify": false,
            "guestCanInviteOthers": true,
            "guestCanSeeOtherGuests": true,
            "attendees": [],
            "alarms": [{
                "id": format!("alarm-{suffix}"),
                "action": "display",
                "triggerType": "relative",
                "triggerValue": "-PT10M",
            }],
            "overrides": [],
        }],
    }))
    .unwrap();
    let summary = ganbaru_calendar::import::bulk_import_on(&mut replica.conn, payload)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(summary).unwrap()["warnings"],
        serde_json::json!([])
    );
}

#[test]
fn calendar_an_unchanged_reimport_keeps_an_alarm_another_replica_added() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        import_file(&mut replicas[0], "first").await;
        converge(&mut replicas).await;
        replicas[1]
            .exec(
                "INSERT INTO calendar_event_alarms (event_id, id, trigger_value, sort_order)
                 VALUES ('event-first', 'alarm-added', '-PT1H', 1)",
            )
            .await;
        import_file(&mut replicas[0], "second").await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT event_id || '>' || id FROM calendar_event_alarms ORDER BY id",
                )
                .await,
                ["event-first>alarm-added", "event-first>alarm-first"]
            );
            assert_eq!(conflicts(replica).await, 0);
        }
    });
}

#[test]
fn calendar_removal_against_a_concurrent_event_add_hides_the_event_and_offers_recovery() {
    block_on(async {
        let mut replicas = shared(2, "").await;
        replicas[0]
            .exec(&format!(
                "DELETE FROM calendars WHERE id = {}",
                quoted(CALENDAR)
            ))
            .await;
        replicas[1]
            .exec(&event_sql("event-new", "uid-new", "Added"))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM calendar_events WHERE id = 'event-new'")
                    .await,
                0,
                "the added event stays hidden"
            );
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(entries.len(), 1, "{entries:?}");
            assert_eq!(
                (entries[0].table, entries[0].row_key.as_str()),
                (CALENDARS_TABLE, CALENDAR)
            );
            assert_eq!(entries[0].device_id, device_id(1));
            assert!(entries[0].descendants >= 1, "{:?}", entries[0]);
        }
    });
}

#[test]
fn calendar_an_event_deleted_while_edited_is_offered_for_recovery() {
    block_on(async {
        let mut replicas = shared(2, &event_sql(EVENT, "uid-a", "Draft")).await;
        replicas[0]
            .exec("DELETE FROM calendar_events WHERE id = 'event-a'")
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE calendar_events SET title = 'Final', updated_at = {} WHERE id = 'event-a'",
                quoted(EDITED)
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM calendar_events WHERE id = 'event-a'")
                    .await,
                0
            );
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(entries.len(), 1, "{entries:?}");
            assert_eq!(
                (entries[0].table, entries[0].row_key.as_str()),
                (EVENTS_TABLE, EVENT)
            );
            assert_eq!(entries[0].device_id, device_id(1));
        }
    });
}

/// Archives the shared event and deletes it under a hold, in one transaction like Calendar:
/// the event delete and the archive insert share one lease.
async fn delete_held(replica: &mut Replica) {
    let now = replica.now_ms;
    let mut tx = replica.conn.begin().await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO calendar_event_archives
             (id, source_event_id, archived_at, title, start_time, end_time, calendar_id,
              source_uid, created_at, updated_at)
         SELECT {archive}, id, {edited}, title, start_time, end_time, calendar_id, source_uid,
                created_at, updated_at
         FROM calendar_events WHERE id = 'event-a';
         DELETE FROM calendar_events WHERE id = 'event-a';",
        archive = quoted(ARCHIVE),
        edited = quoted(EDITED),
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    hold::hold_creates(
        &replica.engine,
        &mut tx,
        ARCHIVES_TABLE,
        &[ARCHIVE],
        now,
        now + MAX_HOLD_MS / 2,
    )
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

/// Restores the held event and removes its archive, in one transaction like Calendar Undo.
async fn undo(replica: &mut Replica) {
    let now = replica.now_ms;
    let mut tx = replica.conn.begin().await.unwrap();
    assert!(
        hold::release_hold(&mut tx, EVENTS_TABLE, &[EVENT], now)
            .await
            .unwrap()
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "{}DELETE FROM calendar_event_archives WHERE id = {};",
        event_sql(EVENT, "uid-a", "Draft"),
        quoted(ARCHIVE),
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// Merge state rows and tombstones of `table` on `replica`.
async fn sync_state(replica: &mut Replica, table: TableId) -> [i64; 2] {
    [
        replica
            .count(&format!(
                "SELECT count(*) FROM sync_rows WHERE table_id = {}",
                table.0
            ))
            .await,
        replica
            .count(&format!(
                "SELECT count(*) FROM sync_tombstones WHERE table_id = {}",
                table.0
            ))
            .await,
    ]
}

/// Captures still held on `replica`.
async fn held_captures(replica: &mut Replica) -> i64 {
    replica
        .count("SELECT count(*) FROM sync_capture WHERE held_until_ms IS NOT NULL")
        .await
}

#[test]
fn calendar_a_deletion_undone_inside_its_hold_publishes_no_event_tombstone() {
    block_on(async {
        let mut replicas = shared(2, &event_sql(EVENT, "uid-a", "Draft")).await;
        let author = &mut replicas[0];
        delete_held(author).await;
        assert_eq!(held_captures(author).await, 2);
        assert_eq!(author.seal().await.unwrap().changes, 0);
        author.now_ms += MAX_HOLD_MS / 4;
        undo(author).await;
        author.seal().await.unwrap();
        author.now_ms += MAX_HOLD_MS;
        author.seal().await.unwrap();
        assert_eq!(
            author.count("SELECT count(*) FROM sync_capture").await,
            0,
            "the expired archive hold ends with nothing to publish"
        );
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT title FROM calendar_events WHERE id = 'event-a'"
                )
                .await,
                ["Draft"]
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM calendar_event_archives")
                    .await,
                0
            );
            assert_eq!(
                sync_state(replica, ARCHIVES_TABLE).await,
                [0, 0],
                "the archive is never published"
            );
            assert_eq!(
                replica.count("SELECT count(*) FROM sync_tombstones").await,
                0,
                "nothing is tombstoned"
            );
        }
    });
}

#[test]
fn calendar_an_expired_deletion_hold_publishes_the_archive_once_and_survives_a_restart() {
    block_on(async {
        let mut replicas = shared(2, &event_sql(EVENT, "uid-a", "Draft")).await;
        let author = &mut replicas[0];
        let start = author.now_ms;
        delete_held(author).await;

        // A restart inside the lease keeps both holds, and a crash while sealing them at
        // expiry leaves them held.
        author.conn = connection_from(&author.snapshot().await).await;
        assert_eq!(held_captures(author).await, 2);
        assert_eq!(author.seal().await.unwrap().changes, 0);
        author.now_ms = start + MAX_HOLD_MS / 2;
        let before = author.snapshot().await;
        failpoints::arm("hold.before_seal");
        let crashed = author.seal().await;
        failpoints::clear();
        assert!(
            matches!(crashed, Err(SyncError::Failpoint("hold.before_seal"))),
            "{crashed:?}"
        );
        assert!(author.snapshot().await == before, "the crash left changes");
        author.conn = connection_from(&before).await;
        assert_eq!(held_captures(author).await, 2);

        assert_eq!(author.seal().await.unwrap().changes, 2);
        assert_eq!(author.count("SELECT count(*) FROM sync_capture").await, 0);
        assert_eq!(
            author.seal().await.unwrap().sealed_ops,
            0,
            "expiry publishes once"
        );
        let now = author.now_ms;
        assert!(
            !hold::release_hold(&mut author.conn, EVENTS_TABLE, &[EVENT], now)
                .await
                .unwrap(),
            "Undo refuses an expired hold"
        );
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(replica, "SELECT id FROM calendar_event_archives").await,
                [ARCHIVE]
            );
            assert_eq!(sync_state(replica, ARCHIVES_TABLE).await, [1, 0]);
            assert_eq!(
                texts(
                    replica,
                    &format!(
                        "SELECT row_key FROM sync_tombstones WHERE table_id = {}",
                        EVENTS_TABLE.0
                    ),
                )
                .await,
                [EVENT]
            );
        }
    });
}
