//! Claude streaming content-block normalization.

use super::{
    ClaudeEventNormalizer, ClaudeItemEvent, ClaudeRouteState, MAX_TEXT_BYTES, StreamBlock,
    bounded_text, text, unsigned,
};
use crate::claude::protocol::protocol_error;
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// Byte limit of a Claude API message id that Ganbaru builds item ids from.
const MAX_MESSAGE_ID_BYTES: usize = 256;

/// The conversation a Claude frame belongs to.
///
/// Claude marks frames produced inside a subagent with the `parent_tool_use_id` of the tool call
/// that spawned it. Those frames live in the subagent's sidechain transcript, never in the root
/// conversation the user sees and resumes.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::claude) enum ClaudeStreamScope {
    Root,
    Subagent(String),
}

impl ClaudeStreamScope {
    /// Reads the scope from the top-level `parent_tool_use_id` of a frame; absent or null is root.
    pub(super) fn of(object: &Map<String, Value>) -> Self {
        match text(object, "parent_tool_use_id") {
            Some(parent) if !parent.is_empty() => Self::Subagent(parent.to_string()),
            _ => Self::Root,
        }
    }

    pub(super) fn is_root(&self) -> bool {
        matches!(self, Self::Root)
    }
}

/// Identity of one streamed API message within a scope.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::claude) enum StreamMessageKey {
    /// The API message id from `message_start`.
    Id(String),
    /// A per-normalizer sequence for messages whose start carried no usable id.
    Sequence(u64),
}

impl StreamMessageKey {
    pub(super) fn message_id(&self) -> Option<&str> {
        match self {
            Self::Id(id) => Some(id),
            Self::Sequence(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::claude) struct StreamBlockKey {
    scope: ClaudeStreamScope,
    message: StreamMessageKey,
    index: u32,
}

/// Open streamed content blocks, keyed by scope, message, and block index.
///
/// Claude restarts block indexes at zero for every API message and for every subagent, so an
/// index alone cannot tell two blocks apart.
#[derive(Clone, Debug, Default)]
pub(in crate::claude) struct ClaudeStreamState {
    blocks: HashMap<StreamBlockKey, StreamBlock>,
    current_messages: HashMap<ClaudeStreamScope, StreamMessageKey>,
    next_message_sequence: u64,
}

impl ClaudeStreamState {
    /// Starts a new message in `scope` and returns the blocks the previous message left open.
    pub(super) fn start_message(
        &mut self,
        scope: &ClaudeStreamScope,
        message_id: Option<&str>,
    ) -> Vec<StreamBlock> {
        let mut abandoned = Vec::new();
        self.blocks.retain(|key, block| {
            if &key.scope == scope {
                abandoned.push(block.clone());
                false
            } else {
                true
            }
        });
        abandoned.sort_by_key(|block| block.content_index);
        let message = match message_id.filter(|id| usable_message_id(id)) {
            Some(id) => StreamMessageKey::Id(id.to_string()),
            None => self.next_sequence(),
        };
        self.current_messages.insert(scope.clone(), message);
        abandoned
    }

    /// The message that `scope` is streaming, allocating a sequence key when no start was seen.
    pub(super) fn current_message(&mut self, scope: &ClaudeStreamScope) -> StreamMessageKey {
        if let Some(message) = self.current_messages.get(scope) {
            return message.clone();
        }
        let message = self.next_sequence();
        self.current_messages.insert(scope.clone(), message.clone());
        message
    }

    /// Forgets every scope's message and returns the blocks still open, in index order.
    pub(super) fn finish_turn(&mut self) -> Vec<StreamBlock> {
        let mut open = self
            .blocks
            .drain()
            .map(|(_, block)| block)
            .collect::<Vec<_>>();
        open.sort_by_key(|block| block.content_index);
        self.current_messages.clear();
        open
    }

    fn key(&mut self, scope: &ClaudeStreamScope, index: u32) -> StreamBlockKey {
        StreamBlockKey {
            scope: scope.clone(),
            message: self.current_message(scope),
            index,
        }
    }

    fn next_sequence(&mut self) -> StreamMessageKey {
        self.next_message_sequence = self.next_message_sequence.saturating_add(1);
        StreamMessageKey::Sequence(self.next_message_sequence)
    }
}

pub(super) fn usable_message_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_MESSAGE_ID_BYTES && !id.chars().any(char::is_control)
}

impl ClaudeEventNormalizer {
    pub(super) fn message_start(
        &self,
        state: &mut ClaudeRouteState,
        scope: &ClaudeStreamScope,
        event: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let message_id = event
            .get("message")
            .and_then(Value::as_object)
            .and_then(|message| text(message, "id"));
        let abandoned = state.stream.start_message(scope, message_id);
        let mut events = Vec::new();
        for block in abandoned.into_iter().filter(|block| block.complete_on_stop) {
            events.push(self.item_event(
                state,
                ClaudeItemEvent {
                    source: "stream/message_start",
                    item_id: block.item_id,
                    kind: block.item_kind,
                    status: ActivityStatus::Completed,
                    title: None,
                    safe_metadata: None,
                },
            )?);
        }
        Ok(events)
    }

    pub(super) fn content_block_start(
        &self,
        state: &mut ClaudeRouteState,
        scope: &ClaudeStreamScope,
        event: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let index = unsigned(event, "index").unwrap_or(0) as u32;
        let block = event
            .get("content_block")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("content block"))?;
        let key = state.stream.key(scope, index);
        let block_type = text(block, "type").unwrap_or("unknown");
        let (item_id, item_kind, stream_kind, title, complete_on_stop) = match block_type {
            "text" => (
                self.content_item_id(&key.message, index, "assistant"),
                CanonicalItemKind::AssistantMessage,
                ContentStreamKind::AssistantText,
                None,
                true,
            ),
            "thinking" | "redacted_thinking" => (
                self.content_item_id(&key.message, index, "reasoning"),
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
        if block_type == "text"
            && let Some(message_id) = key.message.message_id()
        {
            state.turn_text.mark_streamed(message_id);
        }
        state.stream.blocks.insert(
            key,
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
        scope: &ClaudeStreamScope,
        event: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let index = unsigned(event, "index").unwrap_or(0) as u32;
        let key = state.stream.key(scope, index);
        let Some(block) = state.stream.blocks.get(&key).cloned() else {
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
        scope: &ClaudeStreamScope,
        event: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let index = unsigned(event, "index").unwrap_or(0) as u32;
        let key = state.stream.key(scope, index);
        let Some(block) = state.stream.blocks.remove(&key) else {
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

    /// Item id of a text or reasoning block: stable per API message and block index when the
    /// message id is known, otherwise a per-session fallback.
    fn content_item_id(&self, message: &StreamMessageKey, index: u32, prefix: &str) -> String {
        match message.message_id() {
            Some(message_id) => self.message_item_id(message_id, index),
            None => self.fallback_item_id(prefix),
        }
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
