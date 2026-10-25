//! Applies a project history bundle to the current project graph as a diff.
//!
//! Rows are matched by primary key. A bundle row that already exists is updated in place only
//! when it differs, a missing one is inserted, and nothing is deleted and reinserted under the
//! same key, so pages, blocks, and everything that hangs off them keep their identity. Current
//! pages, blocks, databases, and data sources the bundle omits are trashed rather than deleted;
//! leaf rows the bundle omits are deleted only when their owner is part of the bundle.

use super::super::json_optional_string;
use serde_json::Value;
use sqlx::{AssertSqlSafe, QueryBuilder, Sqlite, Transaction};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Restored tables in insertion order with their primary key columns.
const RESTORE_TABLES: [(&str, &[&str]); 24] = [
    ("notes_assets", &["id"]),
    ("notes_folders", &["id"]),
    ("notes_pages", &["id"]),
    ("notes_blocks", &["id"]),
    ("notes_databases", &["id"]),
    ("notes_data_sources", &["id"]),
    (
        "notes_data_source_properties",
        &["data_source_id", "property_id"],
    ),
    (
        "notes_data_source_property_options",
        &["data_source_id", "property_id", "option_id"],
    ),
    ("notes_page_property_values", &["page_id", "property_id"]),
    (
        "notes_relation_edges",
        &["source_page_id", "source_property_id", "target_page_id"],
    ),
    ("notes_database_views", &["id"]),
    ("notes_data_source_templates", &["id"]),
    (
        "notes_data_source_template_blocks",
        &["template_id", "ordinal"],
    ),
    ("notes_page_aliases", &["id"]),
    ("notes_page_notification_settings", &["page_id", "user_id"]),
    ("notes_comment_threads", &["id"]),
    ("notes_comments", &["id"]),
    (
        "notes_comment_reactions",
        &["comment_id", "created_by", "emoji"],
    ),
    ("notes_comment_thread_anchors", &["thread_id"]),
    ("notes_comment_thread_reads", &["thread_id", "user_id"]),
    ("notes_comment_thread_mutes", &["thread_id", "user_id"]),
    ("notes_suggestions", &["id"]),
    ("notes_mention_notifications", &["id"]),
    (
        "notes_asset_references",
        &["asset_id", "owner_type", "owner_id", "role"],
    ),
];

const TEMPLATE_BLOCKS_TABLE: &str = "notes_data_source_template_blocks";
const RELATION_EDGES_TABLE: &str = "notes_relation_edges";

/// Upper bound on ids bound into one `IN` list, well under the SQLite variable limit.
const BIND_CHUNK: usize = 500;

type RowKey = Vec<String>;

/// The ids of the owners present in the bundle, which gate deletion of omitted leaf rows.
struct BundleOwners {
    pages: HashSet<String>,
    databases: HashSet<String>,
    data_sources: HashSet<String>,
    templates: HashSet<String>,
    threads: HashSet<String>,
    comments: HashSet<String>,
    normalized_aliases: HashSet<String>,
}

impl BundleOwners {
    fn collect(rows: &BTreeMap<String, Vec<Value>>) -> Self {
        Self {
            pages: super::ids(rows, "notes_pages"),
            databases: super::ids(rows, "notes_databases"),
            data_sources: super::ids(rows, "notes_data_sources"),
            templates: super::ids(rows, "notes_data_source_templates"),
            threads: super::ids(rows, "notes_comment_threads"),
            comments: super::ids(rows, "notes_comments"),
            normalized_aliases: rows
                .get("notes_page_aliases")
                .into_iter()
                .flatten()
                .filter_map(|row| json_optional_string(row, "normalized_alias"))
                .collect(),
        }
    }

    fn owns(&self, row: &Value, key: &str, owners: &HashSet<String>) -> bool {
        json_optional_string(row, key).is_some_and(|id| owners.contains(&id))
    }
}

/// What happens to a current row that the bundle omits.
enum Removal {
    Keep,
    Trash,
    Delete,
}

