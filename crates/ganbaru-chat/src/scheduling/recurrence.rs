//! Recurrence rules of repeating scheduled messages.
//!
//! An interval rule counts whole intervals from the series anchor in UTC, with no zone involved.
//! A fixed time rule posts at one local minute on chosen weekdays in an IANA zone. A wall time
//! inside a spring-forward gap resolves with the offset before the transition, so a daily 02:30
//! message still posts on that day (at 03:30) instead of disappearing, and a fall-back fold
//! posts once, at its earlier instant.

use std::fmt;

use chrono::{Datelike, Days, NaiveTime};
use serde::{Deserialize, Serialize};

use super::{LAST_MINUTE_OF_DAY, WeekdaySet};
use crate::models::ChatError;

/// Shortest interval between occurrences of an interval rule.
pub const MIN_RECURRENCE_INTERVAL_MINUTES: u32 = 60;
/// Longest interval between occurrences of an interval rule (30 days).
pub const MAX_RECURRENCE_INTERVAL_MINUTES: u32 = 43_200;
/// How late an occurrence may still post; an occurrence missed by more is skipped and counted.
pub const MISSED_OCCURRENCE_GRACE_MINUTES: i64 = 10;
/// Most repeating scheduled messages one channel may hold.
pub const MAX_RECURRING_SCHEDULES_PER_CHANNEL: u32 = 20;

/// Field name that recurrence validation errors carry.
const RECURRENCE_FIELD: &str = "recurrence";
/// Local dates a fixed time rule inspects: one full week plus the starting date, so a candidate
/// on the starting date that is already past still finds the same weekday a week later.
const FIXED_TIME_SEARCH_DAYS: u64 = 8;
const MILLIS_PER_MINUTE: i64 = 60_000;
const MINUTES_PER_HOUR: u16 = 60;

/// When a repeating message posts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum RecurrenceRule {
    /// Every `minutes` from the series anchor.
    Interval {
        /// Minutes between occurrences.
        minutes: u32,
    },
    /// At one local minute on each day in `weekdays`.
    FixedTime {
        /// Minute of the local day, from 0 (midnight) to 1439.
        local_minute: u16,
        /// Days the message posts.
        weekdays: WeekdaySet,
    },
}

impl RecurrenceRule {
    /// Returns the stored rule kind: `interval` or `fixed_time`.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Interval { .. } => "interval",
            Self::FixedTime { .. } => "fixed_time",
        }
    }
}

/// What a repeating message does when its occurrence falls in a quiet period.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuietPolicy {
    /// Wait until the quiet period ends, or skip the occurrence when the next one comes first.
    #[default]
    Defer,
    /// Post at the occurrence time regardless of quiet periods.
    Ignore,
}

impl QuietPolicy {
    /// Returns the stored policy name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Defer => "defer",
            Self::Ignore => "ignore",
        }
    }

    /// Parses a stored policy name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "defer" => Some(Self::Defer),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }
}

/// A recurrence rule with the zone its local times use and its quiet period policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceSpec {
    /// When the message posts.
    pub rule: RecurrenceRule,
    /// IANA zone of fixed local times, and of the times shown for the series.
    pub time_zone: String,
    /// Quiet period behavior.
    pub quiet_policy: QuietPolicy,
}

/// Why a recurrence was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecurrenceError {
    /// The interval is shorter than [`MIN_RECURRENCE_INTERVAL_MINUTES`].
    IntervalTooShort,
    /// The interval is longer than [`MAX_RECURRENCE_INTERVAL_MINUTES`].
    IntervalTooLong,
    /// A fixed time rule names no weekday.
    EmptyWeekdays,
    /// A fixed time rule's minute is past the end of the day.
    MinuteOutOfRange,
    /// The zone is not a known IANA zone on this device.
    UnknownTimeZone,
    /// No occurrence could be computed in the supported time range.
    Unresolvable,
}

impl fmt::Display for RecurrenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::IntervalTooShort => "Repeat interval must be at least one hour",
            Self::IntervalTooLong => "Repeat interval must be at most 30 days",
            Self::EmptyWeekdays => "Choose at least one weekday",
            Self::MinuteOutOfRange => "Repeat time must be within the day",
            Self::UnknownTimeZone => "Repeat time zone is not recognized",
            Self::Unresolvable => "The next repeat time cannot be computed",
        })
    }
}

