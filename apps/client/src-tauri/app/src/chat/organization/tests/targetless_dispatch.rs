//! Dispatch and folder choice of work for a provider agent added to a channel the way the app does
//! it.

use super::super::provider_agents::ensure_provider_agents;
use super::*;
use ganbaru_chat::conversion::provider_agent_id;
use ganbaru_chat::project_defaults::{default_channel_ids, ensure_project_chat_defaults};

const PROJECT_ID: &str = "project-targetless";
const HOME_DEVICE: &str = "device:desk";

/// A vault root of its own for the managed working folder the project creates, distinct for every
/// test so tests running in parallel never share one.
fn empty_vault_root() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
        "ganbaru-targetless-dispatch-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

/// The id of the test family's provider agent.
fn agent_id() -> ChatParticipantId {
    ChatParticipantId::new(provider_agent_id(TEST_FAMILY).unwrap()).unwrap()
}

/// Ensures the test family's provider agent, adds it to `channel_id`, and sets the channel's own
/// access to `access`.
async fn add_channel_agent(pool: &SqlitePool, channel_id: &str, access: &str) {
    ensure_provider_agents(
        pool,
        &[(TEST_FAMILY.to_string(), "Basic agent".to_string())],
    )
    .await
    .unwrap();
    let conversation_id = default_channel_ids(PROJECT_ID).unwrap().0;
    assert_eq!(default_channel_ids(PROJECT_ID).unwrap().1, channel_id);
    let mut connection = pool.acquire().await.unwrap();
    super::super::access::add_agent_to_conversation(
        &mut connection,
        &agent_id(),
        &conversation_id,
        &now_timestamp().unwrap(),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE chat_channels SET agent_access = ? WHERE conversation_id = ?")
        .bind(access)
        .bind(&conversation_id)
        .execute(&mut *connection)
        .await
        .unwrap();
}

/// Posts a mention of the agent, under command and reference ids of its own, in `channel_id` from its home
/// device, in `reply_thread_id` when set, and in `execution_target` when set.
async fn mention_agent_in(
    pool: &SqlitePool,
    channel_id: &str,
    reply_thread_id: Option<ChatReplyThreadId>,
    execution_target: Option<ChatExecutionTarget>,
) -> ChatWorkAssignmentId {
    post_mention(
        pool,
        channel_id,
        reply_thread_id,
        execution_target,
        "@Basic agent Say \"hi\", don't do anything else.",
    )
    .await
}

/// Posts `text`, which starts with the agent's mention, like [`mention_agent_in`].
async fn post_mention(
    pool: &SqlitePool,
    channel_id: &str,
    reply_thread_id: Option<ChatReplyThreadId>,
    execution_target: Option<ChatExecutionTarget>,
    text: &str,
) -> ChatWorkAssignmentId {
    post_mention_of(
        pool,
        channel_id,
        reply_thread_id,
        execution_target,
        text,
        agent_id(),
    )
    .await
}

/// Posts `text`, which starts with a mention of `agent` labeled like the provider agent.
async fn post_mention_of(
    pool: &SqlitePool,
    channel_id: &str,
    reply_thread_id: Option<ChatReplyThreadId>,
    execution_target: Option<ChatExecutionTarget>,
    text: &str,
    agent: ChatParticipantId,
) -> ChatWorkAssignmentId {
    let mut request = message(text);
    static NEXT_POST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let post = NEXT_POST.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    request.client_command_id = ChatCommandId::new(format!("command:mention-{post}")).unwrap();
    request.channel_id = ChatChannelId::new(channel_id).unwrap();
    request.reply_thread_id = reply_thread_id;
    request.execution_target = execution_target;
    request.references = vec![participant_reference(
        &format!("reference:basic-agent-{post}"),
        agent,
        ChatParticipantKind::AiAgent,
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

/// Posts a top-level mention of the agent in `channel_id` from its home device.
async fn mention_agent(pool: &SqlitePool, channel_id: &str) -> ChatWorkAssignmentId {
    mention_agent_in(pool, channel_id, None, None).await
}

/// The folder and execution environment the work of `assignment_id` was authorized in.
async fn authorized_target(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> (Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT working_folder_id, execution_environment_id
         FROM chat_assignment_authorization_revisions WHERE assignment_id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn talk_work_of_a_channel_agent_is_conversation_only_and_reaches_its_launch() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let vault_root = empty_vault_root();
        let channel_id = create_project_channel(&pool, &vault_root).await;
        add_channel_agent(&pool, &channel_id, "talk").await;
        let assignment_id = mention_agent(&pool, &channel_id).await;

        assert_eq!(
            authorized_target(&pool, &assignment_id).await,
            (None, None),
            "talk work has no folder"
        );

        let claim = super::super::dispatch::claim_dispatch_job(
            &pool,
            &assignment_id,
            HOME_DEVICE,
            &test_providers(),
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

/// Creates an external working folder `id` in the project with its current-folder environment,
/// which this device has not located.
async fn create_external_folder(pool: &SqlitePool, id: &str) -> ProjectWorkingFolderId {
    let folder = ganbaru_chat::repository::workspaces::create_workspace(
        pool,
        &ganbaru_chat::workspace::CreateProjectWorkingFolderRequest {
            id: ProjectWorkingFolderId::new(id).unwrap(),
            project_id: PROJECT_ID.to_string(),
            display_name: id.to_string(),
        },
        &now_timestamp().unwrap(),
    )
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ganbaru_chat::project_defaults::ensure_working_folder_chat_defaults(
        &mut connection,
        folder.id.as_str(),
        &now_timestamp().unwrap(),
    )
    .await
    .unwrap();
    folder.id
}

/// The project's managed working folder the project created.
async fn project_folder(pool: &SqlitePool) -> ProjectWorkingFolderId {
    ganbaru_chat::repository::workspaces::resolve_primary_workspace(pool, PROJECT_ID)
        .await
        .unwrap()
        .expect("the project has a managed working folder")
        .id
}

/// The current folder execution environment of `working_folder_id`.
async fn current_folder_environment(
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatExecutionEnvironmentId {
    let id: String = sqlx::query_scalar(
        "SELECT id FROM chat_execution_environments
         WHERE working_folder_id = ? AND kind = 'current_folder' AND archived_at IS NULL",
    )
    .bind(working_folder_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    ChatExecutionEnvironmentId::new(id).unwrap()
}

/// A project channel with the primary folder and `folder:docs`, whose agent works at `access`.
async fn project_with_two_folders(
    access: &str,
) -> (
    SqlitePool,
    std::path::PathBuf,
    String,
    ProjectWorkingFolderId,
    ProjectWorkingFolderId,
) {
    let pool = migrated_pool().await;
    let vault_root = empty_vault_root();
    let channel_id = create_project_channel(&pool, &vault_root).await;
    let primary = project_folder(&pool).await;
    let docs = create_external_folder(&pool, "folder:docs").await;
    add_channel_agent(&pool, &channel_id, access).await;
    (pool, vault_root, channel_id, primary, docs)
}

#[test]
fn work_that_names_no_folder_runs_in_the_primary_folder_at_read_or_above() {
    tauri::async_runtime::block_on(async {
        for access in ["read", "ask", "own"] {
            let (pool, vault_root, channel_id, primary, _) = project_with_two_folders(access).await;
            let assignment_id = mention_agent(&pool, &channel_id).await;

            assert_eq!(
                authorized_target(&pool, &assignment_id).await.0.as_deref(),
                Some(primary.as_str()),
                "a channel at {access}"
            );
            let _ = std::fs::remove_dir_all(&vault_root);
        }
    });
}

#[test]
fn a_requested_folder_wins_over_the_primary_folder() {
    tauri::async_runtime::block_on(async {
        let (pool, vault_root, channel_id, _, docs) = project_with_two_folders("ask").await;
        let environment = current_folder_environment(&pool, &docs).await;
        let assignment_id = mention_agent_in(
            &pool,
            &channel_id,
            None,
            Some(ChatExecutionTarget::WorkingFolder {
                working_folder_id: docs.clone(),
                execution_environment_id: environment.clone(),
            }),
        )
        .await;

        assert_eq!(
            authorized_target(&pool, &assignment_id).await,
            (
                Some(docs.as_str().to_string()),
                Some(environment.as_str().to_string())
            )
        );
        let _ = std::fs::remove_dir_all(&vault_root);
    });
}

#[test]
fn a_new_mention_in_a_thread_keeps_the_thread_folder() {
    tauri::async_runtime::block_on(async {
        let (pool, vault_root, channel_id, _, docs) = project_with_two_folders("ask").await;
        let environment = current_folder_environment(&pool, &docs).await;
        let first = mention_agent_in(
            &pool,
            &channel_id,
            None,
            Some(ChatExecutionTarget::WorkingFolder {
                working_folder_id: docs.clone(),
                execution_environment_id: environment,
            }),
        )
        .await;
        let reply_thread_id: String =
            sqlx::query_scalar("SELECT reply_thread_id FROM chat_work_assignments WHERE id = ?")
                .bind(first.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'completed', settled_at = updated_at, revision = revision + 1
             WHERE id = ?",
        )
        .bind(first.as_str())
        .execute(&pool)
        .await
        .unwrap();

        let second = mention_agent_in(
            &pool,
            &channel_id,
            Some(ChatReplyThreadId::new(reply_thread_id).unwrap()),
            None,
        )
        .await;

        assert_ne!(second, first);
        assert_eq!(
            authorized_target(&pool, &second).await.0.as_deref(),
            Some(docs.as_str())
        );
        let _ = std::fs::remove_dir_all(&vault_root);
    });
}

/// The model and instructions `assignment_id` froze when it was created.
async fn frozen_settings(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> (ChatModelSelection, String) {
    let (data, instructions): (String, String) = sqlx::query_as(
        "SELECT model_selection_data, instructions FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    (serde_json::from_str(&data).unwrap(), instructions)
}

/// Settles `assignment_id` as completed and returns its reply thread.
async fn complete(pool: &SqlitePool, assignment_id: &ChatWorkAssignmentId) -> ChatReplyThreadId {
    sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'completed', settled_at = updated_at, revision = revision + 1
         WHERE id = ?",
    )
    .bind(assignment_id.as_str())
    .execute(pool)
    .await
    .unwrap();
    let reply_thread_id: String =
        sqlx::query_scalar("SELECT reply_thread_id FROM chat_work_assignments WHERE id = ?")
            .bind(assignment_id.as_str())
            .fetch_one(pool)
            .await
            .unwrap();
    ChatReplyThreadId::new(reply_thread_id).unwrap()
}

/// Sets the channel's agent instructions and its model override for the agent.
async fn configure_channel(pool: &SqlitePool, instructions: &str, model: Option<&str>) {
    let conversation_id = default_channel_ids(PROJECT_ID).unwrap().0;
    sqlx::query("UPDATE chat_channels SET agent_instructions = ? WHERE conversation_id = ?")
        .bind(instructions)
        .bind(&conversation_id)
        .execute(pool)
        .await
        .unwrap();
    let selection = model.map(|model| serde_json::to_string(&named_model(model)).unwrap());
    sqlx::query(
        "UPDATE chat_agent_channel_memberships
         SET model_selection_schema_version = CASE WHEN ?1 IS NULL THEN NULL ELSE 1 END,
             model_selection_data = ?1
         WHERE conversation_id = ?2 AND agent_id = ?3",
    )
    .bind(selection)
    .bind(&conversation_id)
    .bind(agent_id().as_str())
    .execute(pool)
    .await
    .unwrap();
}

fn named_model(model: &str) -> ChatModelSelection {
    ChatModelSelection {
        provider_managed_model: false,
        model_id: Some(ModelId::new(model).unwrap()),
        model_options: Vec::new(),
    }
}

#[test]
fn new_work_freezes_the_channel_override_else_the_provider_default_with_the_instructions() {
    tauri::async_runtime::block_on(async {
        let (pool, vault_root, channel_id, _, _) = project_with_two_folders("ask").await;
        let default_work = mention_agent(&pool, &channel_id).await;
        configure_channel(&pool, "Answer in Spanish.", Some("override-model")).await;

        let override_work = mention_agent(&pool, &channel_id).await;

        assert_eq!(
            frozen_settings(&pool, &default_work).await.0,
            ChatModelSelection::provider_default()
        );
        let (model, instructions) = frozen_settings(&pool, &override_work).await;
        assert_eq!(model, named_model("override-model"));
        assert!(instructions.contains("Answer in Spanish."));
        let _ = std::fs::remove_dir_all(&vault_root);
    });
}

#[test]
fn a_model_command_wins_and_a_continuation_keeps_what_its_predecessor_froze() {
    tauri::async_runtime::block_on(async {
        let (pool, vault_root, channel_id, _, _) = project_with_two_folders("ask").await;
        configure_channel(&pool, "Answer in Spanish.", Some("override-model")).await;
        let first = post_mention(
            &pool,
            &channel_id,
            None,
            None,
            "@Basic agent /model command-model Say hi",
        )
        .await;
        let reply_thread_id = complete(&pool, &first).await;
        configure_channel(&pool, "Answer in French.", Some("later-model")).await;

        let continued =
            mention_agent_in(&pool, &channel_id, Some(reply_thread_id.clone()), None).await;
        complete(&pool, &continued).await;
        let switched = post_mention(
            &pool,
            &channel_id,
            Some(reply_thread_id),
            None,
            "@Basic agent /model switched-model Go on",
        )
        .await;

        let (model, instructions) = frozen_settings(&pool, &first).await;
        assert_eq!(model, named_model("command-model"));
        assert!(instructions.contains("Answer in Spanish."));
        assert_eq!(
            frozen_settings(&pool, &continued).await,
            (model, instructions.clone())
        );
        assert_eq!(
            frozen_settings(&pool, &switched).await,
            (named_model("switched-model"), instructions)
        );
        let _ = std::fs::remove_dir_all(&vault_root);
    });
}

#[test]
fn talk_work_discloses_no_folder_and_no_path_in_its_context() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let vault_root = empty_vault_root();
        let channel_id = create_project_channel(&pool, &vault_root).await;
        add_channel_agent(&pool, &channel_id, "talk").await;
        let assignment_id = mention_agent(&pool, &channel_id).await;

        let folder_sources: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_assignment_authorized_folder_sources source
             JOIN chat_assignment_authorization_revisions authorization
               ON authorization.id = source.authorization_revision_id
             WHERE authorization.assignment_id = ?",
        )
        .bind(assignment_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        let context: String = sqlx::query_scalar(
            "SELECT serialized_text FROM chat_assignment_context_packages WHERE assignment_id = ?",
        )
        .bind(assignment_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(folder_sources, 0);
        assert!(context.contains("Say \\\"hi\\\""), "{context}");
        assert!(
            !context.contains(vault_root.to_string_lossy().as_ref()),
            "{context}"
        );
        let _ = std::fs::remove_dir_all(&vault_root);
    });
}

#[test]
fn a_named_agent_keeps_its_own_model_over_the_channel_override() {
    tauri::async_runtime::block_on(async {
        let (pool, vault_root, channel_id, _, _) = project_with_two_folders("ask").await;
        let named = super::super::provider_agents::create_custom_agent(
            &pool,
            &ProviderFamilyId::new(TEST_FAMILY).unwrap(),
            "Reviewer",
            &ChatAgentPolicyInput {
                provider_managed_model: false,
                model_id: Some(ModelId::new("named-model").unwrap()),
                model_options: Vec::new(),
                provider_options: VersionedJson {
                    schema_version: 1,
                    value: json!({}),
                },
                interaction_mode: InteractionMode::Build,
            },
        )
        .await
        .unwrap();
        let conversation_id = default_channel_ids(PROJECT_ID).unwrap().0;
        let mut connection = pool.acquire().await.unwrap();
        super::super::access::add_agent_to_conversation(
            &mut connection,
            &named,
            &conversation_id,
            &now_timestamp().unwrap(),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_agent_channel_memberships
             SET model_selection_schema_version = 1, model_selection_data = ?1
             WHERE conversation_id = ?2",
        )
        .bind(serde_json::to_string(&named_model("override-model")).unwrap())
        .bind(&conversation_id)
        .execute(&mut *connection)
        .await
        .unwrap();
        drop(connection);

        let named_work = post_mention_of(
            &pool,
            &channel_id,
            None,
            None,
            "@Basic agent review this",
            named,
        )
        .await;
        let provider_work = mention_agent(&pool, &channel_id).await;

        assert_eq!(
            frozen_settings(&pool, &named_work).await.0,
            named_model("named-model")
        );
        assert_eq!(
            frozen_settings(&pool, &provider_work).await.0,
            named_model("override-model")
        );
        let _ = std::fs::remove_dir_all(&vault_root);
    });
}
