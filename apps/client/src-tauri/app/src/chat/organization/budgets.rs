//! Assignment budgets: the token and working time limits one assignment runs under.
//!
//! A new assignment takes, when a teammate delegated it, the
//! delegating parent's remaining amounts or the child teammate's default when that is lower,
//! else the limits of the assignment of the same teammate it follows, else its teammate's
//! default token limit. The row id derives from the assignment, so devices that set one
//! assignment's budget concurrently write one row. A person can set or clear the limits of any
//! assignment, including work a limit stopped, so Raise limit before Retry works; the used
//! amounts stay, since only the executing device writes them and they only grow.

use super::{
    LOCAL_PARTICIPANT_ID, chat_pool, i64_value, identifier_error, persistence_error, timestamp,
    u64_value,
};
use ganbaru_chat::coordination::contracts::ChatAssignmentBudgetInput;
use ganbaru_chat::ids::derived_id;
use ganbaru_chat_contracts::models::*;
use ganbaru_sync_contracts::chat_usage::{self, ASSIGNMENT_BUDGET_KEY_PREFIX};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteConnection, SqlitePool, Transaction};

const MIN_TOKEN_LIMIT: u64 = chat_usage::MIN_TOKEN_LIMIT.cast_unsigned();
const MAX_TOKEN_LIMIT: u64 = chat_usage::MAX_TOKEN_LIMIT.cast_unsigned();
const MIN_WORKING_SECONDS_LIMIT: u64 = chat_usage::MIN_WORKING_SECONDS_LIMIT.cast_unsigned();
const MAX_WORKING_SECONDS_LIMIT: u64 = chat_usage::MAX_WORKING_SECONDS_LIMIT.cast_unsigned();

/// Columns of one budget row, shared by every read here.
const BUDGET_ROW_SELECT: &str = "SELECT assignment_id, token_limit, working_seconds_limit,
        set_by_participant_id, set_at, used_counted_tokens, used_working_seconds
     FROM chat_assignment_budgets";

/// The limits of one budget; a stored budget sets at least one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct BudgetLimits {
    pub(super) token_limit: Option<u64>,
    pub(super) working_seconds_limit: Option<u64>,
}

impl BudgetLimits {
    fn is_unset(self) -> bool {
        self.token_limit.is_none() && self.working_seconds_limit.is_none()
    }
}

impl From<&ChatAssignmentBudgetInput> for BudgetLimits {
    fn from(budget: &ChatAssignmentBudgetInput) -> Self {
        Self {
            token_limit: budget.token_limit,
            working_seconds_limit: budget.working_seconds_limit,
        }
    }
}

/// Validates a token limit against the bounds the budget table checks.
fn validate_token_limit(field: &str, limit: u64) -> ChatResult<()> {
    if (MIN_TOKEN_LIMIT..=MAX_TOKEN_LIMIT).contains(&limit) {
        return Ok(());
    }
    Err(ChatError::validation(
        field,
        format!("A token limit is between {MIN_TOKEN_LIMIT} and {MAX_TOKEN_LIMIT}"),
    ))
}

/// Validates `budget`: at least one limit, each within the bounds the budget table checks.
pub(super) fn validate_budget(budget: &ChatAssignmentBudgetInput) -> ChatResult<()> {
    if BudgetLimits::from(budget).is_unset() {
        return Err(ChatError::validation(
            "budget",
            "Set a token limit, a working time limit, or both",
        ));
    }
    if let Some(limit) = budget.token_limit {
        validate_token_limit("budget.tokenLimit", limit)?;
    }
    if let Some(limit) = budget.working_seconds_limit {
        if !(MIN_WORKING_SECONDS_LIMIT..=MAX_WORKING_SECONDS_LIMIT).contains(&limit) {
            return Err(ChatError::validation(
                "budget.workingSecondsLimit",
                format!(
                    "A working time limit is between {MIN_WORKING_SECONDS_LIMIT} and \
                     {MAX_WORKING_SECONDS_LIMIT} seconds"
                ),
            ));
        }
    }
    Ok(())
}

