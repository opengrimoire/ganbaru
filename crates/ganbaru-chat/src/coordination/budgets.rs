//! Enforcement of assignment budgets on the device that executes the work.
//!
//! The executing device compares what an assignment's runs counted here with the limits of its
//! replicated budget row. It records the used amounts on the row only when they cross a warning
//! or stop threshold and when a run settles, so the row changes a handful of times per
//! assignment. Reaching a limit fails the assignment with [`BUDGET_LIMIT_REACHED_REASON`] and one
//! failure update; the caller then stops the provider session after commit.

use super::super::agent_runs::unsettled_run_state_sql;
use super::delegation::post_delegation_outcome;
use super::projection::update_assignment_and_run_state;
use super::reports::{TeammateUpdate, insert_teammate_update};
use crate::ids::derived_id;
use crate::models::{ChatError, ChatErrorCode, ChatResult, ChatWorkAssignmentId};
use crate::usage_counters::{BudgetThreshold, budget_threshold};
use ganbaru_sync_contracts::chat_usage::{MAX_TOKEN_LIMIT, MAX_WORKING_SECONDS_LIMIT};
use serde_json::json;
use sqlx::{Row, SqliteConnection, SqlitePool};

/// State reason of an assignment its executing device stopped at a budget limit.
pub const BUDGET_LIMIT_REACHED_REASON: &str = "budget_limit_reached";
/// State reason of an assignment whose run ended because the provider's usage limit was reached.
pub const PROVIDER_USAGE_LIMIT_REASON: &str = "provider_usage_limit";
/// Seconds between working time checks while a budgeted run is working, since working time
/// passes without provider events.
pub const BUDGET_TICK_SECONDS: u64 = 30;

const MAX_RECORDED_TOKENS: u64 = MAX_TOKEN_LIMIT.cast_unsigned();
const MAX_RECORDED_WORKING_SECONDS: u64 = MAX_WORKING_SECONDS_LIMIT.cast_unsigned();

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

fn stored_amount_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat budget amount is invalid",
        false,
    )
}

/// The limit of a budget that usage reached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetLimit {
    Tokens,
    WorkingTime,
}

impl BudgetLimit {
    /// The name the failure payload uses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tokens => "tokens",
            Self::WorkingTime => "working_time",
        }
    }
}

/// What one budget allows, what its row records, and what this device counted for it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BudgetUsage {
    pub token_limit: Option<u64>,
    pub working_seconds_limit: Option<u64>,
    pub recorded_tokens: u64,
    pub recorded_working_seconds: u64,
    /// Counted tokens of every run of the assignment on this device.
    pub counted_tokens: u64,
    /// Working seconds of every run of the assignment on this device, including the open
    /// interval of a run that is working now.
    pub working_seconds: u64,
}

/// A limit that usage reached, with the amounts the failure update reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReachedLimit {
    pub limit: BudgetLimit,
    pub used: u64,
    pub limit_value: u64,
}

/// What a budget check writes and whether it stops the work.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BudgetDecision {
    /// Used tokens and working seconds to record, when the row should change.
    pub record: Option<(u64, u64)>,
    /// The first limit usage reached: tokens before working time.
    pub reached: Option<ReachedLimit>,
}

/// When a budget check runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetCheck {
    /// While the run may still spend: records a crossed threshold and stops at a limit.
    Running,
    /// After the run settled: records any increase and never stops.
    Settled,
}

impl BudgetUsage {
    /// Used tokens, never below what the row records and never above the recordable bound.
    pub fn used_tokens(&self) -> u64 {
        self.counted_tokens
            .max(self.recorded_tokens)
            .min(MAX_RECORDED_TOKENS)
    }

    /// Used working seconds, never below what the row records and never above the recordable
    /// bound.
    pub fn used_working_seconds(&self) -> u64 {
        self.working_seconds
            .max(self.recorded_working_seconds)
            .min(MAX_RECORDED_WORKING_SECONDS)
    }

    /// The first limit the used amounts reached, tokens before working time.
    pub fn reached(&self) -> Option<ReachedLimit> {
        let tokens = self.token_limit.map(|limit| ReachedLimit {
            limit: BudgetLimit::Tokens,
            used: self.used_tokens(),
            limit_value: limit,
        });
        let working_time = self.working_seconds_limit.map(|limit| ReachedLimit {
            limit: BudgetLimit::WorkingTime,
            used: self.used_working_seconds(),
            limit_value: limit,
        });
        [tokens, working_time]
            .into_iter()
            .flatten()
            .find(|reached| {
                budget_threshold(reached.used, reached.limit_value) == BudgetThreshold::Reached
            })
    }