/// Refuses bundles recorded before the current Notes storage format, which split block content
/// into `text` and `attrs` and nested pages through `child_block_id`.
pub(super) fn refuse_outdated_bundle(rows: &BTreeMap<String, Vec<Value>>) -> Result<(), String> {
    let outdated_block = rows
        .get("notes_blocks")
        .into_iter()
        .flatten()
        .any(|row| row.get("attrs").is_none() || row.get("payload").is_some());
    let outdated_page = rows
        .get("notes_pages")
        .into_iter()
        .flatten()
        .any(|row| row.get("child_block_id").is_none());
    if outdated_block || outdated_page {
        return Err(crate::project_history::OLDER_NOTES_FORMAT_ERROR.to_string());
    }
    Ok(())
}

/// Trashes or deletes the current rows the bundle omits. Rows whose id is a key of `copied_ids`
/// stay as they are: the bundle restores them as copies and the originals live on elsewhere.
pub(super) async fn remove_omitted_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    current: &BTreeMap<String, Vec<Value>>,
    bundle: &BTreeMap<String, Vec<Value>>,
    copied_ids: &HashMap<String, String>,
) -> Result<(), String> {
    let owners = BundleOwners::collect(bundle);
    let replaced_templates = replaced_template_ids(current, bundle);
    for (table, key_columns) in RESTORE_TABLES {
        if table == "notes_assets" {
            continue;
        }
        let bundle_rows = rows_by_key(bundle, table, key_columns)?;
        for row in current.get(table).into_iter().flatten() {
            if json_optional_string(row, "id").is_some_and(|id| copied_ids.contains_key(&id)) {
                continue;
            }
            let key = row_key(row, table, key_columns)?;
            let removal = match bundle_rows.get(&key) {
                Some(bundle_row) if table == "notes_page_aliases" && *bundle_row != row => {
                    Removal::Delete
                }
                Some(_) if is_replaced_template_block(table, row, &replaced_templates) => {
                    Removal::Delete
                }
                Some(_) => Removal::Keep,
                None => omitted_row_removal(table, row, &owners),
            };
            match removal {
                Removal::Keep => {}
                Removal::Trash => trash_row_tx(tx, table, &key).await?,
                Removal::Delete => delete_row_tx(tx, table, key_columns, &key).await?,
            }
        }
    }
    Ok(())
}

/// Template blocks are an owned snapshot keyed by ordinal, so a template whose block list differs
/// between the current graph and the bundle is replaced whole instead of row by row. Updating
/// single ordinals in place could collide on the unique block id or lose children to the parent
/// cascade.
fn replaced_template_ids(
    current: &BTreeMap<String, Vec<Value>>,
    bundle: &BTreeMap<String, Vec<Value>>,
) -> HashSet<String> {
    let current = template_blocks_by_template(current);
    let bundle = template_blocks_by_template(bundle);
    bundle
        .iter()
        .filter(|(template_id, blocks)| current.get(*template_id) != Some(*blocks))
        .map(|(template_id, _)| template_id.clone())
        .collect()
}

/// Groups template block rows by template, each list sorted by ordinal.
fn template_blocks_by_template(
    rows: &BTreeMap<String, Vec<Value>>,
) -> BTreeMap<String, Vec<&Value>> {
    let mut grouped = BTreeMap::<String, Vec<&Value>>::new();
    for row in rows.get(TEMPLATE_BLOCKS_TABLE).into_iter().flatten() {
        if let Some(template_id) = json_optional_string(row, "template_id") {
            grouped.entry(template_id).or_default().push(row);
        }
    }
    for blocks in grouped.values_mut() {
        blocks.sort_by_key(|row| row.get("ordinal").and_then(Value::as_i64));
    }
    grouped
}

fn is_replaced_template_block(table: &str, row: &Value, replaced: &HashSet<String>) -> bool {
    table == TEMPLATE_BLOCKS_TABLE
        && json_optional_string(row, "template_id").is_some_and(|id| replaced.contains(&id))
}

