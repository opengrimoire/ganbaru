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
        sync_effects::ASSIGNMENTS_TABLE,
        sync_effects::ASSIGNMENT_INPUTS_TABLE,
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

/// The folder grant settle returns the strictest override by the Chat order, so that order must
/// be the one sync ranks grant versions by, or a settled value could rank below a version.
#[test]
fn the_settle_approval_order_is_the_sync_grant_ranking() {
    assert_eq!(
        ganbaru_chat::sync_effects::APPROVAL_ORDER,
        ganbaru_sync::domains::chat::APPROVAL_POLICIES
    );
}

/// The engine refuses a hook that settles a group it cannot write back, so building the vault
/// engine with the Chat hooks proves the folder grant and channel access settles are admissible.
#[test]
fn the_vault_engine_accepts_the_chat_folder_grant_settle() {
    let hooks = with_hooks(Hooks::new());
    assert!(
        ganbaru_sync::Engine::with(
            &ganbaru_sync::manifest::vault::VAULT_MANIFEST,
            ganbaru_sync::domains::vault_adapters(),
            hooks,
        )
        .is_ok()
    );
    assert_eq!(
        ChatApplyHook.settled_groups(&chat::FOLDER_GRANTS),
        &[folder_grant_group::GRANT]
    );
    assert_eq!(
        ChatApplyHook.settled_groups(&chat::CHANNEL_MEMBERSHIPS),
        &[channel_membership_group::ACCESS]
    );
}

const NARROW_PROFILE: &str = "access-profile:chat-sync-narrow";
const WIDE_PROFILE: &str = "access-profile:chat-sync-wide";

/// Seeds a read-only profile and a publishing one, each with an effective revision granting
/// history and participation.
async fn seed_access_profiles(pool: &SqlitePool) {
    for (profile, capability) in [(NARROW_PROFILE, "read"), (WIDE_PROFILE, "publish")] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO chat_access_profiles (id, display_name, created_at, updated_at)
             VALUES ('{profile}', 'Profile', '{NOW}', '{NOW}');
             INSERT INTO chat_access_profile_revisions
                (id, access_profile_id, default_read_history, default_participate,
                 default_history_boundary, maximum_folder_capability, created_at)
             VALUES ('{profile}:1', '{profile}', 1, 1, 'entire', '{capability}', '{NOW}');"
        )))
        .execute(pool)
        .await
        .unwrap();
    }
}

/// A channel access version under `profile` inheriting its capabilities and boundary.
fn access_version(
    writer: u8,
    profile: &str,
    channel: Option<&str>,
    scratch: Option<&str>,
    removed: bool,
    won: bool,
) -> ConcurrentVersion {
    let optional = |policy: Option<&str>| policy.map_or(Field::Null, |p| Field::Text(p.into()));
    ConcurrentVersion {
        writer: ganbaru_sync_contracts::WriterId::from_bytes([writer; 16]),
        clock: ganbaru_sync_contracts::Hlc::new(u64::from(writer), 0),
        value: Value::new(vec![
            Field::Text(profile.to_string()),
            Field::Integer(1),
            Field::Integer(1),
            Field::Integer(1),
            Field::Integer(1),
            Field::Text("entire".to_string()),
            Field::Integer(1),
            Field::Null,
            optional(channel),
            optional(scratch),
            if removed {
                Field::Text(NOW.to_string())
            } else {
                Field::Null
            },
        ])
        .unwrap(),
        won,
    }
}

/// A switch to a narrower profile concurrent with an "ask" edit the priority kept settles onto
/// both, a removal stands, and an access value round-trips through its codec.
#[tokio::test]
async fn a_channel_access_settle_keeps_a_narrower_profile_and_a_removal() {
    let pool = migrated_pool().await;
    seed_access_profiles(&pool).await;
    let mut connection = pool.acquire().await.unwrap();

    let narrowing = [
        access_version(1, NARROW_PROFILE, None, None, false, false),
        access_version(2, WIDE_PROFILE, Some("ask"), None, false, true),
    ];
    let settled = settle_channel_access(&mut connection, &narrowing)
        .await
        .unwrap();
    assert_eq!(
        settled,
        Some(access_version(3, NARROW_PROFILE, Some("ask"), None, false, true).value)
    );

    let removed = [
        access_version(1, NARROW_PROFILE, None, None, false, false),
        access_version(2, WIDE_PROFILE, Some("ask"), None, true, true),
    ];
    assert_eq!(
        settle_channel_access(&mut connection, &removed)
            .await
            .unwrap(),
        None
    );

    let live = access_version(
        1,
        WIDE_PROFILE,
        Some("ask"),
        Some("unattended"),
        false,
        true,
    );
    let decoded = channel_access(&live.value).expect("a live access value");
    assert_eq!(channel_access_value(decoded), Some(live.value));
}

