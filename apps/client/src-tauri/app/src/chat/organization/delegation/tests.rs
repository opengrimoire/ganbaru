use super::*;
use crate::chat::revocation::tests::pool_with_folder_runs;
use crate::chat::revocation::{ActiveRunSelector, revoke_active_run_authorizations};
use std::collections::BTreeSet;

const NOW: &str = "2026-10-20T12:00:00.000Z";
const DEVICE: &str = "device:delegation";
const PARENT: &str = "assignment:folders";
const PARENT_AGENT: &str = "participant:folder-agent";
const HELPER: &str = "participant:helper";

/// The revocation fixture with a second agent, `Helper`, in the channel. The channel access lets
/// Helper edit `workspace-2`, which the parent only reads, so a delegated child shows the ceiling
/// narrowing it. The parent's queued follow-up holds the same folders as its working run.
async fn pool_with_helper() -> SqlitePool {
    let pool = pool_with_folder_runs().await;
    let (conversation, _) =
        ganbaru_chat::project_defaults::default_channel_ids("project-chat").unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('{HELPER}', 'ai_agent', 'Helper', '{NOW}', '{NOW}');
         INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
         VALUES ('{HELPER}', 'codex', '{NOW}', '{NOW}');
         INSERT INTO chat_agent_policy_revisions
            (id, agent_id, model_selection_data, created_at)
         VALUES ('policy:helper', '{HELPER}', '{{}}', '{NOW}');
         INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, sync_key, membership_role, created_at,
             updated_at)
         VALUES ('{conversation}', '{HELPER}', 'membership:helper', 'member', '{NOW}', '{NOW}');
         INSERT INTO chat_agent_channel_memberships
            (conversation_id, agent_id, sync_key, created_at, updated_at)
         VALUES ('{conversation}', '{HELPER}', 'channel-membership:helper', '{NOW}', '{NOW}');
         INSERT INTO chat_assignment_authorized_folder_sources
            (authorization_revision_id, root_handle, working_folder_id, capability,
             is_execution_target, created_at, resolved_runtime_approval_policy)
         VALUES
            ('authorization:queued', printf('%032d', 3), 'workspace-1', 'execute', 1, '{NOW}', 'ask'),
            ('authorization:queued', printf('%032d', 4), 'workspace-2', 'read', 0, '{NOW}', 'ask');"
    )))
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn conversation_id() -> ChatConversationId {
    let (conversation, _) =
        ganbaru_chat::project_defaults::default_channel_ids("project-chat").unwrap();
    ChatConversationId::new(conversation).unwrap()
}

fn folder(id: &str, capability: ChatFolderCapability) -> DelegatedFolder {
    DelegatedFolder {
        working_folder_id: ProjectWorkingFolderId::new(id).unwrap(),
        capability,
    }
}

fn parent_folders() -> Vec<DelegatedFolder> {
    vec![
        folder("workspace-1", ChatFolderCapability::Execute),
        folder("workspace-2", ChatFolderCapability::Read),
    ]
}

/// The ids a delegation from `parent` under `authorization` uses.
struct Caller {
    parent: ChatWorkAssignmentId,
    authorization: ChatAuthorizationRevisionId,
}

impl Caller {
    fn root() -> Self {
        Self {
            parent: ChatWorkAssignmentId::new(PARENT).unwrap(),
            authorization: ChatAuthorizationRevisionId::new("authorization:working").unwrap(),
        }
    }
}

async fn delegate(
    pool: &SqlitePool,
    caller: &Caller,
    agent: &str,
    folders: &[DelegatedFolder],
    client_request_id: &str,
) -> Result<CreatedChild, DelegationRefusal> {
    delegate_with(
        pool,
        caller,
        agent,
        folders,
        client_request_id,
        "Check the build",
    )
    .await
}

