//! Which turn a Codex session routes provider notifications to.
//!
//! The chat turn, the provider turn Codex runs for it, and the capacity retry armed for it move
//! together, so they are one value and every change goes through [`TurnRoute::transition`].

use super::capacity_retry::CodexCapacityRetry;
use crate::models::*;

/// The turn a Codex session is running.
#[derive(Clone, Debug, Default)]
pub enum TurnRoute {
    #[default]
    Idle,
    /// Codex runs a provider turn that no chat turn of this session started.
    Unowned { provider_turn: String },
    /// A chat turn runs. Its provider turn is known once Codex reports or accepts it, and a
    /// capacity retry may be armed for it.
    Chat {
        chat_turn: ChatTurnId,
        provider_turn: Option<String>,
        retry: Option<CodexCapacityRetry>,
    },
}

/// A change to the routed turn.
#[derive(Debug)]
pub enum TurnRouteEvent {
    /// A chat turn starts, with the capacity retry armed for it.
    Begin {
        chat_turn: ChatTurnId,
        retry: Option<CodexCapacityRetry>,
    },
    /// Codex reported or accepted this provider turn.
    ProviderTurn(String),
    /// A capacity retry discarded the failed provider turn; the chat turn keeps running.
    RestartAttempt,
    /// The routed turn ended.
    Settle,
}

/// What a transition hands back to its caller.
#[derive(Debug, Default)]
pub struct TurnRouteOutcome {
    /// The chat turn that settled.
    pub settled_chat_turn: Option<ChatTurnId>,
    /// The retry the transition discarded, already cancelled.
    pub cancelled_retry: Option<CodexCapacityRetry>,
}

impl TurnRoute {
    /// Applies `event`. Starting a chat turn while another one runs is refused as busy; every
    /// other event is legal from any state.
    pub fn transition(&mut self, event: TurnRouteEvent) -> ChatResult<TurnRouteOutcome> {
        let mut outcome = TurnRouteOutcome::default();
        match event {
            TurnRouteEvent::Begin { chat_turn, retry } => {
                if self.chat_turn().is_some() {
                    return Err(ChatError::new(
                        ChatErrorCode::Busy,
                        "Codex already has an active turn",
                        true,
                    ));
                }
                *self = Self::Chat {
                    chat_turn,
                    provider_turn: None,
                    retry,
                };
            }
            TurnRouteEvent::ProviderTurn(id) => match self {
                Self::Chat { provider_turn, .. } => *provider_turn = Some(id),
                Self::Idle | Self::Unowned { .. } => *self = Self::Unowned { provider_turn: id },
            },
            TurnRouteEvent::RestartAttempt => {
                if let Self::Chat { provider_turn, .. } = self {
                    *provider_turn = None;
                }
            }
            TurnRouteEvent::Settle => {
                if let Self::Chat {
                    chat_turn, retry, ..
                } = std::mem::take(self)
                {
                    outcome.settled_chat_turn = Some(chat_turn);
                    outcome.cancelled_retry = retry.inspect(CodexCapacityRetry::cancel);
                }
            }
        }
        Ok(outcome)
    }

    /// Takes the armed capacity retry out and cancels it, leaving the turn running.
    pub fn cancel_retry(&mut self) -> Option<CodexCapacityRetry> {
        match self {
            Self::Chat { retry, .. } => retry.take().inspect(CodexCapacityRetry::cancel),
            Self::Idle | Self::Unowned { .. } => None,
        }
    }

    /// Spends one attempt of the capacity retry armed for the running chat turn and returns a
    /// copy of it, or `None` when no retry is armed or its attempts are used up.
    pub fn claim_retry_attempt(&mut self) -> Option<CodexCapacityRetry> {
        match self {
            Self::Chat { retry, .. } => retry.as_mut().and_then(CodexCapacityRetry::claim_attempt),
            Self::Idle | Self::Unowned { .. } => None,
        }
    }

    pub fn chat_turn(&self) -> Option<&ChatTurnId> {
        match self {
            Self::Chat { chat_turn, .. } => Some(chat_turn),
            Self::Idle | Self::Unowned { .. } => None,
        }
    }

    pub fn provider_turn(&self) -> Option<&str> {
        match self {
            Self::Chat { provider_turn, .. } => provider_turn.as_deref(),
            Self::Unowned { provider_turn } => Some(provider_turn),
            Self::Idle => None,
        }
    }

    pub fn retry(&self) -> Option<&CodexCapacityRetry> {
        match self {
            Self::Chat { retry, .. } => retry.as_ref(),
            Self::Idle | Self::Unowned { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat_turn(id: &str) -> ChatTurnId {
        ChatTurnId::new(id.to_string()).unwrap()
    }

    fn begin(route: &mut TurnRoute, id: &str, retry: Option<CodexCapacityRetry>) {
        route
            .transition(TurnRouteEvent::Begin {
                chat_turn: chat_turn(id),
                retry,
            })
            .unwrap();
    }

    #[test]
    fn a_second_chat_turn_is_refused_while_one_runs() {
        let mut route = TurnRoute::Idle;
        begin(&mut route, "turn-1", None);

        let refused = route
            .transition(TurnRouteEvent::Begin {
                chat_turn: chat_turn("turn-2"),
                retry: None,
            })
            .unwrap_err();

        assert_eq!(refused.code, ChatErrorCode::Busy);
        assert_eq!(route.chat_turn(), Some(&chat_turn("turn-1")));
    }

    #[test]
    fn a_chat_turn_replaces_an_unowned_provider_turn() {
        let mut route = TurnRoute::Idle;
        route
            .transition(TurnRouteEvent::ProviderTurn("provider-0".to_string()))
            .unwrap();
        assert_eq!(route.provider_turn(), Some("provider-0"));
        assert_eq!(route.chat_turn(), None);

        begin(&mut route, "turn-1", None);

        assert_eq!(route.provider_turn(), None);
        assert_eq!(route.chat_turn(), Some(&chat_turn("turn-1")));
    }

    #[test]
    fn a_restarted_attempt_forgets_the_provider_turn_and_keeps_the_retry() {
        let mut route = TurnRoute::Idle;
        begin(
            &mut route,
            "turn-1",
            Some(CodexCapacityRetry::arm(&json!({}))),
        );
        route
            .transition(TurnRouteEvent::ProviderTurn("provider-1".to_string()))
            .unwrap();

        route.transition(TurnRouteEvent::RestartAttempt).unwrap();

        assert_eq!(route.provider_turn(), None);
        assert!(route.retry().is_some());
        assert_eq!(route.chat_turn(), Some(&chat_turn("turn-1")));
    }

    #[test]
    fn settling_returns_the_chat_turn_and_cancels_its_retry() {
        let mut route = TurnRoute::Idle;
        begin(
            &mut route,
            "turn-1",
            Some(CodexCapacityRetry::arm(&json!({}))),
        );

        let outcome = route.transition(TurnRouteEvent::Settle).unwrap();

        assert_eq!(outcome.settled_chat_turn, Some(chat_turn("turn-1")));
        assert!(!outcome.cancelled_retry.unwrap().available());
        assert!(matches!(route, TurnRoute::Idle));
    }

    #[test]
    fn cancelling_the_retry_leaves_the_turn_running() {
        let mut route = TurnRoute::Idle;
        begin(
            &mut route,
            "turn-1",
            Some(CodexCapacityRetry::arm(&json!({}))),
        );

        assert!(!route.cancel_retry().unwrap().available());
        assert!(route.retry().is_none());
        assert_eq!(route.chat_turn(), Some(&chat_turn("turn-1")));
        assert!(route.cancel_retry().is_none());
    }
}
