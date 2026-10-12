use crate::models::{NoteDataSourceRowPageCreate, NoteLoadedPage};
use crate::{data_sources, project_history, reads, validation::require_uuid};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

/// Canonical nesting bound, including unloaded and temporarily inactive ancestors.
pub const MAX_ROW_HIERARCHY_DEPTH: i64 = 32;

/// Window-local presentation metadata derived from durable source-owned relationships.
#[derive(Clone, Serialize)]
pub struct NoteDataSourceRowHierarchyMetadata {
    pub parent_row_page_id: Option<String>,
    pub ancestor_row_page_ids: Vec<String>,
    pub depth: i64,
    pub child_count: i64,
}

/// Move one row under another row, or detach it to the source's top level.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDataSourceRowParentUpdate {
    pub parent_row_page_id: Option<String>,
}

/// Read window-local parent metadata with exact counts across the active source.
pub async fn row_metadata_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_id: &str,
    row_ids: &[String],
) -> Result<HashMap<String, NoteDataSourceRowHierarchyMetadata>, String> {
    let rows = sqlx::query(
        "WITH RECURSIVE ancestors(row_id, ancestor_id, depth) AS (
            SELECT value, value, 0 FROM json_each(?)
            UNION ALL
            SELECT ancestors.row_id, hierarchy.parent_row_page_id, ancestors.depth + 1
            FROM ancestors
            JOIN notes_data_source_row_hierarchy AS hierarchy
              ON hierarchy.row_page_id = ancestors.ancestor_id AND hierarchy.data_source_id = ?
            JOIN notes_pages AS parent ON parent.id = hierarchy.parent_row_page_id
              AND parent.parent_type = 'data_source_id' AND parent.parent_data_source_id = ?
              AND parent.in_trash = 0 AND parent.archived = 0
            WHERE ancestors.depth < ?
         )
         SELECT ancestors.row_id, MAX(ancestors.depth) AS depth,
            (SELECT json_group_array(ancestor_id) FROM (
             SELECT chain.ancestor_id FROM ancestors AS chain
             WHERE chain.row_id = ancestors.row_id AND chain.depth > 0 ORDER BY chain.depth
            )) AS ancestor_row_page_ids,
            (SELECT hierarchy.parent_row_page_id FROM notes_data_source_row_hierarchy AS hierarchy
             JOIN notes_pages AS parent ON parent.id = hierarchy.parent_row_page_id
             WHERE hierarchy.row_page_id = ancestors.row_id AND hierarchy.data_source_id = ?
               AND parent.parent_type = 'data_source_id' AND parent.parent_data_source_id = ?
               AND parent.in_trash = 0 AND parent.archived = 0) AS parent_row_page_id,
            (SELECT COUNT(*) FROM notes_data_source_row_hierarchy AS hierarchy
             JOIN notes_pages AS child ON child.id = hierarchy.row_page_id
             WHERE hierarchy.parent_row_page_id = ancestors.row_id AND hierarchy.data_source_id = ?
               AND child.parent_type = 'data_source_id' AND child.parent_data_source_id = ?
               AND child.in_trash = 0 AND child.archived = 0) AS child_count
         FROM ancestors GROUP BY ancestors.row_id",
    )
    .bind(
        serde_json::to_string(row_ids)
            .map_err(|error| format!("encode sub-item row ids: {error}"))?,
    )
    .bind(source_id)
    .bind(source_id)
    .bind(MAX_ROW_HIERARCHY_DEPTH)
    .bind(source_id)
    .bind(source_id)
    .bind(source_id)
    .bind(source_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("read Notes row hierarchy: {error}"))?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.try_get("row_id").map_err(|error| error.to_string())?,
                NoteDataSourceRowHierarchyMetadata {
                    parent_row_page_id: row
                        .try_get("parent_row_page_id")
                        .map_err(|error| error.to_string())?,
                    ancestor_row_page_ids: serde_json::from_str(
                        &row.try_get::<String, _>("ancestor_row_page_ids")
                            .map_err(|error| error.to_string())?,
                    )
                    .map_err(|error| format!("decode Notes sub-item ancestors: {error}"))?,
                    depth: row.try_get("depth").map_err(|error| error.to_string())?,
                    child_count: row
                        .try_get("child_count")
                        .map_err(|error| error.to_string())?,
                },
            ))
        })
        .collect()
}