async fn delegate_with(
    pool: &SqlitePool,
    caller: &Caller,
    agent: &str,
    folders: &[DelegatedFolder],
    client_request_id: &str,
    objective: &str,
) -> Result<CreatedChild, DelegationRefusal> {
    let writers = crate::chat::organization::tests::chat_writers_of(&[DEVICE]);
    let conversation = conversation_id();
    let agent = ChatParticipantId::new(agent).unwrap();
    persist_delegation(
        pool,
        DEVICE,
        &writers,
        DelegationRequest {
            parent_assignment_id: &caller.parent,
            parent_authorization_revision_id: &caller.authorization,
            conversation_id: &conversation,
            agent_id: &agent,
            objective,
            expected_result: "A short report",
            folders,
            client_request_id,
        },
    )
    .await
    .unwrap()
}

/// Marks `child` working, as a started run would, and returns it as a caller.
async fn start_working(pool: &SqlitePool, child: &ChatWorkAssignmentId) -> Caller {
    sqlx::query("UPDATE chat_work_assignments SET state = 'working' WHERE id = ?")
        .bind(child.as_str())
        .execute(pool)
        .await
        .unwrap();
    let authorization: String = sqlx::query_scalar(
        "SELECT id FROM chat_assignment_authorization_revisions WHERE assignment_id = ?",
    )
    .bind(child.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    Caller {
        parent: child.clone(),
        authorization: ChatAuthorizationRevisionId::new(authorization).unwrap(),
    }
}

async fn child_folder_sources(
    pool: &SqlitePool,
    child: &ChatWorkAssignmentId,
) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT source.working_folder_id, source.capability
         FROM chat_assignment_authorized_folder_sources source
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = source.authorization_revision_id
         WHERE authorization.assignment_id = ?
         ORDER BY source.working_folder_id",
    )
    .bind(child.as_str())
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn assignment_state(pool: &SqlitePool, id: &ChatWorkAssignmentId) -> (String, String) {
    sqlx::query_as("SELECT state, state_reason FROM chat_work_assignments WHERE id = ?")
        .bind(id.as_str())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn revoke_parent(pool: &SqlitePool, reason: &str) {
    sqlx::query(
        "UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', reason = ?, revoked_at = ?
         WHERE assignment_id = ? AND decision_state = 'allowed'",
    )
    .bind(reason)
    .bind(NOW)
    .bind(PARENT)
    .execute(pool)
    .await
    .unwrap();
}

async fn claim(pool: &SqlitePool, child: &ChatWorkAssignmentId) -> bool {
    super::super::dispatch::claim_dispatch_job(
        pool,
        child,
        DEVICE,
        &crate::chat::organization::tests::test_providers(),
        &UtcTimestamp::new("2030-01-01T00:00:00.000Z").unwrap(),
    )
    .await
    .unwrap()
    .is_some()
}

#[tokio::test]
async fn delegation_creates_a_queued_child_narrowed_to_the_parent_ceiling() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    assert_eq!(created.child.state, "queued");
    assert!(created.starts_here);
    let child = &created.child.assignment_id;
    let row = sqlx::query(
        "SELECT assignment.parent_assignment_id, assignment.delegation_depth,
                assignment.delegation_folder_ceiling, assignment.reply_thread_id,
                message.author_participant_id, target.participant_id AS invoked_agent_id,
                revision.normalized_markdown
         FROM chat_work_assignments assignment
         JOIN chat_communication_messages message
           ON message.item_id = assignment.triggering_message_item_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         JOIN chat_message_references reference
           ON reference.message_revision_id = revision.id
          AND reference.reference_kind = 'participant'
         JOIN chat_participant_reference_targets target ON target.reference_id = reference.id
         WHERE assignment.id = ?",
    )
    .bind(child.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("parent_assignment_id"), PARENT);
    assert_eq!(row.get::<i64, _>("delegation_depth"), 1);
    assert_eq!(
        row.get::<String, _>("delegation_folder_ceiling"),
        r#"[{"workingFolderId":"workspace-1","capability":"execute"},{"workingFolderId":"workspace-2","capability":"read"}]"#
    );
    assert_eq!(
        row.get::<String, _>("reply_thread_id"),
        created.child.reply_thread_id.as_str()
    );
    assert_eq!(row.get::<String, _>("author_participant_id"), PARENT_AGENT);
    assert_eq!(row.get::<String, _>("invoked_agent_id"), HELPER);
    assert_eq!(
        row.get::<String, _>("normalized_markdown"),
        "@Helper Check the build\n\nExpected result: A short report"
    );
    assert_eq!(
        child_folder_sources(&pool, child).await,
        vec![
            ("workspace-1".to_owned(), "execute".to_owned()),
            ("workspace-2".to_owned(), "read".to_owned()),
        ],
        "Helper's edit grant on workspace-2 and its workspace-3 grant stay outside the ceiling"
    );
    let (requester, approval): (String, String) = sqlx::query_as(
        "SELECT requester_participant_id, resolved_runtime_approval_policy
         FROM chat_assignment_authorization_revisions WHERE assignment_id = ?",
    )
    .bind(child.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(requester, "participant:local-owner");
    assert_eq!(approval, "ask");
    assert!(
        claim(&pool, child).await,
        "a child within its ceiling starts"
    );
}

#[tokio::test]
async fn delegation_retry_returns_the_same_child_and_changed_input_conflicts() {
    let pool = pool_with_helper().await;
    let caller = Caller::root();
    let first = delegate(&pool, &caller, HELPER, &parent_folders(), "request-1")
        .await
        .unwrap();
    let mut reordered = parent_folders();
    reordered.reverse();
    let retry = delegate(&pool, &caller, HELPER, &reordered, "request-1")
        .await
        .unwrap();
    assert_eq!(retry.child, first.child);
    assert!(!retry.starts_here, "a retry does not start the child again");
    let narrower = [folder("workspace-1", ChatFolderCapability::Read)];
    assert_eq!(
        delegate(&pool, &caller, HELPER, &narrower, "request-1")
            .await
            .unwrap_err(),
        DelegationRefusal::RequestConflict
    );
    assert_eq!(
        delegate_with(
            &pool,
            &caller,
            HELPER,
            &parent_folders(),
            "request-1",
            "Other work"
        )
        .await
        .unwrap_err(),
        DelegationRefusal::RequestConflict
    );
    let children: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_work_assignments WHERE parent_assignment_id = ?",
    )
    .bind(PARENT)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(children, 1);
}

