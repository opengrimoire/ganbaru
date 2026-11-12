use ganbaru_chat_contracts::events::{CanonicalEvent, CanonicalRuntimeEvent};
use ganbaru_chat_contracts::models::*;
use ganbaru_chat_providers::{
    DriverFuture, DriverOperationContext, ProviderDriver, ProviderEventSink,
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Default)]
pub struct FakeDriverControl {
    sink: Mutex<Option<Arc<dyn ProviderEventSink>>>,
    events: Mutex<Vec<CanonicalRuntimeEvent>>,
    crash_next_send: AtomicBool,
    hang_on_stop: AtomicBool,
    send_delay_millis: AtomicUsize,
    sends_in_flight: AtomicUsize,
    maximum_sends_in_flight: AtomicUsize,
    opened_instructions: Mutex<Vec<Option<String>>>,
    native_rollback: AtomicBool,
    rollback_requests: Mutex<Vec<RollbackRequest>>,
}

impl FakeDriverControl {
    /// Makes later sessions advertise native rollback and accept rollback requests.
    pub fn enable_native_rollback(&self) {
        self.native_rollback.store(true, Ordering::Release);
    }

    /// Every rollback request the driver accepted, in order.
    pub fn rollback_requests(&self) -> Vec<RollbackRequest> {
        self.rollback_requests.lock().unwrap().clone()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        if !self.native_rollback.load(Ordering::Acquire) {
            return ProviderCapabilities::default();
        }
        ProviderCapabilities {
            entries: vec![ProviderCapabilitySupport {
                capability: ProviderCapability::NativeRollback,
                supported: true,
                explanation: None,
            }],
        }
    }

    pub fn crash_next_send(&self) {
        self.crash_next_send.store(true, Ordering::Release);
    }

    pub fn hang_on_stop(&self) {
        self.hang_on_stop.store(true, Ordering::Release);
    }

    pub fn set_send_delay(&self, duration: Duration) {
        self.send_delay_millis
            .store(duration.as_millis() as usize, Ordering::Release);
    }

    pub fn maximum_sends_in_flight(&self) -> usize {
        self.maximum_sends_in_flight.load(Ordering::Acquire)
    }

    pub fn events(&self) -> Vec<CanonicalRuntimeEvent> {
        self.events.lock().unwrap().clone()
    }

    /// The instructions of every start or resume request, in order.
    pub fn opened_instructions(&self) -> Vec<Option<String>> {
        self.opened_instructions.lock().unwrap().clone()
    }

    pub async fn emit_late(&self) -> ChatResult<()> {
        let sink = self.sink.lock().unwrap().clone().unwrap();
        sink.emit(event(
            "late-event",
            None,
            CanonicalEvent::RuntimeWarning(ganbaru_chat_contracts::events::NotificationEvent {
                code: "stale_session_generation".to_string(),
                title: "Late provider output".to_string(),
                detail: Some("This output belongs to a stopped generation".to_string()),
            }),
        ))
        .await
    }

    pub async fn emit_late_content(&self, turn_id: &str) -> ChatResult<()> {
        let sink = self.sink.lock().unwrap().clone().unwrap();
        sink.emit(content_event("late-content", turn_id, "Late content"))
            .await
    }

    /// Emits the provider's own interrupted abort of `turn_id`, as a provider that confirms Stop
    /// late would.
    pub async fn emit_late_abort(&self, turn_id: &str) -> ChatResult<()> {
        let sink = self.sink.lock().unwrap().clone().unwrap();
        sink.emit(event(
            "late-abort",
            Some(turn_id),
            CanonicalEvent::TurnAborted(ganbaru_chat_contracts::events::TurnAbortedEvent {
                state: ganbaru_chat_contracts::models::ChatTurnState::Interrupted,
                reason: "Interrupted".to_string(),
                recoverable: true,
            }),
        ))
        .await
    }

    pub async fn emit_cross_thread(&self) -> ChatResult<()> {
        let sink = self.sink.lock().unwrap().clone().unwrap();
        let mut runtime_event = event(
            "cross-thread-event",
            None,
            CanonicalEvent::RuntimeWarning(ganbaru_chat_contracts::events::NotificationEvent {
                code: "provider_warning".to_string(),
                title: "Provider warning".to_string(),
                detail: None,
            }),
        );
        runtime_event.thread_id = ChatThreadId::new("other-thread").unwrap();
        sink.emit(runtime_event).await
    }
}

