//! Per-row merge state: register versions, tombstones, winners, and row flags.
//!
//! Every version in `sync_register_versions` comes from an applied operation. A new version
//! removes the versions its operation's causal context covers, so the remaining versions of a
//! group are mutually concurrent and the winner is a pure function of them.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{
    Change, ChangeAction, Digest32, Field, GroupId, GroupMask, GroupValue, Hlc, RevokeReason, Seq,
    SpaceId, TableId, Value, VersionVector, WriterId,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::log::{self, ContextCache, seq_i64, stored_u64};
use crate::manifest::{GroupSpec, MergeKind, TableSpec};

/// Merge state of a published row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowState {
    /// The row exists on every replica that applied its create.
    Live,
    /// A tombstone applied; versions are retained for recovery.
    Tombstoned,
}

/// One retained version of a group.
#[derive(Debug, Clone)]
pub(crate) struct Version {
    pub(crate) group: GroupId,
    pub(crate) writer: WriterId,
    pub(crate) seq: Seq,
    pub(crate) clock: Hlc,
    pub(crate) hash: Digest32,
    pub(crate) value: Value,
}

/// Mutable state of one engine transaction.
#[derive(Debug)]
pub(crate) struct Batch {
    pub(crate) space: SpaceId,
    /// Applied sequence per writer. `sync_writers.applied_seq` matches it except while an
    /// apply loop defers those writes.
    pub(crate) applied: VersionVector,
    pub(crate) contexts: ContextCache,
    /// Rows whose merge state changed and must be refreshed and materialized.
    pub(crate) dirty: BTreeSet<(TableId, String)>,
}

impl Batch {
    pub(crate) async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Self> {
        Ok(Self {
            space,
            applied: log::applied_vector(conn, space).await?,
            contexts: ContextCache::default(),
            dirty: BTreeSet::new(),
        })
    }
}

pub(crate) fn table_param(table: TableId) -> i64 {
    i64::from(table.0)
}

pub(crate) fn mask_param(mask: GroupMask) -> i64 {
    // SQLite integers are signed; bit 63 stores as a negative value.
    mask.0 as i64
}

pub(crate) fn group_id(value: i64) -> SyncResult<GroupId> {
    u8::try_from(value)
        .ok()
        .and_then(GroupId::new)
        .ok_or_else(|| corrupt("group id out of range"))
}