async fn require_active_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_id: &str,
    row_id: &str,
) -> Result<(), String> {
    require_uuid(row_id, "row_page_id")?;
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages AS page
         JOIN notes_data_sources AS source ON source.id = page.parent_data_source_id
         JOIN notes_databases AS database ON database.id = source.database_id
         WHERE page.id = ? AND page.parent_type = 'data_source_id' AND page.parent_data_source_id = ?
           AND page.in_trash = 0 AND page.archived = 0 AND source.in_trash = 0 AND database.in_trash = 0",
    ).bind(row_id).bind(source_id).fetch_optional(&mut **tx).await
        .map_err(|error| format!("check Notes sub-item ownership: {error}"))?;
    if exists.is_none() {
        return Err("Sub-items must reference active rows in the same data source".to_string());
    }
    Ok(())
}

/// Change only a source-owned hierarchy edge after bounded cycle and subtree checks.
pub(crate) async fn set_parent_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_id: &str,
    row_id: &str,
    parent_id: Option<&str>,
) -> Result<(), String> {
    require_active_row_tx(tx, source_id, row_id).await?;
    let previous: Option<String> = sqlx::query_scalar("SELECT parent_row_page_id FROM notes_data_source_row_hierarchy WHERE row_page_id = ? AND data_source_id = ?")
        .bind(row_id).bind(source_id).fetch_optional(&mut **tx).await
        .map_err(|error| format!("read Notes sub-item parent: {error}"))?;
    if previous.as_deref() == parent_id {
        return Ok(());
    }
    if let Some(parent_id) = parent_id {
        require_active_row_tx(tx, source_id, parent_id).await?;
        let ancestors = sqlx::query(
            "WITH RECURSIVE ancestors(id, depth) AS (
                SELECT ?, 0 UNION ALL
                SELECT hierarchy.parent_row_page_id, ancestors.depth + 1 FROM ancestors
                JOIN notes_data_source_row_hierarchy AS hierarchy ON hierarchy.row_page_id = ancestors.id
                 AND hierarchy.data_source_id = ? WHERE ancestors.depth <= ?
             ) SELECT id, depth FROM ancestors",
        ).bind(parent_id).bind(source_id).bind(MAX_ROW_HIERARCHY_DEPTH).fetch_all(&mut **tx).await
            .map_err(|error| format!("check Notes sub-item ancestors: {error}"))?;
        let mut parent_depth = 0;
        for ancestor in ancestors {
            let id: String = ancestor.try_get("id").map_err(|error| error.to_string())?;
            if id == row_id {
                return Err("A sub-item relationship must not create a cycle".to_string());
            }
            parent_depth = parent_depth.max(
                ancestor
                    .try_get::<i64, _>("depth")
                    .map_err(|error| error.to_string())?,
            );
        }
        let descendant_depth: i64 = sqlx::query_scalar(
            "WITH RECURSIVE descendants(id, depth) AS (
                SELECT ?, 0 UNION ALL
                SELECT hierarchy.row_page_id, descendants.depth + 1 FROM descendants
                JOIN notes_data_source_row_hierarchy AS hierarchy ON hierarchy.parent_row_page_id = descendants.id
                 AND hierarchy.data_source_id = ? WHERE descendants.depth <= ?
             ) SELECT COALESCE(MAX(depth), 0) FROM descendants",
        ).bind(row_id).bind(source_id).bind(MAX_ROW_HIERARCHY_DEPTH).fetch_one(&mut **tx).await
            .map_err(|error| format!("check Notes sub-item depth: {error}"))?;
        if parent_depth + 1 + descendant_depth > MAX_ROW_HIERARCHY_DEPTH {
            return Err(format!("Sub-item depth exceeds {MAX_ROW_HIERARCHY_DEPTH}"));
        }
        sqlx::query("INSERT INTO notes_data_source_row_hierarchy(row_page_id, data_source_id, parent_row_page_id) VALUES (?, ?, ?) ON CONFLICT(row_page_id) DO UPDATE SET data_source_id = excluded.data_source_id, parent_row_page_id = excluded.parent_row_page_id")
            .bind(row_id).bind(source_id).bind(parent_id).execute(&mut **tx).await
            .map_err(|error| format!("save Notes sub-item parent: {error}"))?;
    } else {
        sqlx::query("DELETE FROM notes_data_source_row_hierarchy WHERE row_page_id = ? AND data_source_id = ?")
            .bind(row_id).bind(source_id).execute(&mut **tx).await
            .map_err(|error| format!("remove Notes sub-item parent: {error}"))?;
    }
    project_history::mark_data_source_dirty_tx(tx, source_id, "Sub-items", false).await?;
    sqlx::query("UPDATE notes_pages SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(row_id).execute(&mut **tx).await.map_err(|error| format!("touch Notes sub-item row: {error}"))?;
    sqlx::query("UPDATE notes_data_sources SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(source_id).execute(&mut **tx).await.map_err(|error| format!("touch Notes sub-item source: {error}"))?;
    sqlx::query("UPDATE notes_databases SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = (SELECT database_id FROM notes_data_sources WHERE id = ?)")
        .bind(source_id).execute(&mut **tx).await.map_err(|error| format!("touch Notes sub-item database: {error}"))?;
    Ok(())
}

/// Atomically reparent a real database row without changing its Notes page ownership.
pub async fn set_row_parent(
    pool: &SqlitePool,
    source_id: &str,
    row_id: &str,
    update: NoteDataSourceRowParentUpdate,
) -> Result<(), String> {
    let source_id = source_id.trim();
    require_uuid(source_id, "data_source_id")?;
    project_history::ensure_data_source_baseline_for_mutation(pool, source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Notes sub-item move: {error}"))?;
    set_parent_tx(
        &mut tx,
        source_id,
        row_id,
        update.parent_row_page_id.as_deref(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit Notes sub-item move: {error}"))
}

/// Create the row, its document block, and its hierarchy relationship in one transaction.
pub async fn create_subitem(
    pool: &SqlitePool,
    source_id: &str,
    parent_id: &str,
    request: NoteDataSourceRowPageCreate,
) -> Result<NoteLoadedPage, String> {
    let source_id = source_id.trim();
    require_uuid(source_id, "data_source_id")?;
    project_history::ensure_data_source_baseline_for_mutation(pool, source_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Notes sub-item creation: {error}"))?;
    require_active_row_tx(&mut tx, source_id, parent_id).await?;
    data_sources::rows::create_data_source_row_page_tx(&mut tx, source_id, &request).await?;
    set_parent_tx(&mut tx, source_id, &request.id, Some(parent_id)).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit Notes sub-item creation: {error}"))?;
    reads::load_page(pool, &request.id).await
}

/// Copy canonical edges only when the destination child and parent retain the same source.
pub(crate) async fn copy_edges_tx(
    tx: &mut Transaction<'_, Sqlite>,
    identities: &HashMap<String, String>,
) -> Result<(), String> {
    if identities.is_empty() {
        return Ok(());
    }
    let source_ids: Vec<&String> = identities.keys().collect();
    let edges = sqlx::query("SELECT row_page_id, parent_row_page_id, data_source_id FROM notes_data_source_row_hierarchy WHERE row_page_id IN (SELECT value FROM json_each(?))")
        .bind(serde_json::to_string(&source_ids).map_err(|error| format!("encode copied sub-item ids: {error}"))?)
        .fetch_all(&mut **tx).await.map_err(|error| format!("read copied Notes sub-item edges: {error}"))?;
    let source_ids = edges
        .iter()
        .map(|edge| {
            edge.try_get::<String, _>("data_source_id")
                .map_err(|error| error.to_string())
        })
        .collect::<Result<HashSet<_>, _>>()?;
    validate_sources_tx(tx, &source_ids.into_iter().collect::<Vec<_>>()).await?;
    for edge in edges {
        let row_id: String = edge
            .try_get("row_page_id")
            .map_err(|error| error.to_string())?;
        let parent_id: String = edge
            .try_get("parent_row_page_id")
            .map_err(|error| error.to_string())?;
        let Some(new_row_id) = identities.get(&row_id) else {
            continue;
        };
        let new_parent_id = identities.get(&parent_id).unwrap_or(&parent_id);
        sqlx::query("INSERT INTO notes_data_source_row_hierarchy(row_page_id, data_source_id, parent_row_page_id)
            SELECT child.id, child.parent_data_source_id, parent.id FROM notes_pages AS child JOIN notes_pages AS parent ON parent.id = ?
            WHERE child.id = ? AND child.parent_type = 'data_source_id' AND parent.parent_type = 'data_source_id'
              AND child.parent_data_source_id = parent.parent_data_source_id")
            .bind(new_parent_id).bind(new_row_id).execute(&mut **tx).await
            .map_err(|error| format!("copy Notes sub-item relationship: {error}"))?;
    }
    let destination_ids: Vec<&String> = identities.values().collect();
    let copied_sources: Vec<String> = sqlx::query_scalar("SELECT DISTINCT data_source_id FROM notes_data_source_row_hierarchy WHERE row_page_id IN (SELECT value FROM json_each(?))")
        .bind(serde_json::to_string(&destination_ids).map_err(|error| format!("encode copied destination ids: {error}"))?)
        .fetch_all(&mut **tx).await.map_err(|error| format!("read copied sub-item sources: {error}"))?;
    validate_sources_tx(tx, &copied_sources).await?;
    Ok(())
}

/// Validate a captured or restored source graph before accepting direct canonical edge writes.
pub(crate) async fn validate_sources_tx(
    tx: &mut Transaction<'_, Sqlite>,
    source_ids: &[String],
) -> Result<(), String> {
    if source_ids.is_empty() {
        return Ok(());
    }
    let edges = sqlx::query("SELECT hierarchy.row_page_id, hierarchy.parent_row_page_id, hierarchy.data_source_id,
        child.parent_type AS child_type, child.parent_data_source_id AS child_source,
        parent.parent_type AS parent_type, parent.parent_data_source_id AS parent_source, source.id AS source_exists
        FROM notes_data_source_row_hierarchy AS hierarchy
        LEFT JOIN notes_pages AS child ON child.id = hierarchy.row_page_id
        LEFT JOIN notes_pages AS parent ON parent.id = hierarchy.parent_row_page_id
        LEFT JOIN notes_data_sources AS source ON source.id = hierarchy.data_source_id
        WHERE hierarchy.data_source_id IN (SELECT value FROM json_each(?))")
        .bind(serde_json::to_string(source_ids).map_err(|error| format!("encode validated sub-item sources: {error}"))?)
        .fetch_all(&mut **tx).await.map_err(|error| format!("read canonical Notes sub-item integrity: {error}"))?;
    let mut parents = HashMap::<String, String>::new();
    for edge in edges {
        let row_id: String = edge
            .try_get("row_page_id")
            .map_err(|error| error.to_string())?;
        let parent_id: String = edge
            .try_get("parent_row_page_id")
            .map_err(|error| error.to_string())?;
        let source_id: String = edge
            .try_get("data_source_id")
            .map_err(|error| error.to_string())?;
        require_uuid(&row_id, "sub-item row identity")?;
        require_uuid(&parent_id, "sub-item parent identity")?;
        require_uuid(&source_id, "sub-item source identity")?;
        let child_type: Option<String> = edge
            .try_get("child_type")
            .map_err(|error| error.to_string())?;
        let parent_type: Option<String> = edge
            .try_get("parent_type")
            .map_err(|error| error.to_string())?;
        let child_source: Option<String> = edge
            .try_get("child_source")
            .map_err(|error| error.to_string())?;
        let parent_source: Option<String> = edge
            .try_get("parent_source")
            .map_err(|error| error.to_string())?;
        let source_exists: Option<String> = edge
            .try_get("source_exists")
            .map_err(|error| error.to_string())?;
        if child_type.as_deref() != Some("data_source_id")
            || parent_type.as_deref() != Some("data_source_id")
            || child_source.as_deref() != Some(&source_id)
            || parent_source.as_deref() != Some(&source_id)
            || source_exists.is_none()
        {
            return Err(
                "Canonical sub-items must reference rows owned by the same existing data source"
                    .to_string(),
            );
        }
        parents.insert(row_id, parent_id);
    }
    for row_id in parents.keys() {
        let mut current = row_id.as_str();
        let mut ancestors = HashSet::new();
        let mut depth = 0;
        while let Some(parent_id) = parents.get(current) {
            if !ancestors.insert(current) {
                return Err("Canonical sub-items must not contain a cycle".to_string());
            }
            depth += 1;
            if depth > MAX_ROW_HIERARCHY_DEPTH {
                return Err(format!(
                    "Canonical sub-item depth exceeds {MAX_ROW_HIERARCHY_DEPTH}"
                ));
            }
            current = parent_id;
        }
    }
    Ok(())
}
