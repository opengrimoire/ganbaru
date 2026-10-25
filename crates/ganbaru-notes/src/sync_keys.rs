//! Derived sync keys for Notes rows identified by other rows, and the conversion pass run at vault
//! open.
//!
//! Rows whose identity is a composite of other ids carry a `sync_key` that every device derives
//! from the same identity, so the same row written on two devices addresses one sync row. The
//! migrations that create these tables leave `pending:` placeholders, which [`convert`] rewrites
//! to the derived keys before sync captures are seeded.

use ganbaru_sync_contracts::{PENDING_KEY_PREFIX, derived_key};
use serde_json::Value;
use sqlx::{Row, SqliteConnection, SqlitePool};

/// A Notes table whose `sync_key` is derived from its identity columns.
#[derive(Debug, Clone, Copy)]
pub struct DerivedKeyTable {
    /// Table name.
    pub table: &'static str,
    /// Derived key prefix.
    pub prefix: &'static str,
    /// Identity columns, in derivation order.
    pub columns: &'static [&'static str],
}

/// Data source properties, keyed by data source and property.
pub const PROPERTIES: DerivedKeyTable = DerivedKeyTable {
    table: "notes_data_source_properties",
    prefix: "notes-property",
    columns: &["data_source_id", "property_id"],
};

/// Select, multi-select, and status options, keyed by data source, property, and option.
pub const PROPERTY_OPTIONS: DerivedKeyTable = DerivedKeyTable {
    table: "notes_data_source_property_options",
    prefix: "notes-property-option",
    columns: &["data_source_id", "property_id", "option_id"],
};

/// Row page property values, keyed by page and property.
pub const PROPERTY_VALUES: DerivedKeyTable = DerivedKeyTable {
    table: "notes_page_property_values",
    prefix: "notes-property-value",
    columns: &["page_id", "property_id"],
};

/// Canonical relation edges, keyed by source page, source property, and target page.
pub const RELATION_EDGES: DerivedKeyTable = DerivedKeyTable {
    table: "notes_relation_edges",
    prefix: "notes-relation-edge",
    columns: &["source_page_id", "source_property_id", "target_page_id"],
};

/// Every Notes table with a derived `sync_key`.
pub const DERIVED_KEY_TABLES: &[DerivedKeyTable] = &[
    PROPERTIES,
    PROPERTY_OPTIONS,
    PROPERTY_VALUES,
    RELATION_EDGES,
];

/// Matches placeholder keys.
const PENDING_CONDITION: &str = "substr(sync_key, 1, length(?1)) = ?1";

impl DerivedKeyTable {
    /// The derived key of the row with identity `parts`.
    pub fn key(&self, parts: &[&str]) -> Result<String, String> {
        derived_key(self.prefix, parts)
            .map(|key| key.as_str().to_string())
            .map_err(|error| format!("derive {} key: {error}", self.prefix))
    }

    /// The derived key of a row exported as a JSON object, read from its identity columns.
    pub fn row_key(&self, row: &Value) -> Result<String, String> {
        let parts = self
            .columns
            .iter()
            .map(|column| {
                row.get(*column)
                    .and_then(Value::as_str)
                    .ok_or_else(|| format!("{} row is missing {column}", self.table))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.key(&parts)
    }
}

/// The derived-key table named `table`, if it has one.
pub fn derived_key_table(table: &str) -> Option<&'static DerivedKeyTable> {
    DERIVED_KEY_TABLES.iter().find(|entry| entry.table == table)
}

/// Rewrites every placeholder key in one transaction. Returns the rewritten count.
///
/// The pass is idempotent, so a vault that is already converted is left unchanged.
pub async fn convert(pool: &SqlitePool) -> Result<u64, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Notes key conversion: {error}"))?;
    let mut converted = 0;
    for table in DERIVED_KEY_TABLES {
        converted += convert_table(&mut tx, table).await?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit Notes key conversion: {error}"))?;
    Ok(converted)
}

async fn convert_table(
    connection: &mut SqliteConnection,
    table: &DerivedKeyTable,
) -> Result<u64, String> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT rowid, {} FROM {} WHERE {PENDING_CONDITION} ORDER BY rowid",
        table.columns.join(", "),
        table.table
    )))
    .bind(PENDING_KEY_PREFIX)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| format!("load {} placeholder keys: {error}", table.table))?;
    for row in &rows {
        let rowid: i64 = row
            .try_get(0)
            .map_err(|error| format!("read {} rowid: {error}", table.table))?;
        let parts = (1..=table.columns.len())
            .map(|index| row.try_get::<String, _>(index))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("read {} identity: {error}", table.table))?;
        let parts = parts.iter().map(String::as_str).collect::<Vec<_>>();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {} SET sync_key = ?1 WHERE rowid = ?2",
            table.table
        )))
        .bind(table.key(&parts)?)
        .bind(rowid)
        .execute(&mut *connection)
        .await
        .map_err(|error| format!("write {} key: {error}", table.table))?;
    }
    Ok(rows.len() as u64)
}

/// Refuses a vault where any Notes key is still a placeholder, so sync never publishes one.
pub async fn assert_converted(pool: &SqlitePool) -> Result<(), String> {
    for table in DERIVED_KEY_TABLES {
        let remaining: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {} WHERE {PENDING_CONDITION}",
            table.table
        )))
        .bind(PENDING_KEY_PREFIX)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("check {} keys: {error}", table.table))?;
        if remaining > 0 {
            return Err(format!(
                "{} still has {remaining} placeholder keys; run the Notes conversion first",
                table.table
            ));
        }
    }
    Ok(())
}
