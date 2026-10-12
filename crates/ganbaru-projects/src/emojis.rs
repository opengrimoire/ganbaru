use sqlx::SqlitePool;

use super::models::{ProjectCustomEmojiCreate, ProjectMutationRemoval, ProjectsMutationRows};
use super::mutations::custom_emoji_mutation;
use super::validation::{require_non_empty, validate_custom_emoji_create};

pub async fn create_custom_emoji(
    pool: &SqlitePool,
    emoji: ProjectCustomEmojiCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_emoji_create(&emoji)?;
    insert_project_custom_emoji(pool, &emoji).await?;
    custom_emoji_mutation(pool, emoji.id.trim()).await
}

pub(super) async fn insert_project_custom_emoji(
    pool: &sqlx::SqlitePool,
    emoji: &ProjectCustomEmojiCreate,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO project_custom_emojis (id, name, asset_path, sort_order)
         VALUES (?, ?, ?, ?)",
    )
    .bind(emoji.id.trim())
    .bind(emoji.name.trim())
    .bind(emoji.asset_path.trim())
    .bind(emoji.sort_order)
    .execute(pool)
    .await
    .map_err(|e| format!("create project custom emoji: {e}"))?;
    Ok(())
}

pub async fn delete_custom_emoji(
    pool: &SqlitePool,
    emoji_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&emoji_id, "emoji_id")?;
    delete_project_custom_emoji(pool, &emoji_id).await?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.removals.push(ProjectMutationRemoval::CustomEmoji {
        id: emoji_id.trim().to_string(),
    });
    Ok(mutation)
}

pub(super) async fn delete_project_custom_emoji(
    pool: &sqlx::SqlitePool,
    emoji_id: &str,
) -> Result<(), String> {
    let result = sqlx::query("DELETE FROM project_custom_emojis WHERE id = ?")
        .bind(emoji_id.trim())
        .execute(pool)
        .await
        .map_err(|e| format!("delete project custom emoji: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project custom emoji not found".to_string());
    }
    Ok(())
}
