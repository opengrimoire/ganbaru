use std::collections::BTreeMap;

use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use super::icalendar::load_component_jcal;
use super::{DbCalendarEventRow, DbFullEventRow, DbFullOverrideRow};

pub(super) async fn hydrate_window_event_rows(
    transaction: &mut Transaction<'_, Sqlite>,
    rows: &mut [DbCalendarEventRow],
) -> Result<(), String> {
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let notifications = load_i64_list_map_tx(
        transaction,
        "calendar_event_notifications",
        "offset_minutes",
        &ids,
    )
    .await?;
    let exceptions = load_string_list_map_tx(
        transaction,
        "calendar_event_exdates",
        "occurrence_date",
        &ids,
    )
    .await?;
    let rdates = load_string_list_map_tx(
        transaction,
        "calendar_event_rdates",
        "occurrence_start",
        &ids,
    )
    .await?;
    for row in rows {
        row.notifications = notifications.get(&row.id).cloned();
        row.exceptions = exceptions.get(&row.id).cloned();
        row.rdate = rdates.get(&row.id).cloned();
    }
    Ok(())
}

async fn load_i64_list_map_tx(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &'static str,
    column: &'static str,
    ids: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT event_id, {column} AS value FROM {table}
         WHERE event_id IN ({placeholders}) ORDER BY event_id ASC, sort_order ASC"
    );
    let mut query = sqlx::query(&query);
    for id in ids {
        query = query.bind(id);
    }
    let rows = query
        .fetch_all(&mut **transaction)
        .await
        .map_err(|e| format!("load {table}: {e}"))?;
    let mut grouped: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for row in rows {
        let event_id = row
            .try_get::<String, _>("event_id")
            .map_err(|e| format!("read {table}.event_id: {e}"))?;
        let value = row
            .try_get::<i64, _>("value")
            .map_err(|e| format!("read {table}.{column}: {e}"))?;
        grouped.entry(event_id).or_default().push(value);
    }
    grouped
        .into_iter()
        .map(|(id, values)| {
            serde_json::to_string(&values)
                .map(|json| (id, json))
                .map_err(|e| format!("serialize {table}: {e}"))
        })
        .collect()
}

async fn load_string_list_map_tx(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &'static str,
    column: &'static str,
    ids: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT event_id, {column} AS value FROM {table}
         WHERE event_id IN ({placeholders}) ORDER BY event_id ASC, sort_order ASC"
    );
    let mut query = sqlx::query(&query);
    for id in ids {
        query = query.bind(id);
    }
    let rows = query
        .fetch_all(&mut **transaction)
        .await
        .map_err(|e| format!("load {table}: {e}"))?;
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        let event_id = row
            .try_get::<String, _>("event_id")
            .map_err(|e| format!("read {table}.event_id: {e}"))?;
        let value = row
            .try_get::<String, _>("value")
            .map_err(|e| format!("read {table}.{column}: {e}"))?;
        grouped.entry(event_id).or_default().push(value);
    }
    grouped
        .into_iter()
        .map(|(id, values)| {
            serde_json::to_string(&values)
                .map(|json| (id, json))
                .map_err(|e| format!("serialize {table}: {e}"))
        })
        .collect()
}

pub(super) async fn hydrate_full_event_row(
    pool: &SqlitePool,
    row: &mut Option<DbFullEventRow>,
) -> Result<(), String> {
    let Some(event) = row else {
        return Ok(());
    };
    let ids = [event.id.clone()];
    event.notifications =
        load_i64_list_map(pool, "calendar_event_notifications", "offset_minutes", &ids)
            .await?
            .remove(&event.id);
    event.exceptions =
        load_string_list_map(pool, "calendar_event_exdates", "occurrence_date", &ids)
            .await?
            .remove(&event.id);
    event.rdate = load_string_list_map(pool, "calendar_event_rdates", "occurrence_start", &ids)
        .await?
        .remove(&event.id);
    event.categories = load_string_list_map(pool, "calendar_event_categories", "category", &ids)
        .await?
        .remove(&event.id);
    event.extended_properties =
        load_property_map(pool, "calendar_event_extended_properties", "event_id", &ids)
            .await?
            .remove(&event.id);
    event.organizer = load_organizer(pool, &event.id).await?;
    event.geo = load_geo(pool, &event.id).await?;
    event.icalendar_projection_warnings =
        load_component_warnings(pool, event.icalendar_component_id.as_deref()).await?;
    event.icalendar_raw_jcal =
        load_component_jcal(pool, event.icalendar_component_id.as_deref()).await?;
    Ok(())
}

pub(super) async fn hydrate_full_override_rows(
    pool: &SqlitePool,
    rows: &mut [DbFullOverrideRow],
) -> Result<(), String> {
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let properties = load_property_map(
        pool,
        "calendar_event_override_extended_properties",
        "override_id",
        &ids,
    )
    .await?;
    for row in rows {
        row.extended_properties = properties.get(&row.id).cloned();
        row.icalendar_raw_jcal =
            load_component_jcal(pool, row.icalendar_component_id.as_deref()).await?;
    }
    Ok(())
}

