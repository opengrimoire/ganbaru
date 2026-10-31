//! Elided version values.
//!
//! A winning version whose value the materialized domain row holds may keep a null value in
//! `sync_register_versions`, so a vault does not store every value twice. A reader takes such a
//! value from the domain row when that group's encoding hashes to the stored `value_hash`, and
//! otherwise from the body of the operation that wrote the version. Compaction stores an elided
//! value back before it drops that body, and a version whose operation was compacted is never
//! elided, so every elided version's operation still has its body. The hash decides alone: a
//! local edit not yet sealed, or a reference the merge resolved differently, only sends the
//! read to the operation body. A value found in neither place is corrupt state. Before
//! materialize changes or deletes a domain row, it stores the values elided from it again, so
//! tombstoned and hidden rows keep their versions whole.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use ganbaru_sync_contracts::{Content, Digest32, Envelope, GroupId, Seq, TableId, Value, WriterId};
use sqlx::{Row, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::log::{self, seq_i64, stored_u64};
use crate::manifest::TableSpec;
use crate::merge::{Stored, Version, group_id, table_param};
use crate::seal;

/// Decoded operation bodies a vault holds at most, enough for the few operations the versions
/// of neighboring rows come from.
pub(crate) const CACHED_BODIES: usize = 4;

/// Written values of one operation by table, row, and group.
pub(crate) type Body = BTreeMap<(TableId, String, GroupId), Value>;

/// The operation bodies read last, so that hydrating many rows of one operation decodes it
/// once. A hit still has to hash to the stored `value_hash`, so a rolled back transaction, a
/// compaction, or a reset space can only cause a miss.
#[derive(Debug, Default)]
pub(crate) struct BodyCache(Mutex<VecDeque<(WriterId, Seq, Arc<Body>)>>);

impl BodyCache {
    pub(crate) fn lookup(&self, writer: WriterId, seq: Seq) -> Option<Arc<Body>> {
        let bodies = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        bodies
            .iter()
            .find(|(cached, at, _)| *cached == writer && *at == seq)
            .map(|(_, _, body)| Arc::clone(body))
    }

    pub(crate) fn store(&self, writer: WriterId, seq: Seq, body: Arc<Body>) {
        let mut bodies = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        bodies.retain(|(cached, at, _)| *cached != writer || *at != seq);
        if bodies.len() == CACHED_BODIES {
            bodies.pop_front();
        }
        bodies.push_back((writer, seq, body));
    }
}

/// The value `body` holds for a group of a row, when it hashes to `hash`.
pub(crate) fn body_value(
    body: &Body,
    table: TableId,
    key: &str,
    group: GroupId,
    hash: &Digest32,
) -> Option<Value> {
    body.get(&(table, key.to_string(), group))
        .filter(|value| value.hash(table, group) == *hash)
        .cloned()
}

/// Versions of one row with every elided value read back, in the order given.
pub(crate) async fn row(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    stored: Vec<Stored>,
) -> SyncResult<Vec<Version>> {
    let mut versions = Vec::with_capacity(stored.len());
    for stored in stored {
        let version = match stored.value.clone() {
            Some(value) => stored.into_version(value, false),
            None => {
                let value = elided_value(
                    engine,
                    conn,
                    table,
                    key,
                    stored.group,
                    stored.writer,
                    stored.seq,
                    &stored.hash,
                )
                .await?;
                stored.into_version(value, true)
            }
        };
        versions.push(version);
    }
    Ok(versions)
}

/// The value of one elided version: the domain row's group value when it hashes to `hash`,
/// otherwise the group value of the operation `(writer, seq)` for this row.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn elided_value(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    group: GroupId,
    writer: WriterId,
    seq: Seq,
    hash: &Digest32,
) -> SyncResult<Value> {
    let spec = table
        .group(group)
        .ok_or_else(|| corrupt("stored version of an unknown group"))?;
    if let Some(value) = seal::read_group_value(engine, conn, table, spec, key).await?
        && value.hash(table.id, group) == *hash
    {
        return Ok(value);
    }
    if let Some(value) = engine
        .bodies
        .lookup(writer, seq)
        .and_then(|body| body_value(&body, table.id, key, group, hash))
    {
        return Ok(value);
    }
    let envelope: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope FROM sync_ops
         WHERE space_id IN (SELECT space_id FROM sync_spaces) AND writer_id = ? AND seq = ?",
    )
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    let envelope = envelope.ok_or_else(|| corrupt("elided value has no operation"))?;
    if envelope.is_empty() {
        return Err(corrupt("the operation of an elided value was compacted"));
    }
    let operation = Envelope::decode(&envelope)
        .ok()
        .and_then(|envelope| envelope.operation().ok())
        .ok_or_else(|| corrupt("an applied operation does not decode"))?;
    let Content::Changes(changes) = operation.content else {
        return Err(corrupt("elided value names an operation without changes"));
    };
    let mut body = Body::new();
    for change in changes {
        for value in change.groups {
            body.entry((change.table, change.row.as_str().to_string(), value.group))
                .or_insert(value.value);
        }
    }
    let value = body_value(&body, table.id, key, group, hash);
    engine.bodies.store(writer, seq, Arc::new(body));
    value.ok_or_else(|| corrupt("elided value cannot be hydrated"))
}

