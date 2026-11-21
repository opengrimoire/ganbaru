use super::*;
use crate::coordination::access::APP_AGENT_SETTINGS_ID;
use crate::models::UtcTimestamp;
use crate::project_defaults::{default_channel_ids, ensure_project_chat_defaults};
use sqlx::SqlitePool;

const NOW: &str = "2026-10-24T09:00:00.000Z";
const LATER: &str = "2026-10-24T09:05:00.000Z";
const PROJECT_ID: &str = "project-sync-effects";
const OTHER_PROJECT_ID: &str = "project-sync-effects-other";
const AGENT_ID: &str = "participant:sync-effects-agent";
const OWNER_ID: &str = "participant:local-owner";
const FOLDER_ID: &str = "folder:sync-effects";
const FOLDER_ENVIRONMENT_ID: &str = "environment:folder";
const ASSIGNMENT_ID: &str = "assignment:sync-effects";

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

fn channel_id() -> String {
    default_channel_ids(PROJECT_ID).unwrap().1
}

fn membership_key(conversation: &str) -> String {
    format!("membership:{conversation}")
}

fn channel_membership_key(conversation: &str) -> String {
    format!("channel-membership:{conversation}")
}

/// Creates a project in the seeded group with its default channel.
async fn seed_project(pool: &SqlitePool, project_id: &str, order_key: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT OR IGNORE INTO project_groups (id, name, order_key)
         VALUES ('group-sync', 'Sync', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{project_id}', 'group-sync', '{project_id}', '{order_key}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ensure_project_chat_defaults(
        &mut connection,
        project_id,
        &UtcTimestamp::new(NOW.to_string()).unwrap(),
    )
    .await
    .unwrap();
}

/// Adds a channel named after `conversation` to `project_id`.
async fn add_channel(pool: &SqlitePool, project_id: &str, conversation: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         VALUES ('{conversation}', '{project_id}', 'channel', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_channels
            (id, project_id, conversation_id, name, topic, created_at, updated_at)
         VALUES ('channel:{conversation}', '{project_id}', '{conversation}', '{conversation}', '',
                 '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

/// Makes the seeded agent a present member of `conversation`.
async fn add_member(pool: &SqlitePool, conversation: &str) {
    let membership = membership_key(conversation);
    let channel_membership = channel_membership_key(conversation);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ('{conversation}', '{AGENT_ID}', '{membership}', 'member', '{NOW}', '{NOW}');
         INSERT INTO chat_agent_channel_memberships
            (conversation_id, agent_id, sync_key, created_at, updated_at)
         VALUES ('{conversation}', '{AGENT_ID}', '{channel_membership}', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

/// Starts work of the seeded agent in `conversation`: a message, its reply thread, and a
/// working assignment named `assignment`.
async fn add_assignment(pool: &SqlitePool, conversation: &str, assignment: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:{assignment}', '{conversation}', 'message', 'c100', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:{assignment}', '{OWNER_ID}', '{NOW}');
         INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('reply:{assignment}', '{conversation}', 'item:{assignment}', '{NOW}', '{NOW}',
                 '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id, state,
             model_selection_schema_version, model_selection_data, created_at, updated_at)
         VALUES ('{assignment}', 'reply:{assignment}', '{AGENT_ID}', 'item:{assignment}',
                 'working', 1, '{{}}', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

/// Seeds a project channel with one present agent, two messages, a working assignment of that
/// agent, and `authorization:first`, which only replies.
async fn seed_organization(pool: &SqlitePool) {
    seed_project(pool, PROJECT_ID, "a0").await;
    let conversation = conversation_id();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{AGENT_ID}', 'ai_agent', 'Sync agent', '{NOW}', '{NOW}');
         INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
         VALUES ('{AGENT_ID}', 'codex', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    add_member(pool, &conversation).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:first', '{conversation}', 'message', 'c000', '{NOW}'),
                ('item:second', '{conversation}', 'message', 'c001', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:first', '{OWNER_ID}', '{NOW}'),
                ('item:second', '{AGENT_ID}', '{NOW}');
         INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:first', 'item:first', 'Plan the release', '{NOW}'),
                ('revision:second', 'item:second', 'Release notes drafted', '{NOW}');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:' || substr(item_id, 6);
         INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('reply:sync-effects', '{conversation}', 'item:first', '{NOW}', '{NOW}', '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id, state,
             model_selection_schema_version, model_selection_data, created_at, updated_at)
         VALUES ('{ASSIGNMENT_ID}', 'reply:sync-effects', '{AGENT_ID}', 'item:first',
                 'working', 1, '{{}}', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    authorize(
        pool,
        "authorization:first",
        ASSIGNMENT_ID,
        &conversation,
        "ask",
        None,
    )
    .await;
}

/// Adds an external working folder to `project_id` with its current-folder environment.
async fn add_folder(pool: &SqlitePool, project_id: &str, folder: &str, environment: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_working_folders (id, project_id, display_name, kind)
         VALUES ('{folder}', '{project_id}', 'Code', 'external');
         INSERT INTO chat_execution_environments
            (id, working_folder_id, kind, display_name, created_at, updated_at)
         VALUES ('{environment}', '{folder}', 'current_folder', 'Code', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

/// What an authorization runs in: an execution environment, with the folder it works in when
/// the environment is a folder's.
struct Target<'a> {
    environment: &'a str,
    folder: Option<&'a str>,
}

const FOLDER_TARGET: Target<'static> = Target {
    environment: FOLDER_ENVIRONMENT_ID,
    folder: Some(FOLDER_ID),
};

/// Inserts an allowed authorization of `assignment` with the next revision.
async fn authorize(
    pool: &SqlitePool,
    id: &str,
    assignment: &str,
    conversation: &str,
    approval: &str,
    target: Option<Target<'_>>,
) {
    sqlx::query(
        "INSERT INTO chat_assignment_authorization_revisions
            (id, assignment_id, revision, requester_participant_id,
             destination_conversation_id, execution_environment_id, working_folder_id,
             resolved_runtime_approval_policy, scope_digest, decision_state, created_at)
         VALUES (?1, ?2,
                 (SELECT count(*) + 1 FROM chat_assignment_authorization_revisions
                  WHERE assignment_id = ?2),
                 ?3, ?4, ?5, ?6, ?7, printf('%064d', 0), 'allowed', ?8)",
    )
    .bind(id)
    .bind(assignment)
    .bind(OWNER_ID)
    .bind(conversation)
    .bind(target.as_ref().map(|target| target.environment))
    .bind(target.as_ref().and_then(|target| target.folder))
    .bind(approval)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

/// Gives `authorization` the folder `folder` at `capability` with the `approval` approval. The
/// first folder source of an authorization is its execution target.
async fn add_folder_source(
    pool: &SqlitePool,
    authorization: &str,
    folder: &str,
    capability: &str,
    approval: &str,
) {
    sqlx::query(
        "INSERT INTO chat_assignment_authorized_folder_sources
            (authorization_revision_id, root_handle, working_folder_id, capability,
             is_execution_target, resolved_runtime_approval_policy, created_at)
         VALUES (?1, printf('%064s', ?1 || ?2), ?2, ?3,
                 NOT EXISTS (SELECT 1 FROM chat_assignment_authorized_folder_sources
                             WHERE authorization_revision_id = ?1),
                 ?4, ?5)",
    )
    .bind(authorization)
    .bind(folder)
    .bind(capability)
    .bind(approval)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

/// Seeds the seeded project's folder and `authorization:folder`, which works in it at
/// `capability` under the `approval` approval.
async fn seed_folder_authorization(pool: &SqlitePool, capability: &str, approval: &str) {
    add_folder(pool, PROJECT_ID, FOLDER_ID, FOLDER_ENVIRONMENT_ID).await;
    authorize(
        pool,
        "authorization:folder",
        ASSIGNMENT_ID,
        &conversation_id(),
        approval,
        Some(FOLDER_TARGET),
    )
    .await;
    add_folder_source(
        pool,
        "authorization:folder",
        FOLDER_ID,
        capability,
        approval,
    )
    .await;
}

async fn set_channel_access(pool: &SqlitePool, conversation: &str, access: Option<&str>) {
    sqlx::query(
        "UPDATE chat_channels SET agent_access = ?, revision = revision + 1
         WHERE conversation_id = ?",
    )
    .bind(access)
    .bind(conversation)
    .execute(pool)
    .await
    .unwrap();
}

async fn set_project_access(pool: &SqlitePool, project_id: &str, access: Option<&str>) {
    sqlx::query(
        "INSERT INTO chat_project_agent_settings (project_id, agent_access, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT (project_id) DO UPDATE
         SET agent_access = excluded.agent_access, revision = revision + 1",
    )
    .bind(project_id)
    .bind(access)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

async fn set_app_access(pool: &SqlitePool, access: &str) {
    sqlx::query(
        "UPDATE chat_agent_app_settings SET agent_access = ?, revision = revision + 1
         WHERE id = 'vault'",
    )
    .bind(access)
    .execute(pool)
    .await
    .unwrap();
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
    let mut effects = run_hook(pool, CONVERSATIONS_TABLE, &[&conversation]).await;
    let mut rows: Vec<(&str, String)> = Vec::new();
    for (table, keys_sql) in [
        (
            MEMBERSHIPS_TABLE,
            "SELECT sync_key FROM chat_conversation_memberships ORDER BY sync_key",
        ),
        (
            CHANNEL_MEMBERSHIPS_TABLE,
            "SELECT sync_key FROM chat_agent_channel_memberships ORDER BY sync_key",
        ),
    ] {
        let keys: Vec<String> = sqlx::query_scalar(keys_sql).fetch_all(pool).await.unwrap();
        rows.extend(keys.into_iter().map(|key| (table, key)));
    }
    for (table, key) in rows {
        let members = run_hook(pool, table, &[&key]).await;
        effects.audience_revised.extend(members.audience_revised);
        effects.revoked_agents.extend(members.revoked_agents);
    }
    let messages = run_hook(pool, MESSAGES_TABLE, &["item:first", "item:second"]).await;
    effects.search_rebuilt.extend(messages.search_rebuilt);
    effects
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

async fn authorization_state(pool: &SqlitePool, id: &str) -> String {
    sqlx::query_scalar(
        "SELECT decision_state FROM chat_assignment_authorization_revisions WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Returns the ids of the revoked authorizations.
async fn revoked(pool: &SqlitePool) -> Vec<String> {
    authorization_states(pool)
        .await
        .into_iter()
        .filter(|(_, state, _)| state == "revoked")
        .map(|(id, _, _)| id)
        .collect()
}

async fn queued_jobs(pool: &SqlitePool) -> Vec<(String, Option<String>)> {
    sqlx::query_as(
        "SELECT agent_id, authorization_revision_id FROM chat_access_revocation_jobs
         WHERE state = 'queued' ORDER BY authorization_revision_id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

fn agents() -> BTreeSet<String> {
    BTreeSet::from([AGENT_ID.to_string()])
}

#[tokio::test]
async fn hook_derives_the_state_the_gated_triggers_derive_for_commands() {
    let commanded = migrated_pool().await;
    seed_organization(&commanded).await;

    let applied = migrated_pool().await;
    set_applying(&applied, true).await;
    seed_organization(&applied).await;
    assert_eq!(audience_revision(&applied).await, None);
    assert!(search_rows(&applied).await.is_empty());

    let effects = run_hook_for_seeded_rows(&applied).await;
    set_applying(&applied, false).await;

    assert_eq!(search_rows(&applied).await, search_rows(&commanded).await);
    assert_eq!(search_rows(&applied).await.len(), 2);
    assert_eq!(
        audience_revision(&applied).await,
        audience_revision(&commanded).await
    );
    assert_eq!(
        effects.audience_revised,
        BTreeSet::from([conversation_id()])
    );
    assert_eq!(effects.search_rebuilt.len(), 2);
    assert!(effects.revoked_agents.is_empty());
    assert_eq!(
        authorization_states(&applied).await,
        authorization_states(&commanded).await
    );

    set_applying(&applied, true).await;
    let search_before = search_rows(&applied).await;
    run_hook(&applied, MESSAGES_TABLE, &["item:first", "item:second"]).await;
    assert_eq!(search_rows(&applied).await, search_before);
    assert!(
        run_hook(&applied, CHANNEL_MEMBERSHIPS_TABLE, &["unresolved"])
            .await
            .is_empty()
    );
    assert!(
        run_hook(&applied, CHANNELS_TABLE, &[&channel_id()])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_channel_narrowing_revokes_work_above_the_new_level_and_a_widening_keeps_it() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_folder_authorization(&pool, "edit", "ask").await;
    let conversation = conversation_id();
    let channel = channel_id();
    set_applying(&pool, true).await;

    set_channel_access(&pool, &conversation, Some("own")).await;
    let widened = run_hook(&pool, CHANNELS_TABLE, &[&channel]).await;
    assert!(widened.is_empty(), "{widened:?}");

    set_channel_access(&pool, &conversation, Some("read")).await;
    let narrowed = run_hook(&pool, CHANNELS_TABLE, &[&channel]).await;
    assert_eq!(narrowed.revoked_agents, agents());
    assert_eq!(
        authorization_states(&pool).await,
        vec![
            (
                "authorization:first".to_string(),
                "allowed".to_string(),
                String::new()
            ),
            (
                "authorization:folder".to_string(),
                "revoked".to_string(),
                ACCESS_REDUCTION_REASON.to_string()
            ),
        ]
    );
    assert_eq!(
        queued_jobs(&pool).await,
        vec![(
            AGENT_ID.to_string(),
            Some("authorization:folder".to_string())
        )]
    );

    // Talk still lets the agent reply, so work without a target stays.
    set_channel_access(&pool, &conversation, Some("talk")).await;
    let talk = run_hook(&pool, CHANNELS_TABLE, &[&channel]).await;
    assert!(talk.revoked_agents.is_empty());
    assert_eq!(
        authorization_state(&pool, "authorization:first").await,
        "allowed"
    );

    set_channel_access(&pool, &conversation, Some("own")).await;
    let restored = run_hook(&pool, CHANNELS_TABLE, &[&channel]).await;
    assert!(restored.is_empty(), "{restored:?}");
    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:folder".to_string()]
    );
    assert_eq!(queued_jobs(&pool).await.len(), 1);
}

#[tokio::test]
async fn read_keeps_read_only_folder_work_and_talk_revokes_any_target() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_folder_authorization(&pool, "read", "ask").await;
    let conversation = conversation_id();
    set_applying(&pool, true).await;

    set_channel_access(&pool, &conversation, Some("read")).await;
    let read = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert!(read.revoked_agents.is_empty());

    set_channel_access(&pool, &conversation, Some("talk")).await;
    let talk = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert_eq!(talk.revoked_agents, agents());
    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:folder".to_string()]
    );
}

#[tokio::test]
async fn auto_approval_needs_own_on_the_authorization_and_on_each_folder_source() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    set_app_access(&pool, "ask").await;
    let conversation = conversation_id();
    set_channel_access(&pool, &conversation, Some("own")).await;
    seed_folder_authorization(&pool, "execute", "auto_approve").await;
    authorize(
        &pool,
        "authorization:source-only",
        ASSIGNMENT_ID,
        &conversation,
        "ask",
        Some(FOLDER_TARGET),
    )
    .await;
    add_folder_source(
        &pool,
        "authorization:source-only",
        FOLDER_ID,
        "execute",
        "auto_approve",
    )
    .await;
    authorize(
        &pool,
        "authorization:asks",
        ASSIGNMENT_ID,
        &conversation,
        "ask",
        Some(FOLDER_TARGET),
    )
    .await;
    add_folder_source(&pool, "authorization:asks", FOLDER_ID, "publish", "ask").await;
    set_applying(&pool, true).await;

    let own = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert!(own.is_empty(), "{own:?}");

    set_channel_access(&pool, &conversation, None).await;
    let inherited_ask = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert_eq!(inherited_ask.revoked_agents, agents());
    assert_eq!(
        revoked(&pool).await,
        vec![
            "authorization:folder".to_string(),
            "authorization:source-only".to_string()
        ]
    );
}

#[tokio::test]
async fn a_project_narrowing_reaches_every_inheriting_channel() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    add_folder(&pool, PROJECT_ID, FOLDER_ID, FOLDER_ENVIRONMENT_ID).await;
    let default = conversation_id();
    let channels = [default.clone(), "conversation:second".to_string()];
    add_channel(&pool, PROJECT_ID, "conversation:second").await;
    add_channel(&pool, PROJECT_ID, "conversation:own").await;
    set_channel_access(&pool, "conversation:own", Some("own")).await;
    for conversation in ["conversation:second", "conversation:own"] {
        add_member(&pool, conversation).await;
    }
    for conversation in [default.as_str(), "conversation:second", "conversation:own"] {
        let assignment = format!("assignment:{conversation}");
        if conversation != default {
            add_assignment(&pool, conversation, &assignment).await;
        }
        let assignment = if conversation == default {
            ASSIGNMENT_ID.to_string()
        } else {
            assignment
        };
        let authorization = format!("authorization:{conversation}");
        authorize(
            &pool,
            &authorization,
            &assignment,
            conversation,
            "ask",
            Some(FOLDER_TARGET),
        )
        .await;
        add_folder_source(&pool, &authorization, FOLDER_ID, "edit", "ask").await;
        add_push_approval(&pool, conversation).await;
    }
    set_applying(&pool, true).await;

    set_project_access(&pool, PROJECT_ID, Some("read")).await;
    let narrowed = run_hook(&pool, PROJECT_SETTINGS_TABLE, &[PROJECT_ID]).await;

    assert_eq!(narrowed.revoked_agents, agents());
    let mut expected: Vec<String> = channels
        .iter()
        .map(|conversation| format!("authorization:{conversation}"))
        .collect();
    expected.sort();
    assert_eq!(revoked(&pool).await, expected);
    assert_eq!(
        authorization_state(&pool, "authorization:conversation:own").await,
        "allowed"
    );
    assert_eq!(queued_jobs(&pool).await.len(), 2);
    assert_eq!(
        narrowed.lapsed_push_approvals,
        channels.iter().cloned().collect::<BTreeSet<_>>()
    );
    let unrevoked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_push_approvals WHERE revoked_at IS NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unrevoked, 3, "the hook never writes push approvals");

    let replayed = run_hook(&pool, PROJECT_SETTINGS_TABLE, &[PROJECT_ID]).await;
    assert!(replayed.revoked_agents.is_empty());
    assert_eq!(queued_jobs(&pool).await.len(), 2);
}

#[tokio::test]
async fn an_app_narrowing_reaches_inheriting_projects_and_spares_projects_with_their_own() {
    let pool = migrated_pool().await;
    set_app_access(&pool, "own").await;
    seed_organization(&pool).await;
    seed_folder_authorization(&pool, "publish", "auto_approve").await;
    seed_project(&pool, OTHER_PROJECT_ID, "a1").await;
    set_project_access(&pool, OTHER_PROJECT_ID, Some("own")).await;
    let other = default_channel_ids(OTHER_PROJECT_ID).unwrap().0;
    add_member(&pool, &other).await;
    add_assignment(&pool, &other, "assignment:other").await;
    add_folder(&pool, OTHER_PROJECT_ID, "folder:other", "environment:other").await;
    authorize(
        &pool,
        "authorization:other",
        "assignment:other",
        &other,
        "auto_approve",
        Some(Target {
            environment: "environment:other",
            folder: Some("folder:other"),
        }),
    )
    .await;
    add_folder_source(
        &pool,
        "authorization:other",
        "folder:other",
        "publish",
        "auto_approve",
    )
    .await;
    set_applying(&pool, true).await;

    set_app_access(&pool, "ask").await;
    let narrowed = run_hook(&pool, APP_SETTINGS_TABLE, &[APP_AGENT_SETTINGS_ID]).await;

    assert_eq!(narrowed.revoked_agents, agents());
    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:folder".to_string()]
    );
    assert_eq!(
        authorization_state(&pool, "authorization:other").await,
        "allowed"
    );
    assert!(narrowed.lapsed_push_approvals.is_empty());
}

#[tokio::test]
async fn a_missing_app_settings_row_fails_closed_to_talk() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_folder_authorization(&pool, "read", "ask").await;
    set_applying(&pool, true).await;
    sqlx::query("DELETE FROM chat_agent_app_settings")
        .execute(&pool)
        .await
        .unwrap();

    let effects = run_hook(&pool, APP_SETTINGS_TABLE, &[APP_AGENT_SETTINGS_ID]).await;

    assert_eq!(effects.revoked_agents, agents());
    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:folder".to_string()]
    );
}

/// Seeds a private scratch environment of the seeded reply thread and `authorization:scratch`,
/// which works in it.
async fn seed_scratch_authorization(pool: &SqlitePool) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_scratch_scopes (id, reply_thread_id, agent_id, created_at, updated_at)
         VALUES ('scratch:sync-effects', 'reply:sync-effects', '{AGENT_ID}', '{NOW}', '{NOW}');
         INSERT INTO chat_scratch_generations
            (id, scratch_scope_id, generation, created_at, updated_at)
         VALUES ('generation:sync-effects', 'scratch:sync-effects', 1, '{NOW}', '{NOW}');
         INSERT INTO chat_execution_environments
            (id, scratch_generation_id, kind, display_name, created_at, updated_at)
         VALUES ('environment:scratch', 'generation:sync-effects', 'scratch', 'Scratch', '{NOW}',
                 '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
    authorize(
        pool,
        "authorization:scratch",
        ASSIGNMENT_ID,
        &conversation_id(),
        "ask",
        Some(Target {
            environment: "environment:scratch",
            folder: None,
        }),
    )
    .await;
}

#[tokio::test]
async fn scratch_work_stays_under_ask_and_is_revoked_below_it() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_scratch_authorization(&pool).await;
    set_applying(&pool, true).await;

    let ask = run_hook(&pool, APP_SETTINGS_TABLE, &[APP_AGENT_SETTINGS_ID]).await;
    assert!(ask.is_empty(), "{ask:?}");

    set_project_access(&pool, PROJECT_ID, Some("read")).await;
    let read = run_hook(&pool, PROJECT_SETTINGS_TABLE, &[PROJECT_ID]).await;
    assert_eq!(read.revoked_agents, agents());
    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:scratch".to_string()]
    );
}

#[tokio::test]
async fn a_folder_source_outside_the_destination_project_is_revoked() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_project(&pool, OTHER_PROJECT_ID, "a1").await;
    add_folder(&pool, PROJECT_ID, FOLDER_ID, FOLDER_ENVIRONMENT_ID).await;
    add_folder(&pool, OTHER_PROJECT_ID, "folder:other", "environment:other").await;
    authorize(
        &pool,
        "authorization:folder",
        ASSIGNMENT_ID,
        &conversation_id(),
        "ask",
        Some(FOLDER_TARGET),
    )
    .await;
    add_folder_source(&pool, "authorization:folder", FOLDER_ID, "edit", "ask").await;
    add_folder_source(&pool, "authorization:folder", "folder:other", "read", "ask").await;
    set_applying(&pool, true).await;

    let effects = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;

    assert_eq!(effects.revoked_agents, agents());
    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:folder".to_string()]
    );
}

async fn set_participant_archived(pool: &SqlitePool, id: &str, archived_at: Option<&str>) {
    sqlx::query("UPDATE chat_participants SET archived_at = ? WHERE id = ?")
        .bind(archived_at)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn synced_participant_archive_revokes_authorizations() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    set_applying(&pool, true).await;

    let renamed = run_hook(&pool, PARTICIPANTS_TABLE, &[AGENT_ID]).await;
    assert!(renamed.is_empty(), "{renamed:?}");
    assert_eq!(
        authorization_state(&pool, "authorization:first").await,
        "allowed"
    );

    set_participant_archived(&pool, AGENT_ID, Some(NOW)).await;
    let archived = run_hook(&pool, PARTICIPANTS_TABLE, &[AGENT_ID]).await;

    assert_eq!(archived.revoked_agents, agents());
    assert_eq!(
        authorization_states(&pool).await,
        vec![(
            "authorization:first".to_string(),
            "revoked".to_string(),
            AGENT_ARCHIVED_REASON.to_string()
        )]
    );
    assert_eq!(queued_jobs(&pool).await.len(), 1);

    let replayed = run_hook(&pool, PARTICIPANTS_TABLE, &[AGENT_ID]).await;
    assert!(replayed.is_empty(), "{replayed:?}");
    assert_eq!(queued_jobs(&pool).await.len(), 1);
}

#[tokio::test]
async fn search_follows_revisions_that_arrive_after_their_message_and_deleted_messages() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    let conversation = conversation_id();

    set_applying(&pool, true).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_items (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:late', '{conversation}', 'message', 'c002', '{NOW}');
         INSERT INTO chat_communication_messages (item_id, author_participant_id, created_at)
         VALUES ('item:late', '{AGENT_ID}', '{NOW}');"
    )))
    .execute(&pool)
    .await
    .unwrap();
    let pending = run_hook(&pool, MESSAGES_TABLE, &["item:late"]).await;
    assert!(pending.search_rebuilt.is_empty());

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:late', 'item:late', 'Arrived after its message', '{NOW}');
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

#[tokio::test]
async fn an_applied_membership_removal_revokes_the_authorization_it_covered_once() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    let audience_before = audience_revision(&pool).await.unwrap();
    let key = membership_key(&conversation_id());

    set_applying(&pool, true).await;
    sqlx::query(
        "UPDATE chat_conversation_memberships SET removed_at = ?1, updated_at = ?1
         WHERE participant_id = ?2",
    )
    .bind(LATER)
    .bind(AGENT_ID)
    .execute(&pool)
    .await
    .unwrap();
    let effects = run_hook(&pool, MEMBERSHIPS_TABLE, &[&key]).await;

    assert_eq!(effects.revoked_agents, agents());
    assert_eq!(
        effects.audience_revised,
        BTreeSet::from([conversation_id()])
    );
    assert_eq!(
        authorization_states(&pool).await,
        vec![(
            "authorization:first".to_string(),
            "revoked".to_string(),
            ACCESS_REDUCTION_REASON.to_string()
        )]
    );
    assert_eq!(queued_jobs(&pool).await.len(), 1);
    assert_eq!(audience_revision(&pool).await, Some(audience_before + 1));

    let replayed = run_hook(&pool, MEMBERSHIPS_TABLE, &[&key]).await;
    assert!(replayed.revoked_agents.is_empty());
    assert_eq!(queued_jobs(&pool).await.len(), 1);
    assert!(
        run_hook(&pool, MEMBERSHIPS_TABLE, &["membership:unknown"])
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn an_applied_channel_membership_removal_or_deletion_revokes_the_authorization() {
    for delete in [false, true] {
        let pool = migrated_pool().await;
        seed_organization(&pool).await;
        let key = channel_membership_key(&conversation_id());

        set_applying(&pool, true).await;
        let statement = if delete {
            "DELETE FROM chat_agent_channel_memberships WHERE sync_key = ?2"
        } else {
            "UPDATE chat_agent_channel_memberships SET removed_at = ?1, updated_at = ?1
             WHERE sync_key = ?2"
        };
        sqlx::query(sqlx::AssertSqlSafe(statement))
            .bind(LATER)
            .bind(&key)
            .execute(&pool)
            .await
            .unwrap();
        let effects = run_hook(&pool, CHANNEL_MEMBERSHIPS_TABLE, &[&key]).await;

        assert_eq!(effects.revoked_agents, agents(), "delete {delete}");
        assert_eq!(
            authorization_states(&pool).await,
            vec![(
                "authorization:first".to_string(),
                "revoked".to_string(),
                ACCESS_REDUCTION_REASON.to_string()
            )]
        );
        let replayed = run_hook(&pool, CHANNEL_MEMBERSHIPS_TABLE, &[&key]).await;
        assert!(replayed.revoked_agents.is_empty());
        assert_eq!(queued_jobs(&pool).await.len(), 1);
    }
}

#[tokio::test]
async fn leaving_a_disclosed_channel_revokes_the_work_that_read_it() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    add_channel(&pool, PROJECT_ID, "conversation:source").await;
    add_member(&pool, "conversation:source").await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_message_references
            (id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
             start_offset, end_offset, ordinal, created_at)
         VALUES ('reference:source', 'revision:first', 'channel', '#source', '#source', 0, 7, 0,
                 '{NOW}');
         INSERT INTO chat_assignment_authorized_channel_sources
            (authorization_revision_id, source_handle, message_reference_id, conversation_id,
             label_snapshot, lower_order_key, lower_item_id, high_order_key, high_item_id,
             source_revision_cutoff_id, destination_audience_revision, created_at)
         VALUES ('authorization:first', printf('%064d', 7), 'reference:source',
                 'conversation:source', '#source', 'c000', 'item:first', 'c001', 'item:second',
                 'revision:first', 1, '{NOW}');"
    )))
    .execute(&pool)
    .await
    .unwrap();
    let key = membership_key("conversation:source");
    set_applying(&pool, true).await;

    let present = run_hook(&pool, MEMBERSHIPS_TABLE, &[&key]).await;
    assert!(present.revoked_agents.is_empty());

    sqlx::query(
        "UPDATE chat_conversation_memberships SET removed_at = ?1, updated_at = ?1
         WHERE sync_key = ?2",
    )
    .bind(LATER)
    .bind(&key)
    .execute(&pool)
    .await
    .unwrap();
    let left = run_hook(&pool, MEMBERSHIPS_TABLE, &[&key]).await;

    assert_eq!(left.revoked_agents, agents());
    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:first".to_string()]
    );
}

