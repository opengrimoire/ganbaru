//! Dynamic SQL over manifest tables and exact field reads.
//!
//! Domain values are read with their storage class, so a value is never converted on the way
//! into an operation: a real number or text that is not UTF-8 is reported as unreadable.

use ganbaru_sync_contracts::Field;
use sqlx::sqlite::{SqliteArguments, SqliteRow};
use sqlx::{AssertSqlSafe, Row, Sqlite, SqliteConnection};

use crate::codec::OwnedRow;
use crate::manifest::{OwnedOrder, OwnedTable};

/// A query with SQLite arguments.
pub(crate) type SqliteQuery<'q> = sqlx::query::Query<'q, Sqlite, SqliteArguments>;

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
    let row = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    row.map(|row| read_fields(&row, 0, columns.len()))
        .transpose()
}

/// Owned rows of an owner in order with their nested rows, or `None` when a field has no
/// field form.
pub(crate) async fn read_owned(
    conn: &mut SqliteConnection,
    owned: &OwnedTable,
    parent: &str,
) -> Result<Option<Vec<OwnedRow>>, sqlx::Error> {
    // Nested rows name their parent row by its ordinal.
    let ordinal = match owned.order {
        OwnedOrder::Ordinal(ordinal) => Some(ordinal),
        OwnedOrder::Nested { .. } | OwnedOrder::Key(_) => None,
    };
    let rows = select_owned(conn, owned, ordinal, parent).await?;
    let mut parents = Vec::with_capacity(rows.len());
    for (position, fields) in rows {
        let Some(fields) = fields else {
            return Ok(None);
        };
        parents.push((position, OwnedRow::flat(fields)));
    }
    for nested in owned.nested {
        for (_, row) in &mut parents {
            row.nested.push(Vec::new());
        }
        let OwnedOrder::Nested { parent_ordinal, .. } = nested.order else {
            continue;
        };
        for (position, fields) in select_owned(conn, nested, Some(parent_ordinal), parent).await? {
            let Some(fields) = fields else {
                return Ok(None);
            };
            let Some((_, row)) = parents.iter_mut().find(|(at, _)| *at == position) else {
                return Ok(None);
            };
            if let Some(rows) = row.nested.last_mut() {
                rows.push(OwnedRow::flat(fields));
            }
        }
    }
    Ok(Some(parents.into_iter().map(|(_, row)| row).collect()))
}

/// Rows of one owned table for an owner in order: the value of `position` when given, and the
/// data fields, or `None` when a field has no field form.
async fn select_owned(
    conn: &mut SqliteConnection,
    owned: &OwnedTable,
    position: Option<&str>,
    parent: &str,
) -> Result<Vec<(Option<i64>, Option<Vec<Field>>)>, sqlx::Error> {
    let order = owned
        .order
        .order_columns()
        .into_iter()
        .map(ident)
        .collect::<Vec<_>>()
        .join(", ");
    let position_column = position.map_or_else(|| "NULL".to_string(), ident);
    let sql = format!(
        "SELECT {position_column}, {} FROM {} WHERE {} = ? ORDER BY {order}",
        typed_columns(owned.columns.iter().map(|column| column.name)),
        ident(owned.table),
        ident(owned.parent_column),
    );
    let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(parent)
        .fetch_all(&mut *conn)
        .await?;
    rows.iter()
        .map(|row| {
            let position: Option<i64> = row.try_get(0)?;
            let fields = read_fields(row, 1, owned.columns.len())?
                .into_iter()
                .collect::<Option<Vec<_>>>();
            Ok((position, fields))
        })
        .collect()
}

/// Replaces the owned rows of an owner and their nested rows, numbering ordinals from zero:
/// nested rows are deleted first and inserted last.
pub(crate) async fn replace_owned(
    conn: &mut SqliteConnection,
    owned: &OwnedTable,
    parent: &str,
    rows: &[OwnedRow],
) -> Result<(), sqlx::Error> {
    for table in owned.nested.iter().chain(std::iter::once(owned)) {
        let delete = format!(
            "DELETE FROM {} WHERE {} = ?",
            ident(table.table),
            ident(table.parent_column)
        );
        sqlx::query(AssertSqlSafe(delete.as_str()))
            .bind(parent)
            .execute(&mut *conn)
            .await?;
    }
    let insert = insert_owned_sql(owned);
    let ordinals = owned.order.ordinal_columns().len();
    for (position, row) in rows.iter().enumerate() {
        let mut query = sqlx::query(AssertSqlSafe(insert.as_str())).bind(parent);
        if ordinals > 0 {
            query = query.bind(position as i64);
        }
        for field in &row.fields {
            query = bind_field(query, field);
        }
        query.execute(&mut *conn).await?;
    }
    for (index, nested) in owned.nested.iter().enumerate() {
        let insert = insert_owned_sql(nested);
        for (parent_position, row) in rows.iter().enumerate() {
            let nested_rows = row.nested.get(index).map_or(&[][..], Vec::as_slice);
            for (position, nested_row) in nested_rows.iter().enumerate() {
                let mut query = sqlx::query(AssertSqlSafe(insert.as_str()))
                    .bind(parent)
                    .bind(parent_position as i64)
                    .bind(position as i64);
                for field in &nested_row.fields {
                    query = bind_field(query, field);
                }
                query.execute(&mut *conn).await?;
            }
        }
    }
    Ok(())
}

fn insert_owned_sql(owned: &OwnedTable) -> String {
    let columns = owned
        .written_columns()
        .into_iter()
        .map(ident)
        .collect::<Vec<_>>();
    format!(
        "INSERT INTO {} ({}) VALUES ({})",
        ident(owned.table),
        columns.join(", "),
        vec!["?"; columns.len()].join(", ")
    )
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
