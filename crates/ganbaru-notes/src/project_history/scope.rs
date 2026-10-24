use crate::transfers;
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug)]
pub(super) struct ProjectHistoryGraph {
    pub(super) rows_by_table: BTreeMap<String, Vec<Value>>,
    pub(super) asset_ids: Vec<String>,
    pub(super) page_count: i64,
    pub(super) active_page_count: i64,
    pub(super) archived_page_count: i64,
    pub(super) deleted_page_count: i64,
}

#[derive(Default)]
struct ProjectScope {
    folder_ids: HashSet<String>,
    page_ids: HashSet<String>,
    block_ids: HashSet<String>,
    database_ids: HashSet<String>,
    data_source_ids: HashSet<String>,
    data_source_template_ids: HashSet<String>,
    thread_ids: HashSet<String>,
    comment_ids: HashSet<String>,
    asset_ids: HashSet<String>,
}

fn text(row: &Value, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn value_in(row: &Value, key: &str, values: &HashSet<String>) -> bool {
    text(row, key).is_some_and(|value| values.contains(&value))
}

fn page_project_id(row: &Value) -> Option<&str> {
    row.get("project_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn collect_project_scope(rows: &BTreeMap<String, Vec<Value>>, project_id: &str) -> ProjectScope {
    let mut scope = ProjectScope::default();
    if let Some(folders) = rows.get("notes_folders") {
        for folder in folders {
            if text(folder, "project_id").as_deref() == Some(project_id) {
                if let Some(id) = text(folder, "id") {
                    scope.folder_ids.insert(id);
                }
            }
        }
    }
    if let Some(pages) = rows.get("notes_pages") {
        for page in pages {
            if page_project_id(page) == Some(project_id)
                || value_in(page, "folder_id", &scope.folder_ids)
            {
                if let Some(id) = text(page, "id") {
                    scope.page_ids.insert(id);
                }
            }
        }
    }

    loop {
        let before = (
            scope.page_ids.len(),
            scope.block_ids.len(),
            scope.database_ids.len(),
            scope.data_source_ids.len(),
        );
        if let Some(blocks) = rows.get("notes_blocks") {
            for block in blocks {
                if value_in(block, "page_id", &scope.page_ids) {
                    if let Some(id) = text(block, "id") {
                        scope.block_ids.insert(id);
                    }
                }
            }
        }
        if let Some(databases) = rows.get("notes_databases") {
            for database in databases {
                if value_in(database, "id", &scope.block_ids)
                    || value_in(database, "parent_page_id", &scope.page_ids)
                    || value_in(database, "parent_block_id", &scope.block_ids)
                {
                    if let Some(id) = text(database, "id") {
                        scope.database_ids.insert(id);
                    }
                }
            }
        }
        if let Some(data_sources) = rows.get("notes_data_sources") {
            for data_source in data_sources {
                if value_in(data_source, "database_id", &scope.database_ids) {
                    if let Some(id) = text(data_source, "id") {
                        scope.data_source_ids.insert(id);
                    }
                }
            }
        }
        if let Some(pages) = rows.get("notes_pages") {
            for page in pages {
                if value_in(page, "parent_page_id", &scope.page_ids)
                    || value_in(page, "parent_block_id", &scope.block_ids)
                    || value_in(page, "parent_data_source_id", &scope.data_source_ids)
                {
                    if let Some(id) = text(page, "id") {
                        scope.page_ids.insert(id);
                    }
                }
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

    if let Some(templates) = rows.get("notes_data_source_templates") {
        for template in templates {
            if value_in(template, "data_source_id", &scope.data_source_ids) {
                if let Some(id) = text(template, "id") {
                    scope.data_source_template_ids.insert(id);
                }
            }
        }
    }
    if let Some(threads) = rows.get("notes_comment_threads") {
        for thread in threads {
            if value_in(thread, "page_id", &scope.page_ids) {
                if let Some(id) = text(thread, "id") {
                    scope.thread_ids.insert(id);
                }
            }
        }
    }
    if let Some(comments) = rows.get("notes_comments") {
        for comment in comments {
            if value_in(comment, "thread_id", &scope.thread_ids) {
                if let Some(id) = text(comment, "id") {
                    scope.comment_ids.insert(id);
                }
            }
        }
    }
    if let Some(references) = rows.get("notes_asset_references") {
        for reference in references {
            let selected = value_in(reference, "page_id", &scope.page_ids)
                || value_in(reference, "block_id", &scope.block_ids)
                || value_in(reference, "data_source_id", &scope.data_source_ids)
                || value_in(reference, "comment_id", &scope.comment_ids);
            if selected {
                if let Some(asset_id) = text(reference, "asset_id") {
                    scope.asset_ids.insert(asset_id);
                }
            }
        }
    }
    scope
}

fn row_is_selected(table: &str, row: &Value, scope: &ProjectScope) -> bool {
    match table {
        "notes_folders" => value_in(row, "id", &scope.folder_ids),
        "notes_page_notification_settings" => value_in(row, "page_id", &scope.page_ids),
        "notes_pages" | "notes_page_aliases" => value_in(
            row,
            if table == "notes_pages" {
                "id"
            } else {
                "page_id"
            },
            &scope.page_ids,
        ),
        "notes_blocks" => value_in(row, "id", &scope.block_ids),
        "notes_databases" => value_in(row, "id", &scope.database_ids),
        "notes_data_sources" => value_in(row, "id", &scope.data_source_ids),
        "notes_data_source_properties" | "notes_data_source_property_options" => {
            value_in(row, "data_source_id", &scope.data_source_ids)
        }
        "notes_database_views" => {
            value_in(row, "database_id", &scope.database_ids)
                || value_in(row, "data_source_id", &scope.data_source_ids)
        }
        "notes_data_source_templates" => value_in(row, "id", &scope.data_source_template_ids),
        "notes_data_source_row_hierarchy" => {
            value_in(row, "data_source_id", &scope.data_source_ids)
        }
        "notes_page_property_values" => value_in(row, "page_id", &scope.page_ids),
        "notes_relation_edges" => {
            value_in(row, "source_page_id", &scope.page_ids)
                || value_in(row, "target_page_id", &scope.page_ids)
        }
        "notes_data_source_template_blocks" => {
            value_in(row, "template_id", &scope.data_source_template_ids)
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
            value_in(row, "asset_id", &scope.asset_ids)
                && (value_in(row, "page_id", &scope.page_ids)
                    || value_in(row, "block_id", &scope.block_ids)
                    || value_in(row, "data_source_id", &scope.data_source_ids)
                    || value_in(row, "comment_id", &scope.comment_ids))
        }
        "notes_assets" => value_in(row, "id", &scope.asset_ids),
        _ => false,
    }
}

pub(super) async fn load_project_graph(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<ProjectHistoryGraph, String> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load Notes history project: {e}"))?;
    if exists.is_none() {
        return Err("project not found".to_string());
    }
    let source = transfers::json_graph_export::project_history_source_rows(pool).await?;
    let scope = collect_project_scope(&source, project_id);
    let mut selected = BTreeMap::new();
    for (table, rows) in source {
        let table_rows = rows
            .into_iter()
            .filter(|row| row_is_selected(&table, row, &scope))
            .collect::<Vec<_>>();
        if !table_rows.is_empty() {
            selected.insert(table, table_rows);
        }
    }
    let pages = selected.get("notes_pages").cloned().unwrap_or_default();
    let active_page_count = pages
        .iter()
        .filter(|page| {
            page.get("in_trash").and_then(Value::as_i64) == Some(0)
                && page.get("archived").and_then(Value::as_i64) == Some(0)
        })
        .count() as i64;
    let archived_page_count = pages
        .iter()
        .filter(|page| page.get("archived").and_then(Value::as_i64) == Some(1))
        .count() as i64;
    let deleted_page_count = pages
        .iter()
        .filter(|page| page.get("in_trash").and_then(Value::as_i64) == Some(1))
        .count() as i64;
    let mut asset_ids = scope.asset_ids.into_iter().collect::<Vec<_>>();
    asset_ids.sort();
    Ok(ProjectHistoryGraph {
        rows_by_table: selected,
        asset_ids,
        page_count: pages.len() as i64,
        active_page_count,
        archived_page_count,
        deleted_page_count,
    })
}

#[cfg(test)]
mod tests {
    use super::{collect_project_scope, row_is_selected};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    #[test]
    fn project_scope_includes_empty_folders_and_folder_owned_pages() {
        let project_folder = json!({
            "id": "folder-a",
            "project_id": "project-a",
            "parent_folder_id": null,
            "name": "With page"
        });
        let empty_project_folder = json!({
            "id": "folder-empty",
            "project_id": "project-a",
            "parent_folder_id": "folder-a",
            "name": "Empty"
        });
        let other_project_folder = json!({
            "id": "folder-b",
            "project_id": "project-b",
            "parent_folder_id": null,
            "name": "Other"
        });
        let folder_page = json!({
            "id": "page-a",
            "folder_id": "folder-a",
            "properties": {}
        });
        let mut rows = BTreeMap::<String, Vec<Value>>::new();
        rows.insert(
            "notes_folders".to_string(),
            vec![
                project_folder.clone(),
                empty_project_folder.clone(),
                other_project_folder.clone(),
            ],
        );
        rows.insert("notes_pages".to_string(), vec![folder_page.clone()]);

        let scope = collect_project_scope(&rows, "project-a");

        assert_eq!(scope.folder_ids.len(), 2);
        assert_eq!(scope.page_ids.len(), 1);
        assert!(row_is_selected(
            "notes_folders",
            &empty_project_folder,
            &scope
        ));
        assert!(!row_is_selected(
            "notes_folders",
            &other_project_folder,
            &scope
        ));
        assert!(row_is_selected("notes_pages", &folder_page, &scope));
    }
}
