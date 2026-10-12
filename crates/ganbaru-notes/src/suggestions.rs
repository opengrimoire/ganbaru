use crate::models::{NoteBlockRow, NoteSuggestionCreate, NoteSuggestionDto, NoteSuggestionRow};
use crate::validation::require_uuid;
use crate::{collaboration_operations, local_user};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};

const SUGGESTION_MAX_TEXT_LENGTH: usize = 2000;
const SUGGESTION_MAX_CONTEXT_LENGTH: usize = 120;

pub async fn list_suggestions(
    pool: &SqlitePool,
    page_id: &str,
    include_decided: bool,
) -> Result<Vec<NoteSuggestionDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    ensure_active_page(pool, page_id).await?;
    let rows = if include_decided {
        sqlx::query_as::<_, NoteSuggestionRow>(
            "SELECT suggestion.*
             FROM notes_suggestions AS suggestion
             JOIN notes_blocks AS block ON block.id = suggestion.block_id
             WHERE suggestion.page_id = ?
               AND block.in_trash = 0
             ORDER BY
               CASE suggestion.status WHEN 'open' THEN 0 ELSE 1 END,
               suggestion.created_time ASC,
               suggestion.id ASC",
        )
        .bind(page_id)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, NoteSuggestionRow>(
            "SELECT suggestion.*
             FROM notes_suggestions AS suggestion
             JOIN notes_blocks AS block ON block.id = suggestion.block_id
             WHERE suggestion.page_id = ?
               AND suggestion.status = 'open'
               AND block.in_trash = 0
             ORDER BY suggestion.created_time ASC, suggestion.id ASC",
        )
        .bind(page_id)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| format!("list notes suggestions: {e}"))?;
    rows.into_iter().map(NoteSuggestionDto::new).collect()
}

