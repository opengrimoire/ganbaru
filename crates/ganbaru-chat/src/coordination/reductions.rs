//! Comparison of teammate approval behavior against the runtime approval work was frozen under.
//!
//! The command that saves teammate access, the live tool checks, and the sync materialize hook
//! share these rules, so a reduction saved on this device and one applied from another device
//! agree on what still covers running work. Policies use their stored wire spelling.

/// Stored runtime approval that a teammate's provider configuration decides.
const PROVIDER_CUSTOM: &str = "provider_custom";

/// Orders runtime approval policies from narrowest to broadest. Provider-custom approval and
/// unknown values have no rank because nothing here can compare them.
pub fn approval_rank(policy: &str) -> Option<u8> {
    match policy {
        "ask" => Some(0),
        "auto_approve" => Some(1),
        "unattended" => Some(2),
        _ => None,
    }
}

/// Returns whether moving from `current` to `proposed` reduces authority. A change between
/// policies that cannot be ranked against each other counts as a reduction.
pub fn approval_is_reduction(current: &str, proposed: &str) -> bool {
    if current == proposed {
        return false;
    }
    match (approval_rank(current), approval_rank(proposed)) {
        (Some(current), Some(proposed)) => proposed < current,
        _ => true,
    }
}

/// Returns whether the `live` approval still covers the `frozen` one: an equal policy, or a
/// ranked live policy at least as broad as a ranked frozen policy.
pub fn approval_covers(live: &str, frozen: &str) -> bool {
    live == frozen
        || matches!(
            (approval_rank(live), approval_rank(frozen)),
            (Some(live), Some(frozen)) if live >= frozen
        )
}

/// Returns the runtime approval a stored teammate safety mode stands for, or `None` for an
/// unknown safety mode.
fn teammate_default_approval(safety_mode: &str) -> Option<&'static str> {
    match safety_mode {
        "ask_for_approval" => Some("ask"),
        "approve_for_me" => Some("auto_approve"),
        "full_access" => Some("unattended"),
        "custom" => Some(PROVIDER_CUSTOM),
        _ => None,
    }
}

/// Returns whether a teammate with `safety_mode`, under the runtime approval override that
/// governs the work, still covers the `frozen` approval.
///
/// The live approval is the narrower of the teammate's default and the override. A Custom
/// teammate cannot be ranked against an override, so the override alone governs it, as it does
/// when the work freezes. An unknown safety mode covers nothing.
pub fn teammate_policy_covers(
    safety_mode: &str,
    approval_override: Option<&str>,
    frozen: &str,
) -> bool {
    let Some(teammate_default) = teammate_default_approval(safety_mode) else {
        return false;
    };
    let granted = approval_override.unwrap_or(teammate_default);
    let live = match (approval_rank(teammate_default), approval_rank(granted)) {
        (Some(teammate_rank), Some(granted_rank)) if teammate_rank < granted_rank => {
            teammate_default
        }
        _ => granted,
    };
    approval_covers(live, frozen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unranked_approval_changes_count_as_reductions() {
        assert!(approval_is_reduction("unattended", "ask"));
        assert!(!approval_is_reduction("ask", "unattended"));
        assert!(!approval_is_reduction(PROVIDER_CUSTOM, PROVIDER_CUSTOM));
        assert!(approval_is_reduction(PROVIDER_CUSTOM, "unattended"));
        assert!(approval_is_reduction("ask", PROVIDER_CUSTOM));
    }

    #[test]
    fn a_narrower_or_widened_teammate_is_compared_with_the_frozen_approval() {
        assert!(!teammate_policy_covers(
            "ask_for_approval",
            None,
            "auto_approve"
        ));
        assert!(teammate_policy_covers(
            "approve_for_me",
            None,
            "auto_approve"
        ));
        assert!(teammate_policy_covers("full_access", None, "auto_approve"));
        assert!(!teammate_policy_covers(
            "approve_for_me",
            None,
            "unattended"
        ));
    }

    #[test]
    fn an_override_narrows_the_teammate_and_never_widens_it() {
        assert!(!teammate_policy_covers(
            "full_access",
            Some("ask"),
            "auto_approve"
        ));
        assert!(!teammate_policy_covers(
            "ask_for_approval",
            Some("unattended"),
            "auto_approve"
        ));
        assert!(teammate_policy_covers(
            "full_access",
            Some("auto_approve"),
            "ask"
        ));
    }

    #[test]
    fn a_custom_teammate_leaves_the_override_in_charge() {
        assert!(teammate_policy_covers("custom", Some("ask"), "ask"));
        assert!(teammate_policy_covers(
            "custom",
            Some("unattended"),
            "auto_approve"
        ));
        assert!(!teammate_policy_covers("custom", Some("ask"), "unattended"));
        assert!(teammate_policy_covers("custom", None, PROVIDER_CUSTOM));
    }

    #[test]
    fn leaving_custom_or_entering_it_without_an_override_covers_no_ranked_freeze() {
        assert!(!teammate_policy_covers("custom", None, "unattended"));
        assert!(!teammate_policy_covers(
            "full_access",
            None,
            PROVIDER_CUSTOM
        ));
    }

    #[test]
    fn an_unknown_safety_mode_covers_nothing() {
        assert!(!teammate_policy_covers("reckless", None, "ask"));
        assert!(!teammate_policy_covers("reckless", Some("ask"), "ask"));
    }
}