#[tokio::test]
async fn delegation_refuses_a_level_below_the_depth_limit() {
    let pool = pool_with_helper().await;
    let child = delegate(&pool, &Caller::root(), HELPER, &parent_folders(), "level-1")
        .await
        .unwrap();
    let child = start_working(&pool, &child.child.assignment_id).await;
    let grandchild = delegate(&pool, &child, PARENT_AGENT, &parent_folders(), "level-2")
        .await
        .unwrap();
    let depth: i64 =
        sqlx::query_scalar("SELECT delegation_depth FROM chat_work_assignments WHERE id = ?")
            .bind(grandchild.child.assignment_id.as_str())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(depth, 2);
    let grandchild = start_working(&pool, &grandchild.child.assignment_id).await;
    assert_eq!(
        delegate(&pool, &grandchild, HELPER, &parent_folders(), "level-3")
            .await
            .unwrap_err(),
        DelegationRefusal::DepthExceeded
    );
}

#[tokio::test]
async fn delegation_refuses_a_synced_parent_whose_depth_disagrees_with_its_chain() {
    let pool = pool_with_helper().await;
    let child = delegate(&pool, &Caller::root(), HELPER, &parent_folders(), "level-1")
        .await
        .unwrap();
    let child = start_working(&pool, &child.child.assignment_id).await;
    let grandchild = delegate(&pool, &child, PARENT_AGENT, &parent_folders(), "level-2")
        .await
        .unwrap();
    let grandchild = start_working(&pool, &grandchild.child.assignment_id).await;
    // Apply does not compare a child's depth with its parent's, so a forged row can claim less.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         UPDATE chat_work_assignments SET delegation_depth = 1 WHERE id = '{}';
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
        grandchild.parent.as_str()
    )))
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(
        delegate(&pool, &grandchild, HELPER, &parent_folders(), "level-3")
            .await
            .unwrap_err(),
        DelegationRefusal::ParentInactive
    );
}

