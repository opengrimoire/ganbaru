use super::*;
use crate::models::ChatAgentAccessSource;
use crate::project_defaults::{default_channel_ids, ensure_project_chat_defaults};
use sqlx::SqlitePool;

const NOW: &str = "2026-11-18T09:00:00.000Z";
const LATER: &str = "2026-11-18T09:05:00.000Z";
const PROJECT_ID: &str = "project-agent-settings";
const AGENT_ID: &str = "participant:settings-agent";
const OTHER_AGENT_ID: &str = "participant:settings-other";
const OWNER_ID: &str = "participant:local-owner";
const FOLDER_ID: &str = "folder:agent-settings";

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

fn conversation_id() -> String {
    default_channel_ids(PROJECT_ID).unwrap().0
}

fn channel_id() -> String {
    default_channel_ids(PROJECT_ID).unwrap().1
}

/// Seeds a project with its default channel, a working folder, and two AI agents.
async fn seed(pool: &SqlitePool) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-settings', 'S', 'a0');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('{PROJECT_ID}', 'group-settings', 'Settings', 'a0');
         INSERT INTO project_working_folders (id, project_id, display_name, kind)
         VALUES ('{FOLDER_ID}', '{PROJECT_ID}', 'Code', 'external');
         INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{AGENT_ID}', 'ai_agent', 'Codex', '{NOW}', '{NOW}'),
                ('{OTHER_AGENT_ID}', 'ai_agent', 'Claude', '{NOW}', '{NOW}');
         INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
         VALUES ('{AGENT_ID}', 'codex', '{NOW}', '{NOW}'),
                ('{OTHER_AGENT_ID}', 'claude', '{NOW}', '{NOW}');"
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
}

/// Makes `agent` a present member of the default channel.
async fn add_member(pool: &SqlitePool, agent: &str) {
    let conversation = conversation_id();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
         VALUES ('{conversation}', '{agent}', 'membership:{agent}', 'member', '{NOW}', '{NOW}');
         INSERT INTO chat_agent_channel_memberships
            (conversation_id, agent_id, sync_key, created_at, updated_at)
         VALUES ('{conversation}', '{agent}', 'channel-membership:{agent}', '{NOW}', '{NOW}');"
    )))
    .execute(pool)
    .await
    .unwrap();
}

async fn execute(pool: &SqlitePool, sql: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn app_settings_read_the_seeded_own_default_and_its_saved_values() {
    let pool = migrated_pool().await;
    let mut connection = pool.acquire().await.unwrap();
    let seeded = read_app_agent_settings(&mut connection).await.unwrap();
    assert_eq!(seeded.access, ChatAgentAccess::Own);
    assert_eq!(seeded.instructions, "");
    drop(connection);

    execute(
        &pool,
        "UPDATE chat_agent_app_settings
         SET agent_access = 'read', agent_instructions = 'Be brief', revision = revision + 1",
    )
    .await;
    let mut connection = pool.acquire().await.unwrap();
    let saved = read_app_agent_settings(&mut connection).await.unwrap();
    assert_eq!(saved.access, ChatAgentAccess::Read);
    assert_eq!(saved.instructions, "Be brief");
    assert_eq!(saved.revision, seeded.revision + 1);
}

#[tokio::test]
async fn a_missing_app_settings_row_is_invalid_data_rather_than_a_default() {
    let pool = migrated_pool().await;
    execute(&pool, "DELETE FROM chat_agent_app_settings").await;
    let mut connection = pool.acquire().await.unwrap();
    let error = read_app_agent_settings(&mut connection).await.unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Persistence);
}

#[tokio::test]
async fn a_project_without_its_own_settings_inherits_the_app_access_at_revision_zero() {
    let pool = migrated_pool().await;
    seed(&pool).await;
    let mut connection = pool.acquire().await.unwrap();
    let inherited = read_project_agent_settings(&mut connection, PROJECT_ID)
        .await
        .unwrap();
    assert_eq!(inherited.access, None);
    assert_eq!(inherited.instructions, "");
    assert_eq!(inherited.revision, 0);
    assert_eq!(inherited.effective.access, ChatAgentAccess::Own);
    assert_eq!(inherited.effective.source, ChatAgentAccessSource::App);
    drop(connection);

    execute(
        &pool,
        &format!(
            "INSERT INTO chat_project_agent_settings
                (project_id, agent_access, agent_instructions, created_at, updated_at)
             VALUES ('{PROJECT_ID}', 'read', 'Project rules', '{NOW}', '{NOW}')"
        ),
    )
    .await;
    let mut connection = pool.acquire().await.unwrap();
    let own = read_project_agent_settings(&mut connection, PROJECT_ID)
        .await
        .unwrap();
    assert_eq!(own.access, Some(ChatAgentAccess::Read));
    assert_eq!(own.instructions, "Project rules");
    assert_eq!(own.revision, 1);
    assert_eq!(own.effective.access, ChatAgentAccess::Read);
    assert_eq!(own.effective.source, ChatAgentAccessSource::Project);
}

