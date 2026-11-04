//! Per-row merge state: register versions, tombstones, winners, and row flags.
//!
//! Every version in `sync_register_versions` comes from an applied operation. A new version
//! removes the versions its operation's causal context covers, so the remaining versions of a
//! group are mutually concurrent and the winner is a pure function of them.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::{
    Change, ChangeAction, Digest32, Field, GroupId, GroupMask, GroupValue, Hlc, Seq, SpaceId,
    TableId, Value, VersionVector, WriterId,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::Engine;
use crate::codec;
use crate::error::{SyncResult, corrupt};
use crate::exclusion;
use crate::hydrate;
use crate::log::{self, ContextCache, seq_i64, stored_u64};
use crate::manifest::{GroupSpec, MergeKind, TableSpec};
use crate::materialize;
use crate::recovery;
use crate::visibility::Visibility;

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
    /// Whether the stored value is elided and `value` was read back, see [`crate::hydrate`].
    pub(crate) elided: bool,
}

/// One retained version as stored, without a value when it is elided.
#[derive(Debug, Clone)]
pub(crate) struct Stored {
    pub(crate) group: GroupId,
    pub(crate) writer: WriterId,
    pub(crate) seq: Seq,
    pub(crate) clock: Hlc,
    pub(crate) hash: Digest32,
    pub(crate) value: Option<Value>,
}

impl Stored {
    /// The version with its value.
    pub(crate) fn into_version(self, value: Value, elided: bool) -> Version {
        Version {
            group: self.group,
            writer: self.writer,
            seq: self.seq,
            clock: self.clock,
            hash: self.hash,
            value,
            elided,
        }
    }
}

/// Mutable state of one engine transaction.
#[derive(Debug, Clone)]
pub(crate) struct Batch {
    pub(crate) space: SpaceId,
    /// Applied sequence per writer. `sync_writers.applied_seq` matches it except while an
    /// apply loop defers those writes.
    pub(crate) applied: VersionVector,
    pub(crate) contexts: ContextCache,
    /// Rows whose merge state changed and must be refreshed and materialized.
    pub(crate) dirty: BTreeSet<(TableId, String)>,
    /// Dirty rows whose merge state an exclusion dropped because no remaining operation wrote
    /// them. They materialize absent.
    pub(crate) orphaned: BTreeSet<(TableId, String)>,
    /// Groups the anchor seals again with their current values once the call materialized,
    /// because a fork or removal excluded stored operations that wrote them.
    pub(crate) republish: BTreeMap<(TableId, String), GroupMask>,
    /// Groups a former or current anchor seals again because an exclusion removes operations
    /// it applied as anchor, which released what they displaced. They are captured before the
    /// call materializes, while the domain rows still hold the values those operations wrote.
    pub(crate) released: BTreeMap<(TableId, String), GroupMask>,
}

