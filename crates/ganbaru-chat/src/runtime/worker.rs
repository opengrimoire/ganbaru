use super::event_sink::GenerationEventSink;
use super::helpers::*;
use super::*;

struct RuntimeWorker {
    thread_id: ChatThreadId,
    driver: Option<Box<dyn ProviderDriver>>,
    event_sink: Option<Arc<dyn ProviderEventSink>>,
    session: Option<ProviderSessionSnapshot>,
    snapshot: Arc<SnapshotCell>,
    interrupt_confirmation_timeout: Duration,
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
        snapshot: Arc<SnapshotCell>,
        interrupt_confirmation_timeout: Duration,
    ) -> Self {
        Self {
            thread_id,
            driver: None,
            event_sink: None,
            session: None,
            snapshot,
            interrupt_confirmation_timeout,
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
            #[cfg(any(test, feature = "test-support"))]
            ThreadRuntimeCommand::ApplyPhaseInput(input) => {
                self.apply(input);
            }
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
                let turn_id = request.turn_id.clone();
                if !self.apply(RuntimeInput::TurnAccepted(turn_id.clone())) {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session must be ready before context compaction",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.compact_context(request, &context))
                            .await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                if result.is_err() {
                    self.apply(RuntimeInput::TurnWithdrawn(turn_id));
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::ReadMcpStatus {
                request,
                context,
                response,
            } => {
                if !self.read(|state| state.phase.is_running()) {
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
                let Some(sink) = self.begin_session(event_sink) else {
                    let _ = response.send(Err(session_running_error()));
                    return false;
                };
                let result =
                    run_driver_operation(&context, driver.start_session(request, sink, &context))
                        .await;
                self.finish_session(driver, &result, &context).await;
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::ResumeSession {
                mut driver,
                request,
                event_sink,
                context,
                response,
            } => {
                let Some(sink) = self.begin_session(event_sink) else {
                    let _ = response.send(Err(session_running_error()));
                    return false;
                };
                let result =
                    run_driver_operation(&context, driver.resume_session(request, sink, &context))
                        .await;
                self.finish_session(driver, &result, &context).await;
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::SendTurn {
                request,
                reservation,
                context,
                response,
            } => {
                let turn_id = request.turn_id.clone();
                if !self.apply(RuntimeInput::TurnAccepted(turn_id.clone())) {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session is not ready for a new turn",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.send_turn(request, &context)).await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                match &result {
                    Ok(_) => reservation.handoff_to_event_sink(),
                    Err(_) => {
                        let _ = self.flush_events(&context).await;
                        self.abandon_dispatch(turn_id).await;
                    }
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
                let request_id = request.provider_request_id.clone();
                if self.read(|state| state.open_request(&request_id))
                    != Some(OpenRequestKind::Approval)
                {
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
                    self.apply(RuntimeInput::RequestClosed(request_id));
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::ResolveUserInput {
                request,
                context,
                response,
            } => {
                let request_id = request.provider_request_id.clone();
                if self.read(|state| state.open_request(&request_id))
                    != Some(OpenRequestKind::UserInput)
                {
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
                    self.apply(RuntimeInput::RequestClosed(request_id));
                }
                let _ = response.send(result);
            }
            ThreadRuntimeCommand::InterruptTurn {
                request,
                context,
                response,
            } => {
                let turn_id = request.turn_id.clone();
                if !self.apply(RuntimeInput::InterruptRequested(turn_id.clone())) {
                    let _ = response.send(Err(runtime_invalid_state_error(
                        "Chat provider session has no interruptible turn",
                    )));
                    return false;
                }
                let result = match self.driver.as_mut() {
                    Some(driver) => {
                        run_driver_operation(&context, driver.interrupt_turn(request, &context))
                            .await
                    }
                    None => Err(runtime_unavailable_error()),
                };
                // The turn may have settled while the provider answered; then neither input
                // applies and the phase keeps what the provider reported.
                let input = match &result {
                    Ok(_) => RuntimeInput::InterruptAccepted {
                        turn_id,
                        deadline: Instant::now() + self.interrupt_confirmation_timeout,
                    },
                    Err(_) => RuntimeInput::InterruptRejected(turn_id),
                };
                self.apply(input);
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
        self.stop_driver_with_outcome(force, context, StopOutcome::FromResult)
            .await
    }

    /// Stops the provider. The phase ends `Stopped` after a clean stop, or `Failed` after an
    /// unclean one or when `outcome` says the session already failed.
    async fn stop_driver_with_outcome(
        &mut self,
        force: bool,
        context: DriverOperationContext,
        outcome: StopOutcome,
    ) -> ChatResult<()> {
        self.restart_when_quiet = None;
        self.clear_stop_checks();
        let initial_flush = self.flush_events(&context).await;
        let Some(mut driver) = self.driver.take() else {
            self.session = None;
            self.event_sink = None;
            self.update(|state| {
                state.session_id = None;
                state.capabilities = crate::models::ProviderCapabilities::default();
                let _ = state.apply(RuntimeInput::StopStarted);
                let _ = state.apply(RuntimeInput::StopFinished {
                    clean: outcome == StopOutcome::FromResult,
                });
            });
            return initial_flush;
        };
        self.apply(RuntimeInput::StopStarted);
        let stopping_turn = self.read(|state| state.active_turn_id().cloned());
        if let (Some(session), Some(turn_id)) = (self.session.as_ref(), stopping_turn) {
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
        self.session = None;
        self.event_sink = None;
        let clean = result.is_ok() && outcome == StopOutcome::FromResult;
        // Retiring the generation in the same critical section as the phase change keeps a late
        // event of this session from landing on the stopped phase.
        self.update(|state| {
            state.generation += 1;
            state.session_id = None;
            state.capabilities = crate::models::ProviderCapabilities::default();
            let _ = state.apply(RuntimeInput::StopFinished { clean });
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
        let interrupt = self.read(|state| state.phase.interrupt_deadline().map(|(_, at)| at));
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
        let last_activity = self.snapshot.read(|state| state.last_activity)?;
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
        self.read(|state| {
            state
                .phase
                .interrupt_deadline()
                .is_some_and(|(_, deadline)| deadline <= now)
        })
    }

    /// Settles a turn whose provider accepted an interrupt but never confirmed it: records a
    /// warning and an interrupted abort with an id derived from the thread and turn, then stops
    /// the provider so the next turn starts a fresh session. Events the provider sends later
    /// belong to a retired generation and are ignored as late. Does nothing when the turn
    /// already settled.
    async fn expire_interrupt(&mut self) {
        let Some(turn_id) = self.read(|state| {
            state
                .phase
                .interrupt_deadline()
                .map(|(turn_id, _)| turn_id.clone())
        }) else {
            return;
        };
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
                .read(|snapshot| snapshot.can_idle_stop(Instant::now(), idle_timeout))
                .unwrap_or(false)
    }

    fn idle_deadline(&self, idle_timeout: Duration) -> Option<Instant> {
        self.driver.as_ref()?;
        self.snapshot
            .read(|snapshot| snapshot.last_activity + idle_timeout)
    }

    /// Retires the previous session and opens the phase for a new one, unless a session still
    /// runs. Returns the event sink for the new session.
    fn begin_session(
        &mut self,
        event_sink: Arc<dyn ProviderEventSink>,
    ) -> Option<Arc<dyn ProviderEventSink>> {
        let generation = self
            .snapshot
            .update(|state| {
                state.apply(RuntimeInput::SessionStarting).ok()?;
                state.generation += 1;
                state.session_id = None;
                state.capabilities = crate::models::ProviderCapabilities::default();
                Some(state.generation)
            })
            .flatten()?;
        self.driver.take();
        self.restart_when_quiet = None;
        self.clear_stop_checks();
        self.session = None;
        let sink: Arc<dyn ProviderEventSink> = Arc::new(GenerationEventSink {
            thread_id: self.thread_id.clone(),
            generation,
            snapshot: Arc::clone(&self.snapshot),
            inner: event_sink,
        });
        self.event_sink = Some(Arc::clone(&sink));
        Some(sink)
    }

    /// Records the outcome of a start or resume and keeps the driver for later commands.
    async fn finish_session(
        &mut self,
        driver: Box<dyn ProviderDriver>,
        result: &ChatResult<ProviderSessionSnapshot>,
        context: &DriverOperationContext,
    ) {
        match result {
            Ok(session) => {
                self.session = Some(session.clone());
                self.update(|state| {
                    state.session_id = Some(session.session_id.clone());
                    state.capabilities = session.capabilities.clone();
                    let _ = state.apply(RuntimeInput::SessionOpened);
                });
            }
            Err(_) => {
                let _ = self.flush_events(context).await;
                self.apply(RuntimeInput::SessionOpenFailed);
            }
        }
        self.driver = Some(driver);
    }

    /// Ends a turn the driver could not dispatch. The provider may already run it, for example
    /// when the dispatch timed out after the provider started, so the session stops: its later
    /// events would land on a turn recorded as failed.
    async fn abandon_dispatch(&mut self, turn_id: ChatTurnId) {
        self.apply(RuntimeInput::DispatchFailed(turn_id));
        let deadline = Instant::now() + DEFAULT_STOP_TIMEOUT;
        if let Err(error) = self
            .stop_driver_with_outcome(
                false,
                operation_context("runtime-dispatch-failed", deadline),
                StopOutcome::Failed,
            )
            .await
        {
            eprintln!(
                "Chat could not stop a provider after a failed dispatch, code {:?}",
                error.code
            );
        }
    }

    async fn flush_events(&mut self, context: &DriverOperationContext) -> ChatResult<()> {
        match self.event_sink.as_ref() {
            Some(sink) => run_driver_operation(context, sink.flush()).await,
            None => Ok(()),
        }
    }

    fn session_state(&self) -> ProviderSessionState {
        self.read(|state| state.session_state())
    }

    /// Applies `input` to the phase and reports whether the phase allowed it.
    fn apply(&self, input: RuntimeInput) -> bool {
        self.snapshot
            .update(|snapshot| snapshot.apply(input).is_ok())
            .unwrap_or(false)
    }

    /// Reads the snapshot. A poisoned snapshot reads as a failed session.
    fn read<T>(&self, read: impl FnOnce(&ThreadRuntimeSnapshot) -> T) -> T {
        self.snapshot.read_or(
            || ThreadRuntimeSnapshot {
                phase: RuntimePhase::Failed { turn: None },
                ..ThreadRuntimeSnapshot::stopped()
            },
            read,
        )
    }

    fn touch(&self) {
        self.update(|state| state.last_activity = Instant::now());
    }

    fn update(&self, update: impl FnOnce(&mut ThreadRuntimeSnapshot)) {
        self.snapshot.update(update);
    }
}

/// How a stop reports the phase it leaves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StopOutcome {
    /// `Stopped` after a clean stop, `Failed` otherwise.
    FromResult,
    /// `Failed`, because the session already failed before the stop.
    Failed,
}

fn session_running_error() -> ChatError {
    runtime_invalid_state_error("Chat provider session is already running")
}

pub(super) async fn run_thread_runtime(
    thread_id: ChatThreadId,
    mut receiver: mpsc::Receiver<ThreadRuntimeCommand>,
    snapshot: Arc<SnapshotCell>,
    idle_timeout: IdleTimeout,
    interrupt_confirmation_timeout: Duration,
) {
    let mut worker = RuntimeWorker::new(thread_id, snapshot, interrupt_confirmation_timeout);
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
