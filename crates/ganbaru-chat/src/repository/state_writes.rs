//! Guarded writers for persisted Chat states.
//!
//! Every write of a lifecycle state column (`chat_turns`, `chat_threads`,
//! `chat_work_assignments`, `chat_agent_runs`, `chat_assignment_dispatch_jobs`,
//! `chat_secret_requests`, and `chat_queued_followups`) names the
//! target state and lets [`crate::state`] decide the legal sources, so SQLite applies only moves
//! the transition tables allow. A write that changes no row is classified instead of being
//! treated as success.

use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadState, ChatTurnState, ChatWorkAssignmentState,
};
use crate::state::{
    ChatAgentRunState, ChatDispatchJobState, ChatQueuedFollowupState, ChatSecretRequestState,
    can_transition_assignment, can_transition_dispatch_job, can_transition_queued_followup,
    can_transition_run, can_transition_secret_request, can_transition_thread, can_transition_turn,
};
use sqlx::SqliteConnection;

/// A persisted lifecycle state with one transition table in [`crate::state`].
pub trait StoredState: Copy + Eq + std::fmt::Debug + 'static {
    /// Every state, in declaration order.
    const STATES: &'static [Self];

    /// Returns the persisted literal of the state.
    fn stored(self) -> &'static str;

    /// Returns whether the transition table allows moving from `from` to `to`.
    fn can_move(from: Self, to: Self) -> bool;
}

impl StoredState for ChatTurnState {
    const STATES: &'static [Self] = &Self::ALL;

    fn stored(self) -> &'static str {
        self.as_str()
    }

    fn can_move(from: Self, to: Self) -> bool {
        can_transition_turn(from, to)
    }
}

impl StoredState for ChatThreadState {
    const STATES: &'static [Self] = &Self::ALL;

    fn stored(self) -> &'static str {
        self.as_str()
    }

    fn can_move(from: Self, to: Self) -> bool {
        can_transition_thread(from, to)
    }
}

impl StoredState for ChatWorkAssignmentState {
    const STATES: &'static [Self] = &Self::ALL;

    fn stored(self) -> &'static str {
        self.as_str()
    }

    fn can_move(from: Self, to: Self) -> bool {
        can_transition_assignment(from, to)
    }
}

impl StoredState for ChatAgentRunState {
    const STATES: &'static [Self] = &Self::ALL;

    fn stored(self) -> &'static str {
        self.as_str()
    }

    fn can_move(from: Self, to: Self) -> bool {
        can_transition_run(from, to)
    }
}

impl StoredState for ChatDispatchJobState {
    const STATES: &'static [Self] = &Self::ALL;

    fn stored(self) -> &'static str {
        self.as_str()
    }

    fn can_move(from: Self, to: Self) -> bool {
        can_transition_dispatch_job(from, to)
    }
}

impl StoredState for ChatSecretRequestState {
    const STATES: &'static [Self] = &Self::ALL;

    fn stored(self) -> &'static str {
        self.as_str()
    }

    fn can_move(from: Self, to: Self) -> bool {
        can_transition_secret_request(from, to)
    }
}

impl StoredState for ChatQueuedFollowupState {
    const STATES: &'static [Self] = &Self::ALL;

    fn stored(self) -> &'static str {
        self.as_str()
    }

    fn can_move(from: Self, to: Self) -> bool {
        can_transition_queued_followup(from, to)
    }
}

/// The result of a guarded state write.
///
/// Provider events repeat and arrive late (a duplicate completion, a start replayed by a
/// projection rebuild over turns that already settled), so callers treat `AlreadyInState` and
/// `Rejected` as normal outcomes that leave the row untouched, not as pipeline failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateWrite<S> {
    /// The row moved to the target state.
    Applied,
    /// The row was already in the target state, and the table has no move from a state to
    /// itself, so nothing changed.
    AlreadyInState,
    /// The transition table forbids moving from the current state to the target.
    Rejected { from: S },
    /// No row matches the identifiers.
    Missing,
}

impl<S> StateWrite<S> {
    pub fn is_applied(&self) -> bool {
        matches!(self, Self::Applied)
    }
}

/// A value bound to a caller supplied column assignment or key condition, in placeholder order.
#[derive(Clone, Copy, Debug)]
pub enum StateWriteValue<'v> {
    Text(&'v str),
    OptionalText(Option<&'v str>),
    OptionalInteger(Option<i64>),
}

/// Extra columns written together with a state move, as `column = ?` assignments joined by
/// commas, with one value per placeholder.
#[derive(Clone, Copy, Debug, Default)]
pub struct StateWriteColumns<'a> {
    pub assignments: &'a str,
    pub values: &'a [StateWriteValue<'a>],
}