impl std::error::Error for RecurrenceError {}

impl From<RecurrenceError> for ChatError {
    fn from(error: RecurrenceError) -> Self {
        ChatError::validation(RECURRENCE_FIELD, error.to_string())
    }
}

/// Checks a rule's shape without resolving its zone.
///
/// # Errors
///
/// Returns the first bound the rule breaks.
pub fn validate_rule(rule: &RecurrenceRule) -> Result<(), RecurrenceError> {
    match *rule {
        RecurrenceRule::Interval { minutes } if minutes < MIN_RECURRENCE_INTERVAL_MINUTES => {
            Err(RecurrenceError::IntervalTooShort)
        }
        RecurrenceRule::Interval { minutes } if minutes > MAX_RECURRENCE_INTERVAL_MINUTES => {
            Err(RecurrenceError::IntervalTooLong)
        }
        RecurrenceRule::Interval { .. } => Ok(()),
        RecurrenceRule::FixedTime { local_minute, .. } if local_minute > LAST_MINUTE_OF_DAY => {
            Err(RecurrenceError::MinuteOutOfRange)
        }
        RecurrenceRule::FixedTime { weekdays, .. } if weekdays.is_empty() => {
            Err(RecurrenceError::EmptyWeekdays)
        }
        RecurrenceRule::FixedTime { .. } => Ok(()),
    }
}

/// Checks a recurrence, including that its zone resolves on this device.
///
/// # Errors
///
/// Returns the first bound the rule breaks, or [`RecurrenceError::UnknownTimeZone`].
pub fn validate_recurrence(spec: &RecurrenceSpec) -> Result<(), RecurrenceError> {
    validate_rule(&spec.rule)?;
    resolve_zone(&spec.time_zone).map(|_| ())
}

/// Returns the first occurrence strictly after `after_ms` and not before `anchor_ms`.
///
/// For an interval rule it is the first `anchor_ms + k * interval` (k at least 0) after
/// `after_ms`. For a fixed time rule it is the first local minute on a chosen weekday, resolved
/// in the rule's zone, after both `after_ms` and the instant before `anchor_ms`, so a series
/// whose anchor is itself an occurrence starts there.
///
/// # Errors
///
/// Returns a validation error for an invalid rule or zone, or
/// [`RecurrenceError::Unresolvable`] when the result leaves the supported time range.
pub fn next_occurrence(
    spec: &RecurrenceSpec,
    anchor_ms: i64,
    after_ms: i64,
) -> Result<i64, RecurrenceError> {
    validate_rule(&spec.rule)?;
    let zone = resolve_zone(&spec.time_zone)?;
    match spec.rule {
        RecurrenceRule::Interval { minutes } => next_interval(minutes, anchor_ms, after_ms),
        RecurrenceRule::FixedTime {
            local_minute,
            weekdays,
        } => next_fixed_time(
            local_minute,
            weekdays,
            &zone,
            after_ms.max(anchor_ms.saturating_sub(1)),
        ),
    }
}

/// Returns the stable key of one occurrence of a series, used in the ids its post derives.
pub fn occurrence_key(scheduled_message_id: &str, occurrence_ms: i64) -> String {
    format!("{scheduled_message_id}@{occurrence_ms}")
}

fn resolve_zone(name: &str) -> Result<jiff::tz::TimeZone, RecurrenceError> {
    ganbaru_civil_time::zone(name).map_err(|_| RecurrenceError::UnknownTimeZone)
}

fn next_interval(minutes: u32, anchor_ms: i64, after_ms: i64) -> Result<i64, RecurrenceError> {
    if after_ms < anchor_ms {
        return Ok(anchor_ms);
    }
    let step = i64::from(minutes) * MILLIS_PER_MINUTE;
    let elapsed_steps = after_ms
        .checked_sub(anchor_ms)
        .map(|elapsed| elapsed / step + 1)
        .ok_or(RecurrenceError::Unresolvable)?;
    elapsed_steps
        .checked_mul(step)
        .and_then(|offset| anchor_ms.checked_add(offset))
        .ok_or(RecurrenceError::Unresolvable)
}

