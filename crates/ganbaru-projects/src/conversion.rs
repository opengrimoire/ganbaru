//! Projects conversion pass run at vault open, before sync captures are seeded.
//!
//! The order keys migration leaves `'pending:' || rowid` placeholders in every `sync_key` and dependency id, and
//! icon references without the file size they now carry. This pass rewrites each placeholder to the key every device
//! derives from the same row, and fills icon sizes from the vault files. It is idempotent, so a vault that is
//! already converted is left unchanged.

use std::path::Path;

use ganbaru_sync_contracts::PENDING_KEY_PREFIX;
use sqlx::{AssertSqlSafe, SqliteConnection, SqlitePool};

use crate::icons::{
    MISSING_ICON_FALLBACK, PROJECT_ICON_REFERENCE_PREFIX, icon_asset_path, project_icon_byte_size,
};
use crate::keys::{
    custom_field_value_key, dependency_key, priority_key, task_tag_key, view_preference_key,
};

/// What one conversion pass changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConversionReport {
    /// Placeholder keys rewritten to derived keys.
    pub rekeyed_rows: u64,
    /// Icon and emoji references that received their file size.
    pub sized_icons: u64,
    /// Group and project icons reset because their file is missing or too large.
    pub reset_icons: u64,
    /// Custom emojis removed because their file is missing or too large.
    pub removed_emojis: u64,
}

/// A table whose placeholder keys are rewritten from two key columns.
struct KeyedTable {
    table: &'static str,
    parts: [&'static str; 2],
    derive: fn(&str, &str) -> Result<String, String>,
    /// Whether the `id` column holds the same key as `sync_key`, which only dependencies do; such a row is
    /// replaced instead of updated.
    id_is_key: bool,
}

const KEYED_TABLES: [KeyedTable; 5] = [
    KeyedTable {
        table: "project_priorities",
        parts: ["project_id", "id"],
        derive: priority_key,
        id_is_key: false,
    },
    KeyedTable {
        table: "project_task_dependencies",
        parts: ["blocking_task_id", "blocked_task_id"],
        derive: dependency_key,
        id_is_key: true,
    },
    KeyedTable {
        table: "project_task_tag_links",
        parts: ["task_id", "tag_id"],
        derive: task_tag_key,
        id_is_key: false,
    },
    KeyedTable {
        table: "project_custom_field_values",
        parts: ["task_id", "field_id"],
        derive: custom_field_value_key,
        id_is_key: false,
    },
    KeyedTable {
        table: "project_view_preferences",
        parts: ["view_id", "preference_key"],
        derive: view_preference_key,
        id_is_key: false,
    },
];

/// Converts the Projects tables in one transaction. `vault_root` is the active vault folder that holds `assets/`.
pub async fn convert(pool: &SqlitePool, vault_root: &Path) -> Result<ConversionReport, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin projects conversion: {error}"))?;
    let mut report = ConversionReport::default();
    for table in &KEYED_TABLES {
        report.rekeyed_rows += rekey_table(&mut tx, table).await?;
    }
    for table in ["project_groups", "projects"] {
        let (sized, reset) = size_icons(&mut tx, table, vault_root).await?;
        report.sized_icons += sized;
        report.reset_icons += reset;
    }
    let (sized, removed) = size_emojis(&mut tx, vault_root).await?;
    report.sized_icons += sized;
    report.removed_emojis += removed;
    tx.commit()
        .await
        .map_err(|error| format!("commit projects conversion: {error}"))?;
    Ok(report)
}

/// Refuses a vault where any Projects key is still a placeholder, so sync never publishes one.
pub async fn assert_converted(pool: &SqlitePool) -> Result<(), String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire projects conversion check: {error}"))?;
    for table in &KEYED_TABLES {
        let remaining: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT count(*) FROM {} WHERE {}",
            table.table, PENDING_CONDITION
        )))
        .bind(PENDING_KEY_PREFIX)
        .fetch_one(&mut *conn)
        .await
        .map_err(|error| format!("check {} keys: {error}", table.table))?;
        if remaining > 0 {
            return Err(format!(
                "{} still has {remaining} placeholder keys; run the projects conversion first",
                table.table
            ));
        }
    }
    Ok(())
}

/// Matches placeholder keys; a dependency `id` cannot differ from its `sync_key`, so checking
/// `sync_key` covers both.
const PENDING_CONDITION: &str = "substr(sync_key, 1, length(?1)) = ?1";