async fn load_i64_list_map(
    pool: &SqlitePool,
    table: &'static str,
    column: &'static str,
    ids: &[String],
) -> Result<BTreeMap<String, String>, String> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT event_id, {column} AS value FROM {table}
         WHERE event_id IN ({placeholders})
         ORDER BY event_id ASC, sort_order ASC"
    );
    let mut q = sqlx::query(&query);
    for id in ids {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load {table}: {e}"))?;
    let mut grouped: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for row in rows {
        let event_id: String = row
            .try_get("event_id")
            .map_err(|e| format!("read {table}.event_id: {e}"))?;
        let value: i64 = row
            .try_get("value")
            .map_err(|e| format!("read {table}.{column}: {e}"))?;
        grouped.entry(event_id).or_default().push(value);
    }
    grouped
        .into_iter()
        .map(|(id, values)| {
            serde_json::to_string(&values)
                .map(|json| (id, json))
                .map_err(|e| format!("serialize {table}: {e}"))
        })
        .collect()
}

async fn load_string_list_map(
    pool: &SqlitePool,
    table: &'static str,
    column: &'static str,
    ids: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let rows = load_ordered_values(pool, table, column, ids).await?;
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (event_id, value) in rows {
        grouped.entry(event_id).or_default().push(value);
    }
    grouped
        .into_iter()
        .map(|(id, values)| {
            serde_json::to_string(&values)
                .map(|json| (id, json))
                .map_err(|e| format!("serialize {table}: {e}"))
        })
        .collect()
}

async fn load_ordered_values(
    pool: &SqlitePool,
    table: &'static str,
    column: &'static str,
    ids: &[String],
) -> Result<Vec<(String, String)>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT event_id, {column} AS value FROM {table}
         WHERE event_id IN ({placeholders})
         ORDER BY event_id ASC, sort_order ASC"
    );
    let mut q = sqlx::query(&query);
    for id in ids {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load {table}: {e}"))?;
    rows.into_iter()
        .map(|row| {
            let event_id: String = row
                .try_get("event_id")
                .map_err(|e| format!("read {table}.event_id: {e}"))?;
            let value: String = row
                .try_get("value")
                .map_err(|e| format!("read {table}.{column}: {e}"))?;
            Ok((event_id, value))
        })
        .collect()
}

async fn load_property_map(
    pool: &SqlitePool,
    table: &'static str,
    owner_column: &'static str,
    ids: &[String],
) -> Result<BTreeMap<String, String>, String> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT {owner_column} AS owner_id, property_key, property_value FROM {table}
         WHERE {owner_column} IN ({placeholders})
         ORDER BY {owner_column} ASC, sort_order ASC"
    );
    let mut q = sqlx::query(&query);
    for id in ids {
        q = q.bind(id);
    }
    let rows = q
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load {table}: {e}"))?;
    let mut grouped: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for row in rows {
        let owner_id: String = row
            .try_get("owner_id")
            .map_err(|e| format!("read {table}.{owner_column}: {e}"))?;
        let key: String = row
            .try_get("property_key")
            .map_err(|e| format!("read {table}.property_key: {e}"))?;
        let value: String = row
            .try_get("property_value")
            .map_err(|e| format!("read {table}.property_value: {e}"))?;
        grouped.entry(owner_id).or_default().insert(key, value);
    }
    grouped
        .into_iter()
        .map(|(id, values)| {
            serde_json::to_string(&values)
                .map(|json| (id, json))
                .map_err(|e| format!("serialize {table}: {e}"))
        })
        .collect()
}

async fn load_organizer(pool: &SqlitePool, event_id: &str) -> Result<Option<String>, String> {
    let row = sqlx::query("SELECT name, email FROM calendar_event_organizers WHERE event_id = ?")
        .bind(event_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load organizer: {e}"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let name: Option<String> = row
        .try_get("name")
        .map_err(|e| format!("read organizer name: {e}"))?;
    let email: String = row
        .try_get("email")
        .map_err(|e| format!("read organizer email: {e}"))?;
    serde_json::to_string(&serde_json::json!({ "name": name, "email": email }))
        .map(Some)
        .map_err(|e| format!("serialize organizer: {e}"))
}

async fn load_geo(pool: &SqlitePool, event_id: &str) -> Result<Option<String>, String> {
    let row = sqlx::query("SELECT geo_lat, geo_lng FROM calendar_events WHERE id = ?")
        .bind(event_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load geo: {e}"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    let lat: Option<f64> = row
        .try_get("geo_lat")
        .map_err(|e| format!("read geo_lat: {e}"))?;
    let lng: Option<f64> = row
        .try_get("geo_lng")
        .map_err(|e| format!("read geo_lng: {e}"))?;
    match (lat, lng) {
        (Some(lat), Some(lng)) => {
            serde_json::to_string(&serde_json::json!({ "lat": lat, "lng": lng }))
                .map(Some)
                .map_err(|e| format!("serialize geo: {e}"))
        }
        _ => Ok(None),
    }
}

async fn load_component_warnings(
    pool: &SqlitePool,
    component_id: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(component_id) = component_id else {
        return Ok(None);
    };
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT message FROM icalendar_component_projection_warnings
         WHERE component_id = ?
         ORDER BY sort_order ASC",
    )
    .bind(component_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load component warnings: {e}"))?;
    if rows.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&rows)
        .map(Some)
        .map_err(|e| format!("serialize component warnings: {e}"))
}
