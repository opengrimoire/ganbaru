//! Pure provider session, turn, thread, work assignment, agent run, and dispatch job state
//! transitions.

use super::models::{
    ChatError, ChatResult, ChatThreadState, ChatTurnState, ChatWorkAssignmentState,
    ProviderSessionState,
};

/// Legal moves of the reported provider session state. The runtime derives the state from
/// [`crate::runtime::phase::RuntimePhase`], and a test checks every phase move against this table.
/// An interrupt reports `Stopping` until the provider settles or refuses it, so `Stopping` returns
/// to a running state, and a provider whose process ends reports `Stopped` from any running state.
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
    (
        ProviderSessionState::Starting,
        ProviderSessionState::Stopped,
    ),
    (ProviderSessionState::Ready, ProviderSessionState::Active),
    (ProviderSessionState::Ready, ProviderSessionState::Stopping),
    (ProviderSessionState::Ready, ProviderSessionState::Failed),
    (ProviderSessionState::Ready, ProviderSessionState::Stopped),
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
    (ProviderSessionState::Active, ProviderSessionState::Stopped),
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
        ProviderSessionState::WaitingForApproval,
        ProviderSessionState::Stopped,
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
        ProviderSessionState::WaitingForUserInput,
        ProviderSessionState::Stopped,
    ),
    (
        ProviderSessionState::Stopping,
        ProviderSessionState::Stopped,
    ),
    (ProviderSessionState::Stopping, ProviderSessionState::Failed),
    (ProviderSessionState::Stopping, ProviderSessionState::Ready),
    (ProviderSessionState::Stopping, ProviderSessionState::Active),
    (
        ProviderSessionState::Stopping,
        ProviderSessionState::WaitingForApproval,
    ),
    (
        ProviderSessionState::Stopping,
        ProviderSessionState::WaitingForUserInput,
    ),
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

/// Returns whether a local writer may move a work assignment from `from` to `to`.
///
/// Work in progress (queued, working, or waiting) may report any progress or ending, and repeats
/// of the same progress are legal because providers report it again. Only an answered question
/// returns work to `Queued`. Ready-for-review work only ends, since a review that asks for changes
/// continues in a follow-up assignment. A person may cancel ended work, and a retry queues failed
/// or cancelled work again. Completed work never reopens.
///
/// The table governs local writers. A remote apply instead merges concurrent states by the sync
/// priority, where the most restrictive ending wins, so a replicated row may hold any state.
pub fn can_transition_assignment(
    from: ChatWorkAssignmentState,
    to: ChatWorkAssignmentState,
) -> bool {
    use ChatWorkAssignmentState::{
        Cancelled, Completed, Failed, Queued, ReadyForReview, WaitingForAnswer, WaitingForApproval,
        Working,
    };
    match from {
        Queued | Working | WaitingForApproval => !matches!(to, Queued),
        WaitingForAnswer => true,
        ReadyForReview => matches!(to, Completed | Failed | Cancelled),
        Completed => to == Cancelled,
        Failed => matches!(to, Queued | Cancelled),
        Cancelled => to == Queued,
    }
}

pub fn transition_assignment(
    current: ChatWorkAssignmentState,
    next: ChatWorkAssignmentState,
) -> ChatResult<ChatWorkAssignmentState> {
    if can_transition_assignment(current, next) {
        Ok(next)
    } else {
        Err(ChatError::invalid_transition(format!(
            "Chat work assignment cannot transition from {current:?} to {next:?}"
        )))
    }
}

/// Returns the assignment states that may move to `to`, in `ChatWorkAssignmentState::ALL` order.
pub fn assignment_sources(
    to: ChatWorkAssignmentState,
) -> impl Iterator<Item = ChatWorkAssignmentState> {
    ChatWorkAssignmentState::ALL
        .into_iter()
        .filter(move |from| can_transition_assignment(*from, to))
}

/// The persisted state of one agent run, the provider turn that carries an assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatAgentRunState {
    Queued,
    Starting,
    Working,
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

impl ChatAgentRunState {
    pub const ALL: [Self; 7] = [
        Self::Queued,
        Self::Starting,
        Self::Working,
        Self::Waiting,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
    ];

    /// Returns the persisted literal of the state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses a stored state, or returns `None` for an unknown value.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }

