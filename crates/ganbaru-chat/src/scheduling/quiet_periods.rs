//! Device quiet periods: weekly local windows plus this device's Focus phase.
//!
//! A window whose end minute is before its start minute crosses midnight and belongs to the day
//! it starts on. Overlapping and adjacent windows count as one quiet period, so its end is the
//! end of the whole merged span. Only a Focus phase is quiet; short and long breaks are open,
//! because a break is the natural moment for a batch of updates.

use std::fmt;

use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

use super::{LAST_MINUTE_OF_DAY, WeekdaySet};

/// Most quiet windows one device keeps.
pub const MAX_QUIET_WINDOWS: usize = 8;

/// Days after the evaluated date whose windows may extend a merged quiet period. A merged span
/// reaching past them covers the whole week and has no end.
const MERGE_HORIZON_DAYS: u64 = 8;
const MINUTES_PER_HOUR: u16 = 60;

/// A weekly local window during which the device is quiet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuietWindow {
    /// Days the window starts on.
    pub weekdays: WeekdaySet,
    /// Local minute the window starts, from 0 to 1439.
    pub start_minute: u16,
    /// Local minute the window ends, exclusive; before `start_minute` when it crosses midnight.
    pub end_minute: u16,
}

/// The quiet windows of one device.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuietPeriodPreferences {
    /// Weekly windows, at most [`MAX_QUIET_WINDOWS`].
    pub windows: Vec<QuietWindow>,
}

/// This device's Focus state as quiet period evaluation sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusQuiet {
    /// No Focus phase is running.
    Inactive,
    /// A Focus phase is running, ending at a local time when its deadline is known.
    Active {
        /// Local end of the phase, or `None` when it has no deadline.
        phase_ends_local: Option<NaiveDateTime>,
    },
}

/// Why the device is quiet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuietReason {
    /// A quiet window is active; it wins when a Focus phase also runs.
    Window,
    /// A Focus phase is running.
    Focus,
}

/// The device's quiet state at one local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuietPeriodState {
    /// Nothing is quiet.
    Open,
    /// The device is quiet.
    Quiet {
        /// What makes it quiet.
        reason: QuietReason,
        /// Local end of the quiet period, or `None` when it is unknown.
        ends_local: Option<NaiveDateTime>,
    },
}

/// Why quiet period preferences were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuietPeriodError {
    /// More than [`MAX_QUIET_WINDOWS`] windows.
    TooManyWindows,
    /// A window names no weekday.
    EmptyWeekdays,
    /// A window minute is past the end of the day.
    MinuteOutOfRange,
    /// A window starts and ends at the same minute.
    ZeroLengthWindow,
}

impl fmt::Display for QuietPeriodError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooManyWindows => "Too many quiet periods",
            Self::EmptyWeekdays => "Choose at least one weekday for each quiet period",
            Self::MinuteOutOfRange => "Quiet period times must be within the day",
            Self::ZeroLengthWindow => "A quiet period must end at a different time than it starts",
        })
    }
}

impl std::error::Error for QuietPeriodError {}

/// Checks quiet period preferences before they are saved.
///
/// # Errors
///
/// Returns the first bound the preferences break.
pub fn validate_quiet_preferences(
    preferences: &QuietPeriodPreferences,
) -> Result<(), QuietPeriodError> {
    if preferences.windows.len() > MAX_QUIET_WINDOWS {
        return Err(QuietPeriodError::TooManyWindows);
    }
    preferences.windows.iter().try_for_each(validate_window)
}

fn validate_window(window: &QuietWindow) -> Result<(), QuietPeriodError> {
    if window.weekdays.is_empty() {
        Err(QuietPeriodError::EmptyWeekdays)
    } else if window.start_minute > LAST_MINUTE_OF_DAY || window.end_minute > LAST_MINUTE_OF_DAY {
        Err(QuietPeriodError::MinuteOutOfRange)
    } else if window.start_minute == window.end_minute {
        Err(QuietPeriodError::ZeroLengthWindow)
    } else {
        Ok(())
    }
}

/// Returns whether the device is quiet at `local`, and until when.
///
/// Invalid windows are ignored, so stored preferences that bypassed validation never make the
/// device quiet by accident. When a window and a Focus phase both apply, the reason is
/// [`QuietReason::Window`] and the end is the later of the two, unknown when either is unknown.
pub fn evaluate_quiet_period(
    preferences: &QuietPeriodPreferences,
    local: NaiveDateTime,
    focus: FocusQuiet,
) -> QuietPeriodState {
    let window = window_quiet_end(preferences, local);
    match (window, focus) {
        (None, FocusQuiet::Inactive) => QuietPeriodState::Open,
        (None, FocusQuiet::Active { phase_ends_local }) => QuietPeriodState::Quiet {
            reason: QuietReason::Focus,
            ends_local: phase_ends_local,
        },
        (Some(window_end), FocusQuiet::Inactive) => QuietPeriodState::Quiet {
            reason: QuietReason::Window,
            ends_local: window_end,
        },
        (Some(window_end), FocusQuiet::Active { phase_ends_local }) => QuietPeriodState::Quiet {
            reason: QuietReason::Window,
            ends_local: window_end.zip(phase_ends_local).map(|(a, b)| a.max(b)),
        },
    }
}