/// Validates a teammate's default assignment token limit; `None` sets none.
#[cfg(desktop)]
pub(super) fn validate_default_token_limit(limit: Option<u64>) -> ChatResult<()> {
    limit.map_or(Ok(()), |limit| {
        validate_token_limit("policy.defaultAssignmentTokenLimit", limit)
    })
}

/// The budget row id of `assignment_id`, derived so every device writes the same row.
fn budget_id(assignment_id: &ChatWorkAssignmentId) -> ChatResult<String> {
    derived_id(ASSIGNMENT_BUDGET_KEY_PREFIX, &[assignment_id.as_str()])
}

/// What is left of `limit` after `used`, floored at `minimum` so a child always has room to
/// report back.
fn remaining(limit: Option<u64>, used: u64, minimum: u64) -> Option<u64> {
    limit.map(|limit| limit.saturating_sub(used).max(minimum))
}

/// The lower of two optional limits, where `None` is no limit.
fn lower(first: Option<u64>, second: Option<u64>) -> Option<u64> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (limit, None) | (None, limit) => limit,
    }
}

/// The limits of a delegated child: what its parent has left, or the child teammate's default
/// when that is lower. A parent without a budget leaves only the default.
fn delegated_child_limits(
    parent_remaining: Option<BudgetLimits>,
    child_default: BudgetLimits,
) -> BudgetLimits {
    let parent = parent_remaining.unwrap_or_default();
    BudgetLimits {
        token_limit: lower(parent.token_limit, child_default.token_limit),
        working_seconds_limit: lower(
            parent.working_seconds_limit,
            child_default.working_seconds_limit,
        ),
    }
}

fn optional_u64(row: &SqliteRow, column: &str) -> ChatResult<Option<u64>> {
    row.try_get::<Option<i64>, _>(column)
        .map_err(persistence_error)?
        .map(u64_value)
        .transpose()
}

fn budget_read(row: &SqliteRow) -> ChatResult<ChatAssignmentBudgetRead> {
    Ok(ChatAssignmentBudgetRead {
        assignment_id: ChatWorkAssignmentId::new(
            row.try_get::<String, _>("assignment_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        token_limit: optional_u64(row, "token_limit")?,
        working_seconds_limit: optional_u64(row, "working_seconds_limit")?,
        set_by_participant_id: row
            .try_get::<Option<String>, _>("set_by_participant_id")
            .map_err(persistence_error)?
            .map(ChatParticipantId::new)
            .transpose()
            .map_err(identifier_error)?,
        set_at: timestamp(row.try_get("set_at").map_err(persistence_error)?)?,
        used_counted_tokens: u64_value(
            row.try_get("used_counted_tokens")
                .map_err(persistence_error)?,
        )?,
        used_working_seconds: u64_value(
            row.try_get("used_working_seconds")
                .map_err(persistence_error)?,
        )?,
    })
}

/// Reads the budget of `assignment_id`, if it has one.
pub(super) async fn read_assignment_budget(
    connection: &mut SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Option<ChatAssignmentBudgetRead>> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "{BUDGET_ROW_SELECT} WHERE assignment_id = ?"
    )))
    .bind(assignment_id.as_str())
    .fetch_optional(connection)
    .await
    .map_err(persistence_error)?
    .as_ref()
    .map(budget_read)
    .transpose()
}

/// The default limits of a new assignment of `teammate_id`, from its effective policy.
async fn teammate_default_limits(
    transaction: &mut Transaction<'_, Sqlite>,
    teammate_id: &ChatParticipantId,
) -> ChatResult<BudgetLimits> {
    let token_limit = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT revision.default_assignment_token_limit
         FROM chat_teammate_effective_policies policy
         JOIN chat_teammate_policy_revisions revision ON revision.id = policy.id
         WHERE policy.teammate_id = ?",
    )
    .bind(teammate_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?
    .flatten()
    .map(u64_value)
    .transpose()?;
    Ok(BudgetLimits {
        token_limit,
        working_seconds_limit: None,
    })
}

