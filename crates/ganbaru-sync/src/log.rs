//! The stored operation log: writers, causal contexts, version vectors, and incremental reads.
//!
//! The causal context of an operation is every operation it was sealed after:
//! `context(n) = context(n - 1) ∪ dependencies(n) ∪ {(writer, n - 1)}`, empty for a genesis. It
//! is stored as an encoded version vector; a zero-length blob means it is not known yet because
//! the previous operation's context was unknown when the operation was stored.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use ganbaru_sync_contracts::{Digest32, Seq, SpaceId, VersionVector, WriterId, WriterPublicKey};
use sqlx::{Row, SqliteConnection};

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
     applied_seq, head_hash, head_clock";

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
    })
}

pub(crate) async fn writer(
    conn: &mut SqliteConnection,
    space: SpaceId,
    id: WriterId,
) -> SyncResult<Option<WriterRow>> {
    let sql =
        format!("SELECT {WRITER_COLUMNS} FROM sync_writers WHERE space_id = ? AND writer_id = ?");
    let row = sqlx::query(&sql)
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
    let rows = sqlx::query(&sql)
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
#[derive(Debug, Default)]
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
    let rows = sqlx::query(&sql)
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

/// One pending operation of a page with its arrival position.
struct PageEntry {
    rowid: i64,
    seq: Seq,
    bytes: usize,
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
    /// arrival while keeping every writer's operations in sequence order. The page holds at
    /// least one operation when any remains, then stops before `max_bytes`.
    pub async fn ops_after(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        known: &VersionVector,
        max_bytes: usize,
    ) -> SyncResult<OpsPage> {
        ensure_space(conn, ctx.space).await?;
        let mut lanes: Vec<(WriterId, Vec<PageEntry>)> = Vec::new();
        for writer in writers(conn, ctx.space).await? {
            let from = known.get(&writer.id);
            if writer.stored <= from {
                continue;
            }
            let rows = sqlx::query(
                "SELECT rowid, seq, length(envelope) FROM sync_ops
                 WHERE space_id = ? AND writer_id = ? AND seq > ? ORDER BY seq",
            )
            .bind(ctx.space.as_bytes().as_slice())
            .bind(writer.id.as_bytes().as_slice())
            .bind(seq_i64(from)?)
            .fetch_all(&mut *conn)
            .await?;
            let entries = rows
                .iter()
                .map(|row| {
                    Ok(PageEntry {
                        rowid: row.try_get(0)?,
                        seq: stored_u64(row.try_get(1)?, "sequence")?,
                        bytes: usize::try_from(row.try_get::<i64, _>(2)?).unwrap_or(usize::MAX),
                    })
                })
                .collect::<SyncResult<Vec<_>>>()?;
            if !entries.is_empty() {
                lanes.push((writer.id, entries));
            }
        }
        let mut cursors = vec![0usize; lanes.len()];
        let mut page = OpsPage::default();
        let mut total = 0usize;
        loop {
            let next = lanes
                .iter()
                .enumerate()
                .filter_map(|(lane, (_, entries))| {
                    entries.get(cursors[lane]).map(|entry| (entry.rowid, lane))
                })
                .min();
            let Some((_, lane)) = next else {
                break;
            };
            let (writer, entries) = &lanes[lane];
            let entry = &entries[cursors[lane]];
            if !page.ops.is_empty() && total.saturating_add(entry.bytes) > max_bytes {
                page.more = true;
                break;
            }
            let envelope: Vec<u8> = sqlx::query_scalar(
                "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
            )
            .bind(ctx.space.as_bytes().as_slice())
            .bind(writer.as_bytes().as_slice())
            .bind(seq_i64(entry.seq)?)
            .fetch_one(&mut *conn)
            .await?;
            total = total.saturating_add(envelope.len());
            page.ops.push(envelope);
            cursors[lane] += 1;
        }
        Ok(page)
    }

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
