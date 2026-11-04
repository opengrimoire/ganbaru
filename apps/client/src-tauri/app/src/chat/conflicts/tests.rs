use super::*;
use ganbaru_sync_contracts::TableId;
use std::collections::HashMap;

const NOW: &str = "2026-10-28T09:00:00.000Z";

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

/// A channel `channel:c` holding message `item:m` with revisions `revision:a` and `revision:b`,
/// the first current.
async fn seed_channel_message(pool: &SqlitePool) {
    sqlx::raw_sql(AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group:c', 'Group', 'a1');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project:c', 'group:c', 'Project', 'a1');
         INSERT INTO chat_conversations
             (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ('conversation:c', 'project:c', 'channel', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_channels
             (id, project_id, conversation_id, name, topic, created_at, updated_at)
         VALUES ('channel:c', 'project:c', 'conversation:c', 'design', 'Old topic', '{NOW}',
                 '{NOW}');
         INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:m', 'conversation:c', 'message', 'a1', '{NOW}');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES ('item:m', NULL, 'Owner', '{NOW}');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:a', 'item:m', 'First words', '{NOW}'),
                ('revision:b', 'item:m', 'Second words', '{NOW}');
         UPDATE chat_communication_messages SET current_revision_id = 'revision:a'
         WHERE item_id = 'item:m';"
    )))
    .execute(pool)
    .await
    .unwrap();
}

fn text(value: &str) -> Field {
    Field::Text(value.to_string())
}

async fn write(pool: &SqlitePool, target: ChatConflictTarget, id: &str, fields: Vec<Field>) {
    let mut tx = pool.begin().await.unwrap();
    write_group_value(&mut tx, target, id, &Value::new(fields).unwrap())
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

/// Every Chat group shown as a conflict has a target, so no conflict is left without a way to
/// resolve it, and every target names a surfaced group.
#[test]
fn every_surfaced_chat_group_has_a_conflict_target() {
    let surfaced: Vec<(TableId, GroupId)> = chat_manifest::DOMAIN
        .tables
        .iter()
        .flat_map(|table| {
            table
                .groups
                .iter()
                .filter(|group| group.surfaced)
                .map(|group| (table.id, group.id))
        })
        .collect();
    let targets: Vec<(TableId, GroupId)> = ChatConflictTarget::ALL
        .iter()
        .map(|target| (target.table().id, target.group()))
        .collect();
    assert_eq!(surfaced, targets);
}

#[tokio::test]
async fn a_vault_that_does_not_sync_has_no_chat_conflicts() {
    let pool = migrated_pool().await;
    let devices = DeviceNames::new("device-a", HashMap::new());
    let conflicts = conflicts_from_pool(&pool, "vault", &devices).await.unwrap();
    assert!(conflicts.is_empty());
}

#[tokio::test]
async fn a_vault_that_does_not_sync_refuses_a_resolution_as_resolved() {
    let pool = migrated_pool().await;
    let request: ChatConflictResolution = serde_json::from_value(serde_json::json!({
        "target": "channel",
        "id": "channel:c",
        "choice": { "kind": "displayed" },
    }))
    .unwrap();
    let error = resolve_from_pool(&pool, "vault", request, 1)
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::NotFound);
}

#[tokio::test]
async fn a_chosen_channel_version_bumps_the_revision_and_updated_at() {
    let pool = migrated_pool().await;
    seed_channel_message(&pool).await;
    write(
        &pool,
        ChatConflictTarget::Channel,
        "channel:c",
        vec![text("research"), text("New topic")],
    )
    .await;
    let (name, topic, revision, updated_at): (String, String, i64, String) =
        sqlx::query_as("SELECT name, topic, revision, updated_at FROM chat_channels WHERE id = ?")
            .bind("channel:c")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (name.as_str(), topic.as_str(), revision),
        ("research", "New topic", 2)
    );
    assert_ne!(updated_at, NOW);
}

#[tokio::test]
async fn a_chosen_message_version_shows_its_revision() {
    let pool = migrated_pool().await;
    seed_channel_message(&pool).await;
    write(
        &pool,
        ChatConflictTarget::Message,
        "item:m",
        vec![text("revision:b"), text(NOW)],
    )
    .await;
    let mut tx = pool.begin().await.unwrap();
    let current: Option<String> = sqlx::query_scalar(
        "SELECT current_revision_id FROM chat_communication_messages WHERE item_id = 'item:m'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(
        revision_markdown(&mut tx, current).await.unwrap(),
        "Second words"
    );
    assert_eq!(
        message_channel_name(&mut tx, "item:m")
            .await
            .unwrap()
            .as_deref(),
        Some("design")
    );
}

#[tokio::test]
async fn a_version_of_another_shape_is_refused_without_writing() {
    let pool = migrated_pool().await;
    seed_channel_message(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    let error = write_group_value(
        &mut tx,
        ChatConflictTarget::Channel,
        "channel:c",
        &Value::new(vec![text("research")]).unwrap(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Internal);
}

#[tokio::test]
async fn a_missing_row_reads_as_resolved() {
    let pool = migrated_pool().await;
    let mut tx = pool.begin().await.unwrap();
    let error = write_group_value(
        &mut tx,
        ChatConflictTarget::Channel,
        "channel:gone",
        &Value::new(vec![text("research"), text("")]).unwrap(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::NotFound);
}
