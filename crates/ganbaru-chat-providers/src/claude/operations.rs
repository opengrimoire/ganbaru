//! Claude provider command operations.

use super::driver::*;
use super::home::resolve_claude_home;
use super::normalizer::ClaudeRouteState;
use super::protocol::*;
use super::session::*;
use super::support::*;
use crate::events::*;
use crate::models::*;
use crate::{
    DriverFuture, DriverOperationContext, ProviderAuthoritySupport, ProviderDriver,
    ProviderEventSink,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, atomic::Ordering};
use std::time::Duration;

impl ProviderDriver for ClaudeProviderDriver {
    fn metadata(&self) -> ProviderFamilyMetadataRead {
        Self::metadata_read()
    }

    fn instance_configuration(&self) -> &ProviderInstanceConfig {
        &self.configuration
    }

    fn capabilities(&self) -> ProviderCapabilities {
        claude_capabilities()
    }

    fn authority_support(&self) -> ProviderAuthoritySupport {
        ProviderAuthoritySupport {
            isolated_conversation: true,
            internal_host_tools: true,
            deny_shell: false,
            read_only_root: false,
            writable_root: false,
            confined_commands: false,
            network_boundary: false,
            classified_publish: false,
        }
    }

    fn cached_model_catalog(&self) -> Option<ProviderModelCatalog> {
        self.cached_models.clone()
    }

    fn prompt_catalog(&self) -> ChatResult<Vec<ChatPromptCatalogEntry>> {
        Ok(super::protocol::prompt_catalog(&self.cached_commands))
    }

    fn probe<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderProbeResult> {
        Box::pin(async move {
            let checked_at = now_utc()?;
            match self.probe_snapshot(context).await {
                Ok(snapshot) => {
                    self.cached_commands = snapshot.initialize.commands.clone();
                    let authenticated = snapshot.initialize.account.is_some()
                        || self
                            .configuration
                            .environment
                            .contains_key("ANTHROPIC_API_KEY");
                    self.cached_models = Some(ProviderModelCatalog {
                        instance_id: self.configuration.instance_id.clone(),
                        models: snapshot.models,
                        source: ModelCatalogSource::Provider,
                        discovered_at: checked_at.clone(),
                        stale: false,
                    });
                    Ok(ProviderProbeResult {
                        instance_id: self.configuration.instance_id.clone(),
                        state: if authenticated {
                            ProbeState::Healthy
                        } else {
                            ProbeState::AuthenticationRequired
                        },
                        version: Some(snapshot.version.to_string()),
                        negotiated_protocol_version: Some("stream-json".to_string()),
                        account_label: snapshot.initialize.account.and_then(|account| {
                            account
                                .email
                                .or(account.organization)
                                .or(account.subscription_type)
                        }),
                        capabilities: claude_capabilities(),
                        authority_support: self.authority_support(),
                        checked_at,
                        detail: (!authenticated)
                            .then_some("Claude authentication is required".to_string()),
                    })
                }
                Err(error) if context.is_cancelled() => Err(error),
                Err(error) => Ok(ProviderProbeResult {
                    instance_id: self.configuration.instance_id.clone(),
                    state: probe_state_for_error(error.code),
                    version: None,
                    negotiated_protocol_version: None,
                    account_label: None,
                    capabilities: claude_capabilities(),
                    authority_support: self.authority_support(),
                    checked_at,
                    detail: Some(probe_detail(error.code).to_string()),
                }),
            }
        })
    }

    fn discover_models<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderModelCatalog> {
        Box::pin(async move {
            let snapshot = self.probe_snapshot(context).await?;
            self.cached_commands = snapshot.initialize.commands.clone();
            let catalog = ProviderModelCatalog {
                instance_id: self.configuration.instance_id.clone(),
                models: snapshot.models,
                source: ModelCatalogSource::Provider,
                discovered_at: now_utc()?,
                stale: false,
            };
            self.cached_models = Some(catalog.clone());
            Ok(catalog)
        })
    }

    fn derive_continuation_group<'a>(
        &'a mut self,
        request: ContinuationGroupRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ContinuationGroupId> {
        Box::pin(async move {
            let home = resolve_claude_home(&self.configuration)?;
            if let Some(expected) = request.normalized_provider_home.as_deref() {
                let expected = std::fs::canonicalize(expected).map_err(|_| {
                    ChatError::validation(
                        "normalizedProviderHome",
                        "Claude continuation home is unavailable",
                    )
                })?;
                if expected != home.config_directory {
                    return Err(ChatError::new(
                        ChatErrorCode::Conflict,
                        "Claude config directory change requires a thread fork",
                        true,
                    ));
                }
            }
            home.continuation_group()
        })
    }

