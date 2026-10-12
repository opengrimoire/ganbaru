use super::ids::new_note_id;
use crate::assets;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub(super) async fn update_block_comment_thread_pages(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    block_id: &str,
    page_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "WITH RECURSIVE subtree(id) AS (
            SELECT id FROM notes_blocks WHERE id = ?
            UNION ALL
            SELECT notes_blocks.id
            FROM notes_blocks
            JOIN subtree ON notes_blocks.parent_block_id = subtree.id
         )
         UPDATE notes_comment_threads
         SET page_id = ?
         WHERE parent_type = 'block_id'
           AND parent_block_id IN (SELECT id FROM subtree)",
    )
    .bind(block_id)
    .bind(page_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("move notes block comment threads: {e}"))?;
    Ok(())
}

pub(super) async fn duplicate_block_comment_threads(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    duplicate_ids: &HashMap<String, String>,
    page_id: &str,
) -> Result<(), String> {
    let mut reserved_ids = HashSet::new();
    for (source_id, duplicate_id) in duplicate_ids {
        let threads = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                Option<String>,
                String,
                String,
            ),
        >(
            "SELECT id, status, resolved_at, resolved_by, created_time, last_edited_time
             FROM notes_comment_threads
             WHERE parent_type = 'block_id' AND parent_block_id = ?",
        )
        .bind(source_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| format!("load notes block comment threads: {e}"))?;
        for (thread_id, status, resolved_at, resolved_by, created_time, last_edited_time) in threads
        {
            let duplicate_thread_id = new_note_id(tx, &mut reserved_ids).await?;
            sqlx::query(
                "INSERT INTO notes_comment_threads (
                    id,
                    page_id,
                    parent_type,
                    parent_page_id,
                    parent_block_id,
                    status,
                    resolved_at,
                    resolved_by,
                    created_time,
                    last_edited_time
                 )
                 VALUES (?, ?, 'block_id', NULL, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&duplicate_thread_id)
            .bind(page_id)
            .bind(duplicate_id)
            .bind(&status)
            .bind(&resolved_at)
            .bind(&resolved_by)
            .bind(&created_time)
            .bind(&last_edited_time)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("duplicate notes block comment thread: {e}"))?;
            let comments = sqlx::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    String,
                    String,
                    Option<String>,
                    String,
                    String,
                ),
            >(
                "SELECT rich_text,
                        plain_text,
                        created_by,
                        display_name,
                        attachments,
                        deleted_at,
                        created_time,
                        last_edited_time
                 FROM notes_comments
                 WHERE thread_id = ?
                 ORDER BY created_time ASC, id ASC",
            )
            .bind(&thread_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| format!("load notes block comments: {e}"))?;
            for (
                rich_text,
                plain_text,
                created_by,
                display_name,
                attachments,
                deleted_at,
                comment_created_time,
                comment_last_edited_time,
            ) in comments
            {
                let duplicate_comment_id = new_note_id(tx, &mut reserved_ids).await?;
                sqlx::query(
                    "INSERT INTO notes_comments (
                        id,
                        thread_id,
                        rich_text,
                        plain_text,
                        created_by,
                        display_name,
                        attachments,
                        deleted_at,
                        created_time,
                        last_edited_time
                     )
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&duplicate_comment_id)
                .bind(&duplicate_thread_id)
                .bind(&rich_text)
                .bind(&plain_text)
                .bind(&created_by)
                .bind(&display_name)
                .bind(&attachments)
                .bind(&deleted_at)
                .bind(&comment_created_time)
                .bind(&comment_last_edited_time)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("duplicate notes block comment: {e}"))?;
                if deleted_at.is_none() {
                    let attachment_values: Vec<Value> = serde_json::from_str(&attachments)
                        .map_err(|e| format!("parse duplicated notes comment attachments: {e}"))?;
                    assets::sync_comment_asset_references_tx(
                        tx,
                        page_id,
                        &duplicate_comment_id,
                        &attachment_values,
                    )
                    .await?;
                }
            }
        }
    }
    Ok(())
}
