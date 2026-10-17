//! Seeded simulation of installations that edit Quick notes, partly offline, and exchange
//! operations through a hub over a network that drops, duplicates, and reorders messages.
//! Engine calls crash at failpoints and must leave no trace, writers are revoked and re-sealed,
//! and conflicts and recovery offers are resolved along the way. After every step the
//! databases keep their invariants; at quiescence every installation holds the same
//! replicated state.
//!
//! `GANBARU_SYNC_SOAK_SEEDS` sets the number of seeds for a soak run; the default keeps the debug
//! test run near half a minute. A failing run prints its seed, and `GANBARU_SYNC_SEED` replays
//! that seed alone.

use ganbaru_sync_contracts::{Field, RevokeReason, WriterId};

use super::block_on;
use super::replica::{Replica, START_MS, quoted, rank};
use crate::error::failpoints;
use crate::manifest::vault::quick_notes::{NOTES_TABLE, TAGS_TABLE, note_group};
use crate::{ApplyReport, StoreOutcome, StoreRefusal, SyncError, SyncResult};

const DEFAULT_SEEDS: u64 = 48;
const SEEDS_VARIABLE: &str = "GANBARU_SYNC_SOAK_SEEDS";
const SEED_VARIABLE: &str = "GANBARU_SYNC_SEED";
const STEPS: usize = 60;
const TAG_NAMES: &[&str] = &["Work", "work", "Home", "Ideas", "IDEAS", "Errands"];
const WORDS: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "plan", "call", "buy", "read",
];
const ONE_DAY_MS: u64 = 86_400_000;

/// SplitMix64: small, fast, and stable across platforms, so a seed always replays the same run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }
}

/// Prints the seed when a run panics, so the failure can be replayed.
struct SeedGuard(u64);

impl Drop for SeedGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("sync simulation failed at seed {}", self.0);
        }
    }
}

struct Sim {
    rng: Rng,
    replicas: Vec<Replica>,
    hub: Replica,
    offline: Vec<bool>,
    skew_ms: Vec<u64>,
    clock_ms: u64,
    created: usize,
}

