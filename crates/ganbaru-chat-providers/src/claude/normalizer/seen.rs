//! Bounded memory of provider ids that were already projected.

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

/// A new bounded set for ExitPlanMode tool use ids.
pub(in crate::claude) fn plan_id_set() -> BoundedIdSet {
    BoundedIdSet::new(MAX_TRACKED_PLAN_IDS)
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