/// The limits of the assignment `previous_id` when it is work of `teammate_id`, `None` inside
/// when it had no budget, or `None` when it is another teammate's work.
async fn followed_limits(
    transaction: &mut Transaction<'_, Sqlite>,
    previous_id: &ChatWorkAssignmentId,
    teammate_id: &ChatParticipantId,
) -> ChatResult<Option<(BudgetLimits, Option<String>)>> {
    let row = sqlx::query(
        "SELECT budget.token_limit, budget.working_seconds_limit, budget.set_by_participant_id
         FROM chat_work_assignments assignment
         LEFT JOIN chat_assignment_budgets budget ON budget.assignment_id = assignment.id
         WHERE assignment.id = ? AND assignment.teammate_id = ?",
    )
    .bind(previous_id.as_str())
    .bind(teammate_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let limits = BudgetLimits {
        token_limit: optional_u64(&row, "token_limit")?,
        working_seconds_limit: optional_u64(&row, "working_seconds_limit")?,
    };
    let set_by = row
        .try_get::<Option<String>, _>("set_by_participant_id")
        .map_err(persistence_error)?;
    Ok(Some((limits, set_by)))
}

/// Where the budget of a new assignment comes from.
pub(super) struct NewAssignmentBudget<'a> {
    pub(super) assignment_id: &'a ChatWorkAssignmentId,
    pub(super) teammate_id: &'a ChatParticipantId,
    /// The assignment that delegated this one, when a teammate delegated it.
    pub(super) delegating_parent_id: Option<&'a ChatWorkAssignmentId>,
    /// The assignment this one follows, whose limits a continuation or retry keeps.
    pub(super) previous_id: Option<&'a ChatWorkAssignmentId>,
}

/// Writes the budget row of a new assignment from the first source that applies: a delegating
/// parent clamped by the child default, the followed work of the same
/// teammate, then the teammate default. Nothing is written when no source sets a limit.
pub(super) async fn write_new_assignment_budget(
    transaction: &mut Transaction<'_, Sqlite>,
    budget: NewAssignmentBudget<'_>,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let followed = match (budget.delegating_parent_id, budget.previous_id) {
        (None, Some(previous_id)) => {
            followed_limits(transaction, previous_id, budget.teammate_id).await?
        }
        _ => None,
    };
    let (limits, set_by) = if let Some(parent_id) = budget.delegating_parent_id {
        let parent = read_assignment_budget(transaction, parent_id).await?;
        let parent_remaining = parent.map(|parent| BudgetLimits {
            token_limit: remaining(
                parent.token_limit,
                parent.used_counted_tokens,
                MIN_TOKEN_LIMIT,
            ),
            working_seconds_limit: remaining(
                parent.working_seconds_limit,
                parent.used_working_seconds,
                MIN_WORKING_SECONDS_LIMIT,
            ),
        });
        let child_default = teammate_default_limits(transaction, budget.teammate_id).await?;
        (
            delegated_child_limits(parent_remaining, child_default),
            None,
        )
    } else if let Some(followed) = followed {
        followed
    } else {
        (
            teammate_default_limits(transaction, budget.teammate_id).await?,
            None,
        )
    };
    if limits.is_unset() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO chat_assignment_budgets
            (id, assignment_id, token_limit, working_seconds_limit, set_by_participant_id, set_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(budget_id(budget.assignment_id)?)
    .bind(budget.assignment_id.as_str())
    .bind(limits.token_limit.map(i64_value).transpose()?)
    .bind(limits.working_seconds_limit.map(i64_value).transpose()?)
    .bind(set_by)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn assignment_not_found() -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, "Assignment was not found", false)
}

