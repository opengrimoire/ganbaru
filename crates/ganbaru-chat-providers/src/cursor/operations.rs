//! Cursor provider driver operations.

use super::driver::*;
use super::executable::acp_continuation_group;
use super::interactions::{
    PendingCursorRequestKind, resolve_approval_result, resolve_question_result,
};
use super::protocol::*;
use super::session::*;
use crate::events::*;
use crate::models::*;
use crate::{
    DriverCancellation, DriverFuture, DriverOperationContext, ProviderAuthoritySupport,
    ProviderDriver, ProviderEventSink,
};
use base64::{Engine as _, engine::general_purpose};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};

const MAX_PROMPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_TEXT_ATTACHMENT_BYTES: usize = 2 * 1024 * 1024;
const PROMPT_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

impl ProviderDriver for CursorProviderDriver {
    fn metadata(&self) -> ProviderFamilyMetadataRead {
        match self.flavor {
            AcpProviderFlavor::Cursor => Self::metadata_read(),
            AcpProviderFlavor::Grok => Self::grok_metadata_read(),
        }
    }

    fn instance_configuration(&self) -> &ProviderInstanceConfig {
        &self.configuration
    }

    fn capabilities(&self) -> ProviderCapabilities {
        potential_capabilities_for(self.flavor)
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
        let Some(live) = self.live.as_ref() else {
            return Ok(Vec::new());
        };
        let state = live.route.lock().map_err(|_| driver_state_error())?;
        Ok(state
            .available_commands
            .iter()
            .map(|command| ChatPromptCatalogEntry {
                value: format!("/{}", command.name),
                label: command.name.clone(),
                description: Some(command.description.clone()).filter(|value| !value.is_empty()),
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
            match self.probe_snapshot(context).await {
                Ok((started, about)) => {
                    let capabilities = negotiated_capabilities_for(
                        self.flavor,
                        &started.initialize,
                        &started.setup,
                    );
                    self.cached_models = Some(ProviderModelCatalog {
                        instance_id: self.configuration.instance_id.clone(),
                        models: started.models,
                        source: ModelCatalogSource::Provider,
                        discovered_at: checked_at.clone(),
                        stale: false,
                    });
                    Ok(ProviderProbeResult {
                        instance_id: self.configuration.instance_id.clone(),
                        state: ProbeState::Healthy,
                        version: Some(about.version),
                        negotiated_protocol_version: Some(
                            started.initialize.protocol_version.to_string(),
                        ),
                        account_label: about.account_label,
                        capabilities,
                        authority_support: self.authority_support(),
                        checked_at,
                        detail: None,
                    })
                }
                Err(error) if context.is_cancelled() => Err(error),
                Err(error) => Ok(ProviderProbeResult {
                    instance_id: self.configuration.instance_id.clone(),
                    state: probe_state(error.code),
                    version: None,
                    negotiated_protocol_version: None,
                    account_label: None,
                    capabilities: potential_capabilities_for(self.flavor),
                    authority_support: self.authority_support(),
                    checked_at,
                    detail: Some(probe_detail(self.flavor, error.code)),
                }),
            }
        })
    }

    fn discover_models<'a>(
        &'a mut self,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderModelCatalog> {
        Box::pin(async move {
            let (started, _) = self.probe_snapshot(context).await?;
            let catalog = ProviderModelCatalog {
                instance_id: self.configuration.instance_id.clone(),
                models: started.models,
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
                    format!(
                        "{} provider instance does not match continuation request",
                        self.flavor.display_name()
                    ),
                    false,
                ));
            }
            acp_continuation_group(
                &self.configuration,
                &self.settings,
                self.flavor,
                request.account_identity.as_deref(),
            )
        })
    }

    fn start_session<'a>(
        &'a mut self,
        request: StartSessionRequest,
        sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        Box::pin(async move {
            self.open_session(CursorSessionInput::Fresh(request), sink, context)
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
            self.open_session(CursorSessionInput::Resume(request), sink, context)
                .await
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        request: SendTurnRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, TurnDispatchReceipt> {
        Box::pin(async move {
            let flavor = self.flavor;
            let provider_name = flavor.display_name();
            let prompt = build_prompt(&request, flavor)?;
            let live = self.live_mut(&request.session_id)?;
            if live
                .prompt_task
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            {
                return Err(ChatError::new(
                    ChatErrorCode::Busy,
                    format!("{provider_name} already has an active prompt"),
                    true,
                ));
            }
            if request
                .attachments
                .iter()
                .any(|attachment| attachment.kind == "image")
                && !live.capabilities.supports(ProviderCapability::Images)
            {
                return Err(ChatError::unsupported(format!(
                    "{provider_name} did not advertise ACP image prompts"
                )));
            }
            if let Some(task) = live.prompt_task.take() {
                let _ = task.await;
            }
            let mut setup = live.setup.lock().map_err(|_| driver_state_error())?.clone();
            let provider_thread_id = live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .provider_thread_id
                .clone();
            apply_provider_configuration(
                flavor,
                &live.connection.client(),
                &provider_thread_id,
                &mut setup,
                AcpRequestedConfiguration {
                    modes: request.modes,
                    model_id: request.model_id.as_ref(),
                    model_options: &request.model_options,
                },
                context,
            )
            .await?;
            *live.setup.lock().map_err(|_| driver_state_error())? = setup.clone();
            let provider_turn_id = ProviderTurnId::new(format!(
                "{}-{}",
                flavor.turn_prefix(),
                request.turn_id.as_str()
            ))
            .map_err(|_| protocol_error("turn ID"))?;
            let started_event = {
                let mut state = live.route.lock().map_err(|_| driver_state_error())?;
                if state.active_turn_id.is_some() {
                    return Err(ChatError::new(
                        ChatErrorCode::Busy,
                        format!("{provider_name} already has an active turn"),
                        true,
                    ));
                }
                state.active_turn_id = Some(request.turn_id.clone());
                state.session_state = ProviderSessionState::Active;
                state.modes = request.modes;
                state.model_id = request.model_id.clone();
                state.config_options = setup.config_options;
                live.normalizer.external_event(
                    &state,
                    "session/prompt/started",
                    None,
                    CanonicalEvent::TurnStarted(TurnStartedEvent {
                        provider_turn_id: Some(provider_turn_id.clone()),
                        state: ChatTurnState::Active,
                        modes: request.modes,
                        model_id: request.model_id.clone(),
                        model_options: request.model_options.clone(),
                    }),
                )?
            };
            live.sink.emit(started_event).await?;
            let client = live.connection.client();
            let route = Arc::clone(&live.route);
            let normalizer = Arc::clone(&live.normalizer);
            let sink = Arc::clone(&live.sink);
            let terminal_error = Arc::clone(&live.terminal_error);
            let prompt_completions = Arc::clone(&live.prompt_completions);
            let prompt_context = DriverOperationContext {
                operation_id: context.operation_id.clone(),
                deadline: Instant::now() + PROMPT_TIMEOUT,
                cancellation: DriverCancellation::default(),
            };
            let prompt_provider_turn_id = provider_turn_id.clone();
            live.prompt_task = Some(tokio::spawn(async move {
                let prompt_id = prompt_provider_turn_id.as_str().to_string();
                let response = if flavor == AcpProviderFlavor::Grok {
                    let (sender, receiver) = tokio::sync::oneshot::channel();
                    let inserted = prompt_completions
                        .lock()
                        .map(|mut pending| {
                            if pending.len() >= 64 || pending.contains_key(&prompt_id) {
                                false
                            } else {
                                pending.insert(prompt_id.clone(), sender);
                                true
                            }
                        })
                        .unwrap_or(false);
                    if !inserted {
                        Err(super::transport::AcpRpcFailure::Unavailable)
                    } else {
                        let result = client
                            .request_with_fallback(
                                "session/prompt",
                                json!({
                                    "sessionId": provider_thread_id,
                                    "prompt": prompt,
                                    "_meta": { "promptId": prompt_id },
                                }),
                                receiver,
                                &prompt_context,
                            )
                            .await;
                        if let Ok(mut pending) = prompt_completions.lock() {
                            pending.remove(&prompt_id);
                        }
                        result
                    }
                } else {
                    client
                        .request(
                            "session/prompt",
                            json!({ "sessionId": provider_thread_id, "prompt": prompt }),
                            &prompt_context,
                        )
                        .await
                };
                let events = match response {
                    Ok(response) => {
                        let stop_reason = response
                            .get("stopReason")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        route
                            .lock()
                            .map_err(|_| driver_state_error())
                            .and_then(|mut state| normalizer.finish_turn(&mut state, stop_reason))
                    }
                    Err(error) => {
                        let chat_error = error.to_chat_error("prompt");
                        if let Ok(mut terminal) = terminal_error.lock() {
                            if terminal.is_none() && chat_error.code != ChatErrorCode::Cancelled {
                                *terminal = Some(chat_error.clone());
                            }
                        }
                        route
                            .lock()
                            .map_err(|_| driver_state_error())
                            .and_then(|mut state| {
                                normalizer.interrupted(&mut state, &chat_error.message)
                            })
                    }
                };
                if let Ok(events) = events {
                    for event in events {
                        let _ = sink.emit(event).await;
                    }
                    let _ = sink.flush().await;
                }
            }));
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
        _request: SteerTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        let provider_name = self.flavor.display_name();
        Box::pin(async move {
            Err(ChatError::unsupported(format!(
                "{provider_name} steering is not declared"
            )))
        })
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        request: InterruptTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let provider_name = self.flavor.display_name();
            let live = self.live_mut(&request.session_id)?;
            let provider_thread_id = {
                let state = live.route.lock().map_err(|_| driver_state_error())?;
                match state.active_turn_id.as_ref() {
                    Some(active) if active == &request.turn_id => {}
                    Some(_) => {
                        return Err(ChatError::new(
                            ChatErrorCode::Conflict,
                            format!("{provider_name} cancellation does not match the active turn"),
                            false,
                        ));
                    }
                    None => {
                        return Ok(operation_receipt(
                            request.command.client_command_id.as_str(),
                            &format!("{provider_name} turn was already settled"),
                        ));
                    }
                }
                state.provider_thread_id.clone()
            };
            live.connection
                .client()
                .notify("session/cancel", json!({ "sessionId": provider_thread_id }))
                .await
                .map_err(|error| error.to_chat_error("turn cancellation"))?;
            Ok(operation_receipt(
                request.command.client_command_id.as_str(),
                &format!("{provider_name} turn cancellation requested"),
            ))
        })
    }

    fn resolve_approval<'a>(
        &'a mut self,
        request: ResolveApprovalRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let provider_name = self.flavor.display_name();
            let live = self.live_mut(&request.session_id)?;
            let pending = take_pending(&live.pending, &request.provider_request_id)?;
            if !matches!(&pending.kind, PendingCursorRequestKind::Approval { .. }) {
                restore_pending(&live.pending, request.provider_request_id.clone(), pending);
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    format!("{provider_name} request is not an approval"),
                    false,
                ));
            }
            let result = match resolve_approval_result(&pending, &request.decision) {
                Ok(result) => result,
                Err(error) => {
                    restore_pending(&live.pending, request.provider_request_id.clone(), pending);
                    return Err(error);
                }
            };
            if let Err(error) = live
                .connection
                .client()
                .respond(pending.rpc_id.clone(), result)
                .await
            {
                restore_pending(&live.pending, request.provider_request_id.clone(), pending);
                return Err(error.to_chat_error("permission response"));
            }
            if request.decision.kind == ApprovalDecisionKind::Cancel {
                let provider_thread_id = live
                    .route
                    .lock()
                    .map_err(|_| driver_state_error())?
                    .provider_thread_id
                    .clone();
                let _ = live
                    .connection
                    .client()
                    .notify("session/cancel", json!({ "sessionId": provider_thread_id }))
                    .await;
            }
            let event = {
                let state = live.route.lock().map_err(|_| driver_state_error())?;
                live.normalizer.external_event(
                    &state,
                    "session/request_permission/resolved",
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
                &format!("{provider_name} approval response accepted"),
            ))
        })
    }

    fn resolve_user_input<'a>(
        &'a mut self,
        request: ResolveUserInputRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let provider_name = self.flavor.display_name();
            let live = self.live_mut(&request.session_id)?;
            let pending = take_pending(&live.pending, &request.provider_request_id)?;
            if !matches!(
                &pending.kind,
                PendingCursorRequestKind::UserInput { .. }
                    | PendingCursorRequestKind::XaiUserInput { .. }
            ) {
                restore_pending(&live.pending, request.provider_request_id.clone(), pending);
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    format!("{provider_name} request is not structured input"),
                    false,
                ));
            }
            let result = match resolve_question_result(&pending, &request.answers) {
                Ok(result) => result,
                Err(error) => {
                    restore_pending(&live.pending, request.provider_request_id.clone(), pending);
                    return Err(error);
                }
            };
            if let Err(error) = live
                .connection
                .client()
                .respond(pending.rpc_id.clone(), result)
                .await
            {
                restore_pending(&live.pending, request.provider_request_id.clone(), pending);
                return Err(error.to_chat_error("structured question response"));
            }
            let event = {
                let state = live.route.lock().map_err(|_| driver_state_error())?;
                live.normalizer.external_event(
                    &state,
                    "cursor/ask_question/resolved",
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
                &format!("{provider_name} structured answer accepted"),
            ))
        })
    }

    fn rollback<'a>(
        &'a mut self,
        _request: RollbackRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        let provider_name = self.flavor.display_name();
        Box::pin(async move {
            Err(ChatError::unsupported(format!(
                "{provider_name} rollback is not supported"
            )))
        })
    }

    fn read_history<'a>(
        &'a mut self,
        _request: ReadHistoryRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderHistoryPage> {
        let provider_name = self.flavor.display_name();
        Box::pin(async move {
            Err(ChatError::unsupported(format!(
                "{provider_name} ACP replays history only during session loading"
            )))
        })
    }

    fn stop_session<'a>(
        &'a mut self,
        request: StopSessionRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async move {
            let provider_name = self.flavor.display_name();
            let Some(mut live) = self.live.take() else {
                return Ok(operation_receipt(
                    &context.operation_id,
                    &format!("{provider_name} session was already stopped"),
                ));
            };
            if live.session_id != request.session_id {
                self.live = Some(live);
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    format!("{provider_name} session identity does not match the active session"),
                    false,
                ));
            }
            live.expected_shutdown.store(true, Ordering::Release);
            let provider_thread_id = live
                .route
                .lock()
                .map_err(|_| driver_state_error())?
                .provider_thread_id
                .clone();
            let _ = live
                .connection
                .client()
                .notify("session/cancel", json!({ "sessionId": provider_thread_id }))
                .await;
            cancel_pending(&live).await;
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
            if let Some(mut task) = live.prompt_task.take() {
                let _ = tokio::time::timeout(SESSION_GRACEFUL_STOP, &mut task).await;
                if !task.is_finished() {
                    task.abort();
                }
            }
            let _ = tokio::time::timeout(SESSION_GRACEFUL_STOP, &mut live.router_task).await;
            if !live.router_task.is_finished() {
                live.router_task.abort();
            }
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
                        reason: Some(format!("{provider_name} session stopped")),
                    }),
                )?)
                .await?;
            live.sink.flush().await?;
            Ok(operation_receipt(
                &context.operation_id,
                &format!("{provider_name} session stopped"),
            ))
        })
    }
}

