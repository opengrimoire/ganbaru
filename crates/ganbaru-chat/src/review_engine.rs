//! Immutable, provider-neutral Chat review snapshots and guarded Git actions.

use super::git;
use super::models::{ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId};
use super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::workspace::mutation::ChatWorkspaceMutationRegistry;
use sqlx::SqlitePool;
use std::fs;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

pub type ReviewAuthorizationFuture<'a> = Pin<
    Box<
        dyn Future<Output = ChatResult<(ProjectWorkingFolderId, String, AuthorizedWorkingFolder)>>
            + Send
            + 'a,
    >,
>;

pub trait ReviewWorkspaceAuthorizer: Sync {
    fn authorize<'a>(
        &'a self,
        pool: &'a SqlitePool,
        request: &'a OpenChatReviewRequest,
        operation: WorkingFolderAuthorizationOperation,
    ) -> ReviewAuthorizationFuture<'a>;
}

pub trait ChatWorkspaceChangeEmitter: Sync {
    fn invalidate_paths(
        &self,
        working_folder_id: &ProjectWorkingFolderId,
        execution_environment_id: Option<&str>,
        paths: Vec<String>,
        repository_changed: bool,
    );
}

const MIN_PATCH_PAGE_BYTES: usize = 64 * 1024;
const MAX_PATCH_PAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PATCHES_PER_RESPONSE: usize = 64;
const PATCH_RESPONSE_ENVELOPE_BYTES: usize = 4 * 1024;

mod actions;
mod authorization;
mod contracts;
mod material;
mod opening;
mod patch_parser;
mod patch_store;
mod registry;
mod selection;
mod validation;

#[cfg(test)]
use actions::require_action_allowed;
use actions::{action_paths, apply_action};
pub use authorization::resolve_requested_environment_id;
use authorization::{authorize_completed_action, authorize_snapshot, require_request_ownership};
pub use contracts::*;
use material::working_tree_material;
#[cfg(test)]
use material::{enrich_working_tree_files, git_files, parse_name_status};
use opening::open_review;
#[cfg(test)]
use opening::{file_contract, provider_turn_patch};
#[cfg(test)]
use patch_parser::{hunk_id, parse_patch};
use patch_store::{ReviewObjectStore, read_patch_page, unavailable_patch};
pub use registry::ChatReviewRegistry;
pub use registry::ResolveReviewSelectionRequest;
use registry::{
    CompletedReviewOperation, DEFAULT_PATCH_PAGE_BYTES, ReviewSnapshot, action_request_fingerprint,
    review_revision, stale_snapshot_read,
};
#[cfg(test)]
use registry::{ReviewFileInternal, completed_operation_key, file_id, snapshot_id};
#[cfg(test)]
use selection::{select_provider_patch_lines, select_text_range};
#[cfg(test)]
use validation::MAX_CONTEXT_LINES;
pub use validation::{source_requires_thread, thread_required_error};
use validation::{
    validate_action_request, validate_open_request, validate_patch_request, validate_path,
};

#[cfg(test)]
use super::events::{CanonicalEvent, ChangedFileSummary};
#[cfg(test)]
use super::models::{ChatThreadId, ChatTurnId};
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub async fn open_review_service(
    authorizer: &dyn ReviewWorkspaceAuthorizer,
    state: &ChatReviewRegistry,
    pool: &SqlitePool,
    database_identity: &str,
    request: OpenChatReviewRequest,
) -> ChatResult<ChatReviewOpenRead> {
    let mut request = request;
    validate_open_request(&request)?;
    request.context_lines = request.context_lines.max(3);
    open_review(authorizer, state, pool, database_identity, request).await
}

