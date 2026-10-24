//! This device's usage journal and the daily totals it replicates.
//!
//! Usage samples stay on the device that recorded them. The device sums its own samples per
//! source and local day and flushes each sum into `distractions_usage_totals` under a key
//! derived from the device id, source, and day, so other devices count its usage without seeing
//! its samples. A bucket flushes at most once per [`FLUSH_INTERVAL`] unless the caller forces a
//! flush on exit, backgrounding, or a vault snapshot. Totals only grow and are never deleted;
//! the retention window filters reads.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ganbaru_sync::domains::distractions::{
    MAX_USAGE_DAY_SECONDS, MAX_USAGE_LABEL_CHARS, MAX_USAGE_SOURCE_KEY_CHARS,
};
use ganbaru_sync::manifest::vault::distractions::USAGE_TOTAL_KEY_PREFIX;
use ganbaru_sync_contracts::bounds::MAX_DEVICE_ID_BYTES;
use ganbaru_sync_contracts::derived_key;
use sqlx::{Row, SqliteConnection, SqlitePool};

use super::UsageDateWindow;
use crate::contracts::DistractionsUsageSampleRow;
use crate::limits::MAX_SOURCE_GROUPS;

/// Shortest time between two flushes of one bucket that are not forced.
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// One source on one local day: the unit a total covers.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UsageBucket {
    pub source_type: String,
    pub source_key: String,
    pub local_date: String,
}

/// A bucket whose own samples differ from the total this device last flushed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaleTotal {
    pub bucket: UsageBucket,
    pub elapsed_seconds: i64,
    pub display_name: Option<String>,
}

/// When each bucket of one vault last flushed, so a bucket flushes at most once per
/// [`FLUSH_INTERVAL`] unless forced. It lives in memory: after a restart every stale bucket is
/// due once, and staleness itself is read from SQLite.
#[derive(Debug, Default)]
pub struct FlushPlanner {
    vault_id: Option<String>,
    flushed_at: HashMap<UsageBucket, Instant>,
}

impl FlushPlanner {
    /// The stale totals of `vault_id` due at `now`. A different vault starts a fresh history.
    pub fn due(
        &mut self,
        vault_id: &str,
        stale: Vec<StaleTotal>,
        now: Instant,
        force: bool,
    ) -> Vec<StaleTotal> {
        if self.vault_id.as_deref() != Some(vault_id) {
            self.vault_id = Some(vault_id.to_string());
            self.flushed_at.clear();
        }
        stale
            .into_iter()
            .filter(|total| {
                force
                    || self.flushed_at.get(&total.bucket).is_none_or(|flushed| {
                        now.saturating_duration_since(*flushed) >= FLUSH_INTERVAL
                    })
            })
            .collect()
    }

    /// Records that `buckets` of `vault_id` flushed at `now`, and forgets buckets whose interval
    /// has passed, which no longer hold a flush back.
    pub fn record<'a>(
        &mut self,
        vault_id: &str,
        buckets: impl IntoIterator<Item = &'a UsageBucket>,
        now: Instant,
    ) {
        if self.vault_id.as_deref() != Some(vault_id) {
            self.vault_id = Some(vault_id.to_string());
            self.flushed_at.clear();
        }
        self.flushed_at
            .retain(|_, flushed| now.saturating_duration_since(*flushed) < FLUSH_INTERVAL);
        for bucket in buckets {
            self.flushed_at.insert(bucket.clone(), now);
        }
    }
}

