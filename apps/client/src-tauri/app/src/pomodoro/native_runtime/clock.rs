use std::time::{Duration, Instant};

use ganbaru_pomodoro::{
    FOCUS_IDLE_FAILURE_GRACE_MS, FocusExecutionSnapshot, FocusMode, FocusObservation,
};

const SUSPEND_THRESHOLD_MS: i64 = 15_000;

#[derive(Clone, Copy, Eq, PartialEq)]
struct IdleEpisode {
    generation: u64,
    detected_at_ms: i64,
    visible_at_ms: i64,
}

/// A live warning's grace clock cannot be shortened or extended by civil clock edits.
#[derive(Default)]
pub(super) struct IdleGraceClock {
    active: Option<(IdleEpisode, Instant)>,
}

impl IdleGraceClock {
    pub fn update(
        &mut self,
        generation: u64,
        snapshot: Option<&FocusExecutionSnapshot>,
        now: Instant,
    ) {
        let episode = snapshot.and_then(|snapshot| Self::episode(generation, snapshot));
        if episode != self.active.as_ref().map(|(episode, _)| *episode) {
            self.active = episode.map(|episode| (episode, now));
        }
    }

    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.active.map(|(_, started)| {
            Duration::from_millis(FOCUS_IDLE_FAILURE_GRACE_MS)
                .saturating_sub(now.saturating_duration_since(started))
        })
    }

    pub fn elapsed_observation(
        &self,
        generation: u64,
        snapshot: &FocusExecutionSnapshot,
        now: Instant,
    ) -> Option<FocusObservation> {
        let (episode, started) = self.active?;
        if Self::episode(generation, snapshot) != Some(episode)
            || self.remaining(now)? != Duration::ZERO
        {
            return None;
        }
        Some(FocusObservation::IdleGraceElapsed {
            run_id: snapshot.run.as_ref()?.id.clone(),
            segment_id: snapshot.segment.as_ref()?.id.clone(),
            visible_at_ms: episode.visible_at_ms,
            elapsed_ms: now
                .saturating_duration_since(started)
                .as_millis()
                .min(u64::MAX as u128) as u64,
        })
    }

    fn episode(generation: u64, snapshot: &FocusExecutionSnapshot) -> Option<IdleEpisode> {
        if snapshot.mode != FocusMode::IdlePause {
            return None;
        }
        Some(IdleEpisode {
            generation,
            detected_at_ms: snapshot.idle_detected_at_ms?,
            visible_at_ms: snapshot.idle_overlay_visible_at_ms?,
        })
    }
}

/// New work may advance a scheduled wake, but unrelated traffic cannot postpone it.
pub(super) fn earlier_wake(current: Instant, now: Instant, delay: std::time::Duration) -> Instant {
    current.min(now + delay)
}

pub(super) struct ClockObservation {
    pub wall_ms: i64,
    pub monotonic: Instant,
}

/// Both a delayed native owner and a wall-clock discontinuity pause conservatively.
pub(super) fn discontinuity(
    previous: &ClockObservation,
    current: &ClockObservation,
) -> Option<(i64, i64)> {
    let wall_gap = current.wall_ms.saturating_sub(previous.wall_ms);
    let monotonic_gap = current
        .monotonic
        .saturating_duration_since(previous.monotonic)
        .as_millis();
    if !(0..=SUSPEND_THRESHOLD_MS).contains(&wall_gap)
        || monotonic_gap > SUSPEND_THRESHOLD_MS as u128
    {
        Some((previous.wall_ms, current.wall_ms.max(previous.wall_ms)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
