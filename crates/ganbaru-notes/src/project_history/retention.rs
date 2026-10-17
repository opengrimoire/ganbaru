use super::*;

pub(super) async fn effective_retention_days_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
) -> Result<i64, String> {
    let value: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(
             notes_history_retention_days,
             (SELECT retention_days FROM notes_page_history_settings WHERE id = 1),
             ?
         )
         FROM projects
         WHERE id = ?",
    )
    .bind(DEFAULT_RETENTION_DAYS)
    .bind(project_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load effective Notes history retention: {e}"))?;
    let retention_days = value.ok_or_else(|| "project not found".to_string())?;
    validate_retention_days(retention_days)?;
    Ok(retention_days)
}

pub async fn prune_project_history_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
) -> Result<i64, String> {
    let retention_days = effective_retention_days_tx(tx, project_id).await?;
    let result = sqlx::query(
        "DELETE FROM notes_project_history_versions
         WHERE project_id = ?
           AND (
               ? = 0
               OR created_time < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' days')
           )",
    )
    .bind(project_id)
    .bind(retention_days)
    .bind(retention_days)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("prune Notes project history: {e}"))?;
    if retention_days == 0 {
        sqlx::query("DELETE FROM notes_project_history_dirty WHERE project_id = ?")
            .bind(project_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("clear disabled Notes project history dirty state: {e}"))?;
    }
    i64::try_from(result.rows_affected())
        .map_err(|_| "pruned Notes history count is too large".to_string())
}

pub(super) fn validate_retention_days(retention_days: i64) -> Result<(), String> {
    if !SUPPORTED_RETENTION_DAYS.contains(&retention_days) {
        return Err("Notes history retention must be off, 7, 30, 90, 180, or 365 days".to_string());
    }
    Ok(())
}

