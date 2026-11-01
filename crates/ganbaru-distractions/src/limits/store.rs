//! Bounded consistent native usage reads shared by desktop and Android.

use super::{
    MAX_SOURCE_GROUPS, MAX_SOURCE_KEY_BYTES, MAX_WINDOW_SAMPLES, UsageBudgetScope, UsageSourceDay,
    UsageWindow,
};
use sqlx::{AssertSqlSafe, Row, SqliteConnection};

/// Rows and groups one read may keep.
#[derive(Clone, Copy)]
pub(super) struct WindowBounds {
    pub(super) samples: i64,
    pub(super) groups: usize,
}

const WINDOW_BOUNDS: WindowBounds = WindowBounds {
    samples: MAX_WINDOW_SAMPLES,
    groups: MAX_SOURCE_GROUPS,
};

/// Keeps rows of the budget sources in `?4`, `?5`, and `?6`. Rule lists bind as one JSON array each
/// because a configuration can exceed SQLite's expression depth. A key past `?7` could not be
/// totaled, so it stays out of the window. The suffix compares `substr` instead of `LIKE` because
/// `%` and `_` in a host are not wildcards.
const SCOPE_FILTER: &str = "length(CAST(source_key AS BLOB)) <= ?7 AND (
        (source_type = 'website' AND (
            lower(source_key) IN (SELECT rule FROM websites)
            OR EXISTS (
                SELECT 1 FROM websites
                WHERE substr(lower(source_key), -length(rule) - 1) = '.' || rule
            )
        ))
        OR (source_type = 'desktop-app' AND (
            source_key IN (SELECT name FROM desktop_apps)
            OR lower(source_key) IN (SELECT name FROM desktop_apps)
        ))
        OR (source_type = 'mobile-app' AND (
            source_key IN (SELECT name FROM mobile_apps)
            OR lower(source_key) IN (SELECT name FROM mobile_apps)
        ))
    )";

/// Usage of the window from `week` through `local_date` for the sources in `scope`: this device's
/// own samples plus the totals every other device flushed. The totals `own_device_id` flushed are
/// left out because its samples already count that usage. Aggregates a bounded, consistent SQLite
/// window without returning raw intervals to the UI.
///
/// Replicated rows cannot fail the read. Past [`MAX_WINDOW_SAMPLES`] rows or [`MAX_SOURCE_GROUPS`]
/// groups the window keeps this device's samples first, then the most recent days, in a
/// deterministic order, and reports truncation.
pub async fn read_source_days(
    connection: &mut SqliteConnection,
    own_device_id: &str,
    week: &str,
    local_date: &str,
    scope: &UsageBudgetScope,
) -> Result<UsageWindow, String> {
    read_bounded(
        connection,
        own_device_id,
        week,
        local_date,
        scope,
        WINDOW_BOUNDS,
    )
    .await
}

/// Runs the window read under explicit bounds.
pub(super) async fn read_bounded(
    connection: &mut SqliteConnection,
    own_device_id: &str,
    week: &str,
    local_date: &str,
    scope: &UsageBudgetScope,
    bounds: WindowBounds,
) -> Result<UsageWindow, String> {
    if scope.is_empty() {
        return Ok(UsageWindow::default());
    }
    let sql = format!(
        "WITH websites(rule) AS MATERIALIZED (SELECT value FROM json_each(?4)),
              desktop_apps(name) AS MATERIALIZED (SELECT value FROM json_each(?5)),
              mobile_apps(name) AS MATERIALIZED (SELECT value FROM json_each(?6))
         SELECT source_type, source_key, local_date, SUM(elapsed_seconds) AS elapsed_seconds,
                MIN(origin) AS group_origin, SUM(COUNT(*)) OVER () AS window_rows
         FROM (
             SELECT origin, source_type, source_key, local_date, elapsed_seconds
             FROM (
                 SELECT 0 AS origin, '' AS device_id, source_type, source_key, local_date,
                        elapsed_seconds
                 FROM distractions_usage_samples
                 WHERE local_date >= ?1 AND local_date <= ?2 AND {SCOPE_FILTER}
                 UNION ALL
                 SELECT 1, device_id, source_type, source_key, local_date, elapsed_seconds
                 FROM distractions_usage_totals
                 WHERE local_date >= ?1 AND local_date <= ?2 AND device_id <> ?3 AND {SCOPE_FILTER}
             )
             ORDER BY origin, local_date DESC, source_type, source_key, device_id,
                      elapsed_seconds DESC
             LIMIT ?8
         )
         GROUP BY source_type, source_key, local_date
         ORDER BY group_origin, local_date DESC, source_type, source_key
         LIMIT ?9"
    );
    let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(week)
        .bind(local_date)
        .bind(own_device_id)
        .bind(rule_array(&scope.websites)?)
        .bind(rule_array(&scope.desktop_apps)?)
        .bind(rule_array(&scope.mobile_apps)?)
        .bind(MAX_SOURCE_KEY_BYTES as i64)
        .bind(bounds.samples)
        .bind((bounds.groups + 1) as i64)
        .fetch_all(connection)
        .await
        .map_err(|error| format!("aggregate native usage window: {error}"))?;
    let window_rows: i64 = match rows.first() {
        Some(row) => row
            .try_get("window_rows")
            .map_err(|error| error.to_string())?,
        None => 0,
    };
    let truncated = window_rows >= bounds.samples || rows.len() > bounds.groups;
    let sources = rows
        .into_iter()
        .take(bounds.groups)
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
        .collect::<Result<Vec<_>, String>>()?;
    Ok(UsageWindow { sources, truncated })
}

fn rule_array(rules: &std::collections::BTreeSet<String>) -> Result<String, String> {
    serde_json::to_string(rules).map_err(|error| format!("encode usage budget scope: {error}"))
}
