//! Bounded workspace operations behind the current assignment's folder grants.

use super::authorization::verify_folder_source;
use super::{
    DEFAULT_PAGE_SIZE, HostToolContext, MAX_QUERY_BYTES, MAX_WORKSPACE_CONTENT_BYTES,
    MAX_WORKSPACE_PATCH_EDITS, OpaqueCursor, generic_denial_error, internal_error, optional_limit,
    optional_string, required_string, sha256_hex, truncate_utf8, wire_folder_capability,
};
use crate::chat::internal_mcp::{InternalMcpFolderSource, InternalMcpRunScope};
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatFolderCapability, ChatResult, ChatRuntimeApprovalPolicy,
};
use serde_json::{Map, Value, json};
use tauri::Manager;

pub(super) async fn list_authorized_roots(context: &HostToolContext<'_>) -> ChatResult<Value> {
    let mut roots = Vec::with_capacity(context.scope.folder_sources.len());
    for source in &context.scope.folder_sources {
        verify_folder_source(context.pool, context.scope, source).await?;
        roots.push(json!({
            "rootHandle": source.root_handle,
            "capability": wire_folder_capability(source.capability),
            "isExecutionTarget": source.is_execution_target,
        }));
    }
    Ok(json!({ "roots": roots, "truncated": false }))
}

pub(super) async fn search_workspace_paths(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let root_handle = required_string(arguments, "rootHandle", 1024)?;
    let source = folder_source(context.scope, root_handle, ChatFolderCapability::Read)?;
    verify_folder_source(context.pool, context.scope, source).await?;
    let authorized = authorize_folder(context, source, false).await?;
    let query = required_string(arguments, "query", MAX_QUERY_BYTES)?;
    let query_hash = sha256_hex(query.trim().to_lowercase().as_bytes());
    let provider_cursor = match optional_string(arguments, "cursor", 1024)? {
        Some(cursor) => Some(
            context
                .runtime
                .workspace_cursor(cursor, root_handle, &query_hash)
                .await?,
        ),
        None => None,
    };
    let limit = optional_limit(arguments)?.unwrap_or(DEFAULT_PAGE_SIZE);
    let root = authorized.canonical_path;
    let query_owned = query.to_string();
    let page = tauri::async_runtime::spawn_blocking(move || {
        ganbaru_chat::composer::workspace_mentions::search_workspace_paths(
            &root,
            &query_owned,
            false,
            provider_cursor.as_deref(),
            limit,
        )
    })
    .await
    .map_err(|_| internal_error("search authorized workspace"))??;
    let next_cursor = match page.next_cursor {
        Some(provider_cursor) => Some(
            context
                .runtime
                .store_cursor(OpaqueCursor::Workspace {
                    root_handle: root_handle.to_string(),
                    query_hash,
                    provider_cursor,
                })
                .await?,
        ),
        None => None,
    };
    let truncated = next_cursor.is_some();
    Ok(json!({
        "rootHandle": root_handle,
        "entries": page.entries,
        "nextCursor": next_cursor,
        "truncated": truncated,
    }))
}

pub(super) async fn read_workspace_file(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let root_handle = required_string(arguments, "rootHandle", 1024)?;
    let source = folder_source(context.scope, root_handle, ChatFolderCapability::Read)?;
    verify_folder_source(context.pool, context.scope, source).await?;
    let authorized = authorize_folder(context, source, false).await?;
    let relative_path = required_string(arguments, "relativePath", 4096)?.to_string();
    let preview = tauri::async_runtime::spawn_blocking(move || {
        crate::chat::workspace::files::preview_workspace_file(&authorized, &relative_path)
    })
    .await
    .map_err(|_| internal_error("read authorized workspace file"))??;
    if preview.binary || preview.oversized || preview.text.is_none() {
        return Err(generic_denial_error());
    }
    let text = preview.text.as_deref().ok_or_else(generic_denial_error)?;
    let text = truncate_utf8(text, 48 * 1024);
    Ok(json!({
        "rootHandle": root_handle,
        "relativePath": preview.relative_path,
        "normalizedText": text.value,
        "byteSize": preview.byte_size,
        "lineCount": preview.line_count,
        "contentRevision": preview.content_revision,
        "truncated": text.truncated,
    }))
}

pub(super) async fn write_workspace_file(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
    create: bool,
) -> ChatResult<Value> {
    let root_handle = required_string(arguments, "rootHandle", 1024)?;
    let source = folder_source(context.scope, root_handle, ChatFolderCapability::Edit)?;
    require_resolved_mutation_approval(source.runtime_approval_policy)?;
    verify_folder_source(context.pool, context.scope, source).await?;
    let authorized = authorize_folder(context, source, true).await?;
    let relative_path = required_string(arguments, "relativePath", 4096)?.to_string();
    let invalidation_path = relative_path.clone();
    let contents =
        bounded_text_argument(arguments, "contents", MAX_WORKSPACE_CONTENT_BYTES)?.to_string();
    let expected_revision = if create {
        None
    } else {
        Some(required_string(arguments, "expectedRevision", 128)?.to_string())
    };
    let preview = tauri::async_runtime::spawn_blocking(move || match expected_revision {
        Some(expected_revision) => crate::chat::workspace::files::save_workspace_file(
            &authorized,
            &relative_path,
            &contents,
            &expected_revision,
        ),
        None => crate::chat::workspace::files::recreate_workspace_file(
            &authorized,
            &relative_path,
            &contents,
            true,
        ),
    })
    .await
    .map_err(|_| internal_error("write authorized workspace file"))??;
    invalidate_workspace_path(context, source, invalidation_path);
    Ok(json!({
        "rootHandle": root_handle,
        "relativePath": preview.relative_path,
        "byteSize": preview.byte_size,
        "lineCount": preview.line_count,
        "contentRevision": preview.content_revision,
        "created": create,
        "truncated": false,
    }))
}

