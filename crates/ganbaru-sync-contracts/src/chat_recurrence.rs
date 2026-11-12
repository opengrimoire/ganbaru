//! Bounds of a repeating Chat scheduled message.
//!
//! Sync admission and the Chat recurrence rules both read these, so a rule one replica accepts
//! is a rule every replica can apply.

/// Shortest interval between occurrences of an interval rule, in minutes.
pub const MIN_INTERVAL_MINUTES: u32 = 60;
/// Longest interval between occurrences of an interval rule, in minutes (30 days).
pub const MAX_INTERVAL_MINUTES: u32 = 43_200;
/// Last minute of a local day a fixed-time rule may name, counted from midnight.
pub const LAST_LOCAL_MINUTE: u16 = 1439;
/// Weekday mask with every day set: bit 0 is Monday through bit 6 for Sunday.
pub const ALL_WEEKDAYS_MASK: u8 = 0b0111_1111;
/// Longest IANA time zone name a rule stores, in characters.
pub const MAX_TIME_ZONE_CHARS: usize = 255;
/// Rule kind of a rule that repeats every fixed number of minutes.
pub const INTERVAL_RULE: &str = "interval";
/// Rule kind of a rule that repeats at a local time on chosen weekdays.
pub const FIXED_TIME_RULE: &str = "fixed_time";
/// Rule kinds.
pub const RULE_KINDS: [&str; 2] = [INTERVAL_RULE, FIXED_TIME_RULE];
/// Quiet policy that moves an occurrence to the end of a quiet period.
pub const DEFER_QUIET_POLICY: &str = "defer";
/// Quiet policy that posts during quiet periods.
pub const IGNORE_QUIET_POLICY: &str = "ignore";
/// Quiet policies.
pub const QUIET_POLICIES: [&str; 2] = [DEFER_QUIET_POLICY, IGNORE_QUIET_POLICY];
/// Derived key prefix of a recurrence row, over its scheduled message id.
pub const RECURRENCE_KEY_PREFIX: &str = "scheduled-recurrence";
