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
mod activity;
pub use activity::{
    FOCUS_ACTIVITY_PAUSED_GRACE_MS, FOCUS_ACTIVITY_RUNNING_GRACE_MS, FocusActivityProjection,
    FocusActivityState, FocusDeviceActivity, focus_activity_projection,
    focus_read_other_device_activity, focus_write_device_activity_tx,
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
