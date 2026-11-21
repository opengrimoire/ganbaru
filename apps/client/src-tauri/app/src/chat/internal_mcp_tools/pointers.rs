//! The reply pointer host tool: an agent expands a long earlier reply that its context
//! arrived with only as a shortened pointer record.
//!
//! The tool reads only a reply that the run's current native session received as a pointer,
//! in the assignment's own reply thread, while the run's scope is still live. Reading it
//! records the expanded text as delivered, so a continuation does not send it again.

use super::{
    HostToolContext, MAX_RESPONSE_BYTES, generic_denial_error, persistence_error, required_string,
    sha256_hex, truncate_utf8,
};
use crate::chat::command_support::now_timestamp;
use crate::chat::internal_mcp::InternalMcpRunScope;
use ganbaru_chat::agent_runs::{DeliveredSource, DeliveryKind, insert_delivered_sources};
use ganbaru_chat_contracts::models::{ChatResult, ChatThreadId};
use serde_json::{Map, Value, json};
use sqlx::{Row, SqlitePool};

/// Largest message id the tool accepts, matching the delivered source id bound.
const MAX_POINTER_ID_BYTES: usize = 1024;
/// Room kept in a response for everything but the reply text.
const RESPONSE_OVERHEAD_BYTES: usize = 4096;

/// The input properties of the reply pointer tool.
pub(super) fn properties() -> Value {
    json!({
        "id": { "type": "string", "minLength": 1, "maxLength": MAX_POINTER_ID_BYTES }
    })
}

/// Returns the current text of the reply that a `pointer` record named by `id` stood for.
pub(super) async fn read_reply_pointer(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let id = required_string(arguments, "id", MAX_POINTER_ID_BYTES)?;
    expand_pointer(context.pool, context.thread_id, context.scope, id).await
}

