use crate::validation::require_uuid;
use std::collections::HashSet;

/// Reserve a unique version 4 UUID within the bounded Notes copy operation.
pub async fn new_note_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    reserved_ids: &mut HashSet<String>,
) -> Result<String, String> {
    if reserved_ids.len() >= super::database_copy::MAX_COPY_OBJECTS {
        return Err("Notes copy exceeds the object limit".to_string());
    }
    for _ in 0..32 {
        let id: String = sqlx::query_scalar(
            "SELECT lower(hex(randomblob(4))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    '4' || substr(lower(hex(randomblob(2))), 2) || '-' ||
                    substr('89ab', abs(random() % 4) + 1, 1) || substr(lower(hex(randomblob(2))), 2) || '-' ||
                    lower(hex(randomblob(6)))",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("generate notes id: {e}"))?;
        require_uuid(&id, "generated_id")?;
        if reserved_ids.contains(&id) {
            continue;
        }
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1
             WHERE EXISTS (SELECT 1 FROM notes_pages WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_blocks WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_comment_threads WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_comments WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_data_sources WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_database_views WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_data_source_templates WHERE id = ?)",
        )
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check generated notes id: {e}"))?;
        if exists.is_none() {
            reserved_ids.insert(id.clone());
            return Ok(id);
        }
    }
    Err("could not generate a unique notes id".to_string())
}