#[tokio::test]
async fn delegation_refuses_a_fifth_open_child() {
    let pool = pool_with_helper().await;
    let caller = Caller::root();
    let mut children = Vec::new();
    for index in 0..MAX_OPEN_CHILDREN_PER_ASSIGNMENT {
        children.push(
            delegate(
                &pool,
                &caller,
                HELPER,
                &parent_folders(),
                &format!("fan-{index}"),
            )
            .await
            .unwrap(),
        );
    }
    assert_eq!(
        delegate(&pool, &caller, HELPER, &parent_folders(), "fan-extra")
            .await
            .unwrap_err(),
        DelegationRefusal::FanOutExceeded
    );
    sqlx::query(
        "UPDATE chat_work_assignments SET state = 'completed', settled_at = ? WHERE id = ?",
    )
    .bind(NOW)
    .bind(children[0].child.assignment_id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        delegate(&pool, &caller, HELPER, &parent_folders(), "fan-extra")
            .await
            .is_ok(),
        "a settled child frees its slot"
    );
}

#[tokio::test]
async fn delegation_refuses_the_caller_a_stranger_and_an_inactive_parent() {
    let pool = pool_with_helper().await;
    let caller = Caller::root();
    assert_eq!(
        delegate(&pool, &caller, PARENT_AGENT, &parent_folders(), "self")
            .await
            .unwrap_err(),
        DelegationRefusal::TargetUnavailable
    );
    assert_eq!(
        delegate(
            &pool,
            &caller,
            "participant:stranger",
            &parent_folders(),
            "stranger"
        )
        .await
        .unwrap_err(),
        DelegationRefusal::TargetUnavailable
    );
    let stale = Caller {
        parent: caller.parent.clone(),
        authorization: ChatAuthorizationRevisionId::new("authorization:other").unwrap(),
    };
    assert_eq!(
        delegate(&pool, &stale, HELPER, &parent_folders(), "stale")
            .await
            .unwrap_err(),
        DelegationRefusal::ParentInactive
    );
    revoke_parent(&pool, "Access changed").await;
    assert_eq!(
        delegate(&pool, &caller, HELPER, &parent_folders(), "revoked")
            .await
            .unwrap_err(),
        DelegationRefusal::ParentInactive
    );
}

