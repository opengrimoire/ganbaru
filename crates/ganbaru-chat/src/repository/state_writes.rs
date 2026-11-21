//! Guarded writers for persisted Chat turn and thread states.
//!
//! Every write of `chat_turns.state` and `chat_threads.state` names the target state and lets
//! [`crate::state`] decide the legal sources, so SQLite applies only moves the transition
//! tables allow. A write that changes no row is classified instead of being treated as success.

use crate::models::{ChatError, ChatErrorCode, ChatResult, ChatThreadState, ChatTurnState};
use crate::state::{thread_sources, turn_sources};
use sqlx::SqliteConnection;

/// The result of a guarded state write.
///
/// Provider events repeat and arrive late (a duplicate completion, a start replayed by a
/// projection rebuild over turns that already settled), so callers treat `AlreadyInState` and
/// `Rejected` as normal outcomes that leave the row untouched, not as pipeline failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateWrite<S> {
    /// The row moved to the target state.
    Applied,
    /// The row was already in the target state, so nothing changed.
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

/// A value bound to a caller supplied column assignment, in assignment order.
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

/// Returns the quoted SQL list of turn states legal as sources for a move to `to`.
pub fn turn_sources_sql(to: ChatTurnState) -> String {
    sql_list(turn_sources(to).map(ChatTurnState::as_str))
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
    sql_list(thread_sources(to).map(ChatThreadState::as_str))
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

/// Moves one turn to `to` when the transition table allows it from its current state, writing
/// `columns` in the same statement only when the move applies.
pub async fn persist_turn_transition(
    connection: &mut SqliteConnection,
    thread_id: &str,
    turn_id: &str,
    to: ChatTurnState,
    columns: StateWriteColumns<'_>,
) -> ChatResult<StateWrite<ChatTurnState>> {
    let extra = if columns.assignments.is_empty() {
        String::new()
    } else {
        format!(", {}", columns.assignments)
    };
    let statement = format!(
        "UPDATE chat_turns SET state = '{}'{extra}
         WHERE id = ? AND thread_id = ? AND state IN ({})",
        to.as_str(),
        turn_sources_sql(to)
    );
    let mut query = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()));
    for value in columns.values {
        query = match *value {
            StateWriteValue::Text(value) => query.bind(value),
            StateWriteValue::OptionalText(value) => query.bind(value),
            StateWriteValue::OptionalInteger(value) => query.bind(value),
        };
    }
    let updated = query
        .bind(turn_id)
        .bind(thread_id)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    if updated.rows_affected() == 1 {
        return Ok(StateWrite::Applied);
    }
    let current: Option<String> =
        sqlx::query_scalar("SELECT state FROM chat_turns WHERE id = ? AND thread_id = ?")
            .bind(turn_id)
            .bind(thread_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(persistence_error)?;
    let Some(current) = current else {
        return Ok(StateWrite::Missing);
    };
    let current = parse_turn_state(&current)?;
    Ok(if current == to {
        StateWrite::AlreadyInState
    } else {
        StateWrite::Rejected { from: current }
    })
}

/// Moves one thread to `to` when it is a legal move from its current state.
pub async fn persist_thread_state(
    connection: &mut SqliteConnection,
    thread_id: &str,
    to: ChatThreadState,
) -> ChatResult<StateWrite<ChatThreadState>> {
    let statement = format!(
        "UPDATE chat_threads SET state = '{}' WHERE id = ? AND state IN ({})",
        to.as_str(),
        thread_sources_sql(to)
    );
    let updated = sqlx::query(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(thread_id)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    if updated.rows_affected() == 1 {
        return Ok(StateWrite::Applied);
    }
    let current: Option<String> = sqlx::query_scalar("SELECT state FROM chat_threads WHERE id = ?")
        .bind(thread_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(persistence_error)?;
    let Some(current) = current else {
        return Ok(StateWrite::Missing);
    };
    let current = parse_thread_state(&current)?;
    Ok(if current == to {
        StateWrite::AlreadyInState
    } else {
        StateWrite::Rejected { from: current }
    })
}

fn parse_turn_state(value: &str) -> ChatResult<ChatTurnState> {
    ChatTurnState::ALL
        .into_iter()
        .find(|state| state.as_str() == value)
        .ok_or_else(corrupt_state_error)
}

fn parse_thread_state(value: &str) -> ChatResult<ChatThreadState> {
    ChatThreadState::ALL
        .into_iter()
        .find(|state| state.as_str() == value)
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
