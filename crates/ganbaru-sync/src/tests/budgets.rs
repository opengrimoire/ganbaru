//! Desktop performance budgets of the sync engine, measured on temporary in-memory replicas.
//!
//! Ignored by default because timings only mean something in the release profile:
//! `cargo test -p ganbaru-sync --lib --release -j 1 budgets -- --ignored --nocapture --test-threads=1`.
//! Each budget is checked on the median of several runs. The debug profile prints the
//! measurements without asserting them.

use std::time::{Duration, Instant};

use ganbaru_sync_contracts::DeviceRole;

use super::block_on;
use super::calendar::{calendar_sql, event_sql};
use super::focus::{close_run_sql, open_run_sql};
use super::projects::{project_sql, task_sql};
use super::replica::{Replica, enroll_entry, person, quoted, rank};
use crate::authorization;
use crate::exclusion::MAX_EXCLUDED_PER_WRITER;

/// Domain writes timed for the capture overhead.
const CAPTURE_WRITES: u32 = 500;
/// Changed notes in one seal.
const SEAL_NOTES: u32 = 100;
/// Operations in one apply batch.
const APPLY_OPS: u32 = 256;
/// Runs of each measurement; the median is checked so one scheduling stall does not decide it.
const RUNS: usize = 5;

/// Entries of the authorization log whose selection is timed.
const LOG_ENTRIES: usize = 1000;

const CAPTURE_BUDGET: Duration = Duration::from_micros(200);
const SEAL_BUDGET: Duration = Duration::from_millis(20);
const APPLY_BUDGET: Duration = Duration::from_millis(50);
const REMOVAL_BUDGET: Duration = Duration::from_secs(1);
/// Imported Calendar events sealed in one batch and applied on a second replica.
const CALENDAR_EVENTS: u32 = 1000;
const CALENDAR_SEAL_BUDGET: Duration = Duration::from_secs(3);
/// The Android apply budget; desktop measurements are recorded against it.
const CALENDAR_APPLY_BUDGET: Duration = Duration::from_secs(10);
/// Most a member's apply may take relative to the anchor's, which retains no displaced versions.
const RETENTION_MARGIN: f64 = 1.1;

fn insert_note(id: u32, title: &str) -> String {
    format!(
        "INSERT INTO quick_notes (id, title, order_key) VALUES ({}, {}, {});",
        quoted(&format!("note-{id}")),
        quoted(title),
        quoted(&rank(id)),
    )
}

/// Prints the median of `runs` and returns a failure message when it misses `budget` in the
/// release profile, so every measurement prints before the test fails.
fn check(name: &str, mut runs: Vec<Duration>, budget: Duration) -> Option<String> {
    runs.sort();
    let measured = runs[runs.len() / 2];
    println!("{name}: median {measured:?} of {runs:?} (budget {budget:?})");
    (!cfg!(debug_assertions) && measured >= budget)
        .then(|| format!("{name} took {measured:?}, budget {budget:?}"))
}

/// Per-write cost of a captured update minus the same update with capture triggers removed.
async fn capture_overhead() -> Duration {
    let mut captured = Replica::new(0, 7).await;
    let mut plain = Replica::new(1, 7).await;
    captured.exec(&insert_note(0, "start")).await;
    plain.exec(&insert_note(0, "start")).await;
    let triggers: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'quick_notes'",
    )
    .fetch_all(&mut plain.conn)
    .await
    .unwrap();
    assert!(
        !triggers.is_empty(),
        "quick_notes should carry capture triggers"
    );
    for trigger in triggers {
        plain.exec(&format!("DROP TRIGGER \"{trigger}\"")).await;
    }

    let mut per_write = Vec::new();
    for replica in [&mut captured, &mut plain] {
        let started = Instant::now();
        // A bound, cached statement like domain commands use, so trigger compilation is paid
        // once and only trigger execution is timed.
        for write in 0..CAPTURE_WRITES {
            sqlx::query("UPDATE quick_notes SET title = ? WHERE id = 'note-0'")
                .bind(format!("title {write}"))
                .execute(&mut replica.conn)
                .await
                .unwrap();
        }
        per_write.push(started.elapsed() / CAPTURE_WRITES);
    }
    per_write[0].saturating_sub(per_write[1])
}

