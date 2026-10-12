//! Durable inline review comments and immutable composer context.

pub mod engine;

use crate::{db, vault};
use chrono::{SecondsFormat, Utc};
use engine::{ResolveReviewSelectionRequest, ReviewDiffSource, TauriReviewAuthorizer};
use ganbaru_chat::repository::attachments;
use ganbaru_chat_contracts::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId,
    UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use std::sync::OnceLock;

const MAX_PATH_BYTES: usize = 4_096;
const MAX_COMMENT_BYTES: usize = 65_536;
const MAX_SELECTED_TEXT_BYTES: usize = 1_048_576;
const MAX_LIST_SELECTED_TEXT_CHARACTERS: usize = 16_384;
const MAX_LIST_REVIEW_COMMENTS: usize = 256;
const MAX_LIST_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_REVIEW_CONTEXT_BYTES: usize = 128 * 1024;
const MAX_ATTACHMENT_DISPLAY_NAME_BYTES: usize = 1_000;
const REVIEW_CONTEXT_ATTACHMENT_PREFIX: &str = "review-context:";
const MAX_REVIEW_COMMENT_ID_BYTES: usize = 1_024 - REVIEW_CONTEXT_ATTACHMENT_PREFIX.len();
static REVIEW_ATTACHMENT_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatReviewCommentRequest {
    pub id: String,
    pub thread_id: ChatThreadId,
    pub relative_path: String,
    pub content_revision: String,
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
    pub selected_text: String,
    pub comment_text: String,
    pub source_kind: Option<String>,
    pub source_data: Option<ReviewDiffSource>,
    pub snapshot_id: Option<String>,
    pub review_revision: Option<String>,
    pub file_id: Option<String>,
    pub selection_side: Option<String>,
    pub previous_relative_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachChatReviewCommentRequest {
    pub thread_id: ChatThreadId,
    pub comment_id: String,
    pub attachment_id: ChatAttachmentId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatReviewCommentState {
    Open,
    Resolved,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReviewCommentRead {
    pub id: String,
    pub thread_id: ChatThreadId,
    pub relative_path: String,
    pub content_revision: String,
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
    pub selected_text: String,
    pub comment_text: String,
    pub state: ChatReviewCommentState,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub resolved_at: Option<UtcTimestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_data: Option<ReviewDiffSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_side: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicability: Option<String>,
}

#[tauri::command]
pub async fn chat_list_review_comments(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    include_resolved: bool,
) -> ChatResult<Vec<ChatReviewCommentRead>> {
    let pool = chat_pool(app, db_url).await?;
    require_thread_workspace(&pool, &thread_id).await?;
    let query = format!(
        "SELECT id, thread_id, relative_path, content_revision, start_line, start_column,
                end_line, end_column, substr(selected_text, 1, {MAX_LIST_SELECTED_TEXT_CHARACTERS}) AS selected_text,
                comment_text, state, created_at,
                updated_at, resolved_at, source_kind, source_data, snapshot_id,
                review_revision, selection_side, previous_relative_path, applicability
         FROM chat_review_comments
         WHERE thread_id = ? AND (? OR state = 'open')
         ORDER BY relative_path, start_line, start_column, created_at, id
         LIMIT {}",
        MAX_LIST_REVIEW_COMMENTS + 1
    );
    let rows = sqlx::query(&query)
        .bind(thread_id.as_str())
        .bind(include_resolved)
        .fetch_all(&pool)
        .await
        .map_err(persistence_error)?;
    if rows.len() > MAX_LIST_REVIEW_COMMENTS {
        return Err(ChatError::new(
            ChatErrorCode::CapabilityUnsupported,
            "This thread has too many review comments to list safely",
            true,
        ));
    }
    let comments = rows
        .into_iter()
        .map(parse_comment)
        .collect::<ChatResult<Vec<_>>>()?;
    let encoded = serde_json::to_vec(&comments).map_err(persistence_error)?;
    if encoded.len() > MAX_LIST_RESPONSE_BYTES {
        return Err(ChatError::new(
            ChatErrorCode::Protocol,
            "Review comment summaries exceed the supported response limit",
            true,
        ));
    }
    Ok(comments)
}

#[tauri::command]
pub async fn chat_create_review_comment(
    app: tauri::AppHandle,
    reviews: tauri::State<'_, engine::ChatReviewRegistry>,
    db_url: String,
    request: CreateChatReviewCommentRequest,
) -> ChatResult<ChatReviewCommentRead> {
    validate_create_request(&request)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let database_identity = database_identity(&pool).await?;
    require_thread_workspace(&pool, &request.thread_id).await?;
    if comment_id_exists(&pool, &request.id).await? {
        if existing_comment_matches_request(&pool, &request).await? {
            return read_comment(&pool, &request.thread_id, &request.id).await;
        }
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Review comment ID is already owned by different content",
            false,
        ));
    }
    let resolved = match (
        request.snapshot_id.as_deref(),
        request.review_revision.as_deref(),
        request.file_id.as_deref(),
        request.selection_side.as_deref(),
    ) {
        (Some(snapshot_id), Some(review_revision), Some(file_id), Some(side)) => Some(
            reviews
                .resolve_selection(
                    &TauriReviewAuthorizer::new(app.clone()),
                    &pool,
                    &database_identity,
                    ResolveReviewSelectionRequest {
                        thread_id: &request.thread_id,
                        snapshot_id,
                        review_revision,
                        file_id,
                        side,
                        start_line: request.start_line,
                        start_column: request.start_column,
                        end_line: request.end_line,
                        end_column: request.end_column,
                    },
                )
                .await?,
        ),
        (None, None, None, None) => None,
        _ => {
            return Err(ChatError::validation(
                "snapshotId",
                "Snapshot review comments require snapshot, revision, file, and side",
            ));
        }
    };
    if let Some(resolved) = resolved.as_ref() {
        if resolved.relative_path != request.relative_path
            || resolved.previous_relative_path != request.previous_relative_path
            || resolved.content_revision != request.content_revision
            || request.source_data.as_ref() != Some(&resolved.source)
            || request.source_kind.as_deref() != Some(resolved.source.kind_wire())
        {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "Review comment selection does not match the immutable snapshot",
                true,
            ));
        }
    } else if request.source_kind.is_some()
        || request.source_data.is_some()
        || request.snapshot_id.is_some()
        || request.review_revision.is_some()
        || request.file_id.is_some()
        || request
            .selection_side
            .as_deref()
            .is_some_and(|side| side != "file")
        || request.previous_relative_path.is_some()
    {
        return Err(ChatError::validation(
            "snapshotId",
            "File editor comments cannot include snapshot provenance",
        ));
    }
    let selected_text = resolved
        .as_ref()
        .map(|selection| selection.selected_text.as_str())
        .unwrap_or(&request.selected_text);
    if selected_text.len() > MAX_SELECTED_TEXT_BYTES || selected_text.contains('\0') {
        return Err(ChatError::validation(
            "range",
            "Review selection exceeds the supported limit",
        ));
    }
    let now = now_timestamp()?;
    let insert = sqlx::query(
        "INSERT INTO chat_review_comments
            (id, thread_id, relative_path, content_revision, start_line, start_column,
             end_line, end_column, selected_text, comment_text, state, created_at, updated_at,
             source_kind, source_data, snapshot_id, review_revision, file_id, selection_side,
             previous_relative_path, applicability, queued_for_send)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'open', ?, ?, ?, ?, ?, ?, ?, ?, ?, 'current', 1)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(&request.id)
    .bind(request.thread_id.as_str())
    .bind(&request.relative_path)
    .bind(&request.content_revision)
    .bind(i64_value(request.start_line, "startLine")?)
    .bind(i64_value(request.start_column, "startColumn")?)
    .bind(i64_value(request.end_line, "endLine")?)
    .bind(i64_value(request.end_column, "endColumn")?)
    .bind(selected_text)
    .bind(request.comment_text.trim())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(request.source_kind.as_deref().unwrap_or("file"))
    .bind(
        request
            .source_data
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| persistence_error("serialize review source"))?,
    )
    .bind(request.snapshot_id.as_deref())
    .bind(request.review_revision.as_deref())
    .bind(request.file_id.as_deref())
    .bind(request.selection_side.as_deref().unwrap_or("file"))
    .bind(request.previous_relative_path.as_deref())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if insert.rows_affected() == 0 {
        let identical = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM chat_review_comments
             WHERE id = ? AND thread_id = ? AND relative_path = ? AND content_revision = ?
               AND start_line = ? AND start_column = ? AND end_line = ? AND end_column = ?
               AND selected_text = ? AND comment_text = ? AND source_kind = ?
               AND source_data IS ? AND snapshot_id IS ? AND review_revision IS ?
               AND file_id IS ? AND selection_side = ? AND previous_relative_path IS ?",
        )
        .bind(&request.id)
        .bind(request.thread_id.as_str())
        .bind(&request.relative_path)
        .bind(&request.content_revision)
        .bind(i64_value(request.start_line, "startLine")?)
        .bind(i64_value(request.start_column, "startColumn")?)
        .bind(i64_value(request.end_line, "endLine")?)
        .bind(i64_value(request.end_column, "endColumn")?)
        .bind(selected_text)
        .bind(request.comment_text.trim())
        .bind(request.source_kind.as_deref().unwrap_or("file"))
        .bind(
            request
                .source_data
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|_| persistence_error("serialize review source"))?,
        )
        .bind(request.snapshot_id.as_deref())
        .bind(request.review_revision.as_deref())
        .bind(request.file_id.as_deref())
        .bind(request.selection_side.as_deref().unwrap_or("file"))
        .bind(request.previous_relative_path.as_deref())
        .fetch_one(&pool)
        .await
        .map_err(persistence_error)?
            == 1;
        if !identical {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Review comment ID is already owned by different content",
                false,
            ));
        }
    }
    read_comment(&pool, &request.thread_id, &request.id).await
}

