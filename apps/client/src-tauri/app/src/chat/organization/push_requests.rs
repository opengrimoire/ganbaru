//! Push requests a teammate posts and a person confirms.
//!
//! A confirmation pushes only while the request is open, the asking run's publish grant still
//! holds, the folder's repository configuration chooses nothing about how Git connects, and a
//! fresh read of the folder names the recorded commit, remote, destination, push URL, and Git
//! configuration. It then answers the request with a reply by the local person that mentions the
//! teammate, whose ids derive from the request, so confirming one request on two devices posts
//! one reply.

use super::{PostOutcome, chat_pool, identifier_error, persistence_error, post_message};
use crate::chat::internal_mcp::InternalMcpFolderSource;
use crate::chat::internal_mcp_tools::verify_authorized_folder_source;
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry;
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat::coordination::push_requests::{
    PUSH_CONFIRMATION_REFERENCE_PREFIX, PushRequest, PushRequestRefusal, StoredPushRequest,
    push_confirmation_id, read_push_request, read_push_request_state,
};
use ganbaru_chat::git::hardened::requested_configuration;
use ganbaru_chat::git::{GitFolder, GitPushTarget, push_object, push_target};
use ganbaru_chat_contracts::models::*;
use serde_json::json;
use sqlx::{Row, SqlitePool};
use tauri::Manager;

/// Most push requests one state read may name.
const MAX_PUSH_REQUEST_STATE_READS: usize = 200;

/// Pushes the commit a teammate's open push request names and answers the request with
/// `request.normalized_markdown` after a mention of the teammate.
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
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let item_id = request.request_item_id.as_str();
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let stored = read_push_request(&mut connection, item_id).await?;
    drop(connection);
    stored
        .state
        .require_open()
        .map_err(PushRequestRefusal::into_error)?;
    let reply = confirmation_post(&stored, item_id, &request.normalized_markdown)?;
    let source = publish_source(&pool, &stored).await?;
    let authorization_revision_id = stored
        .authorization_revision_id
        .as_deref()
        .ok_or_else(|| PushRequestRefusal::Closed.into_error())?;
    verify_authorized_folder_source(&pool, authorization_revision_id, &source).await?;
    push_confirmed(&app, &pool, &stored.request).await?;
    match post_message(app, db_url, &reply, None, None).await? {
        PostOutcome::Posted(result) => Ok(result),
        PostOutcome::Refused(refusal) => Err(refusal),
    }
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
/// folder's mutation guard, and pushes the recorded commit when nothing changed. The push runs
/// as a confirmed teammate request, so it never runs the folder's repository programs or hooks
/// and needs no trust decision for the folder's Git configuration.
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
    push_object(folder, &request.target).await
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
