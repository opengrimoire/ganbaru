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