async fn comment_id_exists(pool: &SqlitePool, comment_id: &str) -> ChatResult<bool> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM chat_review_comments WHERE id = ?")
        .bind(comment_id)
        .fetch_one(pool)
        .await
        .map(|count| count == 1)
        .map_err(persistence_error)
}

async fn existing_comment_matches_request(
    pool: &SqlitePool,
    request: &CreateChatReviewCommentRequest,
) -> ChatResult<bool> {
    let source_data = request
        .source_data
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| persistence_error("serialize review source"))?;
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_review_comments
         WHERE id = ? AND thread_id = ? AND relative_path = ? AND content_revision = ?
           AND start_line = ? AND start_column = ? AND end_line = ? AND end_column = ?
           AND comment_text = ? AND source_kind = ? AND source_data IS ?
           AND snapshot_id IS ? AND review_revision IS ? AND file_id IS ?
           AND selection_side = ? AND previous_relative_path IS ?
           AND (? OR selected_text = ?)",
    )
    .bind(&request.id)
    .bind(request.thread_id.as_str())
    .bind(&request.relative_path)
    .bind(&request.content_revision)
    .bind(i64_value(request.start_line, "startLine")?)
    .bind(i64_value(request.start_column, "startColumn")?)
    .bind(i64_value(request.end_line, "endLine")?)
    .bind(i64_value(request.end_column, "endColumn")?)
    .bind(request.comment_text.trim())
    .bind(request.source_kind.as_deref().unwrap_or("file"))
    .bind(source_data)
    .bind(request.snapshot_id.as_deref())
    .bind(request.review_revision.as_deref())
    .bind(request.file_id.as_deref())
    .bind(request.selection_side.as_deref().unwrap_or("file"))
    .bind(request.previous_relative_path.as_deref())
    .bind(request.snapshot_id.is_some())
    .bind(&request.selected_text)
    .fetch_one(pool)
    .await
    .map(|count| count == 1)
    .map_err(persistence_error)
}