/// Returns `None` when no window is active at `local`, otherwise the merged end, which is
/// itself `None` when the merged span never ends within the horizon.
fn window_quiet_end(
    preferences: &QuietPeriodPreferences,
    local: NaiveDateTime,
) -> Option<Option<NaiveDateTime>> {
    let date = local.date();
    let first_day = date.checked_sub_days(Days::new(1))?;
    let last_day = date.checked_add_days(Days::new(MERGE_HORIZON_DAYS))?;
    let mut spans = window_spans(preferences, first_day, last_day);
    spans.sort_unstable();
    let mut end = spans
        .iter()
        .filter(|(start, end)| *start <= local && local < *end)
        .map(|(_, end)| *end)
        .max()?;
    for (start, span_end) in &spans {
        if *start <= end && *span_end > end {
            end = *span_end;
        }
    }
    let horizon = last_day.and_time(NaiveTime::MIN);
    Some((end <= horizon).then_some(end))
}

/// Returns every span of a valid window starting on a date from `first_day` to `last_day`.
fn window_spans(
    preferences: &QuietPeriodPreferences,
    first_day: NaiveDate,
    last_day: NaiveDate,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let windows = preferences
        .windows
        .iter()
        .filter(|window| validate_window(window).is_ok())
        .collect::<Vec<_>>();
    first_day
        .iter_days()
        .take_while(|day| *day <= last_day)
        .flat_map(|day| {
            windows
                .iter()
                .filter(move |window| window.weekdays.contains(day.weekday()))
                .filter_map(move |window| window_span(window, day))
        })
        .collect()
}

fn window_span(window: &QuietWindow, day: NaiveDate) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let start = day.and_time(minute_time(window.start_minute)?);
    let end_day = if window.end_minute < window.start_minute {
        day.checked_add_days(Days::new(1))?
    } else {
        day
    };
    Some((start, end_day.and_time(minute_time(window.end_minute)?)))
}

fn minute_time(minute: u16) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(
        u32::from(minute / MINUTES_PER_HOUR),
        u32::from(minute % MINUTES_PER_HOUR),
        0,
    )
}

#[cfg(test)]
mod tests {
    use chrono::Weekday;

    use super::*;

    fn at(value: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M").unwrap()
    }

    fn window(weekdays: WeekdaySet, start: (u16, u16), end: (u16, u16)) -> QuietWindow {
        QuietWindow {
            weekdays,
            start_minute: start.0 * 60 + start.1,
            end_minute: end.0 * 60 + end.1,
        }
    }

    fn preferences(windows: Vec<QuietWindow>) -> QuietPeriodPreferences {
        QuietPeriodPreferences { windows }
    }

    fn quiet(reason: QuietReason, ends: Option<&str>) -> QuietPeriodState {
        QuietPeriodState::Quiet {
            reason,
            ends_local: ends.map(at),
        }
    }

    #[test]
    fn no_windows_and_no_focus_is_open() {
        assert_eq!(
            evaluate_quiet_period(
                &QuietPeriodPreferences::default(),
                at("2026-11-12 23:00"),
                FocusQuiet::Inactive
            ),
            QuietPeriodState::Open
        );
    }

