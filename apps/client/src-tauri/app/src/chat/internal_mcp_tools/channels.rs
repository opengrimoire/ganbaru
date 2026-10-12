//! Frozen channel-history reads and bounded search for authorized sources.

use super::authorization::verify_channel_source;
use super::{
    DEFAULT_PAGE_SIZE, HostToolContext, MAX_QUERY_BYTES, MAX_RESPONSE_BYTES, OpaqueCursor,
    generic_denial_error, internal_error, optional_limit, optional_string, persistence_error,
    required_string, sha256_hex, truncate_utf8,
};
use ganbaru_chat_contracts::models::ChatResult;
use serde_json::{Map, Value, json};
use sqlx::{QueryBuilder, Row, Sqlite};

const MAX_QUERY_TERMS: usize = 20;

pub(super) async fn list_referenced_channels(context: &HostToolContext<'_>) -> ChatResult<Value> {
    let mut channels = Vec::with_capacity(context.scope.channel_sources.len());
    for source in &context.scope.channel_sources {
        verify_channel_source(context.pool, context.scope, source).await?;
        channels.push(json!({
            "sourceHandle": source.source_handle,
            "label": source.label_snapshot,
            "lowerOrdinal": source.lower_ordinal,
            "highOrdinal": source.high_ordinal,
        }));
    }
    Ok(json!({ "channels": channels, "truncated": false }))
}

