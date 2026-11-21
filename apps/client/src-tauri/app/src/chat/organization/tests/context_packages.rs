//! Context packages frozen for work: the records they include and how long replies are delivered.

use super::*;

/// Freezes the seeded review assignment for a reply that mentions the agent, with an optional
/// replacement text for the older thread reply, and returns the source kinds and the excluded
/// thread reply count.
async fn freeze_review_reply_context(older_reply_text: Option<&str>) -> (Vec<String>, i64) {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    if let Some(text) = older_reply_text {
        sqlx::query(
            "INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES ('revision:review-reply-1-edit', 'item:review-reply-1', ?,
                     '2026-08-04T17:01:30.000Z')",
        )
        .bind(text)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_communication_messages
             SET current_revision_id = 'revision:review-reply-1-edit'
             WHERE item_id = 'item:review-reply-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:review-later', 'conversation:review', 'message', 'a2',
                 '2026-08-04T17:03:00.000Z');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES ('item:review-later', 'participant:local-owner', 'You',
                 '2026-08-04T17:03:00.000Z');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:review-later', 'item:review-later', 'Later update',
                 '2026-08-04T17:03:00.000Z');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-later'
         WHERE item_id = 'item:review-later';",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    super::context::freeze_context_package(
        &mut transaction,
        super::context::WorkToFreeze {
            assignment_id: &ChatWorkAssignmentId::new("assignment:review").unwrap(),
            reply_thread_id: &ChatReplyThreadId::new("reply-thread:review").unwrap(),
            triggering_message_item_id: &ChatConversationItemId::new("item:review-reply-2")
                .unwrap(),
            agent_id: &ChatParticipantId::new("participant:review-agent").unwrap(),
        },
        None,
        &test_providers(),
        &UtcTimestamp::new("2026-08-04T17:04:00.000Z").unwrap(),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let kinds = sqlx::query_scalar(
        "SELECT source.source_kind || ':' || source.source_id
         FROM chat_assignment_context_sources source
         JOIN chat_assignment_context_packages package ON package.id = source.context_package_id
         WHERE package.assignment_id = 'assignment:review'
         ORDER BY source.ordinal",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let excluded = sqlx::query_scalar(
        "SELECT excluded_thread_reply_count FROM chat_assignment_context_packages
         WHERE assignment_id = 'assignment:review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    (kinds, excluded)
}

#[test]
fn context_packages_include_the_thread_and_later_channel_messages() {
    tauri::async_runtime::block_on(async {
        let (kinds, excluded) = freeze_review_reply_context(None).await;
        assert_eq!(
            kinds,
            [
                "thread_root:item:review-root",
                "thread_reply:item:review-reply-1",
                "channel_message:item:review-later",
                "trigger:item:review-reply-2",
            ]
            .map(String::from)
        );
        assert_eq!(excluded, 0);
    });
}

#[test]
fn context_package_sources_list_only_the_records_that_fit() {
    tauri::async_runtime::block_on(async {
        let oversized_reply = "x".repeat(super::common::MAX_MESSAGE_BYTES - 64);
        let (kinds, excluded) = freeze_review_reply_context(Some(&oversized_reply)).await;
        assert_eq!(
            kinds,
            [
                "thread_root:item:review-root",
                "channel_message:item:review-later",
                "trigger:item:review-reply-2",
            ]
            .map(String::from)
        );
        assert_eq!(excluded, 1);
    });
}

