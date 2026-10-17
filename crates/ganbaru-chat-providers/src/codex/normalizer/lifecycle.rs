use super::value::*;
use super::*;

impl CodexEventNormalizer {
    pub(super) fn thread_started(
        &self,
        state: &mut CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "thread/started")?;
        let thread = object
            .get("thread")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("thread/started.thread"))?;
        let provider_thread_id = required_text(thread, "id")?.to_string();
        state.provider_thread_id = Some(provider_thread_id.clone());
        let provider_id = ProviderThreadId::new(provider_thread_id.clone())
            .unwrap_or_else(|_| self.fallback_provider_thread_id());
        let provider_title = text(thread, "name")
            .or_else(|| text(thread, "preview"))
            .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES));
        let resume_cursor = VersionedJson {
            schema_version: 1,
            value: json!({ "threadId": provider_thread_id }),
        };
        Ok(vec![
            self.event(
                state,
                "thread/started",
                None,
                None,
                None,
                CanonicalEvent::ThreadStarted(ThreadStartedEvent {
                    provider_thread_id: provider_id.clone(),
                    title: None,
                }),
            )?,
            self.event(
                state,
                "thread/started",
                None,
                None,
                None,
                CanonicalEvent::ThreadMetadataUpdated(ThreadMetadataUpdatedEvent {
                    title: None,
                    provider_thread_id: Some(provider_id),
                    resume_cursor: Some(resume_cursor),
                    metadata: provider_title.map(|title| VersionedJson {
                        schema_version: 1,
                        value: json!({ "providerTitle": title }),
                    }),
                }),
            )?,
        ])
    }

    pub(super) fn thread_status_changed(
        &self,
        state: &mut CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "thread/status/changed")?;
        let status = object
            .get("status")
            .and_then(status_text)
            .unwrap_or("active");
        let next = match status {
            "idle" => ChatThreadState::Idle,
            "error" | "failed" => ChatThreadState::Error,
            "closed" => ChatThreadState::Closed,
            _ => ChatThreadState::Active,
        };
        let previous = state.thread_state;
        state.thread_state = next;
        Ok(vec![self.event(
            state,
            "thread/status/changed",
            None,
            None,
            None,
            CanonicalEvent::ThreadStateChanged(ThreadStateChangedEvent {
                previous_state: previous,
                state: next,
                reason: None,
            }),
        )?])
    }

    pub(super) fn thread_metadata(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "thread/name/updated")?;
        let provider_thread_id = state
            .provider_thread_id
            .as_deref()
            .and_then(|value| ProviderThreadId::new(value.to_string()).ok());
        Ok(vec![self.event(
            state,
            "thread/name/updated",
            None,
            None,
            None,
            CanonicalEvent::ThreadMetadataUpdated(ThreadMetadataUpdatedEvent {
                // Provider names are metadata only. Ganbaru owns the visible title.
                title: None,
                provider_thread_id,
                resume_cursor: None,
                metadata: text(object, "name").map(|name| VersionedJson {
                    schema_version: 1,
                    value: json!({ "providerName": bounded_text(name, MAX_PROVIDER_TEXT_BYTES) }),
                }),
            }),
        )?])
    }

    pub(super) fn thread_usage(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "thread/tokenUsage/updated")?;
        let usage = object
            .get("tokenUsage")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("thread/tokenUsage/updated.tokenUsage"))?;
        let total = usage
            .get("total")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("thread/tokenUsage/updated.total"))?;
        let event = ThreadUsageUpdatedEvent {
            input_tokens: unsigned(total, "inputTokens"),
            output_tokens: unsigned(total, "outputTokens"),
            cached_input_tokens: unsigned(total, "cachedInputTokens"),
            context_tokens: unsigned(total, "totalTokens"),
            context_limit: unsigned(usage, "modelContextWindow"),
            cost: None,
        };
        Ok(vec![self.event(
            state,
            "thread/tokenUsage/updated",
            route_turn(state, Some(object)),
            None,
            None,
            CanonicalEvent::ThreadUsageUpdated(event),
        )?])
    }

    pub(super) fn turn_started(
        &self,
        state: &mut CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "turn/started")?;
        let turn = object
            .get("turn")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("turn/started.turn"))?;
        let provider_turn_id = required_text(turn, "id")?.to_string();
        state.active_provider_turn_id = Some(provider_turn_id.clone());
        state.session_state = ProviderSessionState::Active;
        state.changed_files.clear();
        let chat_turn_id = state.active_chat_turn_id.clone();
        let provider_turn = provider_identifier::<ProviderTurnId>(&provider_turn_id)
            .unwrap_or_else(|| self.fallback_provider_turn_id());
        Ok(vec![self.event(
            state,
            "turn/started",
            chat_turn_id,
            Some(provider_turn.clone()),
            None,
            CanonicalEvent::TurnStarted(TurnStartedEvent {
                provider_turn_id: Some(provider_turn),
                state: ChatTurnState::Active,
                modes: state.modes,
                model_id: state.effective_model_id.clone(),
                model_options: Vec::new(),
            }),
        )?])
    }

    pub(super) fn turn_completed(
        &self,
        state: &mut CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "turn/completed")?;
        let turn = object
            .get("turn")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("turn/completed.turn"))?;
        let provider_turn_id = required_text(turn, "id")?;
        let provider_turn = provider_identifier::<ProviderTurnId>(provider_turn_id);
        let status = required_text(turn, "status")?;
        let turn_state = match status {
            "completed" => ChatTurnState::Completed,
            "interrupted" => ChatTurnState::Interrupted,
            _ => ChatTurnState::Failed,
        };
        let stop_reason = turn
            .get("error")
            .and_then(Value::as_object)
            .and_then(|error| text(error, "message"))
            .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES));
        let chat_turn = state.active_chat_turn_id.clone();
        state.session_state = ProviderSessionState::Ready;
        state.active_chat_turn_id = None;
        state.active_provider_turn_id = None;
        state.stream_indexes.clear();
        let changed_files = std::mem::take(&mut state.changed_files);
        Ok(vec![self.event(
            state,
            "turn/completed",
            chat_turn,
            provider_turn,
            None,
            CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                state: turn_state,
                stop_reason,
                usage: None,
                changed_files,
            }),
        )?])
    }
}
