//! Installation-level helpers around the engine: the space context of a vault, retained group
//! values, the local writer high-water check, snapshot preparation, carry-forward of operations
//! across a database replacement, and the restore of device state into that replacement.

mod restore;

pub use restore::{LocalRestoreReport, restore_local_state};
#[cfg(test)]
pub(crate) use restore::{read_only_uri, repair_foreign_keys};

use ganbaru_contacts::PersonPublicKey;
use ganbaru_sync_contracts::{
    AdminPublicKey, Digest32, EncryptionPublicKey, Envelope, GroupId, RecoveryPublicKeys, Seq,
    SpaceId, TableId, Value, VersionVector, WriterId, WriterPublicKey,
};
use sqlx::{AssertSqlSafe, Connection, Row, SqliteConnection};
use std::collections::BTreeSet;

use crate::error::{SyncError, SyncResult, corrupt};
use crate::hooks::Notices;
use crate::log::{self, WriterState};
use crate::manifest::Manifest;
use crate::{Engine, SpaceContext, StoreOutcome, hydrate, publication, sql};

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

/// The person key of `contacts_local_identity` that anchors the vault's space, or `None` while
/// the vault has no person identity and therefore does not sync.
pub async fn space_anchor(conn: &mut SqliteConnection) -> SyncResult<Option<PersonPublicKey>> {
    let public_key: Option<String> =
        sqlx::query_scalar("SELECT public_key FROM contacts_local_identity WHERE singleton = 1")
            .fetch_optional(&mut *conn)
            .await?;
    public_key
        .map(|public_key| {
            PersonPublicKey::from_text(&public_key)
                .map_err(|_| corrupt("stored person public key is invalid"))
        })
        .transpose()
}

/// The space context of a vault: its personal space, whose charter the person key of
/// `contacts_local_identity` signs, or `None` while the vault has no person identity.
pub async fn space_context(
    conn: &mut SqliteConnection,
    vault_id: &str,
) -> SyncResult<Option<SpaceContext>> {
    Ok(space_anchor(conn).await?.map(|person_key| SpaceContext {
        space: SpaceId::personal(vault_id),
        person_key,
    }))
}

/// Whether local captures wait to be sealed.
pub async fn has_pending_captures(conn: &mut SqliteConnection) -> SyncResult<bool> {
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sync_capture)")
        .fetch_one(&mut *conn)
        .await?;
    Ok(pending)
}