#[tokio::test]
async fn a_command_narrowing_revokes_the_same_work_as_an_applied_one() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_folder_authorization(&pool, "edit", "ask").await;
    set_channel_access(&pool, &conversation_id(), Some("read")).await;

    let mut connection = pool.acquire().await.unwrap();
    let revoked_agents = revoke_exceeding_authorizations(&mut connection, LATER)
        .await
        .unwrap();
    assert_eq!(revoked_agents, agents());
    let again = revoke_exceeding_authorizations(&mut connection, LATER)
        .await
        .unwrap();
    assert!(again.is_empty());
    drop(connection);

    assert_eq!(
        revoked(&pool).await,
        vec!["authorization:folder".to_string()]
    );
    let revoked_at: Option<String> = sqlx::query_scalar(
        "SELECT revoked_at FROM chat_assignment_authorization_revisions
         WHERE id = 'authorization:folder'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revoked_at.as_deref(), Some(LATER));
}

/// Inserts an approval for the seeded agent to push the seeded folder from `conversation`.
async fn add_push_approval(pool: &SqlitePool, conversation: &str) {
    sqlx::query(
        "INSERT INTO chat_push_approvals
            (conversation_id, agent_id, working_folder_id, push_url, destination_branch,
             sync_key, approved_by_participant_id, approved_at)
         VALUES (?, ?, ?, 'https://example.test/repo.git', 'main', ?, ?, ?)",
    )
    .bind(conversation)
    .bind(AGENT_ID)
    .bind(FOLDER_ID)
    .bind(format!("push:{conversation}"))
    .bind(OWNER_ID)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

async fn push_approvals(pool: &SqlitePool) -> Vec<(String, Option<String>, i64)> {
    sqlx::query_as(
        "SELECT conversation_id, revoked_at, revision FROM chat_push_approvals
         ORDER BY conversation_id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn push_approvals_lapse_below_ask_and_the_caller_revokes_them_after_the_apply() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    add_folder(&pool, PROJECT_ID, FOLDER_ID, FOLDER_ENVIRONMENT_ID).await;
    add_channel(&pool, PROJECT_ID, "conversation:kept").await;
    add_member(&pool, "conversation:kept").await;
    set_channel_access(&pool, "conversation:kept", Some("own")).await;
    let conversation = conversation_id();
    add_push_approval(&pool, &conversation).await;
    add_push_approval(&pool, "conversation:kept").await;
    set_applying(&pool, true).await;

    let ask = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert!(ask.lapsed_push_approvals.is_empty());

    set_channel_access(&pool, &conversation, Some("read")).await;
    let read = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert_eq!(
        read.lapsed_push_approvals,
        BTreeSet::from([conversation.clone()])
    );
    assert!(
        push_approvals(&pool)
            .await
            .iter()
            .all(|(_, revoked_at, _)| revoked_at.is_none())
    );
    set_applying(&pool, false).await;

    let mut connection = pool.acquire().await.unwrap();
    let lapsed = revoke_lapsed_push_approvals(&mut connection, LATER)
        .await
        .unwrap();
    assert_eq!(lapsed, BTreeSet::from([conversation.clone()]));
    let again = revoke_lapsed_push_approvals(&mut connection, LATER)
        .await
        .unwrap();
    assert!(again.is_empty());
    drop(connection);

    let mut expected = vec![
        (conversation.clone(), Some(LATER.to_string()), 2),
        ("conversation:kept".to_string(), None, 1),
    ];
    expected.sort();
    assert_eq!(push_approvals(&pool).await, expected);

    set_applying(&pool, true).await;
    let after = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert!(after.lapsed_push_approvals.is_empty());
}

/// Inserts an approval for the seeded agent to fetch from `origin` in `conversation`, granted
/// under `access`.
async fn add_remembered_approval(
    pool: &SqlitePool,
    conversation: &str,
    origin: &str,
    access: &str,
) {
    sqlx::query(
        "INSERT INTO chat_remembered_approvals
            (conversation_id, agent_id, subject_kind, subject_key, sync_key,
             approved_by_participant_id, approved_at, granted_access)
         VALUES (?, ?, 'url_origin', ?, ?, ?, ?, ?)",
    )
    .bind(conversation)
    .bind(AGENT_ID)
    .bind(origin)
    .bind(format!("remembered:{conversation}:{origin}"))
    .bind(OWNER_ID)
    .bind(NOW)
    .bind(access)
    .execute(pool)
    .await
    .unwrap();
}

async fn remembered_approvals(pool: &SqlitePool) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT conversation_id, subject_key, revoked_at FROM chat_remembered_approvals
         ORDER BY conversation_id, subject_key",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// An approval lapses once its channel's access ranks below the access it was granted under,
/// while one granted under a lower access stays.
#[tokio::test]
async fn remembered_approvals_lapse_below_their_granted_access_and_the_caller_revokes_them() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    add_channel(&pool, PROJECT_ID, "conversation:kept").await;
    add_member(&pool, "conversation:kept").await;
    set_channel_access(&pool, "conversation:kept", Some("own")).await;
    let conversation = conversation_id();
    add_remembered_approval(&pool, &conversation, "https://ask.example.com", "ask").await;
    add_remembered_approval(&pool, &conversation, "https://read.example.com", "read").await;
    add_remembered_approval(&pool, "conversation:kept", "https://own.example.com", "own").await;
    set_applying(&pool, true).await;

    let ask = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert!(ask.lapsed_remembered_approvals.is_empty());

    set_channel_access(&pool, &conversation, Some("read")).await;
    let read = run_hook(&pool, CHANNELS_TABLE, &[&channel_id()]).await;
    assert_eq!(
        read.lapsed_remembered_approvals,
        BTreeSet::from([conversation.clone()])
    );
    set_applying(&pool, false).await;

    let mut connection = pool.acquire().await.unwrap();
    let lapsed = revoke_lapsed_remembered_approvals(&mut connection, LATER)
        .await
        .unwrap();
    assert_eq!(lapsed, BTreeSet::from([conversation.clone()]));
    assert!(
        revoke_lapsed_remembered_approvals(&mut connection, LATER)
            .await
            .unwrap()
            .is_empty()
    );
    drop(connection);

    let mut expected = vec![
        (
            conversation.clone(),
            "https://ask.example.com".to_string(),
            Some(LATER.to_string()),
        ),
        (
            conversation.clone(),
            "https://read.example.com".to_string(),
            None,
        ),
        (
            "conversation:kept".to_string(),
            "https://own.example.com".to_string(),
            None,
        ),
    ];
    expected.sort();
    assert_eq!(remembered_approvals(&pool).await, expected);

    // An archive that arrives by sync skips the local trigger, so the sweep revokes the rest.
    set_applying(&pool, true).await;
    set_participant_archived(&pool, AGENT_ID, Some(LATER)).await;
    set_applying(&pool, false).await;
    let mut connection = pool.acquire().await.unwrap();
    let archived = revoke_lapsed_remembered_approvals(&mut connection, LATER)
        .await
        .unwrap();
    assert_eq!(
        archived,
        BTreeSet::from([conversation, "conversation:kept".to_string()])
    );
}

#[tokio::test]
async fn an_applied_conversation_deletion_reports_it_and_sweeps_remaining_authorizations() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_project(&pool, OTHER_PROJECT_ID, "a1").await;
    let other = default_channel_ids(OTHER_PROJECT_ID).unwrap().0;

    set_applying(&pool, true).await;
    sqlx::query("DELETE FROM chat_agent_channel_memberships WHERE sync_key = ?")
        .bind(channel_membership_key(&conversation_id()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM chat_channels WHERE conversation_id = ?")
        .bind(&other)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM chat_conversations WHERE id = ?")
        .bind(&other)
        .execute(&pool)
        .await
        .unwrap();
    let effects = run_hook(&pool, CONVERSATIONS_TABLE, &[&other]).await;

    assert_eq!(
        effects.removed_conversations,
        BTreeSet::from([other.clone()])
    );
    assert_eq!(effects.revoked_agents, agents());
    assert_eq!(
        authorization_state(&pool, "authorization:first").await,
        "revoked"
    );
    assert_eq!(queued_jobs(&pool).await.len(), 1);

    let replayed = run_hook(&pool, CONVERSATIONS_TABLE, &[&other]).await;
    assert!(replayed.revoked_agents.is_empty());
    assert_eq!(queued_jobs(&pool).await.len(), 1);
}

/// The reason recorded on the revoked authorization of the seeded agent.
async fn revocation_reason(pool: &SqlitePool) -> String {
    let states = authorization_states(pool).await;
    assert!(
        states.iter().all(|(_, state, _)| state == "revoked"),
        "{states:?}"
    );
    states[0].2.clone()
}

/// An archive or a narrowing can reach the sweep from any table's hook, and the revocation
/// records the reason that fits the agent rather than the table that ran it.
#[tokio::test]
async fn sweep_records_the_specific_reason() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    set_applying(&pool, true).await;
    set_participant_archived(&pool, AGENT_ID, Some(NOW)).await;
    run_hook(&pool, CONVERSATIONS_TABLE, &["conversation:removed"]).await;
    assert_eq!(revocation_reason(&pool).await, AGENT_ARCHIVED_REASON);

    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    seed_scratch_authorization(&pool).await;
    set_applying(&pool, true).await;
    set_channel_access(&pool, &conversation_id(), Some("talk")).await;
    run_hook(&pool, PARTICIPANTS_TABLE, &[AGENT_ID]).await;
    assert_eq!(
        authorization_states(&pool)
            .await
            .into_iter()
            .find(|(id, _, _)| id == "authorization:scratch")
            .map(|(_, _, reason)| reason),
        Some(ACCESS_REDUCTION_REASON.to_string())
    );

    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    set_applying(&pool, true).await;
    set_participant_archived(&pool, AGENT_ID, Some(NOW)).await;
    run_hook(
        &pool,
        MEMBERSHIPS_TABLE,
        &[&membership_key(&conversation_id())],
    )
    .await;
    assert_eq!(revocation_reason(&pool).await, AGENT_ARCHIVED_REASON);
}

async fn dispatch_job_state(pool: &SqlitePool) -> String {
    sqlx::query_scalar("SELECT state FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?")
        .bind(ASSIGNMENT_ID)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_assignment_state(pool: &SqlitePool, state: &str) {
    sqlx::query("UPDATE chat_work_assignments SET state = ?1, settled_at = CASE WHEN ?1 IN ('completed', 'failed', 'cancelled') THEN updated_at END WHERE id = ?2")
        .bind(state)
        .bind(ASSIGNMENT_ID)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_remotely_settled_assignment_ends_its_dispatch_and_revokes_its_runs_once() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, claim_token, available_at, created_at, updated_at)
         VALUES ('dispatch:sync-effects', ?1, 'claimed', 'claim:sync-effects', ?2, ?2, ?2)",
    )
    .bind(ASSIGNMENT_ID)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();

    let working = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;
    assert_eq!(
        working.started_assignments,
        BTreeSet::from([ASSIGNMENT_ID.to_string()])
    );
    assert!(working.settled_assignments.is_empty());
    assert!(working.revoked_agents.is_empty());
    assert_eq!(dispatch_job_state(&pool).await, "claimed");

    set_assignment_state(&pool, "cancelled").await;
    let settled = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;
    assert_eq!(
        settled.settled_assignments,
        BTreeSet::from([ASSIGNMENT_ID.to_string()])
    );
    assert_eq!(settled.revoked_agents, agents());
    assert_eq!(dispatch_job_state(&pool).await, "cancelled");
    assert_eq!(
        authorization_states(&pool).await,
        vec![(
            "authorization:first".to_string(),
            "revoked".to_string(),
            ASSIGNMENT_SETTLED_REASON.to_string()
        )]
    );
    assert_eq!(queued_jobs(&pool).await.len(), 1);

    let replayed = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;
    assert!(replayed.is_empty());
    assert_eq!(queued_jobs(&pool).await.len(), 1);
}

