//! Installation-level helpers around the engine: the space context of a vault, the local
//! writer high-water check, snapshot preparation, and carry-forward of operations across a
//! database replacement.

use ganbaru_contacts::PersonPublicKey;
use ganbaru_sync_contracts::{Envelope, Seq, SpaceId, VersionVector, WriterId, WriterPublicKey};
use sqlx::{Connection, SqliteConnection};

use crate::error::{SyncResult, corrupt};
use crate::log::{self, WriterState};
use crate::{Engine, SpaceContext, StoreOutcome, sql};

/// Most operations one carry-forward store transaction takes.
const IMPORT_CHUNK: usize = 256;
/// Magic prefix of a carry-forward bundle.
const BUNDLE_MAGIC: &[u8; 8] = b"GSCARRY1";
/// Largest carry-forward bundle accepted, in bytes.
pub const MAX_BUNDLE_BYTES: usize = 256 << 20;

/// What the vault database records about a local writer key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalWriterHead {
    /// The space has no writer for the key.
    Absent,
    /// The writer's head operation verifies with the key.
    Genuine {
        /// Highest stored sequence of the writer.
        stored: Seq,
        /// Lifecycle state of the writer.
        state: WriterState,
    },
    /// The stored writer does not verify with the key.
    Mismatch,
}

/// The space context of a vault: its personal space anchored at the person key of
/// `contacts_local_identity`, or `None` while the vault has no person identity.
pub async fn space_context(
    conn: &mut SqliteConnection,
    vault_id: &str,
) -> SyncResult<Option<SpaceContext>> {
    let public_key: Option<String> =
        sqlx::query_scalar("SELECT public_key FROM contacts_local_identity WHERE singleton = 1")
            .fetch_optional(&mut *conn)
            .await?;
    let Some(public_key) = public_key else {
        return Ok(None);
    };
    let anchor = PersonPublicKey::from_text(&public_key)
        .map_err(|_| corrupt("stored person public key is invalid"))?;
    Ok(Some(SpaceContext {
        space: SpaceId::personal(vault_id),
        anchor,
    }))
}

/// Whether local captures wait to be sealed.
pub async fn has_pending_captures(conn: &mut SqliteConnection) -> SyncResult<bool> {
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sync_capture)")
        .fetch_one(&mut *conn)
        .await?;
    Ok(pending)
}

/// Number of local captures that wait to be sealed.
pub async fn pending_captures(conn: &mut SqliteConnection) -> SyncResult<u64> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_capture")
        .fetch_one(&mut *conn)
        .await?;
    log::stored_u64(count, "capture count")
}

/// Clears the apply flag left by an interrupted process. Engine transactions reset it before
/// commit, so a set flag outside a transaction only comes from a copied database file.
pub async fn reset_applying(conn: &mut SqliteConnection) -> SyncResult<()> {
    sql::set_applying(conn, false).await?;
    Ok(())
}

/// Prepares a snapshot copy for another installation: captures belong to the installation
/// that made them, so the copy drops any that could not be sealed, and the apply flag is clear.
pub async fn prepare_snapshot_copy(conn: &mut SqliteConnection) -> SyncResult<()> {
    let mut tx = conn.begin().await?;
    sqlx::query("DELETE FROM sync_capture")
        .execute(&mut *tx)
        .await?;
    sql::set_applying(&mut tx, false).await?;
    tx.commit().await?;
    Ok(())
}

