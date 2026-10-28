use sqlx::{Row, SqlitePool};

pub(super) async fn migrated_memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    crate::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

pub(super) async fn insert_event(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO calendar_events (id, title, start_time, end_time)
         VALUES ('event-1', 'Focus block', '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z')",
    )
    .execute(pool)
    .await
    .unwrap();
}

pub(super) async fn insert_open_run(
    pool: &SqlitePool,
    id: &str,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO pomodoro_runs
            (id, event_id, original_event_id, event_date, planned_start, planned_end,
             started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat, start_trigger)
         VALUES (?, 'event-1', 'event-1', '2026-05-23',
                 '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z',
                 '2026-05-23T09:00:00Z', 'count', 'preset', 'adaptive',
                 '2026-05-23T09:00:00Z', 'manual')",
    )
    .bind(id)
    .execute(pool)
    .await
}

pub(super) async fn query_plan(pool: &SqlitePool, sql: &str) -> String {
    sqlx::query(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {sql}")))
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn assert_plan_uses(plan: &str, expected: &str) {
    assert!(
        plan.contains(expected),
        "expected query plan to use {expected}, got:\n{plan}",
    );
}

/// Embedded migrations up to and including one version, for upgrade tests.
#[derive(Debug)]
struct MigrationsThrough(i64);

impl sqlx::migrate::MigrationSource<'static> for MigrationsThrough {
    fn resolve(
        self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Vec<sqlx::migrate::Migration>, sqlx::error::BoxDynError>,
                > + Send,
        >,
    > {
        let migrations = crate::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= self.0)
            .cloned()
            .collect();
        Box::pin(async move { Ok(migrations) })
    }
}

/// A memory pool migrated through `version`; `run_migrations` later applies the rest.
pub(super) async fn memory_pool_migrated_through(version: i64) -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    migrate_through(&pool, version).await.unwrap();
    pool
}

/// Applies the pending migrations of `pool` up to and including `version`, so an upgrade test
/// observes one migration without the later ones.
pub(super) async fn migrate_through(
    pool: &SqlitePool,
    version: i64,
) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate::Migrator::new(MigrationsThrough(version))
        .await?
        .run(pool)
        .await
}

/// One project with tasks, an archived external folder, soft-deleted statuses and priorities,
/// and a Chat graph of teammates, assignments, authorizations, runs, and scratch that references
/// the project and both working folders.
const PROJECT_CHAT_GRAPH: &str = include_str!("project_chat_graph.sql");

/// The graph's teammate policy revision as the current schema stores it.
const POLICY_REVISION: &str = "INSERT INTO chat_teammate_policy_revisions
    (id, teammate_id, provider_instance_id, model_selection_data, created_at)
VALUES ('policy-1', 'participant-teammate', 'codex-personal', '{}', '2026-10-23T00:00:00.000Z');";

/// The same policy revision for schemas before Chat identity replication, which store its
/// number.
const NUMBERED_POLICY_REVISION: &str = "INSERT INTO chat_teammate_policy_revisions
    (id, teammate_id, revision, provider_instance_id, model_selection_data, created_at)
VALUES ('policy-1', 'participant-teammate', 1, 'codex-personal', '{}', '2026-10-23T00:00:00.000Z');";

/// Creates the default channel of `project-routine-learning` with its owner membership, as
/// Chat does on first use, since fresh vaults seed no conversations. Returns the conversation
/// and channel ids.
pub(super) async fn insert_default_channel(pool: &SqlitePool) -> (String, String) {
    sqlx::raw_sql(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ('conversation-default-learning', 'project-routine-learning', 'channel',
                 '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z',
                 '2026-10-23T00:00:00.000Z');
         INSERT INTO chat_channels
            (id, project_id, conversation_id, name, is_default, created_at, updated_at)
         VALUES ('channel-default-learning', 'project-routine-learning',
                 'conversation-default-learning', 'general', 1, '2026-10-23T00:00:00.000Z',
                 '2026-10-23T00:00:00.000Z');
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ('conversation-default-learning', 'participant:local-owner',
                 'membership-default-learning-owner', 'owner', '2026-10-23T00:00:00.000Z',
                 '2026-10-23T00:00:00.000Z');",
    )
    .execute(pool)
    .await
    .unwrap();
    (
        "conversation-default-learning".to_owned(),
        "channel-default-learning".to_owned(),
    )
}

