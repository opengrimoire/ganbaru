//! The push request host tool: an agent with a publish grant asks the person who assigned the
//! work to push a commit.
//!
//! The agent names only a folder it may publish from, an optional remote, a summary, and an
//! optional commit message. Ganbaru reads the branch, commit, destination, and push URL from the
//! folder itself, without a network call and without running repository programs, and with a
//! commit message also the working tree it would commit, and records them as a question the
//! person confirms. Nothing is committed or reaches a remote until a person confirms the request.

use super::authorization::verify_folder_source;
use super::reports::{client_request_id, host_call};
use super::workspace::{authorize_folder, folder_source};
use super::{HostToolContext, generic_denial_error, optional_string, required_string};
use crate::chat::command_support::now_timestamp;
use ganbaru_chat::coordination::push_requests::{
    MAX_PUSH_SUMMARY_CHARS, PushRequest, PushRequestCommit, PushRequestRefusal,
    normalize_push_summary, record_host_push_request,
};
use ganbaru_chat::git::hardened::requested_configuration;
use ganbaru_chat::git::{
    GitFolder, GitPushTargetError, GitWorktreeCommitError, MAX_COMMIT_MESSAGE_CHARS,
    normalize_commit_message, push_target, worktree_commit,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatFolderCapability, ChatResult};
use serde_json::{Map, Value, json};

use super::delegation::MAX_CLIENT_REQUEST_ID_CHARS;

/// Longest remote name an agent may name, in bytes.
const MAX_REMOTE_NAME_BYTES: usize = 255;

/// The input properties of the push request tool.
pub(super) fn properties() -> Value {
    json!({
        "rootHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
        "summary": { "type": "string", "minLength": 1, "maxLength": MAX_PUSH_SUMMARY_CHARS },
        "remote": { "type": "string", "minLength": 1, "maxLength": MAX_REMOTE_NAME_BYTES },
        "commitMessage": {
            "type": "string", "minLength": 1, "maxLength": MAX_COMMIT_MESSAGE_CHARS
        },
        "clientRequestId": {
            "type": "string", "minLength": 1, "maxLength": MAX_CLIENT_REQUEST_ID_CHARS
        }
    })
}

/// Reads what a push of the folder's checked out branch would send, with a commit message the
/// commit of its working tree on that branch, and asks the person to confirm it, moving the
/// assignment to waiting for an answer.
pub(super) async fn request_push(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let root_handle = required_string(arguments, "rootHandle", 1024)?;
    let summary = arguments
        .get("summary")
        .and_then(Value::as_str)
        .ok_or_else(generic_denial_error)?;
    let summary = normalize_push_summary(summary).map_err(PushRequestRefusal::into_error)?;
    if summary
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err(generic_denial_error());
    }
    let remote = optional_string(arguments, "remote", MAX_REMOTE_NAME_BYTES)?;
    let commit_message = match arguments.get("commitMessage") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .and_then(normalize_commit_message)
                .ok_or_else(|| PushRequestRefusal::CommitMessageInvalid.into_error())?,
        ),
    };
    let client_request_id = client_request_id(arguments)?;
    let source = folder_source(context.scope, root_handle, ChatFolderCapability::Publish)?;
    verify_folder_source(context.pool, context.scope, source).await?;
    let authorized = authorize_folder(context, source, false).await?;
    // Reads never run repository programs, whatever the folder's trust decision.
    let folder = GitFolder::untrusted(&authorized.canonical_path);
    let target = push_target(folder, remote)
        .await
        .map_err(|error| match error {
            GitPushTargetError::Detached => PushRequestRefusal::Detached.into_error(),
            GitPushTargetError::NoRemote => PushRequestRefusal::RemoteUnavailable.into_error(),
            GitPushTargetError::Unsupported => PushRequestRefusal::TargetUnsupported.into_error(),
            GitPushTargetError::RepositoryConnection => {
                PushRequestRefusal::RepositoryConnection.into_error()
            }
            GitPushTargetError::Failed(error) => error,
        })?;
    let configuration = requested_configuration(authorized.canonical_path.clone())
        .await
        .map_err(|_| configuration_error())?;
    let commit = match commit_message {
        None => None,
        Some(message) => Some(PushRequestCommit {
            message: message.to_string(),
            worktree: worktree_commit(folder, &target.object_id).await.map_err(
                |error| match error {
                    GitWorktreeCommitError::NothingToCommit => {
                        PushRequestRefusal::NothingToCommit.into_error()
                    }
                    GitWorktreeCommitError::NoIdentity => {
                        PushRequestRefusal::CommitIdentityMissing.into_error()
                    }
                    GitWorktreeCommitError::Failed(error) => error,
                },
            )?,
        }),
    };
    let request = PushRequest {
        working_folder_id: source.working_folder_id.as_str().to_string(),
        execution_environment_id: source
            .is_execution_target
            .then(|| context.scope.execution_environment_id.clone())
            .flatten(),
        target,
        git_config_digest: configuration.digest.to_hex(),
        commit,
    };
    let now = now_timestamp()?;
    let mut transaction = context
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    record_host_push_request(
        &mut transaction,
        host_call(context, client_request_id),
        summary,
        &request,
        now.as_str(),
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(json!({
        "status": "waiting",
        "branch": request.target.local_branch,
        "remote": request.target.remote,
        "destinationBranch": request.target.destination_branch,
        "objectId": request.target.object_id,
        "commits": request.commit.is_some(),
    }))
}

fn configuration_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Could not read the folder's Git configuration",
        true,
    )
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat push request could not be saved",
        true,
    )
}
