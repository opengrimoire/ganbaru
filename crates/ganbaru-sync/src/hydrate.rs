//! Elided version values.
//!
//! A winning version whose value the materialized domain row holds may keep a null value in
//! `sync_register_versions`, so a vault does not store every value twice. A reader takes such a
//! value from the domain row when that group's encoding hashes to the stored `value_hash`, and
//! otherwise from the body of the operation that wrote the version, which every retained version
//! still has. The hash decides alone: a local edit not yet sealed, or a reference the merge
//! resolved differently, only sends the read to the operation body. A value found in neither
//! place is corrupt state. Before materialize changes or deletes a domain row, it stores the
//! values elided from it again, so tombstoned and hidden rows keep their versions whole.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::{Content, Digest32, Envelope, GroupId, Seq, TableId, Value, WriterId};
use sqlx::{Row, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::log::{self, seq_i64, stored_u64};
use crate::manifest::TableSpec;
use crate::merge::{Stored, Version, group_id, table_param};
use crate::seal;

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
    let envelope: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT envelope FROM sync_ops
         WHERE space_id IN (SELECT space_id FROM sync_spaces) AND writer_id = ? AND seq = ?",
    )
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .fetch_optional(&mut *conn)
    .await?;
    let envelope = envelope.ok_or_else(|| corrupt("elided value has no operation"))?;
    let operation = Envelope::decode(&envelope)
        .ok()
        .and_then(|envelope| envelope.operation().ok())
        .ok_or_else(|| corrupt("an applied operation does not decode"))?;
    let Content::Changes(changes) = operation.content else {
        return Err(corrupt("elided value names an operation without changes"));
    };
    changes
        .into_iter()
        .find(|change| change.table == table.id && change.row.as_str() == key)
        .and_then(|change| change.groups.into_iter().find(|value| value.group == group))
        .map(|value| value.value)
        .filter(|value| value.hash(table.id, group) == *hash)
        .ok_or_else(|| corrupt("elided value cannot be hydrated"))
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

/// Nulls the stored value of each version in `elisions`.
pub(crate) async fn elide(conn: &mut SqliteConnection, elisions: &[Elision]) -> SyncResult<()> {
    for chunk in elisions.chunks(ELISIONS_PER_STATEMENT) {
        let rows = vec!["(?, ?, ?, ?)"; chunk.len()].join(", ");
        let sql = format!(
            "UPDATE sync_register_versions SET value = NULL
             WHERE value IS NOT NULL
                AND (table_id, row_key, group_id, writer_id) IN (VALUES {rows})"
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
