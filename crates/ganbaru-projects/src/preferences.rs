use sqlx::SqlitePool;

use super::keys::view_preference_key;
use super::models::{ProjectViewPreferenceUpsert, ProjectsMutationRows};
use super::mutations::view_preference_mutation;
use super::validation::validate_view_preference;

/// Saves one preference of a project view, such as its List columns.
pub async fn upsert_view_preference(
    pool: &SqlitePool,
    preference: ProjectViewPreferenceUpsert,
) -> Result<ProjectsMutationRows, String> {
    validate_view_preference(&preference)?;
    let view_id = preference.view_id.trim();
    let preference_key = preference.preference_key.trim();
    let view_exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_views WHERE id = ?")
        .bind(view_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("check project view: {e}"))?;
    if view_exists == 0 {
        return Err("project view not found".to_string());
    }
    sqlx::query(
        "INSERT INTO project_view_preferences (view_id, preference_key, sync_key, preference_value)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(view_id, preference_key)
         DO UPDATE SET
            preference_value = excluded.preference_value,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(view_id)
    .bind(preference_key)
    .bind(view_preference_key(view_id, preference_key)?)
    .bind(&preference.preference_value)
    .execute(pool)
    .await
    .map_err(|e| format!("save project view preference: {e}"))?;
    view_preference_mutation(pool, view_id, preference_key).await
}