impl Engine {
    /// The stored state of the writer a key derives, verifying its head operation.
    pub async fn local_writer_head(
        &self,
        conn: &mut SqliteConnection,
        space: SpaceId,
        key: &WriterPublicKey,
    ) -> SyncResult<LocalWriterHead> {
        let id = WriterId::for_public_key(key);
        let Some(row) = log::writer(conn, space, id).await? else {
            return Ok(LocalWriterHead::Absent);
        };
        if row.public_key != *key {
            return Ok(LocalWriterHead::Mismatch);
        }
        let envelope: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
        )
        .bind(space.as_bytes().as_slice())
        .bind(id.as_bytes().as_slice())
        .bind(log::seq_i64(row.stored)?)
        .fetch_optional(&mut *conn)
        .await?;
        let genuine = envelope.as_deref().is_some_and(|bytes| {
            Envelope::decode(bytes).is_ok_and(|envelope| {
                envelope.seq == row.stored
                    && envelope.hash() == row.head_hash
                    && envelope.verify(key)
            })
        });
        Ok(if genuine {
            LocalWriterHead::Genuine {
                stored: row.stored,
                state: row.state,
            }
        } else {
            LocalWriterHead::Mismatch
        })
    }

    /// Stored operations of `local` that a replacement database with stored vector
    /// `replacement` lacks, from every writer, in an order that keeps each chain contiguous.
    pub async fn carry_forward_ops(
        &self,
        local: &mut SqliteConnection,
        ctx: &SpaceContext,
        replacement: &VersionVector,
    ) -> SyncResult<Vec<Vec<u8>>> {
        if !space_exists(local, ctx.space).await? {
            return Ok(Vec::new());
        }
        Ok(self
            .ops_after(local, ctx, replacement, usize::MAX)
            .await?
            .ops)
    }

    /// Stores and applies carried operations in the replacement database. Operations already
    /// stored with the same hash are skipped, so a repeated import is harmless.
    pub async fn import_carried(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        vault_id: &str,
        ops: &[Vec<u8>],
        now_ms: u64,
    ) -> SyncResult<CarryReport> {
        let mut report = CarryReport::default();
        if ops.is_empty() {
            return Ok(report);
        }
        self.init_space(conn, vault_id, now_ms).await?;
        for chunk in ops.chunks(IMPORT_CHUNK) {
            for outcome in self.store_many(conn, ctx, chunk, now_ms).await? {
                match outcome {
                    StoreOutcome::Refused(_) => report.refused += 1,
                    _ => report.stored += 1,
                }
            }
        }
        loop {
            let applied = self.apply(conn, ctx, None, now_ms).await?;
            report.applied += applied.applied;
            if applied.pending_captures {
                report.pending_captures = true;
                break;
            }
            if !applied.more {
                break;
            }
        }
        Ok(report)
    }
}

/// Result of a carry-forward import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CarryReport {
    /// Operations stored or already present.
    pub stored: usize,
    /// Operations the replacement refused.
    pub refused: usize,
    /// Operations applied.
    pub applied: usize,
    /// Local captures in the replacement stopped the apply.
    pub pending_captures: bool,
}

async fn space_exists(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<bool> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sync_spaces WHERE space_id = ?)")
            .bind(space.as_bytes().as_slice())
            .fetch_one(&mut *conn)
            .await?;
    Ok(exists)
}

/// Stored vector of a replacement database, empty when it has no space yet.
pub async fn replacement_vector(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<VersionVector> {
    if !space_exists(conn, space).await? {
        return Ok(VersionVector::default());
    }
    log::stored_vector(conn, space).await
}

/// Encodes carried operations of a space as a device-local staging bundle.
pub fn encode_bundle(space: SpaceId, ops: &[Vec<u8>]) -> Vec<u8> {
    let length = ops.iter().map(|op| op.len() + 4).sum::<usize>();
    let mut bytes = Vec::with_capacity(BUNDLE_MAGIC.len() + 16 + length);
    bytes.extend_from_slice(BUNDLE_MAGIC);
    bytes.extend_from_slice(space.as_bytes());
    for op in ops {
        let size = u32::try_from(op.len()).expect("stored operations are bounded");
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(op);
    }
    bytes
}

/// Decodes a staging bundle, refusing truncated or oversized input.
pub fn decode_bundle(bytes: &[u8]) -> SyncResult<(SpaceId, Vec<Vec<u8>>)> {
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err(corrupt("carry-forward bundle is too large"));
    }
    let rest = bytes
        .strip_prefix(BUNDLE_MAGIC.as_slice())
        .ok_or_else(|| corrupt("carry-forward bundle has no header"))?;
    let (space, mut rest) = rest
        .split_first_chunk::<16>()
        .ok_or_else(|| corrupt("carry-forward bundle has no space"))?;
    let space = SpaceId::from_bytes(*space);
    let mut ops = Vec::new();
    while !rest.is_empty() {
        let (size, tail) = rest
            .split_first_chunk::<4>()
            .ok_or_else(|| corrupt("carry-forward bundle is truncated"))?;
        let size = usize::try_from(u32::from_be_bytes(*size))
            .map_err(|_| corrupt("carry-forward operation size"))?;
        if size > tail.len() {
            return Err(corrupt("carry-forward bundle is truncated"));
        }
        let (op, tail) = tail.split_at(size);
        ops.push(op.to_vec());
        rest = tail;
    }
    Ok((space, ops))
}
