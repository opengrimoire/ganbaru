//! Closed-world organizational host tools for provider assignments.

use super::internal_mcp::{InternalMcpRunScope, generate_opaque_handle};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatFolderCapability, ChatResult};
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::HashMap;
use tokio::sync::Mutex;

mod audit;
mod authorization;
mod channels;
mod workspace;

use audit::{
    complete_audit_allowed, complete_audit_denied, denial_category, reserve_audit,
    returned_message_revisions,
};
pub(crate) use authorization::{verify_publication_scope, verify_scope};
use channels::{channel_messages, list_referenced_channels};
use workspace::{
    delete_workspace_file, list_authorized_roots, patch_workspace_file, read_workspace_file,
    search_workspace_paths, write_workspace_file,
};

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 50;
const MAX_QUERY_BYTES: usize = 500;
const MAX_CURSORS: usize = 256;
const MAX_WORKSPACE_CONTENT_BYTES: usize = 1024 * 1024;
const MAX_WORKSPACE_PATCH_EDITS: usize = 256;
const GENERIC_DENIAL: &str = "The requested organizational context is unavailable";

#[derive(Default)]
pub(crate) struct InternalMcpToolRuntime {
    cursors: Mutex<HashMap<String, OpaqueCursor>>,
}

#[derive(Clone)]
enum OpaqueCursor {
    Channel {
        source_handle: String,
        query_hash: String,
        before_created_at: String,
        before_item_id: String,
    },
    Workspace {
        root_handle: String,
        query_hash: String,
        provider_cursor: String,
    },
}

pub(crate) struct HostToolContext<'a> {
    pub app: &'a tauri::AppHandle,
    pub pool: &'a SqlitePool,
    pub thread_id: &'a ganbaru_chat_contracts::models::ChatThreadId,
    pub scope: &'a InternalMcpRunScope,
    pub runtime: &'a InternalMcpToolRuntime,
}

pub(crate) fn definitions(scope: &InternalMcpRunScope) -> Vec<Tool> {
    let mut tools = vec![
        tool(
            "chat_list_referenced_channels",
            "List the channel sources explicitly authorized for this assignment",
            json!({}),
            true,
        ),
        tool(
            "chat_search_channel_messages",
            "Search one explicitly referenced channel within its frozen authorization cutoff",
            json!({
                "sourceHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_BYTES },
                "limit": { "type": "integer", "minimum": 1, "maximum": MAX_PAGE_SIZE },
                "cursor": { "type": "string", "minLength": 1, "maxLength": 1024 }
            }),
            true,
        ),
        tool(
            "chat_read_channel_messages",
            "Read one explicitly referenced channel within its frozen authorization cutoff",
            json!({
                "sourceHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                "limit": { "type": "integer", "minimum": 1, "maximum": MAX_PAGE_SIZE },
                "cursor": { "type": "string", "minLength": 1, "maxLength": 1024 }
            }),
            true,
        ),
        tool(
            "chat_list_authorized_roots",
            "List the project folders explicitly authorized for this assignment",
            json!({}),
            true,
        ),
        tool(
            "chat_search_workspace_paths",
            "Search paths in one authorized project folder",
            json!({
                "rootHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                "query": { "type": "string", "maxLength": MAX_QUERY_BYTES },
                "limit": { "type": "integer", "minimum": 1, "maximum": MAX_PAGE_SIZE },
                "cursor": { "type": "string", "minLength": 1, "maxLength": 1024 }
            }),
            true,
        ),
        tool(
            "chat_read_workspace_file",
            "Read a bounded UTF-8 file from one authorized project folder",
            json!({
                "rootHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                "relativePath": { "type": "string", "minLength": 1, "maxLength": 4096 }
            }),
            true,
        ),
    ];
    if scope
        .folder_sources
        .iter()
        .any(|source| source.capability.rank() >= ChatFolderCapability::Edit.rank())
    {
        tools.extend([
            tool(
                "chat_write_workspace_file",
                "Replace an authorized UTF-8 file when its expected revision still matches",
                json!({
                    "rootHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                    "relativePath": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "contents": { "type": "string", "maxLength": MAX_WORKSPACE_CONTENT_BYTES },
                    "expectedRevision": { "type": "string", "minLength": 16, "maxLength": 128 }
                }),
                false,
            ),
            tool(
                "chat_create_workspace_file",
                "Create a new bounded UTF-8 file in one authorized project folder",
                json!({
                    "rootHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                    "relativePath": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "contents": { "type": "string", "maxLength": MAX_WORKSPACE_CONTENT_BYTES }
                }),
                false,
            ),
            tool(
                "chat_patch_workspace_file",
                "Apply bounded byte-range edits when the authorized file revision still matches",
                json!({
                    "rootHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                    "relativePath": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "expectedRevision": { "type": "string", "minLength": 16, "maxLength": 128 },
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_WORKSPACE_PATCH_EDITS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "startByte": { "type": "integer", "minimum": 0 },
                                "endByte": { "type": "integer", "minimum": 0 },
                                "replacement": { "type": "string", "maxLength": MAX_WORKSPACE_CONTENT_BYTES }
                            },
                            "required": ["startByte", "endByte", "replacement"],
                            "additionalProperties": false
                        }
                    }
                }),
                false,
            ),
            destructive_tool(
                "chat_delete_workspace_file",
                "Delete an authorized file when its expected revision still matches",
                json!({
                    "rootHandle": { "type": "string", "minLength": 1, "maxLength": 1024 },
                    "relativePath": { "type": "string", "minLength": 1, "maxLength": 4096 },
                    "expectedRevision": { "type": "string", "minLength": 16, "maxLength": 128 }
                }),
            ),
        ]);
    }
    tools
}

