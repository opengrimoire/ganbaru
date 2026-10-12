use super::*;

pub(super) async fn load_manifest_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
    version_id: &str,
) -> Result<(ProjectHistoryManifest, NotesProjectHistoryVersionDto), String> {
    let row = sqlx::query(
        "SELECT id, project_id, manifest_hash, reason, created_by, display_name,
                changed_note_summary, page_count, active_page_count,
                archived_page_count, deleted_page_count, created_time
         FROM notes_project_history_versions
         WHERE id = ? AND project_id = ?",
    )
    .bind(version_id)
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load Notes project history version: {e}"))?
    .ok_or_else(|| "Notes project history version not found".to_string())?;
    let version = version_from_row(&row)?;
    let raw = load_bundle_tx(tx, &version.manifest_hash).await?;
    let manifest: ProjectHistoryManifest = serde_json::from_slice(&raw)
        .map_err(|e| format!("parse Notes project history manifest: {e}"))?;
    if manifest.schema_version != HISTORY_SCHEMA_VERSION || manifest.project_id != project_id {
        return Err("Notes project history manifest metadata is invalid".to_string());
    }
    Ok((manifest, version))
}

pub(super) async fn load_manifest_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    manifest: &ProjectHistoryManifest,
) -> Result<BTreeMap<String, Vec<Value>>, String> {
    let mut rows_by_table = BTreeMap::new();
    for (table, hashes) in &manifest.rows_by_table {
        let mut rows = Vec::with_capacity(hashes.len());
        for hash in hashes {
            let raw = load_bundle_tx(tx, hash).await?;
            let row = serde_json::from_slice(&raw)
                .map_err(|e| format!("parse Notes history row for {table}: {e}"))?;
            rows.push(row);
        }
        rows_by_table.insert(table.clone(), rows);
    }
    Ok(rows_by_table)
}

pub(super) fn version_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<NotesProjectHistoryVersionDto, String> {
    let display_name_json: String = row
        .try_get("display_name")
        .map_err(|e| format!("read Notes history display name: {e}"))?;
    let display_name = serde_json::from_str(&display_name_json)
        .map_err(|e| format!("parse Notes history display name: {e}"))?;
    Ok(NotesProjectHistoryVersionDto {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        project_id: row.try_get("project_id").map_err(|e| e.to_string())?,
        manifest_hash: row.try_get("manifest_hash").map_err(|e| e.to_string())?,
        reason: row.try_get("reason").map_err(|e| e.to_string())?,
        created_by: row.try_get("created_by").map_err(|e| e.to_string())?,
        display_name,
        changed_note_summary: row
            .try_get("changed_note_summary")
            .map_err(|e| e.to_string())?,
        page_count: row.try_get("page_count").map_err(|e| e.to_string())?,
        active_page_count: row
            .try_get("active_page_count")
            .map_err(|e| e.to_string())?,
        archived_page_count: row
            .try_get("archived_page_count")
            .map_err(|e| e.to_string())?,
        deleted_page_count: row
            .try_get("deleted_page_count")
            .map_err(|e| e.to_string())?,
        created_time: row.try_get("created_time").map_err(|e| e.to_string())?,
    })
}