#[tokio::test]
async fn revocation_of_the_parent_cancels_its_queued_child_at_claim() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    revoke_parent(&pool, "Access changed").await;
    assert!(!claim(&pool, child).await);
    assert_eq!(
        assignment_state(&pool, child).await,
        (
            "cancelled".to_owned(),
            PARENT_AUTHORITY_REVOKED_REASON.to_owned()
        )
    );
    let jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?",
    )
    .bind(child.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(jobs, 0);
    let decision: String = sqlx::query_scalar(
        "SELECT decision_state FROM chat_assignment_authorization_revisions
         WHERE assignment_id = ?",
    )
    .bind(child.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(decision, "revoked");
}

#[tokio::test]
async fn revocation_of_the_parent_cancels_the_child_at_freeze() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    revoke_parent(&pool, "Access changed").await;
    let trigger = read_trigger(&pool, child).await;
    let mut transaction = pool.begin().await.unwrap();
    let outcome = super::super::context::freeze_context_package(
        &mut transaction,
        super::super::context::WorkToFreeze {
            assignment_id: child,
            reply_thread_id: &created.child.reply_thread_id,
            triggering_message_item_id: &trigger,
            agent_id: &ChatParticipantId::new(HELPER).unwrap(),
        },
        None,
        &crate::chat::organization::tests::test_providers(),
        &UtcTimestamp::new(NOW).unwrap(),
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(
        outcome,
        super::super::context::FreezeOutcome::ParentAuthorityRevoked
    );
    assert_eq!(assignment_state(&pool, child).await.0, "cancelled");
}

async fn read_trigger(pool: &SqlitePool, child: &ChatWorkAssignmentId) -> ChatConversationItemId {
    ChatConversationItemId::new(
        sqlx::query_scalar::<_, String>(
            "SELECT triggering_message_item_id FROM chat_work_assignments WHERE id = ?",
        )
        .bind(child.as_str())
        .fetch_one(pool)
        .await
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn revocation_by_settling_the_parent_keeps_the_delegated_authority() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    revoke_parent(&pool, ganbaru_chat::sync_effects::ASSIGNMENT_SETTLED_REASON).await;
    assert!(claim(&pool, &created.child.assignment_id).await);
}

#[tokio::test]
async fn revocation_of_the_parent_run_cascades_to_its_child_and_grandchild() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = start_working(&pool, &created.child.assignment_id).await;
    let grandchild = delegate(&pool, &child, PARENT_AGENT, &parent_folders(), "level-2")
        .await
        .unwrap();
    let grandchild = start_working(&pool, &grandchild.child.assignment_id).await;
    let folder = ProjectWorkingFolderId::new("workspace-2").unwrap();
    assert!(
        revoke_active_run_authorizations(
            &pool,
            ActiveRunSelector::WorkingFolder(&folder),
            "The working folder was unbound",
        )
        .await
        .unwrap()
    );
    for descendant in [&child, &grandchild] {
        let (decision, reason): (String, String) = sqlx::query_as(
            "SELECT decision_state, reason FROM chat_assignment_authorization_revisions
             WHERE id = ?",
        )
        .bind(descendant.authorization.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(decision, "revoked", "{}", descendant.parent.as_str());
        assert_eq!(reason, PARENT_AUTHORITY_REVOKED_REASON);
    }
}

/// The decision of the latest authorization of `assignment` and whether an open revocation job
/// covers it.
async fn child_authorization(
    pool: &SqlitePool,
    assignment: &ChatWorkAssignmentId,
) -> (String, bool) {
    sqlx::query_as(
        "SELECT authorization.decision_state, EXISTS(
           SELECT 1 FROM chat_access_revocation_jobs job
           WHERE job.authorization_revision_id = authorization.id AND job.state = 'queued'
         )
         FROM chat_assignment_authorization_revisions authorization
         WHERE authorization.assignment_id = ?
         ORDER BY authorization.revision DESC LIMIT 1",
    )
    .bind(assignment.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Copies the state of `assignment` on `from` onto `to` as a sync apply writes it, and runs the
/// apply hook on it.
async fn apply_assignment_state(from: &SqlitePool, to: &SqlitePool, assignment: &str) {
    let (state, reason, settled_at): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT state, state_reason, settled_at FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment)
    .fetch_one(from)
    .await
    .unwrap();
    let mut connection = to.acquire().await.unwrap();
    sqlx::query("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE chat_work_assignments SET state = ?, state_reason = ?, settled_at = ?
         WHERE id = ?",
    )
    .bind(&state)
    .bind(&reason)
    .bind(&settled_at)
    .bind(assignment)
    .execute(&mut *connection)
    .await
    .unwrap();
    ganbaru_chat::sync_effects::after_materialize(
        &mut connection,
        ganbaru_chat::sync_effects::ASSIGNMENTS_TABLE,
        &[assignment.to_owned()],
    )
    .await
    .unwrap();
    sqlx::query("UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1")
        .execute(&mut *connection)
        .await
        .unwrap();
}

/// The parent runs on one device and its child on another, which alone holds the child's
/// authorization. Revoking the parent's authority on the parent's device cancels the child
/// there, and applying that state on the child's device revokes the child's run, while applying
/// only the parent's cancelled state would leave it authorized.
#[tokio::test]
async fn revocation_of_the_parent_cancels_a_child_running_on_another_device() {
    let parent_device = pool_with_helper().await;
    let child_device = pool_with_helper().await;
    let mut children = Vec::new();
    for pool in [&parent_device, &child_device] {
        let created = delegate(
            pool,
            &Caller::root(),
            HELPER,
            &parent_folders(),
            "request-1",
        )
        .await
        .unwrap();
        start_working(pool, &created.child.assignment_id).await;
        children.push(created.child.assignment_id);
    }
    assert_eq!(
        children[0], children[1],
        "the child id derives from its request"
    );
    let child = &children[0];
    // The parent's device holds no live authorization of the child, as when the child executes
    // elsewhere, so its local revocation cascade has nothing to revoke.
    sqlx::query(
        "UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', reason = ?, revoked_at = ?
         WHERE assignment_id = ?",
    )
    .bind(super::super::assignments::MOVED_EXECUTOR_REASON)
    .bind(NOW)
    .bind(child.as_str())
    .execute(&parent_device)
    .await
    .unwrap();

    let folder = ProjectWorkingFolderId::new("workspace-2").unwrap();
    let reason = "The working folder was unbound";
    assert!(
        revoke_active_run_authorizations(
            &parent_device,
            ActiveRunSelector::WorkingFolder(&folder),
            reason,
        )
        .await
        .unwrap()
    );
    let job: String = sqlx::query_scalar(
        "UPDATE chat_access_revocation_jobs SET state = 'claimed', claim_token = 'claim'
         WHERE authorization_revision_id = 'authorization:working' RETURNING id",
    )
    .fetch_one(&parent_device)
    .await
    .unwrap();
    crate::chat::revocation::persist_revocation_before_cleanup(
        &parent_device,
        &job,
        "claim",
        Some("authorization:working"),
        reason,
        &[],
    )
    .await
    .unwrap();
    let parent = ChatWorkAssignmentId::new(PARENT).unwrap();
    assert_eq!(
        assignment_state(&parent_device, &parent).await.0,
        "cancelled"
    );
    assert_eq!(
        assignment_state(&parent_device, child).await,
        (
            "cancelled".to_owned(),
            PARENT_AUTHORITY_REVOKED_REASON.to_owned()
        )
    );

    apply_assignment_state(&parent_device, &child_device, PARENT).await;
    assert_eq!(
        child_authorization(&child_device, child).await,
        ("allowed".to_owned(), false)
    );
    apply_assignment_state(&parent_device, &child_device, child.as_str()).await;
    assert_eq!(
        child_authorization(&child_device, child).await,
        ("revoked".to_owned(), true)
    );
}

#[tokio::test]
async fn delegation_bound_narrows_a_broader_frozen_child_at_claim() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    // A replicated freeze from another device names a folder outside the ceiling.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         INSERT INTO chat_assignment_authorized_folder_sources
            (authorization_revision_id, root_handle, working_folder_id, capability,
             is_execution_target, created_at, resolved_runtime_approval_policy)
         SELECT id, printf('%032d', 9), 'workspace-3', 'edit', 0, '{NOW}', 'ask'
         FROM chat_assignment_authorization_revisions WHERE assignment_id = '{}';
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
        child.as_str()
    )))
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        !claim(&pool, child).await,
        "a frozen folder above the ceiling never starts"
    );
    assert_eq!(assignment_state(&pool, child).await.0, "cancelled");
}

