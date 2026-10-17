//! OpenCode implementation of the provider-neutral driver operations.

use super::driver::*;
use super::permissions::permission_reply;
use super::protocol::parse_rollback_cursor;
use super::support::*;
use crate::events::*;
use crate::models::*;
use crate::{
    DriverFuture, DriverOperationContext, ProviderAuthoritySupport, ProviderDriver,
    ProviderEventSink,
};
use std::sync::{Arc, atomic::Ordering};

impl ProviderDriver for OpenCodeProviderDriver {
    fn metadata(&self) -> ProviderFamilyMetadataRead {
        Self::metadata_read()
    }

    fn instance_configuration(&self) -> &ProviderInstanceConfig {
        &self.configuration
    }

    fn capabilities(&self) -> ProviderCapabilities {
        capabilities()
    }

    fn authority_support(&self) -> ProviderAuthoritySupport {
        if self.settings.is_external() {
            return ProviderAuthoritySupport::default();
        }
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
        Ok(self
            .cached_commands
            .iter()
            .map(|command| ChatPromptCatalogEntry {
                value: format!("/{}", command.name),
                label: command.name.clone(),
                description: command.description.clone(),
                argument_hint: command.argument_hint.clone(),
                kind: "command".to_string(),
                source: "provider".to_string(),
                stale: false,
            })
            .collect())
    }

    fn probe<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderProbeResult> {
        Box::pin(async move {
            let checked_at = now_utc()?;
            if context.is_cancelled() {
                return Err(cancelled_error());
            }
            let workspace = canonical_current_directory()?;
            match self.catalog_snapshot(&workspace).await {
                Ok(snapshot) => {
                    let negotiated_protocol_version = snapshot.version.clone();
                    self.cached_commands = snapshot.commands.clone();
                    self.cached_models = Some(ProviderModelCatalog {
                        instance_id: self.configuration.instance_id.clone(),
                        models: snapshot.models,
                        source: ModelCatalogSource::Provider,
                        discovered_at: checked_at.clone(),
                        stale: false,
                    });
                    Ok(ProviderProbeResult {
                        instance_id: self.configuration.instance_id.clone(),
                        state: ProbeState::Healthy,
                        version: snapshot.version,
                        negotiated_protocol_version,
                        account_label: snapshot.account_label,
                        capabilities: capabilities(),
                        authority_support: self.authority_support(),
                        checked_at,
                        detail: Some(snapshot.toolchain_detail),
                    })
                }
                Err(error) if context.is_cancelled() => Err(error),
                Err(error) => Ok(ProviderProbeResult {
                    instance_id: self.configuration.instance_id.clone(),
                    state: probe_state(error.code),
                    version: None,
                    negotiated_protocol_version: None,
                    account_label: None,
                    capabilities: capabilities(),
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
            if context.is_cancelled() {
                return Err(cancelled_error());
            }
            let workspace = canonical_current_directory()?;
            let snapshot = self.catalog_snapshot(&workspace).await?;
            self.cached_commands = snapshot.commands.clone();
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
            if request.provider_instance_id != self.configuration.instance_id {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "OpenCode provider instance does not match continuation request",
                    false,
                ));
            }
            super::config::continuation_group(&self.settings, request.account_identity.as_deref())
        })
    }

    fn start_session<'a>(
        &'a mut self,
        request: StartSessionRequest,
        sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        Box::pin(async move {
            self.open_session(OpenCodeSessionInput::Fresh(request), sink, context)
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
            self.open_session(OpenCodeSessionInput::Resume(request), sink, context)
                .await
        })
    }

    fn fork_thread<'a>(
        &'a mut self,
        request: ProviderForkThreadRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderThreadId> {
        Box::pin(async move { self.fork_native_session(request).await })
    }