pub async fn read_review_patches_service(
    authorizer: &dyn ReviewWorkspaceAuthorizer,
    state: &ChatReviewRegistry,
    pool: &SqlitePool,
    database_identity: &str,
    request: ReadChatReviewPatchesRequest,
) -> ChatResult<ChatReviewPatchesRead> {
    validate_patch_request(&request)?;
    let snapshot = state.snapshot(
        &request.snapshot_id,
        Some(&request.thread_id),
        database_identity,
    )?;
    require_request_ownership(
        pool,
        &snapshot,
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    require_snapshot_revision(&snapshot, &request.review_revision)?;
    authorize_snapshot(
        authorizer,
        pool,
        &snapshot,
        WorkingFolderAuthorizationOperation::Diff,
    )
    .await?;
    let maximum = request
        .byte_limit
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(DEFAULT_PATCH_PAGE_BYTES)
        .clamp(MIN_PATCH_PAGE_BYTES, MAX_PATCH_PAGE_BYTES);
    let requested = request.file_ids.clone();
    if let Some(cursor) = request.continuation_cursor.as_deref() {
        if !requested
            .iter()
            .any(|file_id| parse_cursor(cursor, file_id).is_some())
        {
            return Err(ChatError::validation(
                "continuationCursor",
                "Review continuation cursor is invalid for the requested files",
            ));
        }
    }
    let mut patches = Vec::new();
    let mut remaining = maximum.saturating_sub(PATCH_RESPONSE_ENVELOPE_BYTES);
    let start_index = request
        .continuation_cursor
        .as_deref()
        .and_then(|cursor| {
            requested
                .iter()
                .position(|file_id| parse_cursor(cursor, file_id).is_some())
        })
        .unwrap_or(0);
    let mut response_cursor = None;
    for (index, file_id) in requested.iter().enumerate().skip(start_index) {
        if patches.len() >= MAX_PATCHES_PER_RESPONSE
            || (remaining < MIN_PATCH_PAGE_BYTES && !patches.is_empty())
        {
            response_cursor = Some(format!("{file_id}/0"));
            break;
        }
        let start_hunk = if index == start_index {
            request
                .continuation_cursor
                .as_deref()
                .and_then(|cursor| parse_cursor(cursor, file_id))
                .unwrap_or(0)
        } else {
            0
        };
        let mut patch = read_patch_page(&snapshot, file_id, start_hunk, remaining).await?;
        let mut encoded_bytes = serde_json::to_vec(&patch)
            .map_err(|_| corrupt_data_error())?
            .len();
        if encoded_bytes > remaining && !patches.is_empty() {
            response_cursor = Some(format!("{file_id}/{start_hunk}"));
            break;
        }
        if encoded_bytes > remaining {
            patch = unavailable_patch(file_id);
            encoded_bytes = serde_json::to_vec(&patch)
                .map_err(|_| corrupt_data_error())?
                .len();
        }
        remaining = remaining.saturating_sub(encoded_bytes);
        response_cursor = patch.continuation_cursor.clone();
        patches.push(patch);
        if response_cursor.is_some() {
            break;
        }
        if index + 1 < requested.len() && remaining < MIN_PATCH_PAGE_BYTES {
            response_cursor = Some(format!("{}/0", requested[index + 1]));
            break;
        }
    }
    Ok(ChatReviewPatchesRead {
        patches,
        continuation_cursor: response_cursor,
    })
}

pub async fn apply_review_action_service(
    authorizer: &dyn ReviewWorkspaceAuthorizer,
    state: &ChatReviewRegistry,
    mutations: &ChatWorkspaceMutationRegistry,
    changes: &dyn ChatWorkspaceChangeEmitter,
    pool: &SqlitePool,
    database_identity: &str,
    request: ApplyChatReviewActionRequest,
) -> ChatResult<ChatReviewActionResultRead> {
    validate_action_request(&request)?;
    let request_fingerprint = action_request_fingerprint(&request)?;
    let requested_environment = resolve_requested_environment_id(
        pool,
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    if let Some(result) = state.completed(
        &request.client_operation_id,
        &request.thread_id,
        &request.working_folder_id,
        &requested_environment,
        &request_fingerprint,
        database_identity,
    )? {
        authorize_completed_action(authorizer, pool, &request, &result).await?;
        return Ok(result);
    }
    let snapshot = state.snapshot(
        &request.snapshot_id,
        Some(&request.thread_id),
        database_identity,
    )?;
    require_request_ownership(
        pool,
        &snapshot,
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    require_snapshot_revision(&snapshot, &request.expected_review_revision)?;
    authorize_snapshot(
        authorizer,
        pool,
        &snapshot,
        WorkingFolderAuthorizationOperation::Git,
    )
    .await?;
    let _guard = mutations.try_mutation(&snapshot.root)?;
    if let Some(result) = state.completed(
        &request.client_operation_id,
        &request.thread_id,
        &request.working_folder_id,
        &requested_environment,
        &request_fingerprint,
        database_identity,
    )? {
        return Ok(result);
    }
    let mode = match &snapshot.source {
        ReviewDiffSource::WorkingTree { mode } => *mode,
        _ => return Err(ChatError::unsupported("This review source is read-only")),
    };
    let current = working_tree_material(&snapshot.root, mode).await?;
    let current_revision = review_revision(
        snapshot.thread_id.as_ref(),
        &snapshot.environment_id,
        &snapshot.source,
        &current.before_oid,
        &current.after_oid,
        snapshot.ignore_whitespace,
        snapshot.context_lines,
    )?;
    if current_revision != snapshot.review_revision {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The reviewed changes have changed. Refresh Review before applying this action.",
            true,
        ));
    }
    apply_action(&snapshot, &request, mode).await?;
    let affected_paths = action_paths(&snapshot, request.file_id.as_deref());
    changes.invalidate_paths(
        &snapshot.working_folder_id,
        request.execution_environment_id.as_deref(),
        affected_paths,
        true,
    );
    let refreshed_result = open_review(
        authorizer,
        state,
        pool,
        database_identity,
        OpenChatReviewRequest {
            thread_id: request.thread_id.clone(),
            working_folder_id: snapshot.working_folder_id.clone(),
            execution_environment_id: Some(snapshot.environment_id.clone()),
            source: snapshot.source.clone(),
            ignore_whitespace: snapshot.ignore_whitespace,
            context_lines: snapshot.context_lines,
            preferred_relative_path: request.file_id.as_ref().and_then(|file_id| {
                snapshot
                    .files
                    .iter()
                    .find(|file| &file.read.file_id == file_id)
                    .map(|file| file.read.relative_path.clone())
            }),
        },
    )
    .await;
    let mut refreshed = match refreshed_result {
        Ok(refreshed) => refreshed,
        Err(_) => stale_snapshot_read(&snapshot),
    };
    if refreshed.review_revision == snapshot.review_revision {
        refreshed.freshness = ReviewFreshness::Outdated;
    }
    let result = ChatReviewActionResultRead {
        snapshot: refreshed,
    };
    state.complete(
        request.client_operation_id,
        CompletedReviewOperation {
            database_identity: database_identity.to_string(),
            thread_id: snapshot.thread_id.clone(),
            working_folder_id: snapshot.working_folder_id.clone(),
            environment_id: snapshot.environment_id.clone(),
            request_fingerprint,
            result: result.clone(),
        },
    )?;
    Ok(result)
}

async fn optional_head(root: &Path) -> ChatResult<Option<String>> {
    match git_text(
        root,
        &["rev-parse", "-q", "--verify", "HEAD^{commit}"],
        None,
    )
    .await
    {
        Ok(head) => Ok(Some(head)),
        Err(error) if error.code == ChatErrorCode::Conflict => Ok(None),
        Err(error) => Err(error),
    }
}

async fn review_storage_text(
    root: &Path,
    arguments: &[&str],
    store: Option<&ReviewObjectStore>,
) -> ChatResult<String> {
    String::from_utf8(
        git::review_output_in_storage(
            root,
            arguments,
            None,
            store.map(|store| store.index.as_path()),
            store.map(|store| store.objects.as_path()),
            store.map(|store| store.alternate_objects.as_path()),
        )
        .await?,
    )
    .map(|value| value.trim().to_string())
    .map_err(|_| review_error("Git output is not valid UTF-8"))
}

async fn review_output(
    root: &Path,
    arguments: &[&str],
    input: Option<&[u8]>,
    store: Option<&ReviewObjectStore>,
) -> ChatResult<Vec<u8>> {
    git::review_output_in_storage(
        root,
        arguments,
        input,
        None,
        store.map(|store| store.objects.as_path()),
        store.map(|store| store.alternate_objects.as_path()),
    )
    .await
}

async fn empty_tree(root: &Path) -> ChatResult<String> {
    git_text(root, &["mktree"], Some(&[])).await
}

async fn git_text(root: &Path, arguments: &[&str], input: Option<&[u8]>) -> ChatResult<String> {
    String::from_utf8(git::review_output(root, arguments, input).await?)
        .map(|value| value.trim().to_string())
        .map_err(|_| review_error("Git output is not valid UTF-8"))
}

fn path_field(field: Option<&[u8]>) -> ChatResult<String> {
    let value = field.ok_or_else(corrupt_data_error)?;
    let path = std::str::from_utf8(value).map_err(|_| corrupt_data_error())?;
    validate_path(path)?;
    Ok(path.to_string())
}

fn parse_cursor(value: &str, file_id: &str) -> Option<usize> {
    let (cursor_file, hunk) = value.rsplit_once('/')?;
    (cursor_file == file_id)
        .then(|| hunk.parse().ok())
        .flatten()
}

fn require_snapshot_revision(snapshot: &ReviewSnapshot, revision: &str) -> ChatResult<()> {
    if snapshot.review_revision != revision {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Review snapshot revision does not match",
            true,
        ));
    }
    Ok(())
}

fn default_context_lines() -> u32 {
    3
}

pub async fn database_identity(pool: &SqlitePool) -> ChatResult<String> {
    let path: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main' LIMIT 1")
            .fetch_one(pool)
            .await
            .map_err(persistence_error)?;
    if path.is_empty() {
        return Err(persistence_error("Chat database path is unavailable"));
    }
    fs::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(persistence_error)
}

fn review_error(_message: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Git review operation failed safely",
        true,
    )
}

fn registry_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat review registry is unavailable",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat review persistence failed",
        true,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat review data is invalid",
        false,
    )
}

#[cfg(test)]
mod tests;
