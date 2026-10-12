use super::*;

pub(super) async fn new_version_id_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<String, String> {
    let mut reserved_ids = HashSet::new();
    for _ in 0..32 {
        let id = writes::new_note_id(tx, &mut reserved_ids).await?;
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM notes_project_history_versions WHERE id = ?")
                .bind(&id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(|e| format!("check Notes project history version id: {e}"))?;
        if exists.is_none() {
            return Ok(id);
        }
    }
    Err("could not generate a Notes project history version id".to_string())
}

pub(super) async fn store_graph_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
    graph: &ProjectHistoryGraph,
) -> Result<(String, Vec<String>, i64), String> {
    let mut rows_by_table = BTreeMap::new();
    let mut bundle_hashes = HashSet::new();
    for (table, rows) in &graph.rows_by_table {
        let mut hashes = Vec::with_capacity(rows.len());
        for row in rows {
            let raw = serde_json::to_vec(row)
                .map_err(|e| format!("serialize Notes history row for {table}: {e}"))?;
            let hash = store_bundle_tx(tx, "row", &raw).await?;
            bundle_hashes.insert(hash.clone());
            hashes.push(hash);
        }
        rows_by_table.insert(table.clone(), hashes);
    }
    let manifest = ProjectHistoryManifest {
        schema_version: HISTORY_SCHEMA_VERSION,
        project_id: project_id.to_string(),
        rows_by_table,
        asset_ids: graph.asset_ids.clone(),
    };
    let raw = serde_json::to_vec(&manifest)
        .map_err(|e| format!("serialize Notes project history manifest: {e}"))?;
    let uncompressed_bytes = i64::try_from(raw.len())
        .map_err(|_| "Notes project history manifest is too large".to_string())?;
    let manifest_hash = store_bundle_tx(tx, "manifest", &raw).await?;
    let mut hashes = bundle_hashes.into_iter().collect::<Vec<_>>();
    hashes.sort();
    Ok((manifest_hash, hashes, uncompressed_bytes))
}

pub(super) async fn create_checkpoint(
    pool: &SqlitePool,
    project_id: &str,
    reason: &str,
    actor_id: Option<&str>,
    actor_display_name: Option<&str>,
    changed_note_summary: &str,
) -> Result<Option<NotesProjectHistoryVersionDto>, String> {
    create_checkpoint_after_graph_load(
        pool,
        project_id,
        reason,
        actor_id,
        actor_display_name,
        changed_note_summary,
        |_| async { Ok(()) },
    )
    .await
}

pub(super) async fn create_checkpoint_after_graph_load<F, Fut>(
    pool: &SqlitePool,
    project_id: &str,
    reason: &str,
    actor_id: Option<&str>,
    actor_display_name: Option<&str>,
    changed_note_summary: &str,
    after_graph_load: F,
) -> Result<Option<NotesProjectHistoryVersionDto>, String>
where
    F: FnOnce(SqlitePool) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let project_id = validate_project_id(project_id)?;
    let mut retention_tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes project history retention check: {e}"))?;
    if effective_retention_days_tx(&mut retention_tx, &project_id).await? == 0 {
        prune_project_history_tx(&mut retention_tx, &project_id).await?;
        retention_tx
            .commit()
            .await
            .map_err(|e| format!("commit disabled Notes project history cleanup: {e}"))?;
        return Ok(None);
    }
    retention_tx
        .commit()
        .await
        .map_err(|e| format!("commit Notes project history retention check: {e}"))?;
    let graph = load_project_graph(pool, &project_id).await?;
    after_graph_load(pool.clone()).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes project history checkpoint: {e}"))?;
    if effective_retention_days_tx(&mut tx, &project_id).await? == 0 {
        prune_project_history_tx(&mut tx, &project_id).await?;
        tx.commit()
            .await
            .map_err(|e| format!("commit disabled Notes project history cleanup: {e}"))?;
        return Ok(None);
    }
    let local_user = if actor_id.is_none() || actor_display_name.is_none() {
        Some(local_user::current_local_user_tx(&mut tx).await?)
    } else {
        None
    };
    let resolved_actor_id = actor_id
        .or_else(|| local_user.as_ref().map(|user| user.id.as_str()))
        .ok_or_else(|| "Notes history actor is unavailable".to_string())?;
    let resolved_display_name = actor_display_name
        .or_else(|| local_user.as_ref().map(|user| user.display_name.as_str()))
        .ok_or_else(|| "Notes history display name is unavailable".to_string())?;
    let (manifest_hash, bundle_hashes, manifest_bytes) =
        store_graph_tx(&mut tx, &project_id, &graph).await?;
    let latest_hash: Option<String> = sqlx::query_scalar(
        "SELECT manifest_hash
         FROM notes_project_history_versions
         WHERE project_id = ?
         ORDER BY created_time DESC, id DESC
         LIMIT 1",
    )
    .bind(&project_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("load latest Notes project history version: {e}"))?;
    if latest_hash.as_deref() == Some(&manifest_hash) {
        sqlx::query("DELETE FROM notes_project_history_dirty WHERE project_id = ?")
            .bind(&project_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("clear unchanged Notes project history dirty state: {e}"))?;
        garbage_collect_bundles_tx(&mut tx).await?;
        tx.commit()
            .await
            .map_err(|e| format!("commit unchanged Notes project checkpoint: {e}"))?;
        return Ok(None);
    }
    let version_id = new_version_id_tx(&mut tx).await?;
    let display_name_json = local_user::comment_display_name_json(resolved_display_name);
    sqlx::query(
        "INSERT INTO notes_project_history_versions (
            id, project_id, manifest_hash, reason, created_by, display_name,
            changed_note_summary, page_count, active_page_count,
            archived_page_count, deleted_page_count, manifest_uncompressed_bytes
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&version_id)
    .bind(&project_id)
    .bind(&manifest_hash)
    .bind(reason)
    .bind(resolved_actor_id)
    .bind(display_name_json)
    .bind(changed_note_summary)
    .bind(graph.page_count)
    .bind(graph.active_page_count)
    .bind(graph.archived_page_count)
    .bind(graph.deleted_page_count)
    .bind(manifest_bytes)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("insert Notes project history version: {e}"))?;
    for hash in bundle_hashes {
        sqlx::query(
            "INSERT INTO notes_project_history_bundle_references (version_id, bundle_hash)
             VALUES (?, ?)",
        )
        .bind(&version_id)
        .bind(hash)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("pin Notes project history bundle: {e}"))?;
    }
    for asset_id in &graph.asset_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO notes_project_history_asset_pins (version_id, asset_id)
             VALUES (?, ?)",
        )
        .bind(&version_id)
        .bind(asset_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("pin Notes project history asset: {e}"))?;
    }
    sqlx::query("DELETE FROM notes_project_history_dirty WHERE project_id = ?")
        .bind(&project_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear Notes project history dirty state: {e}"))?;
    prune_project_history_tx(&mut tx, &project_id).await?;
    let row = sqlx::query(
        "SELECT id, project_id, manifest_hash, reason, created_by, display_name,
                changed_note_summary, page_count, active_page_count,
                archived_page_count, deleted_page_count, created_time
         FROM notes_project_history_versions
         WHERE id = ?",
    )
    .bind(&version_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| format!("reload Notes project history version: {e}"))?;
    let version = version_from_row(&row)?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes project history checkpoint: {e}"))?;
    Ok(Some(version))
}
