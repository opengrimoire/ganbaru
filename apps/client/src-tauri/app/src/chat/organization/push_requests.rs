//! Push requests a teammate posts and a person confirms.
//!
//! A confirmation pushes only while the request is open, the asking run's publish grant still
//! holds, the folder's repository configuration chooses nothing about how Git connects, and a
//! fresh read of the folder names the recorded commit, remote, destination, push URL, and Git
//! configuration, and for a request with a commit the recorded working tree. It then answers the
//! request with a reply by the local person that mentions the teammate, whose ids derive from the
//! request, so confirming one request on two devices posts one reply.

use super::{PostOutcome, chat_pool, identifier_error, persistence_error, post_message};
use crate::chat::channels::now_timestamp;
use crate::chat::internal_mcp::InternalMcpFolderSource;
use crate::chat::internal_mcp_tools::verify_authorized_folder_source;
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry;
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat::coordination::push_requests::{
    PUSH_CONFIRMATION_REFERENCE_PREFIX, PushRequest, PushRequestRefusal, StoredPushRequest,
    push_confirmation_id, read_push_request, read_push_request_state,
};
use ganbaru_chat::coordination::reports::HOST_TOOL_SOURCE;
use ganbaru_chat::git::hardened::requested_configuration;
use ganbaru_chat::git::{
    GitFolder, GitPushTarget, advance_branch, commit_tree, push_object, push_target,
    worktree_commit,
};
use ganbaru_chat::ids::derived_id;
use ganbaru_chat_contracts::models::*;
use ganbaru_sync_contracts::chat_push_approvals::PUSH_APPROVAL_KEY_PREFIX;
use serde_json::json;
use sqlx::{Row, SqlitePool};
use tauri::Manager;

/// Most push requests one state read may name.
const MAX_PUSH_REQUEST_STATE_READS: usize = 200;

/// Reply a push made under a person's standing approval posts.
const STANDING_APPROVAL_REPLY: &str = "Pushed automatically under your standing approval.";

/// Reply a push made because the asking turn ran with full access posts.
const FULL_ACCESS_REPLY: &str = "Pushed automatically because this turn ran with full access.";

/// Who lets a push request push.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PushAuthority {
    /// A person confirmed the request, and approves later matching pushes when `remember`.
    Person { remember: bool },
    /// Nobody confirmed it, so a standing approval or a full access turn must let it push.
    Standing,
}

/// What lets a push request push without a person confirming it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StandingPermission {
    /// An active approval names the teammate, folder, push URL, and destination branch, and
    /// still records the run's approval policy and the teammate's safety mode.
    Approval,
    /// The asking run's grant resolved to the unattended approval policy.
    FullAccess,
}

impl StandingPermission {
    /// The reply the push posts after the teammate mention.
    const fn reply(self) -> &'static str {
        match self {
            Self::Approval => STANDING_APPROVAL_REPLY,
            Self::FullAccess => FULL_ACCESS_REPLY,
        }
    }
}

/// Pushes the commit a teammate's open push request names and answers the request with
/// `request.normalized_markdown` after a mention of the teammate. When
/// `request.remember_approval` is set and the push succeeds, later matching requests of the
/// teammate push without asking.
///
/// # Errors
///
/// Returns a [`PushRequestRefusal`] error when the request is not open, the folder no longer
/// names its target, or its repository configuration chooses how Git connects, a generic denial
/// when the publish grant no longer holds, and a conflict carrying Git's output when the push
/// fails.
pub(in crate::chat::organization) async fn confirm_push_request(
    app: tauri::AppHandle,
    db_url: String,
    request: ConfirmPushRequestCommand,
) -> ChatResult<PostChatMessageResult> {
    let authority = PushAuthority::Person {
        remember: request.remember_approval,
    };
    answer_push_request(
        app,
        db_url,
        request.request_item_id.as_str(),
        Some(&request.normalized_markdown),
        authority,
    )
    .await
}

