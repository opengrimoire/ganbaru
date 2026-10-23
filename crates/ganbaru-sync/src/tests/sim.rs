//! Seeded simulation of installations that edit Quick notes and the test-only fixture domain,
//! partly offline, and exchange operations through a hub over a network that drops, duplicates,
//! and reorders messages.
//! Engine calls crash at failpoints and must leave no trace, the hub removes spare devices while
//! they edit and excludes what they write past the cutoff, and conflicts and recovery offers are
//! resolved along the way. Exchanges sometimes end with the hub and the exchanging replica compacting what is stable, except one replica that never
//! compacts; compaction never changes domain rows. After every step the databases keep their
//! invariants; at quiescence every installation holds the same replicated state.
//!
//! Seeds are split across four test functions that the harness runs in parallel.
//! `GANBARU_SYNC_SOAK_SEEDS` sets the number of seeds for a soak run; the default debug
//! test run takes about one minute. A failing run prints its seed, and `GANBARU_SYNC_SEED` replays
//! that seed alone in the shard that owns it.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{Envelope, Field, VersionVector, WriterId};

use super::block_on;
use super::fixture::{
    BOARD_CARDS, BOARD_CHECKS, BOARD_HANDLE_PREFIX, MAX_BOARD_STAGE, MAX_LIST_ENTRIES, MEMOS_TABLE,
    NODE_MAX_DEPTH, edge_key, membership_key, memo_tag_key,
};
use super::replica::{ENROLLED_DEVICES, Replica, START_MS, device_id, quoted, rank};
use crate::acknowledge::{ACKNOWLEDGEMENT_INTERVAL_MS, KEEPALIVE_INTERVAL_MS};
use crate::error::failpoints;
use crate::hold::{self, MAX_HOLD_MS};
use crate::manifest::vault::quick_notes::{NOTES_TABLE, TAGS_TABLE, note_group};
use crate::stability::{CountedDevice, DeviceStanding};
use crate::{
    ApplyReport, HoldReason, StoreOutcome, StoreRefusal, SyncError, SyncResult, compaction_limit,
};

const DEFAULT_SEEDS: u64 = 48;
const SEEDS_VARIABLE: &str = "GANBARU_SYNC_SOAK_SEEDS";
const SEED_VARIABLE: &str = "GANBARU_SYNC_SEED";
/// Test functions the seeds are split across, so the test harness runs them in parallel.
const SHARDS: u64 = 4;
const STEPS: usize = 60;
const TAG_NAMES: &[&str] = &["Work", "work", "Home", "Ideas", "IDEAS", "Errands"];
const WORDS: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "plan", "call", "buy", "read",
];
const ONE_DAY_MS: u64 = 86_400_000;
/// Local transactions in one burst of node or edge edits.
const TREE_BURST: usize = 3;
/// Items and nodes every replica holds before the first step.
const SHARED_ROWS: usize = 5;
/// Id prefix of the shared items, created by replica 0 as its own.
const SHARED_ITEM: &str = "item-0-shared-";

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
    /// Devices enrolled beyond the replicas and the hub that a removal used up.
    spares: usize,
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

const SEAL_FAILPOINTS: &[&str] = &[
    "seal.reserve",
    "seal.before_commit",
    "hold.before_seal",
    "elide.before_commit",
];
const APPLY_FAILPOINTS: &[&str] = &[
    "apply.before_commit",
    "seal.reserve",
    "seal.before_commit",
    "hold.before_seal",
    "elide.before_commit",
];
const SEQUENCE_FAILPOINTS: &[&str] = &["sequence.before_commit", "seal.reserve"];
const RECOVERY_FAILPOINTS: &[&str] = &[
    "recovery.before_commit",
    "seal.reserve",
    "elide.before_commit",
];

/// Seals pending captures, rotating the writer once when it is no longer active.
async fn seal(rng: &mut Rng, replica: &mut Replica) {
    let before = replica.seal_snapshot().await;
    seal_rotating(rng, replica).await;
    replica.check_sealed_writes(&before).await;
}

