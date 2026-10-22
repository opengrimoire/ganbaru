//! Per-session gate for model preview tools when the provider sandbox denies network access.
//!
//! Preview pages run in the app's webview with full network access, so a model whose commands
//! the provider keeps offline could still reach the network through a page it serves. Each
//! provider session gets one endpoint and one gate, so the user approves preview tools once per
//! session instead of losing them.

/// What a preview tool call may do right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreviewNetworkDecision {
    Allowed,
    /// A turn is being dispatched and its network policy is not known yet.
    AwaitingTurn,
    /// The current turn denies network and the user has not approved preview tools.
    NeedsApproval,
}

/// Network policy state for one provider session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PreviewNetworkGate {
    denied: bool,
    awaiting_turn: bool,
    approved: bool,
}

impl PreviewNetworkGate {
    /// Holds preview tools until the turn being dispatched reports its network policy.
    pub(super) fn begin_turn(&mut self) {
        self.awaiting_turn = true;
    }

    /// Records the dispatched turn's network policy. `None` means the provider declared no
    /// policy, or dispatch failed, so the previous policy stays in force.
    pub(super) fn settle_turn(&mut self, network_access: Option<bool>) {
        self.awaiting_turn = false;
        if let Some(access) = network_access {
            self.denied = !access;
        }
    }

    /// Records the user's approval for the rest of the session.
    pub(super) fn approve(&mut self) {
        self.approved = true;
    }

    pub(super) fn decision(&self) -> PreviewNetworkDecision {
        if self.awaiting_turn {
            PreviewNetworkDecision::AwaitingTurn
        } else if self.denied && !self.approved {
            PreviewNetworkDecision::NeedsApproval
        } else {
            PreviewNetworkDecision::Allowed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_tools_are_refused_until_approved_when_the_turn_denies_network() {
        let mut gate = PreviewNetworkGate::default();
        assert_eq!(gate.decision(), PreviewNetworkDecision::Allowed);
        gate.begin_turn();
        assert_eq!(gate.decision(), PreviewNetworkDecision::AwaitingTurn);
        gate.settle_turn(Some(false));
        assert_eq!(gate.decision(), PreviewNetworkDecision::NeedsApproval);
        gate.approve();
        assert_eq!(gate.decision(), PreviewNetworkDecision::Allowed);
        gate.begin_turn();
        gate.settle_turn(Some(false));
        assert_eq!(
            gate.decision(),
            PreviewNetworkDecision::Allowed,
            "approval lasts for the session"
        );
    }

    #[test]
    fn turns_with_network_or_without_a_declared_policy_keep_preview_tools_available() {
        let mut gate = PreviewNetworkGate::default();
        gate.begin_turn();
        gate.settle_turn(Some(true));
        assert_eq!(gate.decision(), PreviewNetworkDecision::Allowed);
        gate.begin_turn();
        gate.settle_turn(None);
        assert_eq!(gate.decision(), PreviewNetworkDecision::Allowed);
    }

    #[test]
    fn an_undeclared_or_failed_turn_keeps_the_previous_denial() {
        let mut gate = PreviewNetworkGate::default();
        gate.begin_turn();
        gate.settle_turn(Some(false));
        gate.begin_turn();
        gate.settle_turn(None);
        assert_eq!(gate.decision(), PreviewNetworkDecision::NeedsApproval);
        gate.begin_turn();
        gate.settle_turn(Some(true));
        assert_eq!(gate.decision(), PreviewNetworkDecision::Allowed);
    }
}