pub(crate) async fn row_state(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<Option<RowState>> {
    let state: Option<String> =
        sqlx::query_scalar("SELECT state FROM sync_rows WHERE table_id = ? AND row_key = ?")
            .bind(table_param(table))
            .bind(key)
            .fetch_optional(&mut *conn)
            .await?;
    parse_row_state(state.as_deref())
}

/// Row state from a stored `sync_rows.state`, where `None` means the row has no sync state.
pub(crate) fn parse_row_state(state: Option<&str>) -> SyncResult<Option<RowState>> {
    match state {
        None => Ok(None),
        Some("live") => Ok(Some(RowState::Live)),
        Some("tombstoned") => Ok(Some(RowState::Tombstoned)),
        Some(other) => Err(corrupt(format!("unknown row state {other}"))),
    }
}

/// Retained versions of a row, ordered by group and writer.
pub(crate) async fn versions(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<Vec<Version>> {
    let rows = sqlx::query(
        "SELECT group_id, writer_id, seq, clock, value_hash, value FROM sync_register_versions
         WHERE table_id = ? AND row_key = ? ORDER BY group_id, writer_id",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter().map(|row| version_from_row(row, 0)).collect()
}

/// A version from the columns `group_id, writer_id, seq, clock, value_hash, value` at `offset`.
pub(crate) fn version_from_row(row: &SqliteRow, offset: usize) -> SyncResult<Version> {
    let writer: Vec<u8> = row.try_get(offset + 1)?;
    let hash: Vec<u8> = row.try_get(offset + 4)?;
    let value: Vec<u8> = row.try_get(offset + 5)?;
    Ok(Version {
        group: group_id(row.try_get(offset)?)?,
        writer: log::writer_id(&writer)?,
        seq: stored_u64(row.try_get(offset + 2)?, "version sequence")?,
        clock: Hlc::from_u64(stored_u64(row.try_get(offset + 3)?, "version clock")?),
        hash: log::digest(&hash)?,
        value: Value::decode(&value).map_err(|_| corrupt("stored value does not decode"))?,
    })
}

fn field_rank(field: &Field) -> u8 {
    match field {
        Field::Null => 0,
        Field::Integer(_) => 1,
        Field::Text(_) => 2,
        Field::Blob(_) => 3,
    }
}

/// Total order of fields: by storage class, then integers by value and text and blobs by bytes.
pub(crate) fn compare_fields(left: &Field, right: &Field) -> Ordering {
    match (left, right) {
        (Field::Integer(left), Field::Integer(right)) => left.cmp(right),
        (Field::Text(left), Field::Text(right)) => left.as_bytes().cmp(right.as_bytes()),
        (Field::Blob(left), Field::Blob(right)) => left.cmp(right),
        _ => field_rank(left).cmp(&field_rank(right)),
    }
}

fn compare_values(left: &Value, right: &Value) -> Ordering {
    let (left, right) = (left.fields(), right.fields());
    left.iter()
        .zip(right)
        .map(|(left, right)| compare_fields(left, right))
        .find(|ordering| ordering.is_ne())
        .unwrap_or_else(|| left.len().cmp(&right.len()))
}

fn by_clock(left: &Version, right: &Version) -> Ordering {
    (left.clock, left.writer).cmp(&(right.clock, right.writer))
}

/// The winning version among concurrent versions of one group.
pub(crate) fn winner<'v>(
    engine: &Engine,
    table: &TableSpec,
    group: &GroupSpec,
    candidates: impl Iterator<Item = &'v Version>,
) -> Option<&'v Version> {
    match group.kind {
        MergeKind::Register | MergeKind::Position | MergeKind::Reference { .. } => {
            candidates.max_by(|left, right| by_clock(left, right))
        }
        MergeKind::Coupled => {
            let priority = |version: &Version| {
                engine.adapter(table).map_or(0, |adapter| {
                    adapter.priority(table.id, group.id, &version.value)
                })
            };
            candidates.max_by(|left, right| {
                priority(left)
                    .cmp(&priority(right))
                    .then_with(|| by_clock(left, right))
            })
        }
        MergeKind::Max => candidates.max_by(|left, right| {
            compare_values(&left.value, &right.value).then_with(|| by_clock(left, right))
        }),
        MergeKind::Immutable => candidates.min_by(|left, right| by_clock(left, right)),
    }
}

/// Winning version per group of a row; groups without versions are absent.
pub(crate) fn winners<'v>(
    engine: &Engine,
    table: &TableSpec,
    versions: &'v [Version],
) -> BTreeMap<GroupId, &'v Version> {
    table
        .groups
        .iter()
        .filter_map(|group| {
            winner(
                engine,
                table,
                group,
                versions.iter().filter(|version| version.group == group.id),
            )
            .map(|version| (group.id, version))
        })
        .collect()
}

/// Records one change of an applied operation in the merge state.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_change(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    writer: WriterId,
    seq: Seq,
    clock: Hlc,
    context: &VersionVector,
    table: &TableSpec,
    change: &Change,
) -> SyncResult<()> {
    let key = change.row.as_str();
    // Register versions reference their row, so a row created here has none to supersede.
    let created = if change.action == ChangeAction::Create {
        sqlx::query(
            "INSERT INTO sync_rows (table_id, row_key, state) VALUES (?, ?, 'live')
             ON CONFLICT DO NOTHING",
        )
        .bind(table_param(change.table))
        .bind(key)
        .execute(&mut *conn)
        .await?
        .rows_affected()
            == 1
    } else if row_state(conn, change.table, key).await?.is_some() {
        false
    } else {
        return Err(corrupt("change for an unpublished row reached apply"));
    };
    if change.action == ChangeAction::Tombstone {
        sqlx::query(
            "INSERT INTO sync_tombstones (table_id, row_key, writer_id, seq, replaced_by)
             VALUES (?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(table_param(change.table))
        .bind(key)
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(seq)?)
        .bind(change.replaced_by.as_ref().map(|target| target.as_str()))
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE sync_rows SET state = 'tombstoned', conflict_mask = 0
             WHERE table_id = ? AND row_key = ?",
        )
        .bind(table_param(change.table))
        .bind(key)
        .execute(&mut *conn)
        .await?;
    }
    let mut upserts = Vec::with_capacity(change.groups.len());
    for group in &change.groups {
        let spec = table
            .group(group.group)
            .ok_or_else(|| corrupt("change names an unknown group"))?;
        let existing = if created {
            Vec::new()
        } else {
            sqlx::query(
                "SELECT writer_id, seq FROM sync_register_versions
                 WHERE table_id = ? AND row_key = ? AND group_id = ?",
            )
            .bind(table_param(change.table))
            .bind(key)
            .bind(i64::from(group.group.get()))
            .fetch_all(&mut *conn)
            .await?
        };
        for row in existing {
            let other: Vec<u8> = row.try_get(0)?;
            let other_seq = stored_u64(row.try_get(1)?, "version sequence")?;
            if context.covers(&log::writer_id(&other)?, other_seq) {
                sqlx::query(
                    "DELETE FROM sync_register_versions
                     WHERE table_id = ? AND row_key = ? AND group_id = ? AND writer_id = ?",
                )
                .bind(table_param(change.table))
                .bind(key)
                .bind(i64::from(group.group.get()))
                .bind(other.as_slice())
                .execute(&mut *conn)
                .await?;
            }
        }
        let reference = match (spec.kind, group.value.fields()) {
            (MergeKind::Reference { .. }, [Field::Text(target)]) => Some(target.as_str()),
            _ => None,
        };
        upserts.push((group, reference));
    }
    upsert_versions(conn, change, writer, seq, clock, &upserts).await?;
    batch.dirty.insert((change.table, key.to_string()));
    Ok(())
}