pub struct RecordingEventSink {
    control: Arc<FakeDriverControl>,
}

impl RecordingEventSink {
    pub fn new(control: Arc<FakeDriverControl>) -> Self {
        Self { control }
    }
}

impl ProviderEventSink for RecordingEventSink {
    fn emit<'a>(&'a self, event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            self.control.events.lock().unwrap().push(event);
            Ok(())
        })
    }
}

pub fn fake_driver(control: Arc<FakeDriverControl>) -> Box<dyn ProviderDriver> {
    Box::new(FakeProviderDriver {
        configuration: serde_json::from_value(json!({
            "schemaVersion": 1,
            "instanceId": "fake-instance",
            "familyId": "fake",
            "label": "Fake provider",
            "enabled": true,
            "executable": "/fake/provider",
            "providerHome": null,
            "launchArguments": [],
            "environment": {},
            "credentialReferences": {},
            "visibleModelIds": [],
            "favoriteModelIds": [],
            "providerConfig": { "schemaVersion": 1, "value": {} }
        }))
        .unwrap(),
        control,
        active_turn_id: None,
    })
}

struct FakeProviderDriver {
    configuration: ProviderInstanceConfig,
    control: Arc<FakeDriverControl>,
    active_turn_id: Option<ChatTurnId>,
}

impl FakeProviderDriver {
    fn record_opened(&self, instructions: Option<String>) {
        self.control
            .opened_instructions
            .lock()
            .unwrap()
            .push(instructions);
    }

    fn unavailable<'a, T>(&self) -> DriverFuture<'a, T> {
        Box::pin(async { Err(ChatError::unsupported("Fake operation is not implemented")) })
    }

    async fn emit(&self, event: CanonicalRuntimeEvent) -> ChatResult<()> {
        let sink = self.control.sink.lock().unwrap().clone().unwrap();
        sink.emit(event).await
    }

    fn enter_send(&self) -> SendGuard<'_> {
        let current = self.control.sends_in_flight.fetch_add(1, Ordering::AcqRel) + 1;
        self.control
            .maximum_sends_in_flight
            .fetch_max(current, Ordering::AcqRel);
        SendGuard(&self.control.sends_in_flight)
    }
}

struct SendGuard<'a>(&'a AtomicUsize);

impl Drop for SendGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl ProviderDriver for FakeProviderDriver {
    fn metadata(&self) -> ProviderFamilyMetadataRead {
        serde_json::from_value(json!({
            "familyId": "fake",
            "displayName": "Fake provider",
            "configurationSchemaVersion": 1,
            "supportedPlatforms": ["test"],
            "minimumTestedCliVersion": null,
            "defaultExecutableCandidates": [],
            "implementationStatus": "available",
            "potentialCapabilities": [],
            "unavailableReason": null
        }))
        .unwrap()
    }

    fn instance_configuration(&self) -> &ProviderInstanceConfig {
        &self.configuration
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.control.capabilities()
    }