async fn seal_rotating(rng: &mut Rng, replica: &mut Replica) {
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

/// Applies until nothing ready remains, rotating like the app while the device stays enrolled.
async fn apply_all(rng: &mut Rng, replica: &mut Replica) {
    for _ in 0..1_000 {
        let report: ApplyReport =
            crashing(rng, replica, APPLY_FAILPOINTS, async |r: &mut Replica| {
                r.apply_once().await
            })
            .await
            .unwrap_or_else(|error| panic!("replica {} failed to apply: {error:?}", replica.index));
        if report.local_writer_frozen && !replica.device_removed().await {
            if !replica.reseal_own_forks().await {
                replica.rotate();
            }
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
    for (op, outcome) in ops.iter().zip(outcomes) {
        match outcome {
            StoreOutcome::Stored { held } => {
                // Only an operation of a removed device past its cutoff is held.
                if held.is_some() {
                    assert_eq!(held, Some(HoldReason::Invalid));
                    let operation = Envelope::decode(op).unwrap().operation().unwrap();
                    let excluded = to
                        .count(&format!(
                            "SELECT count(*) FROM sync_ops WHERE writer_id = x'{}' AND seq = {}
                                AND excluded_revision IS NOT NULL",
                            operation.writer.to_hex(),
                            operation.seq
                        ))
                        .await;
                    assert_eq!(excluded, 1, "replica {} held an operation", to.index);
                }
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
    acknowledge(rng, to).await;
    stored
}

const ACKNOWLEDGE_FAILPOINTS: &[&str] = &["acknowledge.before_commit", "seal.reserve"];

/// Seals the writer's genesis and any due acknowledgement, rotating the writer once like the
/// app. Most calls seal nothing, so only some of them may crash: the snapshots a crash
/// compares would otherwise double the run time.
async fn acknowledge(rng: &mut Rng, replica: &mut Replica) {
    let failpoints = if rng.chance(10) {
        ACKNOWLEDGE_FAILPOINTS
    } else {
        &[]
    };
    let mut call = async |r: &mut Replica| {
        r.engine
            .acknowledge(&mut r.conn, &r.ctx, &mut r.identity.local(), r.now_ms)
            .await
    };
    match crashing(rng, replica, failpoints, &mut call).await {
        Ok(_) => {}
        Err(SyncError::WriterInactive(_)) => {
            replica.rotate();
            crashing(rng, replica, failpoints, call)
                .await
                .unwrap_or_else(|error| {
                    panic!("replica {} failed to acknowledge: {error:?}", replica.index)
                });
        }
        Err(error) => panic!("replica {} failed to acknowledge: {error:?}", replica.index),
    }
}

/// Percent of exchange steps that end with compaction.
const COMPACTION_PERCENT: u64 = 40;

/// Index of the replica that never compacts, which every other replica must still equal.
const NEVER_COMPACTS: usize = 0;

/// Compacts `replica` up to `limit` in batches of `batch` operations, crashing between batches
/// now and then.
async fn compact(rng: &mut Rng, replica: &mut Replica, limit: &VersionVector, batch: usize) {
    crashing(rng, replica, &["compact_batch"], async |r: &mut Replica| {
        r.compact(limit, batch).await
    })
    .await
    .unwrap_or_else(|error| panic!("replica {} failed to compact: {error:?}", replica.index));
}

/// The head of `writer` the hub stores with the same hash as `replica`, as the probe of a
/// pass confirms it: the lower of both stored sequences, when the hashes there agree.
async fn confirmed_head(hub: &mut Replica, replica: &mut Replica, writer: WriterId) -> Option<u64> {
    let seq = hub
        .stored_vector()
        .await
        .get(&writer)
        .min(replica.stored_vector().await.get(&writer));
    if seq == 0 {
        return None;
    }
    let sql = format!(
        "SELECT lower(hex(hash)) FROM sync_ops WHERE writer_id = x'{}' AND seq = {seq}",
        writer.to_hex()
    );
    (ids(hub, &sql).await == ids(replica, &sql).await).then_some(seq)
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
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

fn iso_time(ms: u64) -> String {
    format!("strftime('%Y-%m-%dT%H:%M:%fZ', {ms} / 1000.0, 'unixepoch')")
}

/// The ancestors of `node`, nearest first, in a parent map whose roots map to an empty id.
fn ancestors<'a>(parents: &'a BTreeMap<String, String>, node: &str) -> Vec<&'a str> {
    let mut chain = Vec::new();
    let mut at = parents.get(node);
    while let Some(parent) = at.filter(|parent| !parent.is_empty()) {
        // A local tree has no cycle; the bound only stops a broken one.
        if chain.len() > parents.len() {
            break;
        }
        chain.push(parent.as_str());
        at = parents.get(parent);
    }
    chain
}

/// Depth of `node`: a root has depth 0.
fn depth(parents: &BTreeMap<String, String>, node: &str) -> usize {
    ancestors(parents, node).len()
}

/// Whether `node` is `ancestor` or lies in its subtree.
fn is_within(parents: &BTreeMap<String, String>, node: &str, ancestor: &str) -> bool {
    node == ancestor || ancestors(parents, node).contains(&ancestor)
}

/// How many levels the subtree of `node` reaches below it.
fn height(parents: &BTreeMap<String, String>, node: &str) -> usize {
    let base = depth(parents, node);
    parents
        .keys()
        .filter(|other| is_within(parents, other, node))
        .map(|other| depth(parents, other) - base)
        .max()
        .unwrap_or(0)
}

/// Whether `start` reaches `goal` through `edges`.
fn reaches(edges: &[(String, String)], start: &str, goal: &str) -> bool {
    let mut seen = BTreeSet::from([start]);
    let mut pending = vec![start];
    while let Some(row) = pending.pop() {
        if row == goal {
            return true;
        }
        for (_, to) in edges.iter().filter(|(from, _)| from == row) {
            if seen.insert(to.as_str()) {
                pending.push(to.as_str());
            }
        }
    }
    false
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
        let hub = Replica::anchor(count, seed).await;
        Self {
            rng,
            replicas,
            hub,
            offline: vec![false; count],
            skew_ms,
            clock_ms: START_MS,
            created: 0,
            spares: 0,
        }
    }

    /// Advances every clock by up to two seconds, and now and then by long enough for
    /// acknowledgements or keepalives to fall due again.
    fn advance_clocks(&mut self) {
        let elapsed = if self.rng.chance(3) {
            ACKNOWLEDGEMENT_INTERVAL_MS + self.rng.next() % KEEPALIVE_INTERVAL_MS
        } else {
            1 + self.rng.next() % 2_000
        };
        self.advance_clocks_by(elapsed);
    }

    fn advance_clocks_by(&mut self, elapsed_ms: u64) {
        self.clock_ms += elapsed_ms;
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

    /// One local edit of the kind the app makes, in one transaction, or a burst of node or edge
    /// edits. Edits that break a domain constraint roll back, as a refused app command would.
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
        let kind = self.rng.below(45);
        if kind >= 43 {
            return self.edit_lists(index, now).await;
        }
        if kind >= 41 {
            return self.edit_boards(index, now).await;
        }
        if kind >= 39 {
            return self.edit_memos(index).await;
        }
        if kind >= 37 {
            return self.edit_drafts(index).await;
        }
        if kind >= 25 {
            // Tree and edge edits come in bursts, so concurrent moves and adds meet often.
            for _ in 0..TREE_BURST {
                if kind >= 31 {
                    self.edit_edges(index).await;
                } else {
                    self.edit_nodes(index).await;
                }
            }
            return;
        }
        if kind >= 21 {
            return self.edit_memberships(index, now).await;
        }
        if kind >= 18 {
            return self.edit_links(index).await;
        }
        if kind >= 15 {
            return self.edit_fixture(index, kind - 15, now).await;
        }
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

    /// One local edit of a fixture item: create, relabel, or delete. The fixture domain's
    /// triggers mark local creates, count label edits, and keep the index in step.
    async fn edit_fixture(&mut self, index: usize, kind: usize, now: String) {
        let items = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_items ORDER BY id",
        )
        .await;
        let item = self.rng.pick(&items).map(|id| quoted(id));
        let sql = match (kind, item) {
            (1, Some(item)) => format!(
                "UPDATE fixture_items SET label = {} WHERE id = {item}",
                quoted(&self.words())
            ),
            (2, Some(item)) => format!("DELETE FROM fixture_items WHERE id = {item}"),
            _ => {
                let id = self.next_id("item", index);
                format!(
                    "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ({}, {now}, {}, 0)",
                    quoted(&id),
                    quoted(&self.words())
                )
            }
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// One local edit of fixture drafts and their parts: create a draft that may be published
    /// at once, publish, retitle, add or edit a part, delete a draft, or try to turn a published
    /// draft back into a draft, which the publication guard refuses.
    async fn edit_drafts(&mut self, index: usize) {
        let drafts = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_drafts ORDER BY id",
        )
        .await;
        let parts = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_draft_parts ORDER BY id",
        )
        .await;
        let draft = self.rng.pick(&drafts).map(|id| quoted(id));
        let part = self.rng.pick(&parts).map(|id| quoted(id));
        let sql = match (self.rng.below(7), draft, part) {
            (1, Some(draft), _) => {
                format!("UPDATE fixture_drafts SET published = 1 WHERE id = {draft}")
            }
            (2, Some(draft), _) => format!(
                "UPDATE fixture_drafts SET title = {} WHERE id = {draft}",
                quoted(&self.words())
            ),
            (3, Some(draft), _) => {
                let id = self.next_id("part", index);
                format!(
                    "INSERT INTO fixture_draft_parts (id, draft_id, body) VALUES ({}, {draft}, {})",
                    quoted(&id),
                    quoted(&self.words())
                )
            }
            (4, _, Some(part)) => format!(
                "UPDATE fixture_draft_parts SET body = {} WHERE id = {part}",
                quoted(&self.words())
            ),
            (5, Some(draft), _) => format!("DELETE FROM fixture_drafts WHERE id = {draft}"),
            (6, Some(draft), _) => {
                let sql = format!("UPDATE fixture_drafts SET published = 0 WHERE id = {draft}");
                let replica = &mut self.replicas[index];
                let published = replica
                    .count(&format!(
                        "SELECT count(*) FROM fixture_drafts WHERE id = {draft} AND published = 1"
                    ))
                    .await;
                let reverted = replica.try_exec(&sql).await;
                assert!(
                    published == 0 || !reverted,
                    "replica {index} turned published draft {draft} back into a draft"
                );
                return;
            }
            _ => {
                let id = self.next_id("draft", index);
                let mut sql = format!(
                    "INSERT INTO fixture_drafts (id, title, published) VALUES ({}, {}, {});",
                    quoted(&id),
                    quoted(&self.words()),
                    u8::from(self.rng.chance(30))
                );
                for _ in 0..self.rng.below(3) {
                    let part = self.next_id("part", index);
                    sql.push_str(&format!(
                        "INSERT INTO fixture_draft_parts (id, draft_id, body) VALUES ({}, {}, {});",
                        quoted(&part),
                        quoted(&id),
                        quoted(&self.words())
                    ));
                }
                sql
            }
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// One local edit of fixture memos: create, relabel, add a line or a tag, delete with a
    /// hold the way domain code does, or undo a held delete, which inserts the memo again only
    /// while [`hold::release_hold`] confirms the lease. The lines and tags the delete removed
    /// stay held until the lease expires.
    async fn edit_memos(&mut self, index: usize) {
        let memos = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_memos ORDER BY id",
        )
        .await;
        let held = ids(
            &mut self.replicas[index],
            &format!(
                "SELECT row_key FROM sync_capture
                 WHERE table_id = {} AND deleted = 1 AND held_until_ms IS NOT NULL
                 ORDER BY row_key",
                MEMOS_TABLE.0
            ),
        )
        .await;
        let memo = self.rng.pick(&memos).cloned();
        let label = self.words();
        match (self.rng.below(7), memo) {
            (4, Some(memo)) => {
                let id = self.next_id("line", index);
                let sql = format!(
                    "INSERT INTO fixture_memo_lines (id, memo_id, body) VALUES ({}, {}, {})",
                    quoted(&id),
                    quoted(&memo),
                    quoted(&label)
                );
                self.replicas[index].try_exec(&sql).await;
            }
            (5, Some(memo)) => {
                let tag = self.rng.pick(&["red", "green", "blue"]).copied();
                let Some(tag) = tag else {
                    return;
                };
                let sql = format!(
                    "INSERT INTO fixture_memo_tags (memo_id, tag, sync_key) VALUES ({}, {}, {})",
                    quoted(&memo),
                    quoted(tag),
                    quoted(&memo_tag_key(&memo, tag))
                );
                self.replicas[index].try_exec(&sql).await;
            }
            (1, Some(memo)) => {
                let sql = format!(
                    "UPDATE fixture_memos SET label = {} WHERE id = {}",
                    quoted(&label),
                    quoted(&memo)
                );
                self.replicas[index].try_exec(&sql).await;
            }
            (2, Some(memo)) => {
                let lease = 1 + self.rng.next() % (2 * MAX_HOLD_MS);
                let replica = &mut self.replicas[index];
                let now = replica.now_ms;
                let mut tx = sqlx::Connection::begin(&mut replica.conn).await.unwrap();
                sqlx::query("DELETE FROM fixture_memos WHERE id = ?")
                    .bind(&memo)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                hold::hold_deletes(
                    &replica.engine,
                    &mut tx,
                    MEMOS_TABLE,
                    &[&memo],
                    now,
                    now + lease,
                )
                .await
                .unwrap();
                tx.commit().await.unwrap();
            }
            (3, _) => {
                let Some(memo) = self.rng.pick(&held).cloned() else {
                    return;
                };
                let replica = &mut self.replicas[index];
                let now = replica.now_ms;
                let mut tx = sqlx::Connection::begin(&mut replica.conn).await.unwrap();
                let live = hold::release_hold(&mut tx, MEMOS_TABLE, &[&memo], now)
                    .await
                    .unwrap();
                if live {
                    sqlx::query("INSERT INTO fixture_memos (id, label) VALUES (?, ?)")
                        .bind(&memo)
                        .bind(&label)
                        .execute(&mut *tx)
                        .await
                        .unwrap();
                }
                tx.commit().await.unwrap();
            }
            _ => {
                let id = self.next_id("memo", index);
                let sql = format!(
                    "INSERT INTO fixture_memos (id, label) VALUES ({}, {})",
                    quoted(&id),
                    quoted(&label)
                );
                self.replicas[index].try_exec(&sql).await;
            }
        }
    }

    /// SQL that replaces a list's entries with new ones, each with a fresh id and usually a
    /// membership the replica holds.
    fn entries_sql(&mut self, index: usize, list: &str, members: &[String]) -> String {
        let mut sql = format!(
            "DELETE FROM fixture_list_entries WHERE list_id = {};",
            quoted(list)
        );
        for ordinal in 0..self.rng.below(MAX_LIST_ENTRIES + 1) {
            let id = self.next_id("entry", index);
            let note = if self.rng.chance(50) {
                quoted(&self.words())
            } else {
                "NULL".into()
            };
            let member = match self.rng.pick(members) {
                Some(key) if self.rng.chance(60) => quoted(key),
                _ => "NULL".into(),
            };
            sql.push_str(&format!(
                "INSERT INTO fixture_list_entries (list_id, ordinal, entry_id, body, note, member_key)
                 VALUES ({}, {ordinal}, {}, {}, {note}, {member});",
                quoted(list),
                quoted(&id),
                quoted(&self.words())
            ));
        }
        sql
    }

    /// One local edit of fixture lists: create a list with entries, retitle it, reverse its
    /// entries keeping their ids, replace its entries, or delete it. Removing a membership that
    /// an entry names removes the entry here and hides it on replicas that still hold the
    /// list's older value.
    async fn edit_lists(&mut self, index: usize, now: String) {
        let lists = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_lists ORDER BY id",
        )
        .await;
        let members = ids(
            &mut self.replicas[index],
            "SELECT sync_key FROM fixture_memberships ORDER BY sync_key",
        )
        .await;
        let list = self.rng.pick(&lists).cloned();
        let sql = match (self.rng.below(5), list) {
            (1, Some(list)) => format!(
                "UPDATE fixture_lists SET title = {} WHERE id = {}",
                quoted(&self.words()),
                quoted(&list)
            ),
            (2, Some(list)) => {
                let list = quoted(&list);
                format!(
                    "UPDATE fixture_list_entries SET ordinal = -1 - ordinal WHERE list_id = {list};
                     UPDATE fixture_list_entries SET ordinal = ordinal +
                         (SELECT count(*) FROM fixture_list_entries WHERE list_id = {list})
                     WHERE list_id = {list};"
                )
            }
            (3, Some(list)) => self.entries_sql(index, &list, &members),
            (4, Some(list)) => format!("DELETE FROM fixture_lists WHERE id = {}", quoted(&list)),
            _ => {
                let id = self.next_id("list", index);
                let entries = self.entries_sql(index, &id, &members);
                format!(
                    "INSERT INTO fixture_lists (id, created_at, title) VALUES ({}, {now}, {});
                     {entries}",
                    quoted(&id),
                    quoted(&self.words())
                )
            }
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// SQL that replaces a board's cards and their checks with new ones, each with a fresh id;
    /// a card may name an item the replica holds.
    fn cards_sql(&mut self, index: usize, board: &str, items: &[String]) -> String {
        let mut sql = format!(
            "DELETE FROM fixture_board_cards WHERE board_id = {};",
            quoted(board)
        );
        for ordinal in 0..self.rng.below(BOARD_CARDS.max_rows + 1) {
            let id = self.next_id("card", index);
            let item = match self.rng.pick(items) {
                Some(item) if self.rng.chance(50) => quoted(item),
                _ => "NULL".into(),
            };
            sql.push_str(&format!(
                "INSERT INTO fixture_board_cards (board_id, ordinal, card_id, title, item_id)
                 VALUES ({}, {ordinal}, {}, {}, {item});",
                quoted(board),
                quoted(&id),
                quoted(&self.words())
            ));
            for check in 0..self.rng.below(BOARD_CHECKS.max_rows + 1) {
                let id = self.next_id("check", index);
                sql.push_str(&format!(
                    "INSERT INTO fixture_board_checks (board_id, card_ordinal, ordinal, check_id, done)
                     VALUES ({}, {ordinal}, {check}, {}, {});",
                    quoted(board),
                    quoted(&id),
                    u8::from(self.rng.chance(50))
                ));
            }
        }
        sql
    }

    /// One local edit of fixture boards, whose owner, stage, and cards merge as one coupled
    /// value with the later stage winning: create a board with cards, advance its stage, change
    /// its owner, replace its cards, toggle a check, or delete it.
    async fn edit_boards(&mut self, index: usize, now: String) {
        let boards = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_boards ORDER BY id",
        )
        .await;
        let items = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_items ORDER BY id",
        )
        .await;
        let checks = ids(
            &mut self.replicas[index],
            "SELECT check_id FROM fixture_board_checks ORDER BY check_id",
        )
        .await;
        let board = self.rng.pick(&boards).cloned();
        let sql = match (self.rng.below(6), board) {
            (1, Some(board)) => format!(
                "UPDATE fixture_boards SET stage = min(stage + 1, {MAX_BOARD_STAGE})
                 WHERE id = {}",
                quoted(&board)
            ),
            (2, Some(board)) => format!(
                "UPDATE fixture_boards SET owner_id = {} WHERE id = {}",
                self.rng
                    .pick(&items)
                    .map_or_else(|| "NULL".into(), |item| quoted(item)),
                quoted(&board)
            ),
            (3, Some(board)) => self.cards_sql(index, &board, &items),
            (4, Some(_)) => {
                let Some(check) = self.rng.pick(&checks) else {
                    return;
                };
                format!(
                    "UPDATE fixture_board_checks SET done = 1 - done WHERE check_id = {}",
                    quoted(check)
                )
            }
            (5, Some(board)) => format!("DELETE FROM fixture_boards WHERE id = {}", quoted(&board)),
            _ => {
                let id = self.next_id("board", index);
                let cards = self.cards_sql(index, &id, &items);
                format!(
                    "INSERT INTO fixture_boards (id, created_at, owner_id, stage, handle)
                     VALUES ({}, {now}, NULL, 0, {});
                     {cards}",
                    quoted(&id),
                    quoted(&format!("{BOARD_HANDLE_PREFIX}{id}"))
                )
            }
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// One local edit of fixture links and their ends: create an end whose label may collide,
    /// link ends and a parent link, or delete an end or a link. Deleting clears references to
    /// it, and colliding ends redirect every reference column of a link.
    async fn edit_links(&mut self, index: usize) {
        let ends = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_cycle_a ORDER BY id",
        )
        .await;
        let links = ids(
            &mut self.replicas[index],
            "SELECT id FROM fixture_links ORDER BY id",
        )
        .await;
        let end = self.rng.pick(&ends).map(|id| quoted(id));
        let link = self.rng.pick(&links).map(|id| quoted(id));
        let sql = match (self.rng.below(5), end, link) {
            (0, _, _) => {
                let id = self.next_id("end", index);
                format!(
                    "INSERT INTO fixture_cycle_a (id, label) VALUES ({}, {})",
                    quoted(&id),
                    quoted(self.rng.pick(TAG_NAMES).unwrap())
                )
            }
            (1, end, parent) => {
                let id = self.next_id("link", index);
                format!(
                    "INSERT INTO fixture_links (id, note, left_id, parent_id)
                     VALUES ({}, {}, {}, {})",
                    quoted(&id),
                    quoted(&self.words()),
                    end.unwrap_or("NULL".into()),
                    parent.unwrap_or("NULL".into())
                )
            }
            (2, end, Some(link)) => format!(
                "UPDATE fixture_links SET left_id = {} WHERE id = {link}",
                end.unwrap_or("NULL".into())
            ),
            (3, Some(end), _) => format!("DELETE FROM fixture_cycle_a WHERE id = {end}"),
            (4, _, Some(link)) => format!("DELETE FROM fixture_links WHERE id = {link}"),
            _ => return,
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// One local edit of a derived-key membership of an item in an end: add it, remove it,
    /// change its role, or remove and add it again. Every device derives the same key for the
    /// same pair, so devices add and remove one shared row. Edits usually target a membership
    /// the replica holds; an add may create its item or its end, whose label may collide.
    async fn edit_memberships(&mut self, index: usize, now: String) {
        let held = ids(
            &mut self.replicas[index],
            "SELECT item_id || ' ' || end_id FROM fixture_memberships ORDER BY sync_key",
        )
        .await;
        let mut parents = String::new();
        let (item, end) = match self.rng.pick(&held) {
            Some(pair) if self.rng.chance(60) => {
                let (item, end) = pair.split_once(' ').expect("membership pairs hold a space");
                (item.to_string(), end.to_string())
            }
            _ => {
                let item = self.member_parent(index, "item", &mut parents).await;
                let end = self.member_parent(index, "end", &mut parents).await;
                (item, end)
            }
        };
        let key = quoted(&membership_key(&item, &end));
        let add = format!(
            "{parents}INSERT INTO fixture_memberships (item_id, end_id, sync_key, created_at, role)
             VALUES ({}, {}, {key}, {now}, {});",
            quoted(&item),
            quoted(&end),
            quoted(self.rng.pick(WORDS).unwrap())
        );
        let remove = format!("DELETE FROM fixture_memberships WHERE sync_key = {key};");
        let sql = match self.rng.below(4) {
            0 => add,
            1 => remove,
            2 => format!(
                "UPDATE fixture_memberships SET role = {} WHERE sync_key = {key}",
                quoted(self.rng.pick(WORDS).unwrap())
            ),
            _ => format!("{remove} {add}"),
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// One local edit of the node tree: create a node, move it, relabel it, or delete it with
    /// its subtree. Like the app, a replica refuses a move into the node's own subtree or below
    /// the maximum depth, so cycles and overly deep rows arise only from concurrent moves.
    async fn edit_nodes(&mut self, index: usize) {
        let pairs = ids(
            &mut self.replicas[index],
            "SELECT id || ' ' || coalesce(parent_id, '') FROM fixture_nodes ORDER BY id",
        )
        .await;
        let parents: BTreeMap<String, String> = pairs
            .iter()
            .filter_map(|pair| pair.split_once(' '))
            .map(|(id, parent)| (id.to_string(), parent.to_string()))
            .collect();
        let nodes: Vec<String> = parents.keys().cloned().collect();
        let node = self.rng.pick(&nodes).cloned();
        let target = self
            .rng
            .pick(&nodes)
            .cloned()
            .filter(|_| self.rng.chance(70));
        let position = quoted(&rank(self.rng.below(8) as u32));
        // Moves dominate once a few nodes exist, so concurrent moves meet often.
        let kind = if nodes.len() < 3 {
            0
        } else {
            self.rng.below(6)
        };
        let sql = match (kind, node) {
            (1..=3, Some(node)) => {
                let fits = target.as_deref().is_none_or(|target| {
                    !is_within(&parents, target, &node)
                        && depth(&parents, target) + 1 + height(&parents, &node)
                            <= usize::from(NODE_MAX_DEPTH)
                });
                if !fits {
                    return;
                }
                format!(
                    "UPDATE fixture_nodes SET parent_id = {}, position = {position}
                     WHERE id = {}",
                    target.as_deref().map_or("NULL".to_string(), quoted),
                    quoted(&node)
                )
            }
            (4, Some(node)) => format!(
                "UPDATE fixture_nodes SET label = {} WHERE id = {}",
                quoted(&self.words()),
                quoted(&node)
            ),
            (5, Some(node)) => format!("DELETE FROM fixture_nodes WHERE id = {}", quoted(&node)),
            _ => {
                if target
                    .as_deref()
                    .is_some_and(|target| depth(&parents, target) >= usize::from(NODE_MAX_DEPTH))
                {
                    return;
                }
                let id = self.next_id("node", index);
                format!(
                    "INSERT INTO fixture_nodes (id, parent_id, position, label)
                     VALUES ({}, {}, {position}, {})",
                    quoted(&id),
                    target.as_deref().map_or("NULL".to_string(), quoted),
                    quoted(&self.words())
                )
            }
        };
        self.replicas[index].try_exec(&sql).await;
    }

    /// One local edit of the acyclic edge set between items: add an edge, possibly with a new
    /// item, or remove one. Like the app, a replica refuses an edge that closes a cycle in its
    /// own state, so cycles arise only from concurrent adds.
    async fn edit_edges(&mut self, index: usize) {
        let held = ids(
            &mut self.replicas[index],
            "SELECT from_id || ' ' || to_id FROM fixture_edges ORDER BY sync_key",
        )
        .await;
        let edges: Vec<(String, String)> = held
            .iter()
            .filter_map(|pair| pair.split_once(' '))
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect();
        if let Some((from, to)) = self.rng.pick(&edges).cloned()
            && self.rng.chance(40)
        {
            let sql = format!(
                "DELETE FROM fixture_edges WHERE sync_key = {}",
                quoted(&edge_key(&from, &to))
            );
            self.replicas[index].try_exec(&sql).await;
            return;
        }
        // Joining the shared items makes concurrent adds close cycles often.
        let ends = ids(
            &mut self.replicas[index],
            &format!("SELECT id FROM fixture_items WHERE id LIKE '{SHARED_ITEM}%' ORDER BY id"),
        )
        .await;
        let mut parents = String::new();
        let (from, to) = match (self.rng.pick(&ends).cloned(), self.rng.pick(&ends).cloned()) {
            (Some(from), Some(to)) if self.rng.chance(60) => (from, to),
            _ => {
                let from = self.member_parent(index, "item", &mut parents).await;
                let to = self.member_parent(index, "item", &mut parents).await;
                (from, to)
            }
        };
        if from == to || reaches(&edges, &to, &from) {
            return;
        }
        let sql = format!(
            "{parents}INSERT INTO fixture_edges (from_id, to_id, sync_key) VALUES ({}, {}, {});",
            quoted(&from),
            quoted(&to),
            quoted(&edge_key(&from, &to))
        );
        self.replicas[index].try_exec(&sql).await;
    }

    /// Items and a flat tree of nodes every replica holds before the first step, so concurrent
    /// moves and edge adds meet on the same rows.
    async fn share_rows(&mut self) {
        let now = iso_time(self.replicas[0].now_ms);
        let mut sql = String::new();
        for at in 0..SHARED_ROWS {
            sql.push_str(&format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ('{SHARED_ITEM}{at}', {now}, 'shared', 0);
                 INSERT INTO fixture_nodes (id, parent_id, position, label)
                 VALUES ('node-0-shared-{at}', NULL, {}, 'shared');",
                quoted(&rank(at as u32))
            ));
        }
        self.replicas[0].exec(&sql).await;
        self.quiesce().await;
    }

    /// An item or end id for a membership: an existing one, or a new one whose insert is
    /// appended to `parents`.
    async fn member_parent(&mut self, index: usize, kind: &str, parents: &mut String) -> String {
        let table = if kind == "item" {
            "fixture_items"
        } else {
            "fixture_cycle_a"
        };
        let existing = ids(
            &mut self.replicas[index],
            &format!("SELECT id FROM {table} ORDER BY id"),
        )
        .await;
        if let Some(id) = self.rng.pick(&existing)
            && self.rng.chance(70)
        {
            return id.clone();
        }
        let id = self.next_id(kind, index);
        let now = iso_time(self.replicas[index].now_ms);
        parents.push_str(&if kind == "item" {
            format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ({}, {now}, {}, 0);",
                quoted(&id),
                quoted(&self.words())
            )
        } else {
            format!(
                "INSERT INTO fixture_cycle_a (id, label) VALUES ({}, {});",
                quoted(&id),
                quoted(self.rng.pick(TAG_NAMES).unwrap())
            )
        });
        id
    }

    /// Fixture marks exist only on the replica that created the marked item, because the gated
    /// trigger that writes them stays silent while operations apply.
    async fn assert_marks_stay_local(&mut self) {
        for replica in self
            .replicas
            .iter_mut()
            .chain(std::iter::once(&mut self.hub))
        {
            let prefix = format!("item-{}-", replica.index);
            for item in ids(replica, "SELECT item_id FROM fixture_item_marks").await {
                assert!(
                    item.starts_with(&prefix),
                    "replica {} marked {item} while applying",
                    replica.index
                );
            }
        }
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
            .conflict_details(
                &mut replica.conn,
                replica.ctx.space,
                row.table,
                &row.row_key,
            )
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

    /// A spare enrolled device catches up with the hub, writes an item the hub stores, then
    /// creates another and edits the first while the hub removes it. The hub stores those late
    /// operations but excludes them, offering them as late changes. The spare leaves the run;
    /// its writes before the cutoff stay in every replica, and every replica derives the same
    /// late changes.
    async fn remove_spare(&mut self) {
        let index = self.replicas.len() + 1 + self.spares;
        if index >= ENROLLED_DEVICES {
            return;
        }
        self.spares += 1;
        let mut spare = Replica::new(index, self.rng.next()).await;
        spare.now_ms = self.hub.now_ms;
        let insert = |id: String, label: String, now: &str| {
            format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ({}, {now}, {}, 0)",
                quoted(&id),
                quoted(&label)
            )
        };
        let now = iso_time(spare.now_ms);
        let kept = self.next_id("item", index);
        let sql = insert(kept.clone(), self.words(), &now);
        assert!(spare.try_exec(&sql).await);
        seal(&mut self.rng, &mut spare).await;
        send(&mut self.rng, &mut spare, &mut self.hub, true).await;
        let kept_label = self
            .hub
            .scalar_text(&format!(
                "SELECT label FROM fixture_items WHERE id = {}",
                quoted(&kept)
            ))
            .await;

        // Edits the spare makes while the hub removes it arrive past its cutoff.
        let late = self.next_id("item", index);
        let sql = insert(late.clone(), self.words(), &now);
        assert!(spare.try_exec(&sql).await);
        let sql = format!(
            "UPDATE fixture_items SET label = {} WHERE id = {}",
            quoted(&self.words()),
            quoted(&kept)
        );
        assert!(spare.try_exec(&sql).await);
        seal(&mut self.rng, &mut spare).await;

        let device = spare.device_id.clone();
        let hub = &mut self.hub;
        let revision = crashing(
            &mut self.rng,
            hub,
            SEQUENCE_FAILPOINTS,
            async |r: &mut Replica| r.remove(&device).await,
        )
        .await
        .unwrap_or_else(|error| panic!("hub failed to remove {device}: {error:?}"));
        assert!(revision > 0);

        // The hub stores the late operations but excludes them: the created item stays hidden
        // and the kept item keeps its label.
        send(&mut self.rng, &mut spare, &mut self.hub, true).await;
        let hidden = self
            .hub
            .count(&format!(
                "SELECT count(*) FROM fixture_items WHERE id = {}",
                quoted(&late)
            ))
            .await;
        assert_eq!(
            hidden, 0,
            "a late item of removed device {device} is visible"
        );
        let label = self
            .hub
            .scalar_text(&format!(
                "SELECT label FROM fixture_items WHERE id = {}",
                quoted(&kept)
            ))
            .await;
        assert!(
            kept_label.is_none() || label == kept_label,
            "a late edit of removed device {device} applied"
        );
        assert!(
            self.hub
                .engine
                .late_changes(&mut self.hub.conn, &self.hub.ctx)
                .await
                .unwrap()
                .iter()
                .any(|change| change.row_key == late),
            "the late item of removed device {device} is not offered"
        );
    }

    async fn step(&mut self) {
        self.advance_clocks();
        let index = self.rng.below(self.replicas.len());
        let action = self.rng.below(100);
        match action {
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
            _ => self.remove_spare().await,
        }
        // Only exchanges change what the hub or a replica acknowledged or applied.
        if (40..65).contains(&action) {
            let stable = self.assert_stability().await;
            if self.rng.chance(COMPACTION_PERCENT) {
                self.compact(index, &stable).await;
            }
        }
        // A step, with its compaction, changes only the replica that acted and the hub.
        self.replicas[index].check_invariants().await;
        self.hub.check_invariants().await;
        // Hydrating every version is slow, so only a step that ran the engine checks it, and
        // only on the replica that acted. Local edits elide nothing and are read back by the
        // engine step that follows them.
        if (40..89).contains(&action) || action >= 97 {
            self.replicas[index].check_version_values().await;
        }
        self.assert_marks_stay_local().await;
    }

    /// Stability the hub computes over every replica stays within what each replica and the
    /// hub applied, and no replica counts as dormant while the run lasts. Returns the stable
    /// vector.
    async fn assert_stability(&mut self) -> VersionVector {
        let devices: Vec<CountedDevice> = (0..self.replicas.len())
            .map(|index| CountedDevice {
                device_id: device_id(index),
                linked_at_ms: Some(START_MS),
            })
            .collect();
        let hub = &mut self.hub;
        let stability = hub
            .engine
            .stability(&mut hub.conn, &hub.ctx, &devices, hub.now_ms)
            .await
            .unwrap();
        assert!(hub.applied_vector().await.dominates(&stability.stable));
        for (replica, device) in self.replicas.iter_mut().zip(&stability.devices) {
            assert_eq!(
                device.standing,
                DeviceStanding::Counted,
                "replica {} is not counted",
                replica.index
            );
            assert!(
                replica.applied_vector().await.dominates(&stability.stable),
                "replica {} lacks a stable operation",
                replica.index
            );
        }
        stability.stable
    }

    /// Compacts the hub up to `stable` and, unless it is the replica that never compacts, the
    /// replica that just exchanged, the way a pass ends. The replica records the vector the hub
    /// reported and caps its current writer at the head the hub holds with the same hash.
    async fn compact(&mut self, index: usize, stable: &VersionVector) {
        let batch = 1 + self.rng.below(16);
        let hub = &mut self.hub;
        compact(&mut self.rng, hub, stable, batch).await;
        if index == NEVER_COMPACTS || self.offline[index] {
            return;
        }
        let replica = &mut self.replicas[index];
        replica
            .engine
            .record_stable(&mut replica.conn, &replica.ctx, stable)
            .await
            .unwrap();
        let own: BTreeSet<WriterId> = ids(
            replica,
            &format!(
                "SELECT lower(hex(writer_id)) FROM sync_writers WHERE device_id = {}",
                quoted(&device_id(index))
            ),
        )
        .await
        .iter()
        .map(|hex| WriterId::from_hex(hex).unwrap())
        .collect();
        let writer = replica.writer();
        let confirmed = confirmed_head(&mut self.hub, replica, writer).await;
        let limit = compaction_limit(stable, &own, confirmed.map(|seq| (writer, seq)));
        compact(&mut self.rng, &mut self.replicas[index], &limit, batch).await;
    }

    /// Each writer's acknowledgements are at least the acknowledgement interval apart, and
    /// the hub stored some.
    async fn assert_acknowledgements_bounded(&mut self) {
        for replica in &mut self.replicas {
            let sql = format!(
                "SELECT hex(o.writer_id) || ' ' || o.stored_at_ms FROM sync_ops o
                 JOIN sync_writers w ON w.space_id = o.space_id AND w.writer_id = o.writer_id
                 WHERE w.device_id = {} AND o.kind = 3 ORDER BY o.writer_id, o.seq",
                quoted(&device_id(replica.index))
            );
            let rows = ids(replica, &sql).await;
            let mut previous: Option<(String, u64)> = None;
            for row in rows {
                let (writer, at) = row.split_once(' ').unwrap();
                let at: u64 = at.parse().unwrap();
                if let Some((last_writer, last_at)) = &previous
                    && last_writer == writer
                {
                    // One seal splits an acknowledgement over several headers when more
                    // writers advanced than one header lists, all stored at the same time.
                    assert!(
                        at == *last_at || at - last_at >= ACKNOWLEDGEMENT_INTERVAL_MS,
                        "replica {} acknowledged twice within the interval",
                        replica.index
                    );
                }
                previous = Some((writer.to_string(), at));
            }
        }
        let hub_acknowledgements = ids(
            &mut self.hub,
            "SELECT CAST(seq AS TEXT) FROM sync_ops WHERE kind = 3",
        )
        .await;
        assert!(!hub_acknowledgements.is_empty(), "nobody acknowledged");
    }

    /// Lets every deletion hold expire, reconnects everyone, and exchanges over a reliable
    /// network until nothing changes.
    async fn quiesce(&mut self) {
        self.advance_clocks_by(MAX_HOLD_MS);
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
            replica.check_version_values().await;
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
    sim.share_rows().await;
    for _ in 0..STEPS {
        sim.step().await;
    }
    sim.quiesce().await;
    sim.assert_converged().await;
    sim.assert_acknowledgements_bounded().await;
    // Every replica compacts what is stable at quiescence and still matches the replica that
    // never compacts.
    let stable = sim.assert_stability().await;
    for index in 0..sim.replicas.len() {
        sim.compact(index, &stable).await;
    }
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

/// The seeds of one shard: every seed whose remainder by `SHARDS` is `shard`, so the shards
/// together run each seed exactly once and share the seeds' uneven costs.
fn seeds(shard: u64) -> impl Iterator<Item = u64> {
    let all = match number(SEED_VARIABLE) {
        Some(seed) => seed..seed + 1,
        None => 0..number(SEEDS_VARIABLE).unwrap_or(DEFAULT_SEEDS),
    };
    all.filter(move |seed| seed % SHARDS == shard)
}

fn run_shard(shard: u64) {
    block_on(async {
        for seed in seeds(shard) {
            run(seed).await;
        }
    });
}

#[test]
fn seeded_runs_converge_after_partitions_crashes_and_revocations_shard_0() {
    run_shard(0);
}

#[test]
fn seeded_runs_converge_after_partitions_crashes_and_revocations_shard_1() {
    run_shard(1);
}

#[test]
fn seeded_runs_converge_after_partitions_crashes_and_revocations_shard_2() {
    run_shard(2);
}

#[test]
fn seeded_runs_converge_after_partitions_crashes_and_revocations_shard_3() {
    run_shard(3);
}
