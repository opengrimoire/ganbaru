//! Per-thread provider session ownership, bounded command routing, and shutdown.

use crate::events::{CanonicalEvent, CanonicalRuntimeEvent, NotificationEvent};
use crate::models::{
    ChatCommandContext, ChatCommandId, ChatError, ChatErrorCode, ChatResult, ChatThreadId,
    ChatTurnId, CompactContextRequest, DriverOperationReceipt, InterruptTurnRequest, McpStatusRead,
    McpStatusRequest, ProviderSessionId, ProviderSessionSnapshot, ProviderSessionState,
    ResolveApprovalRequest, ResolveUserInputRequest, ResumeSessionRequest, RollbackRequest,
    SendTurnRequest, StartSessionRequest, SteerTurnRequest, StopSessionRequest,
    TurnDispatchReceipt,
};
use crate::providers::{
    DriverCancellation, DriverFuture, DriverOperationContext, ProviderDriver, ProviderEventSink,
};
use crate::workspace::mutation::{
    ChatWorkspaceMutationRegistry, ProviderTurnReservationHandoff, ThreadReservationCleanup,
};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, mpsc, oneshot};
use tokio::task::JoinHandle;

mod event_sink;
mod helpers;
mod worker;

#[cfg(test)]
use event_sink::event_matches_active_turn;
use helpers::*;
use worker::run_thread_runtime;

const DEFAULT_COMMAND_CAPACITY: usize = 32;
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(2);
const STOP_FLUSH_INTERVAL: Duration = Duration::from_millis(32);

pub enum ThreadRuntimeCommand {
    SessionState(ProviderSessionState),
    TurnActive(bool),
    PendingRequest(bool),
    TerminalLinkedOperation(bool),
    Touch,
    PromptCatalog {
        response: oneshot::Sender<ChatResult<Vec<crate::models::ChatPromptCatalogEntry>>>,
    },
    CompactContext {
        request: CompactContextRequest,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<DriverOperationReceipt>>,
    },
    ReadMcpStatus {
        request: McpStatusRequest,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<McpStatusRead>>,
    },
    StartSession {
        driver: Box<dyn ProviderDriver>,
        request: StartSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<ProviderSessionSnapshot>>,
    },
    ResumeSession {
        driver: Box<dyn ProviderDriver>,
        request: ResumeSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<ProviderSessionSnapshot>>,
    },
    SendTurn {
        request: SendTurnRequest,
        reservation: ProviderTurnReservationHandoff,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<TurnDispatchReceipt>>,
    },
    SteerTurn {
        request: SteerTurnRequest,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<DriverOperationReceipt>>,
    },
    ResolveApproval {
        request: ResolveApprovalRequest,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<DriverOperationReceipt>>,
    },
    ResolveUserInput {
        request: ResolveUserInputRequest,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<DriverOperationReceipt>>,
    },
    InterruptTurn {
        request: InterruptTurnRequest,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<DriverOperationReceipt>>,
    },
    Rollback {
        request: RollbackRequest,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<DriverOperationReceipt>>,
    },
    StopSession {
        force: bool,
        reservation_cleanup: Option<ThreadReservationCleanup>,
        context: DriverOperationContext,
        response: oneshot::Sender<ChatResult<DriverOperationReceipt>>,
    },
    Shutdown {
        deadline: Instant,
        reservation_cleanup: ThreadReservationCleanup,
        response: oneshot::Sender<ChatResult<()>>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadRuntimeSnapshot {
    pub session_id: Option<ProviderSessionId>,
    pub session_state: ProviderSessionState,
    pub capabilities: crate::models::ProviderCapabilities,
    pub active_turn_id: Option<ChatTurnId>,
    pub turn_active: bool,
    pub pending_request: bool,
    pub terminal_linked_operation: bool,
    pub accepting_commands: bool,
    pub generation: u64,
    pub last_activity: Instant,
}

impl ThreadRuntimeSnapshot {
    pub fn can_idle_stop(&self, now: Instant, idle_timeout: Duration) -> bool {
        self.accepting_commands
            && self.session_state == ProviderSessionState::Ready
            && !self.turn_active
            && !self.pending_request
            && !self.terminal_linked_operation
            && now.saturating_duration_since(self.last_activity) >= idle_timeout
    }
}

pub struct ThreadRuntimeOwner {
    thread_id: ChatThreadId,
    command_sender: mpsc::Sender<ThreadRuntimeCommand>,
    operation_lock: Arc<AsyncMutex<()>>,
    snapshot: Arc<Mutex<ThreadRuntimeSnapshot>>,
    worker_task: Mutex<Option<JoinHandle<()>>>,
}

impl ThreadRuntimeOwner {
    pub fn thread_id(&self) -> &ChatThreadId {
        &self.thread_id
    }

    pub fn snapshot(&self) -> ChatResult<ThreadRuntimeSnapshot> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| runtime_state_error())
    }