pub(super) async fn patch_workspace_file(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let root_handle = required_string(arguments, "rootHandle", 1024)?;
    let source = folder_source(context.scope, root_handle, ChatFolderCapability::Edit)?;
    require_resolved_mutation_approval(source.runtime_approval_policy)?;
    verify_folder_source(context.pool, context.scope, source).await?;
    let authorized = authorize_folder(context, source, true).await?;
    let relative_path = required_string(arguments, "relativePath", 4096)?.to_string();
    let expected_revision = required_string(arguments, "expectedRevision", 128)?.to_string();
    let preview_authorization = authorized.clone();
    let preview_path = relative_path.clone();
    let preview = tauri::async_runtime::spawn_blocking(move || {
        crate::chat::workspace::files::preview_workspace_file(&preview_authorization, &preview_path)
    })
    .await
    .map_err(|_| internal_error("read authorized workspace file for patching"))??;
    let current_revision = preview
        .content_revision
        .as_deref()
        .ok_or_else(generic_denial_error)?;
    if current_revision != expected_revision {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The workspace file changed before the patch was applied",
            true,
        ));
    }
    let current = preview.text.ok_or_else(generic_denial_error)?;
    let edits = workspace_patch_edits(arguments, &current)?;
    let mut patched = current;
    for edit in edits.iter().rev() {
        patched.replace_range(edit.start_byte..edit.end_byte, &edit.replacement);
    }
    let invalidation_path = relative_path.clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        crate::chat::workspace::files::save_workspace_file(
            &authorized,
            &relative_path,
            &patched,
            &expected_revision,
        )
    })
    .await
    .map_err(|_| internal_error("patch authorized workspace file"))??;
    invalidate_workspace_path(context, source, invalidation_path);
    Ok(json!({
        "rootHandle": root_handle,
        "relativePath": saved.relative_path,
        "byteSize": saved.byte_size,
        "lineCount": saved.line_count,
        "contentRevision": saved.content_revision,
        "patched": true,
        "truncated": false,
    }))
}

pub(super) async fn delete_workspace_file(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let root_handle = required_string(arguments, "rootHandle", 1024)?;
    let source = folder_source(context.scope, root_handle, ChatFolderCapability::Edit)?;
    require_resolved_mutation_approval(source.runtime_approval_policy)?;
    verify_folder_source(context.pool, context.scope, source).await?;
    let authorized = authorize_folder(context, source, true).await?;
    let relative_path = required_string(arguments, "relativePath", 4096)?.to_string();
    let expected_revision = required_string(arguments, "expectedRevision", 128)?.to_string();
    let invalidation_path = relative_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::chat::workspace::files::delete_workspace_file(
            &authorized,
            &relative_path,
            &expected_revision,
        )
    })
    .await
    .map_err(|_| internal_error("delete authorized workspace file"))??;
    invalidate_workspace_path(context, source, invalidation_path.clone());
    Ok(json!({
        "rootHandle": root_handle,
        "relativePath": invalidation_path,
        "deleted": true,
        "truncated": false,
    }))
}

fn workspace_patch_edits(
    arguments: &Map<String, Value>,
    current: &str,
) -> ChatResult<Vec<WorkspacePatchEdit>> {
    let values = arguments
        .get("edits")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty() && values.len() <= MAX_WORKSPACE_PATCH_EDITS)
        .ok_or_else(generic_denial_error)?;
    let mut edits = Vec::with_capacity(values.len());
    let mut next_minimum = 0_usize;
    let mut previous_start = None;
    let mut resulting_bytes = current.len();
    for value in values {
        let edit = value.as_object().ok_or_else(generic_denial_error)?;
        if edit.len() != 3 {
            return Err(generic_denial_error());
        }
        let start_byte = bounded_usize(edit, "startByte")?;
        let end_byte = bounded_usize(edit, "endByte")?;
        let replacement = bounded_text_argument(edit, "replacement", MAX_WORKSPACE_CONTENT_BYTES)?;
        if start_byte > end_byte
            || end_byte > current.len()
            || start_byte < next_minimum
            || previous_start == Some(start_byte)
            || !current.is_char_boundary(start_byte)
            || !current.is_char_boundary(end_byte)
        {
            return Err(generic_denial_error());
        }
        resulting_bytes = resulting_bytes
            .checked_sub(end_byte - start_byte)
            .and_then(|value| value.checked_add(replacement.len()))
            .filter(|value| *value <= MAX_WORKSPACE_CONTENT_BYTES)
            .ok_or_else(generic_denial_error)?;
        edits.push(WorkspacePatchEdit {
            start_byte,
            end_byte,
            replacement: replacement.to_string(),
        });
        next_minimum = end_byte;
        previous_start = Some(start_byte);
    }
    Ok(edits)
}