    #[test]
    fn a_window_across_midnight_belongs_to_its_start_day() {
        // Thursday nights only: 22:00 to 07:00.
        let prefs = preferences(vec![window(
            WeekdaySet::from_days([Weekday::Thu]),
            (22, 0),
            (7, 0),
        )]);
        // 2026-11-12 is a Thursday.
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-12 23:00"), FocusQuiet::Inactive),
            quiet(QuietReason::Window, Some("2026-11-13 07:00"))
        );
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-13 06:59"), FocusQuiet::Inactive),
            quiet(QuietReason::Window, Some("2026-11-13 07:00"))
        );
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-13 07:00"), FocusQuiet::Inactive),
            QuietPeriodState::Open
        );
        // Wednesday night is not chosen, so Thursday morning is open.
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-12 03:00"), FocusQuiet::Inactive),
            QuietPeriodState::Open
        );
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-12 21:59"), FocusQuiet::Inactive),
            QuietPeriodState::Open
        );
    }

    #[test]
    fn adjacent_and_overlapping_windows_merge_into_one_end() {
        let prefs = preferences(vec![
            window(WeekdaySet::ALL, (22, 0), (7, 0)),
            window(WeekdaySet::ALL, (7, 0), (8, 30)),
            window(WeekdaySet::ALL, (8, 0), (9, 0)),
        ]);
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-12 23:00"), FocusQuiet::Inactive),
            quiet(QuietReason::Window, Some("2026-11-13 09:00"))
        );
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-13 09:00"), FocusQuiet::Inactive),
            QuietPeriodState::Open
        );
    }

    #[test]
    fn a_window_covering_the_whole_week_has_no_end() {
        let prefs = preferences(vec![
            window(WeekdaySet::ALL, (0, 0), (12, 0)),
            window(WeekdaySet::ALL, (12, 0), (0, 0)),
        ]);
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-12 10:00"), FocusQuiet::Inactive),
            quiet(QuietReason::Window, None)
        );
    }

    #[test]
    fn focus_phases_are_quiet_with_and_without_a_deadline() {
        let prefs = QuietPeriodPreferences::default();
        assert_eq!(
            evaluate_quiet_period(
                &prefs,
                at("2026-11-12 10:00"),
                FocusQuiet::Active {
                    phase_ends_local: Some(at("2026-11-12 10:25"))
                }
            ),
            quiet(QuietReason::Focus, Some("2026-11-12 10:25"))
        );
        assert_eq!(
            evaluate_quiet_period(
                &prefs,
                at("2026-11-12 10:00"),
                FocusQuiet::Active {
                    phase_ends_local: None
                }
            ),
            quiet(QuietReason::Focus, None)
        );
    }

    #[test]
    fn a_window_and_focus_together_report_the_window_and_the_later_end() {
        let prefs = preferences(vec![window(WeekdaySet::ALL, (9, 0), (10, 0))]);
        let local = at("2026-11-12 09:50");
        assert_eq!(
            evaluate_quiet_period(
                &prefs,
                local,
                FocusQuiet::Active {
                    phase_ends_local: Some(at("2026-11-12 10:15"))
                }
            ),
            quiet(QuietReason::Window, Some("2026-11-12 10:15"))
        );
        assert_eq!(
            evaluate_quiet_period(
                &prefs,
                local,
                FocusQuiet::Active {
                    phase_ends_local: Some(at("2026-11-12 09:55"))
                }
            ),
            quiet(QuietReason::Window, Some("2026-11-12 10:00"))
        );
        assert_eq!(
            evaluate_quiet_period(
                &prefs,
                local,
                FocusQuiet::Active {
                    phase_ends_local: None
                }
            ),
            quiet(QuietReason::Window, None)
        );
    }

    #[test]
    fn invalid_windows_never_make_the_device_quiet() {
        let prefs = preferences(vec![
            window(WeekdaySet::ALL, (9, 0), (9, 0)),
            window(WeekdaySet::default(), (8, 0), (11, 0)),
        ]);
        assert_eq!(
            evaluate_quiet_period(&prefs, at("2026-11-12 09:30"), FocusQuiet::Inactive),
            QuietPeriodState::Open
        );
    }

    #[test]
    fn invalid_preferences_are_refused() {
        let valid = window(WeekdaySet::ALL, (22, 0), (7, 0));
        assert_eq!(
            validate_quiet_preferences(&preferences(vec![valid])),
            Ok(())
        );
        assert_eq!(
            validate_quiet_preferences(&preferences(vec![valid; MAX_QUIET_WINDOWS])),
            Ok(())
        );
        assert_eq!(
            validate_quiet_preferences(&preferences(vec![valid; MAX_QUIET_WINDOWS + 1])),
            Err(QuietPeriodError::TooManyWindows)
        );
        assert_eq!(
            validate_quiet_preferences(&preferences(vec![window(
                WeekdaySet::default(),
                (22, 0),
                (7, 0)
            )])),
            Err(QuietPeriodError::EmptyWeekdays)
        );
        let mut late = valid;
        late.end_minute = 1440;
        assert_eq!(
            validate_quiet_preferences(&preferences(vec![late])),
            Err(QuietPeriodError::MinuteOutOfRange)
        );
        assert_eq!(
            validate_quiet_preferences(&preferences(vec![window(WeekdaySet::ALL, (7, 0), (7, 0))])),
            Err(QuietPeriodError::ZeroLengthWindow)
        );
    }

    #[test]
    fn preferences_round_trip_and_refuse_unknown_fields() {
        let prefs = preferences(vec![window(
            WeekdaySet::from_days([Weekday::Mon, Weekday::Fri]),
            (22, 0),
            (7, 0),
        )]);
        let value = serde_json::to_value(&prefs).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"windows": [{"weekdays": 17, "startMinute": 1320, "endMinute": 420}]})
        );
        assert_eq!(
            serde_json::from_value::<QuietPeriodPreferences>(value).unwrap(),
            prefs
        );
        assert!(
            serde_json::from_value::<QuietPeriodPreferences>(
                serde_json::json!({"windows": [], "extra": true})
            )
            .is_err()
        );
    }
}
