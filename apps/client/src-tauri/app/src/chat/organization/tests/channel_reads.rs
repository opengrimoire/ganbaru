//! Channel reads: unread counts, pages, reply thread summaries, reference ranges, and message search.

use super::*;

#[test]
fn channel_unread_count_excludes_local_messages() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group:unread', 'Unread', 'a1');
             INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project:unread', 'group:unread', 'Unread', 'a1');
             INSERT INTO chat_conversations
                 (id, project_id, conversation_kind, last_activity_at,
                  created_at, updated_at)
             VALUES (
                 'conversation:unread', 'project:unread', 'channel',
                 '2026-08-04T19:01:00.000Z', '2026-08-04T19:00:00.000Z',
                 '2026-08-04T19:01:00.000Z'
             );
             INSERT INTO chat_channels
                 (id, project_id, conversation_id, name, created_at, updated_at)
             VALUES (
                 'channel:unread', 'project:unread', 'conversation:unread',
                 'general', '2026-08-04T19:00:00.000Z',
                 '2026-08-04T19:01:00.000Z'
             );
             INSERT INTO chat_participants
                 (id, participant_kind, display_name, created_at, updated_at)
             VALUES (
                 'participant:collaborator', 'human', 'Collaborator',
                 '2026-08-04T19:00:00.000Z', '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_conversation_items
                 (id, conversation_id, item_kind, order_key, created_at)
             VALUES
                 ('item:local-unread', 'conversation:unread', 'message', 'a1',
                  '2026-08-04T19:00:00.000Z'),
                 ('item:incoming-unread', 'conversation:unread', 'message', 'a2',
                  '2026-08-04T19:01:00.000Z');
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES
                 ('item:local-unread', 'participant:local-owner', 'You',
                  '2026-08-04T19:00:00.000Z'),
                 ('item:incoming-unread', 'participant:collaborator', 'Collaborator',
                  '2026-08-04T19:01:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let channel = crate::chat::channels::read_channel(
            &pool,
            &ChatChannelId::new("channel:unread").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(channel.message_count, 2);
        assert_eq!(channel.unread_count, 1);
    });
}