/// Changed aliases are deleted with the omitted ones, since their unique normalized alias could
/// otherwise collide while the bundle rows are written one at a time.
fn omitted_row_removal(table: &str, row: &Value, owners: &BundleOwners) -> Removal {
    let gated = |owned: bool| {
        if owned {
            Removal::Delete
        } else {
            Removal::Keep
        }
    };
    match table {
        "notes_pages" | "notes_databases" | "notes_data_sources" => Removal::Trash,
        "notes_blocks" => {
            if owners.owns(row, "page_id", &owners.pages) {
                Removal::Trash
            } else {
                Removal::Keep
            }
        }
        "notes_folders" => Removal::Delete,
        "notes_page_aliases" => gated(
            owners.owns(row, "page_id", &owners.pages)
                || owners.owns(row, "normalized_alias", &owners.normalized_aliases),
        ),
        "notes_data_source_templates"
        | "notes_data_source_properties"
        | "notes_data_source_property_options" => {
            gated(owners.owns(row, "data_source_id", &owners.data_sources))
        }
        "notes_database_views" => gated(
            owners.owns(row, "data_source_id", &owners.data_sources)
                || owners.owns(row, "database_id", &owners.databases),
        ),
        "notes_data_source_template_blocks" => {
            gated(owners.owns(row, "template_id", &owners.templates))
        }
        "notes_page_property_values" => gated(owners.owns(row, "page_id", &owners.pages)),
        "notes_relation_edges" => gated(owners.owns(row, "source_page_id", &owners.pages)),
        "notes_page_notification_settings"
        | "notes_comment_threads"
        | "notes_suggestions"
        | "notes_mention_notifications" => gated(owners.owns(row, "page_id", &owners.pages)),
        "notes_comments"
        | "notes_comment_thread_anchors"
        | "notes_comment_thread_reads"
        | "notes_comment_thread_mutes" => gated(owners.owns(row, "thread_id", &owners.threads)),
        "notes_comment_reactions" => gated(owners.owns(row, "comment_id", &owners.comments)),
        "notes_asset_references" => gated(
            owners.owns(row, "page_id", &owners.pages)
                || owners.owns(row, "data_source_id", &owners.data_sources)
                || owners.owns(row, "comment_id", &owners.comments),
        ),
        _ => Removal::Keep,
    }
}

async fn trash_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    key: &RowKey,
) -> Result<(), String> {
    let sql = match table {
        "notes_pages" => {
            "UPDATE notes_pages
             SET in_trash = 1,
                 trash_owner = NULL,
                 archived = 0,
                 trashed_time = COALESCE(trashed_time, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0"
        }
        "notes_blocks" => {
            "UPDATE notes_blocks
             SET in_trash = 1,
                 trash_owner = NULL,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0"
        }
        "notes_databases" => {
            "UPDATE notes_databases
             SET in_trash = 1, last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0"
        }
        "notes_data_sources" => {
            "UPDATE notes_data_sources
             SET in_trash = 1, last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ? AND in_trash = 0"
        }
        _ => return Err(format!("Notes history cannot trash rows of {table}")),
    };
    let id = key
        .first()
        .ok_or_else(|| format!("Notes history {table} row key is empty"))?;
    sqlx::query(sql)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("trash Notes {table} row omitted by the restored version: {e}"))?;
    Ok(())
}

async fn delete_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    key_columns: &[&str],
    key: &RowKey,
) -> Result<(), String> {
    let predicate = key_columns
        .iter()
        .map(|column| format!("{} = ?", quoted_identifier(column)))
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!("DELETE FROM {} WHERE {predicate}", quoted_identifier(table));
    let mut query = sqlx::query(AssertSqlSafe(sql.as_str()));
    for value in key {
        query = query.bind(value);
    }
    query
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete Notes {table} row omitted by the restored version: {e}"))?;
    Ok(())
}

