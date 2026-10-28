use super::*;
use ganbaru_sync::ChangeNotice;
use sqlx::SqlitePool;
use std::collections::BTreeSet;

const NOW: &str = "2026-10-24T09:00:00.000Z";
const TEAMMATE_ID: &str = "participant:chat-sync-hook";

/// Collects reported notices the way the engine's change sink does.
#[derive(Default)]
struct CollectingSink(Vec<ChangeNotice>);

impl ChangeSink for CollectingSink {
    fn report(&mut self, topic: &'static str, key: String) {
        self.0.push(ChangeNotice { topic, key });
    }
}

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

/// Chat writes the keys the manifest derives, so a row a device writes and the same row another
/// device writes concurrently share one key.
#[test]
fn chat_derives_sync_keys_with_the_manifest_prefixes() {
    use ganbaru_chat::conversion;
    assert_eq!(
        conversion::MEMBERSHIP_KEY_PREFIX,
        chat::MEMBERSHIP_KEY_PREFIX
    );
    assert_eq!(
        conversion::CHANNEL_MEMBERSHIP_KEY_PREFIX,
        chat::CHANNEL_MEMBERSHIP_KEY_PREFIX
    );
    assert_eq!(
        conversion::FOLDER_GRANT_KEY_PREFIX,
        chat::FOLDER_GRANT_KEY_PREFIX
    );
    assert_eq!(
        ganbaru_chat::ids::REPLY_THREAD_ID_PREFIX,
        chat::REPLY_THREAD_KEY_PREFIX
    );
    assert_eq!(
        conversion::CONVERSATION_CURSOR_KEY_PREFIX,
        chat::CONVERSATION_CURSOR_KEY_PREFIX
    );
    assert_eq!(
        conversion::REPLY_CURSOR_KEY_PREFIX,
        chat::REPLY_CURSOR_KEY_PREFIX
    );
}

#[test]
fn every_replicated_table_with_derived_chat_state_has_a_hook() {
    let derived = [
        sync_effects::TEAMMATES_TABLE,
        sync_effects::POLICY_REVISIONS_TABLE,
        sync_effects::CONVERSATIONS_TABLE,
        sync_effects::ACCESS_PROFILES_TABLE,
        sync_effects::ACCESS_PROFILE_REVISIONS_TABLE,
        sync_effects::MEMBERSHIPS_TABLE,
        sync_effects::CHANNEL_MEMBERSHIPS_TABLE,
        sync_effects::FOLDER_GRANTS_TABLE,
        sync_effects::MESSAGES_TABLE,
        sync_effects::MESSAGE_REVISIONS_TABLE,
    ];
    let mut expected: Vec<&str> = chat::DOMAIN
        .tables
        .iter()
        .map(|table| table.name)
        .filter(|name| derived.contains(name))
        .collect();
    let mut hooked: Vec<&str> = HOOKED_TABLES.iter().map(|table| table.name).collect();
    expected.sort_unstable();
    hooked.sort_unstable();
    assert_eq!(hooked, expected);
    assert!(
        HOOKED_TABLES
            .iter()
            .all(|table| chat::DOMAIN.tables.iter().any(|spec| spec.id == table.id))
    );
}

#[test]
fn only_revoked_teammates_and_removed_conversations_are_reported() {
    let mut sink = CollectingSink::default();
    report_effects(
        ChatSyncEffects {
            access_state_created: BTreeSet::from(["participant:created".to_string()]),
            access_revised: BTreeSet::from(["participant:revised".to_string()]),
            audience_revised: BTreeSet::from(["conversation:revised".to_string()]),
            revoked_teammates: BTreeSet::from([
                "participant:b".to_string(),
                "participant:a".to_string(),
            ]),
            search_rebuilt: BTreeSet::from(["item:rebuilt".to_string()]),
            mismatched_pointers: BTreeSet::from(["participant:mismatched".to_string()]),
            removed_conversations: BTreeSet::from(["conversation:removed".to_string()]),
        },
        &mut sink,
    );
    let reported: Vec<(&str, &str)> = sink
        .0
        .iter()
        .map(|notice| (notice.topic, notice.key.as_str()))
        .collect();
    assert_eq!(
        reported,
        vec![
            (CHAT_ACCESS_REVOKED_TOPIC, "participant:a"),
            (CHAT_ACCESS_REVOKED_TOPIC, "participant:b"),
            (CHAT_CONVERSATION_REMOVED_TOPIC, "conversation:removed"),
        ]
    );
}

