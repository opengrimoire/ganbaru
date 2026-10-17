//! Bounded consistent native usage reads shared by desktop and Android.

use super::{MAX_SOURCE_GROUPS, MAX_WINDOW_SAMPLES, UsageSourceDay};
use sqlx::{Row, SqliteConnection};

/// Aggregate a bounded, consistent SQLite window without returning raw intervals to the UI.
pub async fn read_source_days(
    connection: &mut SqliteConnection,
    week: &str,
    local_date: &str,
) -> Result<Vec<UsageSourceDay>, String> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT 1 FROM distractions_usage_samples
         WHERE local_date >= ? AND local_date <= ? LIMIT ?)",
    )
    .bind(week)
    .bind(local_date)
    .bind(MAX_WINDOW_SAMPLES + 1)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| format!("count native usage window: {error}"))?;
    if count > MAX_WINDOW_SAMPLES {
        return Err("usage window exceeds the native sample limit".into());
    }
    let rows = sqlx::query(
        "SELECT source_type, source_key, local_date, SUM(elapsed_seconds) AS elapsed_seconds
         FROM distractions_usage_samples WHERE local_date >= ? AND local_date <= ?
         GROUP BY source_type, source_key, local_date
         ORDER BY local_date, source_type, source_key LIMIT ?",
    )
    .bind(week)
    .bind(local_date)
    .bind((MAX_SOURCE_GROUPS + 1) as i64)
    .fetch_all(connection)
    .await
    .map_err(|error| format!("aggregate native usage window: {error}"))?;
    if rows.len() > MAX_SOURCE_GROUPS {
        return Err("usage source count exceeds the native limit".into());
    }
    rows.into_iter()
        .map(|row| {
            Ok(UsageSourceDay {
                source_type: row
                    .try_get("source_type")
                    .map_err(|error| error.to_string())?,
                source_key: row
                    .try_get("source_key")
                    .map_err(|error| error.to_string())?,
                local_date: row
                    .try_get("local_date")
                    .map_err(|error| error.to_string())?,
                elapsed_seconds: row
                    .try_get("elapsed_seconds")
                    .map_err(|error| error.to_string())?,
            })
        })
        .collect()
}
