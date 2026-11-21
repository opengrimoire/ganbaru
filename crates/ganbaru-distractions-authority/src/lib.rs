//! Runtime authority the app publishes for distraction blocking and the one rule that decides
//! whether it enforces phase blocking.
//!
//! The app writes [`DistractionsRuntimeState`] to the device-local `distractions-state.json` file
//! from committed Focus execution. Desktop close authorization in the app and the Chromium native
//! messaging host both read that file through [`DistractionsRuntimeState::parse`] and decide with
//! [`RuntimeAuthority::enforces_blocking`], so the two can never disagree. The crate depends only
//! on serde so the browser host stays small. Freshness bounds and the `enabled` switches stay with
//! each reader.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
mod tests;

/// Focus phase that carries blocking settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockingPhase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl BlockingPhase {
    /// Every phase, in Focus cycle order.
    pub const ALL: [Self; 3] = [Self::Focus, Self::ShortBreak, Self::LongBreak];

    /// Wire name used in the state file, browser host responses, and block events.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
        }
    }
}

/// Why an accepted phase is paused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    /// The user paused the phase.
    Manual,
    /// Local activity stopped long enough for Focus to pause itself.
    Idle,
    /// The device suspended during the phase.
    Suspend,
}

impl PauseReason {
    /// Every pause reason.
    pub const ALL: [Self; 3] = [Self::Manual, Self::Idle, Self::Suspend];

    /// Wire name used in the state file and browser host responses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Idle => "idle",
            Self::Suspend => "suspend",
        }
    }

    /// Strict pauses keep blocking regardless of the pause setting: an idle or suspended phase was
    /// not paused by a decision of the user.
    pub fn is_strict(self) -> bool {
        matches!(self, Self::Idle | Self::Suspend)
    }

    /// Phase recorded on a block event while the phase is paused for this reason.
    pub fn block_event_phase(self) -> &'static str {
        match self {
            Self::Manual => "manual_pause",
            Self::Idle => "idle_pause",
            Self::Suspend => "suspend_pause",
        }
    }
}

/// Accepted Focus phase that is running or paused.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivePhase {
    pub phase: BlockingPhase,
    /// `None` while the phase runs. Required in the file, written as `null` while running.
    #[serde(deserialize_with = "Option::deserialize")]
    pub pause_reason: Option<PauseReason>,
    pub run_id: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub occurrence_id: Option<String>,
    /// Whole seconds left in the phase when the state was written.
    pub remaining_seconds: i64,
}

impl ActivePhase {
    /// Whether the phase is paused for any reason.
    pub fn is_paused(&self) -> bool {
        self.pause_reason.is_some()
    }
}

/// Whether accepted Focus execution currently has a phase.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeAuthority {
    /// No accepted phase. Phase blocking never applies. The empty fields let serde reject phase
    /// data written next to this status.
    Inactive {},
    /// An accepted phase that is running or paused.
    Active(ActivePhase),
}

impl RuntimeAuthority {
    /// The accepted phase, if any.
    pub fn active_phase(&self) -> Option<&ActivePhase> {
        match self {
            Self::Inactive {} => None,
            Self::Active(active) => Some(active),
        }
    }

    /// The single blocking rule shared by desktop close authorization and the browser host:
    /// an inactive state never blocks, a manual pause stops blocking when `pauseDuringFocusPause`
    /// is set, strict pauses keep blocking, and otherwise the phase's own setting decides.
    pub fn enforces_blocking(&self, settings: PhaseBlockingSettings) -> bool {
        let Some(active) = self.active_phase() else {
            return false;
        };
        let manual_pause = active
            .pause_reason
            .is_some_and(|reason| !reason.is_strict());
        if manual_pause && settings.pause_during_focus_pause {
            return false;
        }
        settings.blocks_phase(active.phase)
    }

