use super::*;
use chrono::{DateTime, Utc};
#[cfg(target_os = "linux")]
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

fn state(phase: &str) -> DistractionsRuntimeState {
    DistractionsRuntimeState {
        active: phase != "inactive",
        paused: false,
        pause_reason: None,
        phase: phase.to_string(),
        active_run_id: None,
        active_occurrence_id: None,
        remaining_seconds: Some(30),
        updated_at: "2026-05-26T00:00:00.000Z".to_string(),
        valid_until_ms: None,
    }
}

mod catalog;
mod close_authorization;
mod contract;
mod rules;
mod state_files;
