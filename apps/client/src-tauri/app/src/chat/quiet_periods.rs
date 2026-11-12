//! This device's quiet periods: the stored windows, the quiet state now, and the commands that
//! read and save them.
//!
//! Quiet windows are device-local and never sync. A Focus phase in progress on this device also
//! counts as quiet; its state comes from the native Focus owner.

use super::device_state::{read_active_device_scope, update_active_device_scope};
use crate::pomodoro::native_runtime::{FocusPhaseActivity, focus_phase_activity};
use chrono::NaiveDateTime;
use ganbaru_chat::scheduling::{
    FocusQuiet, QuietPeriodPreferences, QuietPeriodState, QuietReason, evaluate_quiet_period,
    validate_quiet_preferences,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult, UtcTimestamp};
use jiff::tz::TimeZone;
use serde::Serialize;
use std::time::SystemTime;
use tauri::Runtime;

/// The quiet state of this device at one instant, with the end as an instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct QuietReading {
    /// The evaluated state.
    pub(crate) state: QuietPeriodState,
    /// Unix milliseconds the quiet period ends, when it is quiet and the end is known.
    pub(crate) ends_at_ms: Option<i64>,
}

/// The quiet state that `chat_read_quiet_state` reports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatQuietStateRead {
    /// Whether this device is quiet now.
    pub quiet: bool,
    /// What makes it quiet: `window` or `focus`.
    pub reason: Option<&'static str>,
    /// When the quiet period ends, or `None` when it is open or the end is unknown.
    pub ends_at: Option<UtcTimestamp>,
}

fn device_state_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Quiet periods could not be read or saved",
        true,
    )
}

fn local_time_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "This device's local time could not be determined",
        true,
    )
}

fn now_millis() -> ChatResult<i64> {
    let elapsed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "The clock is before 1970", false))?;
    i64::try_from(elapsed.as_millis())
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "The clock is out of range", false))
}

fn local_at(epoch_ms: i64, zone: &TimeZone) -> ChatResult<NaiveDateTime> {
    ganbaru_civil_time::instant_to_local(epoch_ms, zone).map_err(local_time_error)
}

/// The Focus quiet input of `activity`, with its deadline projected into `zone`.
fn focus_quiet(activity: Option<FocusPhaseActivity>, zone: &TimeZone) -> ChatResult<FocusQuiet> {
    let Some(activity) = activity else {
        return Ok(FocusQuiet::Inactive);
    };
    Ok(FocusQuiet::Active {
        phase_ends_local: activity
            .deadline_ms
            .map(|deadline_ms| local_at(deadline_ms, zone))
            .transpose()?,
    })
}

/// Evaluates `preferences` and `activity` at `now_ms` in `zone`, resolving the end back to an
/// instant. An end that does not lie after `now_ms`, as across a clock change, is unknown.
fn reading_in_zone(
    preferences: &QuietPeriodPreferences,
    activity: Option<FocusPhaseActivity>,
    now_ms: i64,
    zone: &TimeZone,
) -> ChatResult<QuietReading> {
    let state = evaluate_quiet_period(
        preferences,
        local_at(now_ms, zone)?,
        focus_quiet(activity, zone)?,
    );
    let ends_at_ms = match state {
        QuietPeriodState::Quiet {
            ends_local: Some(end),
            ..
        } => Some(ganbaru_civil_time::explicit_instant(end, zone).map_err(local_time_error)?)
            .filter(|end_ms| *end_ms > now_ms),
        _ => None,
    };
    Ok(QuietReading { state, ends_at_ms })
}

/// This device's quiet state at `now_ms`: its quiet windows in the device zone, captured once,
/// and the Focus phase in progress here.
pub(crate) async fn quiet_reading_at<R: Runtime>(
    app: &tauri::AppHandle<R>,
    now_ms: i64,
) -> ChatResult<QuietReading> {
    let preferences = read_active_device_scope(app)
        .map_err(device_state_error)?
        .quiet_periods;
    let zone = ganbaru_civil_time::system_zone().map_err(local_time_error)?;
    let activity = focus_phase_activity(app).await;
    reading_in_zone(&preferences, activity, now_ms, &zone)
}

/// This device's quiet state now, with its end as an instant. Repeating scheduled messages,
/// notifications, and budget warnings hold for it.
pub(crate) async fn current_quiet_state<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> ChatResult<QuietReading> {
    quiet_reading_at(app, now_millis()?).await
}