/// Writes every bundle row that is missing from or differs from the current graph.
pub(super) async fn write_bundle_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    current: &BTreeMap<String, Vec<Value>>,
    bundle: &BTreeMap<String, Vec<Value>>,
) -> Result<(), String> {
    let replaced_templates = replaced_template_ids(current, bundle);
    for (table, key_columns) in RESTORE_TABLES {
        let Some(table_rows) = bundle.get(table) else {
            continue;
        };
        let current_rows = rows_by_key(current, table, key_columns)?;
        for row in rows_in_write_order(table, table_rows)? {
            let key = row_key(row, table, key_columns)?;
            if current_rows
                .get(&key)
                .is_some_and(|current| *current == row)
                && !is_replaced_template_block(table, row, &replaced_templates)
            {
                continue;
            }
            if table == "notes_blocks" {
                check_bundle_block_bounds(row)?;
            }
            if table == RELATION_EDGES_TABLE && !relation_edge_endpoints_exist_tx(tx, row).await? {
                continue;
            }
            upsert_json_row_tx(tx, table, key_columns, row).await?;
        }
    }
    Ok(())
}

/// Refuses a bundle block whose stored parts exceed the bounds every other block write enforces.
/// A bundle keeps each JSON column either as its stored text or as the decoded JSON value, so
/// both are measured as the text the row writes.
fn check_bundle_block_bounds(row: &Value) -> Result<(), String> {
    let block_type = json_optional_string(row, "type")
        .ok_or_else(|| "Notes history block row has no type".to_string())?;
    let part = |column: &str| match row.get(column) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::Null) | None => Err(format!(
            "Notes history {block_type} block row has no {column}"
        )),
        Some(value) => Ok(value.to_string()),
    };
    crate::block_storage::check_block_content_bounds(&block_type, &part("attrs")?, &part("text")?)
}

/// Orders rows so that each write satisfies the unique constraints a later write relies on:
/// folders come parents first.
fn rows_in_write_order<'a>(table: &str, rows: &'a [Value]) -> Result<Vec<&'a Value>, String> {
    match table {
        "notes_folders" => super::folder_rows_in_restore_order(rows),
        _ => Ok(rows.iter().collect()),
    }
}

/// Whether both pages of a bundle relation edge exist. An edge to a page outside the project
/// whose page has since been deleted is dropped instead of failing the restore.
async fn relation_edge_endpoints_exist_tx(
    tx: &mut Transaction<'_, Sqlite>,
    row: &Value,
) -> Result<bool, String> {
    let (Some(source), Some(target)) = (
        json_optional_string(row, "source_page_id"),
        json_optional_string(row, "target_page_id"),
    ) else {
        return Err("Notes history relation edge row has no pages".to_string());
    };
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT id) FROM notes_pages WHERE id IN (?, ?)")
            .bind(&source)
            .bind(&target)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("check restored Notes relation edge pages: {e}"))?;
    let expected = if source == target { 1 } else { 2 };
    Ok(count == expected)
}

