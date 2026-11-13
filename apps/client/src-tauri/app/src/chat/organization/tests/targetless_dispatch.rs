//! Dispatch of work for a teammate created and added to a channel the way the app does it.

use super::*;
use ganbaru_chat::coordination::contracts::{
    ChatTeammateChannelAccessInput, ChatTeammatePolicyInput, CreateChatTeammateCommand,
    ReplaceChatTeammateAccessCommand,
};
use ganbaru_chat::project_defaults::{default_channel_ids, ensure_project_chat_defaults};

const PROJECT_ID: &str = "project-targetless";
const TEAMMATE_ID: &str = "participant:basic-agent";
const HOME_DEVICE: &str = "device:desk";

/// A vault root of its own for the managed working folder the project creates.
fn empty_vault_root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ganbaru-targetless-dispatch-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// Creates the project and its Chat defaults, returning the id of its default channel.
async fn create_project_channel(pool: &SqlitePool, vault_root: &std::path::Path) -> String {
    let project: ganbaru_projects::ProjectCreate = serde_json::from_value(json!({
        "id": PROJECT_ID,
        "groupId": null,
        "name": "Targetless",
        "icon": "lucide:folder",
        "color": null,
        "defaultEventName": null,
        "defaultEventTimeMode": "timed",
        "defaultEventDurationMinutes": null,
        "defaultPomodoroMode": "none",
        "defaultPomodoroPresetKey": null,
        "defaultPomodoroFocusMinutes": null,
        "defaultPomodoroShortBreakMinutes": null,
        "defaultPomodoroLongBreakMinutes": null,
        "defaultPomodoroLongBreakAfterFocusCount": null,
        "defaultIdleSettingsSource": "global",
        "defaultIdlePauseEnabled": true,
        "defaultIdleThresholdMinutes": 5
    }))
    .unwrap();
    ganbaru_projects::project_commands::create_project(pool, vault_root, project)
        .await
        .unwrap();
    let mut transaction = pool.begin().await.unwrap();
    ensure_project_chat_defaults(&mut transaction, PROJECT_ID, &now_timestamp().unwrap())
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    default_channel_ids(PROJECT_ID).unwrap().1
}

/// Creates the teammate as the teammate editor does, then gives it the channel access a new
/// channel member gets: the conversation-only profile, responding without history or folders.
async fn create_channel_teammate(pool: &SqlitePool, channel_id: &str) {
    super::super::create_teammate(
        pool,
        &CreateChatTeammateCommand {
            teammate_id: ChatParticipantId::new(TEAMMATE_ID).unwrap(),
            display_name: "Basic agent".to_string(),
            avatar: VersionedJson {
                schema_version: 1,
                value: json!({ "kind": "initials" }),
            },
            role: "General support".to_string(),
            instructions: String::new(),
            policy: ChatTeammatePolicyInput {
                provider_instance_id: ProviderInstanceId::new("provider:claude").unwrap(),
                safety_mode: ChatApprovalPolicy::AskForApproval,
                provider_managed_model: true,
                model_id: None,
                model_options: Vec::new(),
                effort: None,
                speed: None,
                provider_options: VersionedJson {
                    schema_version: 1,
                    value: json!({}),
                },
                default_assignment_token_limit: None,
            },
        },
        HOME_DEVICE,
    )
    .await
    .unwrap();
    let (profile_id, profile_revision_id): (String, String) = sqlx::query_as(
        "SELECT profile.id, revision.id
         FROM chat_access_profiles profile
         JOIN chat_access_profile_effective_revisions revision
           ON revision.access_profile_id = profile.id
         WHERE profile.builtin_key = 'conversation_only'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let teammate_id = ChatParticipantId::new(TEAMMATE_ID).unwrap();
    let access = super::super::access::read_teammate_access(pool, &teammate_id)
        .await
        .unwrap();
    super::super::access::write_teammate_access(
        pool,
        &ReplaceChatTeammateAccessCommand {
            teammate_id,
            expected_access_revision: access.access_revision,
            channels: vec![ChatTeammateChannelAccessInput {
                channel_id: ChatChannelId::new(channel_id).unwrap(),
                access_profile_id: ChatAccessProfileId::new(profile_id).unwrap(),
                access_profile_revision_id: ChatAccessProfileRevisionId::new(profile_revision_id)
                    .unwrap(),
                capabilities: ChatChannelCapabilities {
                    read_history: false,
                    participate: true,
                },
                history_boundary: ChatHistoryBoundary::Entire,
                runtime_approval_override: None,
                scratch_runtime_approval_override: None,
                folder_grants: Vec::new(),
            }],
            teammate_profile: None,
            policy: None,
        },
        None,
    )
    .await
    .unwrap();
}

/// Posts a top-level mention of the teammate in `channel_id` from its home device.
async fn mention_teammate(pool: &SqlitePool, channel_id: &str) -> ChatWorkAssignmentId {
    let mut request = message("@Basic agent Say \"hi\", don't do anything else.");
    request.channel_id = ChatChannelId::new(channel_id).unwrap();
    request.references = vec![participant_reference(
        "reference:basic-agent",
        ChatParticipantId::new(TEAMMATE_ID).unwrap(),
        ChatParticipantKind::AiTeammate,
        "Basic agent",
        "@Basic agent",
        0,
        12,
    )];
    let writers = chat_writers_of(&[HOME_DEVICE]);
    let posted = persist_post(
        pool,
        &request,
        None,
        PostingDevice::this_device(HOME_DEVICE, &writers),
    )
    .await
    .unwrap();
    posted.assignment_write.assignment_id.unwrap()
}

#[test]
fn conversation_only_work_of_a_new_channel_teammate_reaches_its_launch() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let vault_root = empty_vault_root();
        let channel_id = create_project_channel(&pool, &vault_root).await;
        create_channel_teammate(&pool, &channel_id).await;
        let assignment_id = mention_teammate(&pool, &channel_id).await;

        let target: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT working_folder_id, execution_environment_id
             FROM chat_assignment_authorization_revisions WHERE assignment_id = ?",
        )
        .bind(assignment_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(target, (None, None), "the work is conversation-only");

        let claim = super::super::dispatch::claim_dispatch_job(
            &pool,
            &assignment_id,
            HOME_DEVICE,
            &now_timestamp().unwrap(),
        )
        .await
        .unwrap()
        .expect("the post queued a dispatch this device claims");
        let row = super::super::dispatch::read_dispatch_row(&pool, &assignment_id)
            .await
            .unwrap();
        assert_eq!(
            row.try_get::<String, _>("claim_token").unwrap(),
            claim.token
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("scratch_generation_id")
                .unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(&vault_root);
    });
}
