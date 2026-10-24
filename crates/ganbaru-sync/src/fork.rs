//! Fork evidence and fork exclusion.
//!
//! A writer forks when two installations hold its key, which happens when a vault copy is
//! restored or cloned: both seal different operations at the same sequence. Two operations of
//! one writer at one sequence with different hashes, both signed by its key, prove it. A replica
//! that holds such a pair stores it in `sync_fork_evidence`, keeps the writer's chain through the
//! common prefix, and from then on refuses everything past it, so neither branch survives
//! anywhere: the writer becomes forked with the last shared sequence as its cutoff.
//!
//! Every replica but the forked writer's own device excludes both branches: the merge effects
//! of applied operations past the prefix are removed, the versions they superseded come back
//! from `sync_displaced_versions`, and the operations are dropped, which leaves the replica as if
//! it never stored them. Unlike a removal, a fork leaves no late changes. Each installation of
//! the writer's device keeps its own branch until it re-seals it with a new writer, see
//! [`Engine::reseal_fork`]; an installation destroyed before that loses its branch.
//!
//! The anchor and members whose anchor marks passed a branch retain nothing it displaced, so
//! excluding it can leave a group without a version on them, still showing the branch value,
//! while members that retained restore the value it displaced. The anchor therefore seals again,
//! with its own writer, the current value of every live group that the branch it stored wrote,
//! once the exclusion materialized. Its operations cover every version the anchor holds, so
//! every member converges to the anchor's state whether or not a copy of the branch re-seals.
//!
//! A fork of a writer of the epoch's anchor device freezes the epoch at the revision its forked
//! operations were sealed against: entries past it are superseded, and only a handover continues
//! the log.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    ChangeAction, Content, Digest32, Envelope, GroupId, GroupMask, Seq, SpaceId, TableId, WriterId,
};
use sqlx::{Connection, SqliteConnection};

use crate::error::{SyncResult, corrupt};
use crate::log::{self, clamp_ms, seq_i64, stored_u64};
use crate::merge::{self, Batch, RowState};
use crate::{Engine, SpaceContext};
use crate::{authorization, exclusion, seal, sql};

/// Two operations of one writer at one sequence, ordered by hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkEvidence {
    /// The forked writer.
    pub writer: WriterId,
    /// First sequence where its copies differ.
    pub seq: Seq,
    /// Envelope with the lower hash.
    pub first: Vec<u8>,
    /// Envelope with the higher hash.
    pub second: Vec<u8>,
}

impl ForkEvidence {
    /// Last sequence both copies share, the forked writer's cutoff.
    pub fn keep_through(&self) -> Seq {
        self.seq - 1
    }
}

/// Result of offering fork evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceOutcome {
    /// The evidence was stored; the writer is forked from now on.
    Stored,
    /// Evidence of a fork of the writer at the same or an earlier sequence is already stored.
    Known,
    /// The evidence was not stored.
    Refused(EvidenceRefusal),
}

/// Why fork evidence was not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceRefusal {
    /// An envelope does not decode.
    Malformed,
    /// An envelope belongs to another space.
    WrongSpace,
    /// The envelopes name different writers or sequences.
    Mismatched,
    /// Both envelopes are the same operation.
    SameOperation,
    /// The writer is not stored here, so its key is unknown.
    UnknownWriter,
    /// The writer key did not sign an envelope.
    BadSignature,
}

/// A forked writer as exclusion and chain selection see it.
#[derive(Debug, Clone)]
pub(crate) struct Forked {
    /// First sequence where its copies differ.
    pub(crate) seq: Seq,
    /// Device the writer belongs to.
    pub(crate) device_id: String,
    /// Lowest authorization revision either forked operation was sealed against, when one of
    /// them decodes.
    pub(crate) revision: Option<u64>,
}

impl Forked {
    /// Last sequence both copies share.
    pub(crate) fn keep_through(&self) -> Seq {
        self.seq - 1
    }
}

