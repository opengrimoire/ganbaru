use sqlx::SqlitePool;
use std::fs;
use std::path::Path;

use super::models::{
    ProjectCreate, ProjectPriorityRow, ProjectSectionRow, ProjectStatusRow, ProjectUpdate,
    ProjectsMutationRows,
};
use super::mutations::{
    normalized_optional_identifier, normalized_optional_text, project_mutation,
};
use super::routine::{ROUTINE_GROUP_ID, built_in_routine_project};
use super::templates::{
    insert_default_priorities, insert_default_statuses, insert_template_sections,
};
use super::validation::{
    require_non_empty, validate_enum, validate_project_create, validate_project_update,
};

const NOTES_PAGE_OPEN_MODES: &[&str] = &["center", "side", "full"];
const NOTES_HISTORY_RETENTION_DAYS: &[i64] = &[0, 7, 30, 90, 180, 365];

/// Creates a project with its template rows and managed working folder under `vault_root`.
pub async fn create_project(
    pool: &SqlitePool,
    vault_root: &Path,
    project: ProjectCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_project_create(&project)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    sqlx::query(
        "INSERT INTO projects (
            id, group_id, name, icon, color, sort_order,
            default_event_name, default_event_time_mode,
            default_event_duration_minutes,
            default_pomodoro_mode, default_pomodoro_preset_key,
            default_pomodoro_focus_minutes, default_pomodoro_short_break_minutes,
            default_pomodoro_long_break_minutes, default_pomodoro_long_break_after_focus_count,
            default_idle_settings_source, default_idle_pause_enabled, default_idle_threshold_minutes
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&project.id)
    .bind(&project.group_id)
    .bind(project.name.trim())
    .bind(project.icon.trim())
    .bind(project.color)
    .bind(project.sort_order)
    .bind(normalized_optional_text(
        project.default_event_name.as_deref(),
    ))
    .bind(&project.default_event_time_mode)
    .bind(project.default_event_duration_minutes)
    .bind(&project.default_pomodoro_mode)
    .bind(&project.default_pomodoro_preset_key)
    .bind(project.default_pomodoro_focus_minutes)
    .bind(project.default_pomodoro_short_break_minutes)
    .bind(project.default_pomodoro_long_break_minutes)
    .bind(project.default_pomodoro_long_break_after_focus_count)
    .bind(&project.default_idle_settings_source)
    .bind(if project.default_idle_pause_enabled {
        1_i64
    } else {
        0_i64
    })
    .bind(project.default_idle_threshold_minutes)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project: {e}"))?;

    let managed_folder_id = format!("working-folder:{}", project.id);
    let managed_relative_path = format!("projects/{}", project.id);
    sqlx::query(
        "INSERT INTO project_working_folders
            (id, project_id, display_name, kind, managed_relative_path, sort_order)
         VALUES (?, ?, ?, 'managed', ?, 0)",
    )
    .bind(&managed_folder_id)
    .bind(&project.id)
    .bind(project.name.trim())
    .bind(&managed_relative_path)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create managed project working folder: {e}"))?;

    insert_template_sections(&mut tx, &project.id, &project.template_id).await?;
    insert_default_statuses(&mut tx, &project.id).await?;
    insert_default_priorities(&mut tx, &project.id).await?;

    let managed_folder_path = vault_root.join(&managed_relative_path);
    fs::create_dir_all(&managed_folder_path)
        .map_err(|e| format!("create managed project working folder: {e}"))?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = project_mutation(pool, &project.id).await?;
    mutation.sections = sqlx::query_as::<_, ProjectSectionRow>(
        "SELECT * FROM project_sections WHERE project_id = ? ORDER BY sort_order, id",
    )
    .bind(&project.id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load created project sections: {e}"))?;
    mutation.statuses = sqlx::query_as::<_, ProjectStatusRow>(
        "SELECT * FROM project_statuses WHERE project_id = ? ORDER BY sort_order, id",
    )
    .bind(&project.id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load created project statuses: {e}"))?;
    mutation.priorities = sqlx::query_as::<_, ProjectPriorityRow>(
        "SELECT * FROM project_priorities WHERE project_id = ? ORDER BY sort_order, id",
    )
    .bind(&project.id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load created project priorities: {e}"))?;
    Ok(mutation)
}

pub async fn update_project(
    pool: &SqlitePool,
    project: ProjectUpdate,
) -> Result<ProjectsMutationRows, String> {
    validate_project_update(&project)?;
    let built_in = built_in_routine_project(&project.id);
    let group_id = built_in
        .map(|_| ROUTINE_GROUP_ID)
        .unwrap_or(project.group_id.as_str());
    let name = built_in
        .map(|default| default.name)
        .unwrap_or_else(|| project.name.trim());
    let sort_order = built_in
        .map(|default| default.sort_order)
        .unwrap_or(project.sort_order);
    let mut transaction = pool
        .begin()
        .await
        .map_err(|e| format!("begin project update: {e}"))?;
    let result = sqlx::query(
        "UPDATE projects
         SET group_id = ?,
             name = ?,
             icon = ?,
             color = ?,
             sort_order = ?,
             status = ?,
             default_event_name = ?,
             default_event_time_mode = ?,
             default_event_duration_minutes = ?,
             default_pomodoro_mode = ?,
             default_pomodoro_preset_key = ?,
             default_pomodoro_focus_minutes = ?,
             default_pomodoro_short_break_minutes = ?,
             default_pomodoro_long_break_minutes = ?,
             default_pomodoro_long_break_after_focus_count = ?,
             default_idle_settings_source = ?,
             default_idle_pause_enabled = ?,
             default_idle_threshold_minutes = ?,
             focus_playlist_id = ?,
             break_playlist_id = ?,
             work_environment_id = ?,
             blocker_ruleset_id = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(group_id)
    .bind(name)
    .bind(project.icon.trim())
    .bind(project.color)
    .bind(sort_order)
    .bind(&project.status)
    .bind(normalized_optional_text(
        project.default_event_name.as_deref(),
    ))
    .bind(&project.default_event_time_mode)
    .bind(project.default_event_duration_minutes)
    .bind(&project.default_pomodoro_mode)
    .bind(&project.default_pomodoro_preset_key)
    .bind(project.default_pomodoro_focus_minutes)
    .bind(project.default_pomodoro_short_break_minutes)
    .bind(project.default_pomodoro_long_break_minutes)
    .bind(project.default_pomodoro_long_break_after_focus_count)
    .bind(&project.default_idle_settings_source)
    .bind(if project.default_idle_pause_enabled {
        1_i64
    } else {
        0_i64
    })
    .bind(project.default_idle_threshold_minutes)
    .bind(normalized_optional_identifier(
        project.focus_playlist_id.as_deref(),
    ))
    .bind(normalized_optional_identifier(
        project.break_playlist_id.as_deref(),
    ))
    .bind(normalized_optional_identifier(
        project.work_environment_id.as_deref(),
    ))
    .bind(normalized_optional_identifier(
        project.blocker_ruleset_id.as_deref(),
    ))
    .bind(&project.id)
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("update project: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project not found".to_string());
    }
    if let (Some(assignments), Some(updated_at)) = (
        project.music_assignments,
        project.music_assignments_updated_at,
    ) {
        ganbaru_music::assignments::replace_assignments_in_transaction(
            &mut transaction,
            ganbaru_music::assignments::MusicAssignmentOwnerKind::ProjectDefault,
            &project.id,
            assignments,
            updated_at,
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    transaction
        .commit()
        .await
        .map_err(|e| format!("commit project update: {e}"))?;
    project_mutation(pool, &project.id).await
}

pub async fn update_notes_settings(
    pool: &SqlitePool,
    project_id: String,
    notes_default_open_mode: Option<String>,
    notes_history_retention_days: Option<i64>,
) -> Result<ProjectsMutationRows, String> {
    let project_id = project_id.trim();
    require_non_empty(project_id, "project_id")?;
    if let Some(value) = notes_default_open_mode.as_deref() {
        validate_enum(value, "notes_default_open_mode", NOTES_PAGE_OPEN_MODES)?;
    }
    if let Some(days) = notes_history_retention_days {
        if !NOTES_HISTORY_RETENTION_DAYS.contains(&days) {
            return Err(
                "notes_history_retention_days must be 0, 7, 30, 90, 180, 365, or null".to_string(),
            );
        }
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project Notes settings update: {e}"))?;
    let result = sqlx::query(
        "UPDATE projects
         SET notes_default_open_mode = ?,
             notes_history_retention_days = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(notes_default_open_mode)
    .bind(notes_history_retention_days)
    .bind(project_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update project Notes settings: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project not found".to_string());
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit project Notes settings update: {e}"))?;
    project_mutation(pool, project_id).await
}
