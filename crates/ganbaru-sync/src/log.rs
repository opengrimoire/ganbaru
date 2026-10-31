//! The stored operation log: writers, causal contexts, version vectors, and incremental reads.
//!
//! The causal context of an operation is every operation it was sealed after:
//! `context(n) = context(n - 1) ∪ dependencies(n) ∪ {(writer, n - 1)}`, empty for a genesis. It
//! is stored as an encoded version vector; a zero-length blob means it is not known yet because
//! the previous operation's context was unknown when the operation was stored.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, VecDeque};

use ganbaru_sync_contracts::{
    Digest32, OperationKind, Seq, SpaceId, VersionVector, WriterId, WriterPublicKey,
};
use sqlx::{AssertSqlSafe, Row, SqliteConnection};

use crate::error::{SyncResult, corrupt};
use crate::{Engine, SpaceContext, SyncError};

/// Lifecycle state of a writer in a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriterState {
    /// The writer may seal.
    Active,
    /// The installation replaced the writer after re-sealing its operations above the cutoff,
    /// which are refused from then on.
    Retired,
    /// Operations above the cutoff are refused because the device was unlinked.
    Revoked,
    /// Operations above the cutoff are refused because the chain forked.
    Forked,
}

impl WriterState {
    /// Stored name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retired => "retired",
            Self::Revoked => "revoked",
            Self::Forked => "forked",
        }
    }

    pub(crate) fn parse(text: &str) -> SyncResult<Self> {
        match text {
            "active" => Ok(Self::Active),
            "retired" => Ok(Self::Retired),
            "revoked" => Ok(Self::Revoked),
            "forked" => Ok(Self::Forked),
            _ => Err(corrupt(format!("unknown writer state {text}"))),
        }
    }
}

/// A writer row.
#[derive(Debug, Clone)]
pub(crate) struct WriterRow {
    pub(crate) id: WriterId,
    pub(crate) public_key: WriterPublicKey,
    pub(crate) device_id: String,
    pub(crate) state: WriterState,
    pub(crate) cutoff: Option<Seq>,
    pub(crate) stored: Seq,
    pub(crate) applied: Seq,
    pub(crate) head_hash: Digest32,
    pub(crate) head_clock: u64,
    /// Highest sequence whose body compaction may have dropped.
    pub(crate) compacted: Seq,
    /// Cutoff the selected removal of the writer's device lists for it; later operations are
    /// excluded.
    pub(crate) removal_cutoff: Option<Seq>,
}

/// Operations a peer does not have, in an order that keeps every writer's chain contiguous.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpsPage {
    /// Envelope bytes.
    pub ops: Vec<Vec<u8>>,
    /// Whether more operations remain beyond the byte budget.
    pub more: bool,
}

/// Clamps Unix milliseconds into the stored integer range.
pub(crate) fn clamp_ms(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX)
}

/// Storage order of the next operation a space stores, one past the latest.
///
/// Chain opening follows storage order, which a wall clock step backward cannot reorder, while
/// stored time stays the wall clock that acknowledgement intervals and dormancy measure.
pub(crate) async fn next_stored_order(
    conn: &mut SqliteConnection,
    space: &SpaceId,
) -> SyncResult<i64> {
    let latest: Option<i64> =
        sqlx::query_scalar("SELECT max(stored_order) FROM sync_ops WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_one(&mut *conn)
            .await?;
    Ok(latest.unwrap_or(0).saturating_add(1))
}

/// Converts a stored non-negative integer.
pub(crate) fn stored_u64(value: i64, what: &str) -> SyncResult<u64> {
    u64::try_from(value).map_err(|_| corrupt(format!("negative {what}")))
}

/// Converts a sequence for storage.
pub(crate) fn seq_i64(seq: Seq) -> SyncResult<i64> {
    i64::try_from(seq).map_err(|_| corrupt("sequence out of range"))
}

pub(crate) fn writer_id(bytes: &[u8]) -> SyncResult<WriterId> {
    WriterId::from_slice(bytes).ok_or_else(|| corrupt("writer id length"))
}

pub(crate) fn digest(bytes: &[u8]) -> SyncResult<Digest32> {
    bytes.try_into().map_err(|_| corrupt("digest length"))
}

/// Encoded context; unknown contexts are the empty blob.
pub(crate) fn encode_context(context: Option<&VersionVector>) -> Vec<u8> {
    context.map(VersionVector::encode).unwrap_or_default()
}

pub(crate) fn decode_context(bytes: &[u8]) -> SyncResult<Option<VersionVector>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    VersionVector::decode(bytes)
        .map(Some)
        .map_err(|_| corrupt("stored context does not decode"))
}

