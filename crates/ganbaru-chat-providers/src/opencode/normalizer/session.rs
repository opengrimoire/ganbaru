use super::value::*;
use super::*;

impl OpenCodeEventNormalizer {
    pub(super) fn session_updated(
        &self,
        state: &OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let info = properties
            .get("info")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("session update"))?;
        let title = text(info, "title")
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(|title| bounded_text(title, 512));
        Ok(vec![self.event(
            state,
            "session.updated",
            None,
            None,
            CanonicalEvent::ThreadMetadataUpdated(ThreadMetadataUpdatedEvent {
                title,
                provider_thread_id: ProviderThreadId::new(state.provider_thread_id.clone()).ok(),
                resume_cursor: None,
                metadata: Some(VersionedJson {
                    schema_version: 1,
                    value: json!({
                        "directory": text(info, "directory").map(|value| bounded_text(value, 4096)),
                        "version": text(info, "version").map(|value| bounded_text(value, 128)),
                    }),
                }),
            }),
        )?])
    }

    pub(super) fn session_status(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let status = properties
            .get("status")
            .and_then(Value::as_object)
            .and_then(|status| text(status, "type"))
            .ok_or_else(|| protocol_error("session status"))?;
        match status {
            "busy" => {
                let previous = state.session_state;
                state.session_state = ProviderSessionState::Active;
                if previous == state.session_state {
                    return Ok(Vec::new());
                }
                Ok(vec![self.event(
                    state,
                    "session.status",
                    None,
                    None,
                    CanonicalEvent::SessionStateChanged(SessionStateChangedEvent {
                        session_id: self.session_id.clone(),
                        previous_state: previous,
                        state: state.session_state,
                        reason: None,
                    }),
                )?])
            }
            "idle" => {
                let previous = state.session_state;
                state.session_state = ProviderSessionState::Ready;
                let mut events = Vec::new();
                if previous != state.session_state {
                    events.push(self.event(
                        state,
                        "session.status",
                        None,
                        None,
                        CanonicalEvent::SessionStateChanged(SessionStateChangedEvent {
                            session_id: self.session_id.clone(),
                            previous_state: previous,
                            state: state.session_state,
                            reason: None,
                        }),
                    )?);
                }
                if let Some(turn_id) = state.active_turn_id.take() {
                    events.push(self.event(
                        state,
                        "session.status",
                        None,
                        Some(turn_id),
                        CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                            state: ChatTurnState::Completed,
                            stop_reason: Some("idle".to_string()),
                            usage: None,
                            changed_files: Vec::new(),
                        }),
                    )?);
                }
                Ok(events)
            }
            "retry" => {
                let detail = properties
                    .get("status")
                    .and_then(Value::as_object)
                    .and_then(|status| text(status, "message"))
                    .map(|message| bounded_text(message, 4096));
                Ok(vec![self.event(
                    state,
                    "session.status",
                    None,
                    None,
                    CanonicalEvent::RuntimeWarning(NotificationEvent {
                        code: "opencode_retry".to_string(),
                        title: "OpenCode is retrying".to_string(),
                        detail,
                    }),
                )?])
            }
            unknown => Ok(vec![self.unknown_event(
                state,
                "session.status",
                unknown,
                properties,
            )?]),
        }
    }

    pub(super) fn session_error(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let message = properties
            .get("error")
            .and_then(Value::as_object)
            .and_then(|error| text(error, "message"))
            .map(|message| bounded_text(message, 4096))
            .unwrap_or_else(|| "OpenCode reported a provider error".to_string());
        let mut events = vec![self.event(
            state,
            "session.error",
            None,
            None,
            CanonicalEvent::RuntimeError(RuntimeErrorEvent {
                code: "opencode_provider_error".to_string(),
                message: message.clone(),
                recoverable: true,
                safe_details: properties.get("error").map(safe_shape),
            }),
        )?];
        if let Some(turn_id) = state.active_turn_id.take() {
            state.session_state = ProviderSessionState::Ready;
            events.push(self.event(
                state,
                "session.error",
                None,
                Some(turn_id),
                CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                    state: ChatTurnState::Failed,
                    stop_reason: Some(message),
                    usage: None,
                    changed_files: Vec::new(),
                }),
            )?);
        }
        Ok(events)
    }

    pub(super) fn session_diff(
        &self,
        state: &OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let diff = properties
            .get("diff")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("session diff"))?;
        if diff.len() > MAX_SAFE_COLLECTION_ITEMS {
            return Err(protocol_error("session diff size"));
        }
        let mut files = Vec::new();
        for entry in diff {
            let entry = entry
                .as_object()
                .ok_or_else(|| protocol_error("session diff entry"))?;
            let path = text(entry, "file")
                .or_else(|| text(entry, "path"))
                .ok_or_else(|| protocol_error("session diff path"))?;
            files.push(ChangedFileSummary {
                relative_path: bounded_text(path, 4096),
                previous_relative_path: text(entry, "before")
                    .map(|value| bounded_text(value, 4096)),
                additions: entry.get("additions").and_then(Value::as_u64),
                deletions: entry.get("deletions").and_then(Value::as_u64),
                binary: entry
                    .get("binary")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                status: text(entry, "status").unwrap_or("modified").to_string(),
            });
        }
        Ok(vec![self.event(
            state,
            "session.diff",
            None,
            None,
            CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
                source: "opencode".to_string(),
                files,
                provider_diff: None,
            }),
        )?])
    }
}
