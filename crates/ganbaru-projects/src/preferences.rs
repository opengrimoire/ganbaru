use sqlx::SqlitePool;

use super::models::{ProjectViewPreferenceUpsert, ProjectsMutationRows};
use super::mutations::{ensure_project_exists_in_pool, view_preference_mutation};
use super::validation::validate_view_preference;

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