/// Records the versions a change writes in one statement, with their reference targets.
async fn upsert_versions(
    conn: &mut SqliteConnection,
    change: &Change,
    writer: WriterId,
    seq: Seq,
    clock: Hlc,
    upserts: &[(&GroupValue, Option<&str>)],
) -> SyncResult<()> {
    if upserts.is_empty() {
        return Ok(());
    }
    let rows = vec!["(?, ?, ?, ?, ?, ?, ?, ?, ?)"; upserts.len()].join(", ");
    // A change carries at most one value per group, so the distinct statements stay bounded
    // by the largest group count of the manifest.
    let sql = format!(
        "INSERT INTO sync_register_versions
            (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value, ref_key)
         VALUES {rows}
         ON CONFLICT (table_id, row_key, group_id, writer_id) DO UPDATE SET
            seq = excluded.seq, clock = excluded.clock, value_hash = excluded.value_hash,
            value = excluded.value, ref_key = excluded.ref_key"
    );
    let seq = seq_i64(seq)?;
    let clock = log::clamp_ms(clock.as_u64());
    let mut query = sqlx::query(&sql);
    for (group, reference) in upserts {
        query = query
            .bind(table_param(change.table))
            .bind(change.row.as_str())
            .bind(i64::from(group.group.get()))
            .bind(writer.as_bytes().as_slice())
            .bind(seq)
            .bind(clock)
            .bind(group.hash.as_slice())
            .bind(group.value.encode())
            .bind(*reference);
    }
    query.execute(&mut *conn).await?;
    Ok(())
}

/// Merge state and retained versions of published rows, keyed by table and row.
pub(crate) type MergedRows = BTreeMap<(TableId, String), (RowState, Vec<Version>)>;

/// Recomputes the conflict mask of live rows and the recovery flag of tombstoned rows.
/// Returns the merge state it read for each dirty published row.
pub(crate) async fn refresh_flags(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
) -> SyncResult<MergedRows> {
    let dirty: Vec<(TableId, String)> = batch.dirty.iter().cloned().collect();
    let mut merged = MergedRows::new();
    for (table_id, key) in dirty {
        let Some(table) = engine.manifest().table(table_id) else {
            return Err(corrupt("dirty row of an unknown table"));
        };
        // One read of the row state and its versions; a row without versions yields one row of
        // nulls after its state.
        let rows = sqlx::query(
            "SELECT r.state, r.conflict_mask, r.recovery,
                v.group_id, v.writer_id, v.seq, v.clock, v.value_hash, v.value
             FROM sync_rows r
             LEFT JOIN sync_register_versions v
                ON v.table_id = r.table_id AND v.row_key = r.row_key
             WHERE r.table_id = ? AND r.row_key = ?
             ORDER BY v.group_id, v.writer_id",
        )
        .bind(table_param(table_id))
        .bind(&key)
        .fetch_all(&mut *conn)
        .await?;
        let Some(stored) = rows.first() else {
            continue;
        };
        let Some(state) = parse_row_state(Some(stored.try_get(0)?))? else {
            continue;
        };
        let stored_flags = (
            stored.try_get::<i64, _>(1)?,
            stored.try_get::<i64, _>(2)? != 0,
        );
        let versions = rows
            .iter()
            .filter_map(|row| match row.try_get::<Option<i64>, _>(3) {
                Ok(Some(_)) => Some(version_from_row(row, 3)),
                Ok(None) => None,
                Err(error) => Some(Err(error.into())),
            })
            .collect::<SyncResult<Vec<_>>>()?;
        let (conflict_mask, recovery) = match state {
            RowState::Live => (conflict_mask(table, &versions), false),
            RowState::Tombstoned => {
                let covering = tombstone_contexts(conn, batch, table_id, &key).await?;
                let recovery = versions.iter().any(|version| {
                    table
                        .group(version.group)
                        .is_some_and(|group| group.recoverable)
                        && !covering
                            .iter()
                            .any(|context| context.covers(&version.writer, version.seq))
                });
                (GroupMask::EMPTY, recovery)
            }
        };
        if stored_flags != (mask_param(conflict_mask), recovery) {
            sqlx::query(
                "UPDATE sync_rows SET conflict_mask = ?, recovery = ? WHERE table_id = ? AND row_key = ?",
            )
            .bind(mask_param(conflict_mask))
            .bind(i64::from(recovery))
            .bind(table_param(table_id))
            .bind(&key)
            .execute(&mut *conn)
            .await?;
        }
        merged.insert((table_id, key), (state, versions));
    }
    Ok(merged)
}