#[tokio::test]
async fn a_remote_settle_stops_a_claim_that_has_not_authorized_yet() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    // An earlier run's authorization is already revoked, so the claimed launch has nothing
    // active to revoke.
    sqlx::query(
        "UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', reason = 'Earlier run ended', revoked_at = ?",
    )
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, claim_token, available_at, created_at, updated_at)
         VALUES ('dispatch:sync-effects', ?1, 'claimed', 'claim:first', ?2, ?2, ?2)",
    )
    .bind(ASSIGNMENT_ID)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    set_assignment_state(&pool, "cancelled").await;

    let settled = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;

    assert_eq!(dispatch_job_state(&pool).await, "cancelled");
    assert_eq!(
        settled.settled_assignments,
        BTreeSet::from([ASSIGNMENT_ID.to_string()])
    );
    assert_eq!(settled.revoked_agents, agents());
    let replayed = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;
    assert!(replayed.is_empty());
}

#[tokio::test]
async fn queued_assignments_and_pending_inputs_are_reported_for_their_executing_device() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    set_assignment_state(&pool, "queued").await;
    sqlx::query(
        "INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, delivery_state,
             created_at)
         VALUES ('input:first', ?1, 'item:first', 1, 'trigger', 'delivered', ?2),
                ('input:second', ?1, 'item:second', 2, 'steer', 'pending', ?2)",
    )
    .bind(ASSIGNMENT_ID)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();

    let queued = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;
    assert_eq!(
        queued.queued_assignments,
        BTreeSet::from([ASSIGNMENT_ID.to_string()])
    );
    assert!(queued.revoked_agents.is_empty());
    assert!(queued.started_assignments.is_empty());

    for state in ["working", "waiting_for_answer", "waiting_for_approval"] {
        set_assignment_state(&pool, state).await;
        let started = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;
        assert_eq!(
            started.started_assignments,
            BTreeSet::from([ASSIGNMENT_ID.to_string()]),
            "{state}"
        );
        assert!(started.queued_assignments.is_empty());
        assert!(started.revoked_agents.is_empty());
    }
    set_assignment_state(&pool, "ready_for_review").await;
    assert!(
        run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID])
            .await
            .is_empty()
    );
    set_assignment_state(&pool, "queued").await;

    let delivered = run_hook(&pool, ASSIGNMENT_INPUTS_TABLE, &["input:first"]).await;
    assert!(delivered.is_empty());
    let pending = run_hook(
        &pool,
        ASSIGNMENT_INPUTS_TABLE,
        &["input:second", "input:gone"],
    )
    .await;
    assert_eq!(
        pending.routed_inputs,
        BTreeSet::from([ASSIGNMENT_ID.to_string()])
    );
}