#[test]
fn delegation_approval_clamp_steps_unranked_policies_down_to_ask() {
    assert_eq!(clamp_approval("provider_custom", 2), "ask");
    assert_eq!(clamp_approval("unattended", 1), "ask");
    assert_eq!(clamp_approval("auto_approve", 1), "auto_approve");
    assert_eq!(clamp_approval("auto_approve", 2), "auto_approve");
    assert_eq!(clamp_approval("unattended", 0), "ask");
}

#[test]
fn delegation_ceiling_is_stored_sorted_and_parses_back() {
    let mut folders = parent_folders();
    folders.reverse();
    let stored = folder_ceiling_json(&folders).unwrap();
    assert_eq!(
        parse_folder_ceiling(&stored).unwrap(),
        BTreeMap::from([("workspace-1".to_owned(), 3), ("workspace-2".to_owned(), 1)])
    );
    assert!(parse_folder_ceiling(r#"[{"workingFolderId":"a","capability":"admin"}]"#).is_err());
}

/// The delegation columns of `assignment_id`.
async fn delegation_columns(
    pool: &SqlitePool,
    assignment_id: &str,
) -> (
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    sqlx::query_as(
        "SELECT parent_assignment_id, delegation_depth, delegation_objective,
                delegation_expected_result, delegation_folder_ceiling
         FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// The outcome messages of `child` in the parent's reply thread, with the parent inputs that
/// deliver them.
async fn outcome_rows(pool: &SqlitePool, child: &ChatWorkAssignmentId) -> (i64, i64) {
    let item_id = ganbaru_chat::coordination::delegation::outcome_item_id(child.as_str()).unwrap();
    let messages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_conversation_items
         WHERE id = ? AND reply_thread_id = 'reply:folders'",
    )
    .bind(item_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap();
    let inputs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_work_assignment_inputs
         WHERE message_item_id = ? AND assignment_id = ? AND routing_kind = 'queued_continuation'
           AND delivery_state = 'pending'",
    )
    .bind(item_id.as_str())
    .bind(PARENT)
    .fetch_one(pool)
    .await
    .unwrap();
    (messages, inputs)
}

#[tokio::test]
async fn delegation_outcome_of_a_child_cancelled_at_claim_reaches_the_working_parent() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    revoke_parent(&pool, "Access changed").await;
    assert!(!claim(&pool, child).await);
    assert_eq!(outcome_rows(&pool, child).await, (1, 1));
}

#[tokio::test]
async fn delegation_survives_a_reroute_so_the_successor_stays_under_the_parent_ceiling() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    let original = delegation_columns(&pool, child.as_str()).await;
    let assignment = super::super::reads::read_assignment(&pool, child)
        .await
        .unwrap();
    let writers = crate::chat::organization::tests::chat_writers_of(&[DEVICE]);
    let mut transaction = pool.begin().await.unwrap();
    let successor = super::super::assignments::reroute_assignment(
        &mut transaction,
        &assignment,
        super::super::workflow::PostingDevice::this_device(DEVICE, &writers),
        &UtcTimestamp::new(NOW).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    transaction.commit().await.unwrap();

    assert_eq!(original.0.as_deref(), Some(PARENT));
    assert_eq!(
        delegation_columns(&pool, successor.as_str()).await,
        original
    );
    assert_eq!(assignment_state(&pool, child).await.0, "cancelled");
    let mut connection = pool.acquire().await.unwrap();
    let moved = ganbaru_chat::coordination::delegation::post_delegation_outcome(
        &mut connection,
        child.as_str(),
        NOW,
    )
    .await
    .unwrap();
    assert!(
        moved.is_none(),
        "a rerouted child reports through its successor"
    );
    drop(connection);
    assert_eq!(outcome_rows(&pool, child).await, (0, 0));
}

#[tokio::test]
async fn delegation_outcome_synced_in_is_written_only_by_the_parent_executing_device() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    // The executing device is immutable outside an apply, so the parent arrives as synced.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         UPDATE chat_work_assignments SET executing_device_id = '{DEVICE}' WHERE id = '{PARENT}';
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE chat_work_assignments SET state = 'completed', settled_at = ? WHERE id = ?",
    )
    .bind(NOW)
    .bind(child.as_str())
    .execute(&pool)
    .await
    .unwrap();
    let report = super::super::assignments::report_synced_delegation_outcome;

    assert_eq!(
        report(&pool, child.as_str(), "device:other").await.unwrap(),
        None
    );
    assert_eq!(outcome_rows(&pool, child).await, (0, 0));
    assert_eq!(
        report(&pool, child.as_str(), DEVICE).await.unwrap(),
        Some(ChatWorkAssignmentId::new(PARENT).unwrap())
    );
    assert_eq!(outcome_rows(&pool, child).await, (1, 1));
    assert_eq!(report(&pool, child.as_str(), DEVICE).await.unwrap(), None);
    assert_eq!(outcome_rows(&pool, child).await, (1, 1));
}

