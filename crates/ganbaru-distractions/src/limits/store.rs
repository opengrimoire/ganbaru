//! Bounded consistent native usage reads shared by desktop and Android.

use super::{
    MAX_SOURCE_GROUPS, MAX_SOURCE_KEY_BYTES, MAX_SUFFIX_PROBES, MAX_WINDOW_SAMPLES,
    UsageBudgetScope, UsageSourceDay, UsageWindow,
};
use sqlx::{AssertSqlSafe, Row, SqliteConnection};
use std::collections::BTreeSet;

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
/// `%` and `_` in a host are not wildcards, and it probes once per distinct suffix length, so a
/// row's cost does not grow with the number of website rules.
const SCOPE_FILTER: &str = "length(CAST(source_key AS BLOB)) <= ?7 AND (
        (source_type = 'website' AND (
            lower(source_key) IN (SELECT rule FROM websites)
            OR EXISTS (
                SELECT 1 FROM suffix_lengths
                WHERE substr(lower(source_key), -size) IN (SELECT suffix FROM suffixes)
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

/// Keeps rows whose date is on the calendar, matching `is_valid_local_date` inside any usable
/// window. SQLite rolls an impossible day into the next month, so such a date differs from its own
/// normalized form, and the row takes no share of the read bounds.
pub(super) const CALENDAR_DATE_FILTER: &str = "date(local_date) = local_date";

/// Usage of the window from `week` through `local_date` for the sources in `scope`: this device's
/// own samples plus the totals every other device flushed. The totals `own_device_id` flushed are
/// left out because its samples already count that usage. Aggregates a bounded, consistent SQLite
/// window without returning raw intervals to the UI.
///
/// Replicated rows cannot fail the read. A row dated off the calendar is skipped. The scan stops
/// after [`MAX_WINDOW_SAMPLES`] rows of the window, or sooner when each row would need many suffix
/// comparisons, so the read spends at most [`MAX_SUFFIX_PROBES`] of them. Rows are counted before
/// the budget filter so rows of unrelated sources cannot extend the scan. Past that bound or
/// [`MAX_SOURCE_GROUPS`] groups the window keeps this device's samples first, then the most recent
/// days, in a deterministic order, and reports truncation.
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

/// Runs the window read under explicit bounds. Each scan walks its date index from the newest day
/// and stops at its share of [`scan_bound`], with this device's samples served first.
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
    let scan_rows = scan_bound(bounds.samples, &scope.websites);
    let scanned_rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT (SELECT COUNT(*) FROM (
                     SELECT 1 FROM distractions_usage_samples
                     WHERE local_date >= ?1 AND local_date <= ?2 AND {CALENDAR_DATE_FILTER}
                     LIMIT ?4
                 )) AS own_rows,
                (SELECT COUNT(*) FROM (
                     SELECT 1 FROM distractions_usage_totals
                     WHERE local_date >= ?1 AND local_date <= ?2 AND device_id <> ?3
                           AND {CALENDAR_DATE_FILTER}
                     LIMIT ?4
                 )) AS other_rows"
    )))
    .bind(week)
    .bind(local_date)
    .bind(own_device_id)
    .bind(scan_rows + 1)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| format!("measure native usage window: {error}"))?;
    let own_rows: i64 = scanned_rows
        .try_get("own_rows")
        .map_err(|error| error.to_string())?;
    let other_rows: i64 = scanned_rows
        .try_get("other_rows")
        .map_err(|error| error.to_string())?;
    let sql = format!(
        "WITH websites(rule) AS MATERIALIZED (SELECT value FROM json_each(?4)),
              suffixes(suffix) AS MATERIALIZED (SELECT '.' || rule FROM websites),
              suffix_lengths(size) AS MATERIALIZED (SELECT DISTINCT length(suffix) FROM suffixes),
              desktop_apps(name) AS MATERIALIZED (SELECT value FROM json_each(?5)),
              mobile_apps(name) AS MATERIALIZED (SELECT value FROM json_each(?6))
         SELECT source_type, source_key, local_date, SUM(elapsed_seconds) AS elapsed_seconds,
                MIN(origin) AS group_origin
         FROM (
             SELECT 0 AS origin, source_type, source_key, local_date, elapsed_seconds
             FROM (
                 SELECT source_type, source_key, local_date, elapsed_seconds
                 FROM distractions_usage_samples
                 WHERE local_date >= ?1 AND local_date <= ?2 AND {CALENDAR_DATE_FILTER}
                 ORDER BY local_date DESC, source_type DESC, source_key DESC
                 LIMIT ?8
             )
             UNION ALL
             SELECT 1, source_type, source_key, local_date, elapsed_seconds
             FROM (
                 SELECT source_type, source_key, local_date, elapsed_seconds
                 FROM distractions_usage_totals
                 WHERE local_date >= ?1 AND local_date <= ?2 AND device_id <> ?3
                       AND {CALENDAR_DATE_FILTER}
                 ORDER BY local_date DESC, device_id DESC
                 LIMIT ?9
             )
         )
         WHERE {SCOPE_FILTER}
         GROUP BY source_type, source_key, local_date
         ORDER BY group_origin, local_date DESC, source_type, source_key
         LIMIT ?10"
    );
    let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
        .bind(week)
        .bind(local_date)
        .bind(own_device_id)
        .bind(rule_array(&scope.websites)?)
        .bind(rule_array(&scope.desktop_apps)?)
        .bind(rule_array(&scope.mobile_apps)?)
        .bind(MAX_SOURCE_KEY_BYTES as i64)
        .bind(scan_rows)
        .bind((scan_rows - own_rows).max(0))
        .bind((bounds.groups + 1) as i64)
        .fetch_all(connection)
        .await
        .map_err(|error| format!("aggregate native usage window: {error}"))?;
    let truncated = own_rows + other_rows > scan_rows || rows.len() > bounds.groups;
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

/// Rows one read may scan: `samples`, lowered so that one suffix comparison per distinct website
/// rule length on every row stays within [`MAX_SUFFIX_PROBES`].
fn scan_bound(samples: i64, websites: &BTreeSet<String>) -> i64 {
    let lengths = websites
        .iter()
        .map(|rule| rule.chars().count())
        .collect::<BTreeSet<_>>()
        .len();
    samples.min(MAX_SUFFIX_PROBES / lengths.max(1) as i64)
}

fn rule_array(rules: &BTreeSet<String>) -> Result<String, String> {
    serde_json::to_string(rules).map_err(|error| format!("encode usage budget scope: {error}"))
}