async fn upsert_json_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    key_columns: &[&str],
    row: &Value,
) -> Result<(), String> {
    let object = row
        .as_object()
        .ok_or_else(|| format!("Notes history {table} row is not an object"))?;
    if object.is_empty() {
        return Err(format!("Notes history {table} row is empty"));
    }
    let columns = object.keys().collect::<Vec<_>>();
    let column_sql = columns
        .iter()
        .map(|column| quoted_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    let value_sql = columns
        .iter()
        .map(|column| format!("json_extract(?, '$.{}')", column.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let updates = columns
        .iter()
        .filter(|column| !key_columns.contains(&column.as_str()))
        .map(|column| {
            let column = quoted_identifier(column);
            format!("{column} = excluded.{column}")
        })
        .collect::<Vec<_>>();
    let conflict = if table == "notes_assets" {
        "ON CONFLICT DO NOTHING".to_string()
    } else if updates.is_empty() {
        format!(
            "ON CONFLICT({}) DO NOTHING",
            key_columns
                .iter()
                .map(|column| quoted_identifier(column))
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        format!(
            "ON CONFLICT({}) DO UPDATE SET {}",
            key_columns
                .iter()
                .map(|column| quoted_identifier(column))
                .collect::<Vec<_>>()
                .join(", "),
            updates.join(", ")
        )
    };
    let sql = format!(
        "INSERT INTO {} ({column_sql}) VALUES ({value_sql}) {conflict}",
        quoted_identifier(table)
    );
    let row_json = serde_json::to_string(row)
        .map_err(|e| format!("serialize Notes history {table} row: {e}"))?;
    let mut query = sqlx::query(AssertSqlSafe(sql.as_str()));
    for _ in &columns {
        query = query.bind(&row_json);
    }
    query
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("restore Notes history row in {table}: {e}"))?;
    Ok(())
}

/// Clears per-page state that a restore makes stale: undo history and rollup values of every
/// page the current graph or the bundle names. Rollups are derived and rebuild as they are read.
pub(super) async fn clear_page_derived_state_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_ids: &HashSet<String>,
) -> Result<(), String> {
    let page_ids = page_ids.iter().collect::<Vec<_>>();
    for chunk in page_ids.chunks(BIND_CHUNK) {
        for (sql, context) in [
            (
                "DELETE FROM notes_undo_state WHERE page_id IN (",
                "clear restored Notes undo state",
            ),
            (
                "DELETE FROM notes_data_source_rollup_cache WHERE source_page_id IN (",
                "clear restored Notes rollup values",
            ),
        ] {
            let mut query = QueryBuilder::<Sqlite>::new(sql);
            push_id_list(&mut query, chunk);
            query
                .build()
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("{context}: {e}"))?;
        }
    }
    Ok(())
}

fn push_id_list(query: &mut QueryBuilder<Sqlite>, ids: &[&String]) {
    let mut separated = query.separated(", ");
    for id in ids {
        separated.push_bind((*id).clone());
    }
    separated.push_unseparated(")");
}

fn rows_by_key<'a>(
    rows: &'a BTreeMap<String, Vec<Value>>,
    table: &str,
    key_columns: &[&str],
) -> Result<HashMap<RowKey, &'a Value>, String> {
    rows.get(table)
        .into_iter()
        .flatten()
        .map(|row| row_key(row, table, key_columns).map(|key| (key, row)))
        .collect()
}

fn row_key(row: &Value, table: &str, key_columns: &[&str]) -> Result<RowKey, String> {
    key_columns
        .iter()
        .map(|column| {
            json_optional_string(row, column)
                .ok_or_else(|| format!("Notes history {table} row is missing {column}"))
        })
        .collect()
}

fn quoted_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::{check_bundle_block_bounds, refuse_outdated_bundle};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn bundles_from_before_split_block_storage_are_refused() {
        let mut rows = BTreeMap::new();
        rows.insert(
            "notes_blocks".to_string(),
            vec![json!({ "id": "block", "payload": "{}", "sort_order": 0 })],
        );
        assert!(refuse_outdated_bundle(&rows).is_err());

        rows.insert(
            "notes_blocks".to_string(),
            vec![json!({ "id": "block", "attrs": "{}", "text": "{}" })],
        );
        rows.insert(
            "notes_pages".to_string(),
            vec![json!({ "id": "page", "parent_type": "workspace" })],
        );
        assert!(refuse_outdated_bundle(&rows).is_err());

        rows.insert(
            "notes_pages".to_string(),
            vec![json!({ "id": "page", "child_block_id": null })],
        );
        assert!(refuse_outdated_bundle(&rows).is_ok());
    }

    #[test]
    fn bundle_blocks_above_the_stored_bounds_are_refused() {
        let within = json!({ "id": "block", "type": "paragraph", "attrs": "{}", "text": "{}" });
        assert!(check_bundle_block_bounds(&within).is_ok());
        let oversized_text = "x".repeat(crate::block_storage::MAX_BLOCK_TEXT_BYTES + 1);
        let oversized =
            json!({ "id": "block", "type": "paragraph", "attrs": "{}", "text": oversized_text });
        let error = check_bundle_block_bounds(&oversized).unwrap_err();
        assert!(error.contains("byte limit"), "{error}");
        let missing_text = json!({ "id": "block", "type": "paragraph", "attrs": "{}" });
        assert!(check_bundle_block_bounds(&missing_text).is_err());
    }
}
