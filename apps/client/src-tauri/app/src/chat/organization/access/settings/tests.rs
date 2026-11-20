//! Saving agent settings at each scope, what narrowing them revokes, and what a channel offers
//! each agent.

use super::*;
use crate::chat::organization::tests::{full_support, test_providers_with};
use crate::chat::revocation::tests::pool_with_folder_runs;
use ganbaru_chat::project_defaults::default_channel_ids;

const PROJECT: &str = "project-chat";
const AGENT: &str = "participant:folder-agent";

/// The conversation and channel ids of the fixture project's default channel.
fn channel_ids() -> (String, String) {
    default_channel_ids(PROJECT).unwrap()
}

/// Remembers a push from `workspace-1` for the fixture agent in its channel.
async fn approve_push(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO chat_push_approvals
            (conversation_id, agent_id, working_folder_id, push_url, destination_branch,
             sync_key, approved_by_participant_id, approved_at)
         VALUES (?, ?, 'workspace-1', 'https://example.invalid/repo.git', 'main',
                 'push-approval:folders', 'participant:local-owner', ?)",
    )
    .bind(&channel_ids().0)
    .bind(AGENT)
    .bind(now_timestamp().unwrap().as_str())
    .execute(pool)
    .await
    .unwrap();
}

/// The decision state of every authorization, by id.
async fn authorization_states(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT id, decision_state FROM chat_assignment_authorization_revisions ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Whether the fixture's push approval is still in force.
async fn push_approved(pool: &SqlitePool) -> bool {
    sqlx::query_scalar("SELECT revoked_at IS NULL FROM chat_push_approvals WHERE agent_id = ?")
        .bind(AGENT)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn channel_revision(pool: &SqlitePool) -> u64 {
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM chat_channels WHERE id = ?")
        .bind(channel_ids().1)
        .fetch_one(pool)
        .await
        .unwrap();
    u64::try_from(revision).unwrap()
}

/// The revision of the vault agent settings row.
async fn app_revision(pool: &SqlitePool) -> u64 {
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM chat_agent_app_settings")
        .fetch_one(pool)
        .await
        .unwrap();
    u64::try_from(revision).unwrap()
}

/// The scopes agent access is set at.
#[derive(Clone, Copy, Debug)]
enum Scope {
    App,
    Project,
    Channel,
}

/// Saves `access` at `scope` from the scope's first revision and commits it.
async fn save_access(pool: &SqlitePool, scope: Scope, access: ChatAgentAccess) {
    let now = now_timestamp().unwrap();
    let channel_revision = channel_revision(pool).await;
    let app_revision = app_revision(pool).await;
    let mut transaction = pool.begin().await.unwrap();
    match scope {
        Scope::App => {
            save_app_settings(
                &mut transaction,
                &UpdateChatAppAgentSettingsCommand {
                    access,
                    instructions: String::new(),
                    expected_revision: app_revision,
                },
                &now,
            )
            .await
            .unwrap();
        }
        Scope::Project => {
            save_project_settings(
                &mut transaction,
                &UpdateChatProjectAgentSettingsCommand {
                    project_id: PROJECT.to_string(),
                    access: Some(access),
                    instructions: String::new(),
                    expected_revision: 0,
                },
                &now,
            )
            .await
            .unwrap();
        }
        Scope::Channel => {
            save_channel_settings(
                &mut transaction,
                &UpdateChatChannelAgentSettingsCommand {
                    channel_id: ChatChannelId::new(channel_ids().1).unwrap(),
                    access: Some(access),
                    instructions: String::new(),
                    expected_revision: channel_revision,
                },
                &now,
            )
            .await
            .unwrap();
        }
    }
    transaction.commit().await.unwrap();
}

fn all_states(state: &str) -> Vec<(String, String)> {
    vec![
        ("authorization:queued".to_string(), state.to_string()),
        ("authorization:working".to_string(), state.to_string()),
    ]
}

#[test]
fn narrowing_at_each_scope_revokes_the_work_and_the_push_approvals_it_no_longer_covers() {
    tauri::async_runtime::block_on(async {
        for scope in [Scope::App, Scope::Project, Scope::Channel] {
            let pool = pool_with_folder_runs().await;
            approve_push(&pool).await;

            save_access(&pool, scope, ChatAgentAccess::Read).await;

            // The queued authorization holds only its folder target, which read still covers;
            // the working one executes in its folder, which read does not.
            assert_eq!(
                authorization_states(&pool).await,
                vec![
                    ("authorization:queued".to_string(), "allowed".to_string()),
                    ("authorization:working".to_string(), "revoked".to_string()),
                ],
                "{scope:?}"
            );
            assert!(!push_approved(&pool).await, "{scope:?}");
            let jobs: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM chat_access_revocation_jobs WHERE state = 'queued'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(jobs > 0, "{scope:?} queues the stop of the revoked runs");
        }
    });
}

#[test]
fn widening_at_each_scope_keeps_the_work_and_the_push_approvals() {
    tauri::async_runtime::block_on(async {
        for scope in [Scope::App, Scope::Project, Scope::Channel] {
            let pool = pool_with_folder_runs().await;
            save_access(&pool, Scope::App, ChatAgentAccess::Ask).await;
            approve_push(&pool).await;

            save_access(&pool, scope, ChatAgentAccess::Own).await;

            assert_eq!(
                authorization_states(&pool).await,
                all_states("allowed"),
                "{scope:?}"
            );
            assert!(push_approved(&pool).await, "{scope:?}");
        }
    });
}

#[test]
fn a_channel_that_sets_its_own_access_keeps_its_work_when_the_vault_narrows() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_folder_runs().await;
        approve_push(&pool).await;
        save_access(&pool, Scope::Channel, ChatAgentAccess::Ask).await;

        save_access(&pool, Scope::App, ChatAgentAccess::Talk).await;
        save_access(&pool, Scope::Project, ChatAgentAccess::Talk).await;

        assert_eq!(authorization_states(&pool).await, all_states("allowed"));
        assert!(push_approved(&pool).await);
    });
}