/// The one row a guarded write moves: its table and a key condition, such as `id = ?`, with one
/// value per placeholder. The key must match at most one row.
#[derive(Clone, Copy, Debug)]
pub struct GuardedRow<'a> {
    pub table: &'static str,
    pub key: &'static str,
    pub key_values: &'a [StateWriteValue<'a>],
}

/// Returns a quoted SQL list of the given states, or `NULL` when there are none so that
/// `state IN (NULL)` matches nothing.
fn sql_list(states: impl Iterator<Item = &'static str>) -> String {
    let list = states
        .map(|state| format!("'{state}'"))
        .collect::<Vec<_>>()
        .join(", ");
    if list.is_empty() {
        "NULL".to_string()
    } else {
        list
    }
}

/// Returns the quoted SQL list of the states the transition table of `S` allows to move to `to`.
pub fn sources_sql<S: StoredState>(to: S) -> String {
    sql_list(
        S::STATES
            .iter()
            .copied()
            .filter(|from| S::can_move(*from, to))
            .map(S::stored),
    )
}

/// Returns the quoted SQL list of the states `admits` accepts among those the transition table
/// of `S` allows to move to `to`. A writer whose guard is narrower than the table names its
/// sources here, so the table still bounds the write: a state the table forbids is never listed.
pub fn narrowed_sources_sql<S: StoredState>(to: S, admits: impl Fn(S) -> bool) -> String {
    sql_list(
        S::STATES
            .iter()
            .copied()
            .filter(|from| S::can_move(*from, to) && admits(*from))
            .map(S::stored),
    )
}

/// Returns the quoted SQL list of turn states legal as sources for a move to `to`.
pub fn turn_sources_sql(to: ChatTurnState) -> String {
    sources_sql(to)
}

/// Returns the quoted SQL list of turn states a provider holds (see
/// [`ChatTurnState::is_running`]).
pub fn running_turn_states_sql() -> String {
    sql_list(
        ChatTurnState::ALL
            .into_iter()
            .filter(|state| state.is_running())
            .map(ChatTurnState::as_str),
    )
}

/// Returns the quoted SQL list of thread states legal as sources for a move to `to`.
pub fn thread_sources_sql(to: ChatThreadState) -> String {
    sources_sql(to)
}

/// Returns a `state = ...` assignment that moves a thread to `to` only from a legal source and
/// otherwise keeps its current state, for statements that also write other thread columns.
pub fn guarded_thread_state_assignment(to: ChatThreadState) -> String {
    format!(
        "state = CASE WHEN state IN ({}) THEN '{}' ELSE state END",
        thread_sources_sql(to),
        to.as_str()
    )
}

/// Moves the row `row` to `to` when the transition table of `S` allows it from its current
/// state, writing `columns` in the same statement only when the move applies. A write that
/// changes no row reads the current state back to say why.
pub async fn persist_guarded_transition<S: StoredState>(
    connection: &mut SqliteConnection,
    row: GuardedRow<'_>,
    to: S,
    columns: StateWriteColumns<'_>,
) -> ChatResult<StateWrite<S>> {
    let extra = if columns.assignments.is_empty() {
        String::new()
    } else {
        format!(", {}", columns.assignments)
    };
    let statement = format!(
        "UPDATE {} SET state = '{}'{extra} WHERE ({}) AND state IN ({})",
        row.table,
        to.stored(),
        row.key,
        sources_sql(to)
    );
    let mut query = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()));
    for value in columns.values.iter().chain(row.key_values) {
        query = match *value {
            StateWriteValue::Text(value) => query.bind(value),
            StateWriteValue::OptionalText(value) => query.bind(value),
            StateWriteValue::OptionalInteger(value) => query.bind(value),
        };
    }
    let updated = query
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    if updated.rows_affected() > 0 {
        return Ok(StateWrite::Applied);
    }
    let read = format!("SELECT state FROM {} WHERE {}", row.table, row.key);
    let mut query = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(read.as_str()));
    for value in row.key_values {
        query = match *value {
            StateWriteValue::Text(value) => query.bind(value),
            StateWriteValue::OptionalText(value) => query.bind(value),
            StateWriteValue::OptionalInteger(value) => query.bind(value),
        };
    }
    let current = query
        .fetch_optional(&mut *connection)
        .await
        .map_err(persistence_error)?;
    let Some(current) = current else {
        return Ok(StateWrite::Missing);
    };
    let current = parse_state::<S>(&current)?;
    Ok(if current == to {
        StateWrite::AlreadyInState
    } else {
        StateWrite::Rejected { from: current }
    })
}

