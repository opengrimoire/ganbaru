//! Driver-side scripts that run while a scenario transcript plays.

use super::runner::ReplayProvider;
use crate::models::{ApprovalDecisionKind, InteractionMode, SafetyMode};

/// One driver call or wait a script makes.
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
    /// Rewinds the native thread so `provider_turn` is its newest turn, as a checkpoint
    /// restore does with the cursor of that turn.
    Rollback { provider_turn: &'static str },
    /// Answers the most recently opened request.
    Resolve { decision: ApprovalDecisionKind },
    /// Waits until the driver emitted `count` events of `kind` in total.
    AwaitEvent { kind: &'static str, count: usize },
    /// Waits until the transcript has no entries left to play.
    AwaitTranscript,
    /// Lets `seconds` pass on the paused clock while the transcript plays, for scenarios whose
    /// provider stays silent longer than a step may wait.
    Elapse { seconds: u64 },
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
            Self::Rollback { .. } => "rollback",
            Self::Resolve { .. } => "resolve_approval",
            Self::AwaitEvent { .. } => "await_event",
            Self::AwaitTranscript => "await_transcript",
            Self::Elapse { .. } => "elapse",
            Self::Stop => "stop_session",
        }
    }
}

/// The driver calls of one scenario.
#[derive(Clone, Debug)]
pub(crate) struct ReplayScript {
    /// Whether the driver runs with organizational authority.
    pub(crate) organizational: bool,
    /// Whether the replay runs on the wall clock instead of the paused one, for scenarios
    /// whose driver waits on real processes or sockets.
    pub(crate) real_time: bool,
    /// The silence bound of a busy event stream, in place of the driver default, for HTTP
    /// scenarios that replay a stalled stream.
    pub(crate) busy_stream_timeout: Option<std::time::Duration>,
    pub(crate) steps: Vec<ReplayStep>,
}

pub(super) const ASK: SafetyMode = SafetyMode::AskForApproval;
pub(super) const BUILD: InteractionMode = InteractionMode::Build;

/// Starts a session that asks for approval in build mode.
pub(super) const START: ReplayStep = ReplayStep::Start {
    safety: ASK,
    interaction: BUILD,
};

/// Sends `prompt` as `turn`, asking for approval in build mode.
pub(super) const fn send(turn: &'static str, prompt: &'static str) -> ReplayStep {
    ReplayStep::Send {
        turn,
        prompt,
        safety: ASK,
        interaction: BUILD,
    }
}

/// Waits until `count` turns completed in total.
pub(super) const fn settled(count: usize) -> ReplayStep {
    ReplayStep::AwaitEvent {
        kind: "turn_completed",
        count,
    }
}

/// Waits until `count` events of `kind` were emitted in total.
pub(super) const fn awaited(kind: &'static str, count: usize) -> ReplayStep {
    ReplayStep::AwaitEvent { kind, count }
}

/// A script on a direct thread.
pub(super) fn direct(steps: Vec<ReplayStep>) -> ReplayScript {
    ReplayScript {
        organizational: false,
        real_time: false,
        busy_stream_timeout: None,
        steps,
    }
}

/// A script on a direct thread that runs on the wall clock.
pub(super) fn real_time(steps: Vec<ReplayStep>) -> ReplayScript {
    ReplayScript {
        real_time: true,
        ..direct(steps)
    }
}

/// A script with organizational authority.
pub(super) fn organizational(steps: Vec<ReplayStep>) -> ReplayScript {
    ReplayScript {
        organizational: true,
        real_time: false,
        busy_stream_timeout: None,
        steps,
    }
}

/// The script of `scenario` for `provider`, when one exists.
pub(crate) fn script(provider: ReplayProvider, scenario: &str) -> Option<ReplayScript> {
    match provider {
        ReplayProvider::Claude => claude::script(scenario),
        ReplayProvider::Codex => codex::script(scenario),
        ReplayProvider::Cursor => cursor::script(scenario),
        ReplayProvider::Grok => grok::script(scenario),
        ReplayProvider::Acp => acp::script(scenario),
        ReplayProvider::OpenCode => opencode::script(scenario),
    }
}

/// One test per scenario that replays its transcript and compares the snapshot.
macro_rules! replay_scenarios {
    ($provider:ident: $($scenario:ident),* $(,)?) => {
        #[cfg(test)]
        mod scenarios {
            $(
                #[test]
                fn $scenario() {
                    if let Err(failure) = crate::replay::runner::run_scenario(
                        crate::replay::runner::ReplayProvider::$provider,
                        stringify!($scenario),
                    ) {
                        panic!("{failure}");
                    }
                }
            )*
        }
    };
}

mod acp;
mod claude;
mod codex;
mod cursor;
mod grok;
mod opencode;