#[test]
fn saving_project_settings_twice_advances_the_revision_and_refuses_a_stale_save() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_folder_runs().await;
        let now = now_timestamp().unwrap();
        let request =
            |instructions: &str, expected_revision: u64| UpdateChatProjectAgentSettingsCommand {
                project_id: PROJECT.to_string(),
                access: Some(ChatAgentAccess::Ask),
                instructions: instructions.to_string(),
                expected_revision,
            };
        let mut connection = pool.acquire().await.unwrap();
        assert_eq!(
            read_project_agent_settings(&mut connection, PROJECT)
                .await
                .unwrap()
                .revision,
            0
        );

        let first = save_project_settings(&mut connection, &request("First", 0), &now)
            .await
            .unwrap();
        let second =
            save_project_settings(&mut connection, &request("Second", first.revision), &now)
                .await
                .unwrap();

        assert_eq!(first.revision, 1);
        assert_eq!(second.revision, 2);
        assert_eq!(second.instructions, "Second");
        assert_eq!(second.access, Some(ChatAgentAccess::Ask));
        assert_eq!(second.effective.source, ChatAgentAccessSource::Project);
        for stale in [0, first.revision] {
            let error = save_project_settings(&mut connection, &request("Stale", stale), &now)
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::StaleRevision, "revision {stale}");
        }
        assert_eq!(
            read_project_agent_settings(&mut connection, PROJECT)
                .await
                .unwrap()
                .instructions,
            "Second"
        );
    });
}

#[test]
fn own_is_not_offered_to_an_agent_whose_provider_cannot_confine_commands() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_folder_runs().await;
        save_access(&pool, Scope::Channel, ChatAgentAccess::Own).await;
        let unconfined = ProviderAuthoritySupport {
            confined_commands: false,
            ..full_support()
        };
        let mut connection = pool.acquire().await.unwrap();

        let offered = read_channel_settings(
            &mut connection,
            &channel_ids().0,
            &test_providers_with(full_support()),
        )
        .await
        .unwrap();
        let unoffered = read_channel_settings(
            &mut connection,
            &channel_ids().0,
            &test_providers_with(unconfined),
        )
        .await
        .unwrap();

        let agent = |read: &ChatChannelAgentSettingsRead| {
            read.agents
                .iter()
                .find(|agent| agent.agent_id.as_str() == AGENT)
                .cloned()
                .unwrap()
        };
        assert!(agent(&offered).own_offered);
        assert_eq!(
            agent(&offered).effective,
            Some(ChatEffectiveAgentAccess::Own)
        );
        assert_eq!(agent(&offered).downgrade_reason, None);
        let unoffered = agent(&unoffered);
        assert!(!unoffered.own_offered);
        assert!(unoffered.installed);
        assert_eq!(unoffered.requested, ChatAgentAccess::Own);
        assert_eq!(unoffered.effective, Some(ChatEffectiveAgentAccess::Ask));
        assert_eq!(
            unoffered.downgrade_reason,
            Some(ChatAccessDowngradeReason::CannotConfineCommands)
        );
    });
}