fn timestamp_at_millis(epoch_ms: i64) -> ChatResult<UtcTimestamp> {
    let time =
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(epoch_ms).ok_or_else(|| {
            ChatError::new(ChatErrorCode::Internal, "The time is out of range", false)
        })?;
    UtcTimestamp::new(time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

fn quiet_state_read(reading: QuietReading) -> ChatResult<ChatQuietStateRead> {
    let QuietPeriodState::Quiet { reason, .. } = reading.state else {
        return Ok(ChatQuietStateRead {
            quiet: false,
            reason: None,
            ends_at: None,
        });
    };
    Ok(ChatQuietStateRead {
        quiet: true,
        reason: Some(match reason {
            QuietReason::Window => "window",
            QuietReason::Focus => "focus",
        }),
        ends_at: reading.ends_at_ms.map(timestamp_at_millis).transpose()?,
    })
}

/// Reads this device's quiet windows.
#[tauri::command]
pub fn chat_read_quiet_periods(app: tauri::AppHandle) -> ChatResult<QuietPeriodPreferences> {
    Ok(read_active_device_scope(&app)
        .map_err(device_state_error)?
        .quiet_periods)
}

/// Stores this device's quiet windows after validating them. They never sync.
#[tauri::command]
pub fn chat_save_quiet_periods(
    app: tauri::AppHandle,
    preferences: QuietPeriodPreferences,
) -> ChatResult<QuietPeriodPreferences> {
    validate_quiet_preferences(&preferences)
        .map_err(|error| ChatError::validation("quietPeriods", error.to_string()))?;
    update_active_device_scope(&app, |scope| {
        scope.quiet_periods = preferences.clone();
        Ok(())
    })
    .map_err(device_state_error)?;
    Ok(preferences)
}

/// Reads whether this device is quiet now, why, and until when.
#[tauri::command]
pub async fn chat_read_quiet_state(app: tauri::AppHandle) -> ChatResult<ChatQuietStateRead> {
    quiet_state_read(current_quiet_state(&app).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_chat::scheduling::{QuietWindow, WeekdaySet};

    fn zone() -> TimeZone {
        ganbaru_civil_time::zone("America/Mexico_City").unwrap()
    }

    /// 2026-11-12 is a Thursday; Mexico City is UTC-6 all year.
    fn local_ms(hour: i64, minute: i64) -> i64 {
        1_794_441_600_000 + 6 * 3_600_000 + (hour * 60 + minute) * 60_000
    }

    fn night() -> QuietPeriodPreferences {
        QuietPeriodPreferences {
            windows: vec![QuietWindow {
                weekdays: WeekdaySet::from_mask(0b111_1111).unwrap(),
                start_minute: 22 * 60,
                end_minute: 7 * 60,
            }],
        }
    }

    #[test]
    fn quiet_window_end_resolves_to_the_next_morning_instant() {
        let reading = reading_in_zone(&night(), None, local_ms(23, 0), &zone()).unwrap();
        assert_eq!(
            reading,
            QuietReading {
                state: QuietPeriodState::Quiet {
                    reason: QuietReason::Window,
                    ends_local: Some(local_at(local_ms(31, 0), &zone()).unwrap()),
                },
                ends_at_ms: Some(local_ms(31, 0)),
            }
        );
        let read = quiet_state_read(reading).unwrap();
        assert!(read.quiet);
        assert_eq!(read.reason, Some("window"));
        assert_eq!(read.ends_at.unwrap().as_str(), "2026-11-13T13:00:00.000Z");
    }

    #[test]
    fn quiet_focus_phase_ends_at_its_deadline_or_has_no_known_end() {
        let now = local_ms(10, 0);
        let running = FocusPhaseActivity {
            deadline_ms: Some(now + 25 * 60_000),
        };
        let reading = reading_in_zone(&night(), Some(running), now, &zone()).unwrap();
        assert_eq!(reading.ends_at_ms, Some(now + 25 * 60_000));
        assert_eq!(quiet_state_read(reading).unwrap().reason, Some("focus"));

        let paused = FocusPhaseActivity { deadline_ms: None };
        let reading = reading_in_zone(&night(), Some(paused), now, &zone()).unwrap();
        assert!(matches!(reading.state, QuietPeriodState::Quiet { .. }));
        assert_eq!(reading.ends_at_ms, None);
        let read = quiet_state_read(reading).unwrap();
        assert!(read.quiet);
        assert_eq!(read.ends_at, None);
    }

    #[test]
    fn quiet_state_is_open_outside_windows_without_focus() {
        let reading = reading_in_zone(&night(), None, local_ms(12, 0), &zone()).unwrap();
        assert_eq!(
            reading,
            QuietReading {
                state: QuietPeriodState::Open,
                ends_at_ms: None,
            }
        );
        assert_eq!(
            quiet_state_read(reading).unwrap(),
            ChatQuietStateRead {
                quiet: false,
                reason: None,
                ends_at: None,
            }
        );
    }

    #[test]
    fn quiet_preferences_persist_in_the_device_scope_and_default_to_none() {
        let mut scope = crate::chat::device_state::ChatDeviceScope {
            quiet_periods: night(),
            ..Default::default()
        };
        let stored = serde_json::to_value(&scope).unwrap();
        assert_eq!(
            stored["quietPeriods"]["windows"][0]["startMinute"],
            serde_json::json!(22 * 60)
        );
        scope = serde_json::from_value(stored).unwrap();
        assert_eq!(scope.quiet_periods, night());
        let mut without = serde_json::to_value(&scope).unwrap();
        without.as_object_mut().unwrap().remove("quietPeriods");
        let restored: crate::chat::device_state::ChatDeviceScope =
            serde_json::from_value(without).unwrap();
        assert_eq!(restored.quiet_periods, QuietPeriodPreferences::default());
    }
}
