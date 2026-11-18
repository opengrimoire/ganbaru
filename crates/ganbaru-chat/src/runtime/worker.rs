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
    interrupt_confirmation_timeout: Duration,
    /// The turn an accepted interrupt is waiting on, and when the runtime stops waiting.
    interrupt_deadline: Option<(ChatTurnId, Instant)>,
    /// How long the session must stay quiet before it restarts onto an updated CLI, when one was
    /// installed while it ran.
    restart_when_quiet: Option<Duration>,
    /// When a due stop that found the session busy looks again.
    stop_check_at: Option<Instant>,
    /// The last CPU time measured below the provider process, and when.
    cpu_sample: Option<(Instant, Duration)>,
}

impl RuntimeWorker {
    fn new(
        thread_id: ChatThreadId,
        snapshot: Arc<Mutex<ThreadRuntimeSnapshot>>,
        generation: Arc<AtomicU64>,
        interrupt_confirmation_timeout: Duration,
    ) -> Self {
        Self {
            thread_id,
            driver: None,
            event_sink: None,
            session: None,
            snapshot,
            generation,
            interrupt_confirmation_timeout,
            interrupt_deadline: None,
            restart_when_quiet: None,
            stop_check_at: None,
            cpu_sample: None,
        }
    }

    async fn handle(&mut self, command: ThreadRuntimeCommand) -> bool {
        // A restart request is not session activity, so it must not postpone the quiet period.
        if !matches!(
            command,
            ThreadRuntimeCommand::RestartAfterProviderUpdate { .. }
        ) {
            self.touch();
            // Activity starts a new idle period, so a deferral from an earlier busy check ends.
            self.stop_check_at = None;
        }
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
            ThreadRuntimeCommand::RestartAfterProviderUpdate { family_id, quiet } => {
                let runs_family = self
                    .driver
                    .as_ref()
                    .is_some_and(|driver| driver.instance_configuration().family_id == family_id);
                if runs_family {
                    self.restart_when_quiet = Some(quiet);
                }
            }
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
                self.restart_when_quiet = None;
                self.clear_stop_checks();
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
                self.restart_when_quiet = None;
                self.clear_stop_checks();
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
                let turn_id = request.turn_id.clone();
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
                } else {
                    self.interrupt_deadline = Some((
                        turn_id,
                        Instant::now() + self.interrupt_confirmation_timeout,
                    ));
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
        self.restart_when_quiet = None;
        self.clear_stop_checks();
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

    /// The earlier of a pending interrupt deadline and the next stop check: the idle deadline or
    /// a restart after a CLI update, postponed while an earlier check found the session busy.
    fn next_deadline(&self, idle_timeout: Duration) -> Option<Instant> {
        let interrupt = self
            .interrupt_deadline
            .as_ref()
            .map(|(_, deadline)| *deadline);
        let stop = [self.idle_deadline(idle_timeout), self.restart_deadline()]
            .into_iter()
            .flatten()
            .min()
            .map(|due| self.stop_check_at.map_or(due, |check| due.max(check)));
        [interrupt, stop].into_iter().flatten().min()
    }

    /// When the session can restart onto an updated CLI: once it has been quiet long enough.
    fn restart_deadline(&self) -> Option<Instant> {
        let quiet = self.restart_when_quiet?;
        self.driver.as_ref()?;
        let last_activity = self.snapshot.lock().ok()?.last_activity;
        Some(last_activity + quiet)
    }

    /// Stops a session whose idle or update stop is due, unless it is still busy; a busy session
    /// is checked again later.
    async fn stop_when_due(&mut self, idle_timeout: Duration) {
        let now = Instant::now();
        if !self.can_idle_stop(idle_timeout) && !self.can_restart_after_update() {
            self.stop_check_at = Some(now + STOP_RECHECK_INTERVAL);
            return;
        }
        if let Some(recheck) = self.agent_work_recheck(now) {
            self.stop_check_at = Some(now + recheck);
            return;
        }
        self.idle_stop().await;
    }

    /// How long to wait before looking again when work the agent started may still be running:
    /// the provider reports background work, or the processes below the provider used CPU since
    /// the last sample. `None` when nothing shows the agent's work running.
    ///
    /// A first sample, or one too old to compare, starts a new measurement instead.
    fn agent_work_recheck(&mut self, now: Instant) -> Option<Duration> {
        let driver = self.driver.as_ref()?;
        if driver.background_work_running() {
            return Some(STOP_RECHECK_INTERVAL);
        }
        let used = crate::process::inventory::descendant_cpu_time(driver.session_process_id()?)?;
        match self.cpu_sample.replace((now, used)) {
            Some((sampled_at, earlier))
                if now.duration_since(sampled_at) >= CPU_SAMPLE_INTERVAL
                    && now.duration_since(sampled_at) <= CPU_SAMPLE_MAX_AGE =>
            {
                let busy = used.saturating_sub(earlier).as_nanos() * 100
                    > now.duration_since(sampled_at).as_nanos() * BUSY_CPU_PERCENT;
                busy.then_some(STOP_RECHECK_INTERVAL)
            }
            _ => Some(CPU_SAMPLE_INTERVAL),
        }
    }

    fn clear_stop_checks(&mut self) {
        self.stop_check_at = None;
        self.cpu_sample = None;
    }

    /// Whether a session waiting on a CLI update is idle and has been quiet long enough.
    fn can_restart_after_update(&self) -> bool {
        self.restart_when_quiet
            .is_some_and(|quiet| self.can_idle_stop(quiet))
    }

    /// Whether a pending interrupt deadline has passed.
    fn interrupt_expired(&self, now: Instant) -> bool {
        self.interrupt_deadline
            .as_ref()
            .is_some_and(|(_, deadline)| *deadline <= now)
    }

    /// Settles a turn whose provider accepted an interrupt but never confirmed it: records a
    /// warning and an interrupted abort with an id derived from the thread and turn, then stops
    /// the provider so the next turn starts a fresh session. Events the provider sends later
    /// belong to a retired generation and are ignored as late. Does nothing when the turn
    /// already settled.
    async fn expire_interrupt(&mut self) {
        let Some((turn_id, _)) = self.interrupt_deadline.take() else {
            return;
        };
        if self.active_turn_id().as_ref() != Some(&turn_id) {
            return;
        }
        let provider = self.driver.as_ref().map(|driver| {
            let configuration = driver.instance_configuration();
            (
                configuration.family_id.clone(),
                configuration.instance_id.clone(),
            )
        });
        if let (Some(sink), Some((family_id, instance_id))) = (self.event_sink.clone(), provider) {
            match unconfirmed_interrupt_events(&self.thread_id, &turn_id, &family_id, &instance_id)
            {
                Ok(events) => {
                    for event in events {
                        if let Err(error) = sink.emit(event).await {
                            eprintln!(
                                "Chat could not record an unconfirmed interrupt, code {:?}",
                                error.code
                            );
                        }
                    }
                }
                Err(error) => eprintln!(
                    "Chat could not build an unconfirmed interrupt event, code {:?}",
                    error.code
                ),
            }
        }
        let deadline = Instant::now() + DEFAULT_STOP_TIMEOUT;
        if let Err(error) = self
            .stop_driver(
                false,
                operation_context("runtime-interrupt-timeout", deadline),
            )
            .await
        {
            eprintln!(
                "Chat could not stop a provider that ignored Stop, code {:?}",
                error.code
            );
        }
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
    idle_timeout: IdleTimeout,
    interrupt_confirmation_timeout: Duration,
) {
    let mut worker = RuntimeWorker::new(
        thread_id,
        snapshot,
        generation,
        interrupt_confirmation_timeout,
    );
    loop {
        // Checked before waiting too, so steady commands never postpone an expired interrupt.
        if worker.interrupt_expired(Instant::now()) {
            worker.expire_interrupt().await;
        }
        let idle_timeout = idle_timeout.get();
        let command = match worker.next_deadline(idle_timeout) {
            Some(deadline) => match tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                receiver.recv(),
            )
            .await
            {
                Ok(command) => command,
                Err(_) => {
                    if !worker.interrupt_expired(Instant::now()) {
                        worker.stop_when_due(idle_timeout).await;
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