fn grant_version(
    writer: u8,
    approval: Option<&str>,
    revoked: bool,
    won: bool,
) -> ConcurrentVersion {
    ConcurrentVersion {
        writer: ganbaru_sync_contracts::WriterId::from_bytes([writer; 16]),
        clock: ganbaru_sync_contracts::Hlc::new(u64::from(writer), 0),
        value: Value::new(vec![
            Field::Text("edit".to_string()),
            Field::Integer(0),
            Field::Integer(1),
            approval.map_or(Field::Null, |policy| Field::Text(policy.to_string())),
            if revoked {
                Field::Text(NOW.to_string())
            } else {
                Field::Null
            },
        ])
        .unwrap(),
        won,
    }
}

/// A revoked winner stands, and a grant whose row or channel is missing keeps its winner, since
/// the settle reads only replicated rows.
#[tokio::test]
async fn a_folder_grant_settle_keeps_a_revocation_and_a_grant_without_a_channel_override() {
    let pool = migrated_pool().await;
    let mut connection = pool.acquire().await.unwrap();
    let revoked = [
        grant_version(1, Some("unattended"), true, true),
        grant_version(2, None, false, false),
    ];
    assert_eq!(
        settle_folder_grant(&mut connection, "grant:missing", &revoked)
            .await
            .unwrap(),
        None
    );
    let live = [
        grant_version(1, Some("unattended"), false, true),
        grant_version(2, None, false, false),
    ];
    assert_eq!(
        settle_folder_grant(&mut connection, "grant:missing", &live)
            .await
            .unwrap(),
        None
    );
}

#[test]
fn only_revocations_removals_and_assignment_work_are_reported() {
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
            reclaimed_attachments: BTreeSet::from(["attachment:reclaimed".to_string()]),
            queued_assignments: BTreeSet::from(["assignment:queued".to_string()]),
            started_assignments: BTreeSet::from(["assignment:started".to_string()]),
            settled_assignments: BTreeSet::from(["assignment:settled".to_string()]),
            routed_inputs: BTreeSet::from(["assignment:routed".to_string()]),
            removed_assignments: BTreeSet::from(["assignment:removed".to_string()]),
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
            (CHAT_ASSIGNMENT_REMOVED_TOPIC, "assignment:removed"),
            (CHAT_ASSIGNMENT_QUEUED_TOPIC, "assignment:queued"),
            (CHAT_ASSIGNMENT_STARTED_TOPIC, "assignment:started"),
            (CHAT_ASSIGNMENT_INPUT_TOPIC, "assignment:routed"),
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
    let removed_assignment = ChangeNotice {
        topic: CHAT_ASSIGNMENT_REMOVED_TOPIC,
        key: "assignment:removed".to_string(),
    };
    assert!(waker::access_revoked(&[
        unrelated.clone(),
        removed_assignment
    ]));
    assert!(!waker::access_revoked(&[unrelated]));
    assert!(!waker::access_revoked(&[]));
}

#[cfg(desktop)]
#[test]
fn assignment_notices_hand_queued_and_started_work_and_pending_inputs_to_dispatch() {
    let notice = |topic: &'static str, key: &str| ChangeNotice {
        topic,
        key: key.to_string(),
    };
    let work = waker::assignment_work(&[
        notice(CHAT_ASSIGNMENT_QUEUED_TOPIC, "assignment:queued"),
        notice(CHAT_ASSIGNMENT_INPUT_TOPIC, "assignment:routed"),
        notice(CHAT_ASSIGNMENT_STARTED_TOPIC, "assignment:started"),
        notice(CHAT_ASSIGNMENT_QUEUED_TOPIC, "assignment:queued"),
        notice(CHAT_ACCESS_REVOKED_TOPIC, TEAMMATE_ID),
    ]);
    assert_eq!(
        work,
        waker::SyncedAssignmentWork {
            queued: BTreeSet::from(["assignment:queued".to_string()]),
            started: BTreeSet::from(["assignment:started".to_string()]),
            routed: BTreeSet::from(["assignment:routed".to_string()]),
        }
    );
    assert_eq!(
        waker::assignment_work(&[notice("music-root", "assignment:queued")]),
        waker::SyncedAssignmentWork::default()
    );
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

