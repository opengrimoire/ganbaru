use crate::models::{
    NoteCommentAnchorCreate, NoteCommentAnchorDto, NoteCommentAnchorRow, NoteCommentCreate,
    NoteCommentDto, NoteCommentReactionDto, NoteCommentRow, NoteCommentThreadDto,
    NoteCommentThreadReadUpdate, NoteCommentThreadRow, NoteCommentThreadViewerState,
    NoteCommentUpdate, NoteLocalUserRow, NoteParent,
};
use crate::validation::{
    require_uuid, rich_text_items_plain_text, validate_comment_body_rich_text,
    validate_comment_rich_text,
};
use crate::{assets, local_user, mention_notifications};
use serde_json::Value;
use sqlx::{QueryBuilder, Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

const COMMENT_ANCHOR_MAX_TEXT_LENGTH: usize = 2000;
const COMMENT_ANCHOR_MAX_CONTEXT_LENGTH: usize = 120;
const COMMENT_ATTACHMENTS_MAX_COUNT: usize = 100;
const COMMENT_ATTACHMENTS_MAX_BYTES: usize = 50 * 1024;
const COMMENT_THREAD_READ_MAX_BATCH: usize = 200;
/// Matches the reaction column limit so validation fails before SQLite does.
const COMMENT_REACTION_MAX_CHARS: usize = 16;
const COMMENT_REACTION_MAX_BYTES: usize = 64;
const COMMENT_REACTION_MAX_KINDS: i64 = 20;

struct CommentParentTarget {
    page_id: String,
    parent_type: &'static str,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
}

pub async fn list_comments(
    pool: &SqlitePool,
    page_id: &str,
    include_resolved: bool,
) -> Result<Vec<NoteCommentThreadDto>, String> {
    list_comments_for_blocks(pool, page_id, include_resolved, None).await
}

pub async fn list_comments_for_blocks(
    pool: &SqlitePool,
    page_id: &str,
    include_resolved: bool,
    block_ids: Option<&[String]>,
) -> Result<Vec<NoteCommentThreadDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    ensure_active_page(pool, page_id).await?;
    let mut normalized_block_ids = Vec::new();
    let mut seen = HashSet::new();
    for block_id in block_ids.unwrap_or_default() {
        require_uuid(block_id, "block_id")?;
        if seen.insert(block_id.clone()) {
            normalized_block_ids.push(block_id.clone());
        }
    }
    if normalized_block_ids.len() > 200 {
        return Err("comment block range is limited to 200 blocks".to_string());
    }
    let mut query =
        QueryBuilder::<Sqlite>::new("SELECT * FROM notes_comment_threads WHERE page_id = ");
    query.push_bind(page_id);
    if !include_resolved {
        query.push(" AND status = 'open'");
    }
    query.push(
        " AND EXISTS (
            SELECT 1 FROM notes_comments AS comment
            WHERE comment.thread_id = notes_comment_threads.id
              AND comment.deleted_at IS NULL
          ) AND (parent_type = 'page_id'",
    );
    if block_ids.is_none() {
        query.push(
            " OR EXISTS (
                SELECT 1 FROM notes_blocks AS block
                WHERE block.id = notes_comment_threads.parent_block_id
                  AND block.in_trash = 0
              )",
        );
    } else if !normalized_block_ids.is_empty() {
        query.push(" OR parent_block_id IN (");
        let mut separated = query.separated(", ");
        for block_id in &normalized_block_ids {
            separated.push_bind(block_id);
        }
        query.push(")");
    }
    query.push(") ORDER BY created_time ASC, id ASC");
    let thread_rows = query
        .build_query_as::<NoteCommentThreadRow>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("list notes comment threads: {e}"))?;
    thread_dtos(pool, thread_rows).await
}