async fn rekey_table(conn: &mut SqliteConnection, table: &KeyedTable) -> Result<u64, String> {
    let rows = sqlx::query_as::<_, (i64, String, String)>(AssertSqlSafe(format!(
        "SELECT rowid, {}, {} FROM {} WHERE {} ORDER BY rowid",
        table.parts[0], table.parts[1], table.table, PENDING_CONDITION
    )))
    .bind(PENDING_KEY_PREFIX)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load {} placeholder keys: {error}", table.table))?;
    for (rowid, first, second) in &rows {
        let key = (table.derive)(first, second)?;
        if table.id_is_key {
            replace_dependency(conn, *rowid, &key).await?;
        } else {
            sqlx::query(AssertSqlSafe(format!(
                "UPDATE {} SET sync_key = ?1 WHERE rowid = ?2",
                table.table
            )))
            .bind(&key)
            .bind(rowid)
            .execute(&mut *conn)
            .await
            .map_err(|error| format!("write {} key: {error}", table.table))?;
        }
    }
    Ok(rows.len() as u64)
}

/// Rewrites the key of a dependency by deleting and inserting it again, because sync refuses an
/// update of the `id` row key. The row is read first and deleted before the insert, so the
/// replacement never meets the unique task pair the original still holds.
async fn replace_dependency(
    conn: &mut SqliteConnection,
    rowid: i64,
    key: &str,
) -> Result<(), String> {
    let (blocking, blocked, dependency_type, created_at) =
        sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT blocking_task_id, blocked_task_id, dependency_type, created_at
             FROM project_task_dependencies WHERE rowid = ?",
        )
        .bind(rowid)
        .fetch_one(&mut *conn)
        .await
        .map_err(|error| format!("load project_task_dependencies row: {error}"))?;
    sqlx::query("DELETE FROM project_task_dependencies WHERE rowid = ?")
        .bind(rowid)
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("remove project_task_dependencies placeholder: {error}"))?;
    sqlx::query(
        "INSERT INTO project_task_dependencies
            (id, sync_key, blocking_task_id, blocked_task_id, dependency_type, created_at)
         VALUES (?1, ?1, ?2, ?3, ?4, ?5)",
    )
    .bind(key)
    .bind(&blocking)
    .bind(&blocked)
    .bind(&dependency_type)
    .bind(&created_at)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("write project_task_dependencies key: {error}"))?;
    Ok(())
}

/// Fills icon sizes of groups or projects, resetting icons whose file cannot be stored. Returns the sized and reset
/// counts.
async fn size_icons(
    conn: &mut SqliteConnection,
    table: &'static str,
    vault_root: &Path,
) -> Result<(u64, u64), String> {
    let rows = sqlx::query_as::<_, (String, String)>(AssertSqlSafe(format!(
        "SELECT id, icon FROM {table} WHERE icon_byte_size IS NULL AND substr(icon, 1, ?) = ? ORDER BY id"
    )))
    .bind(PROJECT_ICON_REFERENCE_PREFIX.len() as i64)
    .bind(PROJECT_ICON_REFERENCE_PREFIX)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load {table} icons: {error}"))?;
    let (mut sized, mut reset) = (0, 0);
    for (id, icon) in rows {
        let size = match icon_asset_path(&icon) {
            Some(relative_path) => project_icon_byte_size(vault_root, relative_path)?,
            None => None,
        };
        let result = match size {
            Some(size) => {
                sized += 1;
                sqlx::query(AssertSqlSafe(format!(
                    "UPDATE {table} SET icon_byte_size = ? WHERE id = ?"
                )))
                .bind(size)
                .bind(&id)
                .execute(&mut *conn)
                .await
            }
            None => {
                reset += 1;
                sqlx::query(AssertSqlSafe(format!(
                    "UPDATE {table} SET icon = ?, icon_byte_size = NULL WHERE id = ?"
                )))
                .bind(MISSING_ICON_FALLBACK)
                .bind(&id)
                .execute(&mut *conn)
                .await
            }
        };
        result.map_err(|error| format!("write {table} icon size: {error}"))?;
    }
    Ok((sized, reset))
}

/// Fills custom emoji sizes, removing emojis whose file cannot be stored. Returns the sized and removed counts.
async fn size_emojis(conn: &mut SqliteConnection, vault_root: &Path) -> Result<(u64, u64), String> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, asset_path FROM project_custom_emojis WHERE asset_byte_size IS NULL ORDER BY id",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load custom emoji sizes: {error}"))?;
    let (mut sized, mut removed) = (0, 0);
    for (id, asset_path) in rows {
        let result = match project_icon_byte_size(vault_root, &asset_path)? {
            Some(size) => {
                sized += 1;
                sqlx::query("UPDATE project_custom_emojis SET asset_byte_size = ? WHERE id = ?")
                    .bind(size)
                    .bind(&id)
                    .execute(&mut *conn)
                    .await
            }
            None => {
                removed += 1;
                sqlx::query("DELETE FROM project_custom_emojis WHERE id = ?")
                    .bind(&id)
                    .execute(&mut *conn)
                    .await
            }
        };
        result.map_err(|error| format!("write custom emoji size: {error}"))?;
    }
    Ok((sized, removed))
}
