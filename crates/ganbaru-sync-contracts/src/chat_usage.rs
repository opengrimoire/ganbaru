//! Bounds and keys of Chat usage summaries.
//!
//! Sync admission and the Chat usage flush both read these, so a summary one replica writes is
//! a summary every replica accepts.

use crate::{DerivedKeyError, RowKey, derived_key, device_owner_tag};

/// Prefix of the derived part of a daily teammate usage key, over the device, teammate, and
/// local date.
pub const TEAMMATE_USAGE_KEY_PREFIX: &str = "chat-usage";
/// What a daily teammate usage key starts with: [`TEAMMATE_USAGE_KEY_PREFIX`] and its separator,
/// followed by the owner tag of the device, `:`, and the digest of the derived key.
pub const TEAMMATE_USAGE_OWNER_PREFIX: &str = "chat-usage:";
/// Characters of a local date in `YYYY-MM-DD` form.
pub const LOCAL_DATE_CHARS: usize = 10;
/// Counted tokens one device records for one teammate on one local day.
pub const MAX_DAILY_COUNTED_TOKENS: i64 = 1_000_000_000_000;
/// Working seconds one device records for one teammate on one local day: one hundred
/// concurrent full days.
pub const MAX_DAILY_WORKING_SECONDS: i64 = 8_640_000;
/// Runs one device records for one teammate on one local day.
pub const MAX_DAILY_RUNS: i64 = 100_000;

/// The replicated key of `device_id`'s usage of `teammate_id` on `local_date`: the owner tag of
/// the device, which lets every replica refuse a summary from another device, followed by the
/// digest derived from the three values.
pub fn teammate_usage_key(
    device_id: &str,
    teammate_id: &str,
    local_date: &str,
) -> Result<RowKey, DerivedKeyError> {
    let derived = derived_key(
        TEAMMATE_USAGE_KEY_PREFIX,
        &[device_id, teammate_id, local_date],
    )?;
    let digest = derived
        .as_str()
        .strip_prefix(TEAMMATE_USAGE_OWNER_PREFIX)
        .ok_or(DerivedKeyError::Prefix)?;
    RowKey::new(format!(
        "{TEAMMATE_USAGE_OWNER_PREFIX}{tag}:{digest}",
        tag = device_owner_tag(device_id)
    ))
    .map_err(|_| DerivedKeyError::Prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teammate_usage_key_names_the_device_owner_tag() {
        let key = teammate_usage_key("device-a", "teammate-1", "2026-11-12").expect("valid key");
        let tag = device_owner_tag("device-a");
        assert!(
            key.as_str()
                .starts_with(&format!("{TEAMMATE_USAGE_OWNER_PREFIX}{tag}:"))
        );
        assert_ne!(
            key,
            teammate_usage_key("device-b", "teammate-1", "2026-11-12").expect("valid key")
        );
        assert_ne!(
            key,
            teammate_usage_key("device-a", "teammate-1", "2026-11-13").expect("valid key")
        );
    }
}