    fn probe<'a>(
        &'a mut self,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderProbeResult> {
        self.unavailable()
    }

    fn discover_models<'a>(
        &'a mut self,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderModelCatalog> {
        self.unavailable()
    }

    fn derive_continuation_group<'a>(
        &'a mut self,
        _request: ContinuationGroupRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ContinuationGroupId> {
        self.unavailable()
    }

    fn start_session<'a>(
        &'a mut self,
        request: StartSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        self.record_opened(request.developer_instructions);
        Box::pin(async move {
            *self.control.sink.lock().unwrap() = Some(event_sink);
            self.emit(event(
                "session-started",
                None,
                serde_json::from_value(json!({
                    "type": "session_started",
                    "payload": {
                        "sessionId": "fake-session",
                        "state": "ready",
                        "providerThreadId": "fake-provider-thread",
                        "resumeCursor": { "schemaVersion": 1, "value": { "cursor": 1 } },
                        "effectiveModes": modes_json(),
                        "capabilityOverrides": { "entries": [] }
                    }
                }))
                .unwrap(),
            ))
            .await?;
            let mut snapshot = session_snapshot();
            snapshot.capabilities = self.control.capabilities();
            Ok(snapshot)
        })
    }

    fn resume_session<'a>(
        &'a mut self,
        request: ResumeSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderSessionSnapshot> {
        self.start_session(
            StartSessionRequest {
                developer_instructions: request.developer_instructions,
                ..start_request()
            },
            event_sink,
            context,
        )
    }

    fn send_turn<'a>(
        &'a mut self,
        request: SendTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, TurnDispatchReceipt> {
        self.active_turn_id = Some(request.turn_id.clone());
        Box::pin(async move {
            let _guard = self.enter_send();
            let delay = self.control.send_delay_millis.load(Ordering::Acquire);
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay as u64)).await;
            }
            self.emit(event(
                &format!("turn-started-{}", request.turn_id),
                Some(request.turn_id.as_str()),
                serde_json::from_value(json!({
                    "type": "turn_started",
                    "payload": {
                        "providerTurnId": "fake-provider-turn",
                        "state": "active",
                        "modes": modes_json(),
                        "modelId": null,
                        "modelOptions": []
                    }
                }))
                .unwrap(),
            ))
            .await?;
            self.emit(content_event(
                &format!("content-{}", request.turn_id),
                request.turn_id.as_str(),
                "Streaming response",
            ))
            .await?;
            if self.control.crash_next_send.swap(false, Ordering::AcqRel) {
                return Err(ChatError::driver_unavailable("Fake provider crashed"));
            }
            self.emit(event(
                &format!("request-{}", request.turn_id),
                Some(request.turn_id.as_str()),
                serde_json::from_value(json!({
                    "type": "request_opened",
                    "payload": {
                        "requestId": "fake-provider-request",
                        "kind": "file_change",
                        "title": "Allow file edit?",
                        "detail": null,
                        "allowedDecisions": [{
                            "id": "allow",
                            "label": "Allow",
                            "decisionKind": "allow_once",
                            "description": null
                        }],
                        "safePayload": { "schemaVersion": 1, "value": {} }
                    }
                }))
                .unwrap(),
            ))
            .await?;
            Ok(serde_json::from_value(json!({
                "turnId": request.turn_id,
                "state": "waiting_for_approval",
                "providerTurnId": "fake-provider-turn",
                "acceptedAt": "2026-07-20T00:00:01Z"
            }))
            .unwrap())
        })
    }

    fn steer_turn<'a>(
        &'a mut self,
        _request: SteerTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable()
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        _request: InterruptTurnRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        Box::pin(async { Ok(operation_receipt("fake-interrupt")) })
    }

    fn resolve_approval<'a>(
        &'a mut self,
        request: ResolveApprovalRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        let turn_id = self.active_turn_id.take().unwrap();
        Box::pin(async move {
            self.emit(event(
                "request-resolved",
                Some(turn_id.as_str()),
                serde_json::from_value(json!({
                    "type": "request_resolved",
                    "payload": {
                        "requestId": request.provider_request_id,
                        "state": "resolved",
                        "decision": request.decision
                    }
                }))
                .unwrap(),
            ))
            .await?;
            self.emit(event(
                "turn-completed",
                Some(turn_id.as_str()),
                serde_json::from_value(json!({
                    "type": "turn_completed",
                    "payload": {
                        "state": "completed",
                        "stopReason": "done",
                        "usage": null,
                        "changedFiles": []
                    }
                }))
                .unwrap(),
            ))
            .await?;
            Ok(operation_receipt("fake-approval"))
        })
    }

    fn resolve_user_input<'a>(
        &'a mut self,
        _request: ResolveUserInputRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        self.unavailable()
    }

    fn rollback<'a>(
        &'a mut self,
        request: RollbackRequest,
        context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        if !self.control.native_rollback.load(Ordering::Acquire) {
            return self.unavailable();
        }
        self.control.rollback_requests.lock().unwrap().push(request);
        Box::pin(async move { Ok(operation_receipt(&context.operation_id)) })
    }

    fn read_history<'a>(
        &'a mut self,
        _request: ReadHistoryRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, ProviderHistoryPage> {
        self.unavailable()
    }

    fn stop_session<'a>(
        &'a mut self,
        _request: StopSessionRequest,
        _context: &'a DriverOperationContext,
    ) -> DriverFuture<'a, DriverOperationReceipt> {
        let active_turn_id = self.active_turn_id.take();
        Box::pin(async move {
            if let Some(turn_id) = active_turn_id {
                self.emit(content_event(
                    "content-during-stop",
                    turn_id.as_str(),
                    "Output received while stopping",
                ))
                .await?;
            }
            if self.control.hang_on_stop.load(Ordering::Acquire) {
                std::future::pending::<()>().await;
            }
            self.emit(event(
                "session-exited",
                None,
                serde_json::from_value(json!({
                    "type": "session_exited",
                    "payload": {
                        "sessionId": "fake-session",
                        "expected": true,
                        "exitCode": 0,
                        "reason": "Stopped"
                    }
                }))
                .unwrap(),
            ))
            .await?;
            Ok(operation_receipt("fake-stop"))
        })
    }
}

