use super::{LOCAL_PARTICIPANT_ID, mark_conversation_read, read_channel, timestamp};
use ganbaru_chat_contracts::models::{ChatChannelId, ChatConversationId};

const NOW: &str = "2026-10-20T12:00:00.000Z";
const LATER: &str = "2026-10-20T12:05:00.000Z";

async fn project_channel(pool: &sqlx::SqlitePool) -> (ChatChannelId, ChatConversationId) {
    let (channel_id, conversation_id): (String, String) = sqlx::query_as(
        "SELECT id, conversation_id FROM chat_channels WHERE project_id = 'project-chat'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    (
        ChatChannelId::new(channel_id).unwrap(),
        ChatConversationId::new(conversation_id).unwrap(),
    )
}

async fn seed_agent_messages(
    pool: &sqlx::SqlitePool,
    conversation_id: &ChatConversationId,
    messages: &[(&str, &str, bool)],
) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT OR IGNORE INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant:agent', 'ai_agent', 'Agent', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    for (item_id, order_key, deleted) in messages {
        let deleted_at = deleted.then_some(NOW);
        let revision_id = format!("revision:{item_id}");
        sqlx::query(
            "INSERT INTO chat_conversation_items
                    (id, conversation_id, item_kind, order_key, created_at)
                 VALUES (?, ?, 'message', ?, ?)",
        )
        .bind(item_id)
        .bind(conversation_id.as_str())
        .bind(order_key)
        .bind(NOW)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_messages
                    (item_id, author_participant_id, deleted_at, created_at)
                 VALUES (?, 'participant:agent', ?, ?)",
        )
        .bind(item_id)
        .bind(deleted_at)
        .bind(NOW)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_message_revisions
                    (id, message_item_id, normalized_markdown, created_at)
                 VALUES (?, ?, ?, ?)",
        )
        .bind(&revision_id)
        .bind(item_id)
        .bind(format!("Text of {item_id}"))
        .bind(NOW)
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