pub async fn notes_list_project_history_versions(
    pool: &SqlitePool,
    project_id: String,
    cursor_time: Option<String>,
    cursor_id: Option<String>,
    page_size: Option<i64>,
) -> Result<NotesProjectHistoryVersionListDto, String> {
    let project_id = validate_project_id(&project_id)?;
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let rows = sqlx::query(
        "SELECT id, project_id, manifest_hash, reason, created_by, display_name,
                changed_note_summary, page_count, active_page_count,
                archived_page_count, deleted_page_count, created_time
         FROM notes_project_history_versions
         WHERE project_id = ?
           AND (? IS NULL OR created_time < ? OR (created_time = ? AND id < ?))
         ORDER BY created_time DESC, id DESC
         LIMIT ?",
    )
    .bind(&project_id)
    .bind(cursor_time.as_deref())
    .bind(cursor_time.as_deref())
    .bind(cursor_time.as_deref())
    .bind(cursor_id.as_deref())
    .bind(page_size + 1)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list Notes project history versions: {e}"))?;
    let has_more = rows.len() > page_size as usize;
    let mut versions = rows
        .iter()
        .take(page_size as usize)
        .map(version_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let (next_cursor_time, next_cursor_id) = if has_more {
        versions
            .last()
            .map(|version| (Some(version.created_time.clone()), Some(version.id.clone())))
            .unwrap_or((None, None))
    } else {
        (None, None)
    };
    Ok(NotesProjectHistoryVersionListDto {
        versions: std::mem::take(&mut versions),
        next_cursor_time,
        next_cursor_id,
    })
}

pub async fn notes_load_project_history_tree(
    pool: &SqlitePool,
    project_id: String,
    version_id: String,
) -> Result<NotesProjectHistoryTreeDto, String> {
    let project_id = validate_project_id(&project_id)?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let (manifest, version) = load_manifest_tx(&mut tx, &project_id, &version_id).await?;
    let rows = load_manifest_rows_tx(&mut tx, &manifest).await?;
    let pages = rows
        .get("notes_pages")
        .into_iter()
        .flatten()
        .map(page_summary_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(NotesProjectHistoryTreeDto { version, pages })
}

fn page_summary_from_value(row: &Value) -> Result<NotesHistoricalPageSummaryDto, String> {
    Ok(NotesHistoricalPageSummaryDto {
        id: json_string(row, "id")?,
        title: json_string(row, "title")?,
        parent_page_id: json_optional_string(row, "parent_page_id"),
        parent_data_source_id: json_optional_string(row, "parent_data_source_id"),
        in_trash: json_bool(row, "in_trash"),
        archived: json_bool(row, "archived"),
        icon: row.get("icon").filter(|value| !value.is_null()).cloned(),
    })
}

fn json_string(row: &Value, key: &str) -> Result<String, String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("Notes history row is missing {key}"))
}

pub(super) fn json_optional_string(row: &Value, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn json_bool(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_i64)
        .is_some_and(|value| value != 0)
}

pub async fn notes_load_project_history_page(
    pool: &SqlitePool,
    project_id: String,
    version_id: String,
    page_id: String,
) -> Result<NotesHistoricalPageDto, String> {
    let project_id = validate_project_id(&project_id)?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let (manifest, _) = load_manifest_tx(&mut tx, &project_id, &version_id).await?;
    let rows = load_manifest_rows_tx(&mut tx, &manifest).await?;
    let page = rows
        .get("notes_pages")
        .into_iter()
        .flatten()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(page_id.as_str()))
        .ok_or_else(|| "Historical Notes page not found".to_string())?;
    let databases = rows_for_page(&rows, "notes_databases", "parent_page_id", &page_id);
    let database_ids = databases
        .iter()
        .filter_map(|row| json_optional_string(row, "id"))
        .collect::<HashSet<_>>();
    let data_sources = rows
        .get("notes_data_sources")
        .into_iter()
        .flatten()
        .filter(|row| {
            json_optional_string(row, "database_id").is_some_and(|id| database_ids.contains(&id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let data_source_ids = data_sources
        .iter()
        .filter_map(|row| json_optional_string(row, "id"))
        .collect::<HashSet<_>>();
    let database_views = rows
        .get("notes_database_views")
        .into_iter()
        .flatten()
        .filter(|row| {
            json_optional_string(row, "database_id").is_some_and(|id| database_ids.contains(&id))
                || json_optional_string(row, "data_source_id")
                    .is_some_and(|id| data_source_ids.contains(&id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let result = NotesHistoricalPageDto {
        id: json_string(page, "id")?,
        title: json_string(page, "title")?,
        properties: page.get("properties").cloned().unwrap_or(Value::Null),
        icon: page.get("icon").filter(|value| !value.is_null()).cloned(),
        cover: page.get("cover").filter(|value| !value.is_null()).cloned(),
        in_trash: json_bool(page, "in_trash"),
        archived: json_bool(page, "archived"),
        blocks: rows_for_page(&rows, "notes_blocks", "page_id", &page_id),
        databases,
        data_sources,
        database_views,
    };
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(result)
}

fn rows_for_page(
    rows: &BTreeMap<String, Vec<Value>>,
    table: &str,
    key: &str,
    page_id: &str,
) -> Vec<Value> {
    rows.get(table)
        .into_iter()
        .flatten()
        .filter(|row| row.get(key).and_then(Value::as_str) == Some(page_id))
        .cloned()
        .collect()
}
