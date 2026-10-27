use super::helpers::{load_project_chat_graph, memory_pool_migrated_through};
use crate::run_migrations;
use sqlx::{AssertSqlSafe, SqlitePool};
use std::collections::BTreeMap;

const CHAT_TIMELINE_REPLICATION: i64 = 20261027171458;

/// Timeline rows around the graph's root message in the ordinal shape: root items with gaps
/// between their ordinals, two replies, read cursors that point into and before the timeline, AI
/// history granted from a root ordinal, and channel references with a range and without one.
const ORDINAL_TIMELINE: &str = "
INSERT INTO chat_conversation_items (id, conversation_id, item_kind, ordinal, created_at)
VALUES ('item-root-4', 'conversation-general', 'work_update', 4, '2026-10-23T00:00:01.000Z'),
       ('item-root-7', 'conversation-general', 'work_update', 7, '2026-10-23T00:00:02.000Z');
INSERT INTO chat_conversation_items
    (id, conversation_id, reply_thread_id, item_kind, ordinal, created_at)
VALUES ('item-reply-b', 'conversation-general', 'reply-thread', 'work_update', 1,
        '2026-10-23T00:00:03.000Z'),
       ('item-reply-a', 'conversation-general', 'reply-thread', 'work_update', 3,
        '2026-10-23T00:00:04.000Z');
INSERT INTO chat_conversation_read_cursors
    (conversation_id, participant_id, last_read_root_ordinal, updated_at)
VALUES ('conversation-general', 'participant:local-owner', 5, '2026-10-23T00:00:05.000Z'),
       ('conversation-general', 'participant-teammate', 0, '2026-10-23T00:00:05.000Z');
INSERT INTO chat_reply_thread_read_cursors
    (reply_thread_id, participant_id, last_read_reply_ordinal, updated_at)
VALUES ('reply-thread', 'participant:local-owner', 2, '2026-10-23T00:00:05.000Z');
UPDATE chat_teammate_channel_memberships
SET history_boundary = 'from_grant', history_from_ordinal = 4
WHERE conversation_id = 'conversation-general' AND teammate_id = 'participant-teammate';
INSERT INTO chat_message_references
    (id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
     start_offset, end_offset, ordinal, created_at)
VALUES ('reference-channel', 'message-revision-root', 'channel', 'general', 'general', 17, 24, 4,
        '2026-10-23T00:00:00.000Z'),
       ('reference-channel-empty', 'message-revision-root', 'channel', 'general', 'general', 25,
        32, 5, '2026-10-23T00:00:00.000Z');
INSERT INTO chat_channel_reference_targets
    (reference_id, channel_id, source_conversation_id, destination_conversation_id,
     source_lower_ordinal, source_high_ordinal, source_revision_cutoff_id,
     destination_audience_revision)
VALUES ('reference-channel', 'channel-general', 'conversation-general', 'conversation-general',
        2, 5, 'message-revision-root', 1),
       ('reference-channel-empty', 'channel-general', 'conversation-general',
        'conversation-general', 0, 0, NULL, 1);
";

/// Tables whose rows the migration carries over one for one.
const CARRIED_TABLES: [&str; 11] = [
    "chat_conversation_items",
    "chat_reply_threads",
    "chat_communication_messages",
    "chat_communication_message_revisions",
    "chat_message_references",
    "chat_channel_reference_targets",
    "chat_conversation_read_cursors",
    "chat_reply_thread_read_cursors",
    "chat_teammate_channel_memberships",
    "chat_assignment_authorized_channel_sources",
    "chat_scratch_generation_sources",
];

/// A pool one migration before timeline replication, holding the graph and the ordinal timeline.
async fn ordinal_timeline_pool() -> SqlitePool {
    let pool = memory_pool_migrated_through(CHAT_TIMELINE_REPLICATION - 1).await;
    load_project_chat_graph(&pool).await;
    sqlx::raw_sql(ORDINAL_TIMELINE)
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn row_counts(pool: &SqlitePool) -> BTreeMap<&'static str, i64> {
    let mut counts = BTreeMap::new();
    for table in CARRIED_TABLES {
        let count: i64 = sqlx::query_scalar(AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
            .fetch_one(pool)
            .await
            .unwrap();
        counts.insert(table, count);
    }
    counts
}

/// Item ids per scope in the order `order_by` gives, with root items under ''.
async fn items_by_scope(pool: &SqlitePool, order_by: &str) -> BTreeMap<String, Vec<String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT coalesce(reply_thread_id, ''), id FROM chat_conversation_items
         ORDER BY conversation_id, coalesce(reply_thread_id, ''), {order_by}"
    )))
    .fetch_all(pool)
    .await
    .unwrap();
    let mut scopes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (scope, id) in rows {
        scopes.entry(scope).or_default().push(id);
    }
    scopes
}

/// Root item ids of the general channel that satisfy `filter`, in timeline order.
async fn root_items_where(pool: &SqlitePool, filter: &str, order_by: &str) -> Vec<String> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT id FROM chat_conversation_items item
         WHERE conversation_id = 'conversation-general' AND reply_thread_id IS NULL AND ({filter})
         ORDER BY {order_by}"
    )))
    .fetch_all(pool)
    .await
    .unwrap()
}

