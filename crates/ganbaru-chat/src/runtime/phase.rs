//! The lifecycle of one thread's provider session, owned by a pure transition function.
//!
//! The runtime worker and the generation event sink both change the session only through
//! [`RuntimePhase::apply`]; the wire [`ProviderSessionState`], whether a turn runs, and which
//! requests are open are derived from the phase instead of stored beside it.

use crate::models::{ChatTurnId, ProviderRequestId, ProviderSessionState};
use std::time::Instant;

/// What an open provider request waits for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenRequestKind {
    Approval,
    UserInput,
}

impl OpenRequestKind {
    fn waiting_state(self) -> ProviderSessionState {
        match self {
            Self::Approval => ProviderSessionState::WaitingForApproval,
            Self::UserInput => ProviderSessionState::WaitingForUserInput,
        }
    }
}

/// Whether the provider has reported the start of a turn the runtime dispatched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnProgress {
    /// The runtime accepted the turn and the provider has not reported its start yet.
    Dispatching,
    /// The provider reported the start of the turn.
    Running,
}

/// A turn whose provider events the runtime still accepts, with the requests it holds open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeTurn {
    id: ChatTurnId,
    progress: TurnProgress,
    /// Open requests in the order the provider opened them.
    requests: Vec<(ProviderRequestId, OpenRequestKind)>,
}

impl RuntimeTurn {
    fn dispatched(id: ChatTurnId) -> Self {
        Self {
            id,
            progress: TurnProgress::Dispatching,
            requests: Vec::new(),
        }
    }

    pub fn id(&self) -> &ChatTurnId {
        &self.id
    }

    pub fn progress(&self) -> TurnProgress {
        self.progress
    }

    /// Open requests, oldest first.
    pub fn requests(&self) -> &[(ProviderRequestId, OpenRequestKind)] {
        &self.requests
    }

    fn request_kind(&self, request_id: &ProviderRequestId) -> Option<OpenRequestKind> {
        self.requests
            .iter()
            .find(|(id, _)| id == request_id)
            .map(|(_, kind)| *kind)
    }

    /// The wire state of a running turn: waiting on its oldest open request, or active.
    fn wire_state(&self) -> ProviderSessionState {
        self.requests
            .first()
            .map_or(ProviderSessionState::Active, |(_, kind)| {
                kind.waiting_state()
            })
    }
}

/// The lifecycle of one thread's provider session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimePhase {
    /// No provider session runs.
    Stopped,
    /// A start or resume is in flight.
    Starting,
    /// The session runs and holds no turn.
    Ready,
    /// The session runs `turn`.
    Turn(RuntimeTurn),
    /// An interrupt of `turn` was sent to the provider and has not been answered.
    InterruptRequested(RuntimeTurn),
    /// The provider accepted an interrupt of `turn`; past `deadline` the runtime settles the turn
    /// itself and stops the provider.
    Interrupting {
        turn: RuntimeTurn,
        deadline: Instant,
    },
    /// The runtime is stopping the session; `turn` is a turn it interrupts on the way, whose
    /// settlement events it still accepts.
    Stopping { turn: Option<RuntimeTurn> },
    /// The session failed and must start again; `turn` is a turn left unsettled by the failure,
    /// whose settlement events it still accepts.
    Failed { turn: Option<RuntimeTurn> },
}

/// A change reported by the runtime worker or by a provider event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeInput {
    /// The worker starts or resumes a session.
    SessionStarting,
    /// The driver opened the session.
    SessionOpened,
    /// The driver could not open the session.
    SessionOpenFailed,
    /// The worker hands a turn, or a context compaction, to the driver.
    TurnAccepted(ChatTurnId),
    /// The driver refused a context compaction, so its turn never ran.
    TurnWithdrawn(ChatTurnId),
    /// The driver could not dispatch the turn; the session state is unknown and must stop.
    DispatchFailed(ChatTurnId),
    /// The provider reported the start of the turn.
    TurnStarted(ChatTurnId),
    /// The provider completed or aborted the turn.
    TurnSettled(ChatTurnId),
    /// The provider opened a request in the current turn.
    RequestOpened {
        request_id: ProviderRequestId,
        kind: OpenRequestKind,
    },
    /// A request was answered or resolved. Closing a request that is not open changes nothing.
    RequestClosed(ProviderRequestId),
    /// The worker sends an interrupt of the turn to the provider.
    InterruptRequested(ChatTurnId),
    /// The provider accepted the interrupt; the runtime waits until `deadline`.
    InterruptAccepted {
        turn_id: ChatTurnId,
        deadline: Instant,
    },
    /// The provider refused the interrupt, so the turn keeps running.
    InterruptRejected(ChatTurnId),
    /// The worker stops the session.
    StopStarted,
    /// The worker finished stopping the session, cleanly or not.
    StopFinished { clean: bool },
    /// The provider reported that its session stopped.
    ProviderStopped,
    /// The provider reported a failure that ends the session.
    ProviderFailed,
    /// The provider process exited.
    ProviderExited { expected: bool },
}

