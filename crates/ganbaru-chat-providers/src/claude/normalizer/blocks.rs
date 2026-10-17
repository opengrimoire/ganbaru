//! Claude streaming content-block normalization.

use super::{
    ClaudeEventNormalizer, ClaudeItemEvent, ClaudeRouteState, MAX_TEXT_BYTES, StreamBlock,
    bounded_text, text, unsigned,
};
use crate::claude::protocol::protocol_error;
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};

impl ClaudeEventNormalizer {
    pub(super) fn content_block_start(
        &self,
        state: &mut ClaudeRouteState,
        event: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let index = unsigned(event, "index").unwrap_or(0) as u32;
        let block = event
            .get("content_block")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("content block"))?;
        let block_type = text(block, "type").unwrap_or("unknown");
        let (item_id, item_kind, stream_kind, title, complete_on_stop) = match block_type {
            "text" => (
                self.fallback_item_id("assistant"),
                CanonicalItemKind::AssistantMessage,
                ContentStreamKind::AssistantText,
                None,
                true,
            ),
            "thinking" | "redacted_thinking" => (
                self.fallback_item_id("reasoning"),
                CanonicalItemKind::Reasoning,
                ContentStreamKind::ReasoningText,
                Some("Reasoning".to_string()),
                true,
            ),
            "tool_use" | "server_tool_use" | "mcp_tool_use" => {
                let name = text(block, "name").unwrap_or("Claude tool");
                let id = text(block, "id")
                    .map(str::to_string)
                    .unwrap_or_else(|| self.fallback_item_id("tool"));
                (
                    id,
                    classify_tool(name),
                    tool_stream_kind(name),
                    Some(name.to_string()),
                    false,
                )
            }
            "web_search_tool_result" => (
                text(block, "tool_use_id")
                    .map(str::to_string)
                    .unwrap_or_else(|| self.fallback_item_id("web-result")),
                CanonicalItemKind::WebSearch,
                ContentStreamKind::Unknown,
                Some("Web search result".to_string()),
                true,
            ),
            "mcp_tool_result" => (
                text(block, "tool_use_id")
                    .map(str::to_string)
                    .unwrap_or_else(|| self.fallback_item_id("mcp-result")),
                CanonicalItemKind::McpToolCall,
                ContentStreamKind::Unknown,
                Some("MCP tool result".to_string()),
                true,
            ),
            _ => (
                self.fallback_item_id("unknown"),
                CanonicalItemKind::Unknown,
                ContentStreamKind::Unknown,
                Some(block_type.to_string()),
                true,
            ),
        };
        if !complete_on_stop {
            state.tool_kinds.insert(item_id.clone(), item_kind);
        }
        state.stream_blocks.insert(
            index,
            StreamBlock {
                item_id: item_id.clone(),
                item_kind,
                stream_kind,
                content_index: index,
                complete_on_stop,
            },
        );
        Ok(vec![self.item_event(
            state,
            ClaudeItemEvent {
                source: "stream/content_block_start",
                item_id,
                kind: item_kind,
                status: ActivityStatus::Active,
                title,
                safe_metadata: Some(VersionedJson {
                    schema_version: 1,
                    value: json!({
                        "blockType": block_type,
                        "toolInput": block.get("input").filter(|value| value.is_object()),
                    }),
                }),
            },
        )?])
    }

    pub(super) fn content_block_delta(
        &self,
        state: &mut ClaudeRouteState,
        event: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let index = unsigned(event, "index").unwrap_or(0) as u32;
        let Some(block) = state.stream_blocks.get(&index).cloned() else {
            return Ok(Vec::new());
        };
        let delta = event
            .get("delta")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("content delta"))?;
        let text_delta = text(delta, "text")
            .or_else(|| text(delta, "thinking"))
            .or_else(|| text(delta, "partial_json"));
        let Some(text_delta) = text_delta else {
            return Ok(Vec::new());
        };
        Ok(vec![self.event(
            state,
            "stream/content_block_delta",
            None,
            None,
            ProviderItemId::new(block.item_id.clone()).ok(),
            CanonicalEvent::ContentDelta(ContentDeltaEvent {
                item_id: block.item_id,
                stream_kind: block.stream_kind,
                content_index: block.content_index,
                delta: bounded_text(text_delta, MAX_TEXT_BYTES),
            }),
        )?])
    }

    pub(super) fn content_block_stop(
        &self,
        state: &mut ClaudeRouteState,
        event: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let index = unsigned(event, "index").unwrap_or(0) as u32;
        let Some(block) = state.stream_blocks.remove(&index) else {
            return Ok(Vec::new());
        };
        if !block.complete_on_stop {
            return Ok(Vec::new());
        }
        Ok(vec![self.item_event(
            state,
            ClaudeItemEvent {
                source: "stream/content_block_stop",
                item_id: block.item_id,
                kind: block.item_kind,
                status: ActivityStatus::Completed,
                title: None,
                safe_metadata: None,
            },
        )?])
    }
}

fn classify_tool(name: &str) -> CanonicalItemKind {
    match name {
        "Bash" => CanonicalItemKind::CommandExecution,
        "Edit" | "Write" | "NotebookEdit" => CanonicalItemKind::FileChange,
        "WebSearch" | "WebFetch" => CanonicalItemKind::WebSearch,
        "Task" | "Agent" | "TaskCreate" | "TaskUpdate" => CanonicalItemKind::CollaborationTask,
        "ExitPlanMode" | "TodoWrite" => CanonicalItemKind::Plan,
        name if name.starts_with("mcp__") => CanonicalItemKind::McpToolCall,
        _ => CanonicalItemKind::DynamicToolCall,
    }
}

fn tool_stream_kind(name: &str) -> ContentStreamKind {
    match name {
        "Bash" => ContentStreamKind::CommandOutput,
        "Edit" | "Write" | "NotebookEdit" => ContentStreamKind::FileChangeOutput,
        "ExitPlanMode" | "TodoWrite" => ContentStreamKind::PlanText,
        _ => ContentStreamKind::Unknown,
    }
}