#[cfg(desktop)]
#[test]
fn revocation_notices_wake_the_drain_and_other_notices_do_not() {
    let revoked = ChangeNotice {
        topic: CHAT_ACCESS_REVOKED_TOPIC,
        key: TEAMMATE_ID.to_string(),
    };
    let unrelated = ChangeNotice {
        topic: "music-root",
        key: TEAMMATE_ID.to_string(),
    };
    let removed = ChangeNotice {
        topic: CHAT_CONVERSATION_REMOVED_TOPIC,
        key: "conversation:removed".to_string(),
    };
    assert!(waker::access_revoked(&[unrelated.clone(), revoked]));
    assert!(waker::access_revoked(&[unrelated.clone(), removed]));
    assert!(!waker::access_revoked(&[unrelated]));
    assert!(!waker::access_revoked(&[]));
}

#[tokio::test]
async fn materialized_teammates_get_access_state_without_waking_the_drain() {
    let pool = migrated_pool().await;
    sqlx::query("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'ai_teammate', 'Hook agent', '{NOW}', '{NOW}');
         INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'Test', '{NOW}', '{NOW}');"
    )))
    .execute(&pool)
    .await
    .unwrap();
    let access_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chat_teammate_access_state WHERE teammate_id = ?")
            .bind(TEAMMATE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(access_rows, 0);

    let mut connection = pool.acquire().await.unwrap();
    let mut sink = CollectingSink::default();
    ChatApplyHook
        .after_materialize(
            &mut connection,
            &chat::TEAMMATES,
            &[TEAMMATE_ID.to_string()],
            &mut sink,
        )
        .await
        .unwrap();
    drop(connection);

    let access_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chat_teammate_access_state WHERE teammate_id = ?")
            .bind(TEAMMATE_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(access_rows, 1);
    assert!(sink.0.is_empty());
}

/// A hostile writer could send a pointer whose copy disagrees with its revision; failing the hook
/// would turn that row into a sync error that stops every later apply on this device.
#[tokio::test]
async fn a_mismatched_pointer_applies_without_effective_access_or_blocking_later_rows() {
    let pool = migrated_pool().await;
    sqlx::query("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'ai_teammate', 'Hook agent', '{NOW}', '{NOW}'),
                ('participant:later', 'ai_teammate', 'Later agent', '{NOW}', '{NOW}');
         INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'Test', '{NOW}', '{NOW}'),
                ('participant:later', 'Test', '{NOW}', '{NOW}');
         INSERT INTO chat_teammate_policy_revisions
            (id, teammate_id, provider_instance_id, model_selection_data, created_at)
         VALUES ('policy:hook', '{TEAMMATE_ID}', 'codex-personal', '{{}}', '{NOW}');
         UPDATE chat_teammates
         SET current_policy_revision_id = 'policy:hook', current_policy_safety_mode = 'full_access'
         WHERE participant_id = '{TEAMMATE_ID}';"
    )))
    .execute(&pool)
    .await
    .unwrap();

    let mut connection = pool.acquire().await.unwrap();
    let mut sink = CollectingSink::default();
    ChatApplyHook
        .after_materialize(
            &mut connection,
            &chat::TEAMMATES,
            &[TEAMMATE_ID.to_string()],
            &mut sink,
        )
        .await
        .unwrap();
    ChatApplyHook
        .after_materialize(
            &mut connection,
            &chat::TEAMMATES,
            &["participant:later".to_string()],
            &mut sink,
        )
        .await
        .unwrap();
    drop(connection);

    let effective: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_teammate_effective_policies WHERE teammate_id = ?",
    )
    .bind(TEAMMATE_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(effective, 0);
    let access_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_teammate_access_state")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(access_rows, 2);
}