#[test]
fn reply_thread_unread_excludes_local_messages_and_tracks_item_positions() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, order_key)
             VALUES ('group:thread-unread', 'Thread unread', 'a1');
             INSERT INTO projects (id, group_id, name, order_key)
             VALUES ('project:thread-unread', 'group:thread-unread', 'Thread unread', 'a1');
             INSERT INTO chat_conversations
                 (id, project_id, conversation_kind, last_activity_at,
                  created_at, updated_at)
             VALUES (
                 'conversation:thread-unread', 'project:thread-unread', 'channel',
                 '2026-08-04T19:03:00.000Z', '2026-08-04T19:00:00.000Z',
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_participants
                 (id, participant_kind, display_name, created_at, updated_at)
             VALUES (
                 'participant:thread-collaborator', 'human', 'Collaborator',
                 '2026-08-04T19:00:00.000Z', '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_conversation_items
                 (id, conversation_id, item_kind, order_key, created_at)
             VALUES (
                 'item:thread-root', 'conversation:thread-unread', 'message', 'a1',
                 '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-root', 'participant:local-owner', 'You',
                 '2026-08-04T19:00:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-root', 'item:thread-root', 'Root',
                 '2026-08-04T19:00:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-root'
             WHERE item_id = 'item:thread-root';
             INSERT INTO chat_reply_threads
                 (id, conversation_id, root_item_id, last_activity_at,
                  created_at, updated_at)
             VALUES (
                 'reply-thread:unread', 'conversation:thread-unread',
                 'item:thread-root', '2026-08-04T19:02:00.000Z',
                 '2026-08-04T19:00:00.000Z', '2026-08-04T19:02:00.000Z'
             );
             INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES
                 ('item:thread-work', 'conversation:thread-unread',
                  'reply-thread:unread', 'work_update', 'a1',
                  '2026-08-04T19:01:00.000Z'),
                 ('item:thread-local', 'conversation:thread-unread',
                  'reply-thread:unread', 'message', 'a2',
                  '2026-08-04T19:02:00.000Z');
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-local', 'participant:local-owner', 'You',
                 '2026-08-04T19:02:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-local', 'item:thread-local', 'Local reply',
                 '2026-08-04T19:02:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-local'
             WHERE item_id = 'item:thread-local';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let thread_id = ChatReplyThreadId::new("reply-thread:unread").unwrap();

        let local_only = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(!local_only.unread);

        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES (
                 'item:thread-incoming', 'conversation:thread-unread',
                 'reply-thread:unread', 'message', 'a3',
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-incoming', 'participant:thread-collaborator', 'Collaborator',
                 '2026-08-04T19:03:00.000Z'
             );
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (
                 'revision:thread-incoming', 'item:thread-incoming',
                 'Incoming reply', '2026-08-04T19:03:00.000Z'
             );
             UPDATE chat_communication_messages
             SET current_revision_id = 'revision:thread-incoming'
             WHERE item_id = 'item:thread-incoming';
             UPDATE chat_reply_threads
             SET last_activity_at = '2026-08-04T19:03:00.000Z',
                 updated_at = '2026-08-04T19:03:00.000Z'
             WHERE id = 'reply-thread:unread';",
        )
        .execute(&pool)
        .await
        .unwrap();

        let incoming = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(incoming.unread);

        super::reads::read_reply_thread_page(&pool, &thread_id, None, 50)
            .await
            .unwrap();
        let read_position: (String, String) = sqlx::query_as(
            "SELECT last_read_order_key, last_read_item_id FROM chat_reply_thread_read_cursors
             WHERE reply_thread_id = ? AND participant_id = 'participant:local-owner'",
        )
        .bind(thread_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            read_position,
            ("a3".to_owned(), "item:thread-incoming".to_owned())
        );
        let read = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(!read.unread);

        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES (
                 'item:thread-local-after-read', 'conversation:thread-unread',
                 'reply-thread:unread', 'message', 'a4',
                 '2026-08-04T19:04:00.000Z'
             );
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES (
                 'item:thread-local-after-read', 'participant:local-owner', 'You',
                 '2026-08-04T19:04:00.000Z'
             );",
        )
        .execute(&pool)
        .await
        .unwrap();
        let local_after_read = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert!(!local_after_read.unread);
    });
}

/// Seeds root messages of the review channel as `(item id, order key, deleted)`.
async fn seed_review_root_messages(pool: &SqlitePool, messages: &[(&str, &str, bool)]) {
    for (item_id, order_key, deleted) in messages {
        let revision_id = format!("revision:{item_id}");
        sqlx::query(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, item_kind, order_key, created_at)
             VALUES (?, 'conversation:review', 'message', ?, '2026-08-04T17:05:00.000Z')",
        )
        .bind(item_id)
        .bind(order_key)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, deleted_at, created_at)
             VALUES (?, 'participant:local-owner', 'You', ?, '2026-08-04T17:05:00.000Z')",
        )
        .bind(item_id)
        .bind(deleted.then_some("2026-08-04T17:06:00.000Z"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES (?, ?, ?, '2026-08-04T17:05:00.000Z')",
        )
        .bind(&revision_id)
        .bind(item_id)
        .bind(format!("Text of {item_id}"))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?",
        )
        .bind(&revision_id)
        .bind(item_id)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[test]
