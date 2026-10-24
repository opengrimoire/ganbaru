//! Restores a project version as a diff against the current project graph.
//!
//! Bundle rows that are still part of the project keep their ids and are rewritten in place.
//! Bundle pages and blocks that left the project or no longer exist come back as copies under
//! fresh ids, together with everything nested under them, so a moved graph stays intact where it
//! now lives and a deleted id is never reused.

mod diff;

use super::{
    NotesProjectHistoryRestorePlanDto, NotesProjectHistoryVersionDto, create_checkpoint,
    json_optional_string, load_manifest_rows_tx, load_manifest_tx, validate_project_id,
};
use crate::{links, search, writes};
use serde_json::Value;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use std::collections::{BTreeMap, HashMap, HashSet};

fn ids(rows: &BTreeMap<String, Vec<Value>>, table: &str) -> HashSet<String> {
    rows.get(table)
        .into_iter()
        .flatten()
        .filter_map(|row| json_optional_string(row, "id"))
        .collect()
}

fn canonical_row(row: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(row).map_err(|e| format!("serialize Notes history comparison row: {e}"))
}

async fn restore_plan(
    pool: &SqlitePool,
    project_id: &str,
    version_id: &str,
) -> Result<
    (
        NotesProjectHistoryRestorePlanDto,
        BTreeMap<String, Vec<Value>>,
    ),
    String,
> {
    let project_id = validate_project_id(project_id)?;
    let current = super::scope::load_project_graph(pool, &project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes history restore preview: {e}"))?;
    let (manifest, _) = load_manifest_tx(&mut tx, &project_id, version_id).await?;
    let historical = load_manifest_rows_tx(&mut tx, &manifest).await?;
    let current_pages = ids(&current.rows_by_table, "notes_pages");
    let historical_pages = ids(&historical, "notes_pages");
    let existing_historical_rows = if historical_pages.is_empty() {
        Vec::new()
    } else {
        let mut query = QueryBuilder::<Sqlite>::new("SELECT id FROM notes_pages WHERE id IN (");
        let mut separated = query.separated(", ");
        for id in &historical_pages {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
        query
            .build()
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| format!("find moved Notes history pages: {e}"))?
    };
    let copy_count = existing_historical_rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>("id").ok())
        .filter(|id| !current_pages.contains(id))
        .count() as i64;
    let current_by_id = current
        .rows_by_table
        .get("notes_pages")
        .into_iter()
        .flatten()
        .filter_map(|row| json_optional_string(row, "id").map(|id| (id, row)))
        .collect::<HashMap<_, _>>();
    let change_count = historical
        .get("notes_pages")
        .into_iter()
        .flatten()
        .filter(|row| {
            let Some(id) = json_optional_string(row, "id") else {
                return false;
            };
            current_by_id.get(&id).is_some_and(|current_row| {
                canonical_row(current_row).ok() != canonical_row(row).ok()
            })
        })
        .count() as i64;
    let plan = NotesProjectHistoryRestorePlanDto {
        version_id: version_id.to_string(),
        remove_count: current
            .rows_by_table
            .get("notes_pages")
            .into_iter()
            .flatten()
            .filter(|row| row.get("in_trash").and_then(Value::as_i64) == Some(0))
            .filter(|row| !value_in(row, "id", &historical_pages))
            .count() as i64,
        recreate_count: historical_pages.difference(&current_pages).count() as i64 - copy_count,
        change_count,
        copy_count,
        safety_version_will_be_created: true,
    };
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes history restore preview: {e}"))?;
    Ok((plan, historical))
}

pub(super) async fn preview_restore(
    pool: &SqlitePool,
    project_id: &str,
    version_id: &str,
) -> Result<NotesProjectHistoryRestorePlanDto, String> {
    restore_plan(pool, project_id, version_id)
        .await
        .map(|(plan, _)| plan)
}

