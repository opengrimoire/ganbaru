//! How a committed Focus mode reaches the shared phase blocking rule, on every platform.

use ganbaru_distractions::contracts::PauseReason;
use ganbaru_pomodoro::FocusMode;

/// Whether a Focus mode holds an accepted phase, and why the phase is paused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusPhaseHold {
    /// The phase is running.
    Running,
    /// The phase is paused for this reason.
    Paused(PauseReason),
    /// No phase is held, so phase blocking never applies.
    Released,
}

impl FocusPhaseHold {
    /// The hold a committed Focus mode places on its phase.
    pub(crate) fn of(mode: FocusMode) -> Self {
        match mode {
            FocusMode::Running => Self::Running,
            FocusMode::ManualPause => Self::Paused(PauseReason::Manual),
            FocusMode::IdlePause => Self::Paused(PauseReason::Idle),
            FocusMode::Suspended => Self::Paused(PauseReason::Suspend),
            FocusMode::Stopped | FocusMode::IdleFailed | FocusMode::ReturnWait | FocusMode::Expired => {
                Self::Released
            }
        }
    }

    /// Whether the `pauseDuringFocusPause` setting lifts phase blocking for this hold. Only the
    /// Android notification bridge sends it; desktop applies the rule inside its authority.
    #[cfg_attr(not(mobile), allow(dead_code))]
    pub(crate) fn pause_lifts_blocking(self) -> bool {
        matches!(self, Self::Paused(reason) if reason.lifts_phase_blocking())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_manual_pause_lets_the_pause_setting_lift_blocking() {
        let lifting = [
            FocusMode::Running,
            FocusMode::ManualPause,
            FocusMode::IdlePause,
            FocusMode::Suspended,
            FocusMode::Stopped,
            FocusMode::IdleFailed,
            FocusMode::ReturnWait,
            FocusMode::Expired,
        ]
        .into_iter()
        .filter(|mode| FocusPhaseHold::of(*mode).pause_lifts_blocking())
        .collect::<Vec<_>>();
        assert_eq!(lifting, vec![FocusMode::ManualPause]);
    }
}