/// Stores this device's `samples` dated inside `window` and drops samples older than it.
/// Samples already stored are ignored, so retrying a batch is safe. Returns how many samples
/// fell outside the window and were skipped.
pub async fn insert_own_samples(
    connection: &mut SqliteConnection,
    samples: &[DistractionsUsageSampleRow],
    window: &UsageDateWindow,
) -> Result<usize, String> {
    sqlx::query("DELETE FROM distractions_usage_samples WHERE local_date < ?")
        .bind(&window.earliest)
        .execute(&mut *connection)
        .await
        .map_err(|error| format!("apply usage retention: {error}"))?;
    let mut skipped = 0;
    for sample in samples {
        if !window.contains(&sample.local_date) {
            skipped += 1;
            continue;
        }
        sqlx::query(
            "INSERT OR IGNORE INTO distractions_usage_samples
                (id, source_type, source_key, display_name, started_at_ms, elapsed_seconds,
                 local_date, created_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&sample.id)
        .bind(&sample.source_type)
        .bind(&sample.source_key)
        .bind(&sample.display_name)
        .bind(sample.started_at_ms)
        .bind(sample.elapsed_seconds)
        .bind(&sample.local_date)
        .bind(sample.created_at_ms)
        .execute(&mut *connection)
        .await
        .map_err(|error| format!("store usage sample: {error}"))?;
    }
    Ok(skipped)
}

/// Buckets inside `window` whose summed own samples exceed, or whose label differs from, the
/// total `device_id` last flushed. Sums are capped at the longest local day. Buckets whose
/// source key cannot form a total are left out, so one odd sample never blocks the others.
pub async fn stale_totals(
    connection: &mut SqliteConnection,
    device_id: &str,
    window: &UsageDateWindow,
) -> Result<Vec<StaleTotal>, String> {
    let rows = sqlx::query(
        "SELECT own.source_type, own.source_key, own.local_date, own.elapsed_seconds,
                own.display_name
         FROM (
            SELECT source_type, source_key, local_date,
                   min(SUM(elapsed_seconds), ?) AS elapsed_seconds,
                   MAX(display_name) AS display_name
            FROM distractions_usage_samples
            WHERE local_date >= ? AND local_date <= ?
            GROUP BY source_type, source_key, local_date
         ) AS own
         LEFT JOIN distractions_usage_totals AS total
            ON total.device_id = ? AND total.source_type = own.source_type
            AND total.source_key = own.source_key AND total.local_date = own.local_date
         WHERE total.id IS NULL
            OR own.elapsed_seconds > total.elapsed_seconds
            OR own.display_name IS NOT total.display_name
         ORDER BY own.local_date, own.source_type, own.source_key
         LIMIT ?",
    )
    .bind(MAX_USAGE_DAY_SECONDS)
    .bind(&window.earliest)
    .bind(&window.latest)
    .bind(device_id)
    .bind((MAX_SOURCE_GROUPS + 1) as i64)
    .fetch_all(connection)
    .await
    .map_err(|error| format!("find stale usage totals: {error}"))?;
    if rows.len() > MAX_SOURCE_GROUPS {
        return Err("stale usage totals exceed the source limit".into());
    }
    let mut stale = Vec::with_capacity(rows.len());
    for row in rows {
        let source_key: String = row.try_get("source_key").map_err(|e| e.to_string())?;
        if source_key.trim().is_empty() || source_key.chars().count() > MAX_USAGE_SOURCE_KEY_CHARS {
            continue;
        }
        let display_name: Option<String> =
            row.try_get("display_name").map_err(|e| e.to_string())?;
        stale.push(StaleTotal {
            bucket: UsageBucket {
                source_type: row.try_get("source_type").map_err(|e| e.to_string())?,
                source_key,
                local_date: row.try_get("local_date").map_err(|e| e.to_string())?,
            },
            elapsed_seconds: row.try_get("elapsed_seconds").map_err(|e| e.to_string())?,
            display_name: display_name
                .map(|name| name.chars().take(MAX_USAGE_LABEL_CHARS).collect()),
        });
    }
    Ok(stale)
}

/// The replicated key of `device_id`'s total for `bucket`.
pub fn usage_total_key(device_id: &str, bucket: &UsageBucket) -> Result<String, String> {
    derived_key(
        USAGE_TOTAL_KEY_PREFIX,
        &[
            device_id,
            &bucket.source_type,
            &bucket.source_key,
            &bucket.local_date,
        ],
    )
    .map(|key| key.to_string())
    .map_err(|error| format!("derive usage total key: {error}"))
}

/// Writes `totals` as `device_id`'s flushes. A total only grows and its label follows the
/// latest flush; an unchanged total writes nothing, so it records no sync change.
pub async fn write_totals(
    connection: &mut SqliteConnection,
    device_id: &str,
    totals: &[StaleTotal],
) -> Result<(), String> {
    for total in totals {
        sqlx::query(
            "INSERT INTO distractions_usage_totals
                (id, device_id, source_type, source_key, local_date, elapsed_seconds,
                 display_name)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET
                elapsed_seconds = max(elapsed_seconds, excluded.elapsed_seconds),
                display_name = excluded.display_name
             WHERE excluded.elapsed_seconds > elapsed_seconds
                OR excluded.display_name IS NOT display_name",
        )
        .bind(usage_total_key(device_id, &total.bucket)?)
        .bind(device_id)
        .bind(&total.bucket.source_type)
        .bind(&total.bucket.source_key)
        .bind(&total.bucket.local_date)
        .bind(total.elapsed_seconds)
        .bind(&total.display_name)
        .execute(&mut *connection)
        .await
        .map_err(|error| format!("flush usage total: {error}"))?;
    }
    Ok(())
}

/// Flushes the stale totals of `device_id` in `vault_id` that `planner` finds due at `now`, in
/// one transaction. Returns how many totals were written.
pub async fn flush_own_totals(
    pool: &SqlitePool,
    planner: &mut FlushPlanner,
    vault_id: &str,
    device_id: &str,
    window: &UsageDateWindow,
    now: Instant,
    force: bool,
) -> Result<usize, String> {
    if device_id.trim().is_empty() || device_id.len() > MAX_DEVICE_ID_BYTES {
        return Err("usage totals need a valid device id".into());
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin usage total flush: {error}"))?;
    let stale = stale_totals(&mut transaction, device_id, window).await?;
    let due = planner.due(vault_id, stale, now, force);
    if due.is_empty() {
        transaction
            .rollback()
            .await
            .map_err(|error| format!("finish usage total check: {error}"))?;
        return Ok(0);
    }
    write_totals(&mut transaction, device_id, &due).await?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit usage total flush: {error}"))?;
    planner.record(vault_id, due.iter().map(|total| &total.bucket), now);
    Ok(due.len())
}

#[cfg(test)]
mod tests;