/// Freezes the review assignment after `author` wrote a 10 KiB older reply, and returns the
/// pool, the frozen package id, and its serialized text.
async fn freeze_long_older_reply(author: &str) -> (SqlitePool, String, String, String) {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    let long_reply = "Long answer. ".repeat(10 * 1024 / 13 + 1);
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:review-reply-1-long', 'item:review-reply-1', ?,
                 '2026-08-04T17:01:30.000Z')",
    )
    .bind(&long_reply)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-reply-1-long', author_participant_id = ?
         WHERE item_id = 'item:review-reply-1'",
    )
    .bind(author)
    .execute(&pool)
    .await
    .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    super::context::freeze_context_package(
        &mut transaction,
        super::context::WorkToFreeze {
            assignment_id: &ChatWorkAssignmentId::new("assignment:review").unwrap(),
            reply_thread_id: &ChatReplyThreadId::new("reply-thread:review").unwrap(),
            triggering_message_item_id: &ChatConversationItemId::new("item:review-reply-2")
                .unwrap(),
            agent_id: &ChatParticipantId::new("participant:review-agent").unwrap(),
        },
        None,
        &test_providers(),
        &UtcTimestamp::new("2026-08-04T17:04:00.000Z").unwrap(),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let (package_id, serialized): (String, String) = sqlx::query_as(
        "SELECT id, serialized_text FROM chat_assignment_context_packages
         WHERE assignment_id = 'assignment:review'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    (pool, package_id, serialized, long_reply)
}

#[test]
fn frozen_packages_point_to_long_agent_replies_and_deliver_them_as_pointers() {
    use ganbaru_chat::agent_runs::DeliveryKind;
    tauri::async_runtime::block_on(async {
        let (pool, package_id, serialized, long_reply) =
            freeze_long_older_reply("participant:review-agent").await;

        let records = super::continuation::read_package_records(&pool, &package_id, &serialized)
            .await
            .unwrap();
        let pointer = records
            .iter()
            .find(|record| {
                record
                    .source
                    .as_ref()
                    .is_some_and(|source| source.source_id == "item:review-reply-1")
            })
            .unwrap();
        let record: serde_json::Value = serde_json::from_str(pointer.line).unwrap();
        assert_eq!(record["kind"], "pointer");
        assert_eq!(record["bytes"], long_reply.len());
        assert!(record.get("text").is_none());
        let source = pointer.source.as_ref().unwrap();
        assert_eq!(source.source_kind, "thread_reply");
        assert_eq!(
            source.content_sha256,
            super::common::sha256_hex(long_reply.as_bytes())
        );
        assert_eq!(
            super::context::record_delivery_kind(pointer.line),
            DeliveryKind::Pointer
        );

        let assignment_id = ChatWorkAssignmentId::new("assignment:review").unwrap();
        let fresh = super::continuation::select_delta(
            &assignment_id,
            &records,
            &super::continuation::SessionHistory::default(),
        )
        .unwrap()
        .unwrap();
        assert!(fresh.sources.iter().any(|delivered| {
            delivered.source_id == "item:review-reply-1" && delivered.kind == DeliveryKind::Pointer
        }));
        let mut history = super::continuation::SessionHistory::default();
        history
            .delivered
            .insert((source.source_id.clone(), source.content_sha256.clone()));
        let later = super::continuation::select_delta(&assignment_id, &records, &history)
            .unwrap()
            .unwrap();
        assert!(
            later
                .sources
                .iter()
                .all(|delivered| delivered.source_id != "item:review-reply-1")
        );
        assert!(!later.text.contains("item:review-reply-1"));
    });
}

#[test]
fn frozen_packages_keep_long_human_replies_as_text() {
    tauri::async_runtime::block_on(async {
        let (pool, package_id, serialized, long_reply) =
            freeze_long_older_reply("participant:local-owner").await;

        let records = super::continuation::read_package_records(&pool, &package_id, &serialized)
            .await
            .unwrap();
        let reply = records
            .iter()
            .find(|record| {
                record
                    .source
                    .as_ref()
                    .is_some_and(|source| source.source_id == "item:review-reply-1")
            })
            .unwrap();
        let record: serde_json::Value = serde_json::from_str(reply.line).unwrap();
        assert_eq!(record["kind"], "thread_reply");
        assert_eq!(record["text"], long_reply);
    });
}