fn channel_pages_break_equal_order_keys_by_item_id_without_gaps() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_review_root_messages(
            &pool,
            &[("item:tie-b", "a2", false), ("item:tie-a", "a2", false)],
        )
        .await;
        let channel_id = ChatChannelId::new("channel:review").unwrap();
        let ids = |page: &ChatChannelPageRead| {
            page.messages
                .iter()
                .map(|message| message.item_id.as_str().to_owned())
                .collect::<Vec<_>>()
        };

        let newest = read_channel_page(&pool, channel_id.clone(), None, Some(1))
            .await
            .unwrap();
        assert_eq!(ids(&newest), ["item:tie-b"]);
        assert_eq!(newest.messages[0].order_key, "a2");
        let middle = read_channel_page(
            &pool,
            channel_id.clone(),
            newest.previous_cursor.as_deref(),
            Some(1),
        )
        .await
        .unwrap();
        assert_eq!(ids(&middle), ["item:tie-a"]);
        let oldest = read_channel_page(
            &pool,
            channel_id.clone(),
            middle.previous_cursor.as_deref(),
            Some(2),
        )
        .await
        .unwrap();
        assert_eq!(ids(&oldest), ["item:review-root"]);
        assert_eq!(oldest.previous_cursor, None);

        let through = super::common::PageCursor::through("a2", "item:tie-a");
        let anchored = read_channel_page(&pool, channel_id, Some(&through), Some(5))
            .await
            .unwrap();
        assert_eq!(ids(&anchored), ["item:review-root", "item:tie-a"]);
    });
}

#[test]
fn reply_thread_summaries_count_only_live_reply_messages() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let thread_id = ChatReplyThreadId::new("reply-thread:review").unwrap();
        let summary = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert_eq!(
            summary.reply_count, 0,
            "replies waiting for the work are in the queue"
        );
        sqlx::raw_sql(
            "UPDATE chat_work_assignment_inputs
             SET delivery_state = 'delivered', delivered_at = '2026-08-04T17:02:30.000Z'
             WHERE assignment_id = 'assignment:review' AND routing_kind <> 'trigger'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let summary = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert_eq!(summary.reply_count, 2);

        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             VALUES ('item:review-update', 'conversation:review', 'reply-thread:review',
                     'work_update', 'a3', '2026-08-04T17:03:00.000Z');
             UPDATE chat_communication_messages SET deleted_at = '2026-08-04T17:04:00.000Z'
             WHERE item_id = 'item:review-reply-1';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let summary = super::reads::read_reply_thread_summary(&pool, &thread_id)
            .await
            .unwrap();
        assert_eq!(summary.reply_count, 1);
    });
}

#[test]
fn channel_reference_ranges_break_equal_order_keys_at_both_ends() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_review_root_messages(
            &pool,
            &[
                ("item:tie-a", "a2", false),
                ("item:tie-b", "a2", false),
                ("item:tie-c", "a2", true),
            ],
        )
        .await;
        let first_root: String = sqlx::query_scalar(
            "SELECT min(order_key) FROM chat_conversation_items
             WHERE conversation_id = 'conversation:review' AND reply_thread_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut request = message("See general");
        request.channel_id = ChatChannelId::new("channel:review").unwrap();
        request.reply_thread_id = Some(ChatReplyThreadId::new("reply-thread:review").unwrap());
        request.references = vec![ChatMessageReference::Channel {
            metadata: ChatReferenceMetadata {
                reference_id: ChatMessageReferenceId::new("reference:general").unwrap(),
                label_snapshot: "general".to_owned(),
                start_offset: 4,
                end_offset: 11,
                plain_text_projection: "#general".to_owned(),
            },
            channel_id: ChatChannelId::new("channel:review").unwrap(),
        }];
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let mut transaction = pool.begin().await.unwrap();
        super::workflow::insert_communication_message(
            &mut transaction,
            super::workflow::CommunicationMessageWrite {
                author: super::workflow::MessageAuthor::LocalPerson,
                item_id: &ChatConversationItemId::new("item:review-reference").unwrap(),
                revision_id: &ChatMessageRevisionId::new("revision:review-reference").unwrap(),
                conversation_id: &ChatConversationId::new("conversation:review").unwrap(),
                reply_thread_id: request.reply_thread_id.as_ref(),
                order_key: "a3",
                request: &request,
                attachments: super::common::AttachmentSource::Device(&request.attachment_ids),
                invoked_agent_id: Some(&agent_id),
                now: &UtcTimestamp::new("2026-08-04T17:07:00.000Z").unwrap(),
            },
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let bounds: (String, String, String, String) = sqlx::query_as(
            "SELECT source_lower_order_key, source_lower_item_id,
                    source_high_order_key, source_high_item_id
             FROM chat_channel_reference_targets WHERE reference_id = 'reference:general'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            bounds,
            (
                first_root,
                String::new(),
                "a2".to_owned(),
                "item:tie-b".to_owned()
            )
        );

        seed_review_root_messages(&pool, &[("item:tie-z", "a2", false)]).await;
        let in_range: Vec<String> = sqlx::query_scalar(
            "SELECT item.id FROM chat_conversation_items item
             JOIN chat_channel_reference_targets target
               ON target.reference_id = 'reference:general'
             WHERE item.conversation_id = target.source_conversation_id
               AND item.reply_thread_id IS NULL
               AND (item.order_key, item.id)
                   >= (target.source_lower_order_key, target.source_lower_item_id)
               AND (item.order_key, item.id)
                   <= (target.source_high_order_key, target.source_high_item_id)
             ORDER BY item.order_key, item.id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(
            in_range.ends_with(&["item:tie-a".to_owned(), "item:tie-b".to_owned()]),
            "the range ends at the last live root of equal order key: {in_range:?}"
        );
    });
}