#[tokio::test]
async fn channel_access_resolves_from_the_narrowest_scope_that_sets_it() {
    let pool = migrated_pool().await;
    seed(&pool).await;
    let conversation = conversation_id();
    let read = |pool: SqlitePool, conversation: String| async move {
        let mut connection = pool.acquire().await.unwrap();
        read_channel_agent_settings(&mut connection, &conversation)
            .await
            .unwrap()
    };

    let app = read(pool.clone(), conversation.clone()).await;
    assert_eq!(app.channel_id.as_str(), channel_id());
    assert_eq!(app.conversation_id.as_str(), conversation);
    assert_eq!(app.project_id, PROJECT_ID);
    assert_eq!(app.access, None);
    assert_eq!(
        (app.effective.access, app.effective.source),
        (ChatAgentAccess::Own, ChatAgentAccessSource::App)
    );

    execute(
        &pool,
        "UPDATE chat_agent_app_settings SET agent_access = 'read'",
    )
    .await;
    let narrowed_app = read(pool.clone(), conversation.clone()).await;
    assert_eq!(narrowed_app.effective.access, ChatAgentAccess::Read);

    execute(
        &pool,
        &format!(
            "INSERT INTO chat_project_agent_settings
                (project_id, agent_access, created_at, updated_at)
             VALUES ('{PROJECT_ID}', 'talk', '{NOW}', '{NOW}')"
        ),
    )
    .await;
    let project = read(pool.clone(), conversation.clone()).await;
    assert_eq!(
        (project.effective.access, project.effective.source),
        (ChatAgentAccess::Talk, ChatAgentAccessSource::Project)
    );

    execute(
        &pool,
        &format!(
            "UPDATE chat_channels SET agent_access = 'read', revision = revision + 1
             WHERE conversation_id = '{conversation}'"
        ),
    )
    .await;
    let channel = read(pool.clone(), conversation.clone()).await;
    assert_eq!(channel.access, Some(ChatAgentAccess::Read));
    assert_eq!(
        (channel.effective.access, channel.effective.source),
        (ChatAgentAccess::Read, ChatAgentAccessSource::Channel)
    );
    assert_eq!(channel.revision, app.revision + 1);
}

#[tokio::test]
async fn an_unknown_channel_is_not_found() {
    let pool = migrated_pool().await;
    let mut connection = pool.acquire().await.unwrap();
    let error = read_channel_agent_settings(&mut connection, "conversation:missing")
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::NotFound);
    let error = read_channel_instructions(&mut connection, "conversation:missing")
        .await
        .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::NotFound);
}

#[tokio::test]
async fn channel_instructions_compose_every_scope_that_has_text() {
    let pool = migrated_pool().await;
    seed(&pool).await;
    let conversation = conversation_id();
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        read_channel_instructions(&mut connection, &conversation)
            .await
            .unwrap(),
        ""
    );
    drop(connection);

    execute(
        &pool,
        &format!(
            "UPDATE chat_agent_app_settings SET agent_instructions = 'App rules';
             UPDATE chat_channels SET agent_instructions = '  Channel rules  '
             WHERE conversation_id = '{conversation}';"
        ),
    )
    .await;
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        read_channel_instructions(&mut connection, &conversation)
            .await
            .unwrap(),
        compose_instructions("App rules", "", "Channel rules")
    );
    drop(connection);

    execute(
        &pool,
        &format!(
            "INSERT INTO chat_project_agent_settings
                (project_id, agent_instructions, created_at, updated_at)
             VALUES ('{PROJECT_ID}', 'Project rules', '{NOW}', '{NOW}')"
        ),
    )
    .await;
    let mut connection = pool.acquire().await.unwrap();
    let composed = read_channel_instructions(&mut connection, &conversation)
        .await
        .unwrap();
    assert_eq!(
        composed,
        compose_instructions("App rules", "Project rules", "Channel rules")
    );
    let app = composed.find("App rules").unwrap();
    let project = composed.find("Project rules").unwrap();
    let channel = composed.find("Channel rules").unwrap();
    assert!(app < project && project < channel, "{composed}");
}

