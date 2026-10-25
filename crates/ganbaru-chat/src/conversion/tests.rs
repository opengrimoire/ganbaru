use super::*;
use crate::models::UtcTimestamp;
use crate::project_defaults::{default_channel_ids, ensure_project_chat_defaults};

const NOW: &str = "2026-10-25T06:00:00.000Z";
const PROJECT_ID: &str = "project-chat-conversion";
const TEAMMATE_ID: &str = "participant:conversion-agent";
const FOLDER_ID: &str = "folder:conversion";
const PROFILE_ID: &str = "access-profile:conversion";

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

/// Seeds a project channel with one AI teammate holding a channel membership and a folder
/// grant, every key a placeholder as the migration leaves it.
async fn seed_placeholders(pool: &SqlitePool) -> String {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-conversion', 'C', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{PROJECT_ID}', 'group-conversion', 'Conversion', 'a0');
         INSERT INTO project_working_folders (id, project_id, display_name, kind)
         VALUES ('{FOLDER_ID}', '{PROJECT_ID}', 'Code', 'external');"
    )))
    .execute(pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ensure_project_chat_defaults(
        &mut connection,
        PROJECT_ID,
        &UtcTimestamp::new(NOW.to_string()).unwrap(),
    )
    .await
    .unwrap();
    drop(connection);
    let conversation = default_channel_ids(PROJECT_ID).unwrap().0;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DELETE FROM chat_conversation_memberships;
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ('{conversation}', 'participant:local-owner', 'pending:2', 'owner', '{NOW}',
                 '{NOW}');
         INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'ai_teammate', 'Agent', '{NOW}', '{NOW}');
         INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'Test', '{NOW}', '{NOW}');
         INSERT INTO chat_access_profiles (id, display_name, created_at, updated_at)
         VALUES ('{PROFILE_ID}', 'Profile', '{NOW}', '{NOW}');
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ('{conversation}', '{TEAMMATE_ID}', 'pending:1', 'member', '{NOW}', '{NOW}');
         INSERT INTO chat_teammate_channel_memberships
            (conversation_id, teammate_id, sync_key, access_profile_id, read_history,
             participate, history_boundary, created_at, updated_at)
         VALUES ('{conversation}', '{TEAMMATE_ID}', 'pending:1', '{PROFILE_ID}', 1, 1,
                 'entire', '{NOW}', '{NOW}');
         INSERT INTO chat_teammate_working_folder_grants
            (conversation_id, teammate_id, working_folder_id, sync_key, project_id, capability,
             is_default, created_at)
         VALUES ('{conversation}', '{TEAMMATE_ID}', '{FOLDER_ID}', 'pending:1', '{PROJECT_ID}',
                 'read', 1, '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    conversation
}

async fn key_of(pool: &SqlitePool, table: &str, filter: &str) -> String {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT sync_key FROM {table} WHERE {filter}"
    )))
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn conversion_rewrites_every_placeholder_to_the_derived_key_once() {
    let pool = migrated_pool().await;
    let conversation = seed_placeholders(&pool).await;
    let error = assert_converted(&pool).await.unwrap_err();
    assert!(error.contains("chat_conversation_memberships"), "{error}");

    assert_eq!(convert(&pool).await.unwrap(), 4);
    assert_converted(&pool).await.unwrap();

    let teammate = format!("conversation_id = '{conversation}' AND teammate_id = '{TEAMMATE_ID}'");
    assert_eq!(
        key_of(
            &pool,
            "chat_conversation_memberships",
            &format!("conversation_id = '{conversation}' AND participant_id = '{TEAMMATE_ID}'"),
        )
        .await,
        membership_key(&conversation, TEAMMATE_ID).unwrap()
    );
    assert_eq!(
        key_of(&pool, "chat_teammate_channel_memberships", &teammate).await,
        channel_membership_key(&conversation, TEAMMATE_ID).unwrap()
    );
    assert_eq!(
        key_of(&pool, "chat_teammate_working_folder_grants", &teammate).await,
        folder_grant_key(&conversation, TEAMMATE_ID, FOLDER_ID).unwrap()
    );

    assert_eq!(convert(&pool).await.unwrap(), 0);
}

#[tokio::test]
async fn converted_keys_cannot_change_again() {
    let pool = migrated_pool().await;
    let conversation = seed_placeholders(&pool).await;
    convert(&pool).await.unwrap();
    let result = sqlx::query(
        "UPDATE chat_teammate_channel_memberships SET sync_key = 'other'
         WHERE conversation_id = ?",
    )
    .bind(&conversation)
    .execute(&pool)
    .await;
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn derived_keys_depend_on_every_key_column_and_its_order() {
    let key = folder_grant_key("conversation", "teammate", "folder").unwrap();
    assert!(key.starts_with(FOLDER_GRANT_KEY_PREFIX));
    assert_ne!(
        key,
        folder_grant_key("conversation", "folder", "teammate").unwrap()
    );
    assert_ne!(
        membership_key("a", "b").unwrap(),
        channel_membership_key("a", "b").unwrap()
    );
}
