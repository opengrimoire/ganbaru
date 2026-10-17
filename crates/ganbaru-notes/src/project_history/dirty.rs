use super::*;

pub async fn mark_page_dirty_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    summary: &str,
    force_checkpoint: bool,
) -> Result<(), String> {
    let project_id = resolve_project_id_for_page_tx(tx, page_id).await?;
    let Some(project_id) = project_id else {
        return Ok(());
    };
    mark_project_dirty_tx(tx, &project_id, summary, force_checkpoint).await
}

pub async fn mark_project_dirty_tx(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
    summary: &str,
    force_checkpoint: bool,
) -> Result<(), String> {
    if effective_retention_days_tx(tx, project_id).await? == 0 {
        return Ok(());
    }
    let local_user = local_user::current_local_user_tx(tx).await?;
    let display_name = local_user::comment_display_name_json(&local_user.display_name);
    sqlx::query(
        "INSERT INTO notes_project_history_dirty (
            project_id, first_dirty_at, last_dirty_at, actor_id,
            actor_display_name, changed_note_summary, force_checkpoint
         ) VALUES (
            ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?, ?, ?, ?
         )
         ON CONFLICT(project_id) DO UPDATE SET
            last_dirty_at = excluded.last_dirty_at,
            actor_id = excluded.actor_id,
            actor_display_name = excluded.actor_display_name,
            changed_note_summary = CASE
                WHEN trim(excluded.changed_note_summary) <> ''
                THEN excluded.changed_note_summary
                ELSE notes_project_history_dirty.changed_note_summary
            END,
            force_checkpoint = MAX(
                notes_project_history_dirty.force_checkpoint,
                excluded.force_checkpoint
            )",
    )
    .bind(project_id)
    .bind(&local_user.id)
    .bind(display_name)
    .bind(summary.trim())
    .bind(i64::from(force_checkpoint))
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("mark Notes project history dirty: {e}"))?;
    Ok(())
}

pub async fn ensure_project_baseline_for_mutation(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<(), String> {
    let project_id = validate_project_id(project_id)?;
    let project_exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("check Notes history project: {e}"))?;
    if project_exists.is_none() {
        return Ok(());
    }
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_project_history_versions WHERE project_id = ? LIMIT 1",
    )
    .bind(&project_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check Notes project history baseline: {e}"))?;
    if exists.is_some() {
        return Ok(());
    }
    create_checkpoint(pool, &project_id, "baseline", None, None, "Initial version").await?;
    Ok(())
}

pub async fn ensure_page_baseline_for_mutation(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes history page baseline lookup: {e}"))?;
    let project_id = resolve_project_id_for_page_tx(&mut tx, page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes history page baseline lookup: {e}"))?;
    if let Some(project_id) = project_id {
        ensure_project_baseline_for_mutation(pool, &project_id).await?;
    }
    Ok(())
}

pub async fn ensure_data_source_baseline_for_mutation(
    pool: &SqlitePool,
    data_source_id: &str,
) -> Result<(), String> {
    let page_id: Option<String> = sqlx::query_scalar(
        "SELECT page_id
         FROM (
             SELECT database.parent_page_id AS page_id, 1 AS priority
             FROM notes_data_sources AS data_source
             JOIN notes_databases AS database ON database.id = data_source.database_id
             WHERE data_source.id = ? AND database.parent_page_id IS NOT NULL
             UNION ALL
             SELECT block.page_id AS page_id, 2 AS priority
             FROM notes_data_sources AS data_source
             JOIN notes_databases AS database ON database.id = data_source.database_id
             JOIN notes_blocks AS block ON block.id = database.id
             WHERE data_source.id = ?
             UNION ALL
             SELECT row_page.id AS page_id, 3 AS priority
             FROM notes_pages AS row_page
             WHERE row_page.parent_data_source_id = ?
         )
         ORDER BY priority
         LIMIT 1",
    )
    .bind(data_source_id)
    .bind(data_source_id)
    .bind(data_source_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("resolve Notes history baseline data source: {e}"))?;
    if let Some(page_id) = page_id {
        ensure_page_baseline_for_mutation(pool, &page_id).await?;
    }
    Ok(())
}

pub async fn ensure_parent_baseline_for_mutation(
    pool: &SqlitePool,
    parent: &NoteParent,
) -> Result<(), String> {
    match parent {
        NoteParent::Workspace { .. } => Ok(()),
        NoteParent::PageId { page_id } => ensure_page_baseline_for_mutation(pool, page_id).await,
        NoteParent::BlockId { block_id } => {
            let page_id: Option<String> =
                sqlx::query_scalar("SELECT page_id FROM notes_blocks WHERE id = ?")
                    .bind(block_id.trim())
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| format!("resolve Notes history baseline block: {e}"))?;
            if let Some(page_id) = page_id {
                ensure_page_baseline_for_mutation(pool, &page_id).await?;
            }
            Ok(())
        }
        NoteParent::DataSourceId { data_source_id } => {
            ensure_data_source_baseline_for_mutation(pool, data_source_id).await
        }
    }
}

