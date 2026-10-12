use super::value::*;
use super::*;

impl OpenCodeEventNormalizer {
    pub(super) fn message_updated(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let info = properties
            .get("info")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("message update"))?;
        let id = identifier(info, "id", "message ID")?;
        let role = text(info, "role").ok_or_else(|| protocol_error("message role"))?;
        if !matches!(role, "user" | "assistant") {
            return Err(protocol_error("message role"));
        }
        state.message_roles.insert(id.to_string(), role.to_string());
        if role != "assistant" {
            return Ok(Vec::new());
        }
        state.rollback_message_id = Some(id.to_string());
        let model_id = text(info, "modelID").and_then(|model| ModelId::new(model.to_string()).ok());
        let provider_id = text(info, "providerID").unwrap_or_default();
        let effective_model = model_id.and_then(|model| {
            if provider_id.is_empty() {
                Some(model)
            } else {
                ModelId::new(format!("{provider_id}/{}", model.as_str())).ok()
            }
        });
        let Some(effective_model) = effective_model else {
            return Ok(Vec::new());
        };
        Ok(vec![
            self.event(
                state,
                "message.updated",
                ProviderItemId::new(id.to_string()).ok(),
                None,
                CanonicalEvent::SessionConfigured(SessionConfiguredEvent {
                    session_id: self.session_id.clone(),
                    effective_modes: state.modes,
                    effective_model_id: Some(effective_model),
                    effective_model_options: text(info, "variant")
                        .map(|variant| ModelOptionSelection {
                            key: "variant".to_string(),
                            value: ModelOptionValue::Choice(bounded_text(variant, 256)),
                        })
                        .into_iter()
                        .collect(),
                }),
            )?,
        ])
    }

    pub(super) fn part_delta(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let part_id = identifier(properties, "partID", "part ID")?;
        let delta = text(properties, "delta").ok_or_else(|| protocol_error("part delta"))?;
        if delta.is_empty() {
            return Ok(Vec::new());
        }
        let current = state.part_text.entry(part_id.to_string()).or_default();
        if current.len().saturating_add(delta.len()) > MAX_TEXT_BYTES {
            return Err(protocol_error("part text size"));
        }
        current.push_str(delta);
        let stream_kind = match text(properties, "field") {
            Some("reasoning") => ContentStreamKind::ReasoningText,
            _ => ContentStreamKind::AssistantText,
        };
        Ok(vec![self.event(
            state,
            "message.part.delta",
            ProviderItemId::new(part_id.to_string()).ok(),
            None,
            CanonicalEvent::ContentDelta(ContentDeltaEvent {
                item_id: part_id.to_string(),
                stream_kind,
                content_index: 0,
                delta: bounded_text(delta, MAX_TEXT_BYTES),
            }),
        )?])
    }

    pub(super) fn part_updated(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let part = properties
            .get("part")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("message part"))?;
        let part_type = text(part, "type").ok_or_else(|| protocol_error("part type"))?;
        match part_type {
            "text" | "reasoning" => self.text_part(state, part, part_type),
            "tool" => self.tool_part(state, part),
            "file" => self.file_part(state, part),
            "patch" => self.patch_part(state, part),
            "step-finish" => self.step_finish(state, part),
            "subtask" => self.subtask(state, part),
            "compaction" => self.compaction(state, part),
            "retry" => Ok(vec![self.event(
                state,
                "message.part.updated",
                part_id(part),
                None,
                CanonicalEvent::RuntimeWarning(NotificationEvent {
                    code: "opencode_retry".to_string(),
                    title: "OpenCode retried a model request".to_string(),
                    detail: part.get("error").map(safe_shape).and_then(|value| {
                        serde_json::to_string(&value.value)
                            .ok()
                            .map(|value| bounded_text(&value, 4096))
                    }),
                }),
            )?]),
            "snapshot" | "step-start" | "agent" => Ok(Vec::new()),
            unknown => Ok(vec![self.unknown_event(
                state,
                "message.part.updated",
                unknown,
                part,
            )?]),
        }
    }

    pub(super) fn text_part(
        &self,
        state: &mut OpenCodeRouteState,
        part: &Map<String, Value>,
        part_type: &str,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(part, "id", "part ID")?;
        let text_value = text(part, "text").ok_or_else(|| protocol_error("part text"))?;
        if text_value.len() > MAX_TEXT_BYTES {
            return Err(protocol_error("part text size"));
        }
        let previous = state.part_text.get(id).cloned().unwrap_or_default();
        let delta = if text_value.starts_with(&previous) {
            &text_value[previous.len()..]
        } else if previous.starts_with(text_value) {
            ""
        } else {
            text_value
        };
        state
            .part_text
            .insert(id.to_string(), text_value.to_string());
        let kind = if part_type == "reasoning" {
            CanonicalItemKind::Reasoning
        } else {
            CanonicalItemKind::AssistantMessage
        };
        let mut events = Vec::new();
        if previous.is_empty() && !text_value.is_empty() {
            events.push(self.item_event(state, id, kind, ActivityStatus::Active, None, None)?);
        }
        if !delta.is_empty() {
            events.push(self.event(
                state,
                "message.part.updated",
                ProviderItemId::new(id.to_string()).ok(),
                None,
                CanonicalEvent::ContentDelta(ContentDeltaEvent {
                    item_id: id.to_string(),
                    stream_kind: if part_type == "reasoning" {
                        ContentStreamKind::ReasoningText
                    } else {
                        ContentStreamKind::AssistantText
                    },
                    content_index: 0,
                    delta: delta.to_string(),
                }),
            )?);
        }
        let completed = part
            .get("time")
            .and_then(Value::as_object)
            .and_then(|time| time.get("end"))
            .is_some_and(Value::is_number);
        if completed && state.completed_parts.insert(id.to_string()) {
            events.push(self.item_event(state, id, kind, ActivityStatus::Completed, None, None)?);
        }
        Ok(events)
    }

    pub(super) fn tool_part(
        &self,
        state: &OpenCodeRouteState,
        part: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = text(part, "callID")
            .or_else(|| text(part, "id"))
            .ok_or_else(|| protocol_error("tool call ID"))?;
        validate_identifier(id, "tool call ID")?;
        let tool = text(part, "tool").ok_or_else(|| protocol_error("tool name"))?;
        let state_value = part
            .get("state")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("tool state"))?;
        let status = match text(state_value, "status") {
            Some("pending") => ActivityStatus::Pending,
            Some("running") => ActivityStatus::Active,
            Some("completed") => ActivityStatus::Completed,
            Some("error") => ActivityStatus::Failed,
            _ => ActivityStatus::Unknown,
        };
        let kind = tool_kind(tool);
        let title = text(state_value, "title")
            .map(|title| bounded_text(title, 512))
            .or_else(|| Some(bounded_text(tool, 256)));
        let detail = state_value
            .get("output")
            .and_then(Value::as_str)
            .or_else(|| text(state_value, "error"))
            .map(|detail| bounded_text(detail, MAX_TEXT_BYTES));
        let input = state_value.get("input").map(safe_shape);
        Ok(vec![self.item_lifecycle_event(
            state,
            id,
            ItemLifecycleEvent {
                item_id: id.to_string(),
                kind,
                status,
                title,
                detail,
                safe_metadata: Some(VersionedJson {
                    schema_version: 1,
                    value: json!({
                        "tool": bounded_text(tool, 256),
                        "input": input.map(|value| value.value),
                    }),
                }),
            },
        )?])
    }

    pub(super) fn file_part(
        &self,
        state: &OpenCodeRouteState,
        part: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(part, "id", "file part ID")?;
        let title = text(part, "filename")
            .map(|value| bounded_text(value, 1024))
            .unwrap_or_else(|| "File context".to_string());
        Ok(vec![self.item_event(
            state,
            id,
            CanonicalItemKind::FileChange,
            ActivityStatus::Completed,
            Some(title),
            None,
        )?])
    }

    pub(super) fn patch_part(
        &self,
        state: &OpenCodeRouteState,
        part: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let files = part
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("patch files"))?;
        let summaries = changed_files_from_paths(files)?;
        Ok(vec![self.event(
            state,
            "message.part.updated",
            part_id(part),
            None,
            CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
                source: "opencode".to_string(),
                files: summaries,
                provider_diff: None,
            }),
        )?])
    }

    pub(super) fn step_finish(
        &self,
        state: &OpenCodeRouteState,
        part: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let tokens = part.get("tokens").and_then(Value::as_object);
        let cache = tokens
            .and_then(|tokens| tokens.get("cache"))
            .and_then(Value::as_object);
        let usage = ThreadUsageUpdatedEvent {
            input_tokens: tokens
                .and_then(|tokens| tokens.get("input"))
                .and_then(Value::as_u64),
            output_tokens: tokens
                .and_then(|tokens| tokens.get("output"))
                .and_then(Value::as_u64),
            cached_input_tokens: cache
                .and_then(|cache| cache.get("read"))
                .and_then(Value::as_u64),
            context_tokens: tokens
                .and_then(|tokens| tokens.get("total"))
                .and_then(Value::as_u64),
            context_limit: None,
            cost: part
                .get("cost")
                .and_then(Value::as_f64)
                .map(|amount| ProviderAttributedCost {
                    amount,
                    currency: "USD".to_string(),
                    provider_reported: true,
                }),
        };
        Ok(vec![self.event(
            state,
            "message.part.updated",
            part_id(part),
            None,
            CanonicalEvent::ThreadUsageUpdated(usage),
        )?])
    }

    pub(super) fn subtask(
        &self,
        state: &OpenCodeRouteState,
        part: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(part, "id", "subtask ID")?;
        Ok(vec![
            self.event(
                state,
                "message.part.updated",
                ProviderItemId::new(id.to_string()).ok(),
                None,
                CanonicalEvent::TaskLifecycle(TaskLifecycleEvent {
                    task_id: id.to_string(),
                    parent_task_id: None,
                    status: ActivityStatus::Active,
                    title: text(part, "description")
                        .map(|value| bounded_text(value, 512))
                        .unwrap_or_else(|| "OpenCode subtask".to_string()),
                    detail: text(part, "prompt").map(|value| bounded_text(value, 4096)),
                    safe_metadata: Some(safe_shape(&Value::Object(part.clone()))),
                }),
            )?,
        ])
    }

    pub(super) fn compaction(
        &self,
        state: &OpenCodeRouteState,
        part: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(part, "id", "compaction ID")?;
        Ok(vec![self.item_event(
            state,
            id,
            CanonicalItemKind::ContextCompaction,
            ActivityStatus::Completed,
            Some("Context compacted".to_string()),
            None,
        )?])
    }
}