pub(super) async fn restore_version(
    pool: &SqlitePool,
    project_id: &str,
    version_id: &str,
) -> Result<NotesProjectHistoryVersionDto, String> {
    let project_id = validate_project_id(project_id)?;
    let (_, mut historical) = restore_plan(pool, &project_id, version_id).await?;
    diff::refuse_outdated_bundle(&historical)?;
    create_checkpoint(
        pool,
        &project_id,
        "safety_before_restore",
        None,
        None,
        "Before version history restore",
    )
    .await?;
    let current = super::scope::load_project_graph(pool, &project_id)
        .await?
        .rows_by_table;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes project history restore: {e}"))?;
    sqlx::query("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("defer Notes restore foreign keys: {e}"))?;
    let copy_scope = copied_graph_scope(&historical, &current);
    let id_map = create_copy_id_map(&mut tx, &historical, &copy_scope).await?;
    remap_copy_rows(&mut historical, &id_map);
    refresh_derived_sync_keys(&mut historical)?;
    resolve_copied_alias_conflicts_tx(&mut tx, &mut historical, &id_map).await?;
    suppress_historical_notifications(&mut historical);
    let mut touched_page_ids = ids(&current, "notes_pages");
    touched_page_ids.extend(ids(&historical, "notes_pages"));
    diff::remove_omitted_rows_tx(&mut tx, &current, &historical, &id_map).await?;
    diff::write_bundle_rows_tx(&mut tx, &current, &historical).await?;
    diff::clear_page_derived_state_tx(&mut tx, &touched_page_ids).await?;
    validate_restored_hierarchy_tx(&mut tx, &historical).await?;
    sqlx::query("DELETE FROM notes_project_history_dirty WHERE project_id = ?")
        .bind(&project_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear restored Notes history dirty state: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes project history restore: {e}"))?;

    search::rebuild_index(pool).await?;
    links::backlinks::rebuild_index(pool).await?;
    links::facts::rebuild_index(pool).await?;
    if let Some(version) = create_checkpoint(
        pool,
        &project_id,
        "restore",
        None,
        None,
        "Restored a project version",
    )
    .await?
    {
        return Ok(version);
    }
    let row = sqlx::query(
        "SELECT id, project_id, manifest_hash, reason, created_by, display_name,
                changed_note_summary, page_count, active_page_count,
                archived_page_count, deleted_page_count, created_time
         FROM notes_project_history_versions
         WHERE project_id = ?
         ORDER BY created_time DESC, id DESC
         LIMIT 1",
    )
    .bind(&project_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load restored Notes project history version: {e}"))?;
    super::version_from_row(&row)
}

#[derive(Default)]
struct CopyScope {
    page_ids: HashSet<String>,
    block_ids: HashSet<String>,
    database_ids: HashSet<String>,
    data_source_ids: HashSet<String>,
    template_ids: HashSet<String>,
    thread_ids: HashSet<String>,
    comment_ids: HashSet<String>,
}

/// Collects the bundle rows restored as copies: every bundle page and block that is not part of
/// the current project graph, and everything nested under them. A page, its `child_page` block,
/// and a database and its `child_database` block share an id, so each pair is copied together.
/// A pair whose page or database is still in the current graph keeps its id even when the paired
/// block is gone, so restore mints fresh ids only for rows that now live elsewhere.
fn copied_graph_scope(
    rows: &BTreeMap<String, Vec<Value>>,
    current: &BTreeMap<String, Vec<Value>>,
) -> CopyScope {
    let current_page_ids = ids(current, "notes_pages");
    let current_block_ids = ids(current, "notes_blocks");
    let current_database_ids = ids(current, "notes_databases");
    let in_current_graph = |row: &Value| {
        value_in(row, "id", &current_page_ids) || value_in(row, "id", &current_database_ids)
    };
    let mut scope = CopyScope {
        page_ids: ids(rows, "notes_pages")
            .difference(&current_page_ids)
            .cloned()
            .collect(),
        block_ids: ids(rows, "notes_blocks")
            .difference(&current_block_ids)
            .filter(|id| !current_page_ids.contains(*id) && !current_database_ids.contains(*id))
            .cloned()
            .collect(),
        ..CopyScope::default()
    };
    loop {
        let before = (
            scope.page_ids.len(),
            scope.block_ids.len(),
            scope.database_ids.len(),
            scope.data_source_ids.len(),
        );
        for row in rows.get("notes_blocks").into_iter().flatten() {
            if in_current_graph(row) {
                continue;
            }
            if value_in(row, "page_id", &scope.page_ids)
                || value_in(row, "id", &scope.page_ids)
                || value_in(row, "parent_block_id", &scope.block_ids)
            {
                insert_row_id(&mut scope.block_ids, row);
            }
        }
        for row in rows.get("notes_databases").into_iter().flatten() {
            if in_current_graph(row) {
                continue;
            }
            if value_in(row, "id", &scope.block_ids)
                || value_in(row, "parent_page_id", &scope.page_ids)
                || value_in(row, "parent_block_id", &scope.block_ids)
            {
                insert_row_id(&mut scope.database_ids, row);
            }
        }
        for row in rows.get("notes_data_sources").into_iter().flatten() {
            if value_in(row, "database_id", &scope.database_ids) {
                insert_row_id(&mut scope.data_source_ids, row);
            }
        }
        for row in rows.get("notes_pages").into_iter().flatten() {
            if in_current_graph(row) {
                continue;
            }
            if value_in(row, "id", &scope.block_ids)
                || value_in(row, "parent_page_id", &scope.page_ids)
                || value_in(row, "parent_block_id", &scope.block_ids)
                || value_in(row, "parent_data_source_id", &scope.data_source_ids)
            {
                insert_row_id(&mut scope.page_ids, row);
            }
        }
        let after = (
            scope.page_ids.len(),
            scope.block_ids.len(),
            scope.database_ids.len(),
            scope.data_source_ids.len(),
        );
        if before == after {
            break;
        }
    }
    for row in rows
        .get("notes_data_source_templates")
        .into_iter()
        .flatten()
    {
        if value_in(row, "data_source_id", &scope.data_source_ids) {
            insert_row_id(&mut scope.template_ids, row);
        }
    }
    for row in rows.get("notes_comment_threads").into_iter().flatten() {
        if value_in(row, "page_id", &scope.page_ids) {
            insert_row_id(&mut scope.thread_ids, row);
        }
    }
    for row in rows.get("notes_comments").into_iter().flatten() {
        if value_in(row, "thread_id", &scope.thread_ids) {
            insert_row_id(&mut scope.comment_ids, row);
        }
    }
    scope
}

fn insert_row_id(ids: &mut HashSet<String>, row: &Value) {
    if let Some(id) = json_optional_string(row, "id") {
        ids.insert(id);
    }
}

fn value_in(row: &Value, key: &str, ids: &HashSet<String>) -> bool {
    json_optional_string(row, key).is_some_and(|id| ids.contains(&id))
}

fn row_is_in_copy_scope(table: &str, row: &Value, scope: &CopyScope) -> bool {
    match table {
        "notes_pages" => value_in(row, "id", &scope.page_ids),
        "notes_blocks" => value_in(row, "id", &scope.block_ids),
        "notes_databases" => value_in(row, "id", &scope.database_ids),
        "notes_data_sources" => value_in(row, "id", &scope.data_source_ids),
        "notes_data_source_properties" | "notes_data_source_property_options" => {
            value_in(row, "data_source_id", &scope.data_source_ids)
        }
        "notes_data_source_row_hierarchy" => value_in(row, "row_page_id", &scope.page_ids),
        "notes_database_views" => {
            value_in(row, "database_id", &scope.database_ids)
                || value_in(row, "data_source_id", &scope.data_source_ids)
        }
        "notes_data_source_templates" => value_in(row, "id", &scope.template_ids),
        "notes_data_source_template_blocks" => value_in(row, "template_id", &scope.template_ids),
        "notes_page_aliases" | "notes_page_notification_settings" => {
            value_in(row, "page_id", &scope.page_ids)
        }
        "notes_comment_threads" => value_in(row, "id", &scope.thread_ids),
        "notes_comments" => value_in(row, "id", &scope.comment_ids),
        "notes_comment_reactions" => value_in(row, "comment_id", &scope.comment_ids),
        "notes_comment_thread_anchors"
        | "notes_comment_thread_reads"
        | "notes_comment_thread_mutes" => value_in(row, "thread_id", &scope.thread_ids),
        "notes_suggestions" | "notes_mention_notifications" => {
            value_in(row, "page_id", &scope.page_ids)
        }
        "notes_asset_references" => {
            value_in(row, "page_id", &scope.page_ids)
                || value_in(row, "block_id", &scope.block_ids)
                || value_in(row, "data_source_id", &scope.data_source_ids)
                || value_in(row, "comment_id", &scope.comment_ids)
        }
        _ => false,
    }
}

async fn create_copy_id_map(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &BTreeMap<String, Vec<Value>>,
    scope: &CopyScope,
) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let mut reserved = HashSet::new();
    for (table, table_rows) in rows {
        for row in table_rows {
            if !row_is_in_copy_scope(table, row, scope) {
                continue;
            }
            let Some(id) = json_optional_string(row, "id") else {
                continue;
            };
            if map.contains_key(&id) {
                continue;
            }
            let replacement = writes::new_note_id(tx, &mut reserved).await?;
            map.insert(id, replacement);
        }
    }
    Ok(map)
}

/// Points every bundle row at the copies, so restored rows that mention or nest under a copied
/// row reference the copy rather than the original.
fn remap_copy_rows(rows: &mut BTreeMap<String, Vec<Value>>, id_map: &HashMap<String, String>) {
    if id_map.is_empty() {
        return;
    }
    for table_rows in rows.values_mut() {
        for row in table_rows {
            remap_value(row, id_map);
        }
    }
}

/// Rederives the `sync_key` of rows whose key follows their identity, so a copied row never
/// reuses the key of the original it was copied from.
fn refresh_derived_sync_keys(rows: &mut BTreeMap<String, Vec<Value>>) -> Result<(), String> {
    for table in crate::sync_keys::DERIVED_KEY_TABLES {
        for row in rows.get_mut(table.table).into_iter().flatten() {
            let key = table.row_key(row)?;
            let object = row
                .as_object_mut()
                .ok_or_else(|| format!("Notes history {} row is not an object", table.table))?;
            object.insert("sync_key".to_string(), Value::String(key));
        }
    }
    Ok(())
}

async fn resolve_copied_alias_conflicts_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &mut BTreeMap<String, Vec<Value>>,
    id_map: &HashMap<String, String>,
) -> Result<(), String> {
    let copied_ids = id_map.values().collect::<HashSet<_>>();
    for row in rows.get_mut("notes_page_aliases").into_iter().flatten() {
        let Some(id) = json_optional_string(row, "id") else {
            continue;
        };
        if !copied_ids.contains(&id) {
            continue;
        }
        let Some(original_alias) = json_optional_string(row, "alias") else {
            continue;
        };
        let Some(original_normalized) = json_optional_string(row, "normalized_alias") else {
            continue;
        };
        let conflict: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM notes_page_aliases WHERE normalized_alias = ? LIMIT 1",
        )
        .bind(&original_normalized)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check restored Notes alias conflict: {e}"))?;
        if conflict.is_none() {
            continue;
        }
        let mut resolved = false;
        for suffix in 1..=99 {
            let suffix_text = if suffix == 1 {
                " (restored copy)".to_string()
            } else {
                format!(" (restored copy {suffix})")
            };
            let keep_chars = 200_usize.saturating_sub(suffix_text.chars().count());
            let prefix = original_alias.chars().take(keep_chars).collect::<String>();
            let alias = format!("{prefix}{suffix_text}");
            let Some(normalized) = links::normalized_page_alias(&alias) else {
                continue;
            };
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM notes_page_aliases WHERE normalized_alias = ? LIMIT 1",
            )
            .bind(&normalized)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("check copied Notes alias candidate: {e}"))?;
            if exists.is_some() {
                continue;
            }
            let object = row
                .as_object_mut()
                .ok_or_else(|| "restored Notes alias row is invalid".to_string())?;
            object.insert("alias".to_string(), Value::String(alias));
            object.insert("normalized_alias".to_string(), Value::String(normalized));
            resolved = true;
            break;
        }
        if !resolved {
            return Err("could not create a unique alias for a restored Notes copy".to_string());
        }
    }
    Ok(())
}