    /// Phase recorded on a block event, or `None` when no phase is active.
    pub fn block_event_phase(&self) -> Option<&'static str> {
        let active = self.active_phase()?;
        Some(match active.pause_reason {
            Some(reason) => reason.block_event_phase(),
            None => active.phase.as_str(),
        })
    }
}

/// Phase blocking settings of one blocker surface. Desktop close authorization reads them from
/// `distractions.desktop`, the browser host from `distractions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseBlockingSettings {
    pub block_during_focus: bool,
    pub block_during_short_breaks: bool,
    pub block_during_long_breaks: bool,
    pub pause_during_focus_pause: bool,
}

impl Default for PhaseBlockingSettings {
    /// Every setting defaults on, matching rules that omit them.
    fn default() -> Self {
        Self {
            block_during_focus: true,
            block_during_short_breaks: true,
            block_during_long_breaks: true,
            pause_during_focus_pause: true,
        }
    }
}

impl PhaseBlockingSettings {
    /// Reads `blockDuringFocus`, `blockDuringShortBreaks`, `blockDuringLongBreaks`, and
    /// `pauseDuringFocusPause` from a rules object. A missing or non-boolean value is on.
    pub fn from_rules(rules: &Value) -> Self {
        let flag = |key: &str| rules.get(key).and_then(Value::as_bool).unwrap_or(true);
        Self {
            block_during_focus: flag("blockDuringFocus"),
            block_during_short_breaks: flag("blockDuringShortBreaks"),
            block_during_long_breaks: flag("blockDuringLongBreaks"),
            pause_during_focus_pause: flag("pauseDuringFocusPause"),
        }
    }

    /// Whether blocking applies during `phase` while it runs.
    pub fn blocks_phase(self, phase: BlockingPhase) -> bool {
        match phase {
            BlockingPhase::Focus => self.block_during_focus,
            BlockingPhase::ShortBreak => self.block_during_short_breaks,
            BlockingPhase::LongBreak => self.block_during_long_breaks,
        }
    }
}

/// Contents of `distractions-state.json`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DistractionsRuntimeState {
    pub authority: RuntimeAuthority,
    /// RFC 3339 instant the state was written. Readers bound its age.
    pub updated_at: String,
    /// Epoch milliseconds after which the accepted phase no longer authorizes blocking.
    pub valid_until_ms: i64,
}

impl DistractionsRuntimeState {
    /// Parses and validates file contents. Any malformed or inconsistent file is an error, which
    /// readers treat as no runtime authority.
    pub fn parse(contents: &str) -> Result<Self, String> {
        let state: Self = serde_json::from_str(contents)
            .map_err(|error| format!("invalid distraction runtime state: {error}"))?;
        state.validate()?;
        Ok(state)
    }

    /// Checks the invariants serde cannot express.
    pub fn validate(&self) -> Result<(), String> {
        if self.updated_at.trim().is_empty() {
            return Err("updatedAt is required".into());
        }
        if self.valid_until_ms < 0 {
            return Err("validUntilMs must be non-negative".into());
        }
        if let Some(active) = self.authority.active_phase() {
            if active.run_id.trim().is_empty() {
                return Err("an active phase requires a run id".into());
            }
            if active.remaining_seconds < 0 {
                return Err("remainingSeconds must be non-negative".into());
            }
        }
        Ok(())
    }

    /// Whether the accepted phase validity has ended at `checked_at_ms`, given the parsed
    /// `updatedAt` instant. A running phase also ends once its remaining time elapses; a paused
    /// phase keeps its remaining time.
    pub fn validity_expired(&self, updated_at_ms: i64, checked_at_ms: i64) -> bool {
        if checked_at_ms >= self.valid_until_ms {
            return true;
        }
        self.authority.active_phase().is_some_and(|active| {
            !active.is_paused()
                && checked_at_ms
                    >= updated_at_ms.saturating_add(active.remaining_seconds.saturating_mul(1000))
        })
    }
}