/// A move the phase does not allow. The phase is unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeRejection {
    pub from: ProviderSessionState,
    pub input: &'static str,
}

impl RuntimeInput {
    fn name(&self) -> &'static str {
        match self {
            Self::SessionStarting => "session_starting",
            Self::SessionOpened => "session_opened",
            Self::SessionOpenFailed => "session_open_failed",
            Self::TurnAccepted(_) => "turn_accepted",
            Self::TurnWithdrawn(_) => "turn_withdrawn",
            Self::DispatchFailed(_) => "dispatch_failed",
            Self::TurnStarted(_) => "turn_started",
            Self::TurnSettled(_) => "turn_settled",
            Self::RequestOpened { .. } => "request_opened",
            Self::RequestClosed(_) => "request_closed",
            Self::InterruptRequested(_) => "interrupt_requested",
            Self::InterruptAccepted { .. } => "interrupt_accepted",
            Self::InterruptRejected(_) => "interrupt_rejected",
            Self::StopStarted => "stop_started",
            Self::StopFinished { .. } => "stop_finished",
            Self::ProviderStopped => "provider_stopped",
            Self::ProviderFailed => "provider_failed",
            Self::ProviderExited { .. } => "provider_exited",
        }
    }
}

impl RuntimePhase {
    /// The state reported over the wire.
    pub fn session_state(&self) -> ProviderSessionState {
        match self {
            Self::Stopped => ProviderSessionState::Stopped,
            Self::Starting => ProviderSessionState::Starting,
            Self::Ready => ProviderSessionState::Ready,
            Self::Turn(turn) => turn.wire_state(),
            Self::InterruptRequested(_) | Self::Interrupting { .. } | Self::Stopping { .. } => {
                ProviderSessionState::Stopping
            }
            Self::Failed { .. } => ProviderSessionState::Failed,
        }
    }

    /// The turn whose provider events the runtime still accepts.
    pub fn turn(&self) -> Option<&RuntimeTurn> {
        match self {
            Self::Turn(turn)
            | Self::InterruptRequested(turn)
            | Self::Interrupting { turn, .. }
            | Self::Stopping { turn: Some(turn) }
            | Self::Failed { turn: Some(turn) } => Some(turn),
            _ => None,
        }
    }

    fn turn_mut(&mut self) -> Option<&mut RuntimeTurn> {
        match self {
            Self::Turn(turn)
            | Self::InterruptRequested(turn)
            | Self::Interrupting { turn, .. }
            | Self::Stopping { turn: Some(turn) }
            | Self::Failed { turn: Some(turn) } => Some(turn),
            _ => None,
        }
    }

    pub fn active_turn_id(&self) -> Option<&ChatTurnId> {
        self.turn().map(RuntimeTurn::id)
    }

    /// Whether a turn runs on a usable session, including one being interrupted.
    pub fn turn_active(&self) -> bool {
        matches!(
            self,
            Self::Turn(_) | Self::InterruptRequested(_) | Self::Interrupting { .. }
        )
    }

    pub fn has_open_requests(&self) -> bool {
        self.turn().is_some_and(|turn| !turn.requests.is_empty())
    }

    /// The kind of an open request of the running turn.
    pub fn open_request(&self, request_id: &ProviderRequestId) -> Option<OpenRequestKind> {
        match self {
            Self::Turn(turn) => turn.request_kind(request_id),
            _ => None,
        }
    }