    /// Decides what a check of `kind` records and whether it stops the work.
    pub fn decide(&self, kind: BudgetCheck) -> BudgetDecision {
        let tokens = self.used_tokens();
        let seconds = self.used_working_seconds();
        let crossed = |limit: Option<u64>, recorded: u64, used: u64| {
            limit.is_some_and(|limit| {
                budget_threshold(used, limit) > budget_threshold(recorded, limit)
            })
        };
        let increased = tokens > self.recorded_tokens || seconds > self.recorded_working_seconds;
        let record = match kind {
            BudgetCheck::Running => {
                crossed(self.token_limit, self.recorded_tokens, tokens)
                    || crossed(
                        self.working_seconds_limit,
                        self.recorded_working_seconds,
                        seconds,
                    )
            }
            BudgetCheck::Settled => increased,
        };
        BudgetDecision {
            record: record.then_some((tokens, seconds)),
            reached: match kind {
                BudgetCheck::Running => self.reached(),
                BudgetCheck::Settled => None,
            },
        }
    }
}

fn amount(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<u64> {
    let value: i64 = row.try_get(column).map_err(persistence_error)?;
    u64::try_from(value).map_err(|_| stored_amount_error())
}

fn optional_amount(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<Option<u64>> {
    let value: Option<i64> = row.try_get(column).map_err(persistence_error)?;
    value
        .map(|value| u64::try_from(value).map_err(|_| stored_amount_error()))
        .transpose()
}

/// Reads the budget of `assignment_id` with this device's usage of its runs at `now`, or `None`
/// when the assignment has no budget.
pub async fn read_budget_usage(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    now: &str,
) -> ChatResult<Option<BudgetUsage>> {
    let row = sqlx::query(
        "SELECT budget.token_limit, budget.working_seconds_limit,
                budget.used_counted_tokens, budget.used_working_seconds,
                coalesce(sum(usage.counted_tokens), 0) AS counted_tokens,
                coalesce(sum(usage.working_seconds + CASE
                    WHEN usage.working_since IS NULL THEN 0
                    ELSE coalesce(max(0, CAST(round(
                        (julianday(?) - julianday(usage.working_since)) * 86400
                    ) AS INTEGER)), 0)
                END), 0) AS working_seconds
         FROM chat_assignment_budgets budget
         LEFT JOIN chat_agent_runs run ON run.assignment_id = budget.assignment_id
         LEFT JOIN chat_agent_run_usage usage ON usage.run_id = run.id
         WHERE budget.assignment_id = ?
         GROUP BY budget.id",
    )
    .bind(now)
    .bind(assignment_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(BudgetUsage {
        token_limit: optional_amount(&row, "token_limit")?,
        working_seconds_limit: optional_amount(&row, "working_seconds_limit")?,
        recorded_tokens: amount(&row, "used_counted_tokens")?,
        recorded_working_seconds: amount(&row, "used_working_seconds")?,
        counted_tokens: amount(&row, "counted_tokens")?,
        working_seconds: amount(&row, "working_seconds")?,
    }))
}

/// Whether the budget of `assignment_id` has reached a limit at `now`.
pub async fn assignment_budget_reached(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    now: &str,
) -> ChatResult<bool> {
    Ok(read_budget_usage(connection, assignment_id, now)
        .await?
        .and_then(|usage| usage.reached())
        .is_some())
}

/// Records `tokens` and `seconds` as used on the budget of `assignment_id`. Used amounts only
/// grow, so a lower value never replaces a higher one.
async fn record_used(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    (tokens, seconds): (u64, u64),
) -> ChatResult<()> {
    let tokens = i64::try_from(tokens).map_err(|_| stored_amount_error())?;
    let seconds = i64::try_from(seconds).map_err(|_| stored_amount_error())?;
    sqlx::query(
        "UPDATE chat_assignment_budgets
         SET used_counted_tokens = max(used_counted_tokens, ?),
             used_working_seconds = max(used_working_seconds, ?)
         WHERE assignment_id = ?
           AND (used_counted_tokens < ? OR used_working_seconds < ?)",
    )
    .bind(tokens)
    .bind(seconds)
    .bind(assignment_id)
    .bind(tokens)
    .bind(seconds)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Checks the budget of `assignment_id`, whose run `run_id` executes here, at `now`.
///
/// Records the used amounts as [`BudgetUsage::decide`] says. A [`BudgetCheck::Running`] check
/// that finds a limit reached fails the assignment and its run with
/// [`BUDGET_LIMIT_REACHED_REASON`], posts one failure update for the run, queues the delegation
/// outcome, and returns the assignment so the caller stops its provider session after commit.
/// Work that already settled is never reopened or failed again.
pub async fn check_assignment_budget(
    connection: &mut SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
    run_id: &str,
    now: &str,
    kind: BudgetCheck,
) -> ChatResult<Option<ChatWorkAssignmentId>> {
    let Some(usage) = read_budget_usage(connection, assignment_id.as_str(), now).await? else {
        return Ok(None);
    };
    let decision = usage.decide(kind);
    if let Some(record) = decision.record {
        record_used(connection, assignment_id.as_str(), record).await?;
    }
    let Some(reached) = decision.reached else {
        return Ok(None);
    };
    if !stop_at_limit(connection, assignment_id, run_id, now, reached).await? {
        return Ok(None);
    }
    // Leaving `working` closed the run's open interval, so the settle record counts all of it.
    if let Some(settled) = read_budget_usage(connection, assignment_id.as_str(), now).await?
        && let Some(record) = settled.decide(BudgetCheck::Settled).record
    {
        record_used(connection, assignment_id.as_str(), record).await?;
    }
    Ok(Some(assignment_id.clone()))
}

/// Fails the assignment and its run at the reached limit and posts the failure update, reporting
/// whether the assignment was still active.
async fn stop_at_limit(
    connection: &mut SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
    run_id: &str,
    now: &str,
    reached: ReachedLimit,
) -> ChatResult<bool> {
    let target =
        sqlx::query("SELECT reply_thread_id, teammate_id FROM chat_work_assignments WHERE id = ?")
            .bind(assignment_id.as_str())
            .fetch_optional(&mut *connection)
            .await
            .map_err(persistence_error)?;
    let Some(target) = target else {
        return Ok(false);
    };
    if !update_assignment_and_run_state(
        connection,
        assignment_id,
        run_id,
        "failed",
        "failed",
        Some(BUDGET_LIMIT_REACHED_REASON),
        now,
    )
    .await?
    {
        return Ok(false);
    }
    let reply_thread_id: String = target
        .try_get("reply_thread_id")
        .map_err(persistence_error)?;
    let teammate_id: String = target.try_get("teammate_id").map_err(persistence_error)?;
    // One stop per run: a later run of the same assignment, after a raised limit and a retry,
    // posts its own.
    let parts = ["budget-stop", run_id];
    let markdown = match reached.limit {
        BudgetLimit::Tokens => "Stopped at the token limit set for this work.",
        BudgetLimit::WorkingTime => "Stopped at the working time limit set for this work.",
    };
    insert_teammate_update(
        connection,
        &TeammateUpdate {
            item_id: &derived_id("conversation-item", &parts)?,
            revision_id: &derived_id("message-revision", &parts)?,
            reply_thread_id: &reply_thread_id,
            teammate_id: &teammate_id,
            assignment_id: assignment_id.as_str(),
            run_id: Some(run_id),
            update_kind: "failure",
            markdown,
            payload: json!({
                "code": BUDGET_LIMIT_REACHED_REASON,
                "limit": reached.limit.as_str(),
                "used": reached.used,
                "limitValue": reached.limit_value,
            }),
            created_at: now,
        },
    )
    .await?;
    post_delegation_outcome(connection, assignment_id.as_str(), now).await?;
    Ok(true)
}

/// Checks the budgets of the work this device runs at `now`: working runs with a budget are
/// checked as running, so working time stops at its limit without a provider event, and work
/// whose runs all settled records any usage its row does not show yet. Returns the assignments
/// it stopped, whose provider sessions the caller stops.
pub async fn check_local_budgets(
    pool: &SqlitePool,
    now: &str,
) -> ChatResult<Vec<ChatWorkAssignmentId>> {
    let sql = format!(
        "SELECT run.assignment_id, run.id, run.state = 'working' AND usage.working_since IS NOT NULL
         FROM chat_assignment_budgets budget
         JOIN chat_agent_runs run ON run.assignment_id = budget.assignment_id
         JOIN chat_agent_run_usage usage ON usage.run_id = run.id
         WHERE run.run_ordinal = (
             SELECT max(latest.run_ordinal) FROM chat_agent_runs latest
             WHERE latest.assignment_id = run.assignment_id
         )
           AND (
             (run.state = 'working' AND usage.working_since IS NOT NULL)
             OR (NOT ({settled}) AND (
                 budget.used_counted_tokens < (
                     SELECT coalesce(sum(counted.counted_tokens), 0)
                     FROM chat_agent_run_usage counted
                     JOIN chat_agent_runs counted_run ON counted_run.id = counted.run_id
                     WHERE counted_run.assignment_id = budget.assignment_id
                 )
                 OR budget.used_working_seconds < (
                     SELECT coalesce(sum(counted.working_seconds), 0)
                     FROM chat_agent_run_usage counted
                     JOIN chat_agent_runs counted_run ON counted_run.id = counted.run_id
                     WHERE counted_run.assignment_id = budget.assignment_id
                 )
             ))
           )",
        settled = unsettled_run_state_sql("run.state")
    );
    let candidates: Vec<(String, String, bool)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
    let mut stopped = Vec::new();
    for (assignment_id, run_id, working) in candidates {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(|_| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "Stored Chat identifier is invalid",
                false,
            )
        })?;
        let kind = if working {
            BudgetCheck::Running
        } else {
            BudgetCheck::Settled
        };
        let mut transaction = pool.begin().await.map_err(persistence_error)?;
        let stop =
            check_assignment_budget(&mut transaction, &assignment_id, &run_id, now, kind).await?;
        transaction.commit().await.map_err(persistence_error)?;
        stopped.extend(stop);
    }
    Ok(stopped)
}

#[cfg(test)]
mod tests;
