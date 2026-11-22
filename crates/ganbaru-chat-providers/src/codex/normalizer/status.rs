use super::value::*;
use super::*;

impl CodexEventNormalizer {
    pub(super) fn authentication_status(
        &self,
        state: &CodexRouteState,
        method: &str,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, method)?;
        let account = object.get("account").and_then(Value::as_object);
        let authenticated = account.is_some()
            || object
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let account_label = account
            .and_then(|value| text(value, "email").or_else(|| text(value, "type")))
            .map(str::to_string);
        Ok(vec![self.event(
            state,
            method,
            None,
            None,
            None,
            CanonicalEvent::AuthenticationStatus(AuthenticationStatusEvent {
                authenticated,
                account_label,
                action_required: !authenticated,
                detail: None,
            }),
        )?])
    }

    /// Merges a sparse rate limit update and reports the merged limits when they changed.
    ///
    /// Each event carries every merged limit, because readers keep only the newest status. An
    /// update without a snapshot object is ignored, since a missing status must not end the
    /// session.
    pub(super) fn rate_limit_status(
        &self,
        state: &mut CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "account/rateLimits/updated")?;
        let Some(limits) = object.get("rateLimits").and_then(Value::as_object) else {
            return Ok(Vec::new());
        };
        if !state.rate_limits.merge(limits) {
            return Ok(Vec::new());
        }
        let projection = state.rate_limits.projection();
        Ok(vec![self.event(
            state,
            "account/rateLimits/updated",
            None,
            None,
            None,
            CanonicalEvent::RateLimitStatus(RateLimitStatusEvent {
                limited: projection.limited,
                resets_at: projection.resets_at,
                detail: None,
                provider_data: Some(projection.provider_data),
            }),
        )?])
    }

    pub(super) fn mcp_status(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "mcpServer/startupStatus/updated")?;
        let server_id = text(object, "serverName")
            .or_else(|| text(object, "serverId"))
            .unwrap_or("unknown");
        let status = text(object, "status")
            .map(activity_status)
            .unwrap_or(ActivityStatus::Unknown);
        Ok(vec![
            self.event(
                state,
                "mcpServer/startupStatus/updated",
                None,
                None,
                None,
                CanonicalEvent::McpStatus(McpStatusEvent {
                    server_id: server_id.to_string(),
                    status,
                    detail: text(object, "message")
                        .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES)),
                }),
            )?,
        ])
    }

    pub(super) fn mcp_oauth(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "mcpServer/oauthLogin/completed")?;
        Ok(vec![
            self.event(
                state,
                "mcpServer/oauthLogin/completed",
                None,
                None,
                None,
                CanonicalEvent::McpOauthCompleted(McpOauthCompletedEvent {
                    server_id: text(object, "serverName").unwrap_or("unknown").to_string(),
                    successful: object
                        .get("success")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    detail: text(object, "error")
                        .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES)),
                }),
            )?,
        ])
    }

    pub(super) fn model_rerouted(
        &self,
        state: &mut CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "model/rerouted")?;
        let requested = ModelId::new(required_text(object, "fromModel")?.to_string())
            .map_err(|_| protocol_error("model/rerouted.fromModel"))?;
        let effective = ModelId::new(required_text(object, "toModel")?.to_string())
            .map_err(|_| protocol_error("model/rerouted.toModel"))?;
        state.effective_model_id = Some(effective.clone());
        Ok(vec![self.event(
            state,
            "model/rerouted",
            route_turn(state, Some(object)),
            provider_turn_from(object),
            None,
            CanonicalEvent::ModelRerouted(ModelReroutedEvent {
                requested_model_id: requested,
                effective_model_id: effective,
                reason: object.get("reason").map(provider_reason),
            }),
        )?])
    }

    pub(super) fn notification(
        &self,
        state: &CodexRouteState,
        params: Value,
        code: &str,
        kind: CanonicalNotificationKind,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, code)?;
        let title = text(object, "summary")
            .or_else(|| text(object, "message"))
            .unwrap_or("Codex warning");
        let notification = NotificationEvent {
            code: code.to_string(),
            title: bounded_text(title, MAX_PROVIDER_TEXT_BYTES),
            detail: text(object, "details")
                .or_else(|| text(object, "detail"))
                .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES)),
        };
        let event = match kind {
            CanonicalNotificationKind::Configuration => {
                CanonicalEvent::ConfigurationWarning(notification)
            }
            CanonicalNotificationKind::Deprecation => {
                CanonicalEvent::DeprecationNotice(notification)
            }
            CanonicalNotificationKind::Runtime => CanonicalEvent::RuntimeWarning(notification),
        };
        Ok(vec![self.event(state, code, None, None, None, event)?])
    }

    pub(super) fn runtime_error(
        &self,
        state: &CodexRouteState,
        params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let object = required_object(&params, "error")?;
        let error = object.get("error").and_then(Value::as_object);
        let message = error
            .and_then(|value| text(value, "message"))
            .unwrap_or("Codex turn failed");
        // A capacity error the session retries itself is recoverable even when Codex will not
        // retry it.
        let session_retries = error.is_some_and(CodexStopReason::is_capacity)
            && state.turn().retry().is_some_and(|retry| retry.available());
        let recoverable = session_retries
            || object
                .get("willRetry")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        Ok(vec![self.event(
            state,
            "error",
            route_turn(state, Some(object)),
            provider_turn_from(object),
            None,
            CanonicalEvent::RuntimeError(RuntimeErrorEvent {
                code: "codex_runtime_error".to_string(),
                message: bounded_text(message, MAX_PROVIDER_TEXT_BYTES),
                recoverable,
                safe_details: error.map(|value| VersionedJson {
                    schema_version: 1,
                    value: json!({ "keys": bounded_keys(value) }),
                }),
            }),
        )?])
    }

    pub(super) fn session_closed(
        &self,
        state: &mut CodexRouteState,
        _params: Value,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        Ok(vec![self.event(
            state,
            "thread/closed",
            None,
            None,
            None,
            CanonicalEvent::SessionExited(SessionExitedEvent {
                session_id: self.session_id.clone(),
                expected: false,
                exit_code: None,
                reason: Some("Codex closed the thread".to_string()),
            }),
        )?])
    }
}
