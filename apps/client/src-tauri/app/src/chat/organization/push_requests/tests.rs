use super::*;
use crate::chat::revocation::tests::pool_with_folder_runs;
use ganbaru_chat::coordination::push_requests::PushRequestState;

const ITEM: &str = "item:push-request";

fn request(working_folder_id: &str, execution_environment_id: Option<&str>) -> PushRequest {
    PushRequest {
        working_folder_id: working_folder_id.to_string(),
        execution_environment_id: execution_environment_id.map(str::to_string),
        target: GitPushTarget {
            local_branch: "topic".to_string(),
            remote: "origin".to_string(),
            destination_branch: "topic".to_string(),
            push_url: "git@example.invalid:org/repo.git".to_string(),
            object_id: "a".repeat(40),
        },
        git_config_digest: "b".repeat(64),
        commit: None,
    }
}

fn stored(request: PushRequest, authorization_revision_id: Option<&str>) -> StoredPushRequest {
    StoredPushRequest {
        request,
        summary: "Push the docs fix".to_string(),
        state: PushRequestState::Open,
        assignment_id: "assignment:folders".to_string(),
        authorization_revision_id: authorization_revision_id.map(str::to_string),
        channel_id: "channel:push".to_string(),
        reply_thread_id: "reply:folders".to_string(),
        agent_id: "participant:folder-agent".to_string(),
        agent_name: Some("Folder agent".to_string()),
    }
}

/// Raises the fixture's grant on `working_folder_id` to publish. Sources are immutable once
/// recorded, so this test pool drops that guard to rewrite the fixture in place.
async fn grant_publish(pool: &SqlitePool, working_folder_id: &str) {
    sqlx::query("DROP TRIGGER IF EXISTS chat_authorized_folder_source_immutable_update")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE chat_assignment_authorized_folder_sources SET capability = 'publish'
         WHERE authorization_revision_id = 'authorization:working' AND working_folder_id = ?",
    )
    .bind(working_folder_id)
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn the_confirmation_reply_mentions_the_agent_under_ids_derived_from_the_request() {
    let stored = stored(request("workspace-1", None), Some("authorization:working"));
    let post = confirmation_post(&stored, ITEM, "  Pushed topic to origin. \n").unwrap();
    assert_eq!(
        post.normalized_markdown,
        "@Folder agent Pushed topic to origin."
    );
    assert_eq!(post.reply_thread_id.unwrap().as_str(), "reply:folders");
    assert_eq!(post.channel_id.as_str(), "channel:push");
    let [
        ChatMessageReference::Participant {
            metadata,
            participant_id,
            participant_kind,
        },
    ] = post.references.as_slice()
    else {
        panic!("one participant mention: {:?}", post.references);
    };
    assert_eq!(participant_id.as_str(), "participant:folder-agent");
    assert_eq!(*participant_kind, ChatParticipantKind::AiAgent);
    assert_eq!(
        (metadata.start_offset, metadata.end_offset),
        (0, "@Folder agent".len() as u64)
    );
    assert_eq!(
        metadata.reference_id.as_str(),
        push_confirmation_id(PUSH_CONFIRMATION_REFERENCE_PREFIX, ITEM).unwrap()
    );

    let again = confirmation_post(&stored, ITEM, "Other text").unwrap();
    assert_eq!(again.client_command_id, post.client_command_id);
    let other = confirmation_post(&stored, "item:other", "Pushed").unwrap();
    assert_ne!(other.client_command_id, post.client_command_id);
}

#[test]
fn a_confirmation_needs_text_and_a_agent_that_still_exists() {
    let mut stored = stored(request("workspace-1", None), Some("authorization:working"));
    assert_eq!(
        confirmation_post(&stored, ITEM, " \n").unwrap_err().code,
        ChatErrorCode::Validation
    );
    stored.agent_name = None;
    assert_eq!(
        confirmation_post(&stored, ITEM, "Pushed").unwrap_err().code,
        ChatErrorCode::Conflict
    );
}