impl Batch {
    pub(crate) async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Self> {
        Ok(Self {
            space,
            applied: log::applied_vector(conn, space).await?,
            contexts: ContextCache::default(),
            dirty: BTreeSet::new(),
            orphaned: BTreeSet::new(),
            republish: BTreeMap::new(),
            released: BTreeMap::new(),
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

/// Records that operation `seq` of `writer` created a row, which no longer waits for a
/// creation. A `fresh` row got its merge state from this creation, so it never waited.
async fn record_creation(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    writer: WriterId,
    seq: Seq,
    fresh: bool,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_row_creations (table_id, row_key, writer_id, seq) VALUES (?, ?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(table_param(table))
    .bind(key)
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .execute(&mut *conn)
    .await?;
    if fresh {
        return Ok(());
    }
    sqlx::query(
        "UPDATE sync_rows SET creation_pending = 0
         WHERE table_id = ? AND row_key = ? AND creation_pending = 1",
    )
    .bind(table_param(table))
    .bind(key)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Adds merge state for a row no operation this replica applied created, so changes from an
/// operation that saw it created on an excluded forked branch apply and stay hidden until an
/// operation creates the row.
pub(crate) async fn defer_creation(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_rows (table_id, row_key, state, creation_pending) VALUES (?, ?, 'live', 1)
         ON CONFLICT DO NOTHING",
    )
    .bind(table_param(table))
    .bind(key)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Marks the rows of `rows` that kept merge state but lost every creation as waiting for one.
pub(crate) async fn mark_uncreated(
    conn: &mut SqliteConnection,
    rows: &BTreeSet<(TableId, String)>,
) -> SyncResult<()> {
    for (table, key) in rows {
        sqlx::query(
            "UPDATE sync_rows SET creation_pending = 1
             WHERE table_id = ?1 AND row_key = ?2
                AND NOT EXISTS (
                    SELECT 1 FROM sync_row_creations c
                    WHERE c.table_id = ?1 AND c.row_key = ?2
                )",
        )
        .bind(table_param(*table))
        .bind(key)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Retained versions of a row with elided values read back, ordered by group and writer.
pub(crate) async fn versions(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
) -> SyncResult<Vec<Version>> {
    let spec = engine
        .manifest()
        .table(table)
        .ok_or_else(|| corrupt("versions of an unknown table"))?;
    let rows = sqlx::query(
        "SELECT group_id, writer_id, seq, clock, value_hash, value FROM sync_register_versions
         WHERE table_id = ? AND row_key = ? ORDER BY group_id, writer_id",
    )
    .bind(table_param(table))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    let stored = rows
        .iter()
        .map(|row| stored_from_row(row, 0))
        .collect::<SyncResult<Vec<_>>>()?;
    hydrate::row(engine, conn, spec, key, stored).await
}

/// A stored version from the columns `group_id, writer_id, seq, clock, value_hash, value` at
/// `offset`.
pub(crate) fn stored_from_row(row: &SqliteRow, offset: usize) -> SyncResult<Stored> {
    let writer: Vec<u8> = row.try_get(offset + 1)?;
    let hash: Vec<u8> = row.try_get(offset + 4)?;
    let value: Option<Vec<u8>> = row.try_get(offset + 5)?;
    Ok(Stored {
        group: group_id(row.try_get(offset)?)?,
        writer: log::writer_id(&writer)?,
        seq: stored_u64(row.try_get(offset + 2)?, "version sequence")?,
        clock: Hlc::from_u64(stored_u64(row.try_get(offset + 3)?, "version clock")?),
        hash: log::digest(&hash)?,
        value: value
            .map(|value| Value::decode(&value).map_err(|_| corrupt("stored value does not decode")))
            .transpose()?,
    })
}

/// A version of a group that is never elided, from the columns [`stored_from_row`] reads.
pub(crate) fn version_from_row(row: &SqliteRow, offset: usize) -> SyncResult<Version> {
    let stored = stored_from_row(row, offset)?;
    let value = stored
        .value
        .clone()
        .ok_or_else(|| corrupt("a group that is never elided has an elided value"))?;
    Ok(stored.into_version(value, false))
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

/// Keeps a version an operation supersedes, with its value read back, until the anchor
/// confirms storing the operation.
async fn displace(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    stored: Stored,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<()> {
    let value = match &stored.value {
        Some(value) => value.clone(),
        None => {
            hydrate::elided_value(
                engine,
                conn,
                table,
                key,
                stored.group,
                stored.writer,
                stored.seq,
                &stored.hash,
            )
            .await?
        }
    };
    insert_displaced(conn, table.id, key, &stored, &value, writer, seq).await
}

/// Records `stored` with `value` as displaced by the operation `(writer, seq)`.
async fn insert_displaced(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    stored: &Stored,
    value: &Value,
    writer: WriterId,
    seq: Seq,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_displaced_versions
            (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value,
             displaced_by_writer, displaced_by_seq)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(table_param(table))
    .bind(key)
    .bind(i64::from(stored.group.get()))
    .bind(stored.writer.as_bytes().as_slice())
    .bind(seq_i64(stored.seq)?)
    .bind(log::clamp_ms(stored.clock.as_u64()))
    .bind(stored.hash.as_slice())
    .bind(value.encode())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// One Create a group [`keeps_immutable`] keeps: a version of the group, or a Create held back
/// from it.
struct Kept {
    stored: Stored,
    held: bool,
}

/// The Creates the group `group` of `key` keeps. A held Create is one row of
/// `sync_displaced_versions` that names its own operation as the displacing one, which no
/// displaced version does, so anchor marks and removals of other writers never drop it.
async fn kept_creates(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    group: GroupId,
) -> SyncResult<Vec<Kept>> {
    let rows = sqlx::query(
        "SELECT group_id, writer_id, seq, clock, value_hash, value, 0
         FROM sync_register_versions WHERE table_id = ?1 AND row_key = ?2 AND group_id = ?3
         UNION ALL
         SELECT group_id, writer_id, seq, clock, value_hash, value, 1
         FROM sync_displaced_versions WHERE table_id = ?1 AND row_key = ?2 AND group_id = ?3
            AND displaced_by_writer = writer_id AND displaced_by_seq = seq",
    )
    .bind(table_param(table))
    .bind(key)
    .bind(i64::from(group.get()))
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(Kept {
                stored: stored_from_row(row, 0)?,
                held: row.try_get::<i64, _>(6)? == 1,
            })
        })
        .collect()
}

/// Holds `create` back from its group, keeping its value, hydrated when the version elided it.
async fn hold(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    create: &Stored,
) -> SyncResult<()> {
    let value = match &create.value {
        Some(value) => value.clone(),
        None => {
            hydrate::elided_value(
                engine,
                conn,
                table,
                key,
                create.group,
                create.writer,
                create.seq,
                &create.hash,
            )
            .await?
        }
    };
    insert_displaced(
        conn,
        table.id,
        key,
        create,
        &value,
        create.writer,
        create.seq,
    )
    .await
}

/// Drops `kept` from its group, as a version or as a held Create; the reference targets of a
/// version go with it.
async fn forget(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    kept: &Kept,
) -> SyncResult<()> {
    let statement = if kept.held {
        "DELETE FROM sync_displaced_versions
         WHERE table_id = ?1 AND row_key = ?2 AND group_id = ?3 AND writer_id = ?4
            AND displaced_by_writer = ?4 AND seq = ?5 AND displaced_by_seq = ?5"
    } else {
        "DELETE FROM sync_register_versions
         WHERE table_id = ?1 AND row_key = ?2 AND group_id = ?3 AND writer_id = ?4 AND seq = ?5"
    };
    sqlx::query(statement)
        .bind(table_param(table))
        .bind(key)
        .bind(i64::from(kept.stored.group.get()))
        .bind(kept.stored.writer.as_bytes().as_slice())
        .bind(seq_i64(kept.stored.seq)?)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Settles a Create of a group [`keeps_immutable`] leaves as it is against the Creates the
/// group keeps, and returns whether its version enters. Of each writer the group keeps only the
/// earliest Create. The Create enters unless it saw a kept Create, and is held back otherwise; a
/// version that saw it, which only an operation applied again after an exclusion was lifted can
/// meet, is held back in its place. The group thus keeps, whatever order its Creates arrive in,
/// those that saw no other kept Create, as [`settle_kept`] computes it.
async fn keep_create(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    table: &TableSpec,
    key: &str,
    create: &Stored,
    context: &VersionVector,
) -> SyncResult<bool> {
    let mut kept = kept_creates(conn, table.id, key, create.group).await?;
    if let Some(own) = kept
        .iter()
        .position(|other| other.stored.writer == create.writer)
    {
        if kept[own].stored.seq <= create.seq {
            return Ok(false);
        }
        let own = kept.swap_remove(own);
        forget(conn, table.id, key, &own).await?;
    }
    if kept
        .iter()
        .any(|other| context.covers(&other.stored.writer, other.stored.seq))
    {
        hold(engine, conn, table, key, create).await?;
        return Ok(false);
    }
    // Clocks are not checked against what an operation saw, so every version is asked; the
    // group keeps at most one per writer.
    for other in kept {
        if other.held {
            continue;
        }
        let saw = batch
            .contexts
            .get(conn, batch.space, other.stored.writer, other.stored.seq)
            .await?
            .covers(&create.writer, create.seq);
        if saw {
            hold(engine, conn, table, key, &other.stored).await?;
            forget(conn, table.id, key, &other).await?;
        }
    }
    Ok(true)
}

/// Settles every group [`keeps_immutable`] of the dirty rows that hold a Create back, after
/// removals or a fork dropped kept Creates: the group keeps as versions exactly the Creates that
/// saw no other Create it keeps, whichever writers were removed and in whatever order. A held
/// Create enters with its reference targets, and a version that now saw another is held back.
pub(crate) async fn settle_kept(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
) -> SyncResult<()> {
    let mut groups = Vec::new();
    for (table, key) in &batch.dirty {
        let rows: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT group_id FROM sync_displaced_versions
             WHERE table_id = ? AND row_key = ?
                AND displaced_by_writer = writer_id AND displaced_by_seq = seq",
        )
        .bind(table_param(*table))
        .bind(key)
        .fetch_all(&mut *conn)
        .await?;
        for group in rows {
            groups.push((*table, key.clone(), group_id(group)?));
        }
    }
    for (table, key, group) in groups {
        let spec = engine
            .manifest()
            .table(table)
            .ok_or_else(|| corrupt("held Create of an unknown table"))?;
        let kept = kept_creates(conn, table, &key, group).await?;
        let mut enters = Vec::with_capacity(kept.len());
        for entry in &kept {
            let context = batch
                .contexts
                .get(conn, batch.space, entry.stored.writer, entry.stored.seq)
                .await?;
            enters.push(!kept.iter().any(|other| {
                other.stored.writer != entry.stored.writer
                    && context.covers(&other.stored.writer, other.stored.seq)
            }));
        }
        for (entry, enters) in kept.iter().zip(enters) {
            if enters && entry.held {
                let value = entry
                    .stored
                    .value
                    .clone()
                    .ok_or_else(|| corrupt("a held Create has no value"))?;
                forget(conn, table, &key, entry).await?;
                restore_version(conn, spec, &key, &entry.stored, value).await?;
            } else if !enters && !entry.held {
                hold(engine, conn, spec, &key, &entry.stored).await?;
                forget(conn, table, &key, entry).await?;
            }
        }
    }
    Ok(())
}

/// Drops the Creates `writer` past `keep_through` holds back from a group and marks their rows
/// dirty, so [`settle_kept`] settles what they kept out.
pub(crate) async fn forget_held_past(
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    writer: WriterId,
    keep_through: Seq,
) -> SyncResult<()> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "DELETE FROM sync_displaced_versions
         WHERE writer_id = ?1 AND seq > ?2 AND displaced_by_writer = ?1 AND displaced_by_seq = seq
         RETURNING table_id, row_key",
    )
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(keep_through)?)
    .fetch_all(&mut *conn)
    .await?;
    for (table, key) in rows {
        let table = u16::try_from(table)
            .map(TableId)
            .map_err(|_| corrupt("held Create of a table id out of range"))?;
        batch.dirty.insert((table, key));
    }
    Ok(())
}

/// Puts back a version a removal exclusion restored, with its reference targets.
pub(crate) async fn restore_version(
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    stored: &Stored,
    value: Value,
) -> SyncResult<()> {
    let spec = table
        .group(stored.group)
        .ok_or_else(|| corrupt("restored version of an unknown group"))?;
    sqlx::query(
        "INSERT INTO sync_register_versions
            (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (table_id, row_key, group_id, writer_id) DO UPDATE SET
            seq = excluded.seq, clock = excluded.clock, value_hash = excluded.value_hash,
            value = excluded.value",
    )
    .bind(table_param(table.id))
    .bind(key)
    .bind(i64::from(stored.group.get()))
    .bind(stored.writer.as_bytes().as_slice())
    .bind(seq_i64(stored.seq)?)
    .bind(log::clamp_ms(stored.clock.as_u64()))
    .bind(stored.hash.as_slice())
    .bind(value.encode())
    .execute(&mut *conn)
    .await?;
    let group = GroupValue {
        group: stored.group,
        hash: stored.hash,
        value,
    };
    replace_version_refs(conn, table.id, key, stored.writer, &[(&group, spec)], false).await
}

/// Whether a Create leaves the group `spec` as it is when the row holds a version the Create saw
/// or one of its own writer's: it does not rewrite an Immutable group after that. A concurrent
/// version it did not see still competes for the Immutable minimum, and a derived-key row is
/// added again with every group. Nothing else displaces a version of such a group, so its
/// displaced rows are all Creates held back by [`keep_create`].
pub(crate) fn keeps_immutable(table: &TableSpec, spec: &GroupSpec) -> bool {
    spec.kind == MergeKind::Immutable && table.derived_key.is_none()
}

/// Records one change of an applied operation in the merge state. With `retain`, the versions
/// it supersedes are kept as displaced until the anchor confirms storing the operation.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_change(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    writer: WriterId,
    seq: Seq,
    clock: Hlc,
    context: &VersionVector,
    table: &TableSpec,
    change: &Change,
    retain: bool,
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
    } else if exclusion::created_late(conn, change.table, key).await? {
        // An honest operation that saw a late create of a removed device writes the row, which
        // stays hidden as a late change until an admin restores it.
        sqlx::query("INSERT INTO sync_rows (table_id, row_key, state) VALUES (?, ?, 'live')")
            .bind(table_param(change.table))
            .bind(key)
            .execute(&mut *conn)
            .await?;
        false
    } else {
        return Err(corrupt("change for an unpublished row reached apply"));
    };
    if change.action == ChangeAction::Create {
        record_creation(conn, change.table, key, writer, seq, created).await?;
    }
    if change.action == ChangeAction::Tombstone {
        sqlx::query(
            "INSERT INTO sync_tombstones
                (table_id, row_key, writer_id, seq, replaced_by, cause_table, cause_row)
             VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(table_param(change.table))
        .bind(key)
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(seq)?)
        .bind(change.replaced_by.as_ref().map(|target| target.as_str()))
        .bind(change.cause.as_ref().map(|cause| table_param(cause.table)))
        .bind(change.cause.as_ref().map(|cause| cause.row.as_str()))
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
        if change.action == ChangeAction::Create && !created && keeps_immutable(table, spec) {
            let create = Stored {
                group: group.group,
                writer,
                seq,
                clock,
                hash: group.hash,
                value: Some(group.value.clone()),
            };
            if keep_create(engine, conn, batch, table, key, &create, context).await? {
                upserts.push((group, spec));
            }
            continue;
        }
        let existing = if created {
            Vec::new()
        } else {
            sqlx::query(
                "SELECT group_id, writer_id, seq, clock, value_hash,
                    CASE WHEN ? THEN value END
                 FROM sync_register_versions
                 WHERE table_id = ? AND row_key = ? AND group_id = ?",
            )
            .bind(retain)
            .bind(table_param(change.table))
            .bind(key)
            .bind(i64::from(group.group.get()))
            .fetch_all(&mut *conn)
            .await?
        };
        let versions = existing
            .iter()
            .map(|row| stored_from_row(row, 0))
            .collect::<SyncResult<Vec<_>>>()?;
        let mut superseded = false;
        for other in versions {
            if context.covers(&other.writer, other.seq) {
                if retain {
                    displace(engine, conn, table, key, other.clone(), writer, seq).await?;
                }
                sqlx::query(
                    "DELETE FROM sync_register_versions
                     WHERE table_id = ? AND row_key = ? AND group_id = ? AND writer_id = ?",
                )
                .bind(table_param(change.table))
                .bind(key)
                .bind(i64::from(group.group.get()))
                .bind(other.writer.as_bytes().as_slice())
                .execute(&mut *conn)
                .await?;
            } else if (other.writer == writer && other.seq > seq)
                || batch
                    .contexts
                    .get(conn, batch.space, other.writer, other.seq)
                    .await?
                    .covers(&writer, seq)
            {
                // An operation applied again after an exclusion was lifted never replaces a
                // version that saw it or a later version of its own writer. A version that saw
                // it can carry the lower clock, since a clock far ahead is adopted only in part.
                superseded = true;
            }
        }
        if !superseded {
            upserts.push((group, spec));
        }
    }
    upsert_versions(conn, change, writer, seq, clock, &upserts, created).await?;
    batch.dirty.insert((change.table, key.to_string()));
    Ok(())
}

/// Records the versions a change writes in one statement, then replaces their reference
/// targets. A `fresh` row got its merge state from this change.
async fn upsert_versions(
    conn: &mut SqliteConnection,
    change: &Change,
    writer: WriterId,
    seq: Seq,
    clock: Hlc,
    upserts: &[(&GroupValue, &GroupSpec)],
    fresh: bool,
) -> SyncResult<()> {
    if upserts.is_empty() {
        return Ok(());
    }
    let rows = vec!["(?, ?, ?, ?, ?, ?, ?, ?)"; upserts.len()].join(", ");
    // A change carries at most one value per group, so the distinct statements stay bounded
    // by the largest group count of the manifest.
    let sql = format!(
        "INSERT INTO sync_register_versions
            (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value)
         VALUES {rows}
         ON CONFLICT (table_id, row_key, group_id, writer_id) DO UPDATE SET
            seq = excluded.seq, clock = excluded.clock, value_hash = excluded.value_hash,
            value = excluded.value"
    );
    let seq = seq_i64(seq)?;
    let clock = log::clamp_ms(clock.as_u64());
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    for (group, _) in upserts {
        query = query
            .bind(table_param(change.table))
            .bind(change.row.as_str())
            .bind(i64::from(group.group.get()))
            .bind(writer.as_bytes().as_slice())
            .bind(seq)
            .bind(clock)
            .bind(group.hash.as_slice())
            .bind(group.value.encode());
    }
    query.execute(&mut *conn).await?;
    replace_version_refs(
        conn,
        change.table,
        change.row.as_str(),
        writer,
        upserts,
        fresh,
    )
    .await
}

/// Replaces the reference targets of the versions a change wrote: one row per reference column
/// holding a row key, and one per distinct key the owned rows of a blob field reference. A
/// `fresh` row got its merge state from this change, so it has no targets to replace.
async fn replace_version_refs(
    conn: &mut SqliteConnection,
    table: TableId,
    row: &str,
    writer: WriterId,
    upserts: &[(&GroupValue, &GroupSpec)],
    fresh: bool,
) -> SyncResult<()> {
    let referencing = upserts
        .iter()
        .filter(|(_, spec)| {
            spec.reference_columns().next().is_some() || spec.owned_references().next().is_some()
        })
        .collect::<Vec<_>>();
    if referencing.is_empty() {
        return Ok(());
    }
    if !fresh {
        let groups = vec!["?"; referencing.len()].join(", ");
        let sql = format!(
            "DELETE FROM sync_version_refs
             WHERE table_id = ? AND row_key = ? AND writer_id = ? AND group_id IN ({groups})"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(table_param(table))
            .bind(row)
            .bind(writer.as_bytes().as_slice());
        for (group, _) in &referencing {
            query = query.bind(i64::from(group.group.get()));
        }
        query.execute(&mut *conn).await?;
    }

    let mut targets = BTreeSet::new();
    for (group, spec) in &referencing {
        for (field, _) in spec.reference_columns() {
            if let Some(Field::Text(target)) = group.value.fields().get(field) {
                targets.insert((group.group, field, target.clone()));
            }
        }
        let owned = codec::owned_targets(spec, &group.value)
            .map_err(|error| corrupt(format!("stored owned value: {error}")))?;
        for (field, _, target) in owned {
            targets.insert((group.group, field, target));
        }
    }
    if targets.is_empty() {
        return Ok(());
    }
    let rows = vec!["(?, ?, ?, ?, ?, ?)"; targets.len()].join(", ");
    let sql = format!(
        "INSERT INTO sync_version_refs (table_id, row_key, group_id, writer_id, field, ref_key)
         VALUES {rows}"
    );
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    for (group, field, target) in targets {
        let field = i64::try_from(field).map_err(|_| corrupt("reference field out of range"))?;
        query = query
            .bind(table_param(table))
            .bind(row)
            .bind(i64::from(group.get()))
            .bind(writer.as_bytes().as_slice())
            .bind(field)
            .bind(target);
    }
    query.execute(&mut *conn).await?;
    Ok(())
}

/// Merge state and retained versions of published rows, keyed by table and row.
pub(crate) type MergedRows = BTreeMap<(TableId, String), (RowState, Vec<Version>)>;

/// Recomputes the conflict mask of live rows and the recovery flag of tombstoned rows,
/// including the deleted roots of dirty live rows hidden by cascade. A dirty live row missing
/// from its domain table may have left the group of the root it was hidden under, which is not
/// known after the change, so every flagged row is then recomputed as well. Returns the merge
/// state it read for each dirty published row.
pub(crate) async fn refresh_flags(
    engine: &Engine,
    conn: &mut SqliteConnection,
    batch: &mut Batch,
    visibility: &mut Visibility,
) -> SyncResult<MergedRows> {
    let dirty: Vec<(TableId, String)> = batch.dirty.iter().cloned().collect();
    let mut merged = MergedRows::new();
    let mut roots = BTreeSet::new();
    let mut recheck_flagged = false;
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
        let stored = rows
            .iter()
            .filter_map(|row| match row.try_get::<Option<i64>, _>(3) {
                Ok(Some(_)) => Some(stored_from_row(row, 3)),
                Ok(None) => None,
                Err(error) => Some(Err(error.into())),
            })
            .collect::<SyncResult<Vec<_>>>()?;
        let versions = hydrate::row(engine, conn, table, &key, stored).await?;
        let flags = match state {
            RowState::Live => {
                if let Some(root) =
                    recovery::flag_root(engine, conn, visibility, table_id, &key).await?
                {
                    roots.insert(root);
                }
                // The domain table still holds the state before this batch materializes.
                if !recheck_flagged && !materialize::domain_exists(conn, table, &key).await? {
                    recheck_flagged = true;
                }
                let mut mask = conflict_mask(table, &versions);
                // A severed edge surfaces as a conflict of its existence group.
                if let Some(exists) = table
                    .exists_group()
                    .filter(|_| visibility.is_severed(table_id, &key))
                {
                    mask.insert(exists.id);
                }
                (mask, false)
            }
            RowState::Tombstoned => (
                GroupMask::EMPTY,
                recovery::flagged(engine, conn, batch, visibility, table_id, &key).await?,
            ),
        };
        if stored_flags != (mask_param(flags.0), flags.1) {
            store_flags(conn, table_id, &key, flags).await?;
        }
        merged.insert((table_id, key), (state, versions));
    }
    if recheck_flagged {
        // A flag follows references, so a change cannot reach one of another component.
        let flagged = recovery::flagged_rows(conn).await?;
        roots.extend(
            flagged
                .into_iter()
                .filter(|(table, _)| visibility.in_scope(*table)),
        );
    }
    for (table_id, key) in roots {
        if merged.contains_key(&(table_id, key.clone())) {
            continue;
        }
        let recovery = recovery::flagged(engine, conn, batch, visibility, table_id, &key).await?;
        store_flags(conn, table_id, &key, (GroupMask::EMPTY, recovery)).await?;
    }
    Ok(merged)
}

/// Stores the conflict mask and recovery flag of a published row.
async fn store_flags(
    conn: &mut SqliteConnection,
    table: TableId,
    key: &str,
    (conflict_mask, recovery): (GroupMask, bool),
) -> SyncResult<()> {
    sqlx::query(
        "UPDATE sync_rows SET conflict_mask = ?, recovery = ? WHERE table_id = ? AND row_key = ?",
    )
    .bind(mask_param(conflict_mask))
    .bind(i64::from(recovery))
    .bind(table_param(table))
    .bind(key)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Whether an existence register value records a removal.
pub(crate) fn is_removal(value: Option<&Value>) -> bool {
    value.is_some_and(|value| value.fields() == [Field::Integer(0)])
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
