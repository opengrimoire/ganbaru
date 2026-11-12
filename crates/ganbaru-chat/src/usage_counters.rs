//! Usage counters of agent runs, kept on the device that executes them.
//!
//! Provider usage reports follow one convention (input excludes cache reads and writes, output
//! includes reasoning) and name what their counts cover. [`fold_usage`] turns each report into
//! the run's running counters; reports with no documented scope are never counted, since a
//! guessed count could stop work wrongly once budgets apply.

use crate::events::{ThreadUsageUpdatedEvent, UsageCountScope};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use sqlx::{Row, Sqlite, Transaction};

/// Token counts in the provider usage convention.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenCounts {
    /// Input tokens neither read from nor written to the prompt cache.
    pub input: u64,
    /// Input tokens read from the prompt cache.
    pub cached_input: u64,
    /// Input tokens written to the prompt cache.
    pub cache_write_input: u64,
    /// Output tokens, reasoning included.
    pub output: u64,
}

impl TokenCounts {
    /// Tokens that count toward usage: input, cache writes, and output. Cache reads are left
    /// out because they are cheap for every provider and would make long threads look costly.
    pub fn counted(&self) -> u64 {
        self.input
            .saturating_add(self.cache_write_input)
            .saturating_add(self.output)
    }

    /// The reported counts of `event`, with an unreported component taken from `fallback`.
    fn reported(event: &ThreadUsageUpdatedEvent, fallback: TokenCounts) -> Self {
        Self {
            input: event.input_tokens.unwrap_or(fallback.input),
            cached_input: event.cached_input_tokens.unwrap_or(fallback.cached_input),
            cache_write_input: event
                .cache_write_input_tokens
                .unwrap_or(fallback.cache_write_input),
            output: event.output_tokens.unwrap_or(fallback.output),
        }
    }

    fn components(&self) -> [u64; 4] {
        [
            self.input,
            self.cached_input,
            self.cache_write_input,
            self.output,
        ]
    }

    fn any_below(&self, other: &Self) -> bool {
        self.components()
            .iter()
            .zip(other.components())
            .any(|(value, floor)| *value < floor)
    }

    fn saturating_add(&self, other: &Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            cached_input: self.cached_input.saturating_add(other.cached_input),
            cache_write_input: self
                .cache_write_input
                .saturating_add(other.cache_write_input),
            output: self.output.saturating_add(other.output),
        }
    }

    fn saturating_sub(&self, other: &Self) -> Self {
        Self {
            input: self.input.saturating_sub(other.input),
            cached_input: self.cached_input.saturating_sub(other.cached_input),
            cache_write_input: self
                .cache_write_input
                .saturating_sub(other.cache_write_input),
            output: self.output.saturating_sub(other.output),
        }
    }

    fn max(&self, other: &Self) -> Self {
        Self {
            input: self.input.max(other.input),
            cached_input: self.cached_input.max(other.cached_input),
            cache_write_input: self.cache_write_input.max(other.cache_write_input),
            output: self.output.max(other.output),
        }
    }
}

/// A provider-attributed cost in one currency.
#[derive(Clone, Debug, PartialEq)]
pub struct RunCost {
    pub amount: f64,
    pub currency: String,
}

/// The thread totals a provider last reported for a thread-cumulative count.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThreadTotals {
    pub tokens: TokenCounts,
    pub cost: Option<f64>,
}

/// The running usage of one agent run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunUsage {
    pub tokens: TokenCounts,
    pub cost: Option<RunCost>,
    /// The last thread totals this run saw, set once the provider reports thread totals.
    pub thread_last: Option<ThreadTotals>,
}

/// Why a usage report was not counted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageCountError {
    /// The report does not say what its counts cover.
    UnknownScope,
    /// A thread total fell below the total this run already counted.
    TotalWentBackwards,
}