pub async fn create_suggestion(
    pool: &SqlitePool,
    request: NoteSuggestionCreate,
) -> Result<NoteSuggestionDto, String> {
    validate_suggestion_create(&request)?;
    let page_id: Option<String> =
        sqlx::query_scalar("SELECT page_id FROM notes_blocks WHERE id = ? AND in_trash = 0")
            .bind(request.block_id.trim())
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load Notes suggestion history page: {e}"))?;
    if let Some(page_id) = page_id {
        crate::project_history::ensure_page_baseline_for_mutation(pool, &page_id).await?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes suggestion create: {e}"))?;
    let block = load_active_block_tx(&mut tx, request.block_id.trim()).await?;
    if !block.plain_text.contains(request.original_text.as_str()) {
        return Err("suggestion original text must exist in the block".to_string());
    }
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    let actor_display_name = local_user::comment_display_name_json(&local_user.display_name);
    sqlx::query(
        "INSERT INTO notes_suggestions (
            id,
            page_id,
            block_id,
            created_by,
            display_name,
            range_start,
            range_end,
            original_text,
            proposed_text,
            prefix_text,
            suffix_text
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(request.id.trim())
    .bind(&block.page_id)
    .bind(&block.id)
    .bind(&local_user.id)
    .bind(&actor_display_name)
    .bind(request.range_start)
    .bind(request.range_end)
    .bind(request.original_text.as_str())
    .bind(request.proposed_text.as_str())
    .bind(request.prefix.as_str())
    .bind(request.suffix.as_str())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes suggestion: {e}"))?;
    let row = load_suggestion_row_tx(&mut tx, request.id.trim()).await?;
    collaboration_operations::record_tx(
        &mut tx,
        collaboration_operations::CollaborationOperation {
            entity_type: "suggestion",
            entity_id: &row.id,
            operation_type: "suggestion_create",
            page_id: &row.page_id,
            block_id: Some(&row.block_id),
            actor_id: &local_user.id,
            actor_display_name: &actor_display_name,
            base_version: 0,
            entity_version: row.sync_version,
            conflict_policy: "append_only",
            payload: suggestion_payload(&row)?,
        },
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes suggestion create: {e}"))?;
    NoteSuggestionDto::new(row)
}

pub async fn accept_suggestion(
    pool: &SqlitePool,
    suggestion_id: &str,
) -> Result<NoteSuggestionDto, String> {
    decide_suggestion(pool, suggestion_id, "accepted").await
}

pub async fn reject_suggestion(
    pool: &SqlitePool,
    suggestion_id: &str,
) -> Result<NoteSuggestionDto, String> {
    decide_suggestion(pool, suggestion_id, "rejected").await
}

async fn decide_suggestion(
    pool: &SqlitePool,
    suggestion_id: &str,
    status: &'static str,
) -> Result<NoteSuggestionDto, String> {
    let suggestion_id = suggestion_id.trim();
    require_uuid(suggestion_id, "suggestion_id")?;
    let page_id: Option<String> =
        sqlx::query_scalar("SELECT page_id FROM notes_suggestions WHERE id = ?")
            .bind(suggestion_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load Notes suggestion history page: {e}"))?;
    if let Some(page_id) = page_id {
        crate::project_history::ensure_page_baseline_for_mutation(pool, &page_id).await?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes suggestion decision: {e}"))?;
    let suggestion = load_suggestion_row_tx(&mut tx, suggestion_id).await?;
    if suggestion.status != "open" {
        return Err("suggestion is already decided".to_string());
    }
    load_active_block_tx(&mut tx, &suggestion.block_id).await?;
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    let actor_display_name = local_user::comment_display_name_json(&local_user.display_name);
    if status == "accepted" {
        sqlx::query(
            "UPDATE notes_suggestions
             SET status = 'accepted',
                 accepted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 accepted_by = ?,
                 sync_version = sync_version + 1,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(&local_user.id)
        .bind(suggestion_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("accept notes suggestion: {e}"))?;
    } else {
        sqlx::query(
            "UPDATE notes_suggestions
             SET status = 'rejected',
                 rejected_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 rejected_by = ?,
                 sync_version = sync_version + 1,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(&local_user.id)
        .bind(suggestion_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("reject notes suggestion: {e}"))?;
    }
    let row = load_suggestion_row_tx(&mut tx, suggestion_id).await?;
    collaboration_operations::record_tx(
        &mut tx,
        collaboration_operations::CollaborationOperation {
            entity_type: "suggestion",
            entity_id: &row.id,
            operation_type: if status == "accepted" {
                "suggestion_accept"
            } else {
                "suggestion_reject"
            },
            page_id: &row.page_id,
            block_id: Some(&row.block_id),
            actor_id: &local_user.id,
            actor_display_name: &actor_display_name,
            base_version: suggestion.sync_version,
            entity_version: row.sync_version,
            conflict_policy: "state_transition",
            payload: suggestion_payload(&row)?,
        },
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes suggestion decision: {e}"))?;
    NoteSuggestionDto::new(row)
}

fn validate_suggestion_create(request: &NoteSuggestionCreate) -> Result<(), String> {
    require_uuid(request.id.trim(), "id")?;
    require_uuid(request.block_id.trim(), "block_id")?;
    if request.range_start < 0 || request.range_end <= request.range_start {
        return Err("suggestion range is invalid".to_string());
    }
    if request.original_text.trim().is_empty() {
        return Err("suggestion original text is required".to_string());
    }
    if request.original_text.chars().count() > SUGGESTION_MAX_TEXT_LENGTH {
        return Err("suggestion original text is too long".to_string());
    }
    if request.proposed_text.chars().count() > SUGGESTION_MAX_TEXT_LENGTH {
        return Err("suggestion proposed text is too long".to_string());
    }
    if request.prefix.chars().count() > SUGGESTION_MAX_CONTEXT_LENGTH
        || request.suffix.chars().count() > SUGGESTION_MAX_CONTEXT_LENGTH
    {
        return Err("suggestion context is too long".to_string());
    }
    Ok(())
}

fn suggestion_payload(row: &NoteSuggestionRow) -> Result<Value, String> {
    Ok(json!({
        "schema_version": 1,
        "suggestion_id": &row.id,
        "page_id": &row.page_id,
        "block_id": &row.block_id,
        "status": &row.status,
        "range_start": row.range_start,
        "range_end": row.range_end,
        "original_text": &row.original_text,
        "proposed_text": &row.proposed_text,
        "prefix": &row.prefix_text,
        "suffix": &row.suffix_text,
        "created_by": &row.created_by,
        "display_name": parse_stored_json(&row.display_name)?,
        "accepted_at": row.accepted_at.as_deref(),
        "accepted_by": row.accepted_by.as_deref(),
        "rejected_at": row.rejected_at.as_deref(),
        "rejected_by": row.rejected_by.as_deref(),
        "created_time": &row.created_time,
        "last_edited_time": &row.last_edited_time,
    }))
}

fn parse_stored_json(value: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse suggestion display name: {e}"))
}

async fn load_suggestion_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    suggestion_id: &str,
) -> Result<NoteSuggestionRow, String> {
    sqlx::query_as::<_, NoteSuggestionRow>("SELECT * FROM notes_suggestions WHERE id = ?")
        .bind(suggestion_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes suggestion: {e}"))?
        .ok_or_else(|| "notes suggestion not found".to_string())
}

async fn load_active_block_tx(
    tx: &mut Transaction<'_, Sqlite>,
    block_id: &str,
) -> Result<NoteBlockRow, String> {
    sqlx::query_as::<_, NoteBlockRow>(
        "SELECT
           block.id,
           block.page_id,
           block.parent_type,
           block.parent_page_id,
           block.parent_block_id,
           block.has_children,
           block.in_trash,
           block.type AS block_type,
           block.payload,
           block.plain_text,
           block.sort_order,
           block.source_provider,
           block.source_object_id,
           block.source_last_edited_time,
           block.created_time,
           block.last_edited_time
         FROM notes_blocks AS block
         JOIN notes_pages AS page ON page.id = block.page_id
         WHERE block.id = ?
           AND block.in_trash = 0
           AND page.in_trash = 0
           AND page.archived = 0",
    )
    .bind(block_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes suggestion block: {e}"))?
    .ok_or_else(|| "notes block not found".to_string())
}

async fn ensure_active_page(pool: &SqlitePool, page_id: &str) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check notes page for suggestions: {e}"))?;
    if exists.is_some() {
        Ok(())
    } else {
        Err("notes page not found".to_string())
    }
}