async fn require_assignment(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    sqlx::query_scalar::<_, i64>("SELECT 1 FROM chat_work_assignments WHERE id = ?")
        .bind(assignment_id.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(persistence_error)?
        .map(|_| ())
        .ok_or_else(assignment_not_found)
}

/// Sets the limits of `assignment_id` as the person here, keeping what it already used. Setting
/// the limits it already has as the person changes nothing.
pub(super) async fn set_assignment_budget(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    budget: &ChatAssignmentBudgetInput,
    now: &UtcTimestamp,
) -> ChatResult<ChatAssignmentBudgetRead> {
    validate_budget(budget)?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    require_assignment(&mut transaction, assignment_id).await?;
    sqlx::query(
        "INSERT INTO chat_assignment_budgets
            (id, assignment_id, token_limit, working_seconds_limit, set_by_participant_id, set_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            token_limit = excluded.token_limit,
            working_seconds_limit = excluded.working_seconds_limit,
            set_by_participant_id = excluded.set_by_participant_id,
            set_at = excluded.set_at
         WHERE token_limit IS NOT excluded.token_limit
            OR working_seconds_limit IS NOT excluded.working_seconds_limit
            OR set_by_participant_id IS NOT excluded.set_by_participant_id",
    )
    .bind(budget_id(assignment_id)?)
    .bind(assignment_id.as_str())
    .bind(budget.token_limit.map(i64_value).transpose()?)
    .bind(budget.working_seconds_limit.map(i64_value).transpose()?)
    .bind(LOCAL_PARTICIPANT_ID)
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let read = read_assignment_budget(&mut transaction, assignment_id)
        .await?
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "The assignment budget was not saved",
                true,
            )
        })?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(read)
}

/// Clears the budget of `assignment_id`, so its work runs without a limit.
pub(super) async fn clear_assignment_budget(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    require_assignment(&mut transaction, assignment_id).await?;
    sqlx::query("DELETE FROM chat_assignment_budgets WHERE assignment_id = ?")
        .bind(assignment_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn set_assignment_budget_command(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    budget: ChatAssignmentBudgetInput,
) -> ChatResult<ChatAssignmentBudgetRead> {
    let pool = chat_pool(app, db_url).await?;
    set_assignment_budget(&pool, &assignment_id, &budget, &super::now_timestamp()?).await
}

pub(super) async fn clear_assignment_budget_command(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
) -> ChatResult<()> {
    let pool = chat_pool(app, db_url).await?;
    clear_assignment_budget(&pool, &assignment_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_remaining_is_floored_at_the_minimum() {
        assert_eq!(
            remaining(Some(20_000), 5_000, MIN_TOKEN_LIMIT),
            Some(15_000)
        );
        assert_eq!(
            remaining(Some(20_000), 19_500, MIN_TOKEN_LIMIT),
            Some(1_000)
        );
        assert_eq!(
            remaining(Some(20_000), 30_000, MIN_TOKEN_LIMIT),
            Some(1_000)
        );
        assert_eq!(remaining(None, 30_000, MIN_TOKEN_LIMIT), None);
    }

    #[test]
    fn delegated_child_budget_takes_the_lower_of_parent_remaining_and_default() {
        let default = BudgetLimits {
            token_limit: Some(8_000),
            working_seconds_limit: None,
        };
        let parent = BudgetLimits {
            token_limit: Some(5_000),
            working_seconds_limit: Some(600),
        };
        assert_eq!(delegated_child_limits(Some(parent), default), parent);
        let roomy = BudgetLimits {
            token_limit: Some(50_000),
            working_seconds_limit: None,
        };
        assert_eq!(delegated_child_limits(Some(roomy), default), default);
        assert_eq!(delegated_child_limits(None, default), default);
        assert!(delegated_child_limits(None, BudgetLimits::default()).is_unset());
    }

    #[test]
    fn budget_validation_needs_a_bounded_limit() {
        let budget = |token_limit, working_seconds_limit| ChatAssignmentBudgetInput {
            token_limit,
            working_seconds_limit,
        };
        assert!(validate_budget(&budget(None, None)).is_err());
        assert!(validate_budget(&budget(Some(MIN_TOKEN_LIMIT - 1), None)).is_err());
        assert!(validate_budget(&budget(Some(MAX_TOKEN_LIMIT + 1), None)).is_err());
        assert!(validate_budget(&budget(None, Some(MIN_WORKING_SECONDS_LIMIT - 1))).is_err());
        assert!(validate_budget(&budget(None, Some(MAX_WORKING_SECONDS_LIMIT + 1))).is_err());
        assert!(validate_budget(&budget(Some(MIN_TOKEN_LIMIT), None)).is_ok());
        assert!(validate_budget(&budget(None, Some(MIN_WORKING_SECONDS_LIMIT))).is_ok());
        assert!(validate_default_token_limit(None).is_ok());
        assert!(validate_default_token_limit(Some(MIN_TOKEN_LIMIT - 1)).is_err());
    }
}
