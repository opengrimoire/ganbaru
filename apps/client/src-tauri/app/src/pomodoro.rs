//! Native Focus commands and canonical history reads. WebViews cannot write execution.

use crate::db::connect_sqlite;
pub use ganbaru_pomodoro::*;
use tauri::{AppHandle, Runtime};

#[cfg(desktop)]
pub(crate) mod idle;
pub(crate) mod native_runtime;
#[cfg(desktop)]
pub(crate) mod overlay;
pub(crate) mod sync;
pub(crate) use ganbaru_sync::manifest::vault::calendar::DOMAIN_NAME as CALENDAR_DOMAIN;
pub(crate) use ganbaru_sync::manifest::vault::focus::DOMAIN_NAME as FOCUS_DOMAIN;
#[cfg(desktop)]
pub(crate) use native_runtime::{
    FocusNativeContext, capture_native_context, native_control_in_context,
};
pub(crate) use native_runtime::{
    invalidate_calendar, resume_after_vault_handoff, setup, stop_for_vault_handoff,
};

/// Whether this device may run Focus: as the owner, or as a linked replica, since each device
/// owns its own runs and closed runs replicate.
pub(crate) fn focus_writable(status: &crate::vault::ownership::VaultOwnershipStatus) -> bool {
    status.can_write_domain(FOCUS_DOMAIN)
}

/// Whether this device may save a Calendar edit through the native Focus owner, which writes
/// Calendar rows and the Focus references to them in one transaction.
pub(crate) fn calendar_edit_writable(
    status: &crate::vault::ownership::VaultOwnershipStatus,
) -> bool {
    status.can_write_domain(CALENDAR_DOMAIN) && focus_writable(status)
}

/// A fresh Focus activity row of another linked device, named from the linked-device records.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusOtherDeviceActivity {
    pub device: ganbaru_sync::DeviceRef,
    pub state: FocusActivityState,
    pub occurrence_id: Option<String>,
    pub event_title: Option<String>,
    pub phase_started_at_ms: Option<i64>,
    pub phase_ends_at_ms: Option<i64>,
    pub expires_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Names each activity row with the device `devices` knows it as.
fn name_activity(
    rows: Vec<FocusDeviceActivity>,
    devices: &ganbaru_sync::DeviceNames,
) -> Vec<FocusOtherDeviceActivity> {
    rows.into_iter()
        .map(|row| FocusOtherDeviceActivity {
            device: devices.describe(&row.device_id),
            state: row.state,
            occurrence_id: row.occurrence_id,
            event_title: row.event_title,
            phase_started_at_ms: row.phase_started_at_ms,
            phase_ends_at_ms: row.phase_ends_at_ms,
            expires_at_ms: row.expires_at_ms,
            updated_at_ms: row.updated_at_ms,
        })
        .collect()
}

/// Read what other linked devices are focusing on: their fresh running or paused activity rows,
/// newest first. Expired rows never count as focusing.
#[tauri::command]
pub async fn focus_device_activity<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<FocusOtherDeviceActivity>, String> {
    let devices = crate::sync::devices::read(&app)?;
    let local_device_id = crate::sync::devices::stable_device_id(&app)?;
    let now_ms = i64::try_from(crate::sync::now_ms()?)
        .map_err(|_| "The device clock is outside the Focus activity range".to_owned())?;
    let pool = connect_sqlite(app, db_url).await?;
    let mut connection = pool
        .acquire()
        .await
        .map_err(|error| format!("Open the vault to read Focus activity: {error}"))?;
    let rows = focus_read_other_device_activity(&mut connection, &local_device_id, now_ms)
        .await
        .map_err(|error| error.message)?;
    Ok(name_activity(rows, &devices))
}

/// Read canonical execution history, resolving current occurrence aliases for the Calendar rail.
#[tauri::command]
pub async fn pomodoro_load_segments_for_events<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    event_ids: Vec<String>,
) -> Result<Vec<PomodoroSegmentRead>, String> {
    ganbaru_pomodoro::pomodoro_load_segments_for_events(
        connect_sqlite(app, db_url).await?,
        event_ids,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn device_activity_names_devices_and_matches_the_frontend_shape() {
        let devices = ganbaru_sync::DeviceNames::new(
            "local",
            HashMap::from([("laptop".to_string(), "Laptop".to_string())]),
        );
        let row = |device_id: &str| FocusDeviceActivity {
            device_id: device_id.to_string(),
            state: FocusActivityState::ShortBreak,
            occurrence_id: Some("occurrence".to_string()),
            event_title: Some("Write".to_string()),
            phase_started_at_ms: Some(1_000),
            phase_ends_at_ms: Some(2_000),
            expires_at_ms: 3_000,
            updated_at_ms: 1_500,
        };
        let named = name_activity(vec![row("laptop"), row("phone")], &devices);
        assert_eq!(
            serde_json::to_value(&named[0]).unwrap(),
            serde_json::json!({
                "device": { "deviceId": "laptop", "deviceLabel": "Laptop", "ownDevice": false },
                "state": "short_break",
                "occurrenceId": "occurrence",
                "eventTitle": "Write",
                "phaseStartedAtMs": 1_000,
                "phaseEndsAtMs": 2_000,
                "expiresAtMs": 3_000,
                "updatedAtMs": 1_500,
            })
        );
        assert_eq!(named[1].device.device_label, None);
        assert!(!named[1].device.own_device);
    }
}