#[tauri::command]
pub async fn chat_set_review_comment_resolved(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    comment_id: String,
    resolved: bool,
) -> ChatResult<ChatReviewCommentRead> {
    validate_id(&comment_id)?;
    let pool = chat_pool(app, db_url).await?;
    require_thread_workspace(&pool, &thread_id).await?;
    let now = now_timestamp()?;
    let result = sqlx::query(
        "UPDATE chat_review_comments
         SET state = CASE WHEN ? THEN 'resolved' ELSE 'open' END,
             resolved_at = CASE WHEN ? THEN ? ELSE NULL END,
             queued_for_send = CASE WHEN ? THEN 0 ELSE queued_for_send END,
             updated_at = ?
         WHERE id = ? AND thread_id = ?",
    )
    .bind(resolved)
    .bind(resolved)
    .bind(now.as_str())
    .bind(resolved)
    .bind(now.as_str())
    .bind(&comment_id)
    .bind(thread_id.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if result.rows_affected() == 0 {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Review comment was not found",
            true,
        ));
    }
    read_comment(&pool, &thread_id, &comment_id).await
}

#[tauri::command]
pub async fn chat_attach_review_comment(
    app: tauri::AppHandle,
    db_url: String,
    request: AttachChatReviewCommentRequest,
) -> ChatResult<attachments::ChatAttachmentRead> {
    validate_id(&request.comment_id)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let working_folder_id = require_thread_workspace(&pool, &request.thread_id).await?;
    let comment = read_comment(&pool, &request.thread_id, &request.comment_id).await?;
    let expected_attachment_id =
        format!("{REVIEW_CONTEXT_ATTACHMENT_PREFIX}{}", request.comment_id);
    if request.attachment_id.as_str() != expected_attachment_id.as_str() {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Review context attachment ID does not match this comment",
            false,
        ));
    }
    let context = bounded_review_context(&comment);
    let now = now_timestamp()?;
    let vault_root = vault::active_writable_vault_path(&app).map_err(vault_error)?;
    let _guard = REVIEW_ATTACHMENT_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let display_name = review_attachment_display_name(&comment.relative_path);
    if let Some(existing) = attachments::read_attachment(&pool, &request.attachment_id).await? {
        let expected_hash = format!("{:x}", Sha256::digest(context.as_bytes()));
        let valid = existing.working_folder_id == working_folder_id
            && existing.kind == attachments::ChatAttachmentKind::TextSnippet
            && existing.original_display_name == display_name
            && existing.byte_size == context.len() as u64
            && existing.sha256 == expected_hash
            && attachments::read_managed_attachment_bytes(&vault_root, &existing)
                .is_ok_and(|(_, bytes)| bytes == context.as_bytes());
        if !valid {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Review context attachment ID is already owned by different content",
                false,
            ));
        }
        mark_comment_attached(&pool, &request.thread_id, &request.comment_id, &now).await?;
        return Ok(existing);
    }
    let attachment = attachments::import_attachment_bytes(
        &pool,
        &vault_root,
        attachments::AttachmentBytesImport {
            working_folder_id: &working_folder_id,
            attachment_id: request.attachment_id,
            display_name,
            bytes: context.as_bytes(),
            requested_kind: attachments::ChatAttachmentKind::TextSnippet,
            now: &now,
        },
    )
    .await?;
    mark_comment_attached(&pool, &request.thread_id, &request.comment_id, &now).await?;
    Ok(attachment)
}