#[test]
fn only_an_identical_target_and_configuration_push() {
    let recorded = request("workspace-1", None);
    let digest = recorded.git_config_digest.clone();
    assert_eq!(
        require_unchanged(&recorded, Some(&recorded.target), Some(&digest)),
        Ok(())
    );
    let changes: [fn(&mut GitPushTarget); 5] = [
        |target| target.object_id = "c".repeat(40),
        |target| target.push_url = "git@example.invalid:other/repo.git".to_string(),
        |target| target.destination_branch = "main".to_string(),
        |target| target.local_branch = "other".to_string(),
        |target| target.remote = "upstream".to_string(),
    ];
    for change in changes {
        let mut fresh = recorded.target.clone();
        change(&mut fresh);
        assert_eq!(
            require_unchanged(&recorded, Some(&fresh), Some(&digest)),
            Err(PushRequestRefusal::TargetChanged),
            "{fresh:?}"
        );
    }
    assert_eq!(
        require_unchanged(&recorded, None, Some(&digest)),
        Err(PushRequestRefusal::TargetChanged)
    );
    assert_eq!(
        require_unchanged(&recorded, Some(&recorded.target), Some(&"c".repeat(64))),
        Err(PushRequestRefusal::TargetChanged)
    );
    assert_eq!(
        require_unchanged(&recorded, Some(&recorded.target), None),
        Err(PushRequestRefusal::TargetChanged)
    );
}

#[tokio::test]
async fn the_publish_source_comes_from_the_asking_runs_authorization() {
    let pool = pool_with_folder_runs().await;
    let execution_target = stored(
        request("workspace-1", Some("environment:workspace-1")),
        Some("authorization:working"),
    );
    assert_eq!(
        publish_source(&pool, &execution_target)
            .await
            .unwrap_err()
            .code,
        ChatErrorCode::Permission
    );

    grant_publish(&pool, "workspace-1").await;
    let source = publish_source(&pool, &execution_target).await.unwrap();
    assert_eq!(source.root_handle, format!("{:032}", 1));
    assert_eq!(source.working_folder_id.as_str(), "workspace-1");
    assert_eq!(source.capability, ChatFolderCapability::Publish);
    assert!(source.is_execution_target);
    assert_eq!(
        source.runtime_approval_policy,
        ChatRuntimeApprovalPolicy::Ask
    );

    grant_publish(&pool, "workspace-2").await;
    let reference = stored(request("workspace-2", None), Some("authorization:working"));
    assert!(
        !publish_source(&pool, &reference)
            .await
            .unwrap()
            .is_execution_target
    );
    let reference_in_environment = stored(
        request("workspace-2", Some("environment:workspace-1")),
        Some("authorization:working"),
    );
    assert_eq!(
        publish_source(&pool, &reference_in_environment)
            .await
            .unwrap_err()
            .code,
        ChatErrorCode::Permission
    );

    let other_revision = stored(request("workspace-1", None), Some("authorization:queued"));
    assert_eq!(
        publish_source(&pool, &other_revision)
            .await
            .unwrap_err()
            .code,
        ChatErrorCode::Permission
    );
    let run_gone = stored(request("workspace-1", None), None);
    assert_eq!(
        PushRequestRefusal::from_error(&publish_source(&pool, &run_gone).await.unwrap_err()),
        Some(PushRequestRefusal::Closed)
    );
}

/// Records the fixture agent's current policy with `safety_mode`, which standing approvals
/// compare against.
async fn set_agent_safety_mode(pool: &SqlitePool, safety_mode: &str) {
    sqlx::query(
        "UPDATE chat_agents SET current_policy_revision_id = 'policy:folders',
             current_policy_safety_mode = ?
         WHERE participant_id = 'participant:folder-agent'",
    )
    .bind(safety_mode)
    .execute(pool)
    .await
    .unwrap();
}

/// A pool whose fixture run may publish from `workspace-1` under the ask policy, with the
/// agent's safety mode recorded, and the open request and publish source of that run.
async fn standing_fixture() -> (SqlitePool, StoredPushRequest, InternalMcpFolderSource) {
    let pool = pool_with_folder_runs().await;
    grant_publish(&pool, "workspace-1").await;
    set_agent_safety_mode(&pool, "ask_for_approval").await;
    let stored = stored(request("workspace-1", None), Some("authorization:working"));
    let source = publish_source(&pool, &stored).await.unwrap();
    (pool, stored, source)
}