pub fn start_request() -> StartSessionRequest {
    serde_json::from_value(json!({
        "threadId": "thread-1",
        "workspace": {
            "workingFolderId": "fake-workspace",
            "canonicalPath": "/fake/workspace",
            "repositoryKind": "git",
            "repositoryIdentity": "fake-repository"
        },
        "providerInstanceId": "fake-instance",
        "modes": modes_json(),
        "modelId": null,
        "modelOptions": []
    }))
    .unwrap()
}

pub fn send_request(turn_id: &str) -> SendTurnRequest {
    serde_json::from_value(json!({
        "command": {
            "clientCommandId": format!("send-{turn_id}"),
            "expectedThreadRevision": 1
        },
        "sessionId": "fake-session",
        "turnId": turn_id,
        "prompt": "Test prompt",
        "attachments": [],
        "mentions": [],
        "modelId": null,
        "modelOptions": [],
        "modes": modes_json(),
        "developerInstructions": null
    }))
    .unwrap()
}

pub fn approval_request(turn_id: &str) -> ResolveApprovalRequest {
    serde_json::from_value(json!({
        "command": {
            "clientCommandId": format!("approve-{turn_id}"),
            "expectedThreadRevision": 2
        },
        "sessionId": "fake-session",
        "requestId": format!("chat-request-{turn_id}"),
        "providerRequestId": "fake-provider-request",
        "decision": {
            "kind": "allow_once",
            "providerOptionId": "allow",
            "updatedToolInput": null
        }
    }))
    .unwrap()
}

pub fn operation_context(operation_id: &str, timeout: Duration) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: std::time::Instant::now() + timeout,
        cancellation: Default::default(),
    }
}

fn session_snapshot() -> ProviderSessionSnapshot {
    serde_json::from_value(json!({
        "sessionId": "fake-session",
        "state": "ready",
        "providerThreadId": "fake-provider-thread",
        "continuationGroupId": "fake-continuation",
        "resumeCursor": { "schemaVersion": 1, "value": { "cursor": 1 } },
        "effectiveModes": modes_json(),
        "capabilities": { "entries": [] },
        "startedAt": "2026-07-20T00:00:00Z"
    }))
    .unwrap()
}

fn operation_receipt(operation_id: &str) -> DriverOperationReceipt {
    DriverOperationReceipt {
        accepted: true,
        operation_id: operation_id.to_string(),
        detail: None,
    }
}

fn event(event_id: &str, turn_id: Option<&str>, event: CanonicalEvent) -> CanonicalRuntimeEvent {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "eventId": event_id,
        "providerFamilyId": "fake",
        "providerInstanceId": "fake-instance",
        "threadId": "thread-1",
        "createdAt": "2026-07-20T00:00:02Z",
        "turnId": turn_id,
        "providerTurnId": null,
        "providerItemId": null,
        "providerRequestId": null,
        "providerTaskId": null,
        "providerReference": null,
        "event": event,
        "redactedDiagnostic": null
    }))
    .unwrap()
}

fn content_event(event_id: &str, turn_id: &str, delta: &str) -> CanonicalRuntimeEvent {
    event(
        event_id,
        Some(turn_id),
        serde_json::from_value(json!({
            "type": "content_delta",
            "payload": {
                "itemId": "fake-assistant-item",
                "streamKind": "assistant_text",
                "contentIndex": 0,
                "delta": delta
            }
        }))
        .unwrap(),
    )
}

fn modes_json() -> serde_json::Value {
    json!({ "safetyMode": "ask_for_approval", "interactionMode": "build" })
}