/// A reply that syncs in after the turn ended is reported while its work still continues with
/// it: on work ready for review, and on work that completed by itself with no follow-up yet.
#[tokio::test]
async fn a_pending_input_on_work_whose_turn_ended_is_reported_until_a_follow_up_takes_it() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    sqlx::query(
        "INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, delivery_state,
             created_at)
         VALUES ('input:second', ?, 'item:second', 2, 'queued_continuation', 'pending', ?)",
    )
    .bind(ASSIGNMENT_ID)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    let reported = BTreeSet::from([ASSIGNMENT_ID.to_string()]);
    async fn routed(pool: &SqlitePool) -> BTreeSet<String> {
        run_hook(pool, ASSIGNMENT_INPUTS_TABLE, &["input:second"])
            .await
            .routed_inputs
    }

    for state in ["ready_for_review", "completed"] {
        set_assignment_state(&pool, state).await;
        assert_eq!(routed(&pool).await, reported, "{state}");
    }
    for state in ["failed", "cancelled"] {
        set_assignment_state(&pool, state).await;
        assert!(routed(&pool).await.is_empty(), "{state}");
    }

    sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'completed', review_outcome = 'accepted', reviewed_at = ?,
             reviewed_by_participant_id = 'participant:local'
         WHERE id = ?",
    )
    .bind(NOW)
    .bind(ASSIGNMENT_ID)
    .execute(&pool)
    .await
    .unwrap();
    assert!(routed(&pool).await.is_empty(), "reviewed");

    sqlx::query(
        "UPDATE chat_work_assignments
         SET review_outcome = NULL, reviewed_at = NULL, reviewed_by_participant_id = NULL
         WHERE id = ?",
    )
    .bind(ASSIGNMENT_ID)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id,
             previous_assignment_id, state, model_selection_schema_version,
             model_selection_data, created_at, updated_at)
         VALUES ('assignment:successor', 'reply:sync-effects', ?1, 'item:second',
                 ?2, 'queued', 1, '{}', ?3, ?3)",
    )
    .bind(AGENT_ID)
    .bind(ASSIGNMENT_ID)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    assert!(routed(&pool).await.is_empty(), "followed");
}

