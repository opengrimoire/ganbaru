//! Pure provider session, turn, and thread state transitions.

use super::models::{ChatError, ChatResult, ChatThreadState, ChatTurnState, ProviderSessionState};

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

/// Legal turn moves. A send stores the turn as `Pending`, and every provider reports the start
/// it dispatched as `Active`, so `Pending` moves to `Active` directly. Codex can report a turn
/// completion before its start notification, so `Pending` also settles as `Completed`.
pub const TURN_TRANSITIONS: &[(ChatTurnState, ChatTurnState)] = &[
    (ChatTurnState::Pending, ChatTurnState::Dispatching),
    (ChatTurnState::Pending, ChatTurnState::Active),
    (ChatTurnState::Pending, ChatTurnState::Completed),
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

/// Returns the turn states that may move to `to`, in `ChatTurnState::ALL` order.
pub fn turn_sources(to: ChatTurnState) -> impl Iterator<Item = ChatTurnState> {
    ChatTurnState::ALL
        .into_iter()
        .filter(move |from| can_transition_turn(*from, to))
}

/// Returns whether a persisted thread may move from `from` to `to`.
///
/// `Closed` is absorbing, and `Archived` is never persisted (archiving only sets
/// `archived_at`), so it is neither a source nor a target. Every other pair of distinct
/// activity states is legal because providers, turn events, and restores each report the
/// thread's activity independently.
pub fn can_transition_thread(from: ChatThreadState, to: ChatThreadState) -> bool {
    from != to && from.is_persisted() && to.is_persisted() && !from.is_terminal()
}

pub fn transition_thread(
    current: ChatThreadState,
    next: ChatThreadState,
) -> ChatResult<ChatThreadState> {
    if can_transition_thread(current, next) {
        Ok(next)
    } else {
        Err(ChatError::invalid_transition(format!(
            "Chat thread cannot transition from {current:?} to {next:?}"
        )))
    }
}

/// Returns the thread states that may move to `to`, in `ChatThreadState::ALL` order.
pub fn thread_sources(to: ChatThreadState) -> impl Iterator<Item = ChatThreadState> {
    ChatThreadState::ALL
        .into_iter()
        .filter(move |from| can_transition_thread(*from, to))
}