pub async fn mark_comment_threads_read(
    pool: &SqlitePool,
    request: NoteCommentThreadReadUpdate,
) -> Result<Vec<NoteCommentThreadDto>, String> {
    let page_id = request.page_id.trim().to_string();
    require_uuid(&page_id, "page_id")?;
    if request.discussion_ids.len() > COMMENT_THREAD_READ_MAX_BATCH {
        return Err("too many comment threads to mark read at once".to_string());
    }
    let mut seen = HashSet::new();
    let mut discussion_ids = Vec::with_capacity(request.discussion_ids.len());
    for discussion_id in request.discussion_ids {
        let discussion_id = discussion_id.trim().to_string();
        require_uuid(&discussion_id, "discussion_id")?;
        if seen.insert(discussion_id.clone()) {
            discussion_ids.push(discussion_id);
        }
    }
    if discussion_ids.is_empty() {
        return list_comments(pool, &page_id, request.include_resolved.unwrap_or(false)).await;
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes comment read update: {e}"))?;
    ensure_active_page_tx(&mut tx, &page_id).await?;
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    for discussion_id in discussion_ids {
        let thread = load_thread_row(&mut tx, &discussion_id).await?;
        if thread.page_id != page_id {
            return Err("comment thread does not belong to the requested page".to_string());
        }
        ensure_thread_target_active(&mut tx, &thread).await?;
        mark_thread_read_for_user_tx(&mut tx, &thread.id, &local_user.id).await?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("commit notes comment read update: {e}"))?;

    list_comments(pool, &page_id, request.include_resolved.unwrap_or(false)).await
}

pub async fn create_comment(
    pool: &SqlitePool,
    request: NoteCommentCreate,
) -> Result<NoteCommentThreadDto, String> {
    require_uuid(request.id.trim(), "id")?;
    let attachments = request.attachments.clone().unwrap_or_default();
    validate_comment_body_rich_text(&request.rich_text, !attachments.is_empty())?;
    validate_comment_attachments(&attachments)?;
    match (&request.parent, &request.discussion_id) {
        (Some(_), Some(_)) => {
            return Err("provide either parent or discussion_id, not both".to_string());
        }
        (None, None) => return Err("parent or discussion_id is required".to_string()),
        _ => {}
    }
    if request.anchor.is_some() && request.discussion_id.is_some() {
        return Err("inline comment anchors can only start new block comment threads".to_string());
    }
    if let Some(parent) = request.parent.as_ref() {
        crate::project_history::ensure_parent_baseline_for_mutation(pool, parent).await?;
    } else if let Some(discussion_id) = request.discussion_id.as_deref() {
        ensure_comment_thread_baseline(pool, discussion_id).await?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes comment create: {e}"))?;
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    let thread_id = if let Some(parent) = request.parent {
        let parent = resolve_comment_parent(&mut tx, &parent).await?;
        let thread_id = new_comment_thread_id(&mut tx).await?;
        sqlx::query(
            "INSERT INTO notes_comment_threads (
                id,
                page_id,
                parent_type,
                parent_page_id,
                parent_block_id
             )
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&thread_id)
        .bind(&parent.page_id)
        .bind(parent.parent_type)
        .bind(parent.parent_page_id.as_deref())
        .bind(parent.parent_block_id.as_deref())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("create notes comment thread: {e}"))?;
        if let Some(anchor) = request.anchor.as_ref() {
            insert_comment_anchor_row(&mut tx, &thread_id, &parent, anchor).await?;
        }
        crate::project_history::mark_page_dirty_tx(
            &mut tx,
            &parent.page_id,
            "comment_thread_create",
            false,
        )
        .await?;
        thread_id
    } else {
        let thread_id = request
            .discussion_id
            .as_deref()
            .map(str::trim)
            .ok_or_else(|| "discussion_id is required".to_string())?;
        require_uuid(thread_id, "discussion_id")?;
        let row = load_thread_row(&mut tx, thread_id).await?;
        if row.status != "open" {
            return Err("resolved comment threads cannot receive replies".to_string());
        }
        thread_id.to_string()
    };
    let comment = insert_comment_row(
        &mut tx,
        request.id.trim(),
        &thread_id,
        &request.rich_text,
        &attachments,
        &local_user,
    )
    .await?;
    let thread = load_thread_row(&mut tx, &thread_id).await?;
    crate::project_history::mark_page_dirty_tx(&mut tx, &thread.page_id, "comment_create", false)
        .await?;
    assets::sync_comment_asset_references_tx(&mut tx, &thread.page_id, &comment.id, &attachments)
        .await?;
    let plain_text = rich_text_items_plain_text(&request.rich_text);
    mention_notifications::sync_comment_tx(
        &mut tx,
        request.id.trim(),
        &thread.page_id,
        thread.parent_block_id.as_deref(),
        &request.rich_text,
        &plain_text,
    )
    .await?;
    touch_thread(&mut tx, &thread_id).await?;
    mark_thread_read_for_current_user_tx(&mut tx, &thread_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes comment create: {e}"))?;
    load_thread(pool, &thread_id).await
}

pub async fn update_comment(
    pool: &SqlitePool,
    comment_id: &str,
    update: NoteCommentUpdate,
) -> Result<NoteCommentThreadDto, String> {
    let comment_id = comment_id.trim();
    require_uuid(comment_id, "comment_id")?;
    match update.attachments.as_deref() {
        Some(attachments) => {
            validate_comment_body_rich_text(&update.rich_text, !attachments.is_empty())?;
        }
        None => validate_comment_rich_text(&update.rich_text)?,
    }
    ensure_comment_baseline(pool, comment_id).await?;
    if let Some(attachments) = update.attachments.as_ref() {
        validate_comment_attachments(attachments)?;
    }
    let plain_text = rich_text_items_plain_text(&update.rich_text);
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes comment update: {e}"))?;
    let row = load_comment_row(&mut tx, comment_id).await?;
    let thread = load_thread_row(&mut tx, &row.thread_id).await?;
    if let Some(attachments) = update.attachments.as_ref() {
        sqlx::query(
            "UPDATE notes_comments
             SET rich_text = ?,
                 plain_text = ?,
                 attachments = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(Value::Array(update.rich_text.clone()).to_string())
        .bind(&plain_text)
        .bind(Value::Array(attachments.clone()).to_string())
        .bind(comment_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes comment: {e}"))?;
    } else {
        sqlx::query(
            "UPDATE notes_comments
             SET rich_text = ?,
                 plain_text = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(Value::Array(update.rich_text.clone()).to_string())
        .bind(&plain_text)
        .bind(comment_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes comment: {e}"))?;
    }
    if let Some(attachments) = update.attachments.as_ref() {
        assets::sync_comment_asset_references_tx(&mut tx, &thread.page_id, comment_id, attachments)
            .await?;
    }
    crate::project_history::mark_page_dirty_tx(&mut tx, &thread.page_id, "comment_update", false)
        .await?;
    mention_notifications::sync_comment_tx(
        &mut tx,
        comment_id,
        &thread.page_id,
        thread.parent_block_id.as_deref(),
        &update.rich_text,
        &plain_text,
    )
    .await?;
    touch_thread(&mut tx, &row.thread_id).await?;
    mark_thread_read_for_current_user_tx(&mut tx, &row.thread_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes comment update: {e}"))?;
    load_thread(pool, &row.thread_id).await
}

pub async fn delete_comment(
    pool: &SqlitePool,
    comment_id: &str,
) -> Result<NoteCommentThreadDto, String> {
    let comment_id = comment_id.trim();
    require_uuid(comment_id, "comment_id")?;
    ensure_comment_baseline(pool, comment_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes comment delete: {e}"))?;
    let row = load_comment_row(&mut tx, comment_id).await?;
    let thread = load_thread_row(&mut tx, &row.thread_id).await?;
    ensure_thread_target_active(&mut tx, &thread).await?;
    sqlx::query(
        "UPDATE notes_comments
         SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(comment_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("delete notes comment: {e}"))?;
    assets::sync_comment_asset_references_tx(&mut tx, &thread.page_id, comment_id, &[]).await?;
    crate::project_history::mark_page_dirty_tx(&mut tx, &thread.page_id, "comment_delete", false)
        .await?;
    mention_notifications::clear_source_tx(&mut tx, "comment", comment_id).await?;
    touch_thread(&mut tx, &row.thread_id).await?;
    mark_thread_read_for_current_user_tx(&mut tx, &row.thread_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes comment delete: {e}"))?;
    load_thread(pool, &row.thread_id).await
}

pub async fn resolve_comment_thread(
    pool: &SqlitePool,
    discussion_id: &str,
    resolved: bool,
) -> Result<NoteCommentThreadDto, String> {
    let discussion_id = discussion_id.trim();
    require_uuid(discussion_id, "discussion_id")?;
    ensure_comment_thread_baseline(pool, discussion_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes comment thread resolve: {e}"))?;
    let thread = load_thread_row(&mut tx, discussion_id).await?;
    ensure_thread_target_active(&mut tx, &thread).await?;
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    if resolved {
        sqlx::query(
            "UPDATE notes_comment_threads
             SET status = 'resolved',
                 resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 resolved_by = ?,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(&local_user.id)
        .bind(discussion_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("resolve notes comment thread: {e}"))?;
    } else {
        sqlx::query(
            "UPDATE notes_comment_threads
             SET status = 'open',
                 resolved_at = NULL,
                 resolved_by = NULL,
                 last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?",
        )
        .bind(discussion_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("reopen notes comment thread: {e}"))?;
    }
    let operation_type = if resolved {
        "comment_thread_resolve"
    } else {
        "comment_thread_reopen"
    };
    crate::project_history::mark_page_dirty_tx(&mut tx, &thread.page_id, operation_type, false)
        .await?;
    mark_thread_read_for_current_user_tx(&mut tx, discussion_id).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes comment thread resolve: {e}"))?;
    load_thread(pool, discussion_id).await
}

/// Adds the current person's emoji reaction to a comment, or removes it when they already chose it.
pub async fn toggle_comment_reaction(
    pool: &SqlitePool,
    comment_id: &str,
    emoji: &str,
) -> Result<NoteCommentThreadDto, String> {
    let comment_id = comment_id.trim();
    require_uuid(comment_id, "comment_id")?;
    let emoji = validate_comment_reaction_emoji(emoji)?;
    ensure_comment_baseline(pool, comment_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes comment reaction: {e}"))?;
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    let comment = load_comment_row(&mut tx, comment_id).await?;
    let thread = load_thread_row(&mut tx, &comment.thread_id).await?;
    ensure_thread_target_active(&mut tx, &thread).await?;
    if thread.status != "open" {
        return Err("resolved comment threads cannot receive reactions".to_string());
    }
    let removed = sqlx::query(
        "DELETE FROM notes_comment_reactions
         WHERE comment_id = ? AND created_by = ? AND emoji = ?",
    )
    .bind(comment_id)
    .bind(&local_user.id)
    .bind(emoji)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("remove notes comment reaction: {e}"))?
    .rows_affected();
    if removed == 0 {
        let (distinct, has_emoji): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(DISTINCT emoji), COALESCE(MAX(emoji = ?), 0)
             FROM notes_comment_reactions
             WHERE comment_id = ?",
        )
        .bind(emoji)
        .bind(comment_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("count notes comment reactions: {e}"))?;
        if has_emoji == 0 && distinct >= COMMENT_REACTION_MAX_KINDS {
            return Err(format!(
                "comments are limited to {COMMENT_REACTION_MAX_KINDS} different reactions"
            ));
        }
        sqlx::query(
            "INSERT INTO notes_comment_reactions (comment_id, created_by, emoji, sync_key)
             VALUES (?, ?, ?, ?)",
        )
        .bind(comment_id)
        .bind(&local_user.id)
        .bind(emoji)
        .bind(crate::sync_keys::COMMENT_REACTIONS.key(&[comment_id, &local_user.id, emoji])?)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("add notes comment reaction: {e}"))?;
    }
    crate::project_history::mark_page_dirty_tx(&mut tx, &thread.page_id, "comment_reaction", false)
        .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes comment reaction: {e}"))?;
    load_thread(pool, &thread.id).await
}

/// Mutes or unmutes notifications from one discussion for the current local user.
///
/// Muting is a device-local preference, so it neither changes the discussion nor marks project history dirty.
pub async fn set_comment_thread_muted(
    pool: &SqlitePool,
    discussion_id: &str,
    muted: bool,
) -> Result<NoteCommentThreadDto, String> {
    let discussion_id = discussion_id.trim();
    require_uuid(discussion_id, "discussion_id")?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes comment thread mute: {e}"))?;
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    let thread = load_thread_row(&mut tx, discussion_id).await?;
    ensure_thread_target_active(&mut tx, &thread).await?;
    let statement = if muted {
        "INSERT INTO notes_comment_thread_mutes (thread_id, user_id)
         VALUES (?, ?)
         ON CONFLICT(thread_id, user_id) DO NOTHING"
    } else {
        "DELETE FROM notes_comment_thread_mutes WHERE thread_id = ? AND user_id = ?"
    };
    sqlx::query(statement)
        .bind(discussion_id)
        .bind(&local_user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("update notes comment thread mute: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes comment thread mute: {e}"))?;
    load_thread(pool, discussion_id).await
}

/// Accepts one emoji glyph or sequence, such as a flag, keycap, or skin tone variant.
fn validate_comment_reaction_emoji(emoji: &str) -> Result<&str, String> {
    let emoji = emoji.trim();
    let valid = !emoji.is_empty()
        && emoji.len() <= COMMENT_REACTION_MAX_BYTES
        && emoji.chars().count() <= COMMENT_REACTION_MAX_CHARS
        && !emoji
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        && !emoji
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && !emoji.is_ascii();
    if valid {
        Ok(emoji)
    } else {
        Err("comment reaction must be one emoji".to_string())
    }
}

async fn ensure_comment_baseline(pool: &SqlitePool, comment_id: &str) -> Result<(), String> {
    let page_id: Option<String> = sqlx::query_scalar(
        "SELECT thread.page_id
         FROM notes_comments AS comment
         JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id
         WHERE comment.id = ?",
    )
    .bind(comment_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load Notes comment history page: {e}"))?;
    if let Some(page_id) = page_id {
        crate::project_history::ensure_page_baseline_for_mutation(pool, &page_id).await?;
    }
    Ok(())
}

async fn ensure_comment_thread_baseline(
    pool: &SqlitePool,
    discussion_id: &str,
) -> Result<(), String> {
    let page_id: Option<String> =
        sqlx::query_scalar("SELECT page_id FROM notes_comment_threads WHERE id = ?")
            .bind(discussion_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load Notes discussion history page: {e}"))?;
    if let Some(page_id) = page_id {
        crate::project_history::ensure_page_baseline_for_mutation(pool, &page_id).await?;
    }
    Ok(())
}

async fn load_thread(pool: &SqlitePool, thread_id: &str) -> Result<NoteCommentThreadDto, String> {
    let thread = sqlx::query_as::<_, NoteCommentThreadRow>(
        "SELECT * FROM notes_comment_threads WHERE id = ?",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes comment thread: {e}"))?
    .ok_or_else(|| "notes comment thread not found".to_string())?;
    let local_user_id = current_local_user_id(pool).await?;
    let local_author_name = local_user::local_author_name_from_pool(pool).await?;
    thread_dto(pool, thread, &local_user_id, &local_author_name).await
}

async fn thread_dto(
    pool: &SqlitePool,
    thread: NoteCommentThreadRow,
    local_user_id: &str,
    local_author_name: &str,
) -> Result<NoteCommentThreadDto, String> {
    let comments = load_comment_dtos(pool, &thread, local_user_id, local_author_name).await?;
    let anchor = load_comment_anchor_dto(pool, &thread.id).await?;
    let viewer = NoteCommentThreadViewerState {
        unread: thread_is_unread(pool, &thread.id, local_user_id).await?,
        muted: thread_is_muted(pool, &thread.id, local_user_id).await?,
    };
    NoteCommentThreadDto::new(thread, comments, anchor, viewer)
}

async fn thread_dtos(
    pool: &SqlitePool,
    threads: Vec<NoteCommentThreadRow>,
) -> Result<Vec<NoteCommentThreadDto>, String> {
    if threads.is_empty() {
        return Ok(Vec::new());
    }
    let local_user_id = current_local_user_id(pool).await?;
    let local_author_name = local_user::local_author_name_from_pool(pool).await?;
    let mut dtos = Vec::with_capacity(threads.len());
    for thread in threads {
        dtos.push(thread_dto(pool, thread, &local_user_id, &local_author_name).await?);
    }
    Ok(dtos)
}

pub(crate) async fn current_local_user_id(pool: &SqlitePool) -> Result<String, String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes local user read for comments: {e}"))?;
    let local_user = local_user::current_local_user_tx(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes local user read for comments: {e}"))?;
    Ok(local_user.id)
}

async fn thread_is_unread(
    pool: &SqlitePool,
    thread_id: &str,
    local_user_id: &str,
) -> Result<bool, String> {
    let unread: Option<i64> = sqlx::query_scalar(
        "SELECT 1
         FROM notes_comments AS comment
         LEFT JOIN notes_comment_thread_reads AS read_state
           ON read_state.thread_id = comment.thread_id
          AND read_state.user_id = ?
         WHERE comment.thread_id = ?
           AND comment.deleted_at IS NULL
           AND comment.created_by <> ?
           AND comment.last_edited_time > COALESCE(read_state.read_at, '')
         LIMIT 1",
    )
    .bind(local_user_id)
    .bind(thread_id)
    .bind(local_user_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes comment unread state: {e}"))?;
    Ok(unread.is_some())
}

async fn thread_is_muted(
    pool: &SqlitePool,
    thread_id: &str,
    local_user_id: &str,
) -> Result<bool, String> {
    let muted: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_comment_thread_mutes WHERE thread_id = ? AND user_id = ?",
    )
    .bind(thread_id)
    .bind(local_user_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes comment mute state: {e}"))?;
    Ok(muted.is_some())
}

async fn load_comment_anchor_dto(
    pool: &SqlitePool,
    thread_id: &str,
) -> Result<Option<NoteCommentAnchorDto>, String> {
    let row = sqlx::query_as::<_, NoteCommentAnchorRow>(
        "SELECT *
         FROM notes_comment_thread_anchors
         WHERE thread_id = ?",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes inline comment anchor: {e}"))?;
    Ok(row.map(NoteCommentAnchorDto::new))
}

async fn load_comment_dtos(
    pool: &SqlitePool,
    thread: &NoteCommentThreadRow,
    local_user_id: &str,
    local_author_name: &str,
) -> Result<Vec<NoteCommentDto>, String> {
    let rows = sqlx::query_as::<_, NoteCommentRow>(
        "SELECT *
         FROM notes_comments
         WHERE thread_id = ? AND deleted_at IS NULL
         ORDER BY created_time ASC, id ASC",
    )
    .bind(&thread.id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes comments: {e}"))?;
    let mut reactions =
        load_thread_reactions(pool, &thread.id, local_user_id, local_author_name).await?;
    rows.into_iter()
        .map(|row| {
            let comment_reactions = reactions.remove(&row.id).unwrap_or_default();
            NoteCommentDto::new(thread, row, comment_reactions, local_author_name)
        })
        .collect()
}

/// Groups a discussion's reactions by comment and emoji, keeping the order each emoji was first used.
async fn load_thread_reactions(
    pool: &SqlitePool,
    thread_id: &str,
    local_user_id: &str,
    local_author_name: &str,
) -> Result<HashMap<String, Vec<NoteCommentReactionDto>>, String> {
    let rows = sqlx::query_as::<_, (String, String, Option<String>, String)>(
        "SELECT reaction.comment_id, reaction.created_by, reaction.display_name, reaction.emoji
         FROM notes_comment_reactions AS reaction
         JOIN notes_comments AS comment ON comment.id = reaction.comment_id
         WHERE comment.thread_id = ? AND comment.deleted_at IS NULL
         ORDER BY reaction.created_time ASC, reaction.rowid ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes comment reactions: {e}"))?;
    let mut grouped: HashMap<String, Vec<NoteCommentReactionDto>> = HashMap::new();
    for (comment_id, created_by, display_name, emoji) in rows {
        let reactions = grouped.entry(comment_id).or_default();
        let index = match reactions
            .iter()
            .position(|reaction| reaction.emoji() == emoji)
        {
            Some(index) => index,
            None => {
                reactions.push(NoteCommentReactionDto::new(emoji));
                reactions.len() - 1
            }
        };
        reactions[index].add(
            local_user::comment_display_name_text(&local_user::resolved_display_name_json(
                display_name.as_deref(),
                local_author_name,
            )),
            created_by == local_user_id,
        );
    }
    Ok(grouped)
}

async fn resolve_comment_parent(
    tx: &mut Transaction<'_, Sqlite>,
    parent: &NoteParent,
) -> Result<CommentParentTarget, String> {
    match parent {
        NoteParent::Workspace { .. } => {
            Err("comments can only be parented by pages or blocks".to_string())
        }
        NoteParent::DataSourceId { .. } => {
            Err("comments can only be parented by pages or blocks".to_string())
        }
        NoteParent::PageId { page_id } => {
            require_uuid(page_id, "parent.page_id")?;
            ensure_active_page_tx(tx, page_id).await?;
            Ok(CommentParentTarget {
                page_id: page_id.clone(),
                parent_type: "page_id",
                parent_page_id: Some(page_id.clone()),
                parent_block_id: None,
            })
        }
        NoteParent::BlockId { block_id } => {
            require_uuid(block_id, "parent.block_id")?;
            let (page_id,): (String,) = sqlx::query_as(
                "SELECT block.page_id
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
            .map_err(|e| format!("load notes comment block parent: {e}"))?
            .ok_or_else(|| "notes block not found".to_string())?;
            Ok(CommentParentTarget {
                page_id,
                parent_type: "block_id",
                parent_page_id: None,
                parent_block_id: Some(block_id.clone()),
            })
        }
    }
}

async fn insert_comment_row(
    tx: &mut Transaction<'_, Sqlite>,
    comment_id: &str,
    thread_id: &str,
    rich_text: &[Value],
    attachments: &[Value],
    local_user: &NoteLocalUserRow,
) -> Result<NoteCommentRow, String> {
    let plain_text = rich_text_items_plain_text(rich_text);
    sqlx::query(
        "INSERT INTO notes_comments (
            id,
            thread_id,
            rich_text,
            plain_text,
            created_by,
            attachments
         )
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(comment_id)
    .bind(thread_id)
    .bind(Value::Array(rich_text.to_vec()).to_string())
    .bind(plain_text)
    .bind(&local_user.id)
    .bind(Value::Array(attachments.to_vec()).to_string())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create notes comment: {e}"))?;
    load_comment_row(tx, comment_id).await
}

async fn insert_comment_anchor_row(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    parent: &CommentParentTarget,
    anchor: &NoteCommentAnchorCreate,
) -> Result<(), String> {
    let block_id = parent
        .parent_block_id
        .as_deref()
        .ok_or_else(|| "inline comment anchors require a block parent".to_string())?;
    validate_comment_anchor(anchor)?;
    let plain_text: String = sqlx::query_scalar(
        "SELECT plain_text
         FROM notes_blocks
         WHERE id = ? AND page_id = ? AND in_trash = 0",
    )
    .bind(block_id)
    .bind(&parent.page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes inline comment anchor block: {e}"))?
    .ok_or_else(|| "notes block not found".to_string())?;
    if !plain_text.contains(anchor.text.as_str()) {
        return Err("inline comment anchor text must exist in the block".to_string());
    }
    sqlx::query(
        "INSERT INTO notes_comment_thread_anchors (
            thread_id,
            page_id,
            block_id,
            start_offset,
            end_offset,
            anchor_text,
            prefix_text,
            suffix_text
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(thread_id)
    .bind(&parent.page_id)
    .bind(block_id)
    .bind(anchor.start)
    .bind(anchor.end)
    .bind(anchor.text.as_str())
    .bind(anchor.prefix.as_str())
    .bind(anchor.suffix.as_str())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create notes inline comment anchor: {e}"))?;
    Ok(())
}

fn validate_comment_anchor(anchor: &NoteCommentAnchorCreate) -> Result<(), String> {
    if anchor.start < 0 || anchor.end <= anchor.start {
        return Err("inline comment anchor range is invalid".to_string());
    }
    if anchor.text.trim().is_empty() {
        return Err("inline comment anchor text is required".to_string());
    }
    if anchor.text.chars().count() > COMMENT_ANCHOR_MAX_TEXT_LENGTH {
        return Err("inline comment anchor text is too long".to_string());
    }
    if anchor.prefix.chars().count() > COMMENT_ANCHOR_MAX_CONTEXT_LENGTH
        || anchor.suffix.chars().count() > COMMENT_ANCHOR_MAX_CONTEXT_LENGTH
    {
        return Err("inline comment anchor context is too long".to_string());
    }
    Ok(())
}

fn validate_comment_attachments(attachments: &[Value]) -> Result<(), String> {
    if attachments.len() > COMMENT_ATTACHMENTS_MAX_COUNT {
        return Err("comment attachments are limited to 100 items".to_string());
    }
    let stored = Value::Array(attachments.to_vec()).to_string();
    if stored.len() > COMMENT_ATTACHMENTS_MAX_BYTES {
        return Err("comment attachments must not exceed 50KB".to_string());
    }
    for attachment in attachments {
        if !attachment.is_object() {
            return Err("comment attachments must be objects".to_string());
        }
    }
    Ok(())
}

async fn load_thread_row(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> Result<NoteCommentThreadRow, String> {
    sqlx::query_as::<_, NoteCommentThreadRow>("SELECT * FROM notes_comment_threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes comment thread: {e}"))?
        .ok_or_else(|| "notes comment thread not found".to_string())
}

async fn load_comment_row(
    tx: &mut Transaction<'_, Sqlite>,
    comment_id: &str,
) -> Result<NoteCommentRow, String> {
    sqlx::query_as::<_, NoteCommentRow>(
        "SELECT * FROM notes_comments WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(comment_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes comment: {e}"))?
    .ok_or_else(|| "notes comment not found".to_string())
}

async fn touch_thread(tx: &mut Transaction<'_, Sqlite>, thread_id: &str) -> Result<(), String> {
    sqlx::query(
        "UPDATE notes_comment_threads
         SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(thread_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("touch notes comment thread: {e}"))?;
    Ok(())
}

async fn mark_thread_read_for_current_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> Result<(), String> {
    let local_user = local_user::current_local_user_tx(tx).await?;
    mark_thread_read_for_user_tx(tx, thread_id, &local_user.id).await
}

async fn mark_thread_read_for_user_tx(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    user_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_comment_thread_reads (thread_id, user_id, read_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(thread_id, user_id) DO UPDATE
         SET read_at = excluded.read_at",
    )
    .bind(thread_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("mark notes comment thread read: {e}"))?;
    Ok(())
}

async fn ensure_active_page(pool: &SqlitePool, page_id: &str) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("check notes page for comments: {e}"))?;
    if exists.is_some() {
        Ok(())
    } else {
        Err("notes page not found".to_string())
    }
}

async fn ensure_active_page_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
) -> Result<(), String> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM notes_pages WHERE id = ? AND in_trash = 0 AND archived = 0",
    )
    .bind(page_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check notes page for comments: {e}"))?;
    if exists.is_some() {
        Ok(())
    } else {
        Err("notes page not found".to_string())
    }
}

async fn ensure_thread_target_active(
    tx: &mut Transaction<'_, Sqlite>,
    thread: &NoteCommentThreadRow,
) -> Result<(), String> {
    match thread.parent_type.as_str() {
        "page_id" => {
            let page_id = thread
                .parent_page_id
                .as_deref()
                .ok_or_else(|| "comment thread parent page is missing".to_string())?;
            ensure_active_page_tx(tx, page_id).await
        }
        "block_id" => {
            let block_id = thread
                .parent_block_id
                .as_deref()
                .ok_or_else(|| "comment thread parent block is missing".to_string())?;
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT 1
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
            .map_err(|e| format!("check notes block for comments: {e}"))?;
            if exists.is_some() {
                Ok(())
            } else {
                Err("notes block not found".to_string())
            }
        }
        _ => Err("invalid comment parent".to_string()),
    }
}

async fn new_comment_thread_id(tx: &mut Transaction<'_, Sqlite>) -> Result<String, String> {
    for _ in 0..32 {
        let id: String = sqlx::query_scalar(
            "SELECT lower(hex(randomblob(4))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(2))) || '-' ||
                    lower(hex(randomblob(6)))",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("generate notes comment thread id: {e}"))?;
        require_uuid(&id, "generated_thread_id")?;
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1
             WHERE EXISTS (SELECT 1 FROM notes_comment_threads WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_comments WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_pages WHERE id = ?)
                OR EXISTS (SELECT 1 FROM notes_blocks WHERE id = ?)",
        )
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .bind(&id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("check generated notes comment thread id: {e}"))?;
        if exists.is_none() {
            return Ok(id);
        }
    }
    Err("could not generate a unique notes comment thread id".to_string())
}
