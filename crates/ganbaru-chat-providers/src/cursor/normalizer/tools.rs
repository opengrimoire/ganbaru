//! Cursor ACP tool-call and diff normalization.

use super::*;

impl CursorEventNormalizer {
    pub(super) fn tool_update(
        &self,
        state: &mut CursorRouteState,
        update: &Map<String, Value>,
        source_type: &str,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let mut events = self.close_text_items(state)?;
        let item_id = text(update, "toolCallId")
            .filter(|value| is_valid_identifier(value, 512))
            .ok_or_else(|| protocol_error("tool call ID"))?
            .to_string();
        let previous = state.tools.get(&item_id).cloned();
        let kind_name = text(update, "kind").or_else(|| {
            previous.as_ref().and_then(|previous| match previous.kind {
                CanonicalItemKind::CommandExecution => Some("execute"),
                CanonicalItemKind::FileChange => Some("edit"),
                CanonicalItemKind::WebSearch => Some("search"),
                CanonicalItemKind::Reasoning => Some("think"),
                _ => None,
            })
        });
        let kind = tool_kind(kind_name);
        let title = text(update, "title")
            .map(|value| bounded_text(value, 512))
            .or_else(|| previous.as_ref().and_then(|value| value.title.clone()));
        let status = tool_status(text(update, "status"), source_type == "tool_call");
        let detail = tool_detail(update);
        let metadata = VersionedJson {
            schema_version: 1,
            value: json!({
                "toolKind": kind_name,
                "locations": safe_locations(update.get("locations"), &state.workspace),
                "inputShape": update.get("rawInput").map(safe_shape),
                "outputShape": update.get("rawOutput").map(safe_shape),
            }),
        };
        events.push(self.item_event(
            state,
            "session/update/tool",
            NormalizedItemInput {
                item_id: item_id.clone(),
                kind,
                status,
                title: title.clone(),
                detail_and_metadata: Some((detail, metadata)),
            },
        )?);
        for content in update
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(event) = self.tool_content(state, &item_id, kind, content)? {
                events.push(event);
            }
        }
        if matches!(status, ActivityStatus::Completed | ActivityStatus::Failed) {
            state.tools.remove(&item_id);
        } else {
            state.tools.insert(item_id, ToolState { kind, title });
        }
        Ok(events)
    }

    fn tool_content(
        &self,
        state: &CursorRouteState,
        item_id: &str,
        kind: CanonicalItemKind,
        content: &Value,
    ) -> ChatResult<Option<CanonicalRuntimeEvent>> {
        let object = match content.as_object() {
            Some(object) => object,
            None => return Ok(None),
        };
        match text(object, "type").unwrap_or("unknown") {
            "content" => {
                let nested = object.get("content").and_then(Value::as_object);
                let delta = nested
                    .filter(|nested| text(nested, "type") == Some("text"))
                    .and_then(|nested| text(nested, "text"));
                Ok(delta
                    .map(|delta| {
                        self.event(
                            state,
                            "session/update/tool_content",
                            ProviderItemId::new(item_id.to_string()).ok(),
                            None,
                            CanonicalEvent::ContentDelta(ContentDeltaEvent {
                                item_id: item_id.to_string(),
                                stream_kind: if kind == CanonicalItemKind::FileChange {
                                    ContentStreamKind::FileChangeOutput
                                } else {
                                    ContentStreamKind::CommandOutput
                                },
                                content_index: 0,
                                delta: bounded_text(delta, MAX_PROTOCOL_TEXT_BYTES),
                            }),
                        )
                    })
                    .transpose()?)
            }
            "diff" => {
                let path =
                    text(object, "path").and_then(|path| relative_path(&state.workspace, path));
                Ok(path
                    .map(|relative_path| {
                        self.event(
                            state,
                            "session/update/diff",
                            ProviderItemId::new(item_id.to_string()).ok(),
                            None,
                            CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
                                source: format!("{}-acp", self.provider_family_id),
                                files: vec![ChangedFileSummary {
                                    relative_path,
                                    previous_relative_path: None,
                                    additions: None,
                                    deletions: None,
                                    binary: false,
                                    status: "modified".to_string(),
                                }],
                                provider_diff: diff_text(object),
                            }),
                        )
                    })
                    .transpose()?)
            }
            unknown => Ok(Some(self.event(
                state,
                "session/update/tool_content_unknown",
                ProviderItemId::new(item_id.to_string()).ok(),
                None,
                CanonicalEvent::Unknown(UnknownEvent {
                    source_type: format!("acp/tool_content/{unknown}"),
                    summary: format!(
                        "{} emitted an unsupported ACP tool content block",
                        self.provider_display_name
                    ),
                    safe_payload: Some(safe_shape(content)),
                }),
            )?)),
        }
    }
}

fn tool_kind(kind: Option<&str>) -> CanonicalItemKind {
    match kind {
        Some("execute") => CanonicalItemKind::CommandExecution,
        Some("edit" | "delete" | "move") => CanonicalItemKind::FileChange,
        Some("search" | "fetch") => CanonicalItemKind::WebSearch,
        Some("think") => CanonicalItemKind::Reasoning,
        _ => CanonicalItemKind::DynamicToolCall,
    }
}

fn tool_status(status: Option<&str>, initial: bool) -> ActivityStatus {
    match status {
        Some("completed") => ActivityStatus::Completed,
        Some("failed") => ActivityStatus::Failed,
        Some("in_progress" | "inProgress") => ActivityStatus::Active,
        Some("pending") | None if initial => ActivityStatus::Pending,
        _ => ActivityStatus::Active,
    }
}

fn tool_detail(update: &Map<String, Value>) -> Option<String> {
    let input = update.get("rawInput")?.as_object()?;
    input
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| input.get("path").and_then(Value::as_str))
        .map(|value| bounded_text(value, MAX_PROTOCOL_TEXT_BYTES))
}

fn safe_locations(value: Option<&Value>, workspace: &Path) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| {
            let object = location.as_object()?;
            let path = text(object, "path").and_then(|path| relative_path(workspace, path))?;
            Some(json!({ "relativePath": path, "line": object.get("line") }))
        })
        .take(128)
        .collect()
}

fn relative_path(workspace: &Path, value: &str) -> Option<String> {
    let path = Path::new(value);
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace).ok()?
    } else {
        path
    };
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn diff_text(object: &Map<String, Value>) -> Option<String> {
    let old = object
        .get("oldText")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let new = object
        .get("newText")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if old.is_empty() && new.is_empty() {
        None
    } else {
        Some(bounded_text(
            &format!("Old:\n{old}\nNew:\n{new}"),
            MAX_PROTOCOL_TEXT_BYTES,
        ))
    }
}
