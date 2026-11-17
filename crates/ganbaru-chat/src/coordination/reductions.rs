//! Comparison of agent approval behavior against the runtime approval work was frozen under.
//!
//! The command that saves agent access, the live tool checks, and the sync materialize hook
//! share these rules, so a reduction saved on this device and one applied from another device
//! agree on what still covers running work. Policies use their stored wire spelling.

/// Stored runtime approval that an agent's provider configuration decides.
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

/// Returns the runtime approval a stored agent safety mode stands for, or `None` for an
/// unknown safety mode.
fn agent_default_approval(safety_mode: &str) -> Option<&'static str> {
    match safety_mode {
        "ask_for_approval" => Some("ask"),
        "approve_for_me" => Some("auto_approve"),
        "full_access" => Some("unattended"),
        "custom" => Some(PROVIDER_CUSTOM),
        _ => None,
    }
}

/// Returns whether an agent with `safety_mode`, under the runtime approval override that
/// governs the work, still covers the `frozen` approval.
///
/// The live approval is the narrower of the agent's default and the override. A Custom
/// agent cannot be ranked against an override, so the override alone governs it, as it does
/// when the work freezes. An unknown safety mode covers nothing.
pub fn agent_policy_covers(
    safety_mode: &str,
    approval_override: Option<&str>,
    frozen: &str,
) -> bool {
    let Some(agent_default) = agent_default_approval(safety_mode) else {
        return false;
    };
    let granted = approval_override.unwrap_or(agent_default);
    let live = match (approval_rank(agent_default), approval_rank(granted)) {
        (Some(agent_rank), Some(granted_rank)) if agent_rank < granted_rank => agent_default,
        _ => granted,
    };
    approval_covers(live, frozen)
}

/// Returns an SQL scalar expression for the safety mode that decides coverage for the agent
/// whose participant id is `agent_id`, a column or parameter of the enclosing query.
///
/// While the agent's pointer names a revision that has not arrived on this device, the safety
/// mode the pointer copied decides instead of the newest arrived revision the effective policy
/// view falls back to, which can be a concurrent narrower one the pointer did not choose.
/// Coverage depends only on the safety mode, so an honest copy decides as its revision will, a
/// dishonest copy is contained when its revision arrives, and a pointer whose revision never
/// arrives still refuses work its copied mode does not cover. Otherwise the effective policy
/// decides. The expression is null when the agent has neither, which covers nothing.
pub fn live_agent_safety_mode_sql(agent_id: &str) -> String {
    format!(
        "(SELECT CASE
             WHEN live_agent.current_policy_revision_id IS NOT NULL
              AND NOT EXISTS (
                  SELECT 1 FROM chat_agent_policy_revisions named
                  WHERE named.id = live_agent.current_policy_revision_id
              )
             THEN live_agent.current_policy_safety_mode
             ELSE (
                 SELECT live_policy.safety_mode FROM chat_agent_effective_policies live_policy
                 WHERE live_policy.agent_id = live_agent.participant_id
             )
           END
           FROM chat_agents live_agent
           WHERE live_agent.participant_id = {agent_id})"
    )
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
    fn a_narrower_or_widened_agent_is_compared_with_the_frozen_approval() {
        assert!(!agent_policy_covers(
            "ask_for_approval",
            None,
            "auto_approve"
        ));
        assert!(agent_policy_covers("approve_for_me", None, "auto_approve"));
        assert!(agent_policy_covers("full_access", None, "auto_approve"));
        assert!(!agent_policy_covers("approve_for_me", None, "unattended"));
    }

    #[test]
    fn an_override_narrows_the_agent_and_never_widens_it() {
        assert!(!agent_policy_covers(
            "full_access",
            Some("ask"),
            "auto_approve"
        ));
        assert!(!agent_policy_covers(
            "ask_for_approval",
            Some("unattended"),
            "auto_approve"
        ));
        assert!(agent_policy_covers(
            "full_access",
            Some("auto_approve"),
            "ask"
        ));
    }

    #[test]
    fn a_custom_agent_leaves_the_override_in_charge() {
        assert!(agent_policy_covers("custom", Some("ask"), "ask"));
        assert!(agent_policy_covers(
            "custom",
            Some("unattended"),
            "auto_approve"
        ));
        assert!(!agent_policy_covers("custom", Some("ask"), "unattended"));
        assert!(agent_policy_covers("custom", None, PROVIDER_CUSTOM));
    }

    #[test]
    fn leaving_custom_or_entering_it_without_an_override_covers_no_ranked_freeze() {
        assert!(!agent_policy_covers("custom", None, "unattended"));
        assert!(!agent_policy_covers("full_access", None, PROVIDER_CUSTOM));
    }

    #[test]
    fn an_unknown_safety_mode_covers_nothing() {
        assert!(!agent_policy_covers("reckless", None, "ask"));
        assert!(!agent_policy_covers("reckless", Some("ask"), "ask"));
    }
}