fn review_attachment_display_name(relative_path: &str) -> String {
    const SUFFIX: &str = " review comment.txt";
    let full = format!("{relative_path}{SUFFIX}");
    if full.len() <= MAX_ATTACHMENT_DISPLAY_NAME_BYTES {
        return full;
    }
    let hash = format!("{:x}", Sha256::digest(relative_path.as_bytes()));
    let trailer = format!("-{}{SUFFIX}", &hash[..16]);
    let mut boundary = MAX_ATTACHMENT_DISPLAY_NAME_BYTES - trailer.len();
    while !relative_path.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{trailer}", &relative_path[..boundary])
}

async fn mark_comment_attached(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    comment_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_review_comments SET queued_for_send = 0, updated_at = ?
         WHERE id = ? AND thread_id = ?",
    )
    .bind(now.as_str())
    .bind(comment_id)
    .bind(thread_id.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn require_thread_workspace(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<ProjectWorkingFolderId> {
    let row = sqlx::query(
        "SELECT working_folder_id FROM chat_threads WHERE id = ? AND state != 'closed'",
    )
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Active Chat thread was not found",
            true,
        )
    })?;
    ProjectWorkingFolderId::new(
        row.try_get::<String, _>("working_folder_id")
            .map_err(persistence_error)?,
    )
    .map_err(|_| persistence_error("invalid working folder ID"))
}

async fn read_comment(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    comment_id: &str,
) -> ChatResult<ChatReviewCommentRead> {
    let row = sqlx::query(
        "SELECT id, thread_id, relative_path, content_revision, start_line, start_column,
                end_line, end_column, selected_text, comment_text, state, created_at,
                updated_at, resolved_at, source_kind, source_data, snapshot_id,
                review_revision, selection_side, previous_relative_path, applicability
         FROM chat_review_comments WHERE id = ? AND thread_id = ?",
    )
    .bind(comment_id)
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Review comment was not found",
            true,
        )
    })?;
    parse_comment(row)
}