    pub fn try_command(&self, command: ThreadRuntimeCommand) -> ChatResult<()> {
        if !self.snapshot()?.accepting_commands {
            return Err(runtime_unavailable_error());
        }
        self.try_send(command)
    }

    pub async fn lock_operation(&self) -> OwnedMutexGuard<()> {
        Arc::clone(&self.operation_lock).lock_owned().await
    }

    pub async fn start_session(
        &self,
        driver: Box<dyn ProviderDriver>,
        request: StartSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: DriverOperationContext,
    ) -> ChatResult<ProviderSessionSnapshot> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::StartSession {
            driver,
            request,
            event_sink,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn prompt_catalog(&self) -> ChatResult<Vec<crate::models::ChatPromptCatalogEntry>> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::PromptCatalog { response })?;
        receive_response(receiver).await
    }

    pub async fn compact_context(
        &self,
        request: CompactContextRequest,
        context: DriverOperationContext,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::CompactContext {
            request,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn read_mcp_status(
        &self,
        request: McpStatusRequest,
        context: DriverOperationContext,
    ) -> ChatResult<McpStatusRead> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::ReadMcpStatus {
            request,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn send_turn(
        &self,
        request: SendTurnRequest,
        reservation: ProviderTurnReservationHandoff,
        context: DriverOperationContext,
    ) -> ChatResult<TurnDispatchReceipt> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::SendTurn {
            request,
            reservation,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn resume_session(
        &self,
        driver: Box<dyn ProviderDriver>,
        request: ResumeSessionRequest,
        event_sink: Arc<dyn ProviderEventSink>,
        context: DriverOperationContext,
    ) -> ChatResult<ProviderSessionSnapshot> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::ResumeSession {
            driver,
            request,
            event_sink,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn steer_turn(
        &self,
        request: SteerTurnRequest,
        context: DriverOperationContext,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::SteerTurn {
            request,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn resolve_approval(
        &self,
        request: ResolveApprovalRequest,
        context: DriverOperationContext,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::ResolveApproval {
            request,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn interrupt_turn(
        &self,
        request: InterruptTurnRequest,
        context: DriverOperationContext,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::InterruptTurn {
            request,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn resolve_user_input(
        &self,
        request: ResolveUserInputRequest,
        context: DriverOperationContext,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::ResolveUserInput {
            request,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn rollback(
        &self,
        request: RollbackRequest,
        context: DriverOperationContext,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::Rollback {
            request,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    pub async fn stop_session(
        &self,
        force: bool,
        context: DriverOperationContext,
    ) -> ChatResult<DriverOperationReceipt> {
        self.stop_session_inner(force, context, None).await
    }

    pub async fn stop_session_and_release(
        &self,
        force: bool,
        context: DriverOperationContext,
        mutations: &ChatWorkspaceMutationRegistry,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        let cleanup = mutations.thread_cleanup(&self.thread_id);
        self.queue_stop_session(force, context, Some(cleanup)).await
    }

    async fn stop_session_inner(
        &self,
        force: bool,
        context: DriverOperationContext,
        reservation_cleanup: Option<ThreadReservationCleanup>,
    ) -> ChatResult<DriverOperationReceipt> {
        let _guard = self.lock_operation().await;
        self.queue_stop_session(force, context, reservation_cleanup)
            .await
    }

    async fn queue_stop_session(
        &self,
        force: bool,
        context: DriverOperationContext,
        reservation_cleanup: Option<ThreadReservationCleanup>,
    ) -> ChatResult<DriverOperationReceipt> {
        let (response, receiver) = oneshot::channel();
        self.try_command(ThreadRuntimeCommand::StopSession {
            force,
            reservation_cleanup,
            context,
            response,
        })?;
        receive_response(receiver).await
    }

    fn try_send(&self, command: ThreadRuntimeCommand) -> ChatResult<()> {
        self.command_sender
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ChatError::new(
                    ChatErrorCode::Busy,
                    "Chat thread command queue is full",
                    true,
                ),
                mpsc::error::TrySendError::Closed(_) => runtime_unavailable_error(),
            })
    }

    fn stop_accepting(&self) -> ChatResult<()> {
        self.snapshot
            .lock()
            .map_err(|_| runtime_state_error())?
            .accepting_commands = false;
        Ok(())
    }

    fn abort_worker(&self) -> ChatResult<()> {
        if let Some(task) = self
            .worker_task
            .lock()
            .map_err(|_| runtime_state_error())?
            .take()
        {
            task.abort();
        }
        Ok(())
    }
}

pub struct ChatRuntimeRegistry {
    owners: Mutex<HashMap<ChatThreadId, Arc<ThreadRuntimeOwner>>>,
    idle_timeout: Duration,
}

impl Default for ChatRuntimeRegistry {
    fn default() -> Self {
        Self {
            owners: Mutex::new(HashMap::new()),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }
}

impl ChatRuntimeRegistry {
    pub fn owner(&self, thread_id: ChatThreadId) -> ChatResult<Arc<ThreadRuntimeOwner>> {
        self.owner_with_capacity(thread_id, DEFAULT_COMMAND_CAPACITY)
    }

    fn owner_with_capacity(
        &self,
        thread_id: ChatThreadId,
        capacity: usize,
    ) -> ChatResult<Arc<ThreadRuntimeOwner>> {
        self.owner_with_start_gate(thread_id, capacity, None)
    }

    fn owner_with_start_gate(
        &self,
        thread_id: ChatThreadId,
        capacity: usize,
        start_gate: Option<oneshot::Receiver<()>>,
    ) -> ChatResult<Arc<ThreadRuntimeOwner>> {
        let mut owners = self.owners.lock().map_err(|_| runtime_state_error())?;
        if let Some(owner) = owners.get(&thread_id) {
            return Ok(Arc::clone(owner));
        }
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        let snapshot = Arc::new(Mutex::new(ThreadRuntimeSnapshot {
            session_id: None,
            session_state: ProviderSessionState::Stopped,
            capabilities: crate::models::ProviderCapabilities::default(),
            active_turn_id: None,
            turn_active: false,
            pending_request: false,
            terminal_linked_operation: false,
            accepting_commands: true,
            generation: 0,
            last_activity: Instant::now(),
        }));
        let generation = Arc::new(AtomicU64::new(0));
        let worker_thread_id = thread_id.clone();
        let worker_snapshot = Arc::clone(&snapshot);
        let idle_timeout = self.idle_timeout;
        let worker = tokio::spawn(async move {
            if let Some(start_gate) = start_gate {
                let _ = start_gate.await;
            }
            run_thread_runtime(
                worker_thread_id,
                receiver,
                worker_snapshot,
                generation,
                idle_timeout,
            )
            .await;
        });
        let owner = Arc::new(ThreadRuntimeOwner {
            thread_id: thread_id.clone(),
            command_sender: sender,
            operation_lock: Arc::new(AsyncMutex::new(())),
            snapshot,
            worker_task: Mutex::new(Some(worker)),
        });
        owners.insert(thread_id, Arc::clone(&owner));
        Ok(owner)
    }

    pub fn owners(&self) -> ChatResult<Vec<Arc<ThreadRuntimeOwner>>> {
        self.owners
            .lock()
            .map(|owners| owners.values().cloned().collect())
            .map_err(|_| runtime_state_error())
    }

    pub fn process_counts(&self) -> ChatResult<(usize, usize)> {
        let owners = self.owners()?;
        let mut live_processes = 0;
        let mut active_turns = 0;
        for owner in &owners {
            let snapshot = owner.snapshot()?;
            if !matches!(
                snapshot.session_state,
                ProviderSessionState::Stopped | ProviderSessionState::Failed
            ) {
                live_processes += 1;
            }
            if snapshot.turn_active {
                active_turns += 1;
            }
        }
        Ok((live_processes, active_turns))
    }

    pub async fn shutdown_thread_and_remove(
        &self,
        thread_id: &ChatThreadId,
        timeout: Duration,
        mutations: &ChatWorkspaceMutationRegistry,
    ) -> ChatResult<()> {
        let owner = self
            .owners
            .lock()
            .map_err(|_| runtime_state_error())?
            .remove(thread_id);
        let Some(owner) = owner else {
            mutations.finish_thread(thread_id);
            return Ok(());
        };
        owner.stop_accepting()?;
        let (response, receiver) = oneshot::channel();
        let command = ThreadRuntimeCommand::Shutdown {
            deadline: Instant::now() + timeout,
            reservation_cleanup: mutations.thread_cleanup(thread_id),
            response,
        };
        if owner.command_sender.send(command).await.is_err() {
            owner.abort_worker()?;
            mutations.finish_thread(thread_id);
            return Err(runtime_unavailable_error());
        }
        match tokio::time::timeout(timeout, receive_response(receiver)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                owner.abort_worker()?;
                mutations.finish_thread(thread_id);
                Err(error)
            }
            Err(_) => {
                owner.abort_worker()?;
                mutations.finish_thread(thread_id);
                Err(runtime_timeout_error())
            }
        }
    }

    pub async fn stop_all_and_reset(
        &self,
        timeout: Duration,
        mutations: &ChatWorkspaceMutationRegistry,
    ) -> ChatResult<u64> {
        let owners = {
            let mut registry = self.owners.lock().map_err(|_| runtime_state_error())?;
            registry.drain().map(|(_, owner)| owner).collect::<Vec<_>>()
        };
        let mut count = 0u64;
        for owner in &owners {
            if !matches!(
                owner.snapshot()?.session_state,
                ProviderSessionState::Stopped | ProviderSessionState::Failed
            ) {
                count = count.saturating_add(1);
            }
        }
        for owner in &owners {
            owner.stop_accepting()?;
        }
        let shutdown = async {
            let deadline = Instant::now() + timeout;
            let mut receivers = Vec::with_capacity(owners.len());
            for owner in &owners {
                let (response, receiver) = oneshot::channel();
                let reservation_cleanup = mutations.thread_cleanup(owner.thread_id());
                owner
                    .command_sender
                    .send(ThreadRuntimeCommand::Shutdown {
                        deadline,
                        reservation_cleanup,
                        response,
                    })
                    .await
                    .map_err(|_| runtime_unavailable_error())?;
                receivers.push(receiver);
            }
            for receiver in receivers {
                receive_response(receiver).await?;
            }
            Ok(())
        };
        match tokio::time::timeout(timeout, shutdown).await {
            Ok(Ok(())) => Ok(count),
            Ok(Err(error)) => {
                abort_workers_and_release(&owners, mutations)?;
                Err(error)
            }
            Err(_) => {
                abort_workers_and_release(&owners, mutations)?;
                Err(runtime_timeout_error())
            }
        }
    }

    pub async fn shutdown_and_wait(
        &self,
        timeout: Duration,
        mutations: &ChatWorkspaceMutationRegistry,
    ) -> ChatResult<()> {
        let owners = self.owners()?;
        for owner in &owners {
            owner.stop_accepting()?;
        }
        let shutdown = async {
            let deadline = Instant::now() + timeout;
            let mut receivers = Vec::with_capacity(owners.len());
            for owner in &owners {
                let (response, receiver) = oneshot::channel();
                let reservation_cleanup = mutations.thread_cleanup(owner.thread_id());
                owner
                    .command_sender
                    .send(ThreadRuntimeCommand::Shutdown {
                        deadline,
                        reservation_cleanup,
                        response,
                    })
                    .await
                    .map_err(|_| runtime_unavailable_error())?;
                receivers.push(receiver);
            }
            for receiver in receivers {
                receive_response(receiver).await?;
            }
            Ok(())
        };
        match tokio::time::timeout(timeout, shutdown).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                abort_workers_and_release(&owners, mutations)?;
                Err(error)
            }
            Err(_) => {
                abort_workers_and_release(&owners, mutations)?;
                Err(runtime_timeout_error())
            }
        }
    }
}

fn abort_workers_and_release(
    owners: &[Arc<ThreadRuntimeOwner>],
    mutations: &ChatWorkspaceMutationRegistry,
) -> ChatResult<()> {
    let mut abort_error = None;
    for owner in owners {
        if let Err(error) = owner.abort_worker() {
            abort_error.get_or_insert(error);
        }
        mutations.finish_thread(owner.thread_id());
    }
    abort_error.map_or(Ok(()), Err)
}

#[cfg(any(test, feature = "test-support"))]
impl ChatRuntimeRegistry {
    #[doc(hidden)]
    pub fn owner_for_test(
        &self,
        thread_id: ChatThreadId,
        capacity: usize,
    ) -> ChatResult<(Arc<ThreadRuntimeOwner>, oneshot::Sender<()>)> {
        let (start, wait) = oneshot::channel();
        self.owner_with_start_gate(thread_id, capacity, Some(wait))
            .map(|owner| (owner, start))
    }

    #[doc(hidden)]
    pub fn with_idle_timeout_for_test(idle_timeout: Duration) -> Self {
        Self {
            owners: Mutex::new(HashMap::new()),
            idle_timeout,
        }
    }
}

#[cfg(test)]
mod event_scope_tests {
    use super::*;
    use crate::events::{CANONICAL_EVENT_SCHEMA_VERSION, ItemLifecycleEvent};
    use crate::models::{
        ActivityStatus, CanonicalItemKind, ChatEventId, ProviderFamilyId, ProviderInstanceId,
        UtcTimestamp,
    };

    #[test]
    fn thread_level_compaction_bypasses_active_turn_correlation() {
        let active_turn_id = ChatTurnId::new("turn-1".to_string()).unwrap();
        let snapshot = Arc::new(Mutex::new(ThreadRuntimeSnapshot {
            session_id: None,
            session_state: ProviderSessionState::Active,
            capabilities: crate::models::ProviderCapabilities::default(),
            active_turn_id: Some(active_turn_id),
            turn_active: true,
            pending_request: false,
            terminal_linked_operation: false,
            accepting_commands: true,
            generation: 1,
            last_activity: Instant::now(),
        }));
        let event = CanonicalRuntimeEvent {
            schema_version: CANONICAL_EVENT_SCHEMA_VERSION,
            event_id: ChatEventId::new("event-compaction".to_string()).unwrap(),
            provider_family_id: ProviderFamilyId::new("codex".to_string()).unwrap(),
            provider_instance_id: ProviderInstanceId::new("codex-local".to_string()).unwrap(),
            thread_id: ChatThreadId::new("thread-1".to_string()).unwrap(),
            created_at: UtcTimestamp::new("2026-07-28T00:00:00.000Z".to_string()).unwrap(),
            turn_id: None,
            provider_turn_id: None,
            provider_item_id: None,
            provider_request_id: None,
            provider_task_id: None,
            provider_reference: None,
            event: CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                item_id: "context-compaction".to_string(),
                kind: CanonicalItemKind::ContextCompaction,
                status: ActivityStatus::Completed,
                title: Some("Context compacted".to_string()),
                detail: None,
                safe_metadata: None,
            }),
            redacted_diagnostic: None,
        };

        assert!(event_matches_active_turn(&snapshot, &event));
    }
}