    fn send_turn<'a>(
        &'a mut self,
        request: SendTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, TurnDispatchReceipt> {
        Box::pin(async move {
            let prompt = prompt(&request)?;
            let provider_command = provider_command(&request, &self.cached_commands);
            let live = self.live_mut(&request.session_id)?;
            if live
                .command_task
                .as_ref()
                .is_some_and(tokio::task::JoinHandle::is_finished)
            {
                live.command_task.take();
            }
            if live.command_task.is_some() {
                return Err(ChatError::new(
                    ChatErrorCode::Busy,
                    "OpenCode already has a command request in progress",
                    true,
                ));
            }
            let provider_turn_id =
                ProviderTurnId::new(format!("opencode-{}", request.turn_id.as_str()))
                    .map_err(|_| super::protocol::protocol_error("turn ID"))?;
            let started = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                if state.active_turn_id.is_some() {
                    return Err(ChatError::new(
                        ChatErrorCode::Busy,
                        "OpenCode already has an active turn; steer or queue the prompt",
                        true,
                    ));
                }
                state.active_turn_id = Some(request.turn_id.clone());
                state.session_state = ProviderSessionState::Active;
                state.modes = request.modes;
                live.normalizer.external_event(
                    &state,
                    "turn/started",
                    CanonicalEvent::TurnStarted(TurnStartedEvent {
                        provider_turn_id: Some(provider_turn_id.clone()),
                        state: ChatTurnState::Active,
                        modes: request.modes,
                        model_id: request.model_id.clone(),
                        model_options: request.model_options.clone(),
                    }),
                )?
            };
            live.sink.emit(started).await?;
            if let Some((command, arguments)) = provider_command {
                let client = live.client.clone();
                let provider_thread_id = live.provider_thread_id.clone();
                let route = Arc::clone(&live.route);
                let normalizer = Arc::clone(&live.normalizer);
                let sink = Arc::clone(&live.sink);
                live.command_task =
                    Some(tokio::spawn(async move {
                        if let Err(error) = client
                            .execute_command(&provider_thread_id, &command, &arguments)
                            .await
                        {
                            let aborted = route.lock().map_err(|_| driver_state_error()).and_then(
                                |mut state| {
                                    if state.active_turn_id.is_none() {
                                        return Ok(None);
                                    }
                                    state.active_turn_id = None;
                                    state.session_state = ProviderSessionState::Ready;
                                    normalizer
                                        .external_event(
                                            &state,
                                            "command/failed",
                                            CanonicalEvent::TurnAborted(TurnAbortedEvent {
                                                state: ChatTurnState::Failed,
                                                reason: error.message,
                                                recoverable: error.recoverable,
                                            }),
                                        )
                                        .map(Some)
                                },
                            );
                            if let Ok(Some(event)) = aborted {
                                let _ = sink.emit(event).await;
                            }
                        }
                    }));
            } else if let Err(error) = live
                .client
                .prompt_async(&live.provider_thread_id, &prompt)
                .await
            {
                let aborted = {
                    let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                    state.active_turn_id = None;
                    state.session_state = ProviderSessionState::Ready;
                    live.normalizer.external_event(
                        &state,
                        "turn/dispatch_failed",
                        CanonicalEvent::TurnAborted(TurnAbortedEvent {
                            state: ChatTurnState::Failed,
                            reason: error.message.clone(),
                            recoverable: error.recoverable,
                        }),
                    )?
                };
                live.sink.emit(aborted).await?;
                return Err(error);
            }
            Ok(TurnDispatchReceipt {
                turn_id: request.turn_id,
                state: ChatTurnState::Active,
                provider_turn_id: Some(provider_turn_id),
                accepted_at: now_utc()?,
            })
        })
    }

    fn steer_turn<'a>(
        &'a mut self,
        request: SteerTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let prompt = steering_prompt(&request.prompt)?;
            let live = self.live_mut(&request.session_id)?;
            let active_turn_id = live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .active_turn_id
                .clone();
            if active_turn_id.as_ref() != Some(&request.turn_id) {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "OpenCode steering does not match the active turn",
                    false,
                ));
            }
            live.client
                .prompt_async(&live.provider_thread_id, &prompt)
                .await?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "OpenCode accepted steering input",
            ))
        })
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        request: InterruptTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            let active_turn_id = live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .active_turn_id
                .clone();
            if active_turn_id.is_none() {
                return Ok(operation_receipt(
                    request.command.client_command_id.as_str(),
                    "OpenCode turn was already settled",
                ));
            }
            if active_turn_id.as_ref() != Some(&request.turn_id) {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "OpenCode interrupt does not match the active turn",
                    false,
                ));
            }
            live.client.abort(&live.provider_thread_id).await?;
            if let Some(task) = live.command_task.take() {
                task.abort();
            }
            let event = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                state.active_turn_id = None;
                state.session_state = ProviderSessionState::Ready;
                live.normalizer.external_event(
                    &state,
                    "turn/interrupted",
                    CanonicalEvent::TurnAborted(TurnAbortedEvent {
                        state: ChatTurnState::Interrupted,
                        reason: "Interrupted by user".to_string(),
                        recoverable: true,
                    }),
                )?
            };
            live.sink.emit(event).await?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "OpenCode turn interrupt requested",
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
            if !live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .has_permission(&request.provider_request_id)
            {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "OpenCode approval is no longer pending in this session",
                    false,
                ));
            }
            live.client
                .reply_permission(
                    request.provider_request_id.as_str(),
                    permission_reply(request.decision.kind),
                )
                .await?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "OpenCode approval response accepted",
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
            if request.answers.is_empty() {
                live.client
                    .reject_question(request.provider_request_id.as_str())
                    .await?;
            } else {
                let answers = live
                    .route
                    .lock()
                    .map_err(|_| driver_state_error())?
                    .question_answers(&request.provider_request_id, &request.answers)?;
                live.client
                    .reply_question(request.provider_request_id.as_str(), &answers)
                    .await?;
            }
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "OpenCode question response accepted",
            ))
        })
    }

    fn rollback<'a>(
        &'a mut self,
        request: RollbackRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let cursor = request.provider_cursor.as_ref().ok_or_else(|| {
                ChatError::validation(
                    "providerCursor",
                    "OpenCode rollback requires a native message cursor",
                )
            })?;
            let cursor = parse_rollback_cursor(cursor)?;
            let live = self.live_mut(&request.session_id)?;
            live.client
                .revert(
                    &live.provider_thread_id,
                    &cursor.message_id,
                    cursor.part_id.as_deref(),
                )
                .await?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                "OpenCode native conversation reverted",
            ))
        })
    }

    fn read_history<'a>(
        &'a mut self,
        request: ReadHistoryRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderHistoryPage> {
        Box::pin(async move {
            let live = self.live_mut(&request.session_id)?;
            let page = live
                .client
                .messages(
                    &live.provider_thread_id,
                    request.limit,
                    request.cursor.as_deref(),
                )
                .await?;
            let items = page
                .messages
                .into_iter()
                .map(history_item)
                .collect::<ChatResult<Vec<_>>>()?;
            Ok(ProviderHistoryPage {
                items,
                next_cursor: page.next_cursor,
            })
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
                    "OpenCode session was already stopped",
                ));
            };
            if live.session_id != request.session_id {
                self.live = Some(live);
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "OpenCode stop request does not match the active session",
                    false,
                ));
            }
            let abort_result = live.client.abort(&live.provider_thread_id).await;
            live.expected_shutdown.store(true, Ordering::Release);
            live.event_task.abort();
            if let Some(task) = live.command_task.take() {
                task.abort();
            }
            let server_result = if let Some(server) = live.owned_server.as_mut() {
                server.stop().await
            } else {
                Ok(())
            };
            let state = live.route.lock().map_err(|_| driver_state_error())?.clone();
            let event = live.normalizer.external_event(
                &state,
                "session/stopped",
                CanonicalEvent::SessionExited(SessionExitedEvent {
                    session_id: live.session_id.clone(),
                    expected: true,
                    exit_code: None,
                    reason: Some("OpenCode session stopped".to_string()),
                }),
            )?;
            live.sink.emit(event).await?;
            live.sink.flush().await?;
            server_result?;
            abort_result?;
            Ok(operation_receipt(
                &context.operation_id,
                "OpenCode session stopped",
            ))
        })
    }
}

fn probe_state(code: ChatErrorCode) -> ProbeState {
    match code {
        ChatErrorCode::ExecutableMissing => ProbeState::ExecutableMissing,
        ChatErrorCode::UnsupportedVersion | ChatErrorCode::Protocol => {
            ProbeState::UnsupportedVersion
        }
        ChatErrorCode::AuthenticationRequired => ProbeState::AuthenticationRequired,
        ChatErrorCode::ConfigurationInvalid | ChatErrorCode::Validation => {
            ProbeState::ConfigurationInvalid
        }
        _ => ProbeState::TransportUnavailable,
    }
}

fn probe_detail(code: ChatErrorCode) -> &'static str {
    match probe_state(code) {
        ProbeState::Healthy => "OpenCode is ready",
        ProbeState::ExecutableMissing => "OpenCode executable is unavailable",
        ProbeState::UnsupportedVersion => "OpenCode version or protocol is unsupported",
        ProbeState::AuthenticationRequired => "OpenCode provider authentication is required",
        ProbeState::ConfigurationInvalid => "OpenCode configuration is invalid",
        ProbeState::TransportUnavailable => "OpenCode server is unavailable",
    }
}

fn cancelled_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Cancelled,
        "OpenCode operation was cancelled",
        true,
    )
}
