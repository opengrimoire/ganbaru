//! Provider status each device shares with its linked devices, one row per provider family.
//!
//! A desktop publishes whether it can run each provider family, the canonical catalog of model
//! names and option labels, and the canonical plan limits it last saw. A row changes only when
//! its content does, so a probe that finds nothing new writes nothing and sync carries nothing.
//! A family the device no longer has stays as an unavailable row with an empty catalog, since a
//! published key cannot be created again once deleted. Readers parse rows leniently: a row they
//! cannot read is no status, never an error.

use std::collections::BTreeMap;

use ganbaru_chat_contracts::events::RateLimitStatusEvent;
use ganbaru_chat_contracts::models::{
    ChatLinkedProviderStatusRead, ChatLinkedUsageLimitsRead, ProviderModel, ProviderModelCatalog,
    UtcTimestamp,
};
use ganbaru_sync_contracts::chat_provider_status::{
    canonical_catalog, canonical_limits, provider_status_key,
};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

use crate::models::{ChatError, ChatErrorCode, ChatResult};

/// How much newer an observation of unchanged plan limits must be before the row takes its
/// time, so readers that call limits stale after an hour keep current limits fresh while a
/// desktop that keeps seeing the same figures writes at most twice an hour.
pub const LIMITS_OBSERVATION_REFRESH_SECONDS: i64 = 30 * 60;

/// What one device shares about one provider family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderStatusContent {
    available: bool,
    catalog_data: String,
    limits: Option<(String, String)>,
}

impl ProviderStatusContent {
    /// The status of a family this device can or cannot run now, with the catalog and the
    /// plan limits it last saw, reduced to their canonical shared form.
    pub fn new(
        available: bool,
        catalog: Option<&ProviderModelCatalog>,
        limits: Option<(&RateLimitStatusEvent, &UtcTimestamp)>,
    ) -> Self {
        let catalog_value = catalog
            .and_then(|catalog| serde_json::to_value(catalog).ok())
            .unwrap_or(Value::Null);
        let limits = limits.and_then(|(status, observed_at)| {
            let value = serde_json::to_value(status).ok()?;
            Some((
                canonical_limits(&value).to_string(),
                observed_at.as_str().to_string(),
            ))
        });
        Self {
            available,
            catalog_data: canonical_catalog(&catalog_value).to_string(),
            limits,
        }
    }

    /// The status of a family this device no longer has.
    fn removed() -> Self {
        Self {
            available: false,
            catalog_data: canonical_catalog(&Value::Null).to_string(),
            limits: None,
        }
    }
}