/// The context of operation `seq` given its predecessor's context and its dependencies.
pub(crate) fn next_context(
    previous: &VersionVector,
    writer: WriterId,
    seq: Seq,
    dependencies: &[(WriterId, Seq)],
) -> VersionVector {
    let mut context = previous.clone();
    for (dependency, dependency_seq) in dependencies {
        context.advance(*dependency, *dependency_seq);
    }
    context.advance(writer, seq - 1);
    context
}

/// Fails unless the space is initialized.
pub(crate) async fn ensure_space(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<()> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM sync_spaces WHERE space_id = ?")
        .bind(space.as_bytes().as_slice())
        .fetch_optional(&mut *conn)
        .await?;
    exists.map(|_| ()).ok_or(SyncError::UnknownSpace)
}

const WRITER_COLUMNS: &str = "writer_id, public_key, device_id, state, cutoff_seq, stored_seq, \
     applied_seq, head_hash, head_clock, compacted_seq, removal_cutoff_seq";

fn writer_row(row: &sqlx::sqlite::SqliteRow) -> SyncResult<WriterRow> {
    let id: Vec<u8> = row.try_get(0)?;
    let public_key: Vec<u8> = row.try_get(1)?;
    let state: String = row.try_get(3)?;
    let cutoff: Option<i64> = row.try_get(4)?;
    let head_hash: Vec<u8> = row.try_get(7)?;
    Ok(WriterRow {
        id: writer_id(&id)?,
        public_key: WriterPublicKey::from_slice(&public_key)
            .map_err(|_| corrupt("writer public key"))?,
        device_id: row.try_get(2)?,
        state: WriterState::parse(&state)?,
        cutoff: cutoff.map(|seq| stored_u64(seq, "cutoff")).transpose()?,
        stored: stored_u64(row.try_get(5)?, "stored sequence")?,
        applied: stored_u64(row.try_get(6)?, "applied sequence")?,
        head_hash: digest(&head_hash)?,
        head_clock: stored_u64(row.try_get(8)?, "head clock")?,
        compacted: stored_u64(row.try_get(9)?, "compacted sequence")?,
        removal_cutoff: row
            .try_get::<Option<i64>, _>(10)?
            .map(|seq| stored_u64(seq, "removal cutoff"))
            .transpose()?,
    })
}

pub(crate) async fn writer(
    conn: &mut SqliteConnection,
    space: SpaceId,
    id: WriterId,
) -> SyncResult<Option<WriterRow>> {
    let sql =
        format!("SELECT {WRITER_COLUMNS} FROM sync_writers WHERE space_id = ? AND writer_id = ?");
    let row = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(space.as_bytes().as_slice())
        .bind(id.as_bytes().as_slice())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref().map(writer_row).transpose()
}