fn build_prompt(request: &SendTurnRequest, flavor: AcpProviderFlavor) -> ChatResult<Vec<Value>> {
    let provider_name = flavor.display_name();
    if request.prompt.len() > MAX_PROMPT_BYTES || request.prompt.contains('\0') {
        return Err(ChatError::validation(
            "prompt",
            format!("{provider_name} prompt exceeds the supported limit"),
        ));
    }
    let mut text = request.prompt.clone();
    if !request.mentions.is_empty() {
        text.push_str("\n\nReferenced workspace paths:\n");
        text.push_str(
            &request
                .mentions
                .iter()
                .map(|mention| format!("@{}", mention.relative_path))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if let Some(instructions) = request.developer_instructions.as_deref() {
        if instructions.len() > MAX_TEXT_ATTACHMENT_BYTES || instructions.contains('\0') {
            return Err(ChatError::validation(
                "developerInstructions",
                format!("{provider_name} developer instructions exceed the supported limit"),
            ));
        }
        text.push_str("\n\nAdditional Ganbaru instructions:\n");
        text.push_str(instructions);
    }
    let mut content = Vec::new();
    if !text.trim().is_empty() {
        content.push(json!({ "type": "text", "text": text }));
    }
    for attachment in &request.attachments {
        match attachment.kind.as_str() {
            "image" => content.push(image_content(attachment, flavor)?),
            "text_snippet" => {
                let text = attachment.text_content.as_deref().ok_or_else(|| {
                    ChatError::validation(
                        "attachments",
                        format!("{provider_name} text context is unavailable"),
                    )
                })?;
                if text.len() > MAX_TEXT_ATTACHMENT_BYTES || text.contains('\0') {
                    return Err(ChatError::validation(
                        "attachments",
                        format!("{provider_name} text context exceeds the supported limit"),
                    ));
                }
                content.push(json!({ "type": "text", "text": text }));
            }
            _ => {
                return Err(ChatError::unsupported(format!(
                    "{provider_name} prompt attachment kind is unsupported"
                )));
            }
        }
    }
    if content.is_empty() {
        return Err(ChatError::validation(
            "prompt",
            format!("{provider_name} turn requires text or an image"),
        ));
    }
    Ok(content)
}

fn image_content(
    attachment: &PromptAttachmentReference,
    flavor: AcpProviderFlavor,
) -> ChatResult<Value> {
    let provider_name = flavor.display_name();
    let path = attachment.local_path.as_deref().ok_or_else(|| {
        ChatError::validation(
            "attachments",
            format!("{provider_name} image attachment is unavailable"),
        )
    })?;
    let path = Path::new(path);
    let metadata = fs::metadata(path).map_err(|_| {
        ChatError::validation(
            "attachments",
            format!("{provider_name} image attachment is unavailable"),
        )
    })?;
    if !path.is_absolute() || !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
        return Err(ChatError::validation(
            "attachments",
            format!("{provider_name} image attachment is invalid or oversized"),
        ));
    }
    let mime = attachment
        .mime_type
        .as_deref()
        .filter(|value| {
            matches!(
                *value,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
        })
        .ok_or_else(|| {
            ChatError::validation(
                "attachments",
                format!("{provider_name} image type is unsupported"),
            )
        })?;
    let bytes = fs::read(path).map_err(|_| {
        ChatError::validation(
            "attachments",
            format!("{provider_name} image attachment could not be read"),
        )
    })?;
    Ok(json!({
        "type": "image",
        "data": general_purpose::STANDARD.encode(bytes),
        "mimeType": mime,
    }))
}

async fn cancel_pending(live: &CursorLiveSession) {
    let pending = live
        .pending
        .lock()
        .map(|mut pending| {
            pending
                .drain()
                .map(|(_, request)| request)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for request in pending {
        let _ = live
            .connection
            .client()
            .respond_error(request.rpc_id, -32800, "Request cancelled")
            .await;
    }
}

fn operation_receipt(operation_id: &str, detail: &str) -> DriverOperationReceipt {
    DriverOperationReceipt {
        operation_id: operation_id.to_string(),
        accepted: true,
        detail: Some(detail.to_string()),
    }
}

fn probe_state(code: ChatErrorCode) -> ProbeState {
    match code {
        ChatErrorCode::ExecutableMissing => ProbeState::ExecutableMissing,
        ChatErrorCode::UnsupportedVersion => ProbeState::UnsupportedVersion,
        ChatErrorCode::AuthenticationRequired => ProbeState::AuthenticationRequired,
        ChatErrorCode::ConfigurationInvalid | ChatErrorCode::Validation => {
            ProbeState::ConfigurationInvalid
        }
        _ => ProbeState::TransportUnavailable,
    }
}

fn probe_detail(flavor: AcpProviderFlavor, code: ChatErrorCode) -> String {
    let provider_name = flavor.display_name();
    match code {
        ChatErrorCode::ExecutableMissing => {
            format!("{provider_name} executable is unavailable")
        }
        ChatErrorCode::UnsupportedVersion => {
            format!("{provider_name} version or ACP capability is unsupported")
        }
        ChatErrorCode::AuthenticationRequired => {
            format!("{provider_name} authentication is required")
        }
        ChatErrorCode::ConfigurationInvalid | ChatErrorCode::Validation => {
            format!("{provider_name} provider configuration is invalid")
        }
        _ => format!("{provider_name} ACP transport is unavailable"),
    }
}