pub async fn ensure_blocks_baseline_for_mutation(
    pool: &SqlitePool,
    block_ids: &[String],
) -> Result<(), String> {
    if block_ids.is_empty() {
        return Ok(());
    }
    let mut query = sqlx::QueryBuilder::<Sqlite>::new(
        "SELECT DISTINCT page_id FROM notes_blocks WHERE id IN (",
    );
    let mut separated = query.separated(", ");
    for block_id in block_ids {
        separated.push_bind(block_id.trim());
    }
    separated.push_unseparated(")");
    let page_ids = query
        .build_query_scalar::<String>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("resolve Notes history baseline blocks: {e}"))?;
    for page_id in page_ids {
        ensure_page_baseline_for_mutation(pool, &page_id).await?;
    }
    Ok(())
}

pub(crate) async fn resolve_project_id_for_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<Option<String>, String> {
    sqlx::query_scalar(
        "WITH RECURSIVE ancestors(id, parent_page_id, properties) AS (
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
                    page.properties
             FROM notes_pages AS page
             WHERE page.id = ?
             UNION ALL
             SELECT parent.id,
                    COALESCE(
                        parent.parent_page_id,
                        (SELECT block.page_id FROM notes_blocks AS block
                         WHERE block.id = parent.parent_block_id),
                        (SELECT COALESCE(database.parent_page_id, database_block.page_id)
                         FROM notes_data_sources AS data_source
                         JOIN notes_databases AS database ON database.id = data_source.database_id
                         LEFT JOIN notes_blocks AS database_block ON database_block.id = database.id
                         WHERE data_source.id = parent.parent_data_source_id)
                    ),
                    parent.properties
             FROM notes_pages AS parent
             JOIN ancestors AS child ON child.parent_page_id = parent.id
         )
         SELECT project.id
         FROM ancestors
         JOIN projects AS project
           ON project.id = trim(json_extract(ancestors.properties, '$.__ganbaru_project_id'))
         LIMIT 1",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("resolve Notes history project for page: {e}"))
}

pub async fn page_history_enabled_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<bool, String> {
    if let Some(project_id) = resolve_project_id_for_page_tx(tx, page_id).await? {
        return Ok(effective_retention_days_tx(tx, &project_id).await? > 0);
    }
    let retention_days: Option<i64> =
        sqlx::query_scalar("SELECT retention_days FROM notes_page_history_settings WHERE id = 1")
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load global Notes history retention: {e}"))?;
    let retention_days = retention_days.unwrap_or(DEFAULT_RETENTION_DAYS);
    validate_retention_days(retention_days)?;
    Ok(retention_days > 0)
}

pub async fn create_safety_checkpoint_for_page(
    pool: &SqlitePool,
    page_id: &str,
    reason: &str,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes safety checkpoint project lookup: {e}"))?;
    let project_id = resolve_project_id_for_page_tx(&mut tx, page_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes safety checkpoint project lookup: {e}"))?;
    if let Some(project_id) = project_id {
        create_checkpoint(pool, &project_id, reason, None, None, "Safety checkpoint").await?;
    }
    Ok(())
}

pub async fn mark_data_source_dirty_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    summary: &str,
    force_checkpoint: bool,
) -> Result<(), String> {
    let page_id: Option<String> = sqlx::query_scalar(
        "SELECT page_id
         FROM (
             SELECT database.parent_page_id AS page_id, 1 AS priority
             FROM notes_data_sources AS data_source
             JOIN notes_databases AS database ON database.id = data_source.database_id
             WHERE data_source.id = ? AND database.parent_page_id IS NOT NULL
             UNION ALL
             SELECT block.page_id AS page_id, 2 AS priority
             FROM notes_data_sources AS data_source
             JOIN notes_databases AS database ON database.id = data_source.database_id
             JOIN notes_blocks AS block ON block.id = database.id
             WHERE data_source.id = ?
             UNION ALL
             SELECT row_page.id AS page_id, 3 AS priority
             FROM notes_pages AS row_page
             WHERE row_page.parent_data_source_id = ?
         )
         ORDER BY priority
         LIMIT 1",
    )
    .bind(data_source_id)
    .bind(data_source_id)
    .bind(data_source_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("resolve Notes history page for data source: {e}"))?;
    if let Some(page_id) = page_id {
        mark_page_dirty_tx(tx, &page_id, summary, force_checkpoint).await?;
    }
    Ok(())
}