#[test]
fn message_search_keeps_messages_whose_author_identity_is_gone() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_review_root_messages(
            &pool,
            &[
                ("item:attributed", "a5", false),
                ("item:unattributed", "a6", false),
            ],
        )
        .await;
        sqlx::raw_sql(
            "UPDATE chat_communication_messages SET author_participant_id = NULL
             WHERE item_id = 'item:unattributed';
             UPDATE chat_participants SET display_name = 'Renamed owner'
             WHERE id = 'participant:local-owner';",
        )
        .execute(&pool)
        .await
        .unwrap();

        let results = search_messages(
            &pool,
            normalized_fts_query("Text of item").unwrap(),
            Some("project:review"),
            None,
        )
        .await
        .unwrap();
        let result = |item_id: &str| {
            results
                .iter()
                .find(|result| result.message_item_id.as_str() == item_id)
                .unwrap_or_else(|| panic!("search dropped {item_id}"))
        };

        let attributed = result("item:attributed");
        assert_eq!(
            attributed
                .author_participant_id
                .as_ref()
                .map(ChatParticipantId::as_str),
            Some("participant:local-owner")
        );
        assert_eq!(attributed.author_kind, Some(ChatParticipantKind::LocalUser));
        assert_eq!(attributed.author_display_name, "Renamed owner");
        let unattributed = result("item:unattributed");
        assert_eq!(unattributed.author_participant_id, None);
        assert_eq!(unattributed.author_kind, None);
        assert_eq!(unattributed.author_display_name, "You");
    });
}

/// A mention that arrived for a named agent a linked device deleted concurrently still reads,
/// naming the absent agent.
#[test]
fn a_mention_of_a_deleted_named_agent_still_reads() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_with_unrouted_mention().await;
        sqlx::raw_sql(
            "INSERT INTO chat_message_references
                 (id, message_revision_id, reference_kind, label_snapshot,
                  plain_text_projection, start_offset, end_offset, ordinal, created_at)
             VALUES ('reference:named', 'revision:review-reply-2', 'participant', 'Named',
                     '@Named', 14, 20, 1, '2026-08-04T17:02:00.000Z');
             INSERT INTO chat_participant_reference_targets (reference_id, participant_id)
             VALUES ('reference:named', 'agent:0123456789abcdef0123456789abcdef');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let references = super::workflow::read_message_references(
            &pool,
            &ganbaru_chat_contracts::models::ChatMessageRevisionId::new("revision:review-reply-2")
                .unwrap(),
        )
        .await
        .unwrap();
        let named = references
            .iter()
            .find_map(|reference| match reference {
                ChatMessageReference::Participant {
                    participant_id,
                    participant_kind,
                    ..
                } if participant_id.as_str() == NAMED_AGENT_ID => Some(*participant_kind),
                _ => None,
            })
            .expect("the mention of the absent agent reads");
        assert_eq!(named, ChatParticipantKind::AiAgent);
        assert_eq!(references.len(), 2);
    });
}
