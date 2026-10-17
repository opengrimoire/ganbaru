//! Semantic Focus execution. Application adapters supply authorized transactions
//! and native Calendar/activity observations; callers cannot submit history writes.

mod models;
pub use models::*;
mod decisions;
mod persistence;
pub use persistence::{focus_read_execution_snapshot, focus_read_execution_snapshot_tx};
mod mutations;
mod service;
pub use service::{
    focus_apply_observation_tx, focus_execute_command_tx, focus_read_command_receipt_tx,
};
mod adaptive;
mod calendar;
mod observations;
pub use calendar::{
    FocusCalendarCompletion, FocusCalendarReferenceChange, focus_complete_calendar_tx,
    focus_retarget_calendar_tx,
};
#[cfg(test)]
mod tests;