pub(super) async fn channel_messages(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
    searching: bool,
) -> ChatResult<Value> {
    let source_handle = required_string(arguments, "sourceHandle", 1024)?;
    let source = context
        .scope
        .channel_sources
        .iter()
        .find(|source| source.source_handle == source_handle)
        .ok_or_else(generic_denial_error)?;
    verify_channel_source(context.pool, context.scope, source).await?;
    let terms = if searching {
        normalized_query_terms(required_string(arguments, "query", MAX_QUERY_BYTES)?)?
    } else {
        Vec::new()
    };
    let query_hash = query_terms_hash(&terms);
    let cursor = optional_string(arguments, "cursor", 1024)?;
    let cursor = match cursor {
        Some(cursor) => Some(
            context
                .runtime
                .channel_cursor(cursor, source_handle, &query_hash)
                .await?,
        ),
        None => None,
    };
    let limit = optional_limit(arguments)?.unwrap_or(DEFAULT_PAGE_SIZE);
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT item.id AS item_id, item.reply_thread_id, item.created_at, \
                revision.id AS revision_id, revision.revision, revision.normalized_markdown, \
                message.author_participant_id AS author_id, \
                message.author_label_snapshot AS author_name \
         FROM chat_conversation_items item \
         JOIN chat_communication_messages message ON message.item_id = item.id \
         JOIN chat_assignment_authorization_revisions authorization ON authorization.id = ",
    );
    query.push_bind(context.scope.authorization_revision_id.as_str());
    query.push(
        " JOIN chat_assignment_authorized_channel_sources source_grant \
            ON source_grant.authorization_revision_id = authorization.id \
           AND source_grant.source_handle = ",
    );
    query.push_bind(source_handle);
    query.push(
        " JOIN chat_communication_message_revision_ordinals cutoff_ordinal \
            ON cutoff_ordinal.message_revision_id = source_grant.source_revision_cutoff_id \
         JOIN chat_communication_message_revisions revision \
            ON revision.message_item_id = message.item_id \
           AND revision.revision = ( \
             SELECT max(candidate.revision) \
             FROM chat_communication_message_revisions candidate \
             JOIN chat_communication_message_revision_ordinals candidate_ordinal \
               ON candidate_ordinal.message_revision_id = candidate.id \
             WHERE candidate.message_item_id = message.item_id \
               AND candidate_ordinal.ordinal <= cutoff_ordinal.ordinal \
           ) \
         LEFT JOIN chat_reply_threads reply_thread ON reply_thread.id = item.reply_thread_id \
         LEFT JOIN chat_conversation_items root_item ON root_item.id = reply_thread.root_item_id \
         WHERE item.conversation_id = ",
    );
    query.push_bind(&source.conversation_id);
    query.push(" AND message.deleted_at IS NULL AND coalesce(root_item.ordinal, item.ordinal) >= ");
    query.push_bind(i64::try_from(source.lower_ordinal).map_err(|_| generic_denial_error())?);
    query.push(" AND coalesce(root_item.ordinal, item.ordinal) <= ");
    query.push_bind(i64::try_from(source.high_ordinal).map_err(|_| generic_denial_error())?);
    if let Some(cursor) = cursor {
        query.push(" AND (item.created_at < ");
        query.push_bind(cursor.before_created_at);
        query.push(" OR (item.created_at = ");
        query.push_bind(cursor.before_created_at_again);
        query.push(" AND item.id < ");
        query.push_bind(cursor.before_item_id);
        query.push("))");
    }
    for term in &terms {
        query.push(" AND lower(revision.normalized_markdown) LIKE ");
        query.push_bind(format!("%{}%", escape_like(term)));
        query.push(" ESCAPE '\\'");
    }
    query.push(" ORDER BY item.created_at DESC, item.id DESC LIMIT ");
    query.push_bind(i64::from(limit.saturating_add(1)));
    let rows = query
        .build()
        .fetch_all(context.pool)
        .await
        .map_err(persistence_error)?;
    let has_more_rows = rows.len() > limit as usize;
    let mut messages = Vec::new();
    let mut last_position = None;
    let mut response_truncated = false;
    for row in rows.into_iter().take(limit as usize) {
        let text: String = row
            .try_get("normalized_markdown")
            .map_err(persistence_error)?;
        let content_hash = sha256_hex(text.as_bytes());
        let text = truncate_utf8(&text, 16 * 1024);
        let created_at: String = row.try_get("created_at").map_err(persistence_error)?;
        let item_id: String = row.try_get("item_id").map_err(persistence_error)?;
        let candidate = json!({
            "messageId": item_id,
            "revisionId": row.try_get::<String, _>("revision_id").map_err(persistence_error)?,
            "revision": u64::try_from(row.try_get::<i64, _>("revision").map_err(persistence_error)?)
                .map_err(|_| generic_denial_error())?,
            "author": {
                "id": row.try_get::<String, _>("author_id").map_err(persistence_error)?,
                "displayName": row.try_get::<String, _>("author_name").map_err(persistence_error)?,
            },
            "createdAt": created_at,
            "thread": {
                "kind": if row.try_get::<Option<String>, _>("reply_thread_id").map_err(persistence_error)?.is_some() {
                    "reply"
                } else {
                    "root"
                },
                "replyThreadId": row.try_get::<Option<String>, _>("reply_thread_id").map_err(persistence_error)?,
            },
            "normalizedMarkdown": text.value,
            "contentHash": content_hash,
            "truncated": text.truncated,
        });
        let mut proposed = messages.clone();
        proposed.push(candidate.clone());
        let proposed_size = serde_json::to_vec(&json!({ "messages": proposed }))
            .map_err(|_| internal_error("encode channel messages"))?
            .len();
        if proposed_size > MAX_RESPONSE_BYTES.saturating_sub(4096) {
            response_truncated = true;
            break;
        }
        last_position = Some((created_at, item_id));
        messages.push(candidate);
    }
    let next_cursor = if has_more_rows || response_truncated {
        match last_position {
            Some((before_created_at, before_item_id)) => Some(
                context
                    .runtime
                    .store_cursor(OpaqueCursor::Channel {
                        source_handle: source_handle.to_string(),
                        query_hash,
                        before_created_at,
                        before_item_id,
                    })
                    .await?,
            ),
            None => None,
        }
    } else {
        None
    };
    Ok(json!({
        "sourceHandle": source_handle,
        "sourceLabel": source.label_snapshot,
        "messages": messages,
        "nextCursor": next_cursor,
        "truncated": has_more_rows || response_truncated,
    }))
}

fn normalized_query_terms(query: &str) -> ChatResult<Vec<String>> {
    if query.is_empty() || query.len() > MAX_QUERY_BYTES {
        return Err(generic_denial_error());
    }
    let mut terms = Vec::new();
    for term in query.split_whitespace().map(str::to_lowercase) {
        if term.is_empty() || terms.contains(&term) {
            continue;
        }
        if terms.len() >= MAX_QUERY_TERMS {
            return Err(generic_denial_error());
        }
        terms.push(term);
    }
    if terms.is_empty() {
        Err(generic_denial_error())
    } else {
        Ok(terms)
    }
}

fn query_terms_hash(terms: &[String]) -> String {
    sha256_hex(terms.join("\0").as_bytes())
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_search_terms_are_bounded_and_like_escaped() {
        let terms = normalized_query_terms("Release 100% _Ready_").expect("terms");
        assert_eq!(terms, vec!["release", "100%", "_ready_"]);
        assert_eq!(escape_like("100%_ready\\now"), "100\\%\\_ready\\\\now");
        assert!(normalized_query_terms("").is_err());
        let too_many_terms = (0..=MAX_QUERY_TERMS)
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(normalized_query_terms(&too_many_terms).is_err());
    }
}
