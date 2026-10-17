//! Admission evidence is separate from Calendar eligibility and elapsed timer time.

use std::time::Duration;

/// Maximum age of local input evidence accepted for an automatic desktop start.
pub const AUTOMATIC_START_ACTIVITY_MAX_AGE: Duration = Duration::from_secs(15);

/// Decide whether a fresh local activity observation can admit an eligible commitment.
///
/// The caller obtains the observation locally for this request. Missing observations,
/// input predating the scheduling boundary, and future boundaries cannot authorize a run.
/// Phone usage and webcam suppression are not input evidence for desktop execution.
pub fn automatic_start_allowed(
    boundary_epoch_ms: i64,
    observed_at_epoch_ms: i64,
    idle_ms: Option<u64>,
) -> bool {
    let Some(idle_ms) = idle_ms.and_then(|value| i64::try_from(value).ok()) else {
        return false;
    };
    let Some(last_input_ms) = observed_at_epoch_ms.checked_sub(idle_ms) else {
        return false;
    };
    boundary_epoch_ms > 0
        && last_input_ms >= boundary_epoch_ms
        && u128::try_from(idle_ms)
            .is_ok_and(|age| age <= AUTOMATIC_START_ACTIVITY_MAX_AGE.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schedule_or_unavailable_observation_does_not_admit_focus() {
        assert!(!automatic_start_allowed(100_000, 110_000, None));
        assert!(!automatic_start_allowed(100_000, 110_000, Some(11_000)));
        assert!(!automatic_start_allowed(120_000, 110_000, Some(0)));
        assert!(!automatic_start_allowed(100_000, 200_000, Some(16_000)));
        assert!(!automatic_start_allowed(100_000, 200_000, Some(u64::MAX)));
    }

    #[test]
    fn late_arrival_requires_fresh_input_after_the_boundary() {
        assert!(automatic_start_allowed(100_000, 500_000, Some(100)));
        assert!(automatic_start_allowed(100_000, 115_000, Some(15_000)));
    }
}