#[tokio::test]
async fn delegation_outcome_routing_names_the_parent_that_holds_a_pending_outcome() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = created.child.assignment_id.as_str().to_owned();
    let children = BTreeSet::from([child.clone()]);
    assert!(
        super::super::assignments::parents_with_pending_outcomes(&pool, &children)
            .await
            .unwrap()
            .is_empty()
    );
    revoke_parent(&pool, "Access changed").await;
    assert!(!claim(&pool, &created.child.assignment_id).await);
    assert_eq!(
        super::super::assignments::parents_with_pending_outcomes(&pool, &children)
            .await
            .unwrap(),
        BTreeSet::from([PARENT.to_owned()])
    );
    let undelegated = BTreeSet::from([PARENT.to_owned()]);
    assert!(
        super::super::assignments::parents_with_pending_outcomes(&pool, &undelegated)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn delegation_cancel_of_the_parent_cancels_the_queued_child_and_its_dispatch() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    let parent = ChatWorkAssignmentId::new(PARENT).unwrap();
    let cancelled = super::super::workflow::cancel_delegated_work(&pool, &parent)
        .await
        .unwrap();
    assert_eq!(cancelled, BTreeSet::from([child.as_str().to_owned()]));
    assert_eq!(
        assignment_state(&pool, child).await,
        (
            "cancelled".to_owned(),
            ganbaru_chat::coordination::delegation::PARENT_CANCELLED_REASON.to_owned()
        )
    );
    let open_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_assignment_dispatch_jobs
         WHERE assignment_id = ? AND state IN ('queued', 'claimed')",
    )
    .bind(child.as_str())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open_jobs, 0);
    assert!(!claim(&pool, child).await);
    assert!(
        super::super::workflow::cancel_delegated_work(&pool, &parent)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn delegation_a_person_cancelling_the_parent_settles_it_with_its_open_children() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    let parent = ChatWorkAssignmentId::new(PARENT).unwrap();
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM chat_work_assignments WHERE id = ?")
            .bind(PARENT)
            .fetch_one(&pool)
            .await
            .unwrap();
    let revision = u64::try_from(revision).unwrap();

    let cancelled = super::super::workflow::cancel_assignment_work(&pool, &parent, revision)
        .await
        .unwrap();

    assert_eq!(
        cancelled.assignment.state,
        ChatWorkAssignmentState::Cancelled
    );
    assert!(cancelled.assignment.settled_at.is_some());
    assert_eq!(
        cancelled.descendants,
        BTreeSet::from([child.as_str().to_owned()])
    );
    assert_eq!(assignment_state(&pool, child).await.0, "cancelled");
    let stale = super::super::workflow::cancel_assignment_work(&pool, &parent, revision)
        .await
        .unwrap_err();
    assert_eq!(stale.code, ChatErrorCode::StaleRevision);
}

