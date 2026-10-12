//! Dynamic SQL over manifest tables and exact field reads.
//!
//! Domain values are read with their storage class, so a value is never converted on the way
//! into an operation: a real number or text that is not UTF-8 is reported as unreadable.

use ganbaru_sync_contracts::Field;
use sqlx::sqlite::{SqliteArguments, SqliteRow};
use sqlx::{Row, Sqlite, SqliteConnection};

use crate::manifest::OwnedTable;

/// A query with SQLite arguments.
pub(crate) type SqliteQuery<'q> = sqlx::query::Query<'q, Sqlite, SqliteArguments<'q>>;

/// Quotes an identifier.
pub(crate) fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Binds a field with its storage class.
pub(crate) fn bind_field<'q>(query: SqliteQuery<'q>, field: &Field) -> SqliteQuery<'q> {
    match field {
        Field::Null => query.bind(Option::<i64>::None),
        Field::Integer(value) => query.bind(*value),
        Field::Text(text) => query.bind(text.clone()),
        Field::Blob(bytes) => query.bind(bytes.clone()),
    }
}

/// Select list reading each column as its storage class and its value.
pub(crate) fn typed_columns<'a>(columns: impl IntoIterator<Item = &'a str>) -> String {
    columns
        .into_iter()
        .map(|column| format!("typeof({name}), {name}", name = ident(column)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads the field at `index` written by [`typed_columns`]; `None` when it has no field form.
pub(crate) fn read_field(row: &SqliteRow, index: usize) -> Result<Option<Field>, sqlx::Error> {
    let kind: String = row.try_get(index)?;
    let field = match kind.as_str() {
        "null" => Some(Field::Null),
        "integer" => Some(Field::Integer(row.try_get_unchecked(index + 1)?)),
        "blob" => Some(Field::Blob(row.try_get_unchecked(index + 1)?)),
        "text" => {
            let bytes: Vec<u8> = row.try_get_unchecked(index + 1)?;
            String::from_utf8(bytes).ok().map(Field::Text)
        }
        _ => None,
    };
    Ok(field)
}

/// Reads `count` typed fields starting at column `offset`.
pub(crate) fn read_fields(
    row: &SqliteRow,
    offset: usize,
    count: usize,
) -> Result<Vec<Option<Field>>, sqlx::Error> {
    (0..count)
        .map(|position| read_field(row, offset + position * 2))
        .collect()
}

/// Columns of one domain row by key, or `None` when the row does not exist.
pub(crate) async fn read_columns(
    conn: &mut SqliteConnection,
    table: &str,
    key_column: &str,
    key: &str,
    columns: &[&str],
) -> Result<Option<Vec<Option<Field>>>, sqlx::Error> {
    let select = if columns.is_empty() {
        "1".to_string()
    } else {
        typed_columns(columns.iter().copied())
    };
    let sql = format!(
        "SELECT {select} FROM {} WHERE {} = ?",
        ident(table),
        ident(key_column)
    );
    let row = sqlx::query(&sql)
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(|row| read_fields(&row, 0, columns.len()))
        .transpose()
}

/// Owned child rows of a parent in order, each in [`OwnedTable::columns`] order.
pub(crate) async fn read_owned(
    conn: &mut SqliteConnection,
    owned: &OwnedTable,
    parent: &str,
) -> Result<Vec<Vec<Option<Field>>>, sqlx::Error> {
    let sql = format!(
        "SELECT {} FROM {} WHERE {} = ? ORDER BY {}",
        typed_columns(owned.columns.iter().copied()),
        ident(owned.table),
        ident(owned.parent_column),
        ident(owned.order_column),
    );
    let rows = sqlx::query(&sql).bind(parent).fetch_all(&mut *conn).await?;
    rows.iter()
        .map(|row| read_fields(row, 0, owned.columns.len()))
        .collect()
}

/// Replaces the owned child rows of a parent with `rows`, numbering them from zero.
pub(crate) async fn replace_owned(
    conn: &mut SqliteConnection,
    owned: &OwnedTable,
    parent: &str,
    rows: &[Vec<Field>],
) -> Result<(), sqlx::Error> {
    let delete = format!(
        "DELETE FROM {} WHERE {} = ?",
        ident(owned.table),
        ident(owned.parent_column)
    );
    sqlx::query(&delete)
        .bind(parent)
        .execute(&mut *conn)
        .await?;
    let columns = [owned.parent_column, owned.order_column]
        .into_iter()
        .chain(owned.columns.iter().copied())
        .map(ident)
        .collect::<Vec<_>>();
    let insert = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        ident(owned.table),
        columns.join(", "),
        vec!["?"; columns.len()].join(", ")
    );
    for (position, fields) in rows.iter().enumerate() {
        let mut query = sqlx::query(&insert).bind(parent).bind(position as i64);
        for field in fields {
            query = bind_field(query, field);
        }
        query.execute(&mut *conn).await?;
    }
    Ok(())
}

/// Sets or clears the flag that suppresses capture while the engine writes domain rows.
pub(crate) async fn set_applying(
    conn: &mut SqliteConnection,
    applying: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(i64::from(applying))
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Starts an engine write inside the current transaction: capture is suppressed and foreign
/// keys are checked at commit, because merged rows of several tables materialize in steps.
pub(crate) async fn begin_engine_write(conn: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    set_applying(conn, true).await?;
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *conn)
        .await?;
    Ok(())
}
