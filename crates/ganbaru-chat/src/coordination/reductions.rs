//! Comparison of runtime approvals against the approval work was frozen under.
//!
//! Access narrowing, the live tool checks, and the sync materialize hook share these rules, so a
//! narrowing saved on this device and one applied from another device agree on what still
//! covers running work. Policies use their stored spelling.

use ganbaru_chat_contracts::models::ChatRuntimeApprovalPolicy;

/// Orders stored runtime approval policies from narrowest to broadest; unknown values have no
/// rank.
pub fn approval_rank(policy: &str) -> Option<u8> {
    match ChatRuntimeApprovalPolicy::parse(policy)? {
        ChatRuntimeApprovalPolicy::Ask => Some(0),
        ChatRuntimeApprovalPolicy::AutoApprove => Some(1),
    }
}

/// Returns whether moving from `current` to `proposed` reduces authority. A change involving a
/// policy that cannot be ranked counts as a reduction.
pub fn approval_is_reduction(current: &str, proposed: &str) -> bool {
    if current == proposed {
        return false;
    }
    match (approval_rank(current), approval_rank(proposed)) {
        (Some(current), Some(proposed)) => proposed < current,
        _ => true,
    }
}

/// Returns whether the `live` approval still covers the `frozen` one: a known live policy at
/// least as broad as a known frozen policy.
pub fn approval_covers(live: &str, frozen: &str) -> bool {
    matches!(
        (approval_rank(live), approval_rank(frozen)),
        (Some(live), Some(frozen)) if live >= frozen
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_approve_ranks_above_ask_and_unknown_policies_have_no_rank() {
        assert!(approval_rank("ask") < approval_rank("auto_approve"));
        assert_eq!(approval_rank("unattended"), None);
        assert_eq!(approval_rank("provider_custom"), None);
    }

    #[test]
    fn narrowing_and_unknown_changes_are_reductions() {
        assert!(approval_is_reduction("auto_approve", "ask"));
        assert!(!approval_is_reduction("ask", "auto_approve"));
        assert!(!approval_is_reduction("ask", "ask"));
        assert!(approval_is_reduction("ask", "unattended"));
    }

    #[test]
    fn only_a_known_live_policy_as_broad_covers_frozen_work() {
        assert!(approval_covers("auto_approve", "ask"));
        assert!(approval_covers("ask", "ask"));
        assert!(!approval_covers("ask", "auto_approve"));
        assert!(!approval_covers("unattended", "unattended"));
    }
}
