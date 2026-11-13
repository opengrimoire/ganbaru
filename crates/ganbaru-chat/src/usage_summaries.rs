//! Daily usage summaries per teammate, which every linked device reads.
//!
//! The executing device adds what its runs used since the last flush to its own row for each
//! teammate and local date, at the date of the flush. Each run remembers what it already added,
//! so a flush adds only new usage and counts each run once. Only the owning device writes a row
//! and its counts only grow, so replicas converge on per-device rows that readers sum.

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate};
use ganbaru_sync_contracts::chat_usage::{
    MAX_DAILY_COUNTED_TOKENS, MAX_DAILY_RUNS, MAX_DAILY_WORKING_SECONDS, teammate_usage_key,
};
use serde::Serialize;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::usage_counters::UsageCountError;

/// Seconds between scheduled flushes of the daily summaries.
pub const USAGE_SUMMARY_FLUSH_SECONDS: u64 = 60;
/// Local days a summary read reaches back, today included.
pub const USAGE_DAILY_RETENTION_DAYS: u64 = 90;

/// Which runs a flush adds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlushScope {
    /// Every run, with the open working interval counted up to the flush.
    All,
    /// Only runs that are not working now, which a run state change makes current.
    NotWorking,
}

/// Counts a summary row holds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SummaryCounts {
    counted_tokens: i64,
    working_seconds: i64,
    runs: i64,
}

impl SummaryCounts {
    fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    fn saturating_add(self, other: Self) -> Self {
        Self {
            counted_tokens: self.counted_tokens.saturating_add(other.counted_tokens),
            working_seconds: self.working_seconds.saturating_add(other.working_seconds),
            runs: self.runs.saturating_add(other.runs),
        }
    }
}

/// What one run used and what it already added to a summary.
#[derive(Clone, Debug)]
struct RunLedger {
    run_id: String,
    used: SummaryCounts,
    summarized: SummaryCounts,
}

impl RunLedger {
    /// What the run adds now; usage never shrinks a summary.
    fn delta(&self) -> SummaryCounts {
        SummaryCounts {
            counted_tokens: (self.used.counted_tokens - self.summarized.counted_tokens).max(0),
            working_seconds: (self.used.working_seconds - self.summarized.working_seconds).max(0),
            runs: (self.used.runs - self.summarized.runs).max(0),
        }
    }

    /// The ledger after this flush: never below what was already added.
    fn advanced(&self) -> SummaryCounts {
        SummaryCounts {
            counted_tokens: self.used.counted_tokens.max(self.summarized.counted_tokens),
            working_seconds: self
                .used
                .working_seconds
                .max(self.summarized.working_seconds),
            runs: self.used.runs.max(self.summarized.runs),
        }
    }
}

/// One local day of a teammate's usage, summed across the devices that ran it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTeammateUsageDayRead {
    /// The day in `YYYY-MM-DD` form.
    pub local_date: String,
    /// Input, cache write, and output tokens, leaving out cache reads.
    pub counted_tokens: u64,
    /// Seconds runs spent working, leaving out time waiting for a person.
    pub working_seconds: u64,
    /// Runs that started that day.
    pub run_count: u64,
    /// Devices whose runs used anything that day, sorted.
    pub device_ids: Vec<String>,
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat usage summaries could not be saved or read",
        true,
    )
}

fn validation_error(message: &'static str) -> ChatError {
    ChatError::new(ChatErrorCode::Validation, message, false)
}

/// Parses a `YYYY-MM-DD` local date, refusing any other spelling.
fn parse_local_date(value: &str) -> ChatResult<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|date| date.format("%Y-%m-%d").to_string() == value)
        .ok_or_else(|| validation_error("Usage dates must be local dates as YYYY-MM-DD"))
}

/// Adds `delta` to `current` field by field. A field that would pass its bound keeps its
/// current value and is logged, so the row never holds a clamped count.
fn bounded_sum(current: SummaryCounts, delta: SummaryCounts) -> SummaryCounts {
    let field = |current: i64, delta: i64, max: i64| match current.checked_add(delta) {
        Some(next) if next <= max => next,
        _ => {
            eprintln!(
                "Chat usage: {}; the summary keeps its last value",
                UsageCountError::SummaryOverflow.diagnostic()
            );
            current
        }
    };
    SummaryCounts {
        counted_tokens: field(
            current.counted_tokens,
            delta.counted_tokens,
            MAX_DAILY_COUNTED_TOKENS,
        ),
        working_seconds: field(
            current.working_seconds,
            delta.working_seconds,
            MAX_DAILY_WORKING_SECONDS,
        ),
        runs: field(current.runs, delta.runs, MAX_DAILY_RUNS),
    }
}