#[test]
fn timeline_migration_keeps_rows_order_and_every_position_on_the_same_items() {
    super::block_on(async {
        let pool = ordinal_timeline_pool().await;
        let counts_before = row_counts(&pool).await;
        let order_before = items_by_scope(&pool, "ordinal").await;
        let history_before = root_items_where(&pool, "ordinal >= 4", "ordinal").await;
        let range_before = root_items_where(&pool, "ordinal BETWEEN 2 AND 5", "ordinal").await;

        run_migrations(&pool).await.unwrap();

        assert_eq!(row_counts(&pool).await, counts_before);
        let order_after = items_by_scope(&pool, "order_key, id").await;
        assert_eq!(order_after, order_before);
        assert_eq!(
            order_after[""],
            ["item-root", "item-root-4", "item-root-7"],
            "root items keep ordinal order across ordinal gaps"
        );
        assert_eq!(
            order_after["reply-thread"],
            ["item-reply-b", "item-reply-a"]
        );

        let cursors: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT participant_id, last_read_order_key, last_read_item_id
             FROM chat_conversation_read_cursors ORDER BY participant_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            cursors,
            [
                ("participant-teammate".into(), String::new(), String::new()),
                (
                    "participant:local-owner".into(),
                    "c001".into(),
                    "item-root-4".into()
                ),
            ],
            "a cursor holds the last item at or before its ordinal, and '' before every item"
        );
        let reply_cursor: (String, String) = sqlx::query_as(
            "SELECT last_read_order_key, last_read_item_id FROM chat_reply_thread_read_cursors",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reply_cursor, ("c000".into(), "item-reply-b".into()));

        let history_from: String = sqlx::query_scalar(
            "SELECT history_from_order_key FROM chat_teammate_channel_memberships
             WHERE teammate_id = 'participant-teammate'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let history_after = root_items_where(
            &pool,
            &format!("order_key >= '{history_from}'"),
            "order_key, id",
        )
        .await;
        assert_eq!(history_after, history_before);

        let range: (String, String, String, String, String) = sqlx::query_as(
            "SELECT source_lower_order_key, source_lower_item_id, source_high_order_key,
                    source_high_item_id, source_revision_cutoff_id
             FROM chat_channel_reference_targets WHERE reference_id = 'reference-channel'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let range_after = root_items_where(
            &pool,
            &format!(
                "(order_key, id) >= ('{}', '{}') AND (order_key, id) <= ('{}', '{}')",
                range.0, range.1, range.2, range.3
            ),
            "order_key, id",
        )
        .await;
        assert_eq!(range_after, range_before);
        assert_eq!(range.4, "message-revision-root");
        let empty_range: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_channel_reference_targets
             WHERE reference_id = 'reference-channel-empty'
               AND source_lower_order_key IS NULL AND source_lower_item_id IS NULL
               AND source_high_order_key IS NULL AND source_high_item_id IS NULL
               AND source_revision_cutoff_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            empty_range, 1,
            "a reference without a range keeps no bounds"
        );

        let source: (String, String, String, String) = sqlx::query_as(
            "SELECT lower_order_key, lower_item_id, high_order_key, high_item_id
             FROM chat_assignment_authorized_channel_sources",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            source,
            (
                "c000".into(),
                String::new(),
                "c000".into(),
                "item-root".into()
            )
        );

        for table in [
            "chat_conversation_read_cursors",
            "chat_reply_thread_read_cursors",
        ] {
            let (rows, pending, distinct): (i64, i64, i64) =
                sqlx::query_as(AssertSqlSafe(format!(
                    "SELECT count(*), count(*) FILTER (WHERE sync_key LIKE 'pending:%'),
                            count(DISTINCT sync_key)
                     FROM {table}"
                )))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert!(rows > 0, "{table} has cursors");
            assert_eq!((pending, distinct), (rows, rows), "{table} placeholders");
        }

        let revision: i64 = sqlx::query_scalar(
            "SELECT revision FROM chat_communication_message_revisions_numbered
             WHERE id = 'message-revision-root'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(revision, 1);
        let violations: i64 = sqlx::query_scalar("SELECT count(*) FROM pragma_foreign_key_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(violations, 0);
    });
}

#[test]
fn timeline_migration_rolls_back_when_a_foreign_key_is_violated() {
    super::block_on(async {
        let pool = ordinal_timeline_pool().await;
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO chat_conversation_read_cursors
                 (conversation_id, participant_id, last_read_root_ordinal, updated_at)
             VALUES ('conversation-general', 'participant-missing', 1,
                     '2026-10-23T00:00:05.000Z');
             PRAGMA foreign_keys = ON;",
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        drop(connection);
        let counts_before = row_counts(&pool).await;
        let order_before = items_by_scope(&pool, "ordinal").await;

        let error = run_migrations(&pool).await.unwrap_err();
        assert!(error.contains("CHECK constraint failed"), "{error}");
        // A no-transaction migration owns its BEGIN, so a failure leaves that transaction open on
        // the pool's only connection; the test rolls it back and restores the changed pragmas.
        sqlx::raw_sql("ROLLBACK; PRAGMA legacy_alter_table = OFF; PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(row_counts(&pool).await, counts_before);
        assert_eq!(items_by_scope(&pool, "ordinal").await, order_before);
        let applied: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version = ?")
                .bind(CHAT_TIMELINE_REPLICATION)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(applied, 0);
        let leftovers: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema WHERE name LIKE '%\\_next' ESCAPE '\\'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(leftovers, 0);
    });
}
