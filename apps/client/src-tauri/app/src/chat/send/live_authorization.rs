//! Reuse of a passing live authorization check for streamed text deltas.

use ganbaru_chat_contracts::events::CanonicalEvent;
use ganbaru_chat_contracts::models::ChatTurnId;
use std::time::{Duration, Instant};

/// How long a passing live authorization check covers later text deltas of the same turn.
/// Reason: deltas are text the run already had authority to produce, and half a second bounds
/// the window an authority change that no local path reports can stay unnoticed.
pub(super) const LIVE_AUTHORIZATION_RECHECK_INTERVAL: Duration = Duration::from_millis(500);

/// One passing check: the authority epoch read before it started, its turn, and when it started.
#[derive(Clone, Debug)]
struct PassingCheck {
    epoch: u64,
    turn_id: ChatTurnId,
    started_at: Instant,
}

/// The last passing live authorization check of one event sink.
#[derive(Debug, Default)]
pub(super) struct LiveAuthorizationCache {
    last: Option<PassingCheck>,
}

impl LiveAuthorizationCache {
    /// Whether `event` of `turn_id` may skip the live check. Only plain text and proposed plan
    /// deltas qualify, and only within the recheck interval of a check that passed for the same
    /// turn at the same epoch. Without an epoch nothing is reused.
    pub(super) fn may_reuse(
        &self,
        epoch: Option<u64>,
        turn_id: Option<&ChatTurnId>,
        event: &CanonicalEvent,
        now: Instant,
    ) -> bool {
        let (Some(epoch), Some(turn_id), Some(last)) = (epoch, turn_id, self.last.as_ref()) else {
            return false;
        };
        is_reusable_event(event)
            && last.epoch == epoch
            && &last.turn_id == turn_id
            && now.saturating_duration_since(last.started_at) < LIVE_AUTHORIZATION_RECHECK_INTERVAL
    }

    /// Records a check that passed for `turn_id`, with the epoch read and the time taken before
    /// it started, so an authority change during the check forces the next one.
    pub(super) fn record_pass(
        &mut self,
        epoch: Option<u64>,
        turn_id: Option<&ChatTurnId>,
        started_at: Instant,
    ) {
        self.last = match (epoch, turn_id) {
            (Some(epoch), Some(turn_id)) => Some(PassingCheck {
                epoch,
                turn_id: turn_id.clone(),
                started_at,
            }),
            _ => None,
        };
    }

    /// Forgets the last passing check, after a failed check or a settled turn.
    pub(super) fn clear(&mut self) {
        self.last = None;
    }
}

fn is_reusable_event(event: &CanonicalEvent) -> bool {
    matches!(
        event,
        CanonicalEvent::ContentDelta(_) | CanonicalEvent::ProposedPlanDelta(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_chat_contracts::events::{ContentDeltaEvent, TurnAbortedEvent};
    use ganbaru_chat_contracts::models::{ChatTurnState, ContentStreamKind};

    fn turn(value: &str) -> ChatTurnId {
        ChatTurnId::new(value).unwrap()
    }

    fn delta() -> CanonicalEvent {
        CanonicalEvent::ContentDelta(ContentDeltaEvent {
            item_id: "item:1".to_string(),
            stream_kind: ContentStreamKind::AssistantText,
            content_index: 0,
            delta: "Hello".to_string(),
        })
    }

    fn passed_at(epoch: u64, turn_id: &ChatTurnId, started_at: Instant) -> LiveAuthorizationCache {
        let mut cache = LiveAuthorizationCache::default();
        cache.record_pass(Some(epoch), Some(turn_id), started_at);
        cache
    }

    #[test]
    fn live_authorization_reuses_a_recent_pass_for_a_delta_of_the_same_turn() {
        let start = Instant::now();
        let turn_id = turn("turn:1");
        let cache = passed_at(4, &turn_id, start);
        let now = start + Duration::from_millis(499);
        assert!(cache.may_reuse(Some(4), Some(&turn_id), &delta(), now));
    }

    #[test]
    fn live_authorization_rechecks_once_the_interval_elapses() {
        let start = Instant::now();
        let turn_id = turn("turn:1");
        let cache = passed_at(4, &turn_id, start);
        let now = start + LIVE_AUTHORIZATION_RECHECK_INTERVAL;
        assert!(!cache.may_reuse(Some(4), Some(&turn_id), &delta(), now));
    }

    #[test]
    fn live_authorization_rechecks_after_an_epoch_bump() {
        let start = Instant::now();
        let turn_id = turn("turn:1");
        let cache = passed_at(4, &turn_id, start);
        assert!(!cache.may_reuse(Some(5), Some(&turn_id), &delta(), start));
    }

    #[test]
    fn live_authorization_never_covers_another_turn() {
        let start = Instant::now();
        let cache = passed_at(4, &turn("turn:1"), start);
        assert!(!cache.may_reuse(Some(4), Some(&turn("turn:2")), &delta(), start));
        assert!(!cache.may_reuse(Some(4), None, &delta(), start));
    }

    #[test]
    fn live_authorization_always_checks_events_other_than_text_deltas() {
        let start = Instant::now();
        let turn_id = turn("turn:1");
        let cache = passed_at(4, &turn_id, start);
        let aborted = CanonicalEvent::TurnAborted(TurnAbortedEvent {
            state: ChatTurnState::Interrupted,
            reason: "Stopped".to_string(),
            recoverable: true,
        });
        assert!(!cache.may_reuse(Some(4), Some(&turn_id), &aborted, start));
    }

    #[test]
    fn live_authorization_reuses_nothing_without_an_epoch_or_after_clearing() {
        let start = Instant::now();
        let turn_id = turn("turn:1");
        let mut unmanaged = LiveAuthorizationCache::default();
        unmanaged.record_pass(None, Some(&turn_id), start);
        assert!(!unmanaged.may_reuse(None, Some(&turn_id), &delta(), start));
        assert!(!unmanaged.may_reuse(Some(0), Some(&turn_id), &delta(), start));
        let mut cleared = passed_at(4, &turn_id, start);
        cleared.clear();
        assert!(!cleared.may_reuse(Some(4), Some(&turn_id), &delta(), start));
    }
}