async fn local_cursor(
    pool: &sqlx::SqlitePool,
    conversation_id: &ChatConversationId,
) -> (String, String, String) {
    sqlx::query_as(
        "SELECT last_read_order_key, last_read_item_id, updated_at
             FROM chat_conversation_read_cursors
             WHERE conversation_id = ? AND participant_id = ?",
    )
    .bind(conversation_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn channel_summaries_leave_out_deleted_messages() {
    let pool = crate::chat::tests::repository::pool_with_thread().await;
    let (channel_id, conversation_id) = project_channel(&pool).await;
    seed_agent_messages(
        &pool,
        &conversation_id,
        &[("item:kept", "c000", false), ("item:deleted", "c001", true)],
    )
    .await;

    let channel = read_channel(&pool, &channel_id).await.unwrap();

    assert_eq!(channel.latest_preview.as_deref(), Some("Text of item:kept"));
    assert_eq!(channel.message_count, 1);
    assert_eq!(channel.unread_count, 1);
}

#[tokio::test]
async fn unread_counts_break_equal_order_keys_by_item_id() {
    let pool = crate::chat::tests::repository::pool_with_thread().await;
    let (channel_id, conversation_id) = project_channel(&pool).await;
    seed_agent_messages(
        &pool,
        &conversation_id,
        &[("item:a", "c000", false), ("item:b", "c000", false)],
    )
    .await;
    sqlx::query(
        "INSERT INTO chat_conversation_read_cursors
                (conversation_id, participant_id, sync_key, last_read_order_key,
                 last_read_item_id, updated_at)
             VALUES (?, ?, 'cursor-test', 'c000', 'item:a', ?)",
    )
    .bind(conversation_id.as_str())
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();

    let channel = read_channel(&pool, &channel_id).await.unwrap();

    assert_eq!(channel.unread_count, 1);
    assert_eq!(channel.latest_preview.as_deref(), Some("Text of item:b"));
}

#[tokio::test]
async fn read_cursor_never_moves_backward_or_rewrites_an_unchanged_position() {
    let pool = crate::chat::tests::repository::pool_with_thread().await;
    let (channel_id, conversation_id) = project_channel(&pool).await;
    seed_agent_messages(
        &pool,
        &conversation_id,
        &[("item:a", "c000", false), ("item:b", "c000", false)],
    )
    .await;
    let now = timestamp(NOW.to_owned()).unwrap();
    let later = timestamp(LATER.to_owned()).unwrap();

    mark_conversation_read(&pool, &conversation_id, LOCAL_PARTICIPANT_ID, &now)
        .await
        .unwrap();
    assert_eq!(
        local_cursor(&pool, &conversation_id).await,
        ("c000".to_owned(), "item:b".to_owned(), NOW.to_owned())
    );

    mark_conversation_read(&pool, &conversation_id, LOCAL_PARTICIPANT_ID, &later)
        .await
        .unwrap();
    assert_eq!(
        local_cursor(&pool, &conversation_id).await,
        ("c000".to_owned(), "item:b".to_owned(), NOW.to_owned())
    );

    sqlx::query(
        "UPDATE chat_conversation_read_cursors
             SET last_read_order_key = 'c900', last_read_item_id = 'item:remote'
             WHERE conversation_id = ?",
    )
    .bind(conversation_id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    mark_conversation_read(&pool, &conversation_id, LOCAL_PARTICIPANT_ID, &later)
        .await
        .unwrap();
    assert_eq!(
        local_cursor(&pool, &conversation_id).await,
        ("c900".to_owned(), "item:remote".to_owned(), NOW.to_owned())
    );
    assert_eq!(
        read_channel(&pool, &channel_id).await.unwrap().unread_count,
        0
    );
}

mod attention {
    use super::super::{
        LOCAL_PARTICIPANT_ID, mark_conversation_read, read_channel, read_channels, timestamp,
    };
    use crate::chat::tests::attention::{
        SeedAssignment, days_ago, minutes_after_seed, seed_agent, seed_assignment, seed_channel,
        seed_failure_update, seed_reply, seed_reply_thread, seed_reply_thread_cursor,
    };
    use ganbaru_chat_contracts::models::{
        ChatChannelId, ChatConversationId, ChatReplyThreadId, ChatReplyThreadSummaryRead,
        ChatWorkAssignmentState,
    };

    const AGENT: &str = "participant:attention-agent";

    /// Opens the channel `channel_id` at `minutes` after the seed time.
    async fn open_channel_at(pool: &sqlx::SqlitePool, channel_id: &ChatChannelId, minutes: i64) {
        let conversation_id = read_channel(pool, channel_id)
            .await
            .unwrap()
            .conversation_id;
        mark_conversation_read(
            pool,
            &conversation_id,
            LOCAL_PARTICIPANT_ID,
            &timestamp(minutes_after_seed(minutes)).unwrap(),
        )
        .await
        .unwrap();
    }

    async fn unseen_failure_of(pool: &sqlx::SqlitePool, channel_id: &ChatChannelId) -> bool {
        read_channel(pool, channel_id).await.unwrap().unseen_failure
    }

    async fn unread_replies_of(pool: &sqlx::SqlitePool, channel_id: &ChatChannelId) -> u64 {
        read_channel(pool, channel_id)
            .await
            .unwrap()
            .unread_reply_count
    }

    /// A failed assignment in `thread` with its failure message `item_id`, posted
    /// `minutes` after the seed time.
    async fn seed_failure(
        pool: &sqlx::SqlitePool,
        assignment: &SeedAssignment<'_>,
        item_id: &str,
        order_key: &str,
        minutes: i64,
    ) {
        seed_assignment(pool, assignment).await;
        seed_failure_update(
            pool,
            assignment.reply_thread_id,
            assignment.id,
            AGENT,
            item_id,
            order_key,
            &minutes_after_seed(minutes),
        )
        .await;
    }

    /// A channel of the seeded project with `threads` reply threads and an agent.
    async fn channel_with_threads(
        pool: &sqlx::SqlitePool,
        channel_id: &str,
        threads: &[&str],
    ) -> ChatChannelId {
        let conversation_id =
            seed_channel(pool, "project-chat", channel_id, channel_id, false).await;
        for thread in threads {
            seed_reply_thread(pool, &conversation_id, thread).await;
        }
        ChatChannelId::new(channel_id).unwrap()
    }

    async fn attention_of(
        pool: &sqlx::SqlitePool,
        channel_id: &ChatChannelId,
    ) -> Option<ChatWorkAssignmentState> {
        read_channel(pool, channel_id)
            .await
            .unwrap()
            .attention_state
    }

    async fn thread_summary(
        pool: &sqlx::SqlitePool,
        reply_thread_id: &str,
    ) -> ChatReplyThreadSummaryRead {
        crate::chat::organization::read_reply_thread_summary(
            pool,
            &ChatReplyThreadId::new(reply_thread_id).unwrap(),
        )
        .await
        .unwrap()
    }

    /// The time the local read cursor of `conversation_id` was last stamped.
    async fn cursor_stamp(pool: &sqlx::SqlitePool, conversation_id: &ChatConversationId) -> String {
        sqlx::query_scalar(
            "SELECT updated_at FROM chat_conversation_read_cursors
                 WHERE conversation_id = ? AND participant_id = ?",
        )
        .bind(conversation_id.as_str())
        .bind(LOCAL_PARTICIPANT_ID)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn attention_prefers_answer_over_approval_over_review_and_leaves_out_failures() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let channel = channel_with_threads(
            &pool,
            "channel:priority",
            &["thread:a", "thread:b", "thread:c", "thread:d"],
        )
        .await;
        let mut failed = SeedAssignment::new("assignment:d", "thread:d", AGENT, "failed", 40);
        failed.settled_at = Some(days_ago(1));
        seed_failure(&pool, &failed, "item:failure:d", "a1", 40).await;
        assert_eq!(attention_of(&pool, &channel).await, None);
        assert!(unseen_failure_of(&pool, &channel).await);

        let expected = [
            (
                "assignment:c",
                "thread:c",
                "ready_for_review",
                ChatWorkAssignmentState::ReadyForReview,
            ),
            (
                "assignment:b",
                "thread:b",
                "waiting_for_approval",
                ChatWorkAssignmentState::WaitingForApproval,
            ),
            (
                "assignment:a",
                "thread:a",
                "waiting_for_answer",
                ChatWorkAssignmentState::WaitingForAnswer,
            ),
        ];
        for (id, thread, state, attention) in expected {
            seed_assignment(&pool, &SeedAssignment::new(id, thread, AGENT, state, 1)).await;
            assert_eq!(attention_of(&pool, &channel).await, Some(attention));
        }
    }

    #[tokio::test]
    async fn a_failure_continued_by_a_later_assignment_is_not_marked() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let channel = channel_with_threads(&pool, "channel:successor", &["thread:a"]).await;
        let failed = SeedAssignment::new("assignment:failed", "thread:a", AGENT, "failed", 1);
        seed_failure(&pool, &failed, "item:failure", "a1", 1).await;
        assert!(unseen_failure_of(&pool, &channel).await);

        let mut successor =
            SeedAssignment::new("assignment:successor", "thread:a", AGENT, "completed", 2);
        successor.previous_assignment_id = Some("assignment:failed");
        seed_assignment(&pool, &successor).await;

        assert!(!unseen_failure_of(&pool, &channel).await);
    }

    #[tokio::test]
    async fn a_retried_failure_is_not_marked() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let channel = channel_with_threads(&pool, "channel:retried", &["thread:a"]).await;
        let failed = SeedAssignment::new("assignment:a", "thread:a", AGENT, "failed", 1);
        seed_failure(&pool, &failed, "item:failure", "a1", 1).await;

        sqlx::query(
            "UPDATE chat_work_assignments SET state = 'queued', settled_at = NULL
                 WHERE id = 'assignment:a'",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(!unseen_failure_of(&pool, &channel).await);
        assert_eq!(
            attention_of(&pool, &channel).await,
            Some(ChatWorkAssignmentState::Queued)
        );
    }

    #[tokio::test]
    async fn an_unseen_failure_clears_when_the_channel_is_opened_after_it() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let channel = channel_with_threads(&pool, "channel:opened", &["thread:a"]).await;
        open_channel_at(&pool, &channel, 1).await;
        let failed = SeedAssignment::new("assignment:a", "thread:a", AGENT, "failed", 5);
        seed_failure(&pool, &failed, "item:failure", "a1", 5).await;
        assert!(
            unseen_failure_of(&pool, &channel).await,
            "a failure after the last visit is unseen"
        );

        open_channel_at(&pool, &channel, 10).await;

        assert!(!unseen_failure_of(&pool, &channel).await);
        let thread = thread_summary(&pool, "thread:a").await;
        assert!(
            thread.unseen_failure,
            "the thread keeps its marker until the thread itself is read"
        );
    }

    #[tokio::test]
    async fn an_unseen_failure_clears_when_its_thread_is_read() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let channel = channel_with_threads(&pool, "channel:thread-read", &["thread:a"]).await;
        let failed = SeedAssignment::new("assignment:a", "thread:a", AGENT, "failed", 5);
        seed_failure(&pool, &failed, "item:failure", "a1", 5).await;

        seed_reply_thread_cursor(&pool, "thread:a", "a1", "item:failure").await;

        assert!(!unseen_failure_of(&pool, &channel).await);
        let thread = thread_summary(&pool, "thread:a").await;
        assert!(!thread.unseen_failure);
    }

    #[tokio::test]
    async fn a_delegated_failure_waits_on_its_open_parent() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let parent_channel =
            channel_with_threads(&pool, "channel:parent", &["thread:parent"]).await;
        let child_channel = channel_with_threads(&pool, "channel:child", &["thread:child"]).await;
        seed_assignment(
            &pool,
            &SeedAssignment::new("assignment:parent", "thread:parent", AGENT, "working", 1),
        )
        .await;
        let mut child = SeedAssignment::new("assignment:child", "thread:child", AGENT, "failed", 2);
        child.parent_assignment_id = Some("assignment:parent");
        seed_failure(&pool, &child, "item:failure:child", "a1", 2).await;

        assert_eq!(
            attention_of(&pool, &parent_channel).await,
            Some(ChatWorkAssignmentState::Working)
        );
        assert!(
            !unseen_failure_of(&pool, &child_channel).await,
            "the parent receives the failure as its outcome"
        );

        sqlx::query(
            "UPDATE chat_work_assignments SET state = 'completed', settled_at = ?
                 WHERE id = 'assignment:parent'",
        )
        .bind(days_ago(0))
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            unseen_failure_of(&pool, &child_channel).await,
            "a settled parent no longer handles the failure"
        );
    }

    #[tokio::test]
    async fn unread_replies_count_only_threads_the_person_took_part_in() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let channel = channel_with_threads(
            &pool,
            "channel:replies",
            &["thread:own", "thread:joined", "thread:other"],
        )
        .await;
        // `thread:joined` and `thread:other` start from an agent's message.
        sqlx::query(
            "UPDATE chat_communication_messages SET author_participant_id = ?
                 WHERE item_id IN ('item:root:thread:joined', 'item:root:thread:other')",
        )
        .bind(AGENT)
        .execute(&pool)
        .await
        .unwrap();
        let unread_roots = read_channel(&pool, &channel).await.unwrap().unread_count;
        let at = minutes_after_seed(5);
        seed_reply(&pool, "thread:own", "item:own:1", AGENT, "a1", &at).await;
        seed_reply(&pool, "thread:own", "item:own:2", AGENT, "a2", &at).await;
        seed_reply(
            &pool,
            "thread:own",
            "item:own:mine",
            LOCAL_PARTICIPANT_ID,
            "a3",
            &at,
        )
        .await;
        seed_reply(
            &pool,
            "thread:joined",
            "item:joined:mine",
            LOCAL_PARTICIPANT_ID,
            "a1",
            &at,
        )
        .await;
        seed_reply(&pool, "thread:joined", "item:joined:1", AGENT, "a2", &at).await;
        seed_reply(&pool, "thread:other", "item:other:1", AGENT, "a1", &at).await;

        assert_eq!(unread_replies_of(&pool, &channel).await, 3);
        assert_eq!(
            read_channel(&pool, &channel).await.unwrap().unread_count,
            unread_roots,
            "replies never count as top-level messages"
        );

        seed_reply_thread_cursor(&pool, "thread:own", "a1", "item:own:1").await;
        assert_eq!(
            unread_replies_of(&pool, &channel).await,
            2,
            "a reply read in its thread no longer counts"
        );

        open_channel_at(&pool, &channel, 10).await;
        assert_eq!(unread_replies_of(&pool, &channel).await, 0);

        seed_reply(
            &pool,
            "thread:joined",
            "item:joined:2",
            AGENT,
            "a3",
            &minutes_after_seed(20),
        )
        .await;
        assert_eq!(
            unread_replies_of(&pool, &channel).await,
            1,
            "a reply after the visit counts again"
        );
    }

    #[tokio::test]
    async fn opening_a_channel_stamps_its_cursor_only_when_replies_arrived() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        let channel = channel_with_threads(&pool, "channel:stamp", &["thread:a"]).await;
        open_channel_at(&pool, &channel, 1).await;
        let conversation_id = read_channel(&pool, &channel).await.unwrap().conversation_id;
        assert_eq!(
            cursor_stamp(&pool, &conversation_id).await,
            minutes_after_seed(1)
        );

        open_channel_at(&pool, &channel, 2).await;
        assert_eq!(
            cursor_stamp(&pool, &conversation_id).await,
            minutes_after_seed(1),
            "a quiet channel writes nothing"
        );

        seed_reply(
            &pool,
            "thread:a",
            "item:reply",
            AGENT,
            "a1",
            &minutes_after_seed(3),
        )
        .await;
        open_channel_at(&pool, &channel, 4).await;
        assert_eq!(
            cursor_stamp(&pool, &conversation_id).await,
            minutes_after_seed(4)
        );
    }

    #[tokio::test]
    async fn many_assignments_read_as_one_row_per_channel() {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        seed_agent(&pool, AGENT, "Attention agent").await;
        for (channel, state) in [
            ("channel:busy-a", "ready_for_review"),
            ("channel:busy-b", "working"),
        ] {
            let conversation_id = sqlx::query_scalar::<_, String>(
                "SELECT conversation_id FROM chat_channels WHERE id = ?",
            )
            .bind(channel_with_threads(&pool, channel, &[]).await.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
            for index in 0..300 {
                let thread = format!("thread:{channel}:{index}");
                seed_reply_thread(&pool, &conversation_id, &thread).await;
                let id = format!("assignment:{channel}:{index}");
                let state = if index == 150 { state } else { "completed" };
                seed_assignment(
                    &pool,
                    &SeedAssignment::new(&id, &thread, AGENT, state, index),
                )
                .await;
            }
        }

        let channels = read_channels(&pool, Some("project-chat"), false)
            .await
            .unwrap();

        let busy = channels
            .iter()
            .filter(|channel| channel.id.as_str().starts_with("channel:busy"))
            .map(|channel| (channel.id.as_str(), channel.attention_state))
            .collect::<Vec<_>>();
        assert_eq!(
            busy,
            [
                (
                    "channel:busy-a",
                    Some(ChatWorkAssignmentState::ReadyForReview)
                ),
                ("channel:busy-b", Some(ChatWorkAssignmentState::Working)),
            ]
        );
        assert_eq!(
            channels.len(),
            3,
            "the default channel and the two busy channels"
        );
    }
}
