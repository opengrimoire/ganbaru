//! Bounded memory of provider ids that were already projected.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};

/// Root message ids whose text one turn can remember before the oldest is forgotten.
const MAX_TRACKED_TURN_MESSAGES: usize = 256;
/// ExitPlanMode tool use ids a session remembers before the oldest is forgotten.
const MAX_TRACKED_PLAN_IDS: usize = 64;

/// Insertion-ordered set that forgets its oldest member once it holds `capacity` ids.
#[derive(Clone, Debug)]
pub(in crate::claude) struct BoundedIdSet {
    order: VecDeque<String>,
    members: HashSet<String>,
    capacity: usize,
}

impl BoundedIdSet {
    fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::new(),
            members: HashSet::new(),
            capacity,
        }
    }

    /// Adds `id` and reports whether it was absent.
    pub(super) fn insert(&mut self, id: &str) -> bool {
        if self.members.contains(id) {
            return false;
        }
        if self.order.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.members.remove(&oldest);
            }
        }
        self.order.push_back(id.to_string());
        self.members.insert(id.to_string());
        true
    }

    /// Removes `id` and reports whether it was present.
    fn remove(&mut self, id: &str) -> bool {
        if !self.members.remove(id) {
            return false;
        }
        self.order.retain(|member| member != id);
        true
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.members.contains(id)
    }

    fn clear(&mut self) {
        self.order.clear();
        self.members.clear();
    }
}

/// Assistant text the current turn already projected, keyed by API message id.
///
/// The CLI sends one `assistant` frame per content block, repeating the message id, after the
/// same text already streamed through partial messages. Synthetic notices and CLI versions that
/// skip partial messages only arrive as `assistant` frames.
#[derive(Clone, Debug)]
pub(in crate::claude) struct ClaudeTurnText {
    streamed: BoundedIdSet,
    unstreamed_blocks: HashMap<String, u32>,
}

impl Default for ClaudeTurnText {
    fn default() -> Self {
        Self {
            streamed: BoundedIdSet::new(MAX_TRACKED_TURN_MESSAGES),
            unstreamed_blocks: HashMap::new(),
        }
    }
}

impl ClaudeTurnText {
    /// Records that text of `message_id` reached the timeline through the stream.
    pub(super) fn mark_streamed(&mut self, message_id: &str) {
        self.streamed.insert(message_id);
    }

    pub(super) fn was_streamed(&self, message_id: &str) -> bool {
        self.streamed.contains(message_id)
    }

    /// Next text block position of a message that never streamed, or None once the turn tracks
    /// too many messages to keep positions stable.
    pub(super) fn next_unstreamed_block(&mut self, message_id: &str) -> Option<u32> {
        if !self.unstreamed_blocks.contains_key(message_id)
            && self.unstreamed_blocks.len() >= MAX_TRACKED_TURN_MESSAGES
        {
            return None;
        }
        let next = self
            .unstreamed_blocks
            .entry(message_id.to_string())
            .or_insert(0);
        let position = *next;
        *next = next.saturating_add(1);
        Some(position)
    }

    pub(super) fn clear(&mut self) {
        self.streamed.clear();
        self.unstreamed_blocks.clear();
    }
}

/// Where a projected ExitPlanMode plan came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::claude) enum ClaudePlanSource<'a> {
    /// An assistant `tool_use` block, with its id when it has one.
    Message(Option<&'a str>),
    /// A permission request naming the tool use id of its plan.
    IdentifiedRequest(&'a str),
    /// A permission request without a tool use id, whose plan id is its control request id.
    AnonymousRequest,
}

/// ExitPlanMode plans already projected.
///
/// The assistant message and the permission request both carry each plan. Their ids match
/// when the request names its tool use id. A request without one cannot be matched by id, so
/// within the current turn it pairs with one message carrying the same plan text: whichever
/// arrives first is projected and the second is consumed silently.
#[derive(Clone, Debug)]
pub(in crate::claude) struct ClaudePlanMemory {
    ids: BoundedIdSet,
    unpaired_message_digests: BoundedIdSet,
    unpaired_request_digests: BoundedIdSet,
}

impl Default for ClaudePlanMemory {
    fn default() -> Self {
        Self {
            ids: BoundedIdSet::new(MAX_TRACKED_PLAN_IDS),
            unpaired_message_digests: BoundedIdSet::new(MAX_TRACKED_PLAN_IDS),
            unpaired_request_digests: BoundedIdSet::new(MAX_TRACKED_PLAN_IDS),
        }
    }
}

impl ClaudePlanMemory {
    /// Claims the right to project a plan and reports whether no earlier frame projected it.
    pub(super) fn claim(&mut self, source: ClaudePlanSource<'_>, markdown: &str) -> bool {
        let digest = || format!("{:x}", Sha256::digest(markdown.as_bytes()));
        match source {
            ClaudePlanSource::IdentifiedRequest(id) => self.ids.insert(id),
            ClaudePlanSource::Message(id) => {
                if id.is_some_and(|id| !self.ids.insert(id)) {
                    return false;
                }
                let digest = digest();
                if self.unpaired_request_digests.remove(&digest) {
                    return false;
                }
                self.unpaired_message_digests.insert(&digest);
                true
            }
            ClaudePlanSource::AnonymousRequest => {
                let digest = digest();
                if self.unpaired_message_digests.remove(&digest) {
                    return false;
                }
                self.unpaired_request_digests.insert(&digest);
                true
            }
        }
    }

    /// Forgets the unpaired plan text of the finished turn; plan ids stay remembered.
    pub(super) fn end_turn(&mut self) {
        self.unpaired_message_digests.clear();
        self.unpaired_request_digests.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_set_forgets_the_oldest_id_at_capacity() {
        let mut set = BoundedIdSet::new(2);
        assert!(set.insert("a"));
        assert!(!set.insert("a"));
        assert!(set.insert("b"));
        assert!(set.insert("c"));
        assert!(!set.contains("a"));
        assert!(set.contains("b") && set.contains("c"));
    }

    #[test]
    fn anonymous_plan_requests_pair_with_message_text_only_within_a_turn() {
        let mut plans = ClaudePlanMemory::default();
        assert!(plans.claim(ClaudePlanSource::Message(Some("toolu_a")), "plan"));
        plans.end_turn();
        assert!(
            plans.claim(ClaudePlanSource::AnonymousRequest, "plan"),
            "an earlier turn's message never absorbs a new request"
        );
        assert!(!plans.claim(ClaudePlanSource::Message(None), "plan"));
        assert!(!plans.claim(ClaudePlanSource::IdentifiedRequest("toolu_a"), "other"));
        assert!(plans.claim(ClaudePlanSource::IdentifiedRequest("toolu_b"), "plan"));
    }

    #[test]
    fn unstreamed_block_positions_count_per_message_and_stop_at_the_bound() {
        let mut text = ClaudeTurnText::default();
        assert_eq!(text.next_unstreamed_block("m1"), Some(0));
        assert_eq!(text.next_unstreamed_block("m1"), Some(1));
        assert_eq!(text.next_unstreamed_block("m2"), Some(0));
        for index in 0..MAX_TRACKED_TURN_MESSAGES {
            text.next_unstreamed_block(&format!("filler-{index}"));
        }
        assert_eq!(text.next_unstreamed_block("late"), None);
        assert_eq!(text.next_unstreamed_block("m1"), Some(2));
        text.clear();
        assert_eq!(text.next_unstreamed_block("late"), Some(0));
    }
}