/// Surfaced groups whose retained versions hold two or more distinct values.
pub(crate) fn conflict_mask(table: &TableSpec, versions: &[Version]) -> GroupMask {
    let mut mask = GroupMask::EMPTY;
    for group in table.groups.iter().filter(|group| group.surfaced) {
        let hashes: BTreeSet<&Digest32> = versions
            .iter()
            .filter(|version| version.group == group.id)
            .map(|version| &version.hash)
            .collect();
        if hashes.len() > 1 {
            mask.insert(group.id);
        }
    }
    mask
}

/// Causal contexts of the operations that tombstoned a row.
pub(crate) async fn tombstone_contexts(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    table: TableId,
    key: &str,
) -> SyncResult<Vec<VersionVector>> {
    let rows = sqlx::query(
        "SELECT writer_id, seq FROM sync_tombstones WHERE table_id = ? AND row_key = ?",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    let mut contexts = Vec::with_capacity(rows.len());
    for row in rows {
        let writer: Vec<u8> = row.try_get(0)?;
        let writer = log::writer_id(&writer)?;
        let seq = stored_u64(row.try_get(1)?, "tombstone sequence")?;
        let mut context = batch
            .contexts
            .get(conn, batch.space, writer, seq)
            .await?
            .clone();
        // The tombstoning operation covers its own writer's versions up to itself.
        context.advance(writer, seq);
        contexts.push(context);
    }
    Ok(contexts)
}

/// Whether a version is retained by a tombstoned row and covered by none of its tombstones.
pub(crate) fn uncovered(version: &Version, covering: &[VersionVector]) -> bool {
    !covering
        .iter()
        .any(|context| context.covers(&version.writer, version.seq))
}

/// Records a revocation of `target`: its cutoff only lowers, a fork marking is kept, and its
/// stored operations past the cutoff that are not applied are held as invalid. Returns how many
/// operations it held.
pub(crate) async fn revoke_effects(
    conn: &mut SqliteConnection,
    space: SpaceId,
    target: WriterId,
    cutoff: Seq,
    reason: RevokeReason,
) -> SyncResult<u64> {
    let cutoff = seq_i64(cutoff)?;
    let updated = sqlx::query(
        "UPDATE sync_writers SET
            cutoff_seq = CASE WHEN cutoff_seq IS NULL OR cutoff_seq > ? THEN ? ELSE cutoff_seq END,
            state = CASE WHEN state = 'forked' OR ? THEN 'forked' ELSE 'revoked' END
         WHERE space_id = ? AND writer_id = ?",
    )
    .bind(cutoff)
    .bind(cutoff)
    .bind(reason == RevokeReason::Forked)
    .bind(space.as_bytes().as_slice())
    .bind(target.as_bytes().as_slice())
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(corrupt("revocation of an unknown writer reached apply"));
    }
    let held = sqlx::query(
        "UPDATE sync_ops SET state = 'held', hold_reason = 'invalid'
         WHERE space_id = ? AND writer_id = ? AND state = 'waiting' AND seq > (
            SELECT cutoff_seq FROM sync_writers WHERE space_id = ? AND writer_id = ?
         )",
    )
    .bind(space.as_bytes().as_slice())
    .bind(target.as_bytes().as_slice())
    .bind(space.as_bytes().as_slice())
    .bind(target.as_bytes().as_slice())
    .execute(&mut *conn)
    .await?;
    Ok(held.rows_affected())
}