/// Every retained version value of one group of a table, for live and tombstoned rows alike:
/// the values that open conflicts and recovery offers can still bring back.
/// An elided value is read back from the domain row or the operation that wrote it.
pub async fn retained_group_values(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    group: GroupId,
) -> SyncResult<Vec<Value>> {
    let spec = engine
        .manifest()
        .table(table)
        .ok_or(SyncError::InvalidRequest("unknown table"))?;
    let rows = sqlx::query(
        "SELECT row_key, writer_id, seq, value_hash, value FROM sync_register_versions
         WHERE table_id = ? AND group_id = ?",
    )
    .bind(i64::from(table.0))
    .bind(i64::from(group.get()))
    .fetch_all(&mut *conn)
    .await?;
    let mut values = Vec::with_capacity(rows.len());
    for row in &rows {
        let value = match row.try_get::<Option<Vec<u8>>, _>(4)? {
            Some(value) => {
                Value::decode(&value).map_err(|_| corrupt("stored value does not decode"))?
            }
            None => {
                let key: String = row.try_get(0)?;
                let writer = log::writer_id(&row.try_get::<Vec<u8>, _>(1)?)?;
                let seq = log::stored_u64(row.try_get(2)?, "version sequence")?;
                let hash = log::digest(&row.try_get::<Vec<u8>, _>(3)?)?;
                hydrate::elided_value(engine, conn, spec, &key, group, writer, seq, &hash).await?
            }
        };
        values.push(value);
    }
    Ok(values)
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

/// Clears every engine table in one transaction, which is the database half of Reset sync state
/// on the coordinator: the space, its charter and authorization log, writers, operations, row
/// state, captures, the record of seeded tables, and device-bound engine state. Domain rows and
/// the apply flag row stay, so the next vault open founds a new space and republishes every
/// current row through [`crate::seed::seed_captures`].
pub async fn reset_sync_state(conn: &mut SqliteConnection) -> SyncResult<()> {
    let mut tx = conn.begin().await?;
    sql::begin_engine_write(&mut tx).await?;
    for table in crate::manifest::SYNC_TABLES
        .iter()
        .chain(crate::manifest::SYNC_LOCAL_TABLES)
        .filter(|table| !RESET_KEEPS.contains(*table))
    {
        sqlx::query(AssertSqlSafe(format!("DELETE FROM {}", sql::ident(table))))
            .execute(&mut *tx)
            .await?;
    }
    sql::set_applying(&mut tx, false).await?;
    tx.commit().await?;
    Ok(())
}

/// Engine tables [`reset_sync_state`] keeps: the singleton whose flag domain triggers read,
/// and the public keys of the recovery kit the user saved, which founds the next space too.
pub const RESET_KEEPS: &[&str] = &["sync_apply_state", RECOVERY_KIT_TABLE];

/// The singleton engine table holding the public keys of the vault's recovery kit.
const RECOVERY_KIT_TABLE: &str = "sync_recovery_kit";

/// The public keys of the vault's recovery kit, `None` before a kit exists. Founding a space
/// names the recovery admin key in its charter, and the anchor publishes the recovery
/// encryption key in the space's first recovery key entry.
pub async fn recovery_kit_keys(
    conn: &mut SqliteConnection,
) -> SyncResult<Option<RecoveryPublicKeys>> {
    let row = sqlx::query(
        "SELECT recovery_admin_key, recovery_encryption_key FROM sync_recovery_kit WHERE id = 1",
    )
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let admin: Vec<u8> = row.try_get("recovery_admin_key")?;
    let encryption: Vec<u8> = row.try_get("recovery_encryption_key")?;
    let admin: [u8; 32] = admin
        .try_into()
        .map_err(|_| corrupt("stored recovery admin key has the wrong length"))?;
    let encryption: [u8; 32] = encryption
        .try_into()
        .map_err(|_| corrupt("stored recovery encryption key has the wrong length"))?;
    Ok(Some(RecoveryPublicKeys {
        admin: AdminPublicKey::from_bytes(admin),
        encryption: EncryptionPublicKey::from_bytes(encryption)
            .map_err(|error| corrupt(format!("stored recovery encryption key: {error}")))?,
    }))
}

/// Records `keys` as the public keys of the vault's recovery kit, replacing any earlier kit's.
pub async fn store_recovery_kit_keys(
    conn: &mut SqliteConnection,
    keys: &RecoveryPublicKeys,
    now_ms: u64,
) -> SyncResult<()> {
    let created_at_ms = i64::try_from(now_ms)
        .map_err(|_| SyncError::InvalidRequest("the clock is out of range"))?;
    sqlx::query(
        "INSERT INTO sync_recovery_kit (id, recovery_admin_key, recovery_encryption_key, created_at_ms)
         VALUES (1, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET
             recovery_admin_key = excluded.recovery_admin_key,
             recovery_encryption_key = excluded.recovery_encryption_key,
             created_at_ms = excluded.created_at_ms",
    )
    .bind(keys.admin.as_bytes().as_slice())
    .bind(keys.encryption.as_bytes().as_slice())
    .bind(created_at_ms)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Writes the rows readers of `Local` tables require, keeping any the device already has.
pub(crate) async fn write_local_seeds(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
) -> SyncResult<()> {
    for seed in manifest.local_seeds() {
        sqlx::query(AssertSqlSafe(seed)).execute(&mut *conn).await?;
    }
    Ok(())
}

/// Prepares a snapshot copy for another installation in one transaction: `Local` tables are
/// emptied, keeping only their seeded rows at new-vault values, and every `Local` column is reset
/// to its materialization value, because device state belongs to the device that wrote it; local
/// drafts stay behind for the same reason, and the operation that publishes one later reaches the
/// copy as a new row; captures belong to the installation that made them, so the copy drops any
/// that could not be sealed; and the apply flag is clear.
///
/// The resets run with the apply flag set, so gated domain triggers and capture stay quiet.
pub async fn prepare_snapshot_copy(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
) -> SyncResult<()> {
    manifest.validate().map_err(SyncError::InvalidManifest)?;
    let mut tx = conn.begin().await?;
    sql::set_applying(&mut tx, true).await?;
    for table in manifest.local {
        sqlx::query(AssertSqlSafe(format!("DELETE FROM {}", sql::ident(table))))
            .execute(&mut *tx)
            .await?;
    }
    write_local_seeds(&mut tx, manifest).await?;
    for table in manifest.tables {
        if table.local_columns.is_empty() {
            continue;
        }
        let assignments = table
            .local_columns
            .iter()
            .map(|column| format!("{} = {}", sql::ident(column.name), column.materialize))
            .collect::<Vec<_>>()
            .join(", ");
        sqlx::query(AssertSqlSafe(format!(
            "UPDATE {} SET {assignments}",
            sql::ident(table.name)
        )))
        .execute(&mut *tx)
        .await?;
    }
    publication::delete_local_drafts(&mut tx, manifest).await?;
    sqlx::query("DELETE FROM sync_capture")
        .execute(&mut *tx)
        .await?;
    sql::set_applying(&mut tx, false).await?;
    tx.commit().await?;
    Ok(())
}

impl Engine {
    /// Runs every registered hook in full rebuild mode in one engine write, so capture stays
    /// quiet and a failure leaves the vault unchanged. Run on a closed vault whose derived
    /// state was never computed; what the hooks report is dropped, because nothing is
    /// listening yet.
    pub async fn rebuild_derived_state(&self, conn: &mut SqliteConnection) -> SyncResult<()> {
        let mut tx = conn.begin().await?;
        sql::begin_engine_write(&mut tx).await?;
        let mut notices = Notices::default();
        for table in self.manifest.tables {
            let Some(hook) = self.hooks.get(table.id) else {
                continue;
            };
            hook.rebuild_all(&mut tx, table, &mut notices)
                .await
                .map_err(|error| SyncError::Hook {
                    table: table.name,
                    error,
                })?;
        }
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(())
    }

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
    /// stored with the same hash are skipped, so a repeated import is harmless. A refused
    /// operation of a writer of `own_device_id` fails the import with
    /// [`SyncError::CarriedOwnRefused`], because no other device holds it; refusals of other
    /// writers are counted, and a refusal never drops the operations after it.
    pub async fn import_carried(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        vault_id: &str,
        own_device_id: &str,
        ops: &[Vec<u8>],
        now_ms: u64,
    ) -> SyncResult<CarryReport> {
        let mut report = CarryReport::default();
        if ops.is_empty() {
            return Ok(report);
        }
        self.init_space(conn, vault_id, now_ms).await?;
        let mut own = self.device_writers(conn, ctx, own_device_id).await?;
        own.extend(carried_writers_of(ops, own_device_id));
        for chunk in ops.chunks(IMPORT_CHUNK) {
            let mut rest = chunk;
            while !rest.is_empty() {
                let outcomes = self.store_many(conn, ctx, rest, now_ms).await?;
                if outcomes.is_empty() {
                    return Err(corrupt(
                        "carried operations were neither stored nor refused",
                    ));
                }
                for (bytes, outcome) in rest.iter().zip(&outcomes) {
                    report.record(bytes, outcome, &own)?;
                }
                rest = &rest[outcomes.len().min(rest.len())..];
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
    /// Operations of other devices the replacement refused, such as past a removal cutoff.
    /// Their writers' devices hold them.
    pub refused_foreign: usize,
    /// Operations applied.
    pub applied: usize,
    /// Local captures in the replacement stopped the apply.
    pub pending_captures: bool,
}

impl CarryReport {
    /// Counts the outcome of one carried operation, failing on a refused operation of a writer
    /// in `own`. Bytes that do not decode belong to no known writer and count as foreign.
    fn record(
        &mut self,
        bytes: &[u8],
        outcome: &StoreOutcome,
        own: &BTreeSet<WriterId>,
    ) -> SyncResult<()> {
        let StoreOutcome::Refused(refusal) = outcome else {
            self.stored += 1;
            return Ok(());
        };
        if let Ok(envelope) = Envelope::decode(bytes)
            && own.contains(&envelope.writer)
        {
            return Err(SyncError::CarriedOwnRefused {
                writer: envelope.writer,
                seq: envelope.seq,
                refusal: *refusal,
            });
        }
        self.refused_foreign += 1;
        Ok(())
    }
}

/// Writers whose carried genesis certifies them for `device_id`, so a writer this device
/// created after the copy was made counts as its own though the copy has no row for it.
fn carried_writers_of(ops: &[Vec<u8>], device_id: &str) -> BTreeSet<WriterId> {
    ops.iter()
        .filter_map(|bytes| {
            let envelope = Envelope::decode(bytes).ok()?;
            if envelope.seq != 1 {
                return None;
            }
            let (_, certificate) = envelope.genesis().ok()?;
            (certificate.certificate.device_id == device_id).then_some(envelope.writer)
        })
        .collect()
}

async fn space_exists(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<bool> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sync_spaces WHERE space_id = ?)")
            .bind(space.as_bytes().as_slice())
            .fetch_one(&mut *conn)
            .await?;
    Ok(exists)
}

/// The stored vector of the staged copy `staged` over the writers of `live`, the operations
/// carry-forward may skip. Each staged head is checked against the live log before it is
/// trusted:
///
/// - A writer the copy does not hold stays at zero, and writers `live` does not know are left
///   out, since `live` has nothing of theirs to carry.
/// - A head at or below the live head must hash as the live operation at that sequence. A
///   different hash on a writer of `own_device_id` is [`SyncError::StagedHead`], because the
///   copy would hide operations only this device holds; on another writer it stands, since a
///   fresh copy is how a forked lane recovers.
/// - A head past the live head must be a stored envelope of that sequence whose hash is the
///   head hash and that the writer key of the live log signed; otherwise it is
///   [`SyncError::StagedHead`].
pub async fn verified_replacement_vector(
    live: &mut SqliteConnection,
    staged: &mut SqliteConnection,
    space: SpaceId,
    own_device_id: &str,
) -> SyncResult<VersionVector> {
    let mut vector = VersionVector::default();
    if !space_exists(live, space).await? || !space_exists(staged, space).await? {
        return Ok(vector);
    }
    for row in log::writers(live, space).await? {
        let Some(claimed) = log::writer(staged, space, row.id).await? else {
            continue;
        };
        let seq = claimed.stored;
        let confirmed = if seq <= row.stored {
            let own = row.device_id == own_device_id;
            !own || stored_hash(live, space, row.id, seq).await? == Some(claimed.head_hash)
        } else {
            signed_head(staged, space, &row.public_key, seq, &claimed.head_hash).await?
        };
        if !confirmed {
            return Err(SyncError::StagedHead {
                writer: row.id,
                seq,
            });
        }
        vector.set(row.id, seq);
    }
    Ok(vector)
}

/// The hash of the operation of `writer` stored at `seq`, kept when compaction drops its body.
async fn stored_hash(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<Option<Digest32>> {
    let hash: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT hash FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(log::seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    hash.as_deref().map(log::digest).transpose()
}

/// Whether `conn` stores at `seq` an operation of the writer with `key` whose hash is `head`
/// and that the key signed.
async fn signed_head(
    conn: &mut SqliteConnection,
    space: SpaceId,
    key: &WriterPublicKey,
    seq: Seq,
    head: &Digest32,
) -> SyncResult<bool> {
    let writer = WriterId::for_public_key(key);
    let envelope: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope FROM sync_ops WHERE space_id = ? AND writer_id = ? AND seq = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(log::seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(envelope.as_deref().is_some_and(|bytes| {
        Envelope::decode(bytes).is_ok_and(|envelope| {
            envelope.space == space
                && envelope.writer == writer
                && envelope.seq == seq
                && envelope.hash() == *head
                && envelope.verify(key)
        })
    }))
}

/// Whether the staged copy `staged` continues the authorization log of `live`. Only two
/// charters that differ answer no; a database without one yet continues any log.
pub async fn same_log(
    live: &mut SqliteConnection,
    staged: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<bool> {
    let live_charter = crate::authorization::load_charter(live, space).await?;
    let staged_charter = crate::authorization::load_charter(staged, space).await?;
    Ok(match (live_charter, staged_charter) {
        (Some(live), Some(staged)) => live == staged,
        _ => true,
    })
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
