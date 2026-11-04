//! Publication: which domain rows replicate and which stay local drafts.
//!
//! A table with a `publish_when` predicate replicates a row once the row satisfies it, and a
//! `published_with` child replicates its rows once their parent row does, or at once when it
//! names no parent and its declaration lets null publish. The same condition
//! renders over a trigger row (`NEW` or `OLD`), over the rows of the table itself, and over
//! merged winners, so capture, seeding, and visibility agree on it.
//!
//! A local draft belongs to the device that wrote it and no other replica holds it, so a
//! replicated operation carrying its key is never the same row. Materialization leaves such a
//! draft and its children as they are and keeps the operation in merge state, so every replica
//! merges alike. A snapshot copy leaves drafts behind for the same reason.

use ganbaru_sync_contracts::Field;
use sqlx::{AssertSqlSafe, Row, SqliteConnection};

use crate::Engine;
use crate::error::{SyncResult, corrupt};
use crate::manifest::{GroupSpec, GroupStorage, Manifest, PublishedWith, TableSpec};
use crate::materialize;
use crate::merge;
use crate::sql;

/// Alias of the publishing parent row inside a rendered condition.
const PARENT_ALIAS: &str = "sync_publisher";
/// Alias of the domain row a stored-row query reads.
const ROW_ALIAS: &str = "sync_stored";

/// Columns a predicate may name: the key and every column of a column group.
fn predicate_columns(table: &TableSpec) -> impl Iterator<Item = &'static str> {
    std::iter::once(table.key_column).chain(
        table
            .groups
            .iter()
            .flat_map(|group| group.columns().iter().map(|column| column.name)),
    )
}

/// The table's own predicate over the trigger row `row`, evaluated on a projection of that
/// row so the predicate names plain columns.
pub(crate) fn own_over(table: &TableSpec, row: &str) -> Option<String> {
    let predicate = table.publish_when?;
    let projection = predicate_columns(table)
        .map(|column| format!("{row}.{column} AS {column}"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "EXISTS (SELECT 1 FROM (SELECT {projection}) WHERE {predicate})"
    ))
}

/// The predicate table a `published_with` child names and the child's publication shape.
fn publisher<'m>(
    manifest: &'m Manifest,
    table: &TableSpec,
) -> Option<(&'m TableSpec, PublishedWith)> {
    let with = table.published_with?;
    let parent = manifest
        .table(with.parent)
        .unwrap_or_else(|| panic!("{} publishes with an unknown table", table.name));
    Some((parent, with))
}

/// Whether the child row read under `row` publishes: the parent row it names satisfies its
/// predicate, or it names none and null publishes.
fn parent_published(parent: &TableSpec, with: PublishedWith, row: &str) -> String {
    let predicate = parent
        .publish_when
        .unwrap_or_else(|| panic!("{} has no publication predicate", parent.name));
    let reference = format!("{row}.{}", with.column);
    let published = format!(
        "EXISTS (SELECT 1 FROM {name} AS {PARENT_ALIAS} WHERE {PARENT_ALIAS}.{key} = {reference} AND ({predicate}))",
        name = parent.name,
        key = parent.key_column,
    );
    if with.null_publishes {
        format!("({reference} IS NULL OR {published})")
    } else {
        published
    }
}

/// The publication condition of the trigger row `row`, or `None` when every row publishes.
pub(crate) fn condition_over(manifest: &Manifest, table: &TableSpec, row: &str) -> Option<String> {
    if let Some((parent, with)) = publisher(manifest, table) {
        return Some(parent_published(parent, with, row));
    }
    own_over(table, row)
}

/// The publication condition of a row of `table` read under `alias` in a query, or `None` when
/// every row publishes.
pub(crate) fn condition_in(manifest: &Manifest, table: &TableSpec, alias: &str) -> Option<String> {
    if let Some((parent, with)) = publisher(manifest, table) {
        return Some(parent_published(parent, with, alias));
    }
    let predicate = table.publish_when?;
    Some(format!(
        "EXISTS (SELECT 1 FROM {name} AS {PARENT_ALIAS} WHERE {PARENT_ALIAS}.{key} = {alias}.{key} AND ({predicate}))",
        name = table.name,
        key = table.key_column,
    ))
}