pub(crate) async fn writers(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Vec<WriterRow>> {
    let sql =
        format!("SELECT {WRITER_COLUMNS} FROM sync_writers WHERE space_id = ? ORDER BY writer_id");
    let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(space.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(writer_row).collect()
}

/// Stored context of an operation: outer `None` when the operation is not stored, inner `None`
/// when its context is unknown.
pub(crate) async fn load_context(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<Option<Option<VersionVector>>> {
    let bytes: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT context FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    bytes.map(|bytes| decode_context(&bytes)).transpose()
}

/// Contexts of operations, cached for one engine transaction.
#[derive(Debug, Default, Clone)]
pub(crate) struct ContextCache {
    contexts: HashMap<(WriterId, Seq), VersionVector>,
}

impl ContextCache {
    /// Known context of a stored operation.
    pub(crate) async fn get(
        &mut self,
        conn: &mut SqliteConnection,
        space: SpaceId,
        writer: WriterId,
        seq: Seq,
    ) -> SyncResult<&VersionVector> {
        if let Entry::Vacant(entry) = self.contexts.entry((writer, seq)) {
            let context = load_context(conn, space, writer, seq)
                .await?
                .flatten()
                .ok_or_else(|| corrupt("context of an applied operation is unknown"))?;
            entry.insert(context);
        }
        Ok(&self.contexts[&(writer, seq)])
    }

    pub(crate) fn insert(&mut self, writer: WriterId, seq: Seq, context: VersionVector) {
        self.contexts.insert((writer, seq), context);
    }

    pub(crate) fn clear(&mut self) {
        self.contexts.clear();
    }
}

async fn vector(
    conn: &mut SqliteConnection,
    space: SpaceId,
    column: &str,
) -> SyncResult<VersionVector> {
    let sql = format!("SELECT writer_id, {column} FROM sync_writers WHERE space_id = ?");
    let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(space.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?;
    let mut vector = VersionVector::new();
    for row in rows {
        let id: Vec<u8> = row.try_get(0)?;
        vector.set(writer_id(&id)?, stored_u64(row.try_get(1)?, column)?);
    }
    Ok(vector)
}

pub(crate) async fn applied_vector(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<VersionVector> {
    vector(conn, space, "applied_seq").await
}

pub(crate) async fn stored_vector(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<VersionVector> {
    vector(conn, space, "stored_seq").await
}

/// Highest compacted sequence per writer, without writers that compacted nothing.
pub(crate) async fn compacted_vector(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<VersionVector> {
    vector(conn, space, "compacted_seq").await
}

/// One pending operation of a page with its arrival position.
struct PageEntry {
    rowid: i64,
    seq: Seq,
    bytes: usize,
}

/// Pending operations of one writer a page loads at a time, so a page never reads the
/// metadata of a whole log it will not send.
const PAGE_LANE_CHUNK: i64 = 64;

/// The loaded pending operations of one writer in sequence order.
struct PageLane {
    writer: WriterId,
    entries: VecDeque<PageEntry>,
    after: Seq,
    exhausted: bool,
    /// Whether the selection holds the writer's device for data, so the lane ends before its
    /// first row change.
    held: bool,
}

impl PageLane {
    /// Loads the next chunk when every loaded entry was taken and more may remain.
    async fn refill(&mut self, conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<()> {
        if !self.entries.is_empty() || self.exhausted {
            return Ok(());
        }
        let rows = sqlx::query(
            "SELECT rowid, seq, length(envelope), kind FROM sync_ops
             WHERE space_id = ? AND writer_id = ? AND seq > ? ORDER BY seq LIMIT ?",
        )
        .bind(space.as_bytes().as_slice())
        .bind(self.writer.as_bytes().as_slice())
        .bind(seq_i64(self.after)?)
        .bind(PAGE_LANE_CHUNK)
        .fetch_all(&mut *conn)
        .await?;
        self.exhausted = rows.len() < PAGE_LANE_CHUNK as usize;
        for row in &rows {
            if self.held && row.try_get::<i64, _>(3)? == i64::from(OperationKind::Changes.as_u8()) {
                self.exhausted = true;
                break;
            }
            let entry = PageEntry {
                rowid: row.try_get(0)?,
                seq: stored_u64(row.try_get(1)?, "sequence")?,
                bytes: usize::try_from(row.try_get::<i64, _>(2)?).unwrap_or(usize::MAX),
            };
            self.after = entry.seq;
            self.entries.push_back(entry);
        }
        Ok(())
    }
}

impl Engine {
    /// Highest stored sequence per writer, the vector a peer pulls from.
    pub async fn stored_vector(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<VersionVector> {
        ensure_space(conn, ctx.space).await?;
        stored_vector(conn, ctx.space).await
    }

    /// Highest applied sequence per writer.
    pub async fn applied_vector(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<VersionVector> {
        ensure_space(conn, ctx.space).await?;
        applied_vector(conn, ctx.space).await
    }

    /// Stored operations that `known` does not include, held operations among them, ordered by
    /// arrival while keeping every writer's operations in sequence order. A writer of a device
    /// the selection holds for data is served only up to its first row change, so its
    /// authorization records still spread while its data waits. The page holds at least one
    /// operation when any remains, then stops before `max_bytes`.
    ///
    /// Fails with [`SyncError::Compacted`] when `known` lacks an operation whose body
    /// compaction dropped, since the peer can then only bootstrap from a snapshot.
    pub async fn ops_after(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        known: &VersionVector,
        max_bytes: usize,
    ) -> SyncResult<OpsPage> {
        page_after(conn, ctx, known, max_bytes, false).await
    }

    /// [`Self::ops_after`] for a peer the selection holds for data: every writer's chain is
    /// served only up to its first row change, so the peer still receives the authorization
    /// records that can resolve the contest, but no data.
    pub async fn authorization_ops_after(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        known: &VersionVector,
        max_bytes: usize,
    ) -> SyncResult<OpsPage> {
        page_after(conn, ctx, known, max_bytes, true).await
    }

    /// Whether `device_id` is held for data by the selected chain of the space.
    pub async fn device_held(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        device_id: &str,
    ) -> SyncResult<bool> {
        ensure_space(conn, ctx.space).await?;
        let held: Option<i64> = sqlx::query_scalar(
            "SELECT held FROM sync_devices WHERE space_id = ? AND device_id = ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(device_id)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(held == Some(1))
    }
}

/// The page of [`Engine::ops_after`], with every lane ending before its first row change when
/// `authorization_only` is set.
async fn page_after(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    known: &VersionVector,
    max_bytes: usize,
    authorization_only: bool,
) -> SyncResult<OpsPage> {
    ensure_space(conn, ctx.space).await?;
    let held: BTreeSet<String> =
        sqlx::query_scalar("SELECT device_id FROM sync_devices WHERE space_id = ? AND held = 1")
            .bind(ctx.space.as_bytes().as_slice())
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .collect();
    let mut lanes = Vec::new();
    for writer in writers(conn, ctx.space).await? {
        let from = known.get(&writer.id);
        if writer.stored <= from {
            continue;
        }
        if from < writer.compacted {
            return Err(SyncError::Compacted {
                writer: writer.id,
                seq: writer.compacted,
            });
        }
        let mut lane = PageLane {
            writer: writer.id,
            entries: VecDeque::new(),
            after: from,
            exhausted: false,
            held: authorization_only || held.contains(&writer.device_id),
        };
        lane.refill(conn, ctx.space).await?;
        if !lane.entries.is_empty() {
            lanes.push(lane);
        }
    }
    let mut page = OpsPage::default();
    let mut total = 0usize;
    loop {
        let next = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, lane)| lane.entries.front().map(|entry| (entry.rowid, index)))
            .min();
        let Some((_, index)) = next else {
            break;
        };
        let lane = &mut lanes[index];
        let Some(entry) = lane.entries.front() else {
            break;
        };
        if !page.ops.is_empty() && total.saturating_add(entry.bytes) > max_bytes {
            page.more = true;
            break;
        }
        let envelope: Vec<u8> = sqlx::query_scalar(
            "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(lane.writer.as_bytes().as_slice())
        .bind(seq_i64(entry.seq)?)
        .fetch_one(&mut *conn)
        .await?;
        if envelope.is_empty() {
            return Err(corrupt(
                "an operation above the compacted sequence has no body",
            ));
        }
        total = total.saturating_add(envelope.len());
        page.ops.push(envelope);
        lane.entries.pop_front();
        lane.refill(conn, ctx.space).await?;
    }
    Ok(page)
}

impl Engine {
    /// Hashes of a writer's stored operations from `from_seq`, at most `limit`, for locating
    /// where two copies of a chain diverge.
    pub async fn op_hashes(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: WriterId,
        from_seq: Seq,
        limit: u32,
    ) -> SyncResult<Vec<(Seq, Digest32)>> {
        ensure_space(conn, ctx.space).await?;
        let rows = sqlx::query(
            "SELECT seq, hash FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq >= ?
             ORDER BY seq LIMIT ?",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(from_seq)?)
        .bind(i64::from(limit))
        .fetch_all(&mut *conn)
        .await?;
        rows.iter()
            .map(|row| {
                let hash: Vec<u8> = row.try_get(1)?;
                Ok((stored_u64(row.try_get(0)?, "sequence")?, digest(&hash)?))
            })
            .collect()
    }
}