/// Seals one note so the engine's cached statements are prepared, as on a long-lived service
/// connection.
async fn warm_seal(replica: &mut Replica, id: u32) {
    replica.exec(&insert_note(id, "warm")).await;
    assert_eq!(replica.seal().await.unwrap().sealed_ops, 1);
}

/// Moves every operation `from` holds that `to` lacks and applies them, returning the number
/// applied.
async fn deliver(from: &mut Replica, to: &mut Replica) -> (usize, Duration) {
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    to.receive(&ops).await;
    let started = Instant::now();
    let applied = to
        .apply_all()
        .await
        .unwrap()
        .iter()
        .map(|report| report.applied)
        .sum();
    (applied, started.elapsed())
}

async fn seal_of_changed_notes() -> Duration {
    let mut replica = Replica::new(0, 8).await;
    warm_seal(&mut replica, SEAL_NOTES).await;
    for id in 0..SEAL_NOTES {
        replica.exec(&insert_note(id, &format!("note {id}"))).await;
    }
    let started = Instant::now();
    let report = replica.seal().await.unwrap();
    let elapsed = started.elapsed();
    assert!(report.changes >= SEAL_NOTES as usize);
    elapsed
}

async fn apply_of_a_batch() -> Duration {
    let mut author = Replica::new(0, 9).await;
    let mut reader = Replica::new(1, 9).await;
    warm_seal(&mut author, APPLY_OPS).await;
    // The genesis and the warm-up operation prepare the reader's statements.
    assert_eq!(deliver(&mut author, &mut reader).await.0, 2);
    for id in 0..APPLY_OPS {
        author.exec(&insert_note(id, &format!("note {id}"))).await;
        assert_eq!(author.seal().await.unwrap().sealed_ops, 1);
    }
    let (applied, elapsed) = deliver(&mut author, &mut reader).await;
    assert_eq!(applied, APPLY_OPS as usize);
    elapsed
}

/// Applies a batch of Projects task inserts after the project they join has arrived.
async fn apply_of_a_projects_batch() -> Duration {
    let mut author = Replica::new(0, 11).await;
    let mut reader = Replica::new(1, 11).await;
    author.exec(&project_sql()).await;
    author.seal().await.unwrap();
    author.exec(&task_sql("task-warm", "warm", APPLY_OPS)).await;
    assert_eq!(author.seal().await.unwrap().sealed_ops, 1);
    // The genesis, the project rows, and the warm-up task prepare the reader's statements.
    assert!(deliver(&mut author, &mut reader).await.0 > 2);
    for id in 0..APPLY_OPS {
        author
            .exec(&task_sql(&format!("task-{id}"), &format!("task {id}"), id))
            .await;
        assert_eq!(author.seal().await.unwrap().sealed_ops, 1);
    }
    let (applied, elapsed) = deliver(&mut author, &mut reader).await;
    assert_eq!(applied, APPLY_OPS as usize);
    elapsed
}

/// Retained value bytes of the author and a reader after a Quick notes seed, as stored and with
/// nothing elided.
async fn version_bytes_after_a_seed() -> [(usize, usize); 2] {
    let mut author = Replica::new(0, 10).await;
    let mut reader = Replica::new(1, 10).await;
    for id in 0..APPLY_OPS {
        author
            .exec(&format!(
                "{} INSERT INTO quick_note_text_runs (note_id, sort_order, content)
                     VALUES ({}, 0, {});",
                insert_note(id, &format!("note {id}")),
                quoted(&format!("note-{id}")),
                quoted(&format!("body of note {id}")),
            ))
            .await;
    }
    author.seal().await.unwrap();
    deliver(&mut author, &mut reader).await;
    [
        author.check_version_values().await,
        reader.check_version_values().await,
    ]
}

/// Time to apply a batch of `APPLY_OPS` updates that each supersede an applied version, on
/// `reader`. A member keeps the superseded versions until the anchor confirms the update; the
/// anchor keeps none, so the difference is the cost of that retention.
async fn apply_of_superseding_updates(mut reader: Replica) -> Duration {
    let mut author = Replica::new(0, 11).await;
    for id in 0..APPLY_OPS {
        author.exec(&insert_note(id, "seed")).await;
    }
    author.seal().await.unwrap();
    deliver(&mut author, &mut reader).await;
    for id in 0..APPLY_OPS {
        author
            .exec(&format!(
                "UPDATE quick_notes SET title = 'update {id}' WHERE id = 'note-{id}'"
            ))
            .await;
        assert_eq!(author.seal().await.unwrap().sealed_ops, 1);
    }
    let (applied, elapsed) = deliver(&mut author, &mut reader).await;
    assert_eq!(applied, APPLY_OPS as usize);
    elapsed
}

