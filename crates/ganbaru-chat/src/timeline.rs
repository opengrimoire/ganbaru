//! Positions and size bounds shared by every writer of conversation items and message revisions.
//!
//! Items order by `(order_key, id)` within their scope, the root timeline of a conversation or
//! one reply thread. A new item takes a key after the last key of its scope, so two devices that
//! append concurrently may pick the same key and still converge on one order through the id.

use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use ganbaru_sync_contracts::OrderKey;
use serde_json::Value;
use sqlx::SqliteConnection;

/// The largest normalized Markdown of one message revision in bytes, which the schema enforces.
pub const MAX_MESSAGE_MARKDOWN_BYTES: usize = 128 * 1024;

/// The largest serialized rich content of one message revision in bytes, which the schema
/// enforces. It stays below the replicated text field bound so a revision fits one operation.
pub const MAX_RICH_CONTENT_BYTES: usize = 192 * 1024;

/// The most attachments one message revision or scheduled message names, which the schema
/// enforces through the reference ordinal.
pub const MAX_MESSAGE_ATTACHMENTS: usize = 20;

/// The longest attachment id a message revision or scheduled message may name, which the
/// reference tables enforce so a revision with its attachments fits one operation.
pub const MAX_ATTACHMENT_ID_CHARS: usize = 128;

/// Rich content key that marks a projected revision whose Markdown or payload was shortened, so
/// readers show a localized elision marker.
pub const TRUNCATED_RICH_CONTENT_KEY: &str = "truncated";

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

/// The validation error for rich message content over [`MAX_RICH_CONTENT_BYTES`].
pub fn rich_content_too_large() -> ChatError {
    ChatError::validation(
        "richContent",
        "Rich message content exceeds the 192 KiB limit",
    )
}

/// The order key after `last`, the greatest key of a scope, or the first key of an empty scope.
pub fn order_key_after(last: Option<&str>) -> ChatResult<String> {
    let last = last
        .map(OrderKey::parse)
        .transpose()
        .map_err(|_| stored_order_key_error())?;
    OrderKey::between(last.as_ref(), None)
        .map(OrderKey::into_string)
        .map_err(|_| stored_order_key_error())
}

fn stored_order_key_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat timeline position is invalid",
        false,
    )
}

/// The greatest order key of a scope: the root timeline of `conversation_id` when
/// `reply_thread_id` is `None`, otherwise that reply thread.
pub async fn last_order_key(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    reply_thread_id: Option<&str>,
) -> ChatResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT order_key FROM chat_conversation_items
         WHERE conversation_id = ? AND reply_thread_id IS ?
         ORDER BY order_key DESC, id DESC LIMIT 1",
    )
    .bind(conversation_id)
    .bind(reply_thread_id)
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)
}

/// The order key for an item appended to a scope, after every item the scope holds.
pub async fn next_order_key(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    reply_thread_id: Option<&str>,
) -> ChatResult<String> {
    let last = last_order_key(connection, conversation_id, reply_thread_id).await?;
    order_key_after(last.as_deref())
}

/// The longest prefix of `text` within `max_bytes` that ends on a character boundary, and
/// whether anything was cut.
pub fn truncate_at_char_boundary(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// A projected revision whose Markdown and rich content fit the schema bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedRevision {
    pub normalized_markdown: String,
    pub rich_content_data: String,
}

/// Bounds a projected agent revision. Markdown over [`MAX_MESSAGE_MARKDOWN_BYTES`] is cut at a
/// character boundary; rich content over [`MAX_RICH_CONTENT_BYTES`] keeps its fields but drops
/// its `payload`. Either cut sets [`TRUNCATED_RICH_CONTENT_KEY`] in the rich content. The full
/// text stays in the run's local events.
pub fn bounded_projected_revision(
    markdown: &str,
    mut rich_content: Value,
) -> ChatResult<BoundedRevision> {
    let (markdown, markdown_cut) = truncate_at_char_boundary(markdown, MAX_MESSAGE_MARKDOWN_BYTES);
    if markdown_cut {
        mark_truncated(&mut rich_content);
    }
    let mut data = serialize(&rich_content)?;
    if data.len() > MAX_RICH_CONTENT_BYTES {
        if let Some(object) = rich_content.as_object_mut() {
            object.insert("payload".to_owned(), Value::Null);
        }
        mark_truncated(&mut rich_content);
        data = serialize(&rich_content)?;
    }
    if data.len() > MAX_RICH_CONTENT_BYTES {
        return Err(rich_content_too_large());
    }
    Ok(BoundedRevision {
        normalized_markdown: markdown.to_owned(),
        rich_content_data: data,
    })
}

fn mark_truncated(rich_content: &mut Value) {
    if let Some(object) = rich_content.as_object_mut() {
        object.insert(TRUNCATED_RICH_CONTENT_KEY.to_owned(), Value::Bool(true));
    }
}

fn serialize(value: &Value) -> ChatResult<String> {
    serde_json::to_string(value).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Chat rich content could not be serialized",
            false,
        )
    })
}

#[cfg(test)]
mod tests;