/// Deleting the last replicated revision that names an attachment removes the row this device
/// created for it, while attachments this device imported and replicated attachments a local
/// draft still names stay.
#[tokio::test]
async fn applied_message_delete_reclaims_unreferenced_replicated_attachment_rows() {
    let pool = migrated_pool().await;
    seed_applied_attachment_revision(&pool).await;
    let mut connection = pool.acquire().await.unwrap();
    let mut sink = CollectingSink::default();
    ChatApplyHook
        .after_materialize(
            &mut connection,
            &chat::REVISIONS,
            &["revision:sync".to_string()],
            &mut sink,
        )
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;
         INSERT INTO chat_attachments
             (id, working_folder_id, kind, original_display_name, mime_type, byte_size, sha256,
              managed_relative_path, signature_kind, created_at)
         VALUES ('attachment:imported', 'folder:sync', 'text_snippet', 'notes.txt', 'text/plain',
                 0, '{EMPTY_DIGEST}', 'assets/chat/attachments/{EMPTY_DIGEST}.txt', 'utf8',
                 '{NOW}');
         INSERT INTO chat_drafts (id, working_folder_id, text, updated_at)
         VALUES ('draft:sync', 'folder:sync', 'Forwarding', '{NOW}');
         INSERT INTO chat_attachment_references (id, attachment_id, draft_id, created_at)
         VALUES ('reference:draft', 'attachment:empty', 'draft:sync', '{NOW}');
         UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         DELETE FROM chat_conversation_items WHERE id = 'item:sync';"
    )))
    .execute(&mut *connection)
    .await
    .unwrap();

    ChatApplyHook
        .after_materialize(
            &mut connection,
            &chat::MESSAGES,
            &["item:sync".to_string()],
            &mut sink,
        )
        .await
        .unwrap();
    drop(connection);

    let remaining: Vec<(String, String)> = sqlx::query_as(
        "SELECT attachment.id, attachment.origin FROM chat_attachments attachment
         ORDER BY attachment.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        remaining,
        vec![
            ("attachment:empty".to_string(), "replicated".to_string()),
            ("attachment:imported".to_string(), "imported".to_string()),
        ]
    );
    let resources: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_resources WHERE id = 'attachment:image'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(resources, 0);
}

#[tokio::test]
async fn a_scheduled_attachment_keeps_its_file_after_its_attachment_row_is_gone() {
    let pool = migrated_pool().await;
    let engine = ganbaru_sync::Engine::vault();
    let kept_file = (chat::ATTACHMENTS_CLASS.name, ATTACHMENT_DIGEST.to_string());
    let mut connection = pool.acquire().await.unwrap();
    // A working folder delete cascades the attachment row away while the scheduled message
    // still names its digest; the scheduled message is seeded without its channel.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "PRAGMA foreign_keys = OFF;
         INSERT INTO chat_scheduled_messages
             (id, client_command_id, channel_id, executing_device_id, request_data,
              scheduled_for, created_at, updated_at)
         VALUES ('scheduled:sync', 'command:sync', 'channel:gone', 'device:desk', '{{}}',
                 '2026-08-04T00:00:00.000Z', '2026-08-03T00:00:00.000Z',
                 '2026-08-03T00:00:00.000Z');
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
    assert!(
        !engine
            .manifest()
            .local
            .contains(&"chat_scheduled_message_attachment_references"),
        "scheduled references travel as owned rows of their scheduled message"
    );

    // A whole-vault archive keeps scheduled references, since their scheduled message travels
    // with it.
    ganbaru_sync::local::prepare_snapshot_copy(&mut connection, engine.manifest())
        .await
        .unwrap();
    let carried: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chat_scheduled_message_attachment_references")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(carried, 1, "a snapshot copy keeps scheduled references");
    let kept = engine.kept_asset_digests(&mut connection).await.unwrap();
    assert!(kept.contains(&kept_file));

    sqlx::query("DELETE FROM chat_scheduled_message_attachment_references")
        .execute(&mut *connection)
        .await
        .unwrap();
    let kept = engine.kept_asset_digests(&mut connection).await.unwrap();
    assert!(!kept.contains(&kept_file));
}