/// Time for a member to apply the removal of a device whose `MAX_EXCLUDED_PER_WRITER` applied
/// operations past the cutoff each superseded a version the member retained.
async fn removal_excluding_applied_operations() -> Duration {
    let mut hub = Replica::anchor(0, 12).await;
    let mut removed = Replica::new(1, 12).await;
    let mut member = Replica::new(2, 12).await;
    for id in 0..APPLY_OPS {
        removed.exec(&insert_note(id, "seed")).await;
    }
    removed.seal().await.unwrap();
    hub.pull(&mut removed).await;
    for write in 0..MAX_EXCLUDED_PER_WRITER {
        let id = u32::try_from(write).unwrap() % APPLY_OPS;
        removed
            .exec(&format!(
                "UPDATE quick_notes SET title = 'late {write}' WHERE id = 'note-{id}'"
            ))
            .await;
        assert_eq!(removed.seal().await.unwrap().sealed_ops, 1);
    }
    deliver(&mut removed, &mut member).await;
    hub.remove(&removed.device_id).await.unwrap();
    let (_, elapsed) = deliver(&mut hub, &mut member).await;
    let excluded = member
        .count("SELECT count(*) FROM sync_ops WHERE excluded_revision IS NOT NULL")
        .await;
    assert_eq!(excluded, i64::try_from(MAX_EXCLUDED_PER_WRITER).unwrap());
    let restored = member
        .count("SELECT count(*) FROM quick_notes WHERE title = 'seed'")
        .await;
    assert_eq!(restored, i64::from(APPLY_OPS));
    elapsed
}

/// Time for `member` to apply fork evidence of a writer whose `MAX_EXCLUDED_PER_WRITER` applied
/// operations past the shared prefix each superseded a version. A member restores the versions
/// it retained; the anchor retained none and captures every group the branch wrote instead.
async fn fork_excluding_applied_operations(mut member: Replica, anchor: bool) -> Duration {
    let mut original = Replica::new(1, 14).await;
    for id in 0..APPLY_OPS {
        original.exec(&insert_note(id, "seed")).await;
    }
    original.seal().await.unwrap();
    deliver(&mut original, &mut member).await;
    let mut clone = original.clone_device().await;
    let writer = original.writer();
    let keep = original.stored_vector().await.get(&writer);
    for write in 0..MAX_EXCLUDED_PER_WRITER {
        let id = u32::try_from(write).unwrap() % APPLY_OPS;
        original
            .exec(&format!(
                "UPDATE quick_notes SET title = 'branch {write}' WHERE id = 'note-{id}'"
            ))
            .await;
        assert_eq!(original.seal().await.unwrap().sealed_ops, 1);
    }
    deliver(&mut original, &mut member).await;
    clone
        .exec("UPDATE quick_notes SET title = 'clone' WHERE id = 'note-0'")
        .await;
    assert_eq!(clone.seal().await.unwrap().sealed_ops, 1);
    let first = original
        .engine
        .operation_at(&mut original.conn, &original.ctx, writer, keep + 1)
        .await
        .unwrap()
        .unwrap();
    let second = clone
        .engine
        .operation_at(&mut clone.conn, &clone.ctx, writer, keep + 1)
        .await
        .unwrap()
        .unwrap();
    let outcome = member
        .engine
        .store_fork_evidence(
            &mut member.conn,
            &member.ctx,
            &first,
            &second,
            member.now_ms,
        )
        .await
        .unwrap();
    assert_eq!(outcome, crate::EvidenceOutcome::Stored);
    let started = Instant::now();
    member.apply_all().await.unwrap();
    let elapsed = started.elapsed();
    assert_eq!(
        member.stored_vector().await.get(&writer),
        keep,
        "both branches are excluded"
    );
    if anchor {
        let captured = member.count("SELECT count(*) FROM sync_capture").await;
        assert_eq!(captured, i64::from(APPLY_OPS));
    } else {
        let restored = member
            .count("SELECT count(*) FROM quick_notes WHERE title = 'seed'")
            .await;
        assert_eq!(restored, i64::from(APPLY_OPS));
    }
    elapsed
}