pub(crate) async fn call(
    context: HostToolContext<'_>,
    name: &str,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let request_hash = request_hash(name, arguments)?;
    let response_reservation = host_tool_is_mutating(name).then_some(MAX_RESPONSE_BYTES);
    let invocation_id = reserve_audit(
        context.pool,
        context.scope,
        name,
        &request_hash,
        response_reservation.unwrap_or(0),
    )
    .await
    .map_err(|_| generic_denial_error())?;
    let result = call_authorized(&context, name, arguments).await;
    match result {
        Ok(value) => {
            let response_bytes = serde_json::to_vec(&value)
                .map_err(|_| internal_error("encode host-tool response"))?
                .len();
            if response_bytes > MAX_RESPONSE_BYTES {
                complete_audit_denied(
                    context.pool,
                    context.scope,
                    &invocation_id,
                    "budget_exhausted",
                    host_tool_is_mutating(name),
                )
                .await?;
                return Err(generic_denial_error());
            }
            let truncated = value
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let returned_revisions = returned_message_revisions(&value)?;
            let queried_channel_sources = if name == "chat_list_referenced_channels" {
                context.scope.channel_sources.iter().collect::<Vec<_>>()
            } else {
                value
                    .get("sourceHandle")
                    .and_then(Value::as_str)
                    .and_then(|handle| {
                        context
                            .scope
                            .channel_sources
                            .iter()
                            .find(|source| source.source_handle == handle)
                    })
                    .into_iter()
                    .collect::<Vec<_>>()
            };
            complete_audit_allowed(
                context.pool,
                context.scope,
                &invocation_id,
                response_bytes,
                truncated,
                &returned_revisions,
                &queried_channel_sources,
            )
            .await?;
            Ok(value)
        }
        Err(error) => {
            let mutation_outcome_unknown = host_tool_is_mutating(name);
            complete_audit_denied(
                context.pool,
                context.scope,
                &invocation_id,
                if mutation_outcome_unknown {
                    "mutation_outcome_unknown"
                } else {
                    denial_category(&error)
                },
                mutation_outcome_unknown,
            )
            .await?;
            Err(generic_denial_error())
        }
    }
}