/// Whether `event` reports any token count.
pub fn reports_tokens(event: &ThreadUsageUpdatedEvent) -> bool {
    event.input_tokens.is_some()
        || event.cached_input_tokens.is_some()
        || event.cache_write_input_tokens.is_some()
        || event.output_tokens.is_some()
}

/// Whether a thread-cumulative `event` restarted the provider's count: its first report in a
/// run fell below the totals an earlier run on the same thread carried, as when a new provider
/// process starts its count again.
pub fn thread_count_restarted(
    current: &RunUsage,
    event: &ThreadUsageUpdatedEvent,
    carried: Option<&ThreadTotals>,
) -> bool {
    current.thread_last.is_none()
        && event.count_scope == Some(UsageCountScope::ThreadCumulative)
        && carried.is_some_and(|carried| {
            TokenCounts::reported(event, carried.tokens).any_below(&carried.tokens)
        })
}

/// Folds the usage report `event` into the run usage `current`.
///
/// - A report without token counts changes nothing, whatever its scope.
/// - `Request` adds the report.
/// - `Turn` is the running total of the turn the run executes, so each component keeps the
///   larger of the counted and reported values; a turn total that repeats the requests already
///   counted adds nothing.
/// - `ThreadCumulative` adds the increase over the last reported totals. The first report of a
///   run measures from `carried`, the totals an earlier run on the same provider thread saw,
///   unless the count restarted below them. A total below one this run already saw is refused.
///
/// Costs follow the same rules. A cost in another currency than the one counted, or one that is
/// not a finite non-negative amount, is left out.
pub fn fold_usage(
    current: &RunUsage,
    event: &ThreadUsageUpdatedEvent,
    carried: Option<&ThreadTotals>,
) -> Result<RunUsage, UsageCountError> {
    if !reports_tokens(event) {
        return Ok(current.clone());
    }
    let scope = event.count_scope.ok_or(UsageCountError::UnknownScope)?;
    let reported_cost = event
        .cost
        .as_ref()
        .filter(|cost| cost.amount.is_finite() && cost.amount >= 0.0);
    let mut next = current.clone();
    match scope {
        UsageCountScope::Request => {
            let reported = TokenCounts::reported(event, TokenCounts::default());
            next.tokens = current.tokens.saturating_add(&reported);
            if let Some(cost) = reported_cost {
                add_cost(&mut next.cost, cost.amount, &cost.currency);
            }
        }
        UsageCountScope::Turn => {
            let reported = TokenCounts::reported(event, TokenCounts::default());
            next.tokens = current.tokens.max(&reported);
            if let Some(cost) = reported_cost {
                match &mut next.cost {
                    Some(counted) if counted.currency == cost.currency => {
                        counted.amount = counted.amount.max(cost.amount);
                    }
                    Some(_) => {}
                    None => {
                        next.cost = Some(RunCost {
                            amount: cost.amount,
                            currency: cost.currency.clone(),
                        });
                    }
                }
            }
        }
        UsageCountScope::ThreadCumulative => {
            let baseline = match current.thread_last {
                Some(last) => {
                    let reported = TokenCounts::reported(event, last.tokens);
                    if reported.any_below(&last.tokens) {
                        return Err(UsageCountError::TotalWentBackwards);
                    }
                    last
                }
                None if thread_count_restarted(current, event, carried) => ThreadTotals::default(),
                None => carried.copied().unwrap_or_default(),
            };
            let reported = TokenCounts::reported(event, baseline.tokens);
            next.tokens = current
                .tokens
                .saturating_add(&reported.saturating_sub(&baseline.tokens));
            let mut reported_total = baseline.cost;
            if let Some(cost) = reported_cost {
                let increase = (cost.amount - baseline.cost.unwrap_or(0.0)).max(0.0);
                add_cost(&mut next.cost, increase, &cost.currency);
                reported_total = Some(cost.amount);
            }
            next.thread_last = Some(ThreadTotals {
                tokens: reported,
                cost: reported_total,
            });
        }
    }
    Ok(next)
}

