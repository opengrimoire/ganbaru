//! Desktop performance budgets of the sync engine, measured on temporary in-memory replicas.
//!
//! Ignored by default because timings only mean something in the release profile:
//! `cargo test -p ganbaru-sync --lib --release -j 1 budgets -- --ignored --nocapture --test-threads=1`.
//! Each budget is checked on the median of several runs. The debug profile prints the
//! measurements without asserting them.

use std::time::{Duration, Instant};

use super::block_on;
use super::projects::{project_sql, task_sql};
use super::replica::{Replica, quoted, rank};

/// Domain writes timed for the capture overhead.
const CAPTURE_WRITES: u32 = 500;
/// Changed notes in one seal.
const SEAL_NOTES: u32 = 100;
/// Operations in one apply batch.
const APPLY_OPS: u32 = 256;
/// Runs of each measurement; the median is checked so one scheduling stall does not decide it.
const RUNS: usize = 5;

const CAPTURE_BUDGET: Duration = Duration::from_micros(200);
const SEAL_BUDGET: Duration = Duration::from_millis(20);
const APPLY_BUDGET: Duration = Duration::from_millis(50);

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