/// Runs `call`, first crashing it at a random failpoint now and then. A crashed call must leave
/// the database exactly as it was; the call then runs again without a failpoint.
async fn crashing<T>(
    rng: &mut Rng,
    replica: &mut Replica,
    names: &[&'static str],
    mut call: impl AsyncFnMut(&mut Replica) -> SyncResult<T>,
) -> SyncResult<T> {
    if let Some(&name) = rng.pick(names)
        && rng.chance(15)
    {
        let before = replica.snapshot().await;
        failpoints::arm(name);
        let result = call(replica).await;
        failpoints::clear();
        match result {
            Err(SyncError::Failpoint(hit)) => {
                assert_eq!(hit, name);
                assert!(
                    replica.snapshot().await == before,
                    "{name} left changes on replica {}",
                    replica.index
                );
                replica.check_invariants().await;
            }
            other => return other,
        }
    }
    call(replica).await
}

const SEAL_FAILPOINTS: &[&str] = &["seal.reserve", "seal.before_commit"];
const APPLY_FAILPOINTS: &[&str] = &["apply.before_commit", "seal.reserve", "seal.before_commit"];
const RESEAL_FAILPOINTS: &[&str] = &["reseal.before_commit", "seal.reserve"];
const RECOVERY_FAILPOINTS: &[&str] = &["recovery.before_commit", "seal.reserve"];

/// Seals pending captures, rotating the writer once when it is no longer active.
async fn seal(rng: &mut Rng, replica: &mut Replica) {
    let result = crashing(rng, replica, SEAL_FAILPOINTS, async |r: &mut Replica| {
        r.seal().await
    })
    .await;
    match result {
        Ok(_) => {}
        Err(SyncError::WriterInactive(_)) => {
            replica.rotate();
            crashing(rng, replica, SEAL_FAILPOINTS, async |r: &mut Replica| {
                r.seal().await
            })
            .await
            .unwrap();
        }
        Err(error) => panic!("replica {} failed to seal: {error:?}", replica.index),
    }
}

/// Applies until nothing ready remains, re-sealing and rotating like the app.
async fn apply_all(rng: &mut Rng, replica: &mut Replica) {
    for _ in 0..1_000 {
        let report: ApplyReport =
            crashing(rng, replica, APPLY_FAILPOINTS, async |r: &mut Replica| {
                r.apply_once().await
            })
            .await
            .unwrap_or_else(|error| panic!("replica {} failed to apply: {error:?}", replica.index));
        assert!(
            report.blocked_revocations.is_empty(),
            "replica {} blocked a revocation",
            replica.index
        );
        if let Some(needed) = report.reseal_needed {
            crashing(rng, replica, RESEAL_FAILPOINTS, async |r: &mut Replica| {
                r.reseal(needed).await
            })
            .await
            .unwrap_or_else(|error| {
                panic!("replica {} failed to re-seal: {error:?}", replica.index)
            });
        } else if report.local_writer_frozen {
            replica.rotate();
        } else if !report.more {
            return;
        }
    }
    panic!("replica {} never finished applying", replica.index);
}

/// Stores delivered operations and applies them. Returns whether anything new was stored.
async fn deliver(rng: &mut Rng, to: &mut Replica, ops: &[Vec<u8>]) -> bool {
    let outcomes = crashing(
        rng,
        to,
        &["store.before_commit"],
        async |r: &mut Replica| {
            r.engine
                .store_many(&mut r.conn, &r.ctx, ops, r.now_ms)
                .await
        },
    )
    .await
    .unwrap_or_else(|error| panic!("replica {} failed to store: {error:?}", to.index));
    let mut stored = false;
    for outcome in outcomes {
        match outcome {
            StoreOutcome::Stored { held } => {
                assert_eq!(held, None, "replica {} held an operation", to.index);
                stored = true;
            }
            StoreOutcome::Duplicate
            | StoreOutcome::Refused(
                StoreRefusal::Gap { .. } | StoreRefusal::UnknownWriter | StoreRefusal::Revoked,
            ) => {}
            StoreOutcome::Refused(refusal) => {
                panic!("replica {} refused an operation: {refusal:?}", to.index)
            }
        }
    }
    apply_all(rng, to).await;
    stored
}

/// The operations of a message after the network dropped, duplicated, or reordered some.
fn unreliable(rng: &mut Rng, ops: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    if rng.chance(10) {
        return Vec::new();
    }
    let mut delivered = Vec::with_capacity(ops.len());
    for op in ops {
        if rng.chance(5) {
            continue;
        }
        if rng.chance(5) {
            delivered.push(op.clone());
        }
        delivered.push(op);
    }
    if delivered.len() > 1 && rng.chance(15) {
        for _ in 0..1 + rng.below(3) {
            let a = rng.below(delivered.len());
            let b = rng.below(delivered.len());
            delivered.swap(a, b);
        }
    }
    delivered
}

/// Sends what `from` has and `to` lacks. Returns whether `to` stored anything new.
async fn send(rng: &mut Rng, from: &mut Replica, to: &mut Replica, reliable: bool) -> bool {
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    if ops.is_empty() {
        return false;
    }
    let ops = if reliable { ops } else { unreliable(rng, ops) };
    deliver(rng, to, &ops).await
}

async fn ids(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sql)
        .fetch_all(&mut replica.conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

fn iso_time(ms: u64) -> String {
    format!("strftime('%Y-%m-%dT%H:%M:%fZ', {ms} / 1000.0, 'unixepoch')")
}

impl Sim {
    async fn new(seed: u64) -> Self {
        let mut rng = Rng(seed);
        let count = 3 + rng.below(3);
        let mut replicas = Vec::with_capacity(count);
        let mut skew_ms = Vec::with_capacity(count);
        for index in 0..count {
            replicas.push(Replica::new(index, seed).await);
            // Clocks run up to a day ahead of the hub's.
            skew_ms.push(if rng.chance(30) {
                rng.next() % ONE_DAY_MS
            } else {
                0
            });
        }
        let hub = Replica::new(count, seed).await;
        Self {
            rng,
            replicas,
            hub,
            offline: vec![false; count],
            skew_ms,
            clock_ms: START_MS,
            created: 0,
        }
    }

    fn advance_clocks(&mut self) {
        self.clock_ms += 1 + self.rng.next() % 2_000;
        self.hub.now_ms = self.clock_ms;
        for (replica, skew) in self.replicas.iter_mut().zip(&self.skew_ms) {
            replica.now_ms = self.clock_ms + skew;
        }
    }

    fn next_id(&mut self, prefix: &str, replica: usize) -> String {
        self.created += 1;
        format!("{prefix}-{replica}-{}", self.created)
    }

    fn words(&mut self) -> String {
        let count = 1 + self.rng.below(3);
        (0..count)
            .map(|_| *self.rng.pick(WORDS).unwrap())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// SQL that replaces a note's text runs and keeps its plain text projection in step.
    /// Adjacent runs differ in style, as the editor merges runs that share one.
    fn body_sql(&mut self, note: &str) -> String {
        let mut sql = format!(
            "DELETE FROM quick_note_text_runs WHERE note_id = {};",
            quoted(note)
        );
        let mut plain = String::new();
        let mut previous = None;
        for sort_order in 0..self.rng.below(4) {
            let content = self.words();
            plain.push_str(&content);
            let mut style = self.rng.below(8);
            while previous == Some(style) {
                style = self.rng.below(8);
            }
            previous = Some(style);
            sql.push_str(&format!(
                "INSERT INTO quick_note_text_runs (note_id, sort_order, content, bold, italic, underline)
                 VALUES ({}, {sort_order}, {}, {}, {}, {});",
                quoted(note),
                quoted(&content),
                style & 1,
                (style >> 1) & 1,
                (style >> 2) & 1,
            ));
        }
        sql.push_str(&format!(
            "UPDATE quick_notes SET body_plain_text = {} WHERE id = {};",
            quoted(&plain),
            quoted(note)
        ));
        sql
    }

    /// One local edit of the kind the app makes, in one transaction. Edits that break a domain
    /// constraint roll back, as a refused app command would.
    async fn edit(&mut self, index: usize) {
        let notes = ids(
            &mut self.replicas[index],
            "SELECT id FROM quick_notes ORDER BY id",
        )
        .await;
        let tags = ids(
            &mut self.replicas[index],
            "SELECT id FROM quick_note_tags ORDER BY id",
        )
        .await;
        let now = iso_time(self.replicas[index].now_ms);
        let note = self.rng.pick(&notes).map(|id| quoted(id));
        let tag = self.rng.pick(&tags).map(|id| quoted(id));
        let touch = format!("updated_at = {now}, revision = revision + 1");
        let kind = self.rng.below(15);
        let kind = if note.is_none() && (3..=11).contains(&kind) {
            0
        } else {
            kind
        };
        let sql = match (kind, note, tag) {
            (0..=2, _, tag) => {
                let id = self.next_id("note", index);
                let title = self.words();
                let order = rank(self.rng.below(40) as u32);
                let tag = if self.rng.chance(50) {
                    tag.unwrap_or("NULL".into())
                } else {
                    "NULL".into()
                };
                let body = self.body_sql(&id);
                format!(
                    "INSERT INTO quick_notes (id, title, tag_id, order_key, color, created_at, updated_at)
                     VALUES ({}, {}, {tag}, {}, {}, {now}, {now}); {body}",
                    quoted(&id),
                    quoted(&title),
                    quoted(&order),
                    self.rng.below(32),
                )
            }
            (3, Some(note), _) => {
                format!(
                    "UPDATE quick_notes SET title = {}, {touch} WHERE id = {note}",
                    quoted(&self.words())
                )
            }
            (4, Some(note), _) => {
                let id = note.trim_matches('\'').to_owned();
                let body = self.body_sql(&id);
                format!("{body} UPDATE quick_notes SET {touch} WHERE id = {note}")
            }
            (5, Some(note), _) => {
                format!(
                    "UPDATE quick_notes SET color = {}, {touch} WHERE id = {note}",
                    self.rng.below(32)
                )
            }
            (6, Some(note), _) => {
                format!("UPDATE quick_notes SET pinned = 1 - pinned, {touch} WHERE id = {note}")
            }
            (7, Some(note), _) => {
                format!(
                    "UPDATE quick_notes SET archived = 1 - archived, pinned = 0, {touch} WHERE id = {note}"
                )
            }
            (8, Some(note), _) => format!(
                "UPDATE quick_notes SET trashed_at = CASE WHEN trashed_at IS NULL THEN {now} END,
                     pinned = 0, {touch} WHERE id = {note}"
            ),
            (9, Some(note), _) => format!(
                "UPDATE quick_notes SET order_key = {}, {touch} WHERE id = {note}",
                quoted(&rank(self.rng.below(40) as u32))
            ),
            (10, Some(note), _) => format!("DELETE FROM quick_notes WHERE id = {note}"),
            (11, Some(note), tag) => format!(
                "UPDATE quick_notes SET tag_id = {}, {touch} WHERE id = {note}",
                tag.unwrap_or("NULL".into())
            ),
            (12, _, _) => {
                let id = self.next_id("tag", index);
                format!(
                    "INSERT INTO quick_note_tags (id, name, order_key, created_at, updated_at)
                     VALUES ({}, {}, {}, {now}, {now})",
                    quoted(&id),
                    quoted(self.rng.pick(TAG_NAMES).unwrap()),
                    quoted(&rank(self.rng.below(10) as u32)),
                )
            }
            (13, _, Some(tag)) => {
                if self.rng.chance(50) {
                    format!(
                        "UPDATE quick_note_tags SET name = {}, updated_at = {now} WHERE id = {tag}",
                        quoted(self.rng.pick(TAG_NAMES).unwrap())
                    )
                } else {
                    format!(
                        "UPDATE quick_note_tags SET order_key = {}, updated_at = {now} WHERE id = {tag}",
                        quoted(&rank(self.rng.below(10) as u32))
                    )
                }
            }
            (14, _, Some(tag)) => format!(
                "UPDATE quick_notes SET revision = revision + 1 WHERE tag_id = {tag};
                 DELETE FROM quick_note_tags WHERE id = {tag};"
            ),
            _ => return,
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// Exchanges operations between an online replica and the hub over the unreliable network.
    async fn sync(&mut self, index: usize) {
        let replica = &mut self.replicas[index];
        if self.rng.chance(70) {
            send(&mut self.rng, replica, &mut self.hub, false).await;
        }
        if self.rng.chance(70) {
            send(&mut self.rng, &mut self.hub, replica, false).await;
        }
    }

    /// Resolves a surfaced conflict, keeping either the displayed version or another one.
    async fn resolve(&mut self, index: usize) {
        let replica = &mut self.replicas[index];
        let mut rows = Vec::new();
        for table in [TAGS_TABLE, NOTES_TABLE] {
            rows.extend(
                replica
                    .engine
                    .conflicts(&mut replica.conn, table)
                    .await
                    .unwrap(),
            );
        }
        let Some(row) = self.rng.pick(&rows).cloned() else {
            return;
        };
        let details = replica
            .engine
            .conflict_details(&mut replica.conn, &replica.ctx, row.table, &row.row_key)
            .await
            .unwrap();
        let Some(detail) = self.rng.pick(&details).cloned() else {
            panic!("replica {index} lists a conflict without details");
        };
        assert!(detail.versions.len() >= 2);
        assert_eq!(
            detail
                .versions
                .iter()
                .filter(|version| version.displayed)
                .count(),
            1
        );
        let chosen = self.rng.pick(&detail.versions).unwrap().clone();
        let mut tx = sqlx::Connection::begin(&mut replica.conn).await.unwrap();
        if !chosen.displayed
            && row.table == NOTES_TABLE
            && detail.group == note_group::TITLE
            && let [Field::Text(title)] = chosen.value.fields()
        {
            sqlx::query("UPDATE quick_notes SET title = ? WHERE id = ?")
                .bind(title)
                .bind(&row.row_key)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        replica
            .engine
            .force_group(
                &mut tx,
                row.table,
                &row.row_key,
                detail.group,
                replica.now_ms,
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    /// Restores or discards an offer to recover a deleted row.
    async fn recover(&mut self, index: usize) {
        let entries = {
            let replica = &mut self.replicas[index];
            replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap()
        };
        let Some(entry) = self.rng.pick(&entries).cloned() else {
            return;
        };
        if entry.table == NOTES_TABLE && self.rng.chance(50) {
            let id = self.next_id("note", index);
            let replica = &mut self.replicas[index];
            let now = iso_time(replica.now_ms);
            let title: String = entry.presentation.title.chars().take(200).collect();
            replica
                .exec(&format!(
                    "INSERT INTO quick_notes (id, title, order_key, created_at, updated_at)
                     VALUES ({}, {}, {}, {now}, {now})",
                    quoted(&id),
                    quoted(&title),
                    quoted(&rank(0)),
                ))
                .await;
        }
        let replica = &mut self.replicas[index];
        let mut close = async |r: &mut Replica| {
            r.engine
                .close_recovery(
                    &mut r.conn,
                    &r.ctx,
                    entry.table,
                    &entry.row_key,
                    &mut r.identity.local(),
                    r.now_ms,
                )
                .await
        };
        match crashing(&mut self.rng, replica, RECOVERY_FAILPOINTS, &mut close).await {
            Ok(_) => {}
            Err(SyncError::WriterInactive(_)) => {
                replica.rotate();
                crashing(&mut self.rng, replica, RECOVERY_FAILPOINTS, close)
                    .await
                    .unwrap();
            }
            Err(error) => panic!("replica {index} failed to close a recovery offer: {error:?}"),
        }
    }

    /// The hub unlinks a replica's current writer at the last operation it applied.
    async fn revoke(&mut self, index: usize) {
        let target: WriterId = self.replicas[index].writer();
        let applied = self.hub.applied_vector().await.get(&target);
        if applied == 0 {
            return;
        }
        let hub = &mut self.hub;
        let result = hub
            .engine
            .seal_revoke(
                &mut hub.conn,
                &hub.ctx,
                target,
                applied,
                RevokeReason::Revoked,
                &mut hub.identity.local(),
                hub.now_ms,
            )
            .await;
        match result {
            Ok(_) | Err(SyncError::InvalidRevoke(_)) => {}
            Err(error) => panic!("hub failed to revoke: {error:?}"),
        }
    }

    async fn step(&mut self) {
        self.advance_clocks();
        let index = self.rng.below(self.replicas.len());
        match self.rng.below(100) {
            0..=39 => self.edit(index).await,
            40..=64 => {
                if !self.offline[index] {
                    self.sync(index).await;
                }
            }
            65..=74 => seal(&mut self.rng, &mut self.replicas[index]).await,
            75..=81 => self.resolve(index).await,
            82..=88 => self.recover(index).await,
            89..=96 => self.offline[index] = !self.offline[index],
            _ => self.revoke(index).await,
        }
        for replica in &mut self.replicas {
            replica.check_invariants().await;
        }
        self.hub.check_invariants().await;
    }

    /// Reconnects everyone and exchanges over a reliable network until nothing changes.
    async fn quiesce(&mut self) {
        self.offline.fill(false);
        for _ in 0..32 {
            let mut changed = false;
            for index in 0..self.replicas.len() {
                seal(&mut self.rng, &mut self.replicas[index]).await;
                changed |= send(
                    &mut self.rng,
                    &mut self.replicas[index],
                    &mut self.hub,
                    true,
                )
                .await;
            }
            for index in 0..self.replicas.len() {
                changed |= send(
                    &mut self.rng,
                    &mut self.hub,
                    &mut self.replicas[index],
                    true,
                )
                .await;
            }
            let expected = self.hub.stored_vector().await;
            let mut same = true;
            for replica in &mut self.replicas {
                let status = replica
                    .engine
                    .status(&mut replica.conn, &replica.ctx)
                    .await
                    .unwrap();
                same &= replica.stored_vector().await == expected && status.pending_captures == 0;
            }
            if !changed && same {
                return;
            }
        }
        let hub = self
            .hub
            .engine
            .status(&mut self.hub.conn, &self.hub.ctx)
            .await
            .unwrap();
        let mut states = vec![format!("hub {hub:?}")];
        for replica in &mut self.replicas {
            let status = replica
                .engine
                .status(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            let captures = ids(
                replica,
                "SELECT table_id || ' ' || row_key FROM sync_capture",
            )
            .await;
            states.push(format!(
                "replica {} {status:?} captures {captures:?}",
                replica.index
            ));
        }
        panic!("replicas did not quiesce:\n{}", states.join("\n"));
    }

    async fn assert_converged(&mut self) {
        let expected = self.hub.dump().await;
        for replica in &mut self.replicas {
            assert_eq!(
                replica.dump().await,
                expected,
                "replica {} diverged",
                replica.index
            );
        }
        for replica in self
            .replicas
            .iter_mut()
            .chain(std::iter::once(&mut self.hub))
        {
            let status = replica
                .engine
                .status(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(
                status.waiting, 0,
                "replica {} has waiting operations",
                replica.index
            );
            assert_eq!(
                status.held.total(),
                0,
                "replica {} holds operations",
                replica.index
            );
            assert_eq!(
                status.pending_captures, 0,
                "replica {} has captures",
                replica.index
            );
            let integrity = replica.scalar_text("PRAGMA integrity_check").await;
            assert_eq!(
                integrity.as_deref(),
                Some("ok"),
                "replica {} is corrupt",
                replica.index
            );
        }
    }
}

async fn run(seed: u64) {
    let _guard = SeedGuard(seed);
    let mut sim = Sim::new(seed).await;
    for _ in 0..STEPS {
        sim.step().await;
    }
    sim.quiesce().await;
    sim.assert_converged().await;
}

fn number(variable: &str) -> Option<u64> {
    let value = std::env::var(variable).ok()?;
    Some(
        value
            .parse()
            .unwrap_or_else(|_| panic!("{variable} must be a non-negative integer")),
    )
}

fn seeds() -> std::ops::Range<u64> {
    if let Some(seed) = number(SEED_VARIABLE) {
        return seed..seed + 1;
    }
    0..number(SEEDS_VARIABLE).unwrap_or(DEFAULT_SEEDS)
}

#[test]
fn seeded_runs_converge_after_partitions_crashes_and_revocations() {
    block_on(async {
        for seed in seeds() {
            run(seed).await;
        }
    });
}