const ATTACHMENT_DIGEST: &str = "5f2b0c4e8d6a1b3f7e9c2d4a6b8f0e1c3d5a7b9f1e2c4d6a8b0f2e4c6d8a0b1c";
const EMPTY_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Seeds a project channel message whose revision names one image and one empty text snippet
/// by value, as a materialized revision from another device does.
async fn seed_applied_attachment_revision(pool: &SqlitePool) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         INSERT INTO project_groups (id, name, order_key) VALUES ('group:sync', 'Sync', 'a1');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project:sync', 'group:sync', 'Sync', 'a1');
         INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path)
         VALUES ('folder:sync', 'project:sync', 'Sync', 'managed', 'projects/sync');
         INSERT INTO chat_conversations
             (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ('conversation:sync', 'project:sync', 'channel', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:sync', 'conversation:sync', 'message', 'a1', '{NOW}');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES ('item:sync', NULL, 'Teammate', '{NOW}');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:sync', 'item:sync', 'See attached', '{NOW}');
         UPDATE chat_communication_messages SET current_revision_id = 'revision:sync'
         WHERE item_id = 'item:sync';
         INSERT INTO chat_communication_attachment_references
             (message_revision_id, ordinal, attachment_id, working_folder_id, kind,
              original_display_name, mime_type, byte_size, sha256)
         VALUES
             ('revision:sync', 0, 'attachment:image', 'folder:sync', 'image', 'diagram.png',
              'image/png', 4096, '{ATTACHMENT_DIGEST}'),
             ('revision:sync', 1, 'attachment:empty', 'folder:sync', 'text_snippet',
              'empty.txt', 'text/plain', 0, '{EMPTY_DIGEST}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

async fn attachment_paths(pool: &SqlitePool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT attachment.id, attachment.managed_relative_path, resource.resource_uri
         FROM chat_attachments attachment
         JOIN chat_resources resource ON resource.attachment_id = attachment.id
         ORDER BY attachment.id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// A materialized revision gives this device attachment rows under the digest file names the
/// asset transfer writes, and the sweep keeps the file while the reference or the device's
/// attachment row names its digest.
#[tokio::test]
async fn applied_attachment_references_create_local_rows_and_keep_their_files() {
    let pool = migrated_pool().await;
    seed_applied_attachment_revision(&pool).await;
    let mut connection = pool.acquire().await.unwrap();
    let mut sink = CollectingSink::default();
    for _ in 0..2 {
        ChatApplyHook
            .after_materialize(
                &mut connection,
                &chat::REVISIONS,
                &["revision:sync".to_string()],
                &mut sink,
            )
            .await
            .unwrap();
    }
    drop(connection);
    assert!(sink.0.is_empty());
    assert_eq!(
        attachment_paths(&pool).await,
        vec![
            (
                "attachment:empty".to_string(),
                format!("assets/chat/attachments/{EMPTY_DIGEST}.txt"),
                "ganbaru://chat/resource/attachment:empty".to_string(),
            ),
            (
                "attachment:image".to_string(),
                format!("assets/chat/attachments/{ATTACHMENT_DIGEST}.png"),
                "ganbaru://chat/resource/attachment:image".to_string(),
            ),
        ]
    );

    sqlx::query("UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1")
        .execute(&pool)
        .await
        .unwrap();
    let engine = ganbaru_sync::Engine::vault();
    let class = chat::ATTACHMENTS_CLASS.name;
    let kept_file = (class, ATTACHMENT_DIGEST.to_string());
    let mut connection = pool.acquire().await.unwrap();
    let referenced = engine.referenced_assets(&mut connection).await.unwrap();
    let referenced: Vec<(&str, &str, u32)> = referenced
        .iter()
        .map(|asset| (asset.class().name, asset.sha256(), asset.byte_size()))
        .collect();
    assert_eq!(referenced, vec![(class, ATTACHMENT_DIGEST, 4096)]);
    let kept = engine.kept_asset_digests(&mut connection).await.unwrap();
    assert!(kept.contains(&kept_file));

    sqlx::query("DELETE FROM chat_communication_attachment_references")
        .execute(&mut *connection)
        .await
        .unwrap();
    assert!(
        engine
            .referenced_assets(&mut connection)
            .await
            .unwrap()
            .is_empty()
    );
    let kept = engine.kept_asset_digests(&mut connection).await.unwrap();
    assert!(
        kept.contains(&kept_file),
        "the device's attachment row keeps the file"
    );

    sqlx::query("DELETE FROM chat_attachments")
        .execute(&mut *connection)
        .await
        .unwrap();
    let kept = engine.kept_asset_digests(&mut connection).await.unwrap();
    assert!(!kept.contains(&kept_file));
}

#[tokio::test]
async fn a_scheduled_attachment_keeps_its_file_after_its_attachment_row_is_gone() {
    let pool = migrated_pool().await;
    let engine = ganbaru_sync::Engine::vault();
    let kept_file = (chat::ATTACHMENTS_CLASS.name, ATTACHMENT_DIGEST.to_string());
    let mut connection = pool.acquire().await.unwrap();
    // A working folder delete cascades the attachment row away while the scheduled message
    // still names its digest; only the reference is seeded, without its scheduled message.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "PRAGMA foreign_keys = OFF;
         INSERT INTO chat_scheduled_message_attachment_references
             (scheduled_message_id, ordinal, attachment_id, working_folder_id, kind,
              original_display_name, mime_type, byte_size, sha256)
         VALUES ('scheduled:sync', 0, 'attachment:gone', 'folder:gone', 'image', 'photo.png',
                 'image/png', 4096, '{ATTACHMENT_DIGEST}');
         PRAGMA foreign_keys = ON;"
    )))
    .execute(&mut *connection)
    .await
    .unwrap();
    let kept = engine.kept_asset_digests(&mut connection).await.unwrap();
    assert!(
        kept.contains(&kept_file),
        "a scheduled reference keeps its file without an attachment row"
    );

    sqlx::query("DELETE FROM chat_scheduled_message_attachment_references")
        .execute(&mut *connection)
        .await
        .unwrap();
    let kept = engine.kept_asset_digests(&mut connection).await.unwrap();
    assert!(!kept.contains(&kept_file));
}