/// Runs with usage not yet added, by teammate.
async fn pending_runs(
    transaction: &mut Transaction<'_, Sqlite>,
    now: &str,
    scope: FlushScope,
) -> ChatResult<BTreeMap<String, Vec<RunLedger>>> {
    let rows = sqlx::query(
        "SELECT counters.run_id, assignment.teammate_id, counters.counted_tokens,
                counters.working_seconds + CASE
                    WHEN counters.working_since IS NULL THEN 0
                    ELSE coalesce(max(0, CAST(round(
                        (julianday(?1) - julianday(counters.working_since)) * 86400
                    ) AS INTEGER)), 0)
                END AS working_now,
                run.started_at IS NOT NULL AS started,
                counters.summarized_counted_tokens, counters.summarized_working_seconds,
                counters.summarized_run
         FROM chat_agent_run_usage counters
         JOIN chat_agent_runs run ON run.id = counters.run_id
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         JOIN chat_teammates teammate ON teammate.participant_id = assignment.teammate_id
         WHERE (?2 OR counters.working_since IS NULL)
           AND (counters.counted_tokens > counters.summarized_counted_tokens
                OR counters.working_since IS NOT NULL
                OR counters.working_seconds > counters.summarized_working_seconds
                OR (run.started_at IS NOT NULL AND counters.summarized_run = 0))
         ORDER BY assignment.teammate_id, counters.run_id",
    )
    .bind(now)
    .bind(scope == FlushScope::All)
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let mut pending: BTreeMap<String, Vec<RunLedger>> = BTreeMap::new();
    for row in rows {
        let started: bool = row.try_get("started").map_err(persistence_error)?;
        let ledger = RunLedger {
            run_id: row.try_get("run_id").map_err(persistence_error)?,
            used: SummaryCounts {
                counted_tokens: row.try_get("counted_tokens").map_err(persistence_error)?,
                working_seconds: row.try_get("working_now").map_err(persistence_error)?,
                runs: i64::from(started),
            },
            summarized: SummaryCounts {
                counted_tokens: row
                    .try_get("summarized_counted_tokens")
                    .map_err(persistence_error)?,
                working_seconds: row
                    .try_get("summarized_working_seconds")
                    .map_err(persistence_error)?,
                runs: row.try_get("summarized_run").map_err(persistence_error)?,
            },
        };
        let teammate_id: String = row.try_get("teammate_id").map_err(persistence_error)?;
        pending.entry(teammate_id).or_default().push(ledger);
    }
    Ok(pending)
}

/// The counts of the summary row `key`, zero when it does not exist yet.
async fn stored_summary(
    transaction: &mut Transaction<'_, Sqlite>,
    key: &str,
) -> ChatResult<SummaryCounts> {
    let row: Option<(i64, i64, i64)> = sqlx::query_as(
        "SELECT counted_tokens, working_seconds, run_count
         FROM chat_teammate_usage_daily WHERE id = ?",
    )
    .bind(key)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(row
        .map(|(counted_tokens, working_seconds, runs)| SummaryCounts {
            counted_tokens,
            working_seconds,
            runs,
        })
        .unwrap_or_default())
}

/// Saves the ledger of one run, refusing when another flush moved it first.
async fn advance_ledger(
    transaction: &mut Transaction<'_, Sqlite>,
    ledger: &RunLedger,
) -> ChatResult<()> {
    let next = ledger.advanced();
    let updated = sqlx::query(
        "UPDATE chat_agent_run_usage
         SET summarized_counted_tokens = ?, summarized_working_seconds = ?, summarized_run = ?
         WHERE run_id = ? AND summarized_counted_tokens = ?
           AND summarized_working_seconds = ? AND summarized_run = ?",
    )
    .bind(next.counted_tokens)
    .bind(next.working_seconds)
    .bind(next.runs)
    .bind(&ledger.run_id)
    .bind(ledger.summarized.counted_tokens)
    .bind(ledger.summarized.working_seconds)
    .bind(ledger.summarized.runs)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Chat usage changed while it was being summarized",
            true,
        ));
    }
    Ok(())
}