    /// Returns whether the run ended, after which its state and settle time never change.
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Returns whether a run may move from `from` to `to`. A run that has not settled may work, wait,
/// or settle, and repeats of working or waiting are legal because provider events repeat. No
/// writer returns a run to `Queued` or `Starting`, and a settled run is absorbing.
pub fn can_transition_run(from: ChatAgentRunState, to: ChatAgentRunState) -> bool {
    !from.is_settled() && !matches!(to, ChatAgentRunState::Queued | ChatAgentRunState::Starting)
}

pub fn transition_run(
    current: ChatAgentRunState,
    next: ChatAgentRunState,
) -> ChatResult<ChatAgentRunState> {
    if can_transition_run(current, next) {
        Ok(next)
    } else {
        Err(ChatError::invalid_transition(format!(
            "Chat agent run cannot transition from {current:?} to {next:?}"
        )))
    }
}

/// Returns the run states that may move to `to`, in `ChatAgentRunState::ALL` order.
pub fn run_sources(to: ChatAgentRunState) -> impl Iterator<Item = ChatAgentRunState> {
    ChatAgentRunState::ALL
        .into_iter()
        .filter(move |from| can_transition_run(*from, to))
}

/// The persisted state of the device-local dispatch job that starts runs for one assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatDispatchJobState {
    Queued,
    Claimed,
    Completed,
    Failed,
    Cancelled,
}

impl ChatDispatchJobState {
    pub const ALL: [Self; 5] = [
        Self::Queued,
        Self::Claimed,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
    ];

    /// Returns the persisted literal of the state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses a stored state, or returns `None` for an unknown value.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }

    /// Returns whether the job holds no claim and waits for nothing: it ran, failed, or was
    /// cancelled, and only a requeue makes it dispatchable again.
    pub fn is_ended(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Returns whether a dispatch job may move from `from` to `to`. A queued job is claimed, or ended
/// when its assignment settles first. A claimed job ends with its launch, or returns to the queue
/// when its claim goes stale. An ended job only returns to the queue, for a retry or an answer
/// that arrived after its run ended.
pub fn can_transition_dispatch_job(from: ChatDispatchJobState, to: ChatDispatchJobState) -> bool {
    use ChatDispatchJobState::{Claimed, Queued};
    match from {
        Queued => to == Claimed || to.is_ended(),
        Claimed => to == Queued || to.is_ended(),
        _ => to == Queued,
    }
}

pub fn transition_dispatch_job(
    current: ChatDispatchJobState,
    next: ChatDispatchJobState,
) -> ChatResult<ChatDispatchJobState> {
    if can_transition_dispatch_job(current, next) {
        Ok(next)
    } else {
        Err(ChatError::invalid_transition(format!(
            "Chat dispatch job cannot transition from {current:?} to {next:?}"
        )))
    }
}

/// Returns the dispatch job states that may move to `to`, in `ChatDispatchJobState::ALL` order.
pub fn dispatch_job_sources(
    to: ChatDispatchJobState,
) -> impl Iterator<Item = ChatDispatchJobState> {
    ChatDispatchJobState::ALL
        .into_iter()
        .filter(move |from| can_transition_dispatch_job(*from, to))
}

/// The persisted state of a device-local request for a secret an agent run asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatSecretRequestState {
    Pending,
    Provided,
    Declined,
    Expired,
}

impl ChatSecretRequestState {
    pub const ALL: [Self; 4] = [Self::Pending, Self::Provided, Self::Declined, Self::Expired];

    /// Returns the persisted literal of the state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Provided => "provided",
            Self::Declined => "declined",
            Self::Expired => "expired",
        }
    }

    /// Parses a stored state, or returns `None` for an unknown value.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }
}

/// Returns whether a secret request may move from `from` to `to`. Only a pending request is
/// answered, declined, or expired, and every answer is final.
pub fn can_transition_secret_request(
    from: ChatSecretRequestState,
    to: ChatSecretRequestState,
) -> bool {
    from == ChatSecretRequestState::Pending && to != ChatSecretRequestState::Pending
}

/// The persisted state of a follow-up queued behind a running direct turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatQueuedFollowupState {
    Queued,
    Dispatched,
    Cancelled,
}

impl ChatQueuedFollowupState {
    pub const ALL: [Self; 3] = [Self::Queued, Self::Dispatched, Self::Cancelled];

    /// Returns the persisted literal of the state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Dispatched => "dispatched",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses a stored state, or returns `None` for an unknown value.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.as_str() == value)
    }
}

/// Returns whether a queued follow-up may move from `from` to `to`. A queued follow-up is sent
/// once or cancelled, and either outcome is final.
pub fn can_transition_queued_followup(
    from: ChatQueuedFollowupState,
    to: ChatQueuedFollowupState,
) -> bool {
    from == ChatQueuedFollowupState::Queued && to != ChatQueuedFollowupState::Queued
}

#[cfg(test)]
mod tests;