fn bounded_usize(arguments: &Map<String, Value>, name: &str) -> ChatResult<usize> {
    arguments
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(generic_denial_error)
}

fn require_resolved_mutation_approval(policy: ChatRuntimeApprovalPolicy) -> ChatResult<()> {
    if matches!(
        policy,
        ChatRuntimeApprovalPolicy::AutoApprove | ChatRuntimeApprovalPolicy::Unattended
    ) {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "This host-tool mutation requires a resolved runtime approval",
            true,
        ))
    }
}

fn invalidate_workspace_path(
    context: &HostToolContext<'_>,
    source: &InternalMcpFolderSource,
    relative_path: String,
) {
    context
        .app
        .state::<crate::chat::workspace::observer::ChatWorkspaceObserverRegistry>()
        .invalidate_paths(
            &source.working_folder_id,
            source
                .is_execution_target
                .then_some(context.scope.execution_environment_id.as_deref())
                .flatten(),
            vec![relative_path],
            false,
        );
}

fn folder_source<'a>(
    scope: &'a InternalMcpRunScope,
    root_handle: &str,
    minimum: ChatFolderCapability,
) -> ChatResult<&'a InternalMcpFolderSource> {
    scope
        .folder_sources
        .iter()
        .find(|source| {
            source.root_handle == root_handle && source.capability.rank() >= minimum.rank()
        })
        .ok_or_else(generic_denial_error)
}

async fn authorize_folder(
    context: &HostToolContext<'_>,
    source: &InternalMcpFolderSource,
    write: bool,
) -> ChatResult<crate::chat::workspace::AuthorizedWorkingFolder> {
    let operation = if write {
        WorkingFolderAuthorizationOperation::FileWrite
    } else {
        WorkingFolderAuthorizationOperation::FileRead
    };
    let authorized = crate::chat::workspace::commands::authorize_working_folder(
        context.app,
        context.pool,
        &source.working_folder_id,
        operation,
    )
    .await?;
    let environment_id = source
        .is_execution_target
        .then_some(context.scope.execution_environment_id.as_deref())
        .flatten();
    crate::chat::execution_environment::resolve_environment_workspace(
        context.app,
        context.pool,
        authorized,
        environment_id,
    )
    .await
}

fn bounded_text_argument<'a>(
    arguments: &'a Map<String, Value>,
    name: &str,
    maximum_bytes: usize,
) -> ChatResult<&'a str> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| value.len() <= maximum_bytes && !value.contains('\0'))
        .ok_or_else(generic_denial_error)
}

struct WorkspacePatchEdit {
    start_byte: usize,
    end_byte: usize,
    replacement: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_patch_edits_require_ordered_utf8_byte_ranges() {
        let arguments = json!({
            "edits": [
                { "startByte": 1, "endByte": 3, "replacement": "o" },
                { "startByte": 3, "endByte": 3, "replacement": "!" }
            ]
        });
        let edits = workspace_patch_edits(arguments.as_object().unwrap(), "aéz")
            .expect("valid UTF-8 patch");
        let mut patched = "aéz".to_string();
        for edit in edits.iter().rev() {
            patched.replace_range(edit.start_byte..edit.end_byte, &edit.replacement);
        }
        assert_eq!(patched, "ao!z");

        let split_codepoint = json!({
            "edits": [{ "startByte": 2, "endByte": 3, "replacement": "x" }]
        });
        assert!(workspace_patch_edits(split_codepoint.as_object().unwrap(), "aéz").is_err());
        let overlapping = json!({
            "edits": [
                { "startByte": 0, "endByte": 2, "replacement": "x" },
                { "startByte": 1, "endByte": 3, "replacement": "y" }
            ]
        });
        assert!(workspace_patch_edits(overlapping.as_object().unwrap(), "abcd").is_err());
    }

    #[test]
    fn workspace_mutations_require_a_resolved_noninteractive_approval() {
        assert!(require_resolved_mutation_approval(ChatRuntimeApprovalPolicy::AutoApprove).is_ok());
        assert!(require_resolved_mutation_approval(ChatRuntimeApprovalPolicy::Unattended).is_ok());

        let ask = require_resolved_mutation_approval(ChatRuntimeApprovalPolicy::Ask)
            .expect_err("ask must remain fail closed without a durable host-tool approval");
        assert_eq!(ask.code, ChatErrorCode::Permission);

        let provider_custom =
            require_resolved_mutation_approval(ChatRuntimeApprovalPolicy::ProviderCustom)
                .expect_err("provider custom cannot widen a brokered folder grant");
        assert_eq!(provider_custom.code, ChatErrorCode::Permission);
    }
}