/// Adds the usage of this device's runs since the last flush to its summaries for
/// `local_date`, the device's civil date at `now`, in one transaction. Returns how many
/// summary rows changed.
///
/// A count that would pass its bound keeps the row's last value and is logged; the run still
/// records it as added, so the overflow is not retried on every flush.
pub async fn flush_teammate_usage(
    pool: &SqlitePool,
    device_id: &str,
    local_date: &str,
    now: &str,
    scope: FlushScope,
) -> ChatResult<usize> {
    parse_local_date(local_date)?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let pending = pending_runs(&mut transaction, now, scope).await?;
    let mut changed = 0;
    for (teammate_id, runs) in pending {
        let delta = runs.iter().fold(SummaryCounts::default(), |sum, run| {
            sum.saturating_add(run.delta())
        });
        if !delta.is_zero() {
            let key = teammate_usage_key(device_id, &teammate_id, local_date)
                .map_err(|_| validation_error("Usage summary key parts are invalid"))?;
            let current = stored_summary(&mut transaction, key.as_str()).await?;
            let next = bounded_sum(current, delta);
            let written = sqlx::query(
                "INSERT INTO chat_teammate_usage_daily
                    (id, device_id, teammate_id, local_date, counted_tokens, working_seconds,
                     run_count)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (id) DO UPDATE SET
                    counted_tokens = max(counted_tokens, excluded.counted_tokens),
                    working_seconds = max(working_seconds, excluded.working_seconds),
                    run_count = max(run_count, excluded.run_count)
                 WHERE excluded.counted_tokens > counted_tokens
                    OR excluded.working_seconds > working_seconds
                    OR excluded.run_count > run_count",
            )
            .bind(key.as_str())
            .bind(device_id)
            .bind(&teammate_id)
            .bind(local_date)
            .bind(next.counted_tokens)
            .bind(next.working_seconds)
            .bind(next.runs)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
            changed += usize::try_from(written.rows_affected()).unwrap_or(0);
        }
        for run in &runs {
            if run.advanced() != run.summarized {
                advance_ledger(&mut transaction, run).await?;
            }
        }
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(changed)
}

/// The earliest local date a read reaches when today is `today`.
fn retention_start(today: NaiveDate) -> NaiveDate {
    today
        .checked_sub_days(Days::new(USAGE_DAILY_RETENTION_DAYS - 1))
        .unwrap_or(NaiveDate::MIN)
}

/// Reads `teammate_id`'s usage per local day from `from_date` to `to_date`, both included,
/// summed across devices. Days before the retention window that ends at `today` are left out.
pub async fn read_teammate_usage(
    pool: &SqlitePool,
    teammate_id: &str,
    from_date: &str,
    to_date: &str,
    today: &str,
) -> ChatResult<Vec<ChatTeammateUsageDayRead>> {
    let from = parse_local_date(from_date)?;
    let to = parse_local_date(to_date)?;
    let today = parse_local_date(today)?;
    if from > to {
        return Err(validation_error(
            "A usage read must start on or before its end",
        ));
    }
    let from = from.max(retention_start(today));
    if from > to {
        return Ok(Vec::new());
    }
    let rows: Vec<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT local_date, device_id, counted_tokens, working_seconds, run_count
         FROM chat_teammate_usage_daily
         WHERE teammate_id = ? AND local_date BETWEEN ? AND ?
         ORDER BY local_date, device_id",
    )
    .bind(teammate_id)
    .bind(from.format("%Y-%m-%d").to_string())
    .bind(to.format("%Y-%m-%d").to_string())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut days: Vec<ChatTeammateUsageDayRead> = Vec::new();
    for (local_date, device_id, counted_tokens, working_seconds, runs) in rows {
        let count = |value: i64| u64::try_from(value).unwrap_or(0);
        if days.last().map(|day| day.local_date.as_str()) != Some(local_date.as_str()) {
            days.push(ChatTeammateUsageDayRead {
                local_date,
                counted_tokens: 0,
                working_seconds: 0,
                run_count: 0,
                device_ids: Vec::new(),
            });
        }
        let Some(day) = days.last_mut() else {
            continue;
        };
        day.counted_tokens = day.counted_tokens.saturating_add(count(counted_tokens));
        day.working_seconds = day.working_seconds.saturating_add(count(working_seconds));
        day.run_count = day.run_count.saturating_add(count(runs));
        day.device_ids.push(device_id);
    }
    Ok(days)
}

#[cfg(test)]
mod tests;