/// Moves one turn to `to` when the transition table allows it from its current state, writing
/// `columns` in the same statement only when the move applies.
pub async fn persist_turn_transition(
    connection: &mut SqliteConnection,
    thread_id: &str,
    turn_id: &str,
    to: ChatTurnState,
    columns: StateWriteColumns<'_>,
) -> ChatResult<StateWrite<ChatTurnState>> {
    persist_guarded_transition(
        connection,
        GuardedRow {
            table: "chat_turns",
            key: "id = ? AND thread_id = ?",
            key_values: &[
                StateWriteValue::Text(turn_id),
                StateWriteValue::Text(thread_id),
            ],
        },
        to,
        columns,
    )
    .await
}

/// Moves one thread to `to` when it is a legal move from its current state.
pub async fn persist_thread_state(
    connection: &mut SqliteConnection,
    thread_id: &str,
    to: ChatThreadState,
) -> ChatResult<StateWrite<ChatThreadState>> {
    persist_guarded_transition(
        connection,
        GuardedRow {
            table: "chat_threads",
            key: "id = ?",
            key_values: &[StateWriteValue::Text(thread_id)],
        },
        to,
        StateWriteColumns::default(),
    )
    .await
}

/// Moves one work assignment to `to` when the assignment table allows it, writing `columns`
/// with the move. The caller writes the settle time and revision through `columns`.
pub async fn persist_assignment_transition(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    to: ChatWorkAssignmentState,
    columns: StateWriteColumns<'_>,
) -> ChatResult<StateWrite<ChatWorkAssignmentState>> {
    persist_guarded_transition(
        connection,
        GuardedRow {
            table: "chat_work_assignments",
            key: "id = ?",
            key_values: &[StateWriteValue::Text(assignment_id)],
        },
        to,
        columns,
    )
    .await
}

fn parse_state<S: StoredState>(value: &str) -> ChatResult<S> {
    S::STATES
        .iter()
        .copied()
        .find(|state| state.stored() == value)
        .ok_or_else(corrupt_state_error)
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat persistence operation failed",
        true,
    )
}

fn corrupt_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat state is invalid",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ChatWorkAssignmentState::{
        Cancelled, Completed, Failed, Queued, ReadyForReview, WaitingForAnswer, WaitingForApproval,
        Working,
    };

    #[test]
    fn narrowed_sources_keep_each_writer_guard_unchanged() {
        let narrowed =
            |to, admits: fn(ChatWorkAssignmentState) -> bool| narrowed_sources_sql(to, admits);
        assert_eq!(narrowed(WaitingForAnswer, |s| s == Working), "'working'");
        assert_eq!(
            narrowed(Working, |s| s == WaitingForAnswer),
            "'waiting_for_answer'"
        );
        assert_eq!(
            narrowed(Queued, |s| s == WaitingForAnswer),
            "'waiting_for_answer'"
        );
        assert_eq!(narrowed(Failed, |s| s == Queued), "'queued'");
        assert_eq!(
            narrowed(Completed, |s| s == ReadyForReview),
            "'ready_for_review'"
        );
        assert_eq!(
            narrowed(Cancelled, ChatWorkAssignmentState::is_active),
            "'queued', 'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review'"
        );
        for to in [Failed, Cancelled] {
            assert_eq!(
                narrowed(to, ChatWorkAssignmentState::accepts_continuation),
                "'queued', 'working', 'waiting_for_answer', 'waiting_for_approval'"
            );
        }
    }

    #[test]
    fn narrowed_sources_never_admit_a_move_the_table_forbids() {
        for to in ChatWorkAssignmentState::ALL {
            let narrowed = narrowed_sources_sql(to, |_| true);
            assert_eq!(narrowed, sources_sql(to));
            for from in ChatWorkAssignmentState::ALL {
                let listed = narrowed_sources_sql(to, |state| state == from);
                if can_transition_assignment(from, to) {
                    assert_eq!(listed, format!("'{}'", from.as_str()));
                } else {
                    assert_eq!(listed, "NULL", "{from:?} to {to:?} must stay rejected");
                }
            }
        }
        assert_eq!(narrowed_sources_sql(Working, |s| s == Completed), "NULL");
        assert_eq!(
            narrowed_sources_sql(Queued, |s| s == WaitingForApproval),
            "NULL"
        );
        assert_eq!(narrowed_sources_sql(Completed, |s| s == Cancelled), "NULL");
    }
}