#[tokio::test]
async fn channel_members_are_the_agents_present_in_both_memberships_and_not_archived() {
    let pool = migrated_pool().await;
    seed(&pool).await;
    add_member(&pool, AGENT_ID).await;
    add_member(&pool, OTHER_AGENT_ID).await;
    let conversation = conversation_id();
    let members = |pool: SqlitePool, conversation: String| async move {
        let mut connection = pool.acquire().await.unwrap();
        read_channel_agent_members(&mut connection, &conversation)
            .await
            .unwrap()
            .into_iter()
            .map(|member| {
                (
                    member.agent_id.as_str().to_string(),
                    member.provider_family_id,
                )
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(
        members(pool.clone(), conversation.clone()).await,
        vec![
            (AGENT_ID.to_string(), "codex".to_string()),
            (OTHER_AGENT_ID.to_string(), "claude".to_string()),
        ]
    );

    execute(
        &pool,
        &format!(
            "UPDATE chat_agent_channel_memberships SET removed_at = '{LATER}'
             WHERE agent_id = '{OTHER_AGENT_ID}'"
        ),
    )
    .await;
    assert_eq!(
        members(pool.clone(), conversation.clone()).await,
        vec![(AGENT_ID.to_string(), "codex".to_string())]
    );

    execute(
        &pool,
        &format!(
            "UPDATE chat_agent_channel_memberships SET removed_at = NULL
             WHERE agent_id = '{OTHER_AGENT_ID}';
             UPDATE chat_conversation_memberships SET removed_at = '{LATER}'
             WHERE participant_id = '{OTHER_AGENT_ID}';"
        ),
    )
    .await;
    assert_eq!(members(pool.clone(), conversation.clone()).await.len(), 1);

    execute(
        &pool,
        &format!("UPDATE chat_participants SET archived_at = '{LATER}' WHERE id = '{AGENT_ID}'"),
    )
    .await;
    assert!(members(pool.clone(), conversation.clone()).await.is_empty());

    let mut connection = pool.acquire().await.unwrap();
    assert!(
        !agent_is_present(&mut connection, &conversation, AGENT_ID)
            .await
            .unwrap()
    );
    assert!(
        read_channel_agent_members(&mut connection, "conversation:missing")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn agent_presence_follows_the_membership() {
    let pool = migrated_pool().await;
    seed(&pool).await;
    add_member(&pool, AGENT_ID).await;
    let conversation = conversation_id();
    let mut connection = pool.acquire().await.unwrap();
    assert!(
        agent_is_present(&mut connection, &conversation, AGENT_ID)
            .await
            .unwrap()
    );
    assert!(
        !agent_is_present(&mut connection, &conversation, OTHER_AGENT_ID)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn push_approvals_list_the_unrevoked_ones_newest_first() {
    let pool = migrated_pool().await;
    seed(&pool).await;
    add_member(&pool, AGENT_ID).await;
    let conversation = conversation_id();
    for (key, branch, approved_at) in [
        ("push:old", "main", NOW),
        ("push:new", "release", LATER),
        ("push:revoked", "dev", LATER),
    ] {
        sqlx::query(
            "INSERT INTO chat_push_approvals
                (conversation_id, agent_id, working_folder_id, push_url, destination_branch,
                 sync_key, approved_by_participant_id, approved_at)
             VALUES (?, ?, ?, 'https://example.test/repo.git', ?, ?, ?, ?)",
        )
        .bind(&conversation)
        .bind(AGENT_ID)
        .bind(FOLDER_ID)
        .bind(branch)
        .bind(key)
        .bind(OWNER_ID)
        .bind(approved_at)
        .execute(&pool)
        .await
        .unwrap();
    }
    execute(
        &pool,
        &format!(
            "UPDATE chat_push_approvals SET revoked_at = '{LATER}', revision = revision + 1
             WHERE sync_key = 'push:revoked'"
        ),
    )
    .await;

    let mut connection = pool.acquire().await.unwrap();
    let approvals = read_channel_push_approvals(&mut connection, &conversation)
        .await
        .unwrap();
    let summary: Vec<(&str, &str)> = approvals
        .iter()
        .map(|approval| {
            (
                approval.approval_id.as_str(),
                approval.destination_branch.as_str(),
            )
        })
        .collect();
    assert_eq!(summary, vec![("push:new", "release"), ("push:old", "main")]);
    let newest = &approvals[0];
    assert_eq!(newest.channel_id.as_str(), channel_id());
    assert_eq!(newest.agent_id.as_str(), AGENT_ID);
    assert_eq!(newest.working_folder_id.as_str(), FOLDER_ID);
    assert_eq!(newest.working_folder_name, "Code");
    assert_eq!(newest.push_url, "https://example.test/repo.git");
    assert_eq!(newest.approved_at.as_str(), LATER);
    assert!(
        read_channel_push_approvals(&mut connection, "conversation:missing")
            .await
            .unwrap()
            .is_empty()
    );
}
