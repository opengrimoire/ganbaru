//! Pure rules for repeating scheduled messages and device quiet periods.
//!
//! Recurrences resolve wall-clock times through `ganbaru-civil-time`, so a fixed local time keeps
//! posting across daylight saving transitions. Quiet periods are evaluated against a local time
//! the caller captured once, together with the device's Focus state, and never read a clock.

use chrono::Weekday;
use serde::{Deserialize, Serialize};

pub mod quiet_periods;
pub mod recurrence;

pub use quiet_periods::{
    FocusQuiet, MAX_QUIET_WINDOWS, QuietPeriodError, QuietPeriodPreferences, QuietPeriodState,
    QuietReason, QuietWindow, evaluate_quiet_period, validate_quiet_preferences,
};
pub use recurrence::{
    MAX_RECURRENCE_INTERVAL_MINUTES, MAX_RECURRING_SCHEDULES_PER_CHANNEL,
    MIN_RECURRENCE_INTERVAL_MINUTES, MISSED_OCCURRENCE_GRACE_MINUTES, QuietPolicy, RecurrenceError,
    RecurrenceRule, RecurrenceSpec, next_occurrence, occurrence_key, validate_recurrence,
    validate_rule,
};

/// Last minute of a local day, counted from midnight.
pub const LAST_MINUTE_OF_DAY: u16 = 1439;

/// Mask with every weekday bit set.
const ALL_WEEKDAYS_MASK: u8 = 0b0111_1111;

/// A set of weekdays: bit 0 is Monday through bit 6 for Sunday.
///
/// The mask never carries bits above Sunday; an empty set is representable so validation can
/// name it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct WeekdaySet(u8);

impl WeekdaySet {
    /// Every day of the week.
    pub const ALL: Self = Self(ALL_WEEKDAYS_MASK);

    /// Returns the set for `mask`, or `None` when it sets a bit above Sunday.
    pub const fn from_mask(mask: u8) -> Option<Self> {
        if mask & !ALL_WEEKDAYS_MASK == 0 {
            Some(Self(mask))
        } else {
            None
        }
    }

    /// Returns the set of `days`.
    pub fn from_days(days: impl IntoIterator<Item = Weekday>) -> Self {
        Self(
            days.into_iter()
                .fold(0, |mask, day| mask | weekday_bit(day)),
        )
    }

    /// Returns the bit mask.
    pub const fn mask(self) -> u8 {
        self.0
    }

    /// Returns whether no weekday is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns whether `day` is in the set.
    pub const fn contains(self, day: Weekday) -> bool {
        self.0 & weekday_bit(day) != 0
    }
}

impl TryFrom<u8> for WeekdaySet {
    type Error = &'static str;

    fn try_from(mask: u8) -> Result<Self, Self::Error> {
        Self::from_mask(mask).ok_or("weekday mask sets a bit above Sunday")
    }
}

impl From<WeekdaySet> for u8 {
    fn from(set: WeekdaySet) -> Self {
        set.0
    }
}

const fn weekday_bit(day: Weekday) -> u8 {
    1 << day.num_days_from_monday()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_sets_use_monday_as_bit_zero_and_refuse_bits_after_sunday() {
        let set = WeekdaySet::from_days([Weekday::Mon, Weekday::Sun]);
        assert_eq!(set.mask(), 0b0100_0001);
        assert!(set.contains(Weekday::Mon));
        assert!(set.contains(Weekday::Sun));
        assert!(!set.contains(Weekday::Wed));
        assert_eq!(WeekdaySet::from_mask(0b1000_0000), None);
        assert!(WeekdaySet::from_mask(0).is_some_and(WeekdaySet::is_empty));
        assert!(serde_json::from_str::<WeekdaySet>("128").is_err());
        assert_eq!(serde_json::to_string(&WeekdaySet::ALL).unwrap(), "127");
    }
}
