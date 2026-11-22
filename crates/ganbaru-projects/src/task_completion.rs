//! Completion of a project task. A task is completed exactly while its status is terminal, and
//! the status table ties the terminal flag to the done category. `completed_at` records when
//! the task last became completed and is kept while it stays completed.
//!
//! Every task writer moves completion through [`TaskCompletion::transition`], and every reader
//! decides completion through `task_completed_sql!`. A replica can merge a status flip with a
//! concurrent task move into that status, which may leave a stale or missing `completed_at`;
//! readers therefore never treat the timestamp alone as completion and read it through
//! `task_completed_at_sql!`, and the next transition repairs it.

use super::history::{current_timestamp, insert_task_field_change_event};
use super::task_commands::task_conflict;

/// SQL that is true when task `t` is completed: its status is terminal.
macro_rules! task_completed_sql {
    () => {
        "COALESCE((SELECT s.terminal FROM project_statuses s WHERE s.id = t.status_id), 0) = 1"
    };
}
pub(crate) use task_completed_sql;

/// SQL for the completion time of task `t`, null unless the task is completed.
macro_rules! task_completed_at_sql {
    () => {
        concat!(
            "CASE WHEN ",
            crate::task_completion::task_completed_sql!(),
            " THEN t.completed_at END"
        )
    };
}
pub(crate) use task_completed_at_sql;

/// The completion state of a task as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskCompletion {
    /// The status is not terminal.
    Open,
    /// The status is terminal. The time is missing only after a merge left it unset.
    Completed { at: Option<String> },
}

/// The history kind a status change records for its completion move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionChange {
    /// An open task became completed.
    Completed,
    /// A completed task became open.
    Reopened,
    /// Completion did not change.
    Kept,
}

impl CompletionChange {
    /// The `project_task_change_events.event_type` of a status change with this move.
    pub(crate) fn status_event_type(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Reopened => "reopened",
            Self::Kept => "status_changed",
        }
    }
}

/// The result of one completion transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionMove {
    /// The `completed_at` value to write.
    pub(crate) completed_at: Option<String>,
    /// What the move means for history.
    pub(crate) change: CompletionChange,
}

impl TaskCompletion {
    /// The completion of a task row whose status has the given terminal flag.
    pub(crate) fn of(terminal: bool, completed_at: Option<&str>) -> Self {
        if terminal {
            Self::Completed {
                at: completed_at.map(str::to_owned),
            }
        } else {
            Self::Open
        }
    }

    /// Moves completion to a status with the given terminal flag at time `now`. Every move is
    /// legal: becoming completed records `now`, staying completed keeps the recorded time (or
    /// records `now` when a merge left it missing), and becoming open clears it.
    pub(crate) fn transition(&self, next_terminal: bool, now: &str) -> CompletionMove {
        match (self, next_terminal) {
            (Self::Open, false) => CompletionMove {
                completed_at: None,
                change: CompletionChange::Kept,
            },
            (Self::Open, true) => CompletionMove {
                completed_at: Some(now.to_owned()),
                change: CompletionChange::Completed,
            },
            (Self::Completed { at }, true) => CompletionMove {
                completed_at: Some(at.clone().unwrap_or_else(|| now.to_owned())),
                change: CompletionChange::Kept,
            },
            (Self::Completed { .. }, false) => CompletionMove {
                completed_at: None,
                change: CompletionChange::Reopened,
            },
        }
    }
}

/// The completion move of a status change between statuses with these terminal flags.
pub(crate) fn completion_change(previous_terminal: bool, next_terminal: bool) -> CompletionChange {
    match (previous_terminal, next_terminal) {
        (false, true) => CompletionChange::Completed,
        (true, false) => CompletionChange::Reopened,
        _ => CompletionChange::Kept,
    }
}

/// Moves the completion of every task in a status whose terminal flag flipped, and returns
/// the ids of the tasks whose `completed_at` changed. Each moved task gets a new revision and a
/// completed or reopened history entry naming the status.
pub(crate) async fn move_status_task_completion(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    status_id: &str,
    previous_terminal: bool,
    previous_name: &str,
    next_terminal: bool,
    next_name: &str,
) -> Result<Vec<String>, String> {
    let now = current_timestamp(tx).await?;
    let tasks: Vec<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, completed_at, revision FROM project_tasks WHERE status_id = ? ORDER BY id",
    )
    .bind(status_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load status tasks: {e}"))?;
    let mut moved = Vec::new();
    for (task_id, completed_at, revision) in tasks {
        let completion = TaskCompletion::of(previous_terminal, completed_at.as_deref())
            .transition(next_terminal, &now);
        if completion.completed_at == completed_at {
            continue;
        }
        let updated = sqlx::query(
            "UPDATE project_tasks
             SET completed_at = ?, revision = revision + 1, updated_at = ?
             WHERE id = ? AND status_id = ? AND revision = ?",
        )
        .bind(&completion.completed_at)
        .bind(&now)
        .bind(&task_id)
        .bind(status_id)
        .bind(revision)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("move status task completion: {e}"))?;
        if updated.rows_affected() != 1 {
            return Err(task_conflict(&task_id));
        }
        if completion.change != CompletionChange::Kept {
            insert_task_field_change_event(
                tx,
                &task_id,
                completion.change.status_event_type(),
                "status",
                Some(previous_name.to_owned()),
                Some(next_name.to_owned()),
                None,
            )
            .await?;
        }
        moved.push(task_id);
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EARLIER: &str = "2026-11-01T09:00:00.000Z";
    const NOW: &str = "2026-11-21T12:00:00.000Z";

    #[test]
    fn every_completion_move_writes_the_time_its_target_state_requires() {
        let cases = [
            (TaskCompletion::Open, false, None, CompletionChange::Kept),
            (
                TaskCompletion::Open,
                true,
                Some(NOW),
                CompletionChange::Completed,
            ),
            (
                TaskCompletion::of(true, Some(EARLIER)),
                true,
                Some(EARLIER),
                CompletionChange::Kept,
            ),
            (
                TaskCompletion::of(true, None),
                true,
                Some(NOW),
                CompletionChange::Kept,
            ),
            (
                TaskCompletion::of(true, Some(EARLIER)),
                false,
                None,
                CompletionChange::Reopened,
            ),
            (
                TaskCompletion::of(true, None),
                false,
                None,
                CompletionChange::Reopened,
            ),
        ];
        for (from, next_terminal, completed_at, change) in cases {
            assert_eq!(
                from.transition(next_terminal, NOW),
                CompletionMove {
                    completed_at: completed_at.map(str::to_owned),
                    change,
                },
                "{from:?} to terminal={next_terminal}"
            );
        }
    }

    #[test]
    fn a_stale_time_on_an_open_status_is_not_completion() {
        assert_eq!(
            TaskCompletion::of(false, Some(EARLIER)),
            TaskCompletion::Open
        );
        assert_eq!(
            TaskCompletion::of(false, Some(EARLIER)).transition(true, NOW),
            CompletionMove {
                completed_at: Some(NOW.to_owned()),
                change: CompletionChange::Completed,
            }
        );
    }

    #[test]
    fn status_events_name_the_completion_move() {
        assert_eq!(CompletionChange::Completed.status_event_type(), "completed");
        assert_eq!(CompletionChange::Reopened.status_event_type(), "reopened");
        assert_eq!(CompletionChange::Kept.status_event_type(), "status_changed");
    }
}
