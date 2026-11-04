use std::path::Path;

use sqlx::SqlitePool;

use super::icons::required_asset_byte_size;
use super::models::{ProjectCustomEmojiCreate, ProjectMutationRemoval, ProjectsMutationRows};
use super::mutations::custom_emoji_mutation;
use super::order::OrderGroup;
use super::validation::{require_non_empty, validate_custom_emoji_create};

/// Creates a custom emoji whose icon file already exists under the vault at `vault_root`.
pub async fn create_custom_emoji(
    pool: &SqlitePool,
    vault_root: &Path,
    emoji: ProjectCustomEmojiCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_custom_emoji_create(&emoji)?;
    insert_project_custom_emoji(pool, vault_root, &emoji).await?;
    custom_emoji_mutation(pool, emoji.id.trim()).await
}

pub(super) async fn insert_project_custom_emoji(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    emoji: &ProjectCustomEmojiCreate,
) -> Result<(), String> {
    let asset_byte_size = required_asset_byte_size(vault_root, emoji.asset_path.trim())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin project custom emoji create: {e}"))?;
    let order_key = OrderGroup::new("project_custom_emojis", "order_key")
        .place(&mut tx, emoji.id.trim(), &emoji.placement)
        .await?;
    sqlx::query(
        "INSERT INTO project_custom_emojis (id, name, asset_path, asset_byte_size, order_key)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(emoji.id.trim())
    .bind(emoji.name.trim())
    .bind(emoji.asset_path.trim())
    .bind(asset_byte_size)
    .bind(order_key.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project custom emoji: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit project custom emoji create: {e}"))
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