/// Stores again the elided values of a row's versions in `groups`, or in every group when
/// `groups` is `None`, before materialize changes or deletes the domain row holding them.
pub(crate) async fn persist(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    groups: Option<&BTreeSet<GroupId>>,
) -> SyncResult<()> {
    let rows = sqlx::query(
        "SELECT group_id, writer_id, seq, value_hash FROM sync_register_versions
         WHERE table_id = ? AND row_key = ? AND value IS NULL",
    )
    .bind(table_param(table.id))
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    for row in &rows {
        let group = group_id(row.try_get(0)?)?;
        if groups.is_some_and(|groups| !groups.contains(&group)) {
            continue;
        }
        let writer_bytes: Vec<u8> = row.try_get(1)?;
        let writer = log::writer_id(&writer_bytes)?;
        let seq = stored_u64(row.try_get(2)?, "version sequence")?;
        let hash = log::digest(&row.try_get::<Vec<u8>, _>(3)?)?;
        let value = elided_value(engine, conn, table, key, group, writer, seq, &hash).await?;
        sqlx::query(
            "UPDATE sync_register_versions SET value = ?
             WHERE table_id = ? AND row_key = ? AND group_id = ? AND writer_id = ?",
        )
        .bind(value.encode())
        .bind(table_param(table.id))
        .bind(key)
        .bind(i64::from(group.get()))
        .bind(writer_bytes.as_slice())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// A winning version whose value the domain row holds after a materialization.
pub(crate) type Elision = (TableId, String, GroupId, WriterId);

/// Most versions one elision statement nulls, four parameters each.
const ELISIONS_PER_STATEMENT: usize = 200;

/// Nulls the stored value of each version in `elisions` whose operation was not compacted.
pub(crate) async fn elide(conn: &mut SqliteConnection, elisions: &[Elision]) -> SyncResult<()> {
    for chunk in elisions.chunks(ELISIONS_PER_STATEMENT) {
        let rows = vec!["(?, ?, ?, ?)"; chunk.len()].join(", ");
        let sql = format!(
            "UPDATE sync_register_versions SET value = NULL
             WHERE value IS NOT NULL
                AND (table_id, row_key, group_id, writer_id) IN (VALUES {rows})
                AND NOT EXISTS (
                    SELECT 1 FROM sync_writers w
                    WHERE w.writer_id = sync_register_versions.writer_id
                        AND w.compacted_seq >= sync_register_versions.seq
                )"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
        for (table, key, group, writer) in chunk {
            query = query
                .bind(table_param(*table))
                .bind(key.as_str())
                .bind(i64::from(group.get()))
                .bind(writer.as_bytes().as_slice());
        }
        query.execute(&mut *conn).await?;
    }
    Ok(())
}