#[tokio::test]
async fn delegation_reads_show_the_child_origin_and_the_parent_delegated_work() {
    let pool = pool_with_helper().await;
    let created = delegate(
        &pool,
        &Caller::root(),
        HELPER,
        &parent_folders(),
        "request-1",
    )
    .await
    .unwrap();
    let child = &created.child.assignment_id;
    let parent_thread: String =
        sqlx::query_scalar("SELECT reply_thread_id FROM chat_work_assignments WHERE id = ?")
            .bind(PARENT)
            .fetch_one(&pool)
            .await
            .unwrap();

    let origin = super::super::reads::read_assignment(&pool, child)
        .await
        .unwrap()
        .delegation
        .expect("a delegated child reads its origin");
    assert_eq!(origin.parent_assignment_id.as_str(), PARENT);
    assert_eq!(
        origin.parent_reply_thread_id.as_ref().map(|id| id.as_str()),
        Some(parent_thread.as_str())
    );
    assert_eq!(origin.depth, 1);
    assert_eq!(origin.objective, "Check the build");
    assert_eq!(origin.expected_result, "A short report");

    let parent_thread_id = ChatReplyThreadId::new(parent_thread).unwrap();
    let children = super::super::reads::read_delegated_children(&pool, &parent_thread_id)
        .await
        .unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].assignment_id, *child);
    assert_eq!(children[0].agent.id.as_str(), HELPER);
    assert_eq!(children[0].objective, "Check the build");
    assert_eq!(children[0].reply_thread_id, created.child.reply_thread_id);
    let none = super::super::reads::read_delegated_children(&pool, &created.child.reply_thread_id)
        .await
        .unwrap();
    assert!(none.is_empty(), "a child thread lists no work of its own");
}
