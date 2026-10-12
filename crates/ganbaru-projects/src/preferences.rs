use sqlx::SqlitePool;

use super::models::{ProjectMutationRemoval, ProjectViewPreferenceUpsert, ProjectsMutationRows};
use super::mutations::{ensure_project_exists_in_pool, view_preference_mutation};
use super::validation::{require_non_empty, validate_enum, validate_view_preference};

pub async fn upsert_view_preference(
    pool: &SqlitePool,
    preference: ProjectViewPreferenceUpsert,
) -> Result<ProjectsMutationRows, String> {
    validate_view_preference(&preference)?;
    ensure_project_exists_in_pool(pool, preference.project_id.trim()).await?;
    sqlx::query(
        "INSERT INTO project_view_preferences
            (project_id, view_id, preference_key, preference_value)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(project_id, view_id, preference_key)
         DO UPDATE SET
            preference_value = excluded.preference_value,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(preference.project_id.trim())
    .bind(preference.view_id.trim())
    .bind(preference.preference_key.trim())
    .bind(&preference.preference_value)
    .execute(pool)
    .await
    .map_err(|e| format!("save project view preference: {e}"))?;
    view_preference_mutation(
        pool,
        preference.project_id.trim(),
        preference.view_id.trim(),
        preference.preference_key.trim(),
    )
    .await
}

pub async fn delete_view_preference(
    pool: &SqlitePool,
    project_id: String,
    view_id: String,
    preference_key: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&project_id, "project_id")?;
    validate_enum(
        &view_id,
        "view_id",
        &["dashboard", "list", "kanban", "calendar", "gantt"],
    )?;
    require_non_empty(&preference_key, "preference_key")?;
    let result = sqlx::query(
        "DELETE FROM project_view_preferences
         WHERE project_id = ? AND view_id = ? AND preference_key = ?",
    )
    .bind(project_id.trim())
    .bind(view_id.trim())
    .bind(preference_key.trim())
    .execute(pool)
    .await
    .map_err(|e| format!("delete project view preference: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project view preference not found".to_string());
    }
    let mut mutation = ProjectsMutationRows::default();
    mutation
        .removals
        .push(ProjectMutationRemoval::ViewPreference {
            project_id: project_id.trim().to_string(),
            view_id: view_id.trim().to_string(),
            preference_key: preference_key.trim().to_string(),
        });
    Ok(mutation)
}