#[tokio::test]
async fn a_standing_push_needs_a_matching_approval_or_a_full_access_turn() {
    let (pool, stored, source) = standing_fixture().await;
    assert_eq!(
        standing_permission(&pool, &stored, &source).await.unwrap(),
        None
    );

    let full_access = InternalMcpFolderSource {
        runtime_approval_policy: ChatRuntimeApprovalPolicy::Unattended,
        ..source.clone()
    };
    assert_eq!(
        standing_permission(&pool, &stored, &full_access)
            .await
            .unwrap(),
        Some(StandingPermission::FullAccess)
    );

    remember_push_approval(&pool, &stored, &source)
        .await
        .unwrap();
    assert_eq!(
        standing_permission(&pool, &stored, &source).await.unwrap(),
        Some(StandingPermission::Approval)
    );

    let mut other_branch = stored.clone();
    other_branch.request.target.destination_branch = "main".to_string();
    let mut other_url = stored.clone();
    other_url.request.target.push_url = "git@example.invalid:org/fork.git".to_string();
    let mut other_folder = stored.clone();
    other_folder.request.working_folder_id = "workspace-2".to_string();
    let mut other_agent = stored.clone();
    other_agent.agent_id = "participant:local-owner".to_string();
    let mut removed_agent = stored.clone();
    removed_agent.agent_name = None;
    for (case, request) in [
        ("branch", &other_branch),
        ("push URL", &other_url),
        ("folder", &other_folder),
        ("agent", &other_agent),
        ("removed agent", &removed_agent),
    ] {
        assert_eq!(
            standing_permission(&pool, request, &source).await.unwrap(),
            None,
            "another {case} asks again"
        );
    }
    assert_eq!(
        standing_permission(&pool, &removed_agent, &full_access)
            .await
            .unwrap(),
        None,
        "a removed agent never pushes on its own"
    );
    let auto_approve = InternalMcpFolderSource {
        runtime_approval_policy: ChatRuntimeApprovalPolicy::AutoApprove,
        ..source.clone()
    };
    assert_eq!(
        standing_permission(&pool, &stored, &auto_approve)
            .await
            .unwrap(),
        None,
        "another approval policy asks again"
    );

    set_agent_safety_mode(&pool, "approve_for_me").await;
    assert_eq!(
        standing_permission(&pool, &stored, &source).await.unwrap(),
        None,
        "a changed safety mode drops the approval"
    );
}

#[tokio::test]
async fn remembering_a_push_approves_again_under_one_key_by_the_local_person() {
    let (pool, stored, source) = standing_fixture().await;
    remember_push_approval(&pool, &stored, &source)
        .await
        .unwrap();
    sqlx::query("UPDATE chat_push_approvals SET revoked_at = '2026-10-20T12:30:00.000Z', revision = revision + 1")
        .execute(&pool)
        .await
        .unwrap();
    remember_push_approval(&pool, &stored, &source)
        .await
        .unwrap();

    let rows: Vec<(String, String, String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT sync_key, runtime_approval_policy, safety_mode, approved_by_participant_id,
                revoked_at, revision
         FROM chat_push_approvals",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let expected_key = derived_id(
        PUSH_APPROVAL_KEY_PREFIX,
        &[
            "participant:folder-agent",
            "workspace-1",
            "git@example.invalid:org/repo.git",
            "topic",
        ],
    )
    .unwrap();
    assert_eq!(
        rows,
        [(
            expected_key,
            "ask".to_string(),
            "ask_for_approval".to_string(),
            "participant:local-owner".to_string(),
            None,
            3
        )]
    );
}

#[tokio::test]
async fn a_agent_without_a_current_policy_gets_no_approval() {
    let pool = pool_with_folder_runs().await;
    grant_publish(&pool, "workspace-1").await;
    let stored = stored(request("workspace-1", None), Some("authorization:working"));
    let source = publish_source(&pool, &stored).await.unwrap();
    remember_push_approval(&pool, &stored, &source)
        .await
        .unwrap();
    let approvals: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_push_approvals")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(approvals, 0);
}
