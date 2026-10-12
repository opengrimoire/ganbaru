//! Pure provider session and turn state transitions.

use super::models::{ChatError, ChatResult, ChatTurnState, ProviderSessionState};

pub const SESSION_TRANSITIONS: &[(ProviderSessionState, ProviderSessionState)] = &[
    (
        ProviderSessionState::Stopped,
        ProviderSessionState::Starting,
    ),
    (ProviderSessionState::Starting, ProviderSessionState::Ready),
    (
        ProviderSessionState::Starting,
        ProviderSessionState::Stopping,
    ),
    (ProviderSessionState::Starting, ProviderSessionState::Failed),
    (ProviderSessionState::Ready, ProviderSessionState::Active),
    (ProviderSessionState::Ready, ProviderSessionState::Stopping),
    (ProviderSessionState::Ready, ProviderSessionState::Failed),
    (ProviderSessionState::Active, ProviderSessionState::Ready),
    (
        ProviderSessionState::Active,
        ProviderSessionState::WaitingForApproval,
    ),
    (
        ProviderSessionState::Active,
        ProviderSessionState::WaitingForUserInput,
    ),
    (ProviderSessionState::Active, ProviderSessionState::Stopping),
    (ProviderSessionState::Active, ProviderSessionState::Failed),
    (
        ProviderSessionState::WaitingForApproval,
        ProviderSessionState::Active,
    ),
    (
        ProviderSessionState::WaitingForApproval,
        ProviderSessionState::Ready,
    ),
    (
        ProviderSessionState::WaitingForApproval,
        ProviderSessionState::Stopping,
    ),
    (
        ProviderSessionState::WaitingForApproval,
        ProviderSessionState::Failed,
    ),
    (
        ProviderSessionState::WaitingForUserInput,
        ProviderSessionState::Active,
    ),
    (
        ProviderSessionState::WaitingForUserInput,
        ProviderSessionState::Ready,
    ),
    (
        ProviderSessionState::WaitingForUserInput,
        ProviderSessionState::Stopping,
    ),
    (
        ProviderSessionState::WaitingForUserInput,
        ProviderSessionState::Failed,
    ),
    (
        ProviderSessionState::Stopping,
        ProviderSessionState::Stopped,
    ),
    (ProviderSessionState::Stopping, ProviderSessionState::Failed),
    (ProviderSessionState::Failed, ProviderSessionState::Starting),
    (ProviderSessionState::Failed, ProviderSessionState::Stopping),
    (ProviderSessionState::Failed, ProviderSessionState::Stopped),
];

pub const TURN_TRANSITIONS: &[(ChatTurnState, ChatTurnState)] = &[
    (ChatTurnState::Pending, ChatTurnState::Dispatching),
    (ChatTurnState::Pending, ChatTurnState::Interrupted),
    (ChatTurnState::Pending, ChatTurnState::Failed),
    (ChatTurnState::Dispatching, ChatTurnState::Active),
    (
        ChatTurnState::Dispatching,
        ChatTurnState::WaitingForApproval,
    ),
    (
        ChatTurnState::Dispatching,
        ChatTurnState::WaitingForUserInput,
    ),
    (ChatTurnState::Dispatching, ChatTurnState::Completed),
    (ChatTurnState::Dispatching, ChatTurnState::Interrupted),
    (ChatTurnState::Dispatching, ChatTurnState::Failed),
    (ChatTurnState::Active, ChatTurnState::WaitingForApproval),
    (ChatTurnState::Active, ChatTurnState::WaitingForUserInput),
    (ChatTurnState::Active, ChatTurnState::Completed),
    (ChatTurnState::Active, ChatTurnState::Interrupted),
    (ChatTurnState::Active, ChatTurnState::Failed),
    (ChatTurnState::WaitingForApproval, ChatTurnState::Active),
    (ChatTurnState::WaitingForApproval, ChatTurnState::Completed),
    (
        ChatTurnState::WaitingForApproval,
        ChatTurnState::Interrupted,
    ),
    (ChatTurnState::WaitingForApproval, ChatTurnState::Failed),
    (ChatTurnState::WaitingForUserInput, ChatTurnState::Active),
    (ChatTurnState::WaitingForUserInput, ChatTurnState::Completed),
    (
        ChatTurnState::WaitingForUserInput,
        ChatTurnState::Interrupted,
    ),
    (ChatTurnState::WaitingForUserInput, ChatTurnState::Failed),
];

pub fn can_transition_session(from: ProviderSessionState, to: ProviderSessionState) -> bool {
    SESSION_TRANSITIONS.contains(&(from, to))
}

pub fn transition_session(
    current: ProviderSessionState,
    next: ProviderSessionState,
) -> ChatResult<ProviderSessionState> {
    if can_transition_session(current, next) {
        Ok(next)
    } else {
        Err(ChatError::invalid_transition(format!(
            "provider session cannot transition from {current:?} to {next:?}"
        )))
    }
}

pub fn can_transition_turn(from: ChatTurnState, to: ChatTurnState) -> bool {
    TURN_TRANSITIONS.contains(&(from, to))
}

pub fn transition_turn(current: ChatTurnState, next: ChatTurnState) -> ChatResult<ChatTurnState> {
    if can_transition_turn(current, next) {
        Ok(next)
    } else {
        Err(ChatError::invalid_transition(format!(
            "Chat turn cannot transition from {current:?} to {next:?}"
        )))
    }
}