/// A stored evidence row: writer, fork sequence, device, and both envelopes.
type EvidenceRow = (Vec<u8>, i64, String, Vec<u8>, Vec<u8>);

/// Every forked writer of the space.
pub(crate) async fn forked(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<BTreeMap<WriterId, Forked>> {
    let rows: Vec<EvidenceRow> = sqlx::query_as(
        "SELECT e.writer_id, e.seq, w.device_id, e.first_envelope, e.second_envelope
         FROM sync_fork_evidence e JOIN sync_writers w
            ON w.space_id = e.space_id AND w.writer_id = e.writer_id
         WHERE e.space_id = ?",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|(writer, seq, device_id, first, second)| {
            let revision = [first, second]
                .iter()
                .filter_map(|bytes| {
                    Envelope::decode(bytes)
                        .ok()?
                        .operation()
                        .ok()
                        .map(|operation| operation.header.authorization_revision)
                })
                .min();
            Ok((
                log::writer_id(&writer)?,
                Forked {
                    seq: stored_u64(seq, "fork sequence")?,
                    device_id,
                    revision,
                },
            ))
        })
        .collect()
}

/// The last sequence the copies of each forked writer share.
pub(crate) async fn shared_prefixes(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<BTreeMap<WriterId, Seq>> {
    let rows: Vec<(Vec<u8>, i64)> =
        sqlx::query_as("SELECT writer_id, seq FROM sync_fork_evidence WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_all(&mut *conn)
            .await?;
    rows.iter()
        .map(|(writer, seq)| {
            Ok((
                log::writer_id(writer)?,
                stored_u64(*seq, "fork sequence")? - 1,
            ))
        })
        .collect()
}

/// Marks every stored fork evidence unsettled, so the next engine write excludes again: what
/// this replica excludes depends on its local device.
pub(crate) async fn unsettle(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<()> {
    sqlx::query("UPDATE sync_fork_evidence SET settled = 0 WHERE space_id = ?")
        .bind(space.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Revisions at which forks freeze epochs, by anchor device: the lowest revision of each forked
/// writer of the device whose forked operations decode. A fork whose operations this version
/// cannot read freezes at revision 0, so the device's whole epoch waits for a handover.
pub(crate) async fn freezes(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<BTreeMap<String, Vec<u64>>> {
    let mut freezes: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for fork in forked(conn, space).await?.into_values() {
        freezes
            .entry(fork.device_id)
            .or_default()
            .push(fork.revision.unwrap_or(0));
    }
    for revisions in freezes.values_mut() {
        revisions.sort_unstable();
    }
    Ok(freezes)
}

/// Records evidence that `writer` forked at `seq` and makes it forked with the last shared
/// sequence as its cutoff. Evidence of a fork at an earlier sequence replaces a later one, since
/// the cutoff only falls. A revoked writer keeps its removal cutoff apart, so its cutoff becomes
/// the fork's and the result does not depend on whether the removal or the evidence came first.
/// The envelopes are stored ordered by hash, so every replica holds the same row.
pub(crate) async fn record(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    seq: Seq,
    pair: [(&[u8], Digest32); 2],
    now_ms: u64,
) -> SyncResult<EvidenceOutcome> {
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT seq FROM sync_fork_evidence WHERE space_id = ? AND writer_id = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(existing) = existing
        && stored_u64(existing, "fork sequence")? <= seq
    {
        return Ok(EvidenceOutcome::Known);
    }
    let [(first, first_hash), (second, second_hash)] = pair;
    let (first, second) = if first_hash <= second_hash {
        (first, second)
    } else {
        (second, first)
    };
    sqlx::query(
        "INSERT INTO sync_fork_evidence
            (space_id, writer_id, seq, first_envelope, second_envelope, received_at_ms)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (space_id, writer_id) DO UPDATE SET seq = excluded.seq,
            first_envelope = excluded.first_envelope, second_envelope = excluded.second_envelope,
            received_at_ms = excluded.received_at_ms, settled = 0",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .bind(first)
    .bind(second)
    .bind(clamp_ms(now_ms))
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE sync_writers SET state = 'forked',
            cutoff_seq = CASE WHEN state = 'revoked' THEN ?1 ELSE min(coalesce(cutoff_seq, ?1), ?1) END
         WHERE space_id = ?2 AND writer_id = ?3",
    )
    .bind(seq_i64(seq - 1)?)
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .execute(&mut *conn)
    .await?;
    Ok(EvidenceOutcome::Stored)
}

/// Checks offered evidence and records it.
async fn store_evidence(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    first: &[u8],
    second: &[u8],
    now_ms: u64,
) -> SyncResult<EvidenceOutcome> {
    let refused = |refusal| Ok(EvidenceOutcome::Refused(refusal));
    let (Ok(a), Ok(b)) = (Envelope::decode(first), Envelope::decode(second)) else {
        return refused(EvidenceRefusal::Malformed);
    };
    if a.space != ctx.space || b.space != ctx.space {
        return refused(EvidenceRefusal::WrongSpace);
    }
    if a.writer != b.writer || a.seq != b.seq || a.seq == 0 {
        return refused(EvidenceRefusal::Mismatched);
    }
    if a.hash() == b.hash() {
        return refused(EvidenceRefusal::SameOperation);
    }
    let Some(writer) = log::writer(conn, ctx.space, a.writer).await? else {
        return refused(EvidenceRefusal::UnknownWriter);
    };
    if !a.verify(&writer.public_key) || !b.verify(&writer.public_key) {
        return refused(EvidenceRefusal::BadSignature);
    }
    record(
        conn,
        ctx.space,
        a.writer,
        a.seq,
        [(first, a.hash()), (second, b.hash())],
        now_ms,
    )
    .await
}

/// Settles fork evidence stored since the last engine write, inside one: both branches of every
/// forked writer of another device are excluded, which removes the merge effects of its applied
/// operations past the shared prefix, restores the versions they superseded, and drops the
/// operations, as if this replica had never stored them. The chain is then selected again, since
/// the dropped operations may carry entries and a fork of an anchor's writer freezes its epoch.
pub(crate) async fn reconcile(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
) -> SyncResult<()> {
    let settled =
        sqlx::query("UPDATE sync_fork_evidence SET settled = 1 WHERE space_id = ? AND settled = 0")
            .bind(ctx.space.as_bytes().as_slice())
            .execute(&mut *conn)
            .await?
            .rows_affected();
    if settled == 0 {
        return Ok(());
    }
    exclude_branches(engine, conn, ctx, batch).await?;
    if let Some(selection) = authorization::rederive(conn, ctx.space).await? {
        exclusion::settle(engine, conn, ctx, batch, &selection).await?;
    }
    Ok(())
}

/// Excludes both branches of every forked writer of another device that still holds them.
async fn exclude_branches(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    batch: &mut Batch,
) -> SyncResult<()> {
    let local = exclusion::local_device(conn, ctx.space).await?;
    let mut cut = Vec::new();
    for (writer, fork) in forked(conn, ctx.space).await? {
        if local.as_deref() == Some(fork.device_id.as_str()) {
            continue;
        }
        let Some(row) = log::writer(conn, ctx.space, writer).await? else {
            continue;
        };
        let keep = fork.keep_through();
        if row.stored > keep.max(1) || row.applied > keep {
            cut.push((writer, keep, row.applied));
        }
    }
    if cut.is_empty() {
        return Ok(());
    }
    if is_anchor(conn, ctx.space).await? {
        for (writer, keep, _) in &cut {
            collect_republish(engine, conn, ctx.space, batch, *writer, *keep).await?;
        }
    }
    // A row the branch created keeps the honest changes made to it but waits, hidden, for
    // another creation, as a replica that never stored the branch holds it.
    let mut created = std::collections::BTreeSet::new();
    for (writer, keep, applied) in &cut {
        if *applied > *keep {
            created.extend(exclusion::created_past(engine, conn, ctx.space, *writer, *keep).await?);
        }
    }
    let mut removed = Vec::new();
    for (writer, keep, applied) in &cut {
        if *applied > *keep {
            crate::reseal::remove_effects(conn, batch, *writer, *keep).await?;
            removed.push((*writer, *keep));
        }
        batch.applied.set(*writer, (*applied).min(*keep));
    }
    // Versions the forked operations wrote never come back, whichever writer displaced them.
    for (writer, keep, _) in &cut {
        sqlx::query("DELETE FROM sync_displaced_versions WHERE writer_id = ? AND seq > ?")
            .bind(writer.as_bytes().as_slice())
            .bind(seq_i64(*keep)?)
            .execute(&mut *conn)
            .await?;
    }
    for (writer, keep) in &removed {
        exclusion::restore_displaced(engine, conn, batch, *writer, *keep).await?;
    }
    if !removed.is_empty() {
        exclusion::drop_orphan_rows(conn, batch).await?;
    }
    merge::mark_uncreated(conn, &created).await?;
    for (writer, keep, applied) in &cut {
        drop_branch(conn, ctx.space, *writer, *keep, *applied).await?;
    }
    batch.contexts.clear();
    Ok(())
}

/// Whether the local device is the selected anchor.
pub(crate) async fn is_anchor(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<bool> {
    let devices: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT anchor_device_id, local_device_id FROM sync_spaces WHERE space_id = ?",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(matches!(devices, Some((Some(anchor), Some(local))) if anchor == local))
}

/// Whether the local device anchors an epoch a handover opened, so it seals again the groups
/// that operations past a removal cutoff write: an earlier anchor installation may have stored
/// and confirmed them, and members then released what they displaced. In the charter epoch no
/// earlier anchor exists, and sealing again would only bury late values an admin may restore.
pub(crate) async fn reseals_excluded(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<bool> {
    if !is_anchor(conn, space).await? {
        return Ok(false);
    }
    let handed_over: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM sync_auth_entries WHERE space_id = ? AND selected = 1 AND epoch > 0
         )",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_one(&mut *conn)
    .await?;
    Ok(handed_over)
}

/// Adds to [`Batch::republish`] every group that a stored operation of `writer` past `keep`
/// writes. Members may have released what those operations displaced once an anchor stored
/// them, applied or not, so the anchor seals all of them again.
pub(crate) async fn collect_republish(
    engine: &Engine,
    conn: &mut SqliteConnection,
    space: SpaceId,
    batch: &mut Batch,
    writer: WriterId,
    keep: Seq,
) -> SyncResult<()> {
    let envelopes: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq > ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(keep)?)
    .fetch_all(&mut *conn)
    .await?;
    for envelope in envelopes {
        let operation = Envelope::decode(&envelope)
            .ok()
            .and_then(|envelope| envelope.operation().ok())
            .ok_or_else(|| corrupt("a stored operation past a cutoff does not decode"))?;
        written_groups(engine, &operation.content, &mut batch.republish);
    }
    Ok(())
}

/// Adds to `groups` the groups that `content` writes. Tombstones need nothing, since removing
/// them revives the row with the versions they left in place.
pub(crate) fn written_groups(
    engine: &Engine,
    content: &Content,
    groups: &mut BTreeMap<(TableId, String), GroupMask>,
) {
    let Content::Changes(changes) = content else {
        return;
    };
    for change in changes {
        if change.action == ChangeAction::Tombstone
            || engine.manifest().table(change.table).is_none()
        {
            continue;
        }
        let mut mask = GroupMask::EMPTY;
        for group in &change.groups {
            mask.insert(group.group);
        }
        if mask != GroupMask::EMPTY {
            let entry = groups
                .entry((change.table, change.row.as_str().to_string()))
                .or_insert(GroupMask::EMPTY);
            *entry = GroupMask(entry.0 | mask.0);
        }
    }
}

/// Captures the groups [`Batch::republish`] holds, after the call materialized them, so the
/// anchor's next seal publishes their current values as its own operations.
pub(crate) async fn republish(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    now_ms: u64,
) -> SyncResult<()> {
    capture_groups(engine, conn, std::mem::take(&mut batch.republish), now_ms).await
}

/// Captures `groups` on live rows, so the next seal publishes their current values. A live row
/// of a derived-key table its domain table does not hold publishes its existence register as
/// absent when `groups` names it; any other row the domain table does not hold, and a row an
/// exclusion left orphaned or a tombstone deleted, is skipped.
pub(crate) async fn capture_groups(
    engine: &Engine,
    conn: &mut SqliteConnection,
    groups: BTreeMap<(TableId, String), GroupMask>,
    now_ms: u64,
) -> SyncResult<()> {
    for ((table, key), mask) in groups {
        let Some(spec) = engine.manifest().table(table) else {
            continue;
        };
        if merge::row_state(conn, table, &key).await? != Some(RowState::Live) {
            continue;
        }
        if sql::read_columns(conn, spec.name, spec.key_column, &key, &[])
            .await?
            .is_some()
        {
            record_restated(engine, conn, table, &key, mask).await?;
            seal::force_capture(conn, table, &key, mask, now_ms).await?;
        } else if let Some(exists) = spec.exists_group()
            && mask.contains(exists.id)
        {
            seal::force_delete_capture(conn, table, &key, exists.mask(), now_ms).await?;
        }
    }
    Ok(())
}

/// Records the winning value hash of each group in `mask` that a capture is about to seal again,
/// see [`restated`].
async fn record_restated(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    mask: GroupMask,
) -> SyncResult<()> {
    let Some(spec) = engine.manifest().table(table) else {
        return Ok(());
    };
    let versions = merge::versions(engine, conn, table, key).await?;
    for (group, winner) in merge::winners(engine, spec, &versions) {
        if !mask.contains(group) {
            continue;
        }
        sqlx::query(
            "INSERT INTO sync_restated_groups (table_id, row_key, group_id, value_hash)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (table_id, row_key, group_id) DO UPDATE SET value_hash = excluded.value_hash",
        )
        .bind(merge::table_param(table))
        .bind(key)
        .bind(i64::from(group.get()))
        .bind(winner.hash.as_slice())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Value hashes, by group, that the anchor sealed again on a row without a newer edit. A version
/// holding one of them restates an older value, so restoring a late change does not count it as
/// newer than the late value.
pub(crate) async fn restated(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<BTreeMap<GroupId, Digest32>> {
    let rows: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT group_id, value_hash FROM sync_restated_groups WHERE table_id = ? AND row_key = ?",
    )
    .bind(merge::table_param(table))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|(group, hash)| Ok((merge::group_id(group)?, log::digest(&hash)?)))
        .collect()
}

/// Forgets the restatements of rows with no late change waiting for a decision.
pub(crate) async fn drop_restated(conn: &mut SqliteConnection) -> SyncResult<()> {
    sqlx::query(
        "DELETE FROM sync_restated_groups AS g WHERE NOT EXISTS (
            SELECT 1 FROM sync_late_changes l
            WHERE l.table_id = g.table_id AND l.row_key = g.row_key AND l.resolution IS NULL
         )",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Drops the operations of `writer` past `keep`, with the authorization entries they carry, and
/// makes `keep` its head. A fork at the genesis keeps the stored genesis, unapplied, since a
/// known writer keeps at least one operation. Chain selection and late changes have to be
/// derived again afterwards.
pub(crate) async fn drop_branch(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    keep: Seq,
    applied: Seq,
) -> SyncResult<()> {
    let stored = keep.max(1);
    sqlx::query("DELETE FROM sync_auth_entries WHERE space_id = ? AND writer_id = ? AND seq > ?")
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(keep)?)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq > ?")
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(stored)?)
        .execute(&mut *conn)
        .await?;
    let (head_hash, head_clock): (Vec<u8>, i64) = sqlx::query_as(
        "SELECT hash, (SELECT max(clock) FROM sync_ops
                WHERE space_id = ?1 AND writer_id = ?2 AND seq <= ?3)
         FROM sync_ops WHERE space_id = ?1 AND writer_id = ?2 AND seq = ?3",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(stored)?)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| corrupt("the kept head of a forked writer is missing"))?;
    if keep == 0 {
        sqlx::query(
            "UPDATE sync_ops SET state = 'held', hold_reason = 'invalid'
             WHERE space_id = ? AND writer_id = ? AND seq = 1",
        )
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query(
        "UPDATE sync_writers SET stored_seq = ?1, applied_seq = ?2, head_hash = ?3,
            head_clock = ?4
         WHERE space_id = ?5 AND writer_id = ?6",
    )
    .bind(seq_i64(stored)?)
    .bind(seq_i64(applied.min(keep))?)
    .bind(head_hash.as_slice())
    .bind(head_clock)
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

impl Engine {
    /// Stores evidence that a writer forked: two envelopes of one writer at one sequence with
    /// different hashes, each signed by the writer's stored key. The writer becomes forked with
    /// the last shared sequence as its cutoff, so neither branch is stored from then on; the
    /// next [`Engine::apply`] excludes what this replica already holds of them.
    pub async fn store_fork_evidence(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        first: &[u8],
        second: &[u8],
        now_ms: u64,
    ) -> SyncResult<EvidenceOutcome> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let outcome = store_evidence(&mut tx, ctx, first, second, now_ms).await?;
        tx.commit().await?;
        Ok(outcome)
    }

    /// The stored fork evidence of `writer`.
    pub async fn fork_evidence(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: WriterId,
    ) -> SyncResult<Option<ForkEvidence>> {
        log::ensure_space(conn, ctx.space).await?;
        let row: Option<(i64, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT seq, first_envelope, second_envelope FROM sync_fork_evidence
             WHERE space_id = ? AND writer_id = ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .fetch_optional(&mut *conn)
        .await?;
        row.map(|(seq, first, second)| {
            Ok(ForkEvidence {
                writer,
                seq: stored_u64(seq, "fork sequence")?,
                first,
                second,
            })
        })
        .transpose()
    }

    /// Forked writers of the local device whose copy past the shared prefix this installation
    /// still holds, with that prefix. Other replicas exclude that copy, so it reaches them only
    /// once [`Engine::reseal_fork`] seals it again with a new writer.
    pub async fn own_forks(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Vec<(WriterId, Seq)>> {
        log::ensure_space(conn, ctx.space).await?;
        let Some(local) = exclusion::local_device(conn, ctx.space).await? else {
            return Ok(Vec::new());
        };
        let mut own = Vec::new();
        for (writer, fork) in forked(conn, ctx.space).await? {
            if fork.device_id != local {
                continue;
            }
            let keep = fork.keep_through();
            if log::writer(conn, ctx.space, writer)
                .await?
                .is_some_and(|row| row.applied > keep)
            {
                own.push((writer, keep));
            }
        }
        Ok(own)
    }

    /// Every writer with stored fork evidence and the sequence where it forked, by writer id.
    pub async fn forked_writers(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Vec<(WriterId, Seq)>> {
        log::ensure_space(conn, ctx.space).await?;
        Ok(forked(conn, ctx.space)
            .await?
            .into_iter()
            .map(|(writer, fork)| (writer, fork.seq))
            .collect())
    }

    /// The stored envelope of `writer` at `seq`, unless it is not stored or compaction dropped
    /// its body.
    pub async fn operation_at(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: WriterId,
        seq: Seq,
    ) -> SyncResult<Option<Vec<u8>>> {
        log::ensure_space(conn, ctx.space).await?;
        let envelope: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(seq)?)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(envelope.filter(|envelope| !envelope.is_empty()))
    }
}
