//! Review ownership and working-folder authorization.

use super::super::models::{ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId};
use super::super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use super::contracts::{
    ApplyChatReviewActionRequest, ChatReviewActionResultRead, OpenChatReviewRequest,
};
use super::registry::ReviewSnapshot;
use super::{ReviewWorkspaceAuthorizer, default_context_lines, persistence_error};
use sqlx::SqlitePool;

pub async fn require_request_ownership(
    pool: &SqlitePool,
    snapshot: &ReviewSnapshot,
    working_folder_id: &ProjectWorkingFolderId,
    execution_environment_id: Option<&str>,
) -> ChatResult<()> {
    let requested_environment =
        resolve_requested_environment_id(pool, working_folder_id, execution_environment_id).await?;
    if &snapshot.working_folder_id != working_folder_id
        || requested_environment != snapshot.environment_id
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Review snapshot belongs to another execution environment",
            false,
        ));
    }
    Ok(())
}

pub async fn resolve_requested_environment_id(
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
    execution_environment_id: Option<&str>,
) -> ChatResult<String> {
    match execution_environment_id {
        Some(environment) => Ok(environment.to_string()),
        None => sqlx::query_scalar(
            "SELECT id FROM chat_execution_environments
             WHERE working_folder_id = ? AND kind = 'current_folder'
               AND lifecycle_state = 'available' AND archived_at IS NULL",
        )
        .bind(working_folder_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                "The current-folder execution environment is unavailable",
                true,
            )
        }),
    }
}

pub async fn authorize_review_request(
    authorizer: &dyn ReviewWorkspaceAuthorizer,
    pool: &SqlitePool,
    request: &OpenChatReviewRequest,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<(ProjectWorkingFolderId, String, AuthorizedWorkingFolder)> {
    authorizer.authorize(pool, request, operation).await
}

pub async fn authorize_snapshot(
    authorizer: &dyn ReviewWorkspaceAuthorizer,
    pool: &SqlitePool,
    snapshot: &ReviewSnapshot,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<()> {
    let request = OpenChatReviewRequest {
        thread_id: snapshot.thread_id.clone(),
        working_folder_id: snapshot.working_folder_id.clone(),
        execution_environment_id: Some(snapshot.environment_id.clone()),
        source: snapshot.source.clone(),
        ignore_whitespace: snapshot.ignore_whitespace,
        context_lines: snapshot.context_lines,
        preferred_relative_path: None,
    };
    let (working_folder_id, environment_id, authorized) =
        authorize_review_request(authorizer, pool, &request, operation).await?;
    if working_folder_id != snapshot.working_folder_id
        || environment_id != snapshot.environment_id
        || authorized.canonical_path != snapshot.root
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Review snapshot belongs to another execution environment",
            false,
        ));
    }
    Ok(())
}

pub async fn authorize_completed_action(
    authorizer: &dyn ReviewWorkspaceAuthorizer,
    pool: &SqlitePool,
    request: &ApplyChatReviewActionRequest,
    result: &ChatReviewActionResultRead,
) -> ChatResult<()> {
    let completed_request = OpenChatReviewRequest {
        thread_id: request.thread_id.clone(),
        working_folder_id: request.working_folder_id.clone(),
        execution_environment_id: request.execution_environment_id.clone(),
        source: result.snapshot.source.clone(),
        ignore_whitespace: false,
        context_lines: default_context_lines(),
        preferred_relative_path: None,
    };
    authorize_review_request(
        authorizer,
        pool,
        &completed_request,
        WorkingFolderAuthorizationOperation::Git,
    )
    .await
    .map(|_| ())
}