/// Whether `table` in the schema of `pool` has `column`.
async fn has_column(pool: &SqlitePool, table: &str, column: &str) -> bool {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pragma_table_info(?) WHERE name = ?)")
        .bind(table)
        .bind(column)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Fragments of the graph's membership and grant inserts that name their `sync_key`, removed
/// for schemas before Chat conversations replication, which have no such column.
const SYNC_KEY_FRAGMENTS: [&str; 8] = [
    ", sync_key, membership_role",
    ", sync_key, access_profile_id",
    ", sync_key, capability",
    "'membership-owner', ",
    "'membership-teammate', ",
    "'channel-membership-teammate',\n        ",
    "'grant-managed', ",
    "'grant-archived', ",
];

/// The graph's timeline inserts in the current shape and in the shape schemas before Chat
/// timeline replication store, with integer ordinals and revision numbers.
const ORDINAL_TIMELINE_FRAGMENTS: [(&str, &str); 6] = [
    (
        "(id, conversation_id, item_kind, order_key, created_at)\nVALUES ('item-root', 'conversation-general', 'message', 'c000',",
        "(id, conversation_id, item_kind, ordinal, created_at)\nVALUES ('item-root', 'conversation-general', 'message', 1,",
    ),
    (
        "(id, message_item_id, normalized_markdown, created_at)\nVALUES ('message-revision-root', 'item-root', ",
        "(id, message_item_id, revision, normalized_markdown, created_at)\nVALUES ('message-revision-root', 'item-root', 1, ",
    ),
    (
        "lower_order_key, lower_item_id, high_order_key, high_item_id, source_revision_cutoff_id,\n     destination_audience_revision,",
        "lower_ordinal, high_ordinal, source_revision_cutoff_id, destination_audience_revision,",
    ),
    (
        "'general', 'c000', '', 'c000', 'item-root', 'message-revision-root',",
        "'general', 1, 1, 'message-revision-root',",
    ),
    (
        "lower_order_key, lower_item_id, high_order_key, high_item_id, audience_revision,",
        "lower_ordinal, high_ordinal, audience_revision,",
    ),
    (
        "'conversation-general', 'c000', '', 'c000', 'item-root', 1,",
        "'conversation-general', 1, 1, 1,",
    ),
];

/// Loads the project and Chat graph in the shape the schema of `pool` stores, so upgrade tests
/// that stop before Chat identity, conversations, or timeline replication share the graph with
/// current schema tests.
pub(super) async fn load_project_chat_graph(pool: &SqlitePool) {
    assert!(
        PROJECT_CHAT_GRAPH.contains(POLICY_REVISION),
        "the graph inserts its policy revision in the current shape"
    );
    let numbered = has_column(pool, "chat_teammate_policy_revisions", "revision").await;
    let keyed = has_column(pool, "chat_conversation_memberships", "sync_key").await;
    let ordered = has_column(pool, "chat_conversation_items", "order_key").await;
    let mut graph = if numbered {
        PROJECT_CHAT_GRAPH.replace(POLICY_REVISION, NUMBERED_POLICY_REVISION)
    } else {
        PROJECT_CHAT_GRAPH.to_string()
    };
    if !keyed {
        for fragment in SYNC_KEY_FRAGMENTS {
            let keyless = match fragment.strip_prefix(", sync_key") {
                Some(rest) => rest.to_string(),
                None => String::new(),
            };
            let replaced = graph.replace(fragment, &keyless);
            assert_ne!(replaced, graph, "the graph names {fragment:?}");
            graph = replaced;
        }
    }
    if !ordered {
        for (current, ordinal) in ORDINAL_TIMELINE_FRAGMENTS {
            let replaced = graph.replace(current, ordinal);
            assert_ne!(replaced, graph, "the graph names {current:?}");
            graph = replaced;
        }
    }
    sqlx::raw_sql(sqlx::AssertSqlSafe(graph))
        .execute(pool)
        .await
        .unwrap();
}