async fn call_authorized(
    context: &HostToolContext<'_>,
    name: &str,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    verify_scope(context.pool, context.thread_id, context.scope).await?;
    match name {
        "chat_list_referenced_channels" => list_referenced_channels(context).await,
        "chat_search_channel_messages" => channel_messages(context, arguments, true).await,
        "chat_read_channel_messages" => channel_messages(context, arguments, false).await,
        "chat_list_authorized_roots" => list_authorized_roots(context).await,
        "chat_search_workspace_paths" => search_workspace_paths(context, arguments).await,
        "chat_read_workspace_file" => read_workspace_file(context, arguments).await,
        "chat_write_workspace_file" => write_workspace_file(context, arguments, false).await,
        "chat_create_workspace_file" => write_workspace_file(context, arguments, true).await,
        "chat_patch_workspace_file" => patch_workspace_file(context, arguments).await,
        "chat_delete_workspace_file" => delete_workspace_file(context, arguments).await,
        _ => Err(generic_denial_error()),
    }
}

struct ChannelCursorRead {
    before_created_at: String,
    before_created_at_again: String,
    before_item_id: String,
}

struct TruncatedText<'a> {
    value: &'a str,
    truncated: bool,
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> TruncatedText<'_> {
    if value.len() <= maximum_bytes {
        return TruncatedText {
            value,
            truncated: false,
        };
    }
    let mut end = maximum_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    TruncatedText {
        value: &value[..end],
        truncated: true,
    }
}

impl InternalMcpToolRuntime {
    pub(crate) async fn reset_cursors(&self) {
        self.cursors.lock().await.clear();
    }

    async fn store_cursor(&self, cursor: OpaqueCursor) -> ChatResult<String> {
        let token = generate_opaque_handle("cursor")?;
        let mut cursors = self.cursors.lock().await;
        if cursors.len() >= MAX_CURSORS {
            cursors.clear();
        }
        cursors.insert(token.clone(), cursor);
        Ok(token)
    }

    async fn channel_cursor(
        &self,
        token: &str,
        source_handle: &str,
        query_hash: &str,
    ) -> ChatResult<ChannelCursorRead> {
        match self.cursors.lock().await.get(token).cloned() {
            Some(OpaqueCursor::Channel {
                source_handle: stored_source,
                query_hash: stored_query,
                before_created_at,
                before_item_id,
            }) if stored_source == source_handle && stored_query == query_hash => {
                Ok(ChannelCursorRead {
                    before_created_at_again: before_created_at.clone(),
                    before_created_at,
                    before_item_id,
                })
            }
            _ => Err(generic_denial_error()),
        }
    }

    async fn workspace_cursor(
        &self,
        token: &str,
        root_handle: &str,
        query_hash: &str,
    ) -> ChatResult<String> {
        match self.cursors.lock().await.get(token).cloned() {
            Some(OpaqueCursor::Workspace {
                root_handle: stored_root,
                query_hash: stored_query,
                provider_cursor,
            }) if stored_root == root_handle && stored_query == query_hash => Ok(provider_cursor),
            _ => Err(generic_denial_error()),
        }
    }
}

fn tool(name: &'static str, description: &'static str, properties: Value, read_only: bool) -> Tool {
    configured_tool(name, description, properties, read_only, false)
}

fn destructive_tool(name: &'static str, description: &'static str, properties: Value) -> Tool {
    configured_tool(name, description, properties, false, true)
}

fn configured_tool(
    name: &'static str,
    description: &'static str,
    properties: Value,
    read_only: bool,
    destructive: bool,
) -> Tool {
    let properties = properties.as_object().cloned().unwrap_or_default();
    let required = properties
        .keys()
        .filter(|key| !matches!(key.as_str(), "cursor" | "limit"))
        .cloned()
        .map(Value::String)
        .collect::<Vec<_>>();
    let schema = json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    Tool::new(name, description, schema).with_annotations(
        ToolAnnotations::new()
            .read_only(read_only)
            .destructive(destructive)
            .open_world(false),
    )
}

fn required_string<'a>(
    arguments: &'a Map<String, Value>,
    name: &str,
    maximum_bytes: usize,
) -> ChatResult<&'a str> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| value.len() <= maximum_bytes)
        .filter(|value| !value.contains('\0') && !value.chars().any(char::is_control))
        .ok_or_else(generic_denial_error)
}

fn optional_string<'a>(
    arguments: &'a Map<String, Value>,
    name: &str,
    maximum_bytes: usize,
) -> ChatResult<Option<&'a str>> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => required_string(arguments, name, maximum_bytes).map(Some),
    }
}

