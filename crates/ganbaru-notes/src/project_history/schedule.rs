use super::*;

pub async fn mutation_result<T: Serialize>(
    pool: &SqlitePool,
    value: T,
) -> Result<NotesMutationResultDto<T>, String> {
    let next_history_checkpoint_at = next_checkpoint_at(pool).await?;
    Ok(NotesMutationResultDto {
        value,
        next_history_checkpoint_at,
    })
}

pub(super) async fn next_checkpoint_at(pool: &SqlitePool) -> Result<Option<String>, String> {
    sqlx::query_scalar(
        "SELECT MIN(
             CASE
                 WHEN force_checkpoint = 1 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHEN datetime(first_dirty_at, '+' || ? || ' minutes')
                    <= datetime(last_dirty_at, '+' || ? || ' minutes')
                 THEN strftime('%Y-%m-%dT%H:%M:%fZ', first_dirty_at, '+' || ? || ' minutes')
                 ELSE strftime('%Y-%m-%dT%H:%M:%fZ', last_dirty_at, '+' || ? || ' minutes')
             END
         )
         FROM notes_project_history_dirty",
    )
    .bind(ACTIVE_CHECKPOINT_MINUTES)
    .bind(IDLE_CHECKPOINT_MINUTES)
    .bind(ACTIVE_CHECKPOINT_MINUTES)
    .bind(IDLE_CHECKPOINT_MINUTES)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load next Notes history checkpoint deadline: {e}"))
}

#[cfg(test)]
pub(super) fn checkpoint_is_due(
    first_dirty_seconds_ago: i64,
    last_dirty_seconds_ago: i64,
    force_checkpoint: bool,
) -> bool {
    force_checkpoint
        || first_dirty_seconds_ago >= ACTIVE_CHECKPOINT_MINUTES * 60
        || last_dirty_seconds_ago >= IDLE_CHECKPOINT_MINUTES * 60
}

pub async fn notes_initialize_project_history(
    pool: &SqlitePool,
    project_id: String,
) -> Result<Option<NotesProjectHistoryVersionDto>, String> {
    initialize_project_history(pool, &project_id).await
}

pub(super) async fn initialize_project_history(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Option<NotesProjectHistoryVersionDto>, String> {
    let project_id = validate_project_id(project_id)?;
    let has_content: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM notes_folders WHERE project_id = ?
             UNION ALL
             SELECT 1 FROM notes_pages
             WHERE trim(json_extract(properties, '$.__ganbaru_project_id')) = ?
         )",
    )
    .bind(&project_id)
    .bind(&project_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("check Notes project history content: {e}"))?;
    if !has_content {
        return Ok(None);
    }
    ensure_project_baseline_for_mutation(pool, &project_id).await?;
    let row = sqlx::query(
        "SELECT id, project_id, manifest_hash, reason, created_by, display_name,
                changed_note_summary, page_count, active_page_count,
                archived_page_count, deleted_page_count, created_time
         FROM notes_project_history_versions
         WHERE project_id = ?
         ORDER BY created_time ASC, id ASC LIMIT 1",
    )
    .bind(&project_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load Notes project history baseline: {e}"))?;
    row.as_ref().map(version_from_row).transpose()
}

pub async fn notes_flush_due_project_history(
    pool: &SqlitePool,
) -> Result<NotesProjectHistoryScheduleDto, String> {
    flush_due_checkpoints(pool).await
}

pub async fn flush_due_checkpoints(
    pool: &SqlitePool,
) -> Result<NotesProjectHistoryScheduleDto, String> {
    let rows = sqlx::query(
        "SELECT project_id, actor_id, actor_display_name, changed_note_summary
         FROM notes_project_history_dirty
         WHERE force_checkpoint = 1
            OR first_dirty_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' minutes')
            OR last_dirty_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-' || ? || ' minutes')
         ORDER BY first_dirty_at, project_id",
    )
    .bind(ACTIVE_CHECKPOINT_MINUTES)
    .bind(IDLE_CHECKPOINT_MINUTES)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list due Notes project history checkpoints: {e}"))?;
    let mut created = 0_i64;
    for row in rows {
        let display_name_json: String = row
            .try_get("actor_display_name")
            .map_err(|e| e.to_string())?;
        let display_name: Value = serde_json::from_str(&display_name_json)
            .map_err(|e| format!("parse Notes history dirty actor: {e}"))?;
        let resolved_name = display_name
            .get("resolved_name")
            .and_then(Value::as_str)
            .unwrap_or("You");
        let project_id: String = row.try_get("project_id").map_err(|e| e.to_string())?;
        let actor_id: String = row.try_get("actor_id").map_err(|e| e.to_string())?;
        let summary: String = row
            .try_get("changed_note_summary")
            .map_err(|e| e.to_string())?;
        let version = create_checkpoint(
            pool,
            &project_id,
            "checkpoint",
            Some(&actor_id),
            Some(resolved_name),
            &summary,
        )
        .await?;
        if version.is_some() {
            created += 1;
        }
    }
    run_due_maintenance(pool).await?;
    history_schedule(pool, created).await
}

pub(super) async fn history_schedule(
    pool: &SqlitePool,
    created_count: i64,
) -> Result<NotesProjectHistoryScheduleDto, String> {
    let next_checkpoint_at = next_checkpoint_at(pool).await?;
    let next_maintenance_at: String = sqlx::query_scalar(
        "SELECT CASE
             WHEN last_run_at IS NULL THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             ELSE strftime('%Y-%m-%dT%H:%M:%fZ', last_run_at, '+' || ? || ' hours')
         END
         FROM notes_history_maintenance_state WHERE id = 1",
    )
    .bind(MAINTENANCE_INTERVAL_HOURS)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load next Notes history maintenance deadline: {e}"))?;
    Ok(NotesProjectHistoryScheduleDto {
        created_count,
        next_checkpoint_at,
        next_maintenance_at,
    })
}
