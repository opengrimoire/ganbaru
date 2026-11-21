use super::*;
use chrono::{DateTime, Utc};
use ganbaru_distractions::contracts::{ActivePhase, BlockingPhase, PauseReason, RuntimeAuthority};
#[cfg(target_os = "linux")]
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

fn state(phase: BlockingPhase, pause_reason: Option<PauseReason>) -> DistractionsRuntimeState {
    DistractionsRuntimeState {
        authority: RuntimeAuthority::Active(ActivePhase {
            phase,
            pause_reason,
            run_id: "run-1".to_string(),
            occurrence_id: None,
            remaining_seconds: 30,
        }),
        updated_at: "2026-05-26T00:00:00.000Z".to_string(),
        valid_until_ms: i64::MAX,
    }
}

fn focus_state() -> DistractionsRuntimeState {
    state(BlockingPhase::Focus, None)
}

mod catalog;
mod close_authorization;
mod contract;
mod rules;
mod state_files;