/// Pushes, once the organizational provider turn `turn_id` ended, each open push request its
/// run asked that a standing approval or a full access turn lets push, through the checks a
/// person's confirmation runs. A request nothing lets push stays open for a person. Call it
/// after the turn's settlement committed and its workspace mutation ended.
pub(crate) fn push_standing_requests_after_turn(app: &tauri::AppHandle, turn_id: &ChatTurnId) {
    let app = app.clone();
    let turn_id = turn_id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = push_standing_requests(app, &turn_id).await {
            eprintln!(
                "Chat push requests of a turn were not checked for standing approval: {error}"
            );
        }
    });
}

async fn push_standing_requests(app: tauri::AppHandle, turn_id: &ChatTurnId) -> ChatResult<()> {
    let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let item_ids = push_request_items_of_turn(&pool, turn_id).await?;
    for item_id in item_ids {
        match answer_push_request(
            app.clone(),
            db_url.clone(),
            &item_id,
            None,
            PushAuthority::Standing,
        )
        .await
        {
            Ok(_) => {}
            // A refused request waits for a person, who sees the same refusal on the card.
            Err(error) if error.code == ChatErrorCode::Permission => {}
            Err(error) => eprintln!("A standing Chat push did not complete: {error}"),
        }
    }
    Ok(())
}

/// The push request items the run of provider turn `turn_id` posted, oldest first.
async fn push_request_items_of_turn(
    pool: &SqlitePool,
    turn_id: &ChatTurnId,
) -> ChatResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT semantic.item_id
         FROM chat_agent_runs run
         JOIN chat_work_semantic_updates semantic
           ON semantic.assignment_id = run.assignment_id
          AND json_extract(semantic.payload_data, '$.agentRunId') = run.id
         JOIN chat_conversation_items item ON item.id = semantic.item_id
         WHERE run.provider_turn_id = ? AND semantic.update_kind = 'question'
           AND json_extract(semantic.payload_data, '$.payload.source') = ?
           AND json_type(semantic.payload_data, '$.payload.push') = 'object'
         ORDER BY item.order_key, item.id",
    )
    .bind(turn_id.as_str())
    .bind(HOST_TOOL_SOURCE)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)
}

/// Pushes the open push request `item_id` when `authority` lets it, and answers it with `text`,
/// or with the standing reply when `text` is `None`.
async fn answer_push_request(
    app: tauri::AppHandle,
    db_url: String,
    item_id: &str,
    text: Option<&str>,
    authority: PushAuthority,
) -> ChatResult<PostChatMessageResult> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let stored = read_push_request(&mut connection, item_id).await?;
    drop(connection);
    stored
        .state
        .require_open()
        .map_err(PushRequestRefusal::into_error)?;
    let source = publish_source(&pool, &stored).await?;
    let authorization_revision_id = stored
        .authorization_revision_id
        .as_deref()
        .ok_or_else(|| PushRequestRefusal::Closed.into_error())?;
    verify_authorized_folder_source(&pool, authorization_revision_id, &source).await?;
    let text = match authority {
        PushAuthority::Person { .. } => text.unwrap_or_default(),
        PushAuthority::Standing => standing_permission(&pool, &stored, &source)
            .await?
            .ok_or_else(confirmation_needed)?
            .reply(),
    };
    let reply = confirmation_post(&stored, item_id, text)?;
    push_confirmed(&app, &pool, &stored.request).await?;
    if authority == (PushAuthority::Person { remember: true })
        && let Err(error) = remember_push_approval(&pool, &stored, &source).await
    {
        eprintln!("The standing push approval was not saved: {error}");
    }
    match post_message(app, db_url, &reply, None, None).await? {
        PostOutcome::Posted(result) => Ok(result),
        PostOutcome::Refused(refusal) => Err(refusal),
    }
}