fn parse_comment(row: sqlx::sqlite::SqliteRow) -> ChatResult<ChatReviewCommentRead> {
    let state = match row
        .try_get::<String, _>("state")
        .map_err(persistence_error)?
        .as_str()
    {
        "open" => ChatReviewCommentState::Open,
        "resolved" => ChatReviewCommentState::Resolved,
        _ => return Err(persistence_error("invalid review comment state")),
    };
    Ok(ChatReviewCommentRead {
        id: row.try_get("id").map_err(persistence_error)?,
        thread_id: ChatThreadId::new(
            row.try_get::<String, _>("thread_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| persistence_error("invalid thread ID"))?,
        relative_path: row.try_get("relative_path").map_err(persistence_error)?,
        content_revision: row.try_get("content_revision").map_err(persistence_error)?,
        start_line: u64_value(row.try_get("start_line").map_err(persistence_error)?)?,
        start_column: u64_value(row.try_get("start_column").map_err(persistence_error)?)?,
        end_line: u64_value(row.try_get("end_line").map_err(persistence_error)?)?,
        end_column: u64_value(row.try_get("end_column").map_err(persistence_error)?)?,
        selected_text: row.try_get("selected_text").map_err(persistence_error)?,
        comment_text: row.try_get("comment_text").map_err(persistence_error)?,
        state,
        created_at: timestamp(&row, "created_at")?,
        updated_at: timestamp(&row, "updated_at")?,
        resolved_at: row
            .try_get::<Option<String>, _>("resolved_at")
            .map_err(persistence_error)?
            .map(UtcTimestamp::new)
            .transpose()
            .map_err(|_| persistence_error("invalid resolved timestamp"))?,
        source_kind: match row
            .try_get::<String, _>("source_kind")
            .map_err(persistence_error)?
            .as_str()
        {
            "file" => None,
            value => Some(value.to_string()),
        },
        source_data: row
            .try_get::<Option<String>, _>("source_data")
            .map_err(persistence_error)?
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|_| persistence_error("invalid review source"))?,
        snapshot_id: row.try_get("snapshot_id").map_err(persistence_error)?,
        review_revision: row.try_get("review_revision").map_err(persistence_error)?,
        selection_side: match row
            .try_get::<String, _>("selection_side")
            .map_err(persistence_error)?
            .as_str()
        {
            "file" => None,
            value => Some(value.to_string()),
        },
        previous_relative_path: row
            .try_get("previous_relative_path")
            .map_err(persistence_error)?,
        applicability: Some(row.try_get("applicability").map_err(persistence_error)?),
    })
}

fn validate_create_request(request: &CreateChatReviewCommentRequest) -> ChatResult<()> {
    validate_id(&request.id)?;
    let path = request.relative_path.as_str();
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.starts_with("../")
        || path.contains("/../")
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || !canonical_relative_path(path)
    {
        return Err(ChatError::validation(
            "relativePath",
            "Review path is invalid",
        ));
    }
    if request.content_revision.len() < 16
        || request.content_revision.len() > 128
        || request.content_revision.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "contentRevision",
            "Review content revision is invalid",
        ));
    }
    if request.start_line == 0
        || request.start_column == 0
        || request.end_line < request.start_line
        || request.end_column == 0
        || (request.end_line == request.start_line && request.end_column < request.start_column)
    {
        return Err(ChatError::validation("range", "Review range is invalid"));
    }
    if request.selected_text.len() > MAX_SELECTED_TEXT_BYTES || request.selected_text.contains('\0')
    {
        return Err(ChatError::validation(
            "selectedText",
            "Review selection is invalid",
        ));
    }
    let comment = request.comment_text.trim();
    if comment.is_empty() || comment.len() > MAX_COMMENT_BYTES || comment.contains('\0') {
        return Err(ChatError::validation(
            "commentText",
            "Review comment is invalid",
        ));
    }
    if let Some(previous) = request.previous_relative_path.as_deref() {
        validate_review_path(previous, "previousRelativePath")?;
    }
    if let Some(side) = request.selection_side.as_deref() {
        if !matches!(side, "file" | "old" | "new") {
            return Err(ChatError::validation(
                "selectionSide",
                "Review selection side is invalid",
            ));
        }
    }
    for (field, value) in [
        ("snapshotId", request.snapshot_id.as_deref()),
        ("fileId", request.file_id.as_deref()),
    ] {
        if let Some(value) = value {
            if value.is_empty() || value.len() > 1_024 || value.chars().any(char::is_control) {
                return Err(ChatError::validation(field, "Review identifier is invalid"));
            }
        }
    }
    if let Some(revision) = request.review_revision.as_deref() {
        if revision.len() < 16
            || revision.len() > 128
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ChatError::validation(
                "reviewRevision",
                "Review revision is invalid",
            ));
        }
    }
    match (&request.source_kind, &request.source_data) {
        (Some(kind), Some(source)) if kind == source.kind_wire() => {}
        (None, None) => {}
        _ => {
            return Err(ChatError::validation(
                "sourceData",
                "Review source metadata is inconsistent",
            ));
        }
    }
    Ok(())
}