/// Publishes this device's status of every provider family it has and marks every family it
/// published before but no longer has as unavailable, at `now`. Returns how many rows changed.
pub async fn publish_provider_statuses(
    pool: &SqlitePool,
    device_id: &str,
    statuses: &BTreeMap<String, ProviderStatusContent>,
    now: &UtcTimestamp,
) -> ChatResult<usize> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let published: Vec<String> = sqlx::query_scalar(
        "SELECT provider_family_id FROM chat_provider_device_status WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let removed = ProviderStatusContent::removed();
    let gone = published
        .iter()
        .filter(|family| !statuses.contains_key(*family))
        .map(|family| (family.as_str(), &removed));
    let mut changed = 0;
    for (family, content) in statuses
        .iter()
        .map(|(family, content)| (family.as_str(), content))
        .chain(gone)
    {
        let key = provider_status_key(device_id, family)
            .map_err(|_| validation_error("Provider status keys need a device and a family"))?;
        let (limits_data, limits_observed_at) =
            content.limits.as_ref().map_or((None, None), |(data, at)| {
                (Some(data.as_str()), Some(at.as_str()))
            });
        let result = sqlx::query(
            "INSERT INTO chat_provider_device_status
                (id, device_id, provider_family_id, available, status_changed_at, catalog_data,
                 catalog_changed_at, limits_data, limits_observed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET
                available = excluded.available,
                status_changed_at = CASE WHEN available IS NOT excluded.available
                    THEN excluded.status_changed_at ELSE status_changed_at END,
                catalog_data = excluded.catalog_data,
                catalog_changed_at = CASE WHEN catalog_data IS NOT excluded.catalog_data
                    THEN excluded.catalog_changed_at ELSE catalog_changed_at END,
                limits_data = excluded.limits_data,
                limits_observed_at = CASE WHEN limits_data IS NOT excluded.limits_data
                    OR (julianday(excluded.limits_observed_at) - julianday(limits_observed_at))
                        * 86400 >= ?10
                    THEN excluded.limits_observed_at ELSE limits_observed_at END
             WHERE available IS NOT excluded.available
                OR catalog_data IS NOT excluded.catalog_data
                OR limits_data IS NOT excluded.limits_data
                OR (julianday(excluded.limits_observed_at) - julianday(limits_observed_at))
                    * 86400 >= ?10",
        )
        .bind(key.as_str())
        .bind(device_id)
        .bind(family)
        .bind(i64::from(content.available))
        .bind(now.as_str())
        .bind(&content.catalog_data)
        .bind(now.as_str())
        .bind(limits_data)
        .bind(limits_observed_at)
        .bind(LIMITS_OBSERVATION_REFRESH_SECONDS)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        changed += usize::try_from(result.rows_affected()).unwrap_or(usize::MAX);
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(changed)
}

/// The status `device_id` shares for `family_id`, or `None` when it shares none or the row
/// cannot be read.
pub async fn read_provider_status(
    pool: &SqlitePool,
    device_id: &str,
    family_id: &str,
) -> ChatResult<Option<ChatLinkedProviderStatusRead>> {
    let row = sqlx::query(
        "SELECT available, catalog_data, limits_data, limits_observed_at
         FROM chat_provider_device_status
         WHERE device_id = ? AND provider_family_id = ?",
    )
    .bind(device_id)
    .bind(family_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let available: i64 = row.try_get("available").map_err(persistence_error)?;
    let catalog_data: String = row.try_get("catalog_data").map_err(persistence_error)?;
    let limits_data: Option<String> = row.try_get("limits_data").map_err(persistence_error)?;
    let limits_observed_at: Option<String> = row
        .try_get("limits_observed_at")
        .map_err(persistence_error)?;
    Ok(Some(ChatLinkedProviderStatusRead {
        device_id: device_id.to_string(),
        available: available == 1,
        models: parse_models(&catalog_data),
        usage_limits: parse_limits(limits_data.as_deref(), limits_observed_at.as_deref()),
    }))
}

/// The models of a stored catalog, skipping any model that does not parse.
fn parse_models(catalog_data: &str) -> Vec<ProviderModel> {
    let Ok(catalog) = serde_json::from_str::<Value>(catalog_data) else {
        return Vec::new();
    };
    canonical_catalog(&catalog)
        .get("models")
        .and_then(Value::as_array)
        .map(|models| models.iter().filter_map(parse_model).collect())
        .unwrap_or_default()
}

/// A canonical catalog model as a provider model without the fields a catalog leaves out.
fn parse_model(model: &Value) -> Option<ProviderModel> {
    let mut model = model.as_object()?.clone();
    model.insert("capabilities".to_string(), Value::Array(Vec::new()));
    serde_json::from_value(Value::Object(model)).ok()
}

fn parse_limits(
    limits_data: Option<&str>,
    observed_at: Option<&str>,
) -> Option<ChatLinkedUsageLimitsRead> {
    let limits = serde_json::from_str::<Value>(limits_data?).ok()?;
    let status = serde_json::from_value(canonical_limits(&limits)).ok()?;
    let observed_at = UtcTimestamp::new(observed_at?).ok()?;
    Some(ChatLinkedUsageLimitsRead {
        observed_at,
        status,
    })
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat provider status could not be saved or read",
        true,
    )
}

fn validation_error(message: &'static str) -> ChatError {
    ChatError::new(ChatErrorCode::Validation, message, false)
}

#[cfg(test)]
mod tests;
