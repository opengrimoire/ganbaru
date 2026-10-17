use super::event_sink::GenerationEventSink;
use super::helpers::*;
use super::*;

struct RuntimeWorker {
    thread_id: ChatThreadId,
    driver: Option<Box<dyn ProviderDriver>>,
    event_sink: Option<Arc<dyn ProviderEventSink>>,
    session: Option<ProviderSessionSnapshot>,
    snapshot: Arc<Mutex<ThreadRuntimeSnapshot>>,
    generation: Arc<AtomicU64>,
}

impl RuntimeWorker {
    fn new(
        thread_id: ChatThreadId,
        snapshot: Arc<Mutex<ThreadRuntimeSnapshot>>,
        generation: Arc<AtomicU64>,
    ) -> Self {
        Self {
            thread_id,
            driver: None,
            event_sink: None,
            session: None,
            snapshot,
            generation,
        }
    }

    async fn handle(&mut self, command: ThreadRuntimeCommand) -> bool {
        self.touch();
        match command {
            ThreadRuntimeCommand::SessionState(value) => self.update(|state| {
                state.session_state = value;
            }),
            ThreadRuntimeCommand::TurnActive(value) => self.update(|state| {
                state.turn_active = value;
                if !value {
                    state.active_turn_id = None;
                }
            }),
            ThreadRuntimeCommand::PendingRequest(value) => self.update(|state| {
                state.pending_request = value;
            }),
            ThreadRuntimeCommand::TerminalLinkedOperation(value) => self.update(|state| {
                state.terminal_linked_operation = value;
            }),
            ThreadRuntimeCommand::Touch => {}
            ThreadRuntimeCommand::PromptCatalog { response } => {
                let result = self
                    .driver
                    .as_ref()
                    .map_or_else(|| Ok(Vec::new()), |driver| driver.prompt_catalog());
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::CompactContext {
                request,
                context,
                response,
            } => {
                if self.session_state() != ProviderSessionState::Ready {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session must be ready before context compaction",
                    )));
                    return false;
                }
                let turn_id = request.turn_id.clone();
                self.update(|state| {
                    state.session_state = ProviderSessionState::Active;
                    state.active_turn_id = Some(turn_id);
                    state.turn_active = true;
                });
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.compact_context(request, &context))
                            .await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                if result.is_err() {
                    self.update(|state| {
                        state.session_state = ProviderSessionState::Ready;
                        state.active_turn_id = None;
                        state.turn_active = false;
                    });
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::ReadMcpStatus {
                request,
                context,
                response,
            } => {
                if matches!(
                    self.session_state(),
                    ProviderSessionState::Stopped
                        | ProviderSessionState::Starting
                        | ProviderSessionState::Stopping
                        | ProviderSessionState::Failed
                ) {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session is unavailable for MCP status",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.read_mcp_status(request, &context))
                            .await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::StartSession {
                mut driver,
                request,
                event_sink,
                context,
                response,
            } => {
                if self.driver.is_some()
                    && !matches!(
                        self.session_state(),
                        ProviderSessionState::Stopped | ProviderSessionState::Failed
                    )
                {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session is already running",
                    )));
                    return false;
                }
                let generation = self.next_generation();
                self.driver.take();
                self.session = None;
                self.update(|state| {
                    state.session_id = None;
                    state.session_state = ProviderSessionState::Starting;
                    state.capabilities = crate::models::ProviderCapabilities::default();
                    state.turn_active = false;
                    state.active_turn_id = None;
                    state.pending_request = false;
                    state.generation = generation;
                });
                let sink: Arc<dyn ProviderEventSink> = Arc::new(GenerationEventSink {
                    thread_id: self.thread_id.clone(),
                    generation,
                    current_generation: Arc::clone(&self.generation),
                    snapshot: Arc::clone(&self.snapshot),
                    inner: event_sink,
                });
                self.event_sink = Some(Arc::clone(&sink));
                let result =
                    run_driver_operation(&context, driver.start_session(request, sink, &context))
                        .await;
                if result.is_err() {
                    let _ = self.flush_events(&context).await;
                }
                match &result {
                    Ok(session) => {
                        self.session = Some(session.clone());
                        self.update(|state| {
                            state.session_id = Some(session.session_id.clone());
                            state.session_state = session.state;
                            state.capabilities = session.capabilities.clone();
                        });
                    }
                    Err(_) => {
                        self.update(|state| state.session_state = ProviderSessionState::Failed)
                    }
                }
                self.driver = Some(driver);
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::ResumeSession {
                mut driver,
                request,
                event_sink,
                context,
                response,
            } => {
                if self.driver.is_some()
                    && !matches!(
                        self.session_state(),
                        ProviderSessionState::Stopped | ProviderSessionState::Failed
                    )
                {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session is already running",
                    )));
                    return false;
                }
                let generation = self.next_generation();
                self.driver.take();
                self.session = None;
                self.update(|state| {
                    state.session_id = None;
                    state.session_state = ProviderSessionState::Starting;
                    state.capabilities = crate::models::ProviderCapabilities::default();
                    state.turn_active = false;
                    state.active_turn_id = None;
                    state.pending_request = false;
                    state.generation = generation;
                });
                let sink: Arc<dyn ProviderEventSink> = Arc::new(GenerationEventSink {
                    thread_id: self.thread_id.clone(),
                    generation,
                    current_generation: Arc::clone(&self.generation),
                    snapshot: Arc::clone(&self.snapshot),
                    inner: event_sink,
                });
                self.event_sink = Some(Arc::clone(&sink));
                let result =
                    run_driver_operation(&context, driver.resume_session(request, sink, &context))
                        .await;
                if result.is_err() {
                    let _ = self.flush_events(&context).await;
                }
                match &result {
                    Ok(session) => {
                        self.session = Some(session.clone());
                        self.update(|state| {
                            state.session_id = Some(session.session_id.clone());
                            state.session_state = session.state;
                            state.capabilities = session.capabilities.clone();
                        });
                    }
                    Err(_) => {
                        self.update(|state| state.session_state = ProviderSessionState::Failed)
                    }
                }
                self.driver = Some(driver);
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::SendTurn {
                request,
                reservation,
                context,
                response,
            } => {
                if self.session_state() != ProviderSessionState::Ready {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session is not ready for a new turn",
                    )));
                    return false;
                }
                let turn_id = request.turn_id.clone();
                self.update(|state| {
                    state.session_state = ProviderSessionState::Active;
                    state.active_turn_id = Some(turn_id);
                    state.turn_active = true;
                });
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.send_turn(request, &context)).await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                if result.is_err() {
                    let _ = self.flush_events(&context).await;
                    self.update(|state| state.session_state = ProviderSessionState::Failed);
                } else {
                    reservation.handoff_to_event_sink();
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::SteerTurn {
                request,
                context,
                response,
            } => {
                if self.session_state() != ProviderSessionState::Active {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session has no steerable turn",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.steer_turn(request, &context)).await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::ResolveApproval {
                request,
                context,
                response,
            } => {
                if self.session_state() != ProviderSessionState::WaitingForApproval {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session has no pending approval",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.resolve_approval(request, &context))
                            .await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                if result.is_ok() {
                    self.update(|state| {
                        state.pending_request = false;
                        if matches!(
                            state.session_state,
                            ProviderSessionState::WaitingForApproval
                                | ProviderSessionState::WaitingForUserInput
                        ) {
                            state.session_state = ProviderSessionState::Active;
                        }
                    });
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::ResolveUserInput {
                request,
                context,
                response,
            } => {
                if self.session_state() != ProviderSessionState::WaitingForUserInput {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session has no pending user input request",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.resolve_user_input(request, &context))
                            .await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                if result.is_ok() {
                    self.update(|state| {
                        state.pending_request = false;
                        state.session_state = ProviderSessionState::Active;
                    });
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::InterruptTurn {
                request,
                context,
                response,
            } => {
                if !matches!(
                    self.session_state(),
                    ProviderSessionState::Active
                        | ProviderSessionState::WaitingForApproval
                        | ProviderSessionState::WaitingForUserInput
                ) {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session has no interruptible turn",
                    )));
                    return false;
                }
                let previous_state = self.session_state();
                self.update(|state| state.session_state = ProviderSessionState::Stopping);
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.interrupt_turn(request, &context))
                            .await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                if result.is_err() {
                    self.update(|state| state.session_state = previous_state);
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::Rollback {
                request,
                context,
                response,
            } => {
                if self.session_state() != ProviderSessionState::Ready {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session must be ready before rollback",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.rollback(request, &context)).await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::StopSession {
                force,
                mut reservation_cleanup,
                context,
                response,
            } => {
                if let Some(cleanup) = reservation_cleanup.as_mut() {
                    cleanup.arm();
                }
                let result = self.stop_driver(force, context).await;
                let receipt = result.map(|()| DriverOperationReceipt {
                    accepted: true,
                    operation_id: "runtime-stop".to_string(),
                    detail: None,
                });
                drop(reservation_cleanup);
                let _ = response.send(receipt);
            }
            ThreadRuntimeCommand::Shutdown {
                deadline,
                mut reservation_cleanup,
                response,
            } => {
                reservation_cleanup.arm();
                self.update(|state| state.accepting_commands = false);
                let context = operation_context("runtime-shutdown", deadline);
                let result = self.stop_driver(true, context).await;
                drop(reservation_cleanup);
                let _ = response.send(result);
                return true;
            }
        }
        false
    }

    async fn stop_driver(
        &mut self,
        force: bool,
        context: DriverOperationContext,
    ) -> ChatResult<()> {
        let initial_flush = self.flush_events(&context).await;
        let Some(mut driver) = self.driver.take() else {
            self.session = None;
            self.event_sink = None;
            self.update(|state| {
                state.session_id = None;
                state.session_state = ProviderSessionState::Stopped;
                state.capabilities = crate::models::ProviderCapabilities::default();
                state.active_turn_id = None;
                state.turn_active = false;
                state.pending_request = false;
            });
            return initial_flush;
        };
        self.update(|state| state.session_state = ProviderSessionState::Stopping);
        if let (Some(session), Some(turn_id)) = (self.session.as_ref(), self.active_turn_id()) {
            let request = InterruptTurnRequest {
                command: ChatCommandContext {
                    client_command_id: ChatCommandId::new("runtime-stop-interrupt")
                        .map_err(runtime_identifier_error)?,
                    expected_thread_revision: None,
                },
                session_id: session.session_id.clone(),
                turn_id,
            };
            let _ = run_driver_operation_with_flush(
                &context,
                driver.interrupt_turn(request, &context),
                self.event_sink.clone(),
            )
            .await;
        }
        let stop_result = if let Some(session) = self.session.as_ref() {
            let request = StopSessionRequest {
                session_id: session.session_id.clone(),
                force,
            };
            run_driver_operation_with_flush(
                &context,
                driver.stop_session(request, &context),
                self.event_sink.clone(),
            )
            .await
            .map(|_| ())
        } else {
            Ok(())
        };
        let final_flush = self.flush_events(&context).await;
        let result = initial_flush.and(stop_result).and(final_flush);
        self.next_generation();
        self.session = None;
        self.event_sink = None;
        self.update(|state| {
            state.session_id = None;
            state.session_state = if result.is_ok() {
                ProviderSessionState::Stopped
            } else {
                ProviderSessionState::Failed
            };
            state.capabilities = crate::models::ProviderCapabilities::default();
            state.active_turn_id = None;
            state.turn_active = false;
            state.pending_request = false;
        });
        result
    }

    async fn idle_stop(&mut self) {
        let deadline = Instant::now() + DEFAULT_STOP_TIMEOUT;
        let _ = self
            .stop_driver(false, operation_context("runtime-idle-stop", deadline))
            .await;
    }

    fn can_idle_stop(&self, idle_timeout: Duration) -> bool {
        self.driver.is_some()
            && self
                .snapshot
                .lock()
                .map(|snapshot| snapshot.can_idle_stop(Instant::now(), idle_timeout))
                .unwrap_or(false)
    }

    fn idle_deadline(&self, idle_timeout: Duration) -> Option<Instant> {
        self.driver.as_ref()?;
        self.snapshot
            .lock()
            .ok()
            .map(|snapshot| snapshot.last_activity + idle_timeout)
    }

    fn active_turn_id(&self) -> Option<ChatTurnId> {
        self.snapshot
            .lock()
            .ok()
            .and_then(|snapshot| snapshot.active_turn_id.clone())
    }

    async fn flush_events(&mut self, context: &DriverOperationContext) -> ChatResult<()> {
        match self.event_sink.as_ref() {
            Some(sink) => run_driver_operation(context, sink.flush()).await,
            None => Ok(()),
        }
    }

    fn session_state(&self) -> ProviderSessionState {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.session_state)
            .unwrap_or(ProviderSessionState::Failed)
    }

    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn touch(&self) {
        self.update(|state| state.last_activity = Instant::now());
    }

    fn update(&self, update: impl FnOnce(&mut ThreadRuntimeSnapshot)) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            update(&mut snapshot);
        }
    }
}

pub(super) async fn run_thread_runtime(
    thread_id: ChatThreadId,
    mut receiver: mpsc::Receiver<ThreadRuntimeCommand>,
    snapshot: Arc<Mutex<ThreadRuntimeSnapshot>>,
    generation: Arc<AtomicU64>,
    idle_timeout: Duration,
) {
    let mut worker = RuntimeWorker::new(thread_id, snapshot, generation);
    loop {
        let command = match worker.idle_deadline(idle_timeout) {
            Some(deadline) => match tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                receiver.recv(),
            )
            .await
            {
                Ok(command) => command,
                Err(_) => {
                    if worker.can_idle_stop(idle_timeout) {
                        worker.idle_stop().await;
                    }
                    continue;
                }
            },
            None => receiver.recv().await,
        };
        let Some(command) = command else {
            let deadline = Instant::now() + DEFAULT_STOP_TIMEOUT;
            let _ = worker
                .stop_driver(true, operation_context("runtime-channel-close", deadline))
                .await;
            worker.update(|state| state.accepting_commands = false);
            return;
        };
        if worker.handle(command).await {
            return;
        }
    }
}