/// Reads the reply `id` when a run of the current native session on `thread_id` received it as
/// a pointer and it is a live message in the reply thread of the scope's assignment, then
/// records the text it returns as delivered by the scope's run. Any other id gets the generic
/// denial, so the tool reveals nothing about messages the session was not pointed to.
async fn expand_pointer(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    scope: &InternalMcpRunScope,
    id: &str,
) -> ChatResult<Value> {
    let row = sqlx::query(
        "SELECT item.id AS item_id, revision.id AS revision_id, revision.revision,
                revision.normalized_markdown
         FROM chat_work_assignments assignment
         JOIN chat_conversation_items item
           ON item.reply_thread_id = assignment.reply_thread_id AND item.id = ?3
         JOIN chat_communication_messages message
           ON message.item_id = item.id AND message.deleted_at IS NULL
         JOIN chat_communication_message_revisions_numbered revision
           ON revision.id = message.current_revision_id
         WHERE assignment.id = ?2
           AND EXISTS(
             SELECT 1
             FROM chat_agent_run_delivered_sources delivered
             JOIN chat_agent_runs run ON run.id = delivered.run_id
             JOIN chat_work_assignments pointed ON pointed.id = run.assignment_id
             WHERE delivered.source_id = item.id
               AND delivered.delivery_kind = 'pointer'
               AND run.provider_thread_id = ?1
               AND run.state IN ('working', 'waiting', 'completed')
               AND pointed.reply_thread_id = assignment.reply_thread_id
               AND run.created_at >= (
                 SELECT max(opener.created_at) FROM chat_agent_runs opener
                 WHERE opener.provider_thread_id = ?1
                   AND opener.attempt_reason <> 'continuation'
               )
           )",
    )
    .bind(thread_id.as_str())
    .bind(scope.assignment_id.as_str())
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(generic_denial_error)?;
    let text: String = row
        .try_get("normalized_markdown")
        .map_err(persistence_error)?;
    let revision = u64::try_from(
        row.try_get::<i64, _>("revision")
            .map_err(persistence_error)?,
    )
    .map_err(|_| generic_denial_error())?;
    let revision_id: String = row.try_get("revision_id").map_err(persistence_error)?;
    let item_id: String = row.try_get("item_id").map_err(persistence_error)?;
    let sent = truncate_utf8(
        &text,
        MAX_RESPONSE_BYTES.saturating_sub(RESPONSE_OVERHEAD_BYTES),
    );
    // A whole reply hashes as its package source does, so the delta treats it as received; a
    // cut reply hashes the text actually sent, so a continuation still sends the whole reply.
    let content_hash = sha256_hex(sent.value.as_bytes());
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    insert_delivered_sources(
        &mut connection,
        &scope.run_id,
        &[DeliveredSource {
            source_id: item_id.clone(),
            content_sha256: content_hash.clone(),
            kind: DeliveryKind::Package,
        }],
        &now_timestamp()?,
    )
    .await?;
    Ok(json!({
        "messages": [{
            "messageId": item_id,
            "revisionId": revision_id,
            "revision": revision,
            "bytes": text.len(),
            "normalizedMarkdown": sent.value,
            "contentHash": content_hash,
            "truncated": sent.truncated,
        }],
        "truncated": sent.truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::internal_mcp::InternalMcpRunScope;
    use crate::chat::revocation::tests::pool_with_folder_runs;
    use ganbaru_chat_contracts::models::{
        ChatAgentRunId, ChatAuthorizationRevisionId, ChatConversationId, ChatRuntimeApprovalPolicy,
        ChatTurnId, ChatWorkAssignmentId,
    };

    const NOW: &str = "2026-11-12T09:00:00.000Z";
    const AGENT: &str = "participant:folder-agent";

    fn scope() -> InternalMcpRunScope {
        InternalMcpRunScope {
            run_id: ChatAgentRunId::new("run:working").unwrap(),
            provider_turn_id: ChatTurnId::new("turn:working").unwrap(),
            assignment_id: ChatWorkAssignmentId::new("assignment:folders").unwrap(),
            authorization_revision_id: ChatAuthorizationRevisionId::new("authorization:working")
                .unwrap(),
            destination_conversation_id: ChatConversationId::new(
                ganbaru_chat::project_defaults::default_channel_ids("project-chat")
                    .unwrap()
                    .0,
            )
            .unwrap(),
            authorization_scope_digest: "0".repeat(64),
            execution_environment_id: None,
            scratch_generation_id: None,
            runtime_approval_policy: ChatRuntimeApprovalPolicy::Ask,
            channel_sources: Vec::new(),
            folder_sources: Vec::new(),
            delegation_depth: 0,
            delegation_target_available: true,
        }
    }

    /// Posts an agent reply `item_id` with `text` in the run's reply thread, or at the channel
    /// root when `in_thread` is false.
    async fn post_reply(pool: &SqlitePool, item_id: &str, text: &str, in_thread: bool) {
        let (conversation, _) =
            ganbaru_chat::project_defaults::default_channel_ids("project-chat").unwrap();
        let reply_thread = if in_thread { "'reply:folders'" } else { "NULL" };
        let order_key: String = item_id
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO chat_conversation_items
                (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES ('{item_id}', '{conversation}', {reply_thread}, 'message',
                     'b{order_key}', '{NOW}');
             INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
             VALUES ('{item_id}', '{AGENT}', '{NOW}');"
        )))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_message_revisions
                (id, message_item_id, normalized_markdown, created_at)
             VALUES (?1 || ':revision', ?1, ?2, ?3)",
        )
        .bind(item_id)
        .bind(text)
        .bind(NOW)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_communication_messages SET current_revision_id = ?1 || ':revision'
             WHERE item_id = ?1",
        )
        .bind(item_id)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn deliver(pool: &SqlitePool, source_id: &str, kind: &str) {
        sqlx::query(
            "INSERT INTO chat_agent_run_delivered_sources
                (run_id, source_id, content_sha256, delivery_kind, delivered_at)
             VALUES ('run:working', ?, ?, ?, ?)",
        )
        .bind(source_id)
        .bind("b".repeat(64))
        .bind(kind)
        .bind(NOW)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn expand(pool: &SqlitePool, id: &str) -> ChatResult<Value> {
        expand_pointer(pool, &ChatThreadId::new("thread-1").unwrap(), &scope(), id).await
    }

    #[tokio::test]
    async fn a_pointer_delivered_to_the_session_expands_and_counts_as_delivered() {
        let pool = pool_with_folder_runs().await;
        let text = "long answer ".repeat(600);
        post_reply(&pool, "item:long", &text, true).await;
        deliver(&pool, "item:long", "pointer").await;

        let value = expand(&pool, "item:long").await.unwrap();
        let message = &value["messages"][0];
        assert_eq!(message["normalizedMarkdown"], text.as_str());
        assert_eq!(message["bytes"], text.len());
        assert_eq!(value["truncated"], false);
        let recorded: Vec<(String, String)> = sqlx::query_as(
            "SELECT content_sha256, delivery_kind FROM chat_agent_run_delivered_sources
             WHERE run_id = 'run:working' AND source_id = 'item:long' ORDER BY delivery_kind",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            recorded,
            [
                (sha256_hex(text.as_bytes()), "package".to_owned()),
                ("b".repeat(64), "pointer".to_owned()),
            ]
        );
        expand(&pool, "item:long")
            .await
            .expect("a repeated expansion keeps one record");
    }

    #[tokio::test]
    async fn only_replies_the_session_received_as_pointers_in_its_thread_expand() {
        let pool = pool_with_folder_runs().await;
        post_reply(&pool, "item:sent-whole", "short", true).await;
        deliver(&pool, "item:sent-whole", "package").await;
        post_reply(&pool, "item:never-sent", "unseen", true).await;
        post_reply(&pool, "item:elsewhere", "outside", false).await;
        deliver(&pool, "item:elsewhere", "pointer").await;
        post_reply(&pool, "item:deleted", "gone", true).await;
        deliver(&pool, "item:deleted", "pointer").await;
        sqlx::query(
            "UPDATE chat_communication_messages SET deleted_at = ?
             WHERE item_id = 'item:deleted'",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        for id in [
            "item:sent-whole",
            "item:never-sent",
            "item:elsewhere",
            "item:deleted",
            "item:missing",
        ] {
            let error = expand(&pool, id).await.unwrap_err();
            assert_eq!(error.message, super::super::GENERIC_DENIAL, "{id}");
        }
    }

    #[tokio::test]
    async fn a_pointer_from_another_session_does_not_expand() {
        let pool = pool_with_folder_runs().await;
        post_reply(&pool, "item:long", "long reply", true).await;
        deliver(&pool, "item:long", "pointer").await;
        let error = expand_pointer(
            &pool,
            &ChatThreadId::new("thread-other").unwrap(),
            &scope(),
            "item:long",
        )
        .await
        .unwrap_err();
        assert_eq!(error.message, super::super::GENERIC_DENIAL);
    }
}
