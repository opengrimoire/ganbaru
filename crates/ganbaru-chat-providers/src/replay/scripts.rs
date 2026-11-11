//! Driver-side scripts that run while a scenario transcript plays.

use super::runner::ReplayProvider;
use crate::models::{ApprovalDecisionKind, InteractionMode, SafetyMode};

/// One driver call or wait a script makes.
#[expect(
    dead_code,
    reason = "the steering, interruption, approval, and stop scenarios construct these"
)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum ReplayStep {
    /// Starts a fresh session.
    Start {
        safety: SafetyMode,
        interaction: InteractionMode,
    },
    /// Sends a turn on the started session.
    Send {
        turn: &'static str,
        prompt: &'static str,
        safety: SafetyMode,
        interaction: InteractionMode,
    },
    /// Steers the running turn.
    Steer {
        turn: &'static str,
        prompt: &'static str,
    },
    /// Interrupts the running turn.
    Interrupt { turn: &'static str },
    /// Answers the most recently opened request.
    Resolve { decision: ApprovalDecisionKind },
    /// Waits until the driver emitted `count` events of `kind` in total.
    AwaitEvent { kind: &'static str, count: usize },
    /// Waits until the transcript has no entries left to play.
    AwaitTranscript,
    /// Stops the session.
    Stop,
}

impl ReplayStep {
    /// The call name a snapshot records for this step.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Start { .. } => "start_session",
            Self::Send { .. } => "send_turn",
            Self::Steer { .. } => "steer_turn",
            Self::Interrupt { .. } => "interrupt_turn",
            Self::Resolve { .. } => "resolve_approval",
            Self::AwaitEvent { .. } => "await_event",
            Self::AwaitTranscript => "await_transcript",
            Self::Stop => "stop_session",
        }
    }
}

/// The driver calls of one scenario.
#[derive(Clone, Debug)]
pub(crate) struct ReplayScript {
    /// Whether the driver runs with organizational authority.
    pub(crate) organizational: bool,
    pub(crate) steps: Vec<ReplayStep>,
}

const ASK: SafetyMode = SafetyMode::AskForApproval;
const BUILD: InteractionMode = InteractionMode::Build;

const START: ReplayStep = ReplayStep::Start {
    safety: ASK,
    interaction: BUILD,
};

const fn send(turn: &'static str, prompt: &'static str) -> ReplayStep {
    ReplayStep::Send {
        turn,
        prompt,
        safety: ASK,
        interaction: BUILD,
    }
}

const fn settled(count: usize) -> ReplayStep {
    ReplayStep::AwaitEvent {
        kind: "turn_completed",
        count,
    }
}

fn direct(steps: Vec<ReplayStep>) -> ReplayScript {
    ReplayScript {
        organizational: false,
        steps,
    }
}

/// The script of `scenario` for `provider`, when one exists.
pub(crate) fn script(provider: ReplayProvider, scenario: &str) -> Option<ReplayScript> {
    match (provider, scenario) {
        (ReplayProvider::Claude, "simple") => {
            Some(direct(vec![START, send("turn-1", "Say hello"), settled(1)]))
        }
        _ => None,
    }
}
