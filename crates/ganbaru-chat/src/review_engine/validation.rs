//! Validation for review requests, Git references, and workspace paths.

use super::super::models::{ChatError, ChatResult};
use super::contracts::{
    ApplyChatReviewActionRequest, OpenChatReviewRequest, ReadChatReviewPatchesRequest,
    ReviewDiffSource,
};
use std::path::Path;

pub const MAX_CONTEXT_LINES: u32 = 100;
const MAX_FILE_SELECTION: usize = 100;

pub fn source_requires_thread(source: &ReviewDiffSource) -> bool {
    matches!(
        source,
        ReviewDiffSource::Checkpoint { .. } | ReviewDiffSource::ProviderTurn { .. }
    )
}

pub fn thread_required_error() -> ChatError {
    ChatError::validation(
        "threadId",
        "This review source requires an existing Chat thread",
    )
}

pub fn validate_open_request(request: &OpenChatReviewRequest) -> ChatResult<()> {
    if source_requires_thread(&request.source) && request.thread_id.is_none() {
        return Err(thread_required_error());
    }
    if request.context_lines > MAX_CONTEXT_LINES {
        return Err(ChatError::validation(
            "contextLines",
            "Review context is too large",
        ));
    }
    if let Some(path) = request.preferred_relative_path.as_deref() {
        validate_path(path)?;
    }
    match &request.source {
        ReviewDiffSource::Commit { revision } => validate_reference(revision, "revision")?,
        ReviewDiffSource::Branch {
            base_ref, head_ref, ..
        } => {
            if let Some(base_ref) = base_ref {
                validate_reference(base_ref, "baseRef")?;
            }
            validate_reference(head_ref, "headRef")?;
        }
        ReviewDiffSource::ChangeRequest {
            provider,
            repository_slug,
            number,
        } => {
            if !matches!(
                provider.as_str(),
                "github" | "gitlab" | "azure_devops" | "bitbucket"
            ) {
                return Err(ChatError::validation(
                    "provider",
                    "Hosted source-control provider is invalid",
                ));
            }
            if repository_slug.is_empty()
                || repository_slug.len() > 2_048
                || repository_slug.contains('\0')
                || repository_slug.chars().any(char::is_control)
                || *number == 0
            {
                return Err(ChatError::validation(
                    "repositorySlug",
                    "Hosted repository reference is invalid",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_patch_request(request: &ReadChatReviewPatchesRequest) -> ChatResult<()> {
    validate_opaque_id(&request.snapshot_id, "snapshotId")?;
    validate_revision(&request.review_revision)?;
    if request.file_ids.is_empty() || request.file_ids.len() > MAX_FILE_SELECTION {
        return Err(ChatError::validation(
            "fileIds",
            "Review patch requests require between one and 100 files",
        ));
    }
    for file_id in &request.file_ids {
        validate_opaque_id(file_id, "fileId")?;
    }
    if let Some(cursor) = request.continuation_cursor.as_deref() {
        validate_opaque_id(cursor, "continuationCursor")?;
    }
    Ok(())
}

pub fn validate_action_request(request: &ApplyChatReviewActionRequest) -> ChatResult<()> {
    validate_opaque_id(&request.snapshot_id, "snapshotId")?;
    validate_revision(&request.expected_review_revision)?;
    validate_opaque_id(&request.client_operation_id, "clientOperationId")?;
    if let Some(file_id) = request.file_id.as_deref() {
        validate_opaque_id(file_id, "fileId")?;
    }
    if request.hunk_ids.len() > 1_000 || (!request.hunk_ids.is_empty() && request.file_id.is_none())
    {
        return Err(ChatError::validation(
            "hunkIds",
            "Review hunk selection is invalid",
        ));
    }
    for hunk_id in &request.hunk_ids {
        validate_opaque_id(hunk_id, "hunkId")?;
    }
    Ok(())
}

fn validate_revision(value: &str) -> ChatResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ChatError::validation(
            "reviewRevision",
            "Review revision is invalid",
        ));
    }
    Ok(())
}

fn validate_opaque_id(value: &str, field: &str) -> ChatResult<()> {
    if value.is_empty()
        || value.len() > 1_024
        || value.chars().any(char::is_control)
        || value.chars().any(char::is_whitespace)
    {
        return Err(ChatError::validation(field, "Review identifier is invalid"));
    }
    Ok(())
}

pub fn validate_reference(value: &str, field: &str) -> ChatResult<()> {
    if value.is_empty()
        || value.len() > 1_024
        || value.starts_with('-')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(ChatError::validation(field, "Git reference is invalid"));
    }
    Ok(())
}

pub fn validate_path(path: &str) -> ChatResult<()> {
    if path.is_empty()
        || path.len() > 4_096
        || path.starts_with('/')
        || path.contains('\0')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        || !Path::new(path)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ChatError::validation(
            "relativePath",
            "Review path is invalid",
        ));
    }
    Ok(())
}
