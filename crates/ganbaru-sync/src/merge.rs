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
#[derive(Debug)]
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
}

impl Batch {
    pub(crate) async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Self> {
        Ok(Self {
            space,
            applied: log::applied_vector(conn, space).await?,
            contexts: ContextCache::default(),
            dirty: BTreeSet::new(),
            orphaned: BTreeSet::new(),
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

/// A version of a group read across its whole table, whose winners are never elided, from the
/// columns [`stored_from_row`] reads.
pub(crate) fn version_from_row(row: &SqliteRow, offset: usize) -> SyncResult<Version> {
    let stored = stored_from_row(row, offset)?;
    let value = stored
        .value
        .clone()
        .ok_or_else(|| corrupt("a group read across its table has an elided value"))?;
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
    let value = match stored.value {
        Some(value) => value,
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
    sqlx::query(
        "INSERT INTO sync_displaced_versions
            (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value,
             displaced_by_writer, displaced_by_seq)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(table_param(table.id))
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
    replace_version_refs(conn, table.id, key, stored.writer, &[(&group, spec)]).await
}

/// Records one change of an applied operation in the merge state. With `retain`, the versions
/// it supersedes are kept as displaced until the anchor confirms storing the operation.
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
    retain: Option<&Engine>,
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
        let existing = if created {
            Vec::new()
        } else {
            sqlx::query(
                "SELECT group_id, writer_id, seq, clock, value_hash,
                    CASE WHEN ? THEN value END
                 FROM sync_register_versions
                 WHERE table_id = ? AND row_key = ? AND group_id = ?",
            )
            .bind(retain.is_some())
            .bind(table_param(change.table))
            .bind(key)
            .bind(i64::from(group.group.get()))
            .fetch_all(&mut *conn)
            .await?
        };
        let mut superseded = false;
        for row in existing {
            let other = stored_from_row(&row, 0)?;
            if context.covers(&other.writer, other.seq) {
                if let Some(engine) = retain {
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
                || (other.clock > clock
                    && batch
                        .contexts
                        .get(conn, batch.space, other.writer, other.seq)
                        .await?
                        .covers(&writer, seq))
            {
                // An operation applied again after an exclusion was lifted never replaces a
                // version that saw it or a later version of its own writer.
                superseded = true;
            }
        }
        if !superseded {
            upserts.push((group, spec));
        }
    }
    upsert_versions(conn, change, writer, seq, clock, &upserts).await?;
    batch.dirty.insert((change.table, key.to_string()));
    Ok(())
}

/// Records the versions a change writes in one statement, then replaces their reference
/// targets.
async fn upsert_versions(
    conn: &mut SqliteConnection,
    change: &Change,
    writer: WriterId,
    seq: Seq,
    clock: Hlc,
    upserts: &[(&GroupValue, &GroupSpec)],
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
    replace_version_refs(conn, change.table, change.row.as_str(), writer, upserts).await
}

/// Replaces the reference targets of the versions a change wrote: one row per reference column
/// holding a row key, and one per distinct key the owned rows of a blob field reference.
async fn replace_version_refs(
    conn: &mut SqliteConnection,
    table: TableId,
    row: &str,
    writer: WriterId,
    upserts: &[(&GroupValue, &GroupSpec)],
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
        roots.extend(recovery::flagged_rows(conn).await?);
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