fn validate_review_path(path: &str, field: &str) -> ChatResult<()> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.starts_with('/')
        || path.starts_with("../")
        || path.contains("/../")
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || !canonical_relative_path(path)
    {
        return Err(ChatError::validation(field, "Review path is invalid"));
    }
    Ok(())
}

fn canonical_relative_path(path: &str) -> bool {
    path.split('/')
        .all(|component| !component.is_empty() && !matches!(component, "." | ".."))
        && Path::new(path)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn validate_id(value: &str) -> ChatResult<()> {
    if value.is_empty()
        || value.len() > MAX_REVIEW_COMMENT_ID_BYTES
        || value.chars().any(char::is_control)
        || value.chars().any(char::is_whitespace)
    {
        return Err(ChatError::validation(
            "commentId",
            "Review comment ID is invalid",
        ));
    }
    Ok(())
}

fn bounded_review_context(comment: &ChatReviewCommentRead) -> String {
    let source = comment.source_kind.as_deref().unwrap_or("file");
    let side = comment.selection_side.as_deref().unwrap_or("file");
    let previous = comment
        .previous_relative_path
        .as_deref()
        .map(|path| format!("\nPrevious path: {path}"))
        .unwrap_or_default();
    let heading = format!(
        "Review comment\nSource: {source}\nPath: {}{previous}\nSide: {side}\nRange: {}:{} to {}:{}\nContent revision: {}\nComment: {}\n\nSelected text:\n",
        comment.relative_path,
        comment.start_line,
        comment.start_column,
        comment.end_line,
        comment.end_column,
        comment.content_revision,
        comment.comment_text,
    );
    if heading.len() >= MAX_REVIEW_CONTEXT_BYTES {
        let mut boundary = MAX_REVIEW_CONTEXT_BYTES;
        while !heading.is_char_boundary(boundary) {
            boundary -= 1;
        }
        return heading[..boundary].to_string();
    }
    let maximum = MAX_REVIEW_CONTEXT_BYTES - heading.len();
    let mut boundary = comment.selected_text.len().min(maximum);
    while !comment.selected_text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{heading}{}", &comment.selected_text[..boundary])
}

fn timestamp(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<UtcTimestamp> {
    UtcTimestamp::new(
        row.try_get::<String, _>(column)
            .map_err(persistence_error)?,
    )
    .map_err(|_| persistence_error("invalid timestamp"))
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

fn i64_value(value: u64, field: &str) -> ChatResult<i64> {
    i64::try_from(value).map_err(|_| ChatError::validation(field, "Review range is too large"))
}

fn u64_value(value: i64) -> ChatResult<u64> {
    u64::try_from(value).map_err(|_| persistence_error("invalid review range"))
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

async fn database_identity(pool: &SqlitePool) -> ChatResult<String> {
    let path: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main' LIMIT 1")
            .fetch_one(pool)
            .await
            .map_err(persistence_error)?;
    if path.is_empty() {
        return Err(persistence_error("Chat database path is unavailable"));
    }
    std::fs::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(persistence_error)
}

fn vault_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Active Ganbaru folder is unavailable",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat review comment persistence failed",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_context_preserves_location_revision_and_comment() {
        let timestamp = UtcTimestamp::new("2026-07-29T12:00:00.000Z").expect("timestamp");
        let comment = ChatReviewCommentRead {
            id: "review:1".to_string(),
            thread_id: ChatThreadId::new("thread:1").expect("thread ID"),
            relative_path: "src/lib.rs".to_string(),
            content_revision: "0123456789abcdef".to_string(),
            start_line: 4,
            start_column: 2,
            end_line: 5,
            end_column: 8,
            selected_text: "let value = 1;".to_string(),
            comment_text: "Handle this error explicitly.".to_string(),
            state: ChatReviewCommentState::Open,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            resolved_at: None,
            source_kind: None,
            source_data: None,
            snapshot_id: None,
            review_revision: None,
            selection_side: None,
            previous_relative_path: None,
            applicability: Some("current".to_string()),
        };

        let context = bounded_review_context(&comment);
        assert!(context.contains("src/lib.rs"));
        assert!(context.contains("4:2 to 5:8"));
        assert!(context.contains("0123456789abcdef"));
        assert!(context.contains("Handle this error explicitly."));
        assert!(context.ends_with("let value = 1;"));
    }
}