/// What lets the open request `stored` push without a person, given the asking run's publish
/// `source`, or `None` when a person must confirm it. An archived or removed teammate never
/// pushes on its own.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
async fn standing_permission(
    pool: &SqlitePool,
    stored: &StoredPushRequest,
    source: &InternalMcpFolderSource,
) -> ChatResult<Option<StandingPermission>> {
    if stored.teammate_name.is_none() {
        return Ok(None);
    }
    if source.runtime_approval_policy == ChatRuntimeApprovalPolicy::Unattended {
        return Ok(Some(StandingPermission::FullAccess));
    }
    let approved: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_push_approvals approval
           JOIN chat_teammates teammate ON teammate.participant_id = approval.teammate_id
           WHERE approval.teammate_id = ? AND approval.working_folder_id = ?
             AND approval.push_url = ? AND approval.destination_branch = ?
             AND approval.revoked_at IS NULL AND approval.runtime_approval_policy = ?
             AND approval.safety_mode = teammate.current_policy_safety_mode
         )",
    )
    .bind(&stored.teammate_id)
    .bind(&stored.request.working_folder_id)
    .bind(&stored.request.target.push_url)
    .bind(&stored.request.target.destination_branch)
    .bind(super::wire_runtime_approval_policy(
        source.runtime_approval_policy,
    ))
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    Ok(approved.then_some(StandingPermission::Approval))
}

/// Records, after a person's confirmed push succeeded, that later pushes of the teammate from
/// the folder to the same push URL and destination branch need no confirmation while the run's
/// approval policy `source` resolved to and the teammate's current safety mode stay the same.
/// Approving again clears an earlier revocation. A teammate without a current policy gets no
/// approval.
///
/// # Errors
///
/// Returns a validation error when the key cannot be derived and a persistence error when the
/// write fails.
async fn remember_push_approval(
    pool: &SqlitePool,
    stored: &StoredPushRequest,
    source: &InternalMcpFolderSource,
) -> ChatResult<()> {
    let request = &stored.request;
    let sync_key = derived_id(
        PUSH_APPROVAL_KEY_PREFIX,
        &[
            &stored.teammate_id,
            &request.working_folder_id,
            &request.target.push_url,
            &request.target.destination_branch,
        ],
    )?;
    let now = now_timestamp()?;
    sqlx::query(
        "INSERT INTO chat_push_approvals
            (teammate_id, working_folder_id, push_url, destination_branch, sync_key,
             runtime_approval_policy, safety_mode, approved_by_participant_id, approved_at)
         SELECT teammate.participant_id, ?, ?, ?, ?, ?, teammate.current_policy_safety_mode, ?, ?
         FROM chat_teammates teammate
         WHERE teammate.participant_id = ? AND teammate.current_policy_safety_mode IS NOT NULL
         ON CONFLICT (teammate_id, working_folder_id, push_url, destination_branch) DO UPDATE SET
             runtime_approval_policy = excluded.runtime_approval_policy,
             safety_mode = excluded.safety_mode,
             approved_by_participant_id = excluded.approved_by_participant_id,
             approved_at = excluded.approved_at, revoked_at = NULL, revision = revision + 1",
    )
    .bind(&request.working_folder_id)
    .bind(&request.target.push_url)
    .bind(&request.target.destination_branch)
    .bind(&sync_key)
    .bind(super::wire_runtime_approval_policy(
        source.runtime_approval_policy,
    ))
    .bind(super::LOCAL_PARTICIPANT_ID)
    .bind(now.as_str())
    .bind(&stored.teammate_id)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn confirmation_needed() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "The push needs a person's confirmation",
        false,
    )
}

