use super::*;
use crate::models::UtcTimestamp;
use crate::project_defaults::{default_channel_ids, ensure_project_chat_defaults};
use sqlx::SqlitePool;

const NOW: &str = "2026-10-24T09:00:00.000Z";
const PROJECT_ID: &str = "project-sync-effects";
const TEAMMATE_ID: &str = "participant:sync-effects-agent";
const PROFILE_ID: &str = "access-profile:sync-effects";
const FIRST_PROFILE_REVISION_ID: &str = "profile-revision:sync-effects:1";

type AccessStateRow = (String, i64, String);
type SearchRow = (String, String, Option<String>, String, String);

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

async fn set_applying(pool: &SqlitePool, applying: bool) {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(applying)
        .execute(pool)
        .await
        .unwrap();
}

fn conversation_id() -> String {
    default_channel_ids(PROJECT_ID).unwrap().0
}

/// Seeds a project channel with one teammate using a custom profile (revision 1 allows
/// editing), two messages, and one active assignment authorization of that teammate.
async fn seed_organization(pool: &SqlitePool) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-sync', 'Sync', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{PROJECT_ID}', 'group-sync', 'Sync', 'a0');"
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
    let conversation = conversation_id();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'ai_teammate', 'Sync agent', '{NOW}', '{NOW}');
         INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
         VALUES ('{TEAMMATE_ID}', 'Test', '{NOW}', '{NOW}');
         INSERT INTO chat_teammate_policy_revisions
            (id, teammate_id, provider_instance_id, model_selection_data, created_at)
         VALUES ('policy:sync-effects', '{TEAMMATE_ID}', 'codex-personal', '{{}}', '{NOW}');
         INSERT INTO chat_access_profiles (id, display_name, created_at, updated_at)
         VALUES ('{PROFILE_ID}', 'Sync profile', '{NOW}', '{NOW}');
         INSERT INTO chat_access_profile_revisions
            (id, access_profile_id, default_read_history, default_participate,
             default_history_boundary, maximum_folder_capability, created_at)
         VALUES ('profile-revision:sync-effects:1', '{PROFILE_ID}', 1, 1, 'entire', 'edit',
                 '{NOW}');
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, membership_role, created_at, updated_at)
         VALUES ('{conversation}', '{TEAMMATE_ID}', 'member', '{NOW}', '{NOW}');
         INSERT INTO chat_teammate_channel_memberships
            (conversation_id, teammate_id, access_profile_id,
             read_history, participate, history_boundary, created_at, updated_at)
         VALUES ('{conversation}', '{TEAMMATE_ID}', '{PROFILE_ID}', 1, 1, 'entire',
                 '{NOW}', '{NOW}');
         INSERT INTO chat_conversation_items (id, conversation_id, item_kind, ordinal, created_at)
         VALUES ('item:first', '{conversation}', 'message', 1, '{NOW}'),
                ('item:second', '{conversation}', 'message', 2, '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:first', 'participant:local-owner', '{NOW}'),
                ('item:second', '{TEAMMATE_ID}', '{NOW}');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, revision, normalized_markdown, created_at)
         VALUES ('revision:first', 'item:first', 1, 'Plan the release', '{NOW}'),
                ('revision:second', 'item:second', 1, 'Release notes drafted', '{NOW}');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:' || substr(item_id, 6);
         INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('reply:sync-effects', '{conversation}', 'item:first', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, teammate_id, triggering_message_item_id, state,
             created_at, updated_at)
         VALUES ('assignment:sync-effects', 'reply:sync-effects', '{TEAMMATE_ID}', 'item:first',
                 'working', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    insert_authorization(pool, "authorization:first", 1, FIRST_PROFILE_REVISION_ID).await;
}