/// Whether `key` names a domain row of `table` that fails its publication condition. This reads
/// the device's own row and never merged winners, so the answer differs between replicas and
/// must not reach visibility or merge state.
pub(crate) async fn is_local_draft(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
    table: &TableSpec,
    key: &str,
) -> SyncResult<bool> {
    let Some(condition) = condition_in(manifest, table, ROW_ALIAS) else {
        return Ok(false);
    };
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM {name} AS {ROW_ALIAS}
         WHERE {ROW_ALIAS}.{key_column} = ? AND NOT ({condition}))",
        name = sql::ident(table.name),
        key_column = sql::ident(table.key_column),
    );
    let draft: i64 = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(key)
        .fetch_one(&mut *conn)
        .await?
        .try_get(0)?;
    Ok(draft != 0)
}

/// Whether the resolved `columns` of a `published_with` row name a parent that is a local draft.
pub(crate) async fn names_local_draft(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
    table: &TableSpec,
    columns: &[(&GroupSpec, Vec<Field>)],
) -> SyncResult<bool> {
    let Some((parent, with)) = publisher(manifest, table) else {
        return Ok(false);
    };
    let reference = columns.iter().find_map(|(group, fields)| {
        let index = group
            .columns()
            .iter()
            .position(|column| column.name == with.column)?;
        fields.get(index)
    });
    match reference {
        Some(Field::Text(key)) => is_local_draft(conn, manifest, parent, key).await,
        _ => Ok(false),
    }
}

/// The keys of every local draft of `table`.
async fn local_draft_keys(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
    table: &TableSpec,
) -> SyncResult<Vec<String>> {
    let Some(condition) = condition_in(manifest, table, ROW_ALIAS) else {
        return Ok(Vec::new());
    };
    let key_column = sql::ident(table.key_column);
    let sql = format!(
        "SELECT {ROW_ALIAS}.{key_column} FROM {name} AS {ROW_ALIAS} WHERE NOT ({condition})",
        name = sql::ident(table.name),
    );
    Ok(sqlx::query_scalar(AssertSqlSafe(sql.as_str()))
        .fetch_all(&mut *conn)
        .await?)
}

/// Deletes every local draft and the rows it owns from a database about to leave this device.
/// Children go first, because their condition reads the parent row. The caller holds the apply
/// flag, so capture and gated triggers stay quiet.
pub(crate) async fn delete_local_drafts(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
) -> SyncResult<()> {
    let (children, parents): (Vec<&TableSpec>, Vec<&TableSpec>) = manifest
        .tables
        .iter()
        .partition(|table| table.published_with.is_some());
    for table in children.into_iter().chain(parents) {
        for key in local_draft_keys(conn, manifest, table).await? {
            materialize::delete_row(conn, table, &key).await?;
        }
    }
    Ok(())
}

/// Whether a live row of a predicate table fails its predicate with the winners of its retained
/// `versions`, which hides it and the children published with it.
pub(crate) async fn unpublished(
    engine: &Engine,
    conn: &mut SqliteConnection,
    table: &TableSpec,
    key: &str,
    versions: &[merge::Version],
) -> SyncResult<bool> {
    let Some(predicate) = table.publish_when else {
        return Ok(false);
    };
    let winners = merge::winners(engine, table, versions);
    let mut fields = vec![Field::Text(key.to_string())];
    let mut names = vec![table.key_column];
    for group in table.groups {
        let GroupStorage::Columns(columns) = group.storage else {
            continue;
        };
        let values = winners.get(&group.id).map(|version| version.value.fields());
        for (index, column) in columns.iter().enumerate() {
            names.push(column.name);
            let field = match values {
                Some(values) => values
                    .get(index)
                    .cloned()
                    .ok_or_else(|| corrupt("stored value does not match its group"))?,
                None => Field::Null,
            };
            fields.push(field);
        }
    }
    let projection = names
        .iter()
        .map(|name| format!("? AS {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT EXISTS (SELECT 1 FROM (SELECT {projection}) WHERE {predicate})");
    let mut query = sqlx::query(AssertSqlSafe(sql.as_str()));
    for field in &fields {
        query = sql::bind_field(query, field);
    }
    let published: i64 = query.fetch_one(&mut *conn).await?.try_get(0)?;
    Ok(published == 0)
}