    fn start_session<'a>(
        &'a mut self,
        request: StartSessionRequest,
        sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        Box::pin(async move {
            self.open_session(SessionOpenInput::Fresh(request), sink, context)
                .await
        })
    }

    fn resume_session<'a>(
        &'a mut self,
        request: ResumeSessionRequest,
        sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        Box::pin(async move {
            self.open_session(SessionOpenInput::Resume(request), sink, context)
                .await
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        request: SendTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, TurnDispatchReceipt> {
        Box::pin(async move {
            let message = build_user_message(&request)?;
            let requested_effort = selected_effort(&request.model_options)?;
            let requested_fast_mode = selected_fast_mode(&request.model_options)?;
            let live = self.live_mut(&request.session_id)?;
            if requested_effort.is_some() && requested_effort != live.effective_effort {
                return Err(ChatError::unsupported(
                    "Claude effort can only be changed when starting a session",
                ));
            }
            if requested_fast_mode.is_some() && requested_fast_mode != live.effective_fast_mode {
                return Err(ChatError::unsupported(
                    "Claude Fast mode can only be changed when starting a session",
                ));
            }
            let client = live.connection.client();
            if let Some(mode) = permission_mode(request.modes) {
                client
                    .request("set_permission_mode", json!({ "mode": mode }), context)
                    .await
                    .map_err(|error| error.to_chat_error("permission mode change"))?;
            }
            if let Some(model) = request.model_id.as_ref() {
                if Some(model) != live.effective_model.as_ref() {
                    client
                        .request("set_model", json!({ "model": model.as_str() }), context)
                        .await
                        .map_err(|error| error.to_chat_error("model change"))?;
                    live.effective_model = Some(model.clone());
                }
            }
            let effective_model = live.effective_model.clone();
            let provider_turn_id =
                ProviderTurnId::new(format!("claude-{}", request.turn_id.as_str()))
                    .map_err(|_| protocol_identifier_error("turn"))?;
            let event = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                if state.active_chat_turn_id.is_some() {
                    return Err(ChatError::new(
                        ChatErrorCode::Busy,
                        "Claude already has an active turn; queue or steer the prompt",
                        true,
                    ));
                }
                state.active_chat_turn_id = Some(request.turn_id.clone());
                state.session_state = ProviderSessionState::Active;
                state.modes = request.modes;
                state.model_id = effective_model.clone();
                live.normalizer.external_event(
                    &state,
                    "turn/started",
                    None,
                    CanonicalEvent::TurnStarted(TurnStartedEvent {
                        provider_turn_id: Some(provider_turn_id.clone()),
                        state: ChatTurnState::Active,
                        modes: request.modes,
                        model_id: effective_model,
                        model_options: request.model_options.clone(),
                    }),
                )?
            };
            if let Err(error) = live.sink.emit(event).await {
                release_active_turn(&live.route);
                return Err(error);
            }
            if let Err(error) = client.send(message).await {
                release_active_turn(&live.route);
                return Err(error.to_chat_error("turn dispatch"));
            }
            Ok(TurnDispatchReceipt {
                turn_id: request.turn_id,
                state: ChatTurnState::Active,
                provider_turn_id: Some(provider_turn_id),
                accepted_at: now_utc()?,
            })
        })
    }

    fn compact_context<'a>(
        &'a mut self,
        request: CompactContextRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let message = build_slash_command_message("/compact")?;
            let live = self.live_mut(&request.session_id)?;
            let provider_turn_id =
                ProviderTurnId::new(format!("claude-{}", request.turn_id.as_str()))
                    .map_err(|_| protocol_identifier_error("turn"))?;
            let started = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                if state.active_chat_turn_id.is_some() {
                    return Err(ChatError::new(
                        ChatErrorCode::Busy,
                        "Claude already has an active turn",
                        true,
                    ));
                }
                state.active_chat_turn_id = Some(request.turn_id);
                state.session_state = ProviderSessionState::Active;
                live.normalizer.external_event(
                    &state,
                    "turn/started",
                    None,
                    CanonicalEvent::TurnStarted(TurnStartedEvent {
                        provider_turn_id: Some(provider_turn_id),
                        state: ChatTurnState::Active,
                        modes: state.modes,
                        model_id: state.model_id.clone(),
                        model_options: Vec::new(),
                    }),
                )?
            };
            if let Err(error) = live.sink.emit(started).await {
                release_active_turn(&live.route);
                return Err(error);
            }
            if let Err(error) = live.connection.client().send(message).await {
                release_active_turn(&live.route);
                return Err(error.to_chat_error("context compaction"));
            }
            Ok(operation_receipt(
                &context.operation_id,
                "Claude context compaction started",
            ))
        })
    }

    fn steer_turn<'a>(
        &'a mut self,
        request: SteerTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            if request.prompt.is_empty()
                || request.prompt.len() > 4 * 1024 * 1024
                || request.prompt.contains('\0')
            {
                return Err(ChatError::validation(
                    "prompt",
                    "Claude steering text is invalid",
                ));
            }
            let live = self.live_mut(&request.session_id)?;
            if live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .active_chat_turn_id
                .is_none()
            {
                return Err(ChatError::invalid_transition(
                    "Claude has no active turn to steer",
                ));
            }
            live.connection
                .client()
                .send(json!({
                    "type": "user",
                    "message": {
                        "role": "user",
                        "content": [{ "type": "text", "text": request.prompt }],
                    },
                    "parent_tool_use_id": Value::Null,
                    "session_id": "",
                }))
                .await
                .map_err(|error| error.to_chat_error("turn steering"))?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "Claude accepted steering input",
            ))
        })
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        request: InterruptTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            live.connection
                .client()
                .request("interrupt", json!({}), context)
                .await
                .map_err(|error| error.to_chat_error("turn interrupt"))?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "Claude turn interrupt requested",
            ))
        })
    }

    fn resolve_approval<'a>(
        &'a mut self,
        request: ResolveApprovalRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            resolve_approval(&live.connection.client(), &live.pending_requests, &request).await?;
            let event = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                state.session_state = ProviderSessionState::Active;
                live.normalizer.external_event(
                    &state,
                    "control/approval_resolved",
                    Some(request.provider_request_id.clone()),
                    CanonicalEvent::RequestResolved(RequestResolvedEvent {
                        request_id: request.provider_request_id.clone(),
                        state: RequestResolutionState::Resolved,
                        decision: Some(request.decision.clone()),
                    }),
                )?
            };
            live.sink.emit(event).await?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "Claude approval response accepted",
            ))
        })
    }

    fn resolve_user_input<'a>(
        &'a mut self,
        request: ResolveUserInputRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            resolve_user_input(&live.connection.client(), &live.pending_requests, &request).await?;
            let event = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                state.session_state = ProviderSessionState::Active;
                live.normalizer.external_event(
                    &state,
                    "control/user_input_resolved",
                    Some(request.provider_request_id.clone()),
                    CanonicalEvent::UserInputResolved(UserInputResolvedEvent {
                        request_id: request.provider_request_id.clone(),
                        state: RequestResolutionState::Resolved,
                        answers: request.answers.clone(),
                    }),
                )?
            };
            live.sink.emit(event).await?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "Claude user input response accepted",
            ))
        })
    }

    fn rollback<'a>(
        &'a mut self,
        _request: RollbackRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async { Err(ChatError::unsupported("Claude rollback is not supported")) })
    }

    fn read_history<'a>(
        &'a mut self,
        _request: ReadHistoryRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderHistoryPage> {
        Box::pin(async {
            Err(ChatError::unsupported(
                "Claude native history reads are not exposed by the SDK protocol",
            ))
        })
    }

    fn stop_session<'a>(
        &'a mut self,
        request: StopSessionRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let Some(mut live) = self.live.take() else {
                return Ok(operation_receipt(
                    &context.operation_id,
                    "Claude session was already stopped",
                ));
            };
            if live.session_id != request.session_id {
                self.live = Some(live);
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "Claude session identity does not match the active session",
                    false,
                ));
            }
            live.expected_shutdown.store(true, Ordering::Release);
            let _ = live
                .connection
                .client()
                .request("interrupt", json!({}), context)
                .await;
            live.sink.flush().await?;
            let state = live.route.lock().map_err(|_| driver_state_error())?.clone();
            live.sink
                .emit(live.normalizer.external_event(
                    &state,
                    "session/exited",
                    None,
                    CanonicalEvent::SessionExited(SessionExitedEvent {
                        session_id: live.session_id.clone(),
                        expected: true,
                        exit_code: None,
                        reason: Some("Claude session stopped".to_string()),
                    }),
                )?)
                .await?;
            live.sink.flush().await?;
            live.connection
                .stop(
                    if request.force {
                        Duration::ZERO
                    } else {
                        SESSION_GRACEFUL_STOP
                    },
                    SESSION_FORCE_STOP,
                )
                .await?;
            let _ = tokio::time::timeout(SESSION_GRACEFUL_STOP, &mut live.router_task).await;
            if !live.router_task.is_finished() {
                live.router_task.abort();
            }
            Ok(operation_receipt(
                &context.operation_id,
                "Claude session stopped",
            ))
        })
    }
}

/// Clears the active turn claim after a turn fails before the provider accepted it.
///
/// A poisoned route lock is left untouched because the session is already unusable.
fn release_active_turn(route: &Mutex<ClaudeRouteState>) {
    if let Ok(mut state) = route.lock() {
        state.active_chat_turn_id = None;
        state.session_state = ProviderSessionState::Ready;
    }
}