#[tokio::test]
async fn an_applied_delegated_child_in_an_outcome_state_is_reported_for_its_parent() {
    let pool = migrated_pool().await;
    seed_organization(&pool).await;
    let conversation = conversation_id();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES ('reply:sync-effects-child', '{conversation}', 'item:second', '{NOW}', '{NOW}',
                 '{NOW}');
         INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id, state,
             parent_assignment_id, delegation_depth, delegation_objective,
             delegation_expected_result, delegation_folder_ceiling,
             model_selection_schema_version, model_selection_data, created_at, updated_at)
         VALUES ('assignment:sync-effects-child', 'reply:sync-effects-child', '{AGENT_ID}',
                 'item:second', 'working', '{ASSIGNMENT_ID}', 1, 'Draft notes',
                 'A summary', '[]', 1, '{{}}', '{NOW}', '{NOW}');"
    )))
    .execute(&pool)
    .await
    .unwrap();
    let child = "assignment:sync-effects-child";

    let working = run_hook(&pool, ASSIGNMENTS_TABLE, &[child]).await;
    assert!(working.delegated_outcomes.is_empty());
    for state in ["completed", "ready_for_review", "failed", "cancelled"] {
        sqlx::query("UPDATE chat_work_assignments SET state = ?1, settled_at = CASE WHEN ?1 IN ('completed', 'failed', 'cancelled') THEN updated_at END WHERE id = ?2")
            .bind(state)
            .bind(child)
            .execute(&pool)
            .await
            .unwrap();
        let settled = run_hook(&pool, ASSIGNMENTS_TABLE, &[child]).await;
        assert_eq!(
            settled.delegated_outcomes,
            BTreeSet::from([child.to_string()]),
            "{state}"
        );
    }

    set_assignment_state(&pool, "completed").await;
    let parent = run_hook(&pool, ASSIGNMENTS_TABLE, &[ASSIGNMENT_ID]).await;
    assert!(parent.delegated_outcomes.is_empty());
}

