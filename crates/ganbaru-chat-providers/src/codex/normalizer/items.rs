use super::diff::*;
use super::value::*;
use super::*;

impl CodexEventNormalizer {
    pub(super) fn plan_updated(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "turn/plan/updated")?;
        let steps = object
            .get("plan")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("turn/plan/updated.plan"))?
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                let value = value.as_object()?;
                let step = text(value, "step")?;
                Some(PlanStep {
                    id: format!("codex-plan-step-{index}"),
                    text: bounded_text(step, MAX_PROVIDER_TEXT_BYTES),
                    status: match text(value, "status") {
                        Some("completed") => ActivityStatus::Completed,
                        Some("inProgress") => ActivityStatus::Active,
                        _ => ActivityStatus::Pending,
                    },
                })
            })
            .collect::<Vec<_>>();
        let markdown = steps
            .iter()
            .map(|step| format!("- {}", step.text))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(vec![self.event(
            state,
            "turn/plan/updated",
            route_turn(state, Some(object)),
            None,
            None,
            CanonicalEvent::PlanUpdated(PlanUpdatedEvent { markdown, steps }),
        )?])
    }

    pub(super) fn diff_updated(
        &self,
        state: &mut CodexRouteState,
        method: &str,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, method)?;
        let diff = text(object, "diff")
            .or_else(|| text(object, "patch"))
            .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES));
        let files = diff
            .as_deref()
            .map(changed_files_from_unified_diff)
            .unwrap_or_default();
        merge_changed_files(&mut state.changed_files, &files);
        Ok(vec![self.event(
            state,
            method,
            route_turn(state, Some(object)),
            provider_turn_from(object),
            provider_item_from(object),
            CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
                source: "codex".to_string(),
                files,
                provider_diff: diff,
            }),
        )?])
    }

    pub(super) fn item_lifecycle(
        &self,
        state: &mut CodexRouteState,
        params: Value,
        completed: bool,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let method = if completed {
            "item/completed"
        } else {
            "item/started"
        };
        let object = required_object(&params, method)?;
        let item = object
            .get("item")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("item lifecycle item"))?;
        let raw_id = required_text(item, "id")?;
        let item_id = provider_identifier::<ProviderItemId>(raw_id)
            .unwrap_or_else(|| self.fallback_provider_item_id());
        let raw_kind = required_text(item, "type")?;
        let kind = item_kind(raw_kind);
        if completed && kind == CanonicalItemKind::Plan {
            if let Some(plan) = text(item, "text") {
                return Ok(vec![self.event(
                    state,
                    method,
                    route_turn(state, Some(object)),
                    provider_turn_from(object),
                    Some(item_id.clone()),
                    CanonicalEvent::ProposedPlanCompleted(ProposedPlanCompletedEvent {
                        plan_id: item_id.as_str().to_string(),
                        markdown: bounded_text(plan, MAX_PROVIDER_TEXT_BYTES),
                    }),
                )?]);
            }
        }
        let status = item_status(item, completed);
        let title = item_title(item, raw_kind);
        let detail = item_detail(item, raw_kind);
        let metadata = item_safe_metadata(item, raw_kind);
        let lifecycle = ItemLifecycleEvent {
            item_id: item_id.as_str().to_string(),
            kind,
            status,
            title,
            detail,
            safe_metadata: metadata,
        };
        let chat_turn_id = if kind == CanonicalItemKind::ContextCompaction {
            None
        } else {
            route_turn(state, Some(object))
        };
        let mut events = vec![self.event(
            state,
            method,
            chat_turn_id,
            provider_turn_from(object),
            Some(item_id.clone()),
            if completed {
                CanonicalEvent::ItemCompleted(lifecycle)
            } else {
                CanonicalEvent::ItemStarted(lifecycle)
            },
        )?];
        if raw_kind == "fileChange" {
            let files = changed_files_from_item(item);
            if !files.is_empty() {
                merge_changed_files(&mut state.changed_files, &files);
                let provider_diff = item
                    .get("changes")
                    .and_then(Value::as_array)
                    .map(|changes| {
                        changes
                            .iter()
                            .filter_map(Value::as_object)
                            .filter_map(|change| text(change, "diff"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|diff| !diff.is_empty())
                    .map(|diff| bounded_text(&diff, MAX_PROVIDER_TEXT_BYTES));
                events.push(self.event(
                    state,
                    method,
                    route_turn(state, Some(object)),
                    provider_turn_from(object),
                    Some(item_id.clone()),
                    CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
                        source: "codex_file_change".to_string(),
                        files,
                        provider_diff,
                    }),
                )?);
            }
        }
        if raw_kind == "collabAgentToolCall" {
            events.push(
                self.event(
                    state,
                    method,
                    route_turn(state, Some(object)),
                    provider_turn_from(object),
                    Some(item_id.clone()),
                    CanonicalEvent::TaskLifecycle(TaskLifecycleEvent {
                        task_id: item_id.as_str().to_string(),
                        parent_task_id: None,
                        status,
                        title: text(item, "tool").unwrap_or("Codex task").to_string(),
                        detail: text(item, "prompt")
                            .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES)),
                        safe_metadata: None,
                    }),
                )?,
            );
        }
        Ok(events)
    }

    pub(super) fn content_delta(
        &self,
        state: &mut CodexRouteState,
        params: Value,
        kind: ContentStreamKind,
        delta_key: &str,
        index_key: Option<&str>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "content delta")?;
        let raw_item = required_text(object, "itemId")?;
        let item_id = provider_identifier::<ProviderItemId>(raw_item)
            .unwrap_or_else(|| self.fallback_provider_item_id());
        let delta = bounded_text(required_text(object, delta_key)?, MAX_PROVIDER_TEXT_BYTES);
        if delta.is_empty() {
            return Ok(Vec::new());
        }
        let content_index = index_key
            .and_then(|key| unsigned(object, key))
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or_else(|| {
                *state
                    .stream_indexes
                    .entry((item_id.as_str().to_string(), kind))
                    .or_insert(0)
            });
        Ok(vec![self.event(
            state,
            "content/delta",
            route_turn(state, Some(object)),
            provider_turn_from(object),
            Some(item_id.clone()),
            CanonicalEvent::ContentDelta(ContentDeltaEvent {
                item_id: item_id.as_str().to_string(),
                stream_kind: kind,
                content_index,
                delta,
            }),
        )?])
    }

    pub(super) fn proposed_plan_delta(
        &self,
        state: &mut CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "item/plan/delta")?;
        let raw_item = required_text(object, "itemId")?;
        let item_id = provider_identifier::<ProviderItemId>(raw_item)
            .unwrap_or_else(|| self.fallback_provider_item_id());
        let index = *state
            .stream_indexes
            .entry((item_id.as_str().to_string(), ContentStreamKind::PlanText))
            .or_insert(0);
        Ok(vec![self.event(
            state,
            "item/plan/delta",
            route_turn(state, Some(object)),
            provider_turn_from(object),
            Some(item_id.clone()),
            CanonicalEvent::ProposedPlanDelta(ProposedPlanDeltaEvent {
                plan_id: item_id.as_str().to_string(),
                delta: bounded_text(required_text(object, "delta")?, MAX_PROVIDER_TEXT_BYTES),
                content_index: index,
            }),
        )?])
    }

    pub(super) fn tool_progress(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "item/mcpToolCall/progress")?;
        let item_id = required_text(object, "itemId")?;
        Ok(vec![
            self.event(
                state,
                "item/mcpToolCall/progress",
                route_turn(state, Some(object)),
                provider_turn_from(object),
                provider_identifier::<ProviderItemId>(item_id),
                CanonicalEvent::ToolProgress(ToolProgressEvent {
                    tool_id: item_id.to_string(),
                    status: ActivityStatus::Active,
                    title: "MCP tool".to_string(),
                    progress: None,
                    summary: text(object, "message")
                        .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES)),
                }),
            )?,
        ])
    }

    pub(super) fn hook_lifecycle(
        &self,
        state: &CodexRouteState,
        params: Value,
        completed: bool,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "hook lifecycle")?;
        let run = object
            .get("run")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("hook lifecycle run"))?;
        let id = required_text(run, "id")?;
        Ok(vec![
            self.event(
                state,
                if completed {
                    "hook/completed"
                } else {
                    "hook/started"
                },
                route_turn(state, Some(object)),
                provider_turn_from(object),
                None,
                CanonicalEvent::HookLifecycle(HookLifecycleEvent {
                    hook_id: id.to_string(),
                    status: if completed {
                        status_text(run.get("status").unwrap_or(&Value::Null))
                            .map(activity_status)
                            .unwrap_or(ActivityStatus::Completed)
                    } else {
                        ActivityStatus::Active
                    },
                    title: text(run, "eventName").unwrap_or("Codex hook").to_string(),
                    detail: text(run, "statusMessage")
                        .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES)),
                }),
            )?,
        ])
    }

    pub(super) fn context_compacted(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "thread/compacted")?;
        let item_id = self.fallback_provider_item_id();
        Ok(vec![self.event(
            state,
            "thread/compacted",
            None,
            provider_turn_from(object),
            Some(item_id.clone()),
            CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                item_id: item_id.as_str().to_string(),
                kind: CanonicalItemKind::ContextCompaction,
                status: ActivityStatus::Completed,
                title: Some("Context compacted".to_string()),
                detail: None,
                safe_metadata: None,
            }),
        )?])
    }
}