fn optional_limit(arguments: &Map<String, Value>) -> ChatResult<Option<u32>> {
    match arguments.get("limit") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| (1..=MAX_PAGE_SIZE).contains(value))
            .map(Some)
            .ok_or_else(generic_denial_error),
    }
}

fn request_hash(name: &str, arguments: &Map<String, Value>) -> ChatResult<String> {
    let encoded = serde_json::to_vec(&(name, arguments))
        .map_err(|_| internal_error("encode host-tool request"))?;
    Ok(sha256_hex(&encoded))
}

fn sha256_hex(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn wire_folder_capability(capability: ChatFolderCapability) -> &'static str {
    match capability {
        ChatFolderCapability::None => "none",
        ChatFolderCapability::Read => "read",
        ChatFolderCapability::Edit => "edit",
        ChatFolderCapability::Execute => "execute",
        ChatFolderCapability::Publish => "publish",
    }
}

fn host_tool_is_mutating(name: &str) -> bool {
    matches!(
        name,
        "chat_write_workspace_file"
            | "chat_create_workspace_file"
            | "chat_patch_workspace_file"
            | "chat_delete_workspace_file"
    )
}

fn generic_denial_error() -> ChatError {
    ChatError::new(ChatErrorCode::Permission, GENERIC_DENIAL, false)
}

fn internal_error(operation: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        format!("Could not {operation}"),
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Organizational host-tool authorization could not be verified",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_truncation_preserves_utf8_at_byte_boundaries() {
        let text = "aéz";
        for (maximum, expected, truncated) in [
            (0, "", true),
            (1, "a", true),
            (2, "a", true),
            (3, "aé", true),
            (4, text, false),
            (5, text, false),
        ] {
            let result = truncate_utf8(text, maximum);
            assert_eq!(result.value, expected);
            assert_eq!(result.truncated, truncated);
        }
    }

    #[tokio::test]
    async fn cursors_are_bound_to_their_source_query_and_tool_family() {
        let runtime = InternalMcpToolRuntime::default();
        let channel = runtime
            .store_cursor(OpaqueCursor::Channel {
                source_handle: "source".to_string(),
                query_hash: "query".to_string(),
                before_created_at: "2026-08-17T00:00:00.000Z".to_string(),
                before_item_id: "item".to_string(),
            })
            .await
            .unwrap();
        let workspace = runtime
            .store_cursor(OpaqueCursor::Workspace {
                root_handle: "root".to_string(),
                query_hash: "query".to_string(),
                provider_cursor: "position".to_string(),
            })
            .await
            .unwrap();
        assert!(
            runtime
                .channel_cursor(&channel, "source", "query")
                .await
                .is_ok()
        );
        assert_eq!(
            runtime
                .workspace_cursor(&workspace, "root", "query")
                .await
                .unwrap(),
            "position"
        );
        for (source, query) in [("other", "query"), ("source", "other")] {
            assert!(
                runtime
                    .channel_cursor(&channel, source, query)
                    .await
                    .is_err()
            );
        }
        for (root, query) in [("other", "query"), ("root", "other")] {
            assert!(
                runtime
                    .workspace_cursor(&workspace, root, query)
                    .await
                    .is_err()
            );
        }
        assert!(
            runtime
                .workspace_cursor(&channel, "source", "query")
                .await
                .is_err()
        );
        assert!(
            runtime
                .channel_cursor(&workspace, "root", "query")
                .await
                .is_err()
        );
        runtime.reset_cursors().await;
        assert!(
            runtime
                .workspace_cursor(&workspace, "root", "query")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn activating_a_scope_discards_opaque_cursors() {
        let runtime = InternalMcpToolRuntime::default();
        let cursor = runtime
            .store_cursor(OpaqueCursor::Channel {
                source_handle: "source".to_string(),
                query_hash: "query".to_string(),
                before_created_at: "2026-08-17T00:00:00.000Z".to_string(),
                before_item_id: "item".to_string(),
            })
            .await
            .expect("cursor");
        runtime
            .channel_cursor(&cursor, "source", "query")
            .await
            .expect("active cursor");
        runtime.reset_cursors().await;
        assert!(
            runtime
                .channel_cursor(&cursor, "source", "query")
                .await
                .is_err()
        );
    }
}