/// Inserts an allowed authorization of the seeded assignment granted under
/// `profile_revision_id`.
async fn insert_authorization(
    pool: &SqlitePool,
    id: &str,
    revision: i64,
    profile_revision_id: &str,
) {
    sqlx::query(
        "INSERT INTO chat_assignment_authorization_revisions
            (id, assignment_id, revision, requester_participant_id,
             teammate_policy_revision_id, teammate_access_revision,
             destination_conversation_id, access_profile_revision_id,
             resolved_runtime_approval_policy, scope_digest, decision_state, created_at)
         VALUES (?, 'assignment:sync-effects', ?, 'participant:local-owner',
                 'policy:sync-effects', 1, ?, ?,
                 'ask', printf('%064d', 0), 'allowed', ?)",
    )
    .bind(id)
    .bind(revision)
    .bind(conversation_id())
    .bind(profile_revision_id)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_profile_revision(pool: &SqlitePool, revision: i64, capability: &str) -> String {
    let id = format!("profile-revision:sync-effects:{revision}");
    sqlx::query(
        "INSERT INTO chat_access_profile_revisions
            (id, access_profile_id, default_read_history, default_participate,
             default_history_boundary, maximum_folder_capability, created_at)
         VALUES (?, ?, 1, 1, 'entire', ?, ?)",
    )
    .bind(&id)
    .bind(PROFILE_ID)
    .bind(capability)
    .bind(format!("2026-10-24T09:0{revision}:00.000Z"))
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn run_hook(pool: &SqlitePool, table: &str, keys: &[&str]) -> ChatSyncEffects {
    let keys: Vec<String> = keys.iter().map(|key| (*key).to_string()).collect();
    let mut connection = pool.acquire().await.unwrap();
    after_materialize(&mut connection, table, &keys)
        .await
        .unwrap()
}

/// Runs the hook for every seeded row the way sync reports them after a materialization.
async fn run_hook_for_seeded_rows(pool: &SqlitePool) -> ChatSyncEffects {
    let conversation = conversation_id();
    let mut effects = run_hook(pool, TEAMMATES_TABLE, &[TEAMMATE_ID]).await;
    let conversations = run_hook(pool, CONVERSATIONS_TABLE, &[&conversation]).await;
    effects
        .audience_revised
        .extend(conversations.audience_revised);
    let profiles = run_hook(
        pool,
        ACCESS_PROFILE_REVISIONS_TABLE,
        &["profile-revision:sync-effects:1"],
    )
    .await;
    effects.audience_revised.extend(profiles.audience_revised);
    let messages = run_hook(pool, MESSAGES_TABLE, &["item:first", "item:second"]).await;
    effects.search_rebuilt.extend(messages.search_rebuilt);
    effects
}

async fn access_state(pool: &SqlitePool) -> Vec<AccessStateRow> {
    sqlx::query_as(
        "SELECT teammate_id, access_revision, updated_at
         FROM chat_teammate_access_state ORDER BY teammate_id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn audience_revision(pool: &SqlitePool) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT revision FROM chat_conversation_audience_state WHERE conversation_id = ?",
    )
    .bind(conversation_id())
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn search_rows(pool: &SqlitePool) -> Vec<SearchRow> {
    sqlx::query_as(
        "SELECT message_item_id, conversation_id, reply_thread_id, author_display_name,
                normalized_markdown
         FROM chat_communication_search_fts ORDER BY message_item_id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn authorization_states(pool: &SqlitePool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT id, decision_state, reason FROM chat_assignment_authorization_revisions
         ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn queued_jobs(pool: &SqlitePool) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT teammate_id, authorization_revision_id FROM chat_access_revocation_jobs
         WHERE state = 'queued' ORDER BY authorization_revision_id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn hook_derives_the_state_the_gated_triggers_derive_for_commands() {
    let commanded = migrated_pool().await;
    seed_organization(&commanded).await;

    let applied = migrated_pool().await;
    set_applying(&applied, true).await;
    seed_organization(&applied).await;
    assert!(access_state(&applied).await.is_empty());
    assert_eq!(audience_revision(&applied).await, None);
    assert!(search_rows(&applied).await.is_empty());

    let effects = run_hook_for_seeded_rows(&applied).await;
    set_applying(&applied, false).await;

    assert_eq!(access_state(&applied).await, access_state(&commanded).await);
    assert_eq!(search_rows(&applied).await, search_rows(&commanded).await);
    assert_eq!(search_rows(&applied).await.len(), 2);
    assert!(audience_revision(&applied).await.is_some());
    assert_eq!(
        effects.access_state_created,
        BTreeSet::from([TEAMMATE_ID.to_string()])
    );
    assert_eq!(
        effects.audience_revised,
        BTreeSet::from([conversation_id()])
    );
    assert_eq!(effects.search_rebuilt.len(), 2);
    assert!(effects.revoked_teammates.is_empty());
    assert_eq!(
        authorization_states(&applied).await,
        authorization_states(&commanded).await
    );

    set_applying(&applied, true).await;
    let access_before = access_state(&applied).await;
    let search_before = search_rows(&applied).await;
    let again = run_hook(&applied, TEAMMATES_TABLE, &[TEAMMATE_ID]).await;
    run_hook(&applied, MESSAGES_TABLE, &["item:first", "item:second"]).await;
    assert!(again.is_empty());
    assert_eq!(access_state(&applied).await, access_before);
    assert_eq!(search_rows(&applied).await, search_before);
    assert!(
        run_hook(
            &applied,
            "chat_teammate_channel_memberships",
            &["unresolved"]
        )
        .await
        .is_empty()
    );
}

#[tokio::test]
async fn narrowed_profile_revisions_revoke_once_and_widened_ones_keep_authority() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    let access_before = access_state(&pool).await[0].1;
    let audience_before = audience_revision(&pool).await.unwrap();

    set_applying(&pool, true).await;
    let narrowed = insert_profile_revision(&pool, 2, "read").await;
    assert_eq!(audience_revision(&pool).await, Some(audience_before));
    let effects = run_hook(&pool, ACCESS_PROFILE_REVISIONS_TABLE, &[&narrowed]).await;

    assert_eq!(
        effects.revoked_teammates,
        BTreeSet::from([TEAMMATE_ID.to_string()])
    );
    assert_eq!(
        effects.access_revised,
        BTreeSet::from([TEAMMATE_ID.to_string()])
    );
    assert_eq!(
        authorization_states(&pool).await,
        vec![(
            "authorization:first".to_string(),
            "revoked".to_string(),
            PROFILE_REDUCTION_REASON.to_string()
        )]
    );
    assert_eq!(
        queued_jobs(&pool).await,
        vec![(
            TEAMMATE_ID.to_string(),
            Some("authorization:first".to_string())
        )]
    );
    assert_eq!(access_state(&pool).await[0].1, access_before + 1);
    assert_eq!(audience_revision(&pool).await, Some(audience_before + 1));

    let replayed = run_hook(&pool, ACCESS_PROFILE_REVISIONS_TABLE, &[&narrowed]).await;
    assert!(replayed.revoked_teammates.is_empty());
    assert_eq!(queued_jobs(&pool).await.len(), 1);

    set_applying(&pool, false).await;
    insert_authorization(&pool, "authorization:second", 2, FIRST_PROFILE_REVISION_ID).await;
    set_applying(&pool, true).await;
    let access_after_narrowing = access_state(&pool).await[0].1;
    let widened = insert_profile_revision(&pool, 3, "publish").await;
    let effects = run_hook(&pool, ACCESS_PROFILE_REVISIONS_TABLE, &[&widened]).await;

    assert!(effects.revoked_teammates.is_empty());
    assert!(effects.access_revised.is_empty());
    assert_eq!(
        effects.audience_revised,
        BTreeSet::from([conversation_id()])
    );
    assert_eq!(access_state(&pool).await[0].1, access_after_narrowing);
    assert_eq!(authorization_states(&pool).await[1].1, "allowed");
    assert_eq!(queued_jobs(&pool).await.len(), 1);
}

/// Teammate policy pointers have no counterpart: dispatch reads the effective policy revision when
/// it runs and authorizations never carry a wider policy, so policy rows never revoke.
#[tokio::test]
async fn a_pointer_naming_an_older_stricter_revision_revokes_what_the_wider_one_allowed() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;

    set_applying(&pool, true).await;
    let stricter = insert_profile_revision(&pool, 2, "edit").await;
    let wider = insert_profile_revision(&pool, 3, "publish").await;
    point_profile_at(&pool, &wider, "publish").await;
    let widened = run_hook(&pool, ACCESS_PROFILES_TABLE, &[PROFILE_ID]).await;
    assert!(widened.revoked_teammates.is_empty());
    assert!(widened.access_revised.is_empty());

    set_applying(&pool, false).await;
    insert_authorization(&pool, "authorization:wider", 2, &wider).await;
    set_applying(&pool, true).await;
    let access_before = access_state(&pool).await[0].1;

    // The stricter revision is older than the wider one and as wide as the revision before it,
    // so only the comparison with later revisions can find the narrowing.
    point_profile_at(&pool, &stricter, "edit").await;
    let effects = run_hook(&pool, ACCESS_PROFILES_TABLE, &[PROFILE_ID]).await;

    assert_eq!(
        effects.revoked_teammates,
        BTreeSet::from([TEAMMATE_ID.to_string()])
    );
    assert_eq!(
        effects.access_revised,
        BTreeSet::from([TEAMMATE_ID.to_string()])
    );
    assert_eq!(access_state(&pool).await[0].1, access_before + 1);
    let states = authorization_states(&pool).await;
    assert!(
        states
            .iter()
            .all(|(_, state, reason)| state == "revoked" && reason == PROFILE_REDUCTION_REASON),
        "{states:?}"
    );
    assert_eq!(
        queued_jobs(&pool).await,
        vec![
            (
                TEAMMATE_ID.to_string(),
                Some("authorization:first".to_string())
            ),
            (
                TEAMMATE_ID.to_string(),
                Some("authorization:wider".to_string())
            ),
        ]
    );
}

/// Points the seeded profile at `revision_id`, copying its folder capability the way commands
/// and the coupled sync group do.
async fn point_profile_at(pool: &SqlitePool, revision_id: &str, capability: &str) {
    sqlx::query(
        "UPDATE chat_access_profiles
         SET current_revision_id = ?, current_maximum_folder_capability = ?
         WHERE id = ?",
    )
    .bind(revision_id)
    .bind(capability)
    .bind(PROFILE_ID)
    .execute(pool)
    .await
    .unwrap();
}

/// Runs the hook for `keys` of `table` and returns the error code it refused them with.
async fn hook_refusal(pool: &SqlitePool, table: &str, keys: &[&str]) -> Option<ChatErrorCode> {
    let keys: Vec<String> = keys.iter().map(|key| (*key).to_string()).collect();
    let mut connection = pool.acquire().await.unwrap();
    after_materialize(&mut connection, table, &keys)
        .await
        .err()
        .map(|error| error.code)
}

/// Points the seeded teammate at its seeded policy revision with a copied `safety_mode`.
async fn point_teammate_policy_at(pool: &SqlitePool, safety_mode: &str) {
    sqlx::query(
        "UPDATE chat_teammates
         SET current_policy_revision_id = 'policy:sync-effects', current_policy_safety_mode = ?
         WHERE participant_id = ?",
    )
    .bind(safety_mode)
    .bind(TEAMMATE_ID)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn pointers_whose_copied_restrictiveness_differs_from_their_revision_are_refused() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    set_applying(&pool, true).await;

    point_teammate_policy_at(&pool, "full_access").await;
    assert_eq!(
        hook_refusal(&pool, TEAMMATES_TABLE, &[TEAMMATE_ID]).await,
        Some(ChatErrorCode::Validation)
    );
    assert_eq!(
        hook_refusal(&pool, POLICY_REVISIONS_TABLE, &["policy:sync-effects"]).await,
        Some(ChatErrorCode::Validation)
    );
    point_teammate_policy_at(&pool, "ask_for_approval").await;
    assert_eq!(
        hook_refusal(&pool, TEAMMATES_TABLE, &[TEAMMATE_ID]).await,
        None
    );
    assert_eq!(
        hook_refusal(&pool, POLICY_REVISIONS_TABLE, &["policy:sync-effects"]).await,
        None
    );

    point_profile_at(&pool, FIRST_PROFILE_REVISION_ID, "publish").await;
    assert_eq!(
        hook_refusal(&pool, ACCESS_PROFILES_TABLE, &[PROFILE_ID]).await,
        Some(ChatErrorCode::Validation)
    );
    assert_eq!(
        hook_refusal(
            &pool,
            ACCESS_PROFILE_REVISIONS_TABLE,
            &[FIRST_PROFILE_REVISION_ID]
        )
        .await,
        Some(ChatErrorCode::Validation)
    );

    // A pointer that arrives before its revision applies, and the revision is checked on arrival.
    let pending = "profile-revision:sync-effects:2";
    point_profile_at(&pool, pending, "read").await;
    assert_eq!(
        hook_refusal(&pool, ACCESS_PROFILES_TABLE, &[PROFILE_ID]).await,
        None
    );
    insert_profile_revision(&pool, 2, "publish").await;
    assert_eq!(
        hook_refusal(&pool, ACCESS_PROFILE_REVISIONS_TABLE, &[pending]).await,
        Some(ChatErrorCode::Validation)
    );
    point_profile_at(&pool, pending, "publish").await;
    assert_eq!(
        hook_refusal(&pool, ACCESS_PROFILE_REVISIONS_TABLE, &[pending]).await,
        None
    );
}

#[tokio::test]
async fn search_follows_revisions_that_arrive_after_their_message_and_deleted_messages() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    let conversation = conversation_id();

    set_applying(&pool, true).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_items (id, conversation_id, item_kind, ordinal, created_at)
         VALUES ('item:late', '{conversation}', 'message', 3, '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:late', '{TEAMMATE_ID}', '{NOW}');"
    )))
    .execute(&pool)
    .await
    .unwrap();
    let pending = run_hook(&pool, MESSAGES_TABLE, &["item:late"]).await;
    assert!(pending.search_rebuilt.is_empty());

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, revision, normalized_markdown, created_at)
         VALUES ('revision:late', 'item:late', 1, 'Arrived after its message', '{NOW}');
         UPDATE chat_communication_messages SET current_revision_id = 'revision:late'
         WHERE item_id = 'item:late';"
    )))
    .execute(&pool)
    .await
    .unwrap();
    let rebuilt = run_hook(&pool, MESSAGE_REVISIONS_TABLE, &["revision:late"]).await;
    assert_eq!(
        rebuilt.search_rebuilt,
        BTreeSet::from(["item:late".to_string()])
    );
    let late: Vec<String> = sqlx::query_scalar(
        "SELECT normalized_markdown FROM chat_communication_search_fts
         WHERE message_item_id = 'item:late'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(late, vec!["Arrived after its message".to_string()]);

    sqlx::query("DELETE FROM chat_conversation_items WHERE id = 'item:second'")
        .execute(&pool)
        .await
        .unwrap();
    // The search delete trigger stays active during apply, so the hook finds nothing left to remove.
    let removed = run_hook(&pool, MESSAGES_TABLE, &["item:second"]).await;
    assert!(removed.search_rebuilt.is_empty());
    let remaining: Vec<String> = search_rows(&pool)
        .await
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(
        remaining,
        vec!["item:first".to_string(), "item:late".to_string()]
    );
}
