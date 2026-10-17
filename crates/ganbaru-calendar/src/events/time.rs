use chrono::{DateTime, NaiveDateTime};

pub(super) async fn current_utc_iso(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<String, String> {
    sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("read current time: {e}"))
}
pub(super) fn calendar_timestamp_millis(value: &str) -> Option<i64> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.timestamp_millis());
    }
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(value, format) {
            return Some(parsed.and_utc().timestamp_millis());
        }
    }
    None
}