fn next_fixed_time(
    local_minute: u16,
    weekdays: WeekdaySet,
    zone: &jiff::tz::TimeZone,
    after_ms: i64,
) -> Result<i64, RecurrenceError> {
    let time = NaiveTime::from_hms_opt(
        u32::from(local_minute / MINUTES_PER_HOUR),
        u32::from(local_minute % MINUTES_PER_HOUR),
        0,
    )
    .ok_or(RecurrenceError::MinuteOutOfRange)?;
    let start = ganbaru_civil_time::instant_to_local(after_ms, zone)
        .map_err(|_| RecurrenceError::Unresolvable)?
        .date();
    for offset in 0..FIXED_TIME_SEARCH_DAYS {
        let date = start
            .checked_add_days(Days::new(offset))
            .ok_or(RecurrenceError::Unresolvable)?;
        if !weekdays.contains(date.weekday()) {
            continue;
        }
        let candidate = ganbaru_civil_time::explicit_instant(date.and_time(time), zone)
            .map_err(|_| RecurrenceError::Unresolvable)?;
        if candidate > after_ms {
            return Ok(candidate);
        }
    }
    Err(RecurrenceError::Unresolvable)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Weekday};

    use super::*;

    fn instant(value: &str) -> i64 {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis()
    }

    fn interval(minutes: u32) -> RecurrenceSpec {
        RecurrenceSpec {
            rule: RecurrenceRule::Interval { minutes },
            time_zone: "UTC".into(),
            quiet_policy: QuietPolicy::Defer,
        }
    }

    fn fixed(zone: &str, hour: u16, minute: u16, weekdays: WeekdaySet) -> RecurrenceSpec {
        RecurrenceSpec {
            rule: RecurrenceRule::FixedTime {
                local_minute: hour * 60 + minute,
                weekdays,
            },
            time_zone: zone.into(),
            quiet_policy: QuietPolicy::Defer,
        }
    }

    fn daily(zone: &str, hour: u16, minute: u16) -> RecurrenceSpec {
        fixed(zone, hour, minute, WeekdaySet::ALL)
    }

    #[test]
    fn interval_bounds_are_refused_outside_one_hour_to_thirty_days() {
        assert_eq!(
            next_occurrence(&interval(59), 0, 0),
            Err(RecurrenceError::IntervalTooShort)
        );
        assert_eq!(
            next_occurrence(&interval(43_201), 0, 0),
            Err(RecurrenceError::IntervalTooLong)
        );
        assert!(next_occurrence(&interval(60), 0, 0).is_ok());
        assert!(next_occurrence(&interval(43_200), 0, 0).is_ok());
    }

    #[test]
    fn interval_occurrences_count_from_the_anchor_strictly_after_the_bound() {
        let anchor = instant("2026-11-12T10:15:00Z");
        let spec = interval(90);
        assert_eq!(next_occurrence(&spec, anchor, anchor - 1), Ok(anchor));
        assert_eq!(
            next_occurrence(&spec, anchor, anchor),
            Ok(instant("2026-11-12T11:45:00Z"))
        );
        assert_eq!(
            next_occurrence(&spec, anchor, instant("2026-11-12T13:14:59Z")),
            Ok(instant("2026-11-12T13:15:00Z"))
        );
        assert_eq!(
            next_occurrence(&spec, anchor, instant("2026-11-12T13:15:00Z")),
            Ok(instant("2026-11-12T14:45:00Z"))
        );
        assert_eq!(
            next_occurrence(&spec, anchor, instant("2020-01-01T00:00:00Z")),
            Ok(anchor)
        );
    }

    #[test]
    fn interval_occurrences_ignore_daylight_saving_transitions() {
        let mut spec = interval(60);
        spec.time_zone = "Europe/Madrid".into();
        let before = instant("2026-03-29T00:30:00Z");
        assert_eq!(
            next_occurrence(&spec, before, before),
            Ok(instant("2026-03-29T01:30:00Z"))
        );
    }

    #[test]
    fn interval_overflow_is_unresolvable() {
        assert_eq!(
            next_occurrence(&interval(60), i64::MIN, i64::MAX),
            Err(RecurrenceError::Unresolvable)
        );
    }

    #[test]
    fn fixed_time_bounds_and_unknown_zones_are_refused() {
        assert_eq!(
            next_occurrence(&fixed("UTC", 9, 0, WeekdaySet::default()), 0, 0),
            Err(RecurrenceError::EmptyWeekdays)
        );
        let mut late = daily("UTC", 0, 0);
        late.rule = RecurrenceRule::FixedTime {
            local_minute: 1440,
            weekdays: WeekdaySet::ALL,
        };
        assert_eq!(
            next_occurrence(&late, 0, 0),
            Err(RecurrenceError::MinuteOutOfRange)
        );
        assert_eq!(
            next_occurrence(&daily("Mars/Olympus_Mons", 9, 0), 0, 0),
            Err(RecurrenceError::UnknownTimeZone)
        );
        assert_eq!(
            validate_recurrence(&daily("", 9, 0)),
            Err(RecurrenceError::UnknownTimeZone)
        );
        assert_eq!(validate_recurrence(&daily("Europe/Madrid", 23, 59)), Ok(()));
    }

    #[test]
    fn fixed_time_posts_later_the_same_day_or_on_the_next_chosen_weekday() {
        let spec = daily("Europe/Madrid", 9, 0);
        let anchor = instant("2026-11-12T00:00:00Z");
        assert_eq!(
            next_occurrence(&spec, anchor, instant("2026-11-12T07:59:59Z")),
            Ok(instant("2026-11-12T08:00:00Z"))
        );
        assert_eq!(
            next_occurrence(&spec, anchor, instant("2026-11-12T08:00:00Z")),
            Ok(instant("2026-11-13T08:00:00Z"))
        );
    }

    #[test]
    fn fixed_time_weekday_masks_skip_excluded_days() {
        let weekdays = WeekdaySet::from_days([Weekday::Mon, Weekday::Wed, Weekday::Fri]);
        let spec = fixed("Europe/Madrid", 9, 0, weekdays);
        // 2026-11-13 is a Friday; Saturday through Sunday are excluded.
        let friday_after = instant("2026-11-13T08:00:00Z");
        assert_eq!(
            next_occurrence(&spec, 0, friday_after),
            Ok(instant("2026-11-16T08:00:00Z"))
        );
        let only_tuesday = fixed("UTC", 9, 0, WeekdaySet::from_days([Weekday::Tue]));
        // From Tuesday after 09:00 the next one is a full week later.
        assert_eq!(
            next_occurrence(&only_tuesday, 0, instant("2026-11-10T09:00:00Z")),
            Ok(instant("2026-11-17T09:00:00Z"))
        );
    }

    #[test]
    fn fixed_time_starts_at_the_anchor_when_it_is_later_than_the_bound() {
        let spec = daily("UTC", 9, 0);
        let anchor = instant("2026-12-01T09:00:00Z");
        assert_eq!(next_occurrence(&spec, anchor, 0), Ok(anchor));
        assert_eq!(
            next_occurrence(&spec, anchor, anchor),
            Ok(instant("2026-12-02T09:00:00Z"))
        );
    }

    #[test]
    fn madrid_spring_forward_posts_a_gap_time_once_at_the_shifted_wall_time() {
        // 2026-03-29 02:00 CET jumps to 03:00 CEST; 02:30 does not exist that day.
        let spec = daily("Europe/Madrid", 2, 30);
        let saturday = instant("2026-03-28T01:30:00Z");
        let sunday = next_occurrence(&spec, 0, saturday).unwrap();
        // 02:30 at the pre-transition offset (+01:00) is 03:30 CEST.
        assert_eq!(sunday, instant("2026-03-29T01:30:00Z"));
        assert_eq!(
            next_occurrence(&spec, 0, sunday),
            Ok(instant("2026-03-30T00:30:00Z"))
        );
    }

    #[test]
    fn madrid_fall_back_fold_posts_once_at_the_earlier_instant() {
        // 2026-10-25 03:00 CEST falls back to 02:00 CET; 02:30 happens twice.
        let spec = daily("Europe/Madrid", 2, 30);
        let first = next_occurrence(&spec, 0, instant("2026-10-24T12:00:00Z")).unwrap();
        assert_eq!(first, instant("2026-10-25T00:30:00Z"));
        // The repeated 02:30 one hour later is not a second occurrence.
        assert_eq!(
            next_occurrence(&spec, 0, first),
            Ok(instant("2026-10-26T01:30:00Z"))
        );
    }

    #[test]
    fn santiago_midnight_gap_shifts_the_occurrence_into_the_new_offset() {
        // 2026-09-06 00:00 at -04:00 jumps to 01:00 at -03:00; 00:30 does not exist.
        let spec = daily("America/Santiago", 0, 30);
        let gap_day = next_occurrence(&spec, 0, instant("2026-09-05T12:00:00Z")).unwrap();
        assert_eq!(gap_day, instant("2026-09-06T04:30:00Z"));
        assert_eq!(
            next_occurrence(&spec, 0, gap_day),
            Ok(instant("2026-09-07T03:30:00Z"))
        );
    }

    #[test]
    fn santiago_midnight_fold_posts_once() {
        // 2026-04-05 00:00 at -03:00 falls back to 2026-04-04 23:00 at -04:00; 23:30 repeats.
        let spec = daily("America/Santiago", 23, 30);
        let first = next_occurrence(&spec, 0, instant("2026-04-04T12:00:00Z")).unwrap();
        assert_eq!(first, instant("2026-04-05T02:30:00Z"));
        assert_eq!(
            next_occurrence(&spec, 0, first),
            Ok(instant("2026-04-06T03:30:00Z"))
        );
    }

    #[test]
    fn lord_howe_half_hour_gap_and_fold_follow_the_actual_shift() {
        // 2026-10-04 02:00 at +10:30 jumps to 02:30 at +11:00; 02:15 does not exist.
        let spring = daily("Australia/Lord_Howe", 2, 15);
        let gap_day = next_occurrence(&spring, 0, instant("2026-10-03T00:00:00Z")).unwrap();
        // 02:15 at +10:30 is 02:45 at +11:00.
        assert_eq!(gap_day, instant("2026-10-03T15:45:00Z"));
        assert_eq!(
            next_occurrence(&spring, 0, gap_day),
            Ok(instant("2026-10-04T15:15:00Z"))
        );
        // 2026-04-05 02:00 at +11:00 falls back to 01:30 at +10:30; 01:45 repeats.
        let autumn = daily("Australia/Lord_Howe", 1, 45);
        let first = next_occurrence(&autumn, 0, instant("2026-04-04T00:00:00Z")).unwrap();
        assert_eq!(first, instant("2026-04-04T14:45:00Z"));
        assert_eq!(
            next_occurrence(&autumn, 0, first),
            Ok(instant("2026-04-05T15:15:00Z"))
        );
    }

    #[test]
    fn occurrence_keys_differ_per_occurrence_and_are_stable() {
        assert_eq!(occurrence_key("scheduled:a", 1_000), "scheduled:a@1000");
        assert_ne!(
            occurrence_key("scheduled:a", 1_000),
            occurrence_key("scheduled:a", 2_000)
        );
        assert_ne!(
            occurrence_key("scheduled:a", 1_000),
            occurrence_key("scheduled:b", 1_000)
        );
    }

    #[test]
    fn errors_map_to_recurrence_validation_errors() {
        let error = ChatError::from(RecurrenceError::IntervalTooShort);
        assert_eq!(error.field.as_deref(), Some("recurrence"));
    }

    #[test]
    fn specs_serialize_with_a_kind_tag_and_policy_names() {
        let spec = fixed("Europe/Madrid", 9, 0, WeekdaySet::from_days([Weekday::Mon]));
        let value = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "rule": {"kind": "fixed_time", "localMinute": 540, "weekdays": 1},
                "timeZone": "Europe/Madrid",
                "quietPolicy": "defer",
            })
        );
        assert_eq!(QuietPolicy::parse("ignore"), Some(QuietPolicy::Ignore));
        assert_eq!(QuietPolicy::parse("later"), None);
        assert_eq!(spec.rule.kind(), "fixed_time");
    }
}