/// Reads where each push request in `request_item_ids` stands.
pub(in crate::chat::organization) async fn read_push_request_states(
    app: tauri::AppHandle,
    db_url: String,
    request_item_ids: Vec<ChatConversationItemId>,
) -> ChatResult<Vec<ChatPushRequestStateRead>> {
    if request_item_ids.len() > MAX_PUSH_REQUEST_STATE_READS {
        return Err(ChatError::validation(
            "requestItemIds",
            format!("Read at most {MAX_PUSH_REQUEST_STATE_READS} push requests at once"),
        ));
    }
    let pool = chat_pool(app, db_url).await?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let mut states = Vec::with_capacity(request_item_ids.len());
    for request_item_id in request_item_ids {
        let state = read_push_request_state(&mut connection, request_item_id.as_str()).await?;
        states.push(ChatPushRequestStateRead {
            request_item_id,
            state,
        });
    }
    Ok(states)
}

/// Authorizes the request's folder, re-reads its push target and Git configuration under the
/// folder's mutation guard, and pushes the recorded commit when nothing changed. A request with a
/// commit also needs the working tree to record the same tree, author, signing, and paths; the
/// commit is made on the recorded commit and pushed, and the local branch then moves to it. Every
/// run is a confirmed teammate request, so it never runs the folder's repository programs or
/// hooks and needs no trust decision for the folder's Git configuration.
async fn push_confirmed(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: &PushRequest,
) -> ChatResult<()> {
    let working_folder_id =
        ProjectWorkingFolderId::new(request.working_folder_id.clone()).map_err(identifier_error)?;
    let authorized = crate::chat::workspace::commands::authorize_working_folder(
        app,
        pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::Git,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        app,
        pool,
        authorized,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    let _guard = app
        .state::<ChatWorkspaceMutationRegistry>()
        .try_mutation(&authorized.canonical_path)?;
    let folder = GitFolder::untrusted(&authorized.canonical_path);
    let configuration = requested_configuration(authorized.canonical_path.clone())
        .await
        .ok();
    if configuration.is_some_and(|configuration| configuration.repository_connection) {
        return Err(PushRequestRefusal::RepositoryConnection.into_error());
    }
    let fresh = push_target(folder, Some(&request.target.remote)).await.ok();
    let digest = configuration.map(|configuration| configuration.digest.to_hex());
    require_unchanged(request, fresh.as_ref(), digest.as_deref())
        .map_err(PushRequestRefusal::into_error)?;
    let Some(commit) = &request.commit else {
        return push_object(folder, &request.target).await;
    };
    let parent = request.target.object_id.as_str();
    let worktree = worktree_commit(folder, parent).await.ok();
    if worktree.as_ref() != Some(&commit.worktree) {
        return Err(PushRequestRefusal::TargetChanged.into_error());
    }
    let target = GitPushTarget {
        object_id: commit_tree(
            folder,
            parent,
            &commit.worktree.tree_id,
            &commit.message,
            commit.worktree.signed,
        )
        .await?,
        ..request.target.clone()
    };
    push_object(folder, &target).await?;
    // The remote holds the commit now, so the confirmation stands even when the local branch
    // cannot follow; the folder then shows the change as uncommitted until a fetch.
    advance_branch(folder, &target.local_branch, &target.object_id, parent)
        .await
        .or(Ok(()))
}

/// `Ok` when a fresh read names exactly the recorded target and Git configuration digest.
///
/// # Errors
///
/// Returns [`PushRequestRefusal::TargetChanged`] when either read failed or differs.
fn require_unchanged(
    request: &PushRequest,
    fresh: Option<&GitPushTarget>,
    digest: Option<&str>,
) -> Result<(), PushRequestRefusal> {
    if fresh == Some(&request.target) && digest == Some(request.git_config_digest.as_str()) {
        Ok(())
    } else {
        Err(PushRequestRefusal::TargetChanged)
    }
}

/// The publish folder source of the asking run's authorization revision for the request's
/// folder. A request recorded in an execution environment needs the execution target source.
///
/// # Errors
///
/// Returns [`PushRequestRefusal::Closed`] when the run is gone, and a generic denial when the
/// revision grants no matching publish source.
async fn publish_source(
    pool: &SqlitePool,
    stored: &StoredPushRequest,
) -> ChatResult<InternalMcpFolderSource> {
    let Some(authorization_revision_id) = stored.authorization_revision_id.as_deref() else {
        return Err(PushRequestRefusal::Closed.into_error());
    };
    let row = sqlx::query(
        "SELECT root_handle, is_execution_target, resolved_runtime_approval_policy
         FROM chat_assignment_authorized_folder_sources
         WHERE authorization_revision_id = ? AND working_folder_id = ? AND capability = 'publish'",
    )
    .bind(authorization_revision_id)
    .bind(&stored.request.working_folder_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(publish_denied)?;
    let is_execution_target = row
        .try_get::<i64, _>("is_execution_target")
        .map_err(persistence_error)?
        == 1;
    if stored.request.execution_environment_id.is_some() && !is_execution_target {
        return Err(publish_denied());
    }
    Ok(InternalMcpFolderSource {
        root_handle: row.try_get("root_handle").map_err(persistence_error)?,
        working_folder_id: ProjectWorkingFolderId::new(stored.request.working_folder_id.clone())
            .map_err(identifier_error)?,
        capability: ChatFolderCapability::Publish,
        is_execution_target,
        runtime_approval_policy: super::parse_runtime_approval_policy(
            &row.try_get::<String, _>("resolved_runtime_approval_policy")
                .map_err(persistence_error)?,
        )?,
    })
}

/// The reply that answers the request `item_id`: `text` after a mention of the asking teammate,
/// in the request's reply thread, under ids derived from the request.
///
/// # Errors
///
/// Returns a validation error for empty text, and a conflict when the teammate was removed.
fn confirmation_post(
    stored: &StoredPushRequest,
    item_id: &str,
    text: &str,
) -> ChatResult<PostChatMessageCommand> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ChatError::validation(
            "normalizedMarkdown",
            "Message content is required",
        ));
    }
    let teammate_name = stored.teammate_name.clone().ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Conflict,
            "The teammate that requested this push was removed",
            false,
        )
    })?;
    let mention = format!("@{teammate_name}");
    let end_offset = u64::try_from(mention.len()).map_err(|_| {
        ChatError::validation("normalizedMarkdown", "The teammate name is too long")
    })?;
    let normalized_markdown = format!("{mention} {text}");
    let derived = |prefix: &str| push_confirmation_id(prefix, item_id);
    let reference = ChatMessageReference::Participant {
        metadata: ChatReferenceMetadata {
            reference_id: ChatMessageReferenceId::new(derived(PUSH_CONFIRMATION_REFERENCE_PREFIX)?)
                .map_err(identifier_error)?,
            label_snapshot: teammate_name,
            start_offset: 0,
            end_offset,
            plain_text_projection: mention,
        },
        participant_id: ChatParticipantId::new(stored.teammate_id.clone())
            .map_err(identifier_error)?,
        participant_kind: ChatParticipantKind::AiTeammate,
    };
    Ok(PostChatMessageCommand {
        client_command_id: ChatCommandId::new(derived("command")?).map_err(identifier_error)?,
        channel_id: ChatChannelId::new(stored.channel_id.clone()).map_err(identifier_error)?,
        reply_thread_id: Some(
            ChatReplyThreadId::new(stored.reply_thread_id.clone()).map_err(identifier_error)?,
        ),
        normalized_markdown,
        rich_content: VersionedJson {
            schema_version: 1,
            value: json!({ "type": "document", "children": [] }),
        },
        attachment_ids: Vec::new(),
        references: vec![reference],
        execution_target: None,
        also_send_to_channel: false,
        followup_routing: None,
    })
}

fn publish_denied() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "The teammate may no longer publish from this folder",
        false,
    )
}

#[cfg(test)]
mod tests;