    /// When an accepted interrupt stops waiting for the provider.
    pub fn interrupt_deadline(&self) -> Option<(&ChatTurnId, Instant)> {
        match self {
            Self::Interrupting { turn, deadline } => Some((turn.id(), *deadline)),
            _ => None,
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Whether a new session may start: none runs, or the last one ended.
    pub fn can_start(&self) -> bool {
        matches!(self, Self::Stopped | Self::Failed { .. })
    }

    /// Whether the session runs, so its provider can answer status reads.
    pub fn is_running(&self) -> bool {
        !matches!(
            self,
            Self::Stopped | Self::Starting | Self::Stopping { .. } | Self::Failed { .. }
        )
    }

    /// Applies `input`, or leaves the phase unchanged and reports why the move is not allowed.
    pub fn apply(&mut self, input: RuntimeInput) -> Result<(), RuntimeRejection> {
        let rejection = RuntimeRejection {
            from: self.session_state(),
            input: input.name(),
        };
        let next = match (std::mem::replace(self, Self::Stopped), input) {
            (Self::Stopped | Self::Failed { .. }, RuntimeInput::SessionStarting) => Self::Starting,
            (Self::Starting, RuntimeInput::SessionOpened) => Self::Ready,
            (Self::Starting, RuntimeInput::SessionOpenFailed) => Self::Failed { turn: None },
            (Self::Ready, RuntimeInput::TurnAccepted(id)) => {
                Self::Turn(RuntimeTurn::dispatched(id))
            }
            (Self::Turn(turn), RuntimeInput::TurnWithdrawn(id)) if turn.id == id => Self::Ready,
            (Self::Turn(turn), RuntimeInput::DispatchFailed(id)) if turn.id == id => {
                Self::Failed { turn: None }
            }
            (mut phase, RuntimeInput::TurnStarted(id))
                if phase.active_turn_id() == Some(&id) && phase.turn_active() =>
            {
                if let Some(turn) = phase.turn_mut() {
                    turn.progress = TurnProgress::Running;
                }
                phase
            }
            (
                Self::Turn(turn) | Self::InterruptRequested(turn) | Self::Interrupting { turn, .. },
                RuntimeInput::TurnSettled(id),
            ) if turn.id == id => Self::Ready,
            (Self::Stopping { turn: Some(turn) }, RuntimeInput::TurnSettled(id))
                if turn.id == id =>
            {
                Self::Stopping { turn: None }
            }
            (Self::Failed { turn: Some(turn) }, RuntimeInput::TurnSettled(id)) if turn.id == id => {
                Self::Failed { turn: None }
            }
            (mut phase, RuntimeInput::RequestOpened { request_id, kind })
                if phase.turn().is_some() =>
            {
                if let Some(turn) = phase.turn_mut()
                    && turn.request_kind(&request_id).is_none()
                {
                    turn.requests.push((request_id, kind));
                }
                phase
            }
            (mut phase, RuntimeInput::RequestClosed(request_id)) => {
                if let Some(turn) = phase.turn_mut() {
                    turn.requests.retain(|(id, _)| *id != request_id);
                }
                phase
            }
            (Self::Turn(turn), RuntimeInput::InterruptRequested(id)) if turn.id == id => {
                Self::InterruptRequested(turn)
            }
            (
                Self::InterruptRequested(turn),
                RuntimeInput::InterruptAccepted { turn_id, deadline },
            ) if turn.id == turn_id => Self::Interrupting { turn, deadline },
            (Self::InterruptRequested(turn), RuntimeInput::InterruptRejected(id))
                if turn.id == id =>
            {
                Self::Turn(turn)
            }
            (Self::Stopped, RuntimeInput::StopStarted) => Self::Stopped,
            (Self::Stopping { turn }, RuntimeInput::StopStarted) => Self::Stopping { turn },
            (phase, RuntimeInput::StopStarted) => Self::Stopping {
                turn: phase.turn().cloned(),
            },
            (Self::Stopped, RuntimeInput::StopFinished { .. }) => Self::Stopped,
            (_, RuntimeInput::StopFinished { clean: true }) => Self::Stopped,
            (_, RuntimeInput::StopFinished { clean: false }) => Self::Failed { turn: None },
            (Self::Stopping { turn }, RuntimeInput::ProviderStopped) => Self::Stopping { turn },
            (_, RuntimeInput::ProviderStopped) => Self::Stopped,
            (Self::Stopping { turn }, RuntimeInput::ProviderFailed) => Self::Stopping { turn },
            (Self::Failed { turn }, RuntimeInput::ProviderFailed) => Self::Failed { turn },
            (phase @ Self::Stopped, RuntimeInput::ProviderFailed) => {
                *self = phase;
                return Err(rejection);
            }
            (phase, RuntimeInput::ProviderFailed) => Self::Failed {
                turn: phase.turn().cloned(),
            },
            (Self::Stopping { .. }, RuntimeInput::ProviderExited { .. }) => {
                Self::Stopping { turn: None }
            }
            (Self::Stopped, RuntimeInput::ProviderExited { .. }) => Self::Stopped,
            (_, RuntimeInput::ProviderExited { expected: true }) => Self::Stopped,
            (_, RuntimeInput::ProviderExited { expected: false }) => Self::Failed { turn: None },
            (phase, _) => {
                *self = phase;
                return Err(rejection);
            }
        };
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