/// An approval lapses with its agent: an applied removal from the conversation or an applied
/// archive reports it, and a commanded removal, which no trigger covers, is revoked by the
/// caller's call.
#[tokio::test]
async fn push_approvals_lapse_when_their_agent_leaves_or_is_archived() {
    for case in ["applied removal", "applied archive", "commanded removal"] {
        let pool = migrated_pool().await;
        seed_organization(&pool).await;
        add_folder(&pool, PROJECT_ID, FOLDER_ID, FOLDER_ENVIRONMENT_ID).await;
        let conversation = conversation_id();
        add_push_approval(&pool, &conversation).await;
        let key = membership_key(&conversation);
        let applied = case != "commanded removal";
        set_applying(&pool, applied).await;

        let present = run_hook(&pool, MEMBERSHIPS_TABLE, &[&key]).await;
        assert!(present.lapsed_push_approvals.is_empty(), "{case}");

        let effects = if case == "applied archive" {
            set_participant_archived(&pool, AGENT_ID, Some(LATER)).await;
            run_hook(&pool, PARTICIPANTS_TABLE, &[AGENT_ID]).await
        } else {
            sqlx::query(
                "UPDATE chat_conversation_memberships SET removed_at = ?1, updated_at = ?1
                 WHERE sync_key = ?2",
            )
            .bind(LATER)
            .bind(&key)
            .execute(&pool)
            .await
            .unwrap();
            run_hook(&pool, MEMBERSHIPS_TABLE, &[&key]).await
        };
        assert_eq!(
            effects.lapsed_push_approvals,
            BTreeSet::from([conversation.clone()]),
            "{case}"
        );
        assert_eq!(
            push_approvals(&pool).await,
            vec![(conversation.clone(), None, 1)],
            "{case}: only the caller revokes"
        );

        set_applying(&pool, false).await;
        let mut connection = pool.acquire().await.unwrap();
        let revoked = revoke_lapsed_push_approvals(&mut connection, LATER)
            .await
            .unwrap();
        assert_eq!(revoked, BTreeSet::from([conversation.clone()]), "{case}");
        drop(connection);
        assert_eq!(
            push_approvals(&pool).await,
            vec![(conversation.clone(), Some(LATER.to_string()), 2)],
            "{case}"
        );
    }
}