/// Adds `amount` in `currency` to `cost`, leaving it unchanged for another currency.
fn add_cost(cost: &mut Option<RunCost>, amount: f64, currency: &str) {
    match cost {
        Some(counted) if counted.currency == currency => counted.amount += amount,
        Some(_) => {}
        None => {
            *cost = Some(RunCost {
                amount,
                currency: currency.to_string(),
            });
        }
    }
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat usage counters could not be saved or read",
        true,
    )
}

fn count_from_row(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<u64> {
    let value: i64 = row.try_get(column).map_err(persistence_error)?;
    Ok(u64::try_from(value).unwrap_or(0))
}

fn optional_count_from_row(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<Option<u64>> {
    let value: Option<i64> = row.try_get(column).map_err(persistence_error)?;
    Ok(value.map(|value| u64::try_from(value).unwrap_or(0)))
}

fn stored_count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The thread totals in a usage row, when the row holds them.
fn thread_totals_from_row(row: &sqlx::sqlite::SqliteRow) -> ChatResult<Option<ThreadTotals>> {
    let tokens = (
        optional_count_from_row(row, "thread_input_tokens_last")?,
        optional_count_from_row(row, "thread_cached_input_tokens_last")?,
        optional_count_from_row(row, "thread_cache_write_input_tokens_last")?,
        optional_count_from_row(row, "thread_output_tokens_last")?,
    );
    let (Some(input), Some(cached_input), Some(cache_write_input), Some(output)) = tokens else {
        return Ok(None);
    };
    Ok(Some(ThreadTotals {
        tokens: TokenCounts {
            input,
            cached_input,
            cache_write_input,
            output,
        },
        cost: row.try_get("thread_cost_last").map_err(persistence_error)?,
    }))
}

/// The stored usage of the run `run_id` and whether its unknown scope was already logged.
async fn read_run_usage(
    transaction: &mut Transaction<'_, Sqlite>,
    run_id: &str,
) -> ChatResult<Option<(RunUsage, bool)>> {
    let row = sqlx::query(
        "SELECT input_tokens, cached_input_tokens, cache_write_input_tokens, output_tokens,
                cost_amount, cost_currency, thread_input_tokens_last,
                thread_cached_input_tokens_last, thread_cache_write_input_tokens_last,
                thread_output_tokens_last, thread_cost_last, scope_unknown_logged
         FROM chat_agent_run_usage WHERE run_id = ?",
    )
    .bind(run_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let amount: Option<f64> = row.try_get("cost_amount").map_err(persistence_error)?;
    let currency: Option<String> = row.try_get("cost_currency").map_err(persistence_error)?;
    let usage = RunUsage {
        tokens: TokenCounts {
            input: count_from_row(&row, "input_tokens")?,
            cached_input: count_from_row(&row, "cached_input_tokens")?,
            cache_write_input: count_from_row(&row, "cache_write_input_tokens")?,
            output: count_from_row(&row, "output_tokens")?,
        },
        cost: amount
            .zip(currency)
            .map(|(amount, currency)| RunCost { amount, currency }),
        thread_last: thread_totals_from_row(&row)?,
    };
    let logged: i64 = row
        .try_get("scope_unknown_logged")
        .map_err(persistence_error)?;
    Ok(Some((usage, logged != 0)))
}

/// The thread totals the latest earlier run on the provider thread `thread_id` saw.
async fn carried_thread_totals(
    transaction: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    thread_id: &str,
) -> ChatResult<Option<ThreadTotals>> {
    let row = sqlx::query(
        "SELECT usage.thread_input_tokens_last, usage.thread_cached_input_tokens_last,
                usage.thread_cache_write_input_tokens_last, usage.thread_output_tokens_last,
                usage.thread_cost_last
         FROM chat_agent_run_usage usage
         JOIN chat_agent_runs run ON run.id = usage.run_id
         WHERE run.provider_thread_id = ? AND run.id <> ?
           AND usage.thread_input_tokens_last IS NOT NULL
           AND run.created_at <= (SELECT created_at FROM chat_agent_runs WHERE id = ?)
         ORDER BY run.created_at DESC, run.run_ordinal DESC, run.id DESC
         LIMIT 1",
    )
    .bind(thread_id)
    .bind(run_id)
    .bind(run_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    match row {
        Some(row) => thread_totals_from_row(&row),
        None => Ok(None),
    }
}

async fn write_run_usage(
    transaction: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    usage: &RunUsage,
    occurred_at: &str,
) -> ChatResult<()> {
    let thread = usage.thread_last;
    sqlx::query(
        "UPDATE chat_agent_run_usage
         SET input_tokens = ?, cached_input_tokens = ?, cache_write_input_tokens = ?,
             output_tokens = ?, counted_tokens = ?, cost_amount = ?, cost_currency = ?,
             thread_input_tokens_last = ?, thread_cached_input_tokens_last = ?,
             thread_cache_write_input_tokens_last = ?, thread_output_tokens_last = ?,
             thread_cost_last = ?, updated_at = ?
         WHERE run_id = ?",
    )
    .bind(stored_count(usage.tokens.input))
    .bind(stored_count(usage.tokens.cached_input))
    .bind(stored_count(usage.tokens.cache_write_input))
    .bind(stored_count(usage.tokens.output))
    .bind(stored_count(usage.tokens.counted()))
    .bind(usage.cost.as_ref().map(|cost| cost.amount))
    .bind(usage.cost.as_ref().map(|cost| cost.currency.as_str()))
    .bind(thread.map(|thread| stored_count(thread.tokens.input)))
    .bind(thread.map(|thread| stored_count(thread.tokens.cached_input)))
    .bind(thread.map(|thread| stored_count(thread.tokens.cache_write_input)))
    .bind(thread.map(|thread| stored_count(thread.tokens.output)))
    .bind(thread.and_then(|thread| thread.cost))
    .bind(occurred_at)
    .bind(run_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Folds the usage report `event` into the counters of the run `run_id`, which executes on the
/// provider thread `thread_id`, inside the caller's transaction.
///
/// A report without a documented scope is logged once per run and never counted. A thread
/// total that went backwards is logged and keeps the counted values.
pub async fn record_run_usage(
    transaction: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    thread_id: &str,
    event: &ThreadUsageUpdatedEvent,
    occurred_at: &str,
) -> ChatResult<()> {
    if !reports_tokens(event) {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO chat_agent_run_usage (run_id, updated_at) VALUES (?, ?)
         ON CONFLICT(run_id) DO NOTHING",
    )
    .bind(run_id)
    .bind(occurred_at)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let Some((current, scope_unknown_logged)) = read_run_usage(transaction, run_id).await? else {
        return Ok(());
    };
    let carried = if current.thread_last.is_none()
        && event.count_scope == Some(UsageCountScope::ThreadCumulative)
    {
        carried_thread_totals(transaction, run_id, thread_id).await?
    } else {
        None
    };
    match fold_usage(&current, event, carried.as_ref()) {
        Ok(next) => {
            if thread_count_restarted(&current, event, carried.as_ref()) {
                eprintln!("Chat usage: the provider thread count restarted; counting from zero");
            }
            if next != current {
                write_run_usage(transaction, run_id, &next, occurred_at).await?;
            }
        }
        Err(UsageCountError::UnknownScope) => {
            if !scope_unknown_logged {
                eprintln!("Chat usage: usage_scope_unknown; the report is not counted");
                sqlx::query(
                    "UPDATE chat_agent_run_usage SET scope_unknown_logged = 1 WHERE run_id = ?",
                )
                .bind(run_id)
                .execute(&mut **transaction)
                .await
                .map_err(persistence_error)?;
            }
        }
        Err(UsageCountError::TotalWentBackwards) => {
            eprintln!("Chat usage: a thread total went backwards; the counted usage is kept");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