fn remap_value(value: &mut Value, id_map: &HashMap<String, String>) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = id_map.get(text) {
                *text = replacement.clone();
            }
        }
        Value::Array(values) => {
            for value in values {
                remap_value(value, id_map);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                remap_value(value, id_map);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn suppress_historical_notifications(rows: &mut BTreeMap<String, Vec<Value>>) {
    for row in rows
        .get_mut("notes_mention_notifications")
        .into_iter()
        .flatten()
    {
        if let Some(object) = row.as_object_mut() {
            object.insert("suppressed_by_history_restore".to_string(), Value::from(1));
        }
    }
}
/// Keep untrusted snapshot edges inside the graph being restored, then validate canonical ownership.
async fn validate_restored_hierarchy_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &BTreeMap<String, Vec<Value>>,
) -> Result<(), String> {
    let page_ids = ids(rows, "notes_pages");
    let source_ids = ids(rows, "notes_data_sources");
    for edge in rows
        .get("notes_data_source_row_hierarchy")
        .into_iter()
        .flatten()
    {
        if !value_in(edge, "data_source_id", &source_ids)
            || !value_in(edge, "row_page_id", &page_ids)
            || !value_in(edge, "parent_row_page_id", &page_ids)
        {
            return Err(
                "Restored sub-items must stay within the restored project graph".to_string(),
            );
        }
    }
    if crate::structure::misplaced_folder_page_count_tx(tx).await? != 0 {
        return Err("Restored folder notes must belong to their folder project".to_string());
    }
    crate::data_sources::row_hierarchy::validate_sources_tx(
        tx,
        &source_ids.into_iter().collect::<Vec<_>>(),
    )
    .await
}

fn required_row_text(row: &Value, key: &str, entity: &str) -> Result<String, String> {
    json_optional_string(row, key).ok_or_else(|| format!("restored {entity} is missing {key}"))
}

fn folder_rows_in_restore_order(rows: &[Value]) -> Result<Vec<&Value>, String> {
    let mut pending = rows.iter().collect::<Vec<_>>();
    let mut inserted_ids = HashSet::new();
    let mut ordered = Vec::with_capacity(rows.len());
    while !pending.is_empty() {
        let pending_count = pending.len();
        let mut deferred = Vec::new();
        for row in pending {
            let id = required_row_text(row, "id", "folder")?;
            let parent_id = json_optional_string(row, "parent_folder_id");
            if parent_id
                .as_ref()
                .is_some_and(|parent_id| !inserted_ids.contains(parent_id))
            {
                deferred.push(row);
                continue;
            }
            if !inserted_ids.insert(id) {
                return Err("restored Notes folder ids must be unique".to_string());
            }
            ordered.push(row);
        }
        if deferred.len() == pending_count {
            return Err(
                "restored Notes folder hierarchy has a cycle or missing parent".to_string(),
            );
        }
        pending = deferred;
    }
    Ok(ordered)
}
#[cfg(test)]
mod tests {
    use super::{folder_rows_in_restore_order, restore_version};
    use crate::project_history::create_checkpoint;
    use serde_json::json;
    use sqlx::SqlitePool;

    const PROJECT_ID: &str = "10101010-1010-4010-8010-101010101010";
    const ROOT_FOLDER_ID: &str = "20202020-2020-4020-8020-202020202020";
    const EMPTY_FOLDER_ID: &str = "30303030-3030-4030-8030-303030303030";
    const LATER_FOLDER_ID: &str = "40404040-4040-4040-8040-404040404040";
    const PAGE_ID: &str = "50505050-5050-4050-8050-505050505050";

    async fn migrated_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    #[test]
    fn folder_restore_order_places_each_parent_before_its_children() {
        let rows = vec![
            json!({ "id": "child", "parent_folder_id": "root" }),
            json!({ "id": "grandchild", "parent_folder_id": "child" }),
            json!({ "id": "root", "parent_folder_id": null }),
        ];

        let ordered = folder_rows_in_restore_order(&rows).unwrap();
        let ids = ordered
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["root", "child", "grandchild"]);
    }

    #[test]
    fn folder_restore_order_rejects_cycles_and_missing_parents() {
        let cycle = vec![
            json!({ "id": "first", "parent_folder_id": "second" }),
            json!({ "id": "second", "parent_folder_id": "first" }),
        ];
        let missing_parent = vec![json!({
            "id": "child",
            "parent_folder_id": "missing"
        })];

        assert!(folder_rows_in_restore_order(&cycle).is_err());
        assert!(folder_rows_in_restore_order(&missing_parent).is_err());
    }

    #[test]
    fn project_restore_preserves_empty_folders_and_page_folder_membership() {
        crate::test_block_on(async {
            let pool = migrated_pool().await;
            sqlx::query("INSERT INTO project_groups (id, name, order_key) VALUES ('folder-history', 'Notes', 'a1')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO projects (id, group_id, name, order_key)
                 VALUES (?, 'folder-history', 'Folder history', 'a1')",
            )
            .bind(PROJECT_ID)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO notes_folders (id, project_id, name)
                 VALUES (?, ?, 'Root')",
            )
            .bind(ROOT_FOLDER_ID)
            .bind(PROJECT_ID)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO notes_folders (id, project_id, parent_folder_id, name)
                 VALUES (?, ?, ?, 'Empty')",
            )
            .bind(EMPTY_FOLDER_ID)
            .bind(PROJECT_ID)
            .bind(ROOT_FOLDER_ID)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO notes_pages (id, parent_type, folder_id, project_id, title, properties)
                 VALUES (?, 'workspace', ?, ?, 'Folder note', json_object('title', json_object()))",
            )
            .bind(PAGE_ID)
            .bind(ROOT_FOLDER_ID)
            .bind(PROJECT_ID)
            .execute(&pool)
            .await
            .unwrap();
            let baseline = create_checkpoint(
                &pool,
                PROJECT_ID,
                "baseline",
                None,
                None,
                "Initial folder version",
            )
            .await
            .unwrap()
            .unwrap();

            sqlx::query("UPDATE notes_folders SET name = 'Renamed' WHERE id = ?")
                .bind(ROOT_FOLDER_ID)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM notes_folders WHERE id = ?")
                .bind(EMPTY_FOLDER_ID)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO notes_folders (id, project_id, name)
                 VALUES (?, ?, 'Later')",
            )
            .bind(LATER_FOLDER_ID)
            .bind(PROJECT_ID)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("UPDATE notes_pages SET folder_id = ? WHERE id = ?")
                .bind(LATER_FOLDER_ID)
                .bind(PAGE_ID)
                .execute(&pool)
                .await
                .unwrap();

            restore_version(&pool, PROJECT_ID, &baseline.id)
                .await
                .unwrap();

            let root_name: String =
                sqlx::query_scalar("SELECT name FROM notes_folders WHERE id = ?")
                    .bind(ROOT_FOLDER_ID)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let empty_parent_id: Option<String> =
                sqlx::query_scalar("SELECT parent_folder_id FROM notes_folders WHERE id = ?")
                    .bind(EMPTY_FOLDER_ID)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let later_folder_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM notes_folders WHERE id = ?")
                    .bind(LATER_FOLDER_ID)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let page_folder_id: Option<String> =
                sqlx::query_scalar("SELECT folder_id FROM notes_pages WHERE id = ?")
                    .bind(PAGE_ID)
                    .fetch_one(&pool)
                    .await
                    .unwrap();

            assert_eq!(root_name, "Root");
            assert_eq!(empty_parent_id.as_deref(), Some(ROOT_FOLDER_ID));
            assert_eq!(later_folder_count, 0);
            assert_eq!(page_folder_id.as_deref(), Some(ROOT_FOLDER_ID));
        });
    }
}