pub(super) async fn run_due_maintenance(pool: &SqlitePool) -> Result<(), String> {
    let maintenance_due: bool = sqlx::query_scalar(
        "SELECT last_run_at IS NULL
             OR last_run_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' hours')
         FROM notes_history_maintenance_state WHERE id = 1",
    )
    .bind(MAINTENANCE_INTERVAL_HOURS)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("check Notes history maintenance deadline: {e}"))?;
    if !maintenance_due {
        return Ok(());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes history maintenance: {e}"))?;
    crate::page_history::cleanup_history_retention_tx(&mut tx).await?;
    let project_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM projects ORDER BY id")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("list projects for Notes history maintenance: {e}"))?;
    for project_id in project_ids {
        prune_project_history_tx(&mut tx, &project_id).await?;
    }
    garbage_collect_bundles_tx(&mut tx).await?;
    sqlx::query(
        "UPDATE notes_history_maintenance_state
         SET last_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = 1",
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record Notes history maintenance: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes history maintenance: {e}"))
}

pub async fn notes_history_retention_impact(
    pool: &SqlitePool,
    project_id: Option<String>,
    retention_days: i64,
) -> Result<NotesHistoryRetentionImpactDto, String> {
    validate_retention_days(retention_days)?;
    let project_filter = project_id.as_deref().map(validate_project_id).transpose()?;
    let row = if let Some(project_id) = project_filter.as_deref() {
        sqlx::query(
            "WITH affected_versions AS (
                 SELECT id, manifest_hash
                 FROM notes_project_history_versions
                 WHERE project_id = ?
                   AND (
                       ? = 0
                       OR created_time < strftime(
                           '%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' days'
                       )
                   )
             ),
             affected_bundles AS (
                 SELECT manifest_hash AS hash FROM affected_versions
                 UNION
                 SELECT reference.bundle_hash
                 FROM notes_project_history_bundle_references AS reference
                 JOIN affected_versions AS version ON version.id = reference.version_id
             ),
             retained_bundles AS (
                 SELECT manifest_hash AS hash
                 FROM notes_project_history_versions
                 WHERE id NOT IN (SELECT id FROM affected_versions)
                 UNION
                 SELECT reference.bundle_hash
                 FROM notes_project_history_bundle_references AS reference
                 WHERE reference.version_id NOT IN (SELECT id FROM affected_versions)
             ),
             reclaimable_bundles AS (
                 SELECT hash FROM affected_bundles
                 EXCEPT SELECT hash FROM retained_bundles
             )
             SELECT (SELECT COUNT(*) FROM affected_versions) AS version_count,
                    COALESCE((
                        SELECT SUM(bundle.stored_bytes)
                        FROM notes_history_bundles AS bundle
                        JOIN reclaimable_bundles AS reclaimable ON reclaimable.hash = bundle.hash
                    ), 0) AS stored_bytes",
        )
        .bind(project_id)
        .bind(retention_days)
        .bind(retention_days)
        .fetch_one(pool)
        .await
    } else {
        sqlx::query(
            "WITH affected_versions AS (
                 SELECT version.id, version.manifest_hash
                 FROM notes_project_history_versions AS version
                 JOIN projects AS project ON project.id = version.project_id
                 WHERE project.notes_history_retention_days IS NULL
                   AND (
                       ? = 0
                       OR version.created_time < strftime(
                           '%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' days'
                       )
                   )
             ),
             affected_bundles AS (
                 SELECT manifest_hash AS hash FROM affected_versions
                 UNION
                 SELECT reference.bundle_hash
                 FROM notes_project_history_bundle_references AS reference
                 JOIN affected_versions AS version ON version.id = reference.version_id
             ),
             retained_bundles AS (
                 SELECT manifest_hash AS hash
                 FROM notes_project_history_versions
                 WHERE id NOT IN (SELECT id FROM affected_versions)
                 UNION
                 SELECT reference.bundle_hash
                 FROM notes_project_history_bundle_references AS reference
                 WHERE reference.version_id NOT IN (SELECT id FROM affected_versions)
             ),
             reclaimable_bundles AS (
                 SELECT hash FROM affected_bundles
                 EXCEPT SELECT hash FROM retained_bundles
             )
             SELECT (SELECT COUNT(*) FROM affected_versions) AS version_count,
                    COALESCE((
                        SELECT SUM(bundle.stored_bytes)
                        FROM notes_history_bundles AS bundle
                        JOIN reclaimable_bundles AS reclaimable ON reclaimable.hash = bundle.hash
                    ), 0) AS stored_bytes",
        )
        .bind(retention_days)
        .bind(retention_days)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| format!("calculate Notes history retention impact: {e}"))?;
    let page_history_row = if let Some(project_id) = project_filter.as_deref() {
        sqlx::query(
            "WITH RECURSIVE ownership(page_id, parent_page_id, project_id) AS (
                 SELECT page.id,
                        COALESCE(
                            page.parent_page_id,
                            (SELECT block.page_id FROM notes_blocks AS block
                             WHERE block.id = page.parent_block_id),
                            (SELECT COALESCE(database.parent_page_id, database_block.page_id)
                             FROM notes_data_sources AS data_source
                             JOIN notes_databases AS database ON database.id = data_source.database_id
                             LEFT JOIN notes_blocks AS database_block ON database_block.id = database.id
                             WHERE data_source.id = page.parent_data_source_id)
                        ),
                        NULLIF(trim(json_extract(properties, '$.__ganbaru_project_id')), '')
                 FROM notes_pages AS page
                 UNION ALL
                 SELECT ownership.page_id,
                        parent.parent_page_id,
                        COALESCE(
                            ownership.project_id,
                            NULLIF(trim(json_extract(parent.properties, '$.__ganbaru_project_id')), '')
                        )
                 FROM ownership
                 JOIN notes_pages AS parent ON parent.id = ownership.parent_page_id
                 WHERE ownership.project_id IS NULL
             ),
             resolved AS (
                 SELECT page_id, MAX(project_id) AS project_id
                 FROM ownership
                 GROUP BY page_id
             )
             SELECT COUNT(*) AS version_count,
                    COALESCE(SUM(
                        COALESCE(bundle.stored_bytes, length(snapshot.blocks))
                        + length(snapshot.properties)
                        + length(snapshot.title)
                        + COALESCE(length(snapshot.icon), 0)
                        + COALESCE(length(snapshot.cover), 0)
                    ), 0) AS stored_bytes
             FROM notes_page_history_snapshots AS snapshot
             JOIN resolved ON resolved.page_id = snapshot.page_id
             LEFT JOIN notes_history_bundles AS bundle
               ON bundle.hash = snapshot.block_bundle_hash
             WHERE resolved.project_id = ?
               AND (
                   ? = 0
                   OR snapshot.created_time < strftime(
                       '%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' days'
                   )
               )",
        )
        .bind(project_id)
        .bind(retention_days)
        .bind(retention_days)
        .fetch_one(pool)
        .await
    } else {
        sqlx::query(
            "WITH RECURSIVE ownership(page_id, parent_page_id, project_id) AS (
                 SELECT page.id,
                        COALESCE(
                            page.parent_page_id,
                            (SELECT block.page_id FROM notes_blocks AS block
                             WHERE block.id = page.parent_block_id),
                            (SELECT COALESCE(database.parent_page_id, database_block.page_id)
                             FROM notes_data_sources AS data_source
                             JOIN notes_databases AS database ON database.id = data_source.database_id
                             LEFT JOIN notes_blocks AS database_block ON database_block.id = database.id
                             WHERE data_source.id = page.parent_data_source_id)
                        ),
                        NULLIF(trim(json_extract(properties, '$.__ganbaru_project_id')), '')
                 FROM notes_pages AS page
                 UNION ALL
                 SELECT ownership.page_id,
                        parent.parent_page_id,
                        COALESCE(
                            ownership.project_id,
                            NULLIF(trim(json_extract(parent.properties, '$.__ganbaru_project_id')), '')
                        )
                 FROM ownership
                 JOIN notes_pages AS parent ON parent.id = ownership.parent_page_id
                 WHERE ownership.project_id IS NULL
             ),
             resolved AS (
                 SELECT page_id, MAX(project_id) AS project_id
                 FROM ownership
                 GROUP BY page_id
             )
             SELECT COUNT(*) AS version_count,
                    COALESCE(SUM(
                        COALESCE(bundle.stored_bytes, length(snapshot.blocks))
                        + length(snapshot.properties)
                        + length(snapshot.title)
                        + COALESCE(length(snapshot.icon), 0)
                        + COALESCE(length(snapshot.cover), 0)
                    ), 0) AS stored_bytes
             FROM notes_page_history_snapshots AS snapshot
             JOIN resolved ON resolved.page_id = snapshot.page_id
             JOIN projects AS project ON project.id = resolved.project_id
             LEFT JOIN notes_history_bundles AS bundle
               ON bundle.hash = snapshot.block_bundle_hash
             WHERE project.notes_history_retention_days IS NULL
               AND (
                   ? = 0
                   OR snapshot.created_time < strftime(
                       '%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' days'
                   )
               )",
        )
        .bind(retention_days)
        .bind(retention_days)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| format!("calculate Notes page history retention impact: {e}"))?;
    let project_version_count: i64 = row.try_get("version_count").map_err(|e| e.to_string())?;
    let project_stored_bytes: i64 = row.try_get("stored_bytes").map_err(|e| e.to_string())?;
    let page_history_version_count: i64 = page_history_row
        .try_get("version_count")
        .map_err(|e| e.to_string())?;
    let page_history_stored_bytes: i64 = page_history_row
        .try_get("stored_bytes")
        .map_err(|e| e.to_string())?;
    Ok(NotesHistoryRetentionImpactDto {
        version_count: project_version_count.saturating_add(page_history_version_count),
        stored_bytes: project_stored_bytes.saturating_add(page_history_stored_bytes),
    })
}

pub async fn notes_prune_project_history(
    pool: &SqlitePool,
    project_id: String,
) -> Result<i64, String> {
    let project_id = validate_project_id(&project_id)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes project history pruning: {e}"))?;
    crate::page_history::cleanup_history_retention_tx(&mut tx).await?;
    let pruned = prune_project_history_tx(&mut tx, &project_id).await?;
    garbage_collect_bundles_tx(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes project history pruning: {e}"))?;
    Ok(pruned)
}