/// Time to select the chain again over an authorization log of `LOG_ENTRIES` entries.
async fn rederive_of_a_long_log() -> Duration {
    let mut hub = Replica::anchor(0, 13).await;
    let base = view_revision(&mut hub).await;
    for index in 0..LOG_ENTRIES {
        hub.sequence(enroll_entry(&person(), 1000 + index, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        view_revision(&mut hub).await,
        base + u64::try_from(LOG_ENTRIES).unwrap()
    );
    let started = Instant::now();
    authorization::rederive(&mut hub.conn, hub.ctx.space)
        .await
        .unwrap()
        .unwrap();
    started.elapsed()
}

async fn view_revision(replica: &mut Replica) -> u64 {
    replica
        .engine
        .authorization(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .unwrap()
        .revision
}

#[test]
#[ignore = "timing budgets are meaningful only in the release profile"]
fn removal_and_retention_budgets_hold() {
    block_on(async {
        let (mut retaining, mut anchor, mut removal, mut rederive) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut fork, mut anchor_fork) = (Vec::new(), Vec::new());
        for _ in 0..RUNS {
            retaining.push(apply_of_superseding_updates(Replica::new(1, 11).await).await);
            anchor.push(apply_of_superseding_updates(Replica::anchor(1, 11).await).await);
            removal.push(removal_excluding_applied_operations().await);
            fork.push(fork_excluding_applied_operations(Replica::new(2, 14).await, false).await);
            anchor_fork
                .push(fork_excluding_applied_operations(Replica::anchor(2, 14).await, true).await);
            rederive.push(rederive_of_a_long_log().await);
        }
        retaining.sort();
        anchor.sort();
        let (kept, none) = (retaining[RUNS / 2], anchor[RUNS / 2]);
        println!(
            "apply of 256 superseding updates: member median {kept:?}, anchor median {none:?}"
        );
        let failures: Vec<String> = [
            check(
                "apply of 256 superseding updates retaining displaced versions",
                retaining,
                none.mul_f64(RETENTION_MARGIN),
            ),
            check(
                "removal excluding 1,024 applied operations",
                removal,
                REMOVAL_BUDGET,
            ),
            check(
                "fork exclusion of 1,024 applied operations",
                fork,
                REMOVAL_BUDGET,
            ),
            check(
                "fork exclusion of 1,024 applied operations on the anchor",
                anchor_fork,
                REMOVAL_BUDGET,
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        // The plan sets no budget for selection; the cost is recorded.
        rederive.sort();
        println!(
            "rederive of a 1,000-entry log: median {:?} of {rederive:?}",
            rederive[RUNS / 2]
        );
        assert!(failures.is_empty(), "{}", failures.join("; "));
    });
}

#[test]
fn elision_keeps_at_most_half_of_the_version_values_after_a_seed() {
    block_on(async {
        for (stored, full) in version_bytes_after_a_seed().await {
            println!("retained version values: {stored} bytes stored of {full}");
            assert!(stored * 2 <= full, "{stored} of {full} bytes stored");
        }
    });
}

#[test]
#[ignore = "timing budgets are meaningful only in the release profile"]
fn budgets_hold_for_capture_seal_and_apply() {
    block_on(async {
        let (mut capture, mut seal, mut apply) = (Vec::new(), Vec::new(), Vec::new());
        let mut apply_projects = Vec::new();
        for _ in 0..RUNS {
            capture.push(capture_overhead().await);
            seal.push(seal_of_changed_notes().await);
            apply.push(apply_of_a_batch().await);
            apply_projects.push(apply_of_a_projects_batch().await);
        }
        let failures: Vec<String> = [
            check("capture overhead per write", capture, CAPTURE_BUDGET),
            check("seal of 100 changed notes", seal, SEAL_BUDGET),
            check("apply of a 256-operation batch", apply, APPLY_BUDGET),
            check(
                "apply of a 256-operation Projects batch",
                apply_projects,
                APPLY_BUDGET,
            ),
        ]
        .into_iter()
        .flatten()
        .collect();
        assert!(failures.is_empty(), "{}", failures.join("; "));
    });
}

/// Seals `CALENDAR_EVENTS` imported events with one alarm each on one replica and applies them
/// on another, returning both durations.
async fn calendar_import_round() -> (Duration, Duration) {
    let mut author = Replica::new(0, 13).await;
    let mut reader = Replica::new(1, 13).await;
    author.exec(&calendar_sql()).await;
    author.seal().await.unwrap();
    // The genesis and the calendar prepare the reader's statements.
    assert!(deliver(&mut author, &mut reader).await.0 >= 2);
    let import: String = (0..CALENDAR_EVENTS)
        .map(|id| event_sql(&format!("event-{id}"), &format!("uid-{id}"), "Imported"))
        .collect();
    author.exec(&import).await;
    // Seal calls continue while captures remain beyond one transaction budget.
    let started = Instant::now();
    let mut changes = 0;
    loop {
        let report = author.seal().await.unwrap();
        assert!(report.invalid_rows.is_empty(), "{:?}", report.invalid_rows);
        changes += report.changes;
        if !report.more {
            break;
        }
    }
    let sealed = started.elapsed();
    assert!(changes >= CALENDAR_EVENTS as usize);
    let (applied, applying) = deliver(&mut author, &mut reader).await;
    assert!(applied >= 1);
    assert_eq!(
        reader.count("SELECT count(*) FROM calendar_events").await,
        i64::from(CALENDAR_EVENTS)
    );
    (sealed, applying)
}

/// Prints the median of `runs` against `budget` without asserting it.
fn record(name: &str, mut runs: Vec<Duration>, budget: Duration) {
    runs.sort();
    let measured = runs[runs.len() / 2];
    println!("{name}: median {measured:?} of {runs:?} (recorded against {budget:?})");
}

/// The Calendar import budgets are recorded, not enforced: the desktop apply measured here
/// stands in for the Android budget, which needs a device.
#[test]
#[ignore = "timing budgets are meaningful only in the release profile"]
fn calendar_import_budgets_are_recorded() {
    block_on(async {
        let (mut seal, mut apply) = (Vec::new(), Vec::new());
        for _ in 0..RUNS {
            let (sealed, applying) = calendar_import_round().await;
            seal.push(sealed);
            apply.push(applying);
        }
        record(
            "seal of 1,000 imported Calendar events",
            seal,
            CALENDAR_SEAL_BUDGET,
        );
        record(
            "apply of 1,000 imported Calendar events",
            apply,
            CALENDAR_APPLY_BUDGET,
        );
    });
}

/// Segments and run events of the closed run whose seal is timed.
const FOCUS_SEGMENTS: u32 = 40;
const FOCUS_RUN_EVENTS: u32 = 300;
/// Block events publish with their run in a later Focus step and are not part of this seal.
const FOCUS_SEAL_BUDGET: Duration = Duration::from_millis(100);

/// Seals one closed run with `FOCUS_SEGMENTS` segments and `FOCUS_RUN_EVENTS` run events.
async fn seal_of_a_closed_run() -> Duration {
    let mut author = Replica::new(0, 17).await;
    author
        .exec(&format!(
            "{}{}{}",
            calendar_sql(),
            event_sql("event-a", "uid-a", "Deep work"),
            open_run_sql("run-a", "event-a", FOCUS_SEGMENTS, FOCUS_RUN_EVENTS)
        ))
        .await;
    author.seal().await.unwrap();
    author.exec(&close_run_sql("run-a", FOCUS_SEGMENTS)).await;
    let started = Instant::now();
    let report = author.seal().await.unwrap();
    let elapsed = started.elapsed();
    assert!(report.invalid_rows.is_empty(), "{:?}", report.invalid_rows);
    assert_eq!(report.sealed_ops, 1);
    assert!(!report.more);
    elapsed
}

#[test]
#[ignore = "timing budgets are meaningful only in the release profile"]
fn focus_seal_of_a_closed_run_holds_its_budget() {
    block_on(async {
        let mut seal = Vec::new();
        for _ in 0..RUNS {
            seal.push(seal_of_a_closed_run().await);
        }
        let failure = check(
            "seal of a closed run with 40 segments and 300 run events",
            seal,
            FOCUS_SEAL_BUDGET,
        );
        assert!(failure.is_none(), "{failure:?}");
    });
}
