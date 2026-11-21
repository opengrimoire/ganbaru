//! Keys, bounds, and canonical payloads of the provider status each desktop shares.
//!
//! A desktop that runs Chat agents tells its linked devices, one row per provider family,
//! whether it can run that family now, the model names and option labels its catalog lists,
//! and the plan limits the provider last reported. Sync admission and the Chat writer both read
//! this module, so a status one replica writes is a status every replica accepts.
//!
//! Payloads are JSON objects reduced to a whitelist: model ids, display names, availability,
//! and the labels of choice and on or off options for the catalog, and the usage windows
//! `providerPlanLimits` reads for the limits. Descriptions, account details, raw provider
//! payloads, paths, and executables never enter a payload, and admission refuses any value that
//! is not already in its canonical form.

use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};

use crate::{DerivedKeyError, RowKey, derived_key, device_owner_tag};

/// Prefix of the derived part of a provider status key, over the device and provider family.
pub const PROVIDER_STATUS_KEY_PREFIX: &str = "chat-provider-status";
/// What a provider status key starts with: [`PROVIDER_STATUS_KEY_PREFIX`] and its separator,
/// followed by the owner tag of the device, `:`, and the digest of the derived key.
pub const PROVIDER_STATUS_OWNER_PREFIX: &str = "chat-provider-status:";
/// Characters of a provider family id, as Chat agents store it.
pub const MAX_PROVIDER_FAMILY_CHARS: usize = 1_024;
/// Characters of a canonical catalog payload.
pub const MAX_CATALOG_DATA_CHARS: usize = 32_768;
/// Characters of a canonical limits payload.
pub const MAX_LIMITS_DATA_CHARS: usize = 16_384;
/// Characters of a timestamp a status row records.
pub const MAX_STATUS_TIMESTAMP_CHARS: usize = 64;
/// Models a catalog payload lists.
pub const MAX_CATALOG_MODELS: usize = 64;
/// Options one catalog model lists.
pub const MAX_MODEL_OPTIONS: usize = 8;
/// Choices one catalog option lists.
pub const MAX_OPTION_CHOICES: usize = 16;
/// Characters of a model id, display name, option key, label, or choice value in a catalog.
pub const MAX_CATALOG_TEXT_CHARS: usize = 128;
/// Limit buckets one limits map lists.
pub const MAX_LIMIT_BUCKETS: usize = 8;
/// Characters of a limit bucket key.
pub const MAX_LIMIT_KEY_CHARS: usize = 64;
/// Characters of a limit name, limit id, or limit state.
pub const MAX_LIMIT_TEXT_CHARS: usize = 128;

/// Model availabilities a catalog payload names; anything else reads as `unknown`.
const MODEL_AVAILABILITIES: [&str; 5] =
    ["available", "unavailable", "deprecated", "stale", "unknown"];
/// Usage windows of a limit bucket keyed by limit id.
const LIMIT_WINDOWS: [&str; 2] = ["primary", "secondary"];

/// The replicated key of `device_id`'s status for `family_id`: the owner tag of the device,
/// which lets every replica refuse a status from another device, followed by the digest derived
/// from both values.
pub fn provider_status_key(device_id: &str, family_id: &str) -> Result<RowKey, DerivedKeyError> {
    let derived = derived_key(PROVIDER_STATUS_KEY_PREFIX, &[device_id, family_id])?;
    let digest = derived
        .as_str()
        .strip_prefix(PROVIDER_STATUS_OWNER_PREFIX)
        .ok_or(DerivedKeyError::Prefix)?;
    RowKey::new(format!(
        "{PROVIDER_STATUS_OWNER_PREFIX}{tag}:{digest}",
        tag = device_owner_tag(device_id)
    ))
    .map_err(|_| DerivedKeyError::Prefix)
}

/// The canonical catalog payload of a provider model catalog in its camelCase JSON form, or of
/// a payload already canonical, which it returns unchanged.
///
/// It keeps each model's id, display name, and availability, and its choice and on or off
/// options with their keys, labels, defaults, and choice values and labels. Entries whose text
/// is empty or too long are left out, lists are cut to their bounds, and models are dropped from
/// the end until the payload fits [`MAX_CATALOG_DATA_CHARS`].
pub fn canonical_catalog(catalog: &Value) -> Value {
    let mut models: Vec<Value> = catalog
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(canonical_model)
                .take(MAX_CATALOG_MODELS)
                .collect()
        })
        .unwrap_or_default();
    loop {
        let payload = catalog_payload(models.clone());
        if payload_chars(&payload) <= MAX_CATALOG_DATA_CHARS || models.pop().is_none() {
            return payload;
        }
    }
}

/// Whether `data` is a canonical catalog payload within its bound.
pub fn is_canonical_catalog(data: &str) -> bool {
    is_canonical(data, MAX_CATALOG_DATA_CHARS, canonical_catalog)
}

/// The canonical limits payload of a rate limit status in its camelCase JSON form, or of a
/// payload already canonical, which it returns unchanged.
///
/// It keeps whether the provider is limited, when that ends, and the usage windows of the
/// `rateLimitsByLimitId` and `rateLimitsByType` provider data shapes; the detail text and every
/// other provider field are left out. Provider data that keeps no window, or that would not fit
/// [`MAX_LIMITS_DATA_CHARS`], is left out whole.
pub fn canonical_limits(status: &Value) -> Value {
    let limited = status
        .get("limited")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let resets_at = status
        .get("resetsAt")
        .and_then(|value| bounded_text(value, MAX_STATUS_TIMESTAMP_CHARS))
        .map_or(Value::Null, Value::String);
    let provider_data = status
        .get("providerData")
        .and_then(canonical_provider_data)
        .unwrap_or(Value::Null);
    let payload = limits_payload(limited, resets_at.clone(), provider_data);
    if payload_chars(&payload) <= MAX_LIMITS_DATA_CHARS {
        payload
    } else {
        limits_payload(limited, resets_at, Value::Null)
    }
}

/// Whether `data` is a canonical limits payload within its bound.
pub fn is_canonical_limits(data: &str) -> bool {
    is_canonical(data, MAX_LIMITS_DATA_CHARS, canonical_limits)
}

fn is_canonical(data: &str, max_chars: usize, canonical: fn(&Value) -> Value) -> bool {
    if data.chars().count() > max_chars {
        return false;
    }
    match serde_json::from_str::<Value>(data) {
        Ok(value) => value.is_object() && canonical(&value) == value,
        Err(_) => false,
    }
}

fn payload_chars(payload: &Value) -> usize {
    payload.to_string().chars().count()
}

fn catalog_payload(models: Vec<Value>) -> Value {
    object([("models", Value::Array(models))])
}

fn limits_payload(limited: bool, resets_at: Value, provider_data: Value) -> Value {
    object([
        ("limited", Value::Bool(limited)),
        ("resetsAt", resets_at),
        ("providerData", provider_data),
    ])
}

fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

/// Text of 1 to `max_chars` characters with no NUL, or `None`.
fn bounded_text(value: &Value, max_chars: usize) -> Option<String> {
    let text = value.as_str()?;
    let chars = text.chars().count();
    (chars >= 1 && chars <= max_chars && !text.contains('\0')).then(|| text.to_string())
}

fn catalog_text(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| bounded_text(value, MAX_CATALOG_TEXT_CHARS))
}

fn canonical_model(model: &Value) -> Option<Value> {
    let id = catalog_text(model.get("id"))?;
    let display_name = catalog_text(model.get("displayName"))?;
    let availability = model
        .get("availability")
        .and_then(Value::as_str)
        .filter(|availability| MODEL_AVAILABILITIES.contains(availability))
        .unwrap_or("unknown");
    let options = model
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(canonical_option)
                .take(MAX_MODEL_OPTIONS)
                .collect()
        })
        .unwrap_or_default();
    Some(object([
        ("id", Value::String(id)),
        ("displayName", Value::String(display_name)),
        ("availability", Value::String(availability.to_string())),
        ("options", Value::Array(options)),
    ]))
}

fn canonical_option(option: &Value) -> Option<Value> {
    let key = catalog_text(option.get("key"))?;
    let label = catalog_text(option.get("label"))?;
    match option.get("kind").and_then(Value::as_str)? {
        "boolean" => Some(object([
            ("kind", Value::String("boolean".to_string())),
            ("key", Value::String(key)),
            ("label", Value::String(label)),
            (
                "defaultValue",
                option
                    .get("defaultValue")
                    .and_then(Value::as_bool)
                    .map_or(Value::Null, Value::Bool),
            ),
        ])),
        "choice" => {
            let choices: Vec<Value> = option
                .get("options")
                .and_then(Value::as_array)
                .map(|choices| {
                    choices
                        .iter()
                        .filter_map(canonical_choice)
                        .take(MAX_OPTION_CHOICES)
                        .collect()
                })
                .unwrap_or_default();
            if choices.is_empty() {
                return None;
            }
            Some(object([
                ("kind", Value::String("choice".to_string())),
                ("key", Value::String(key)),
                ("label", Value::String(label)),
                (
                    "defaultValue",
                    catalog_text(option.get("defaultValue")).map_or(Value::Null, Value::String),
                ),
                ("options", Value::Array(choices)),
            ]))
        }
        _ => None,
    }
}

fn canonical_choice(choice: &Value) -> Option<Value> {
    Some(object([
        ("value", Value::String(catalog_text(choice.get("value"))?)),
        ("label", Value::String(catalog_text(choice.get("label"))?)),
    ]))
}

fn canonical_provider_data(provider_data: &Value) -> Option<Value> {
    let schema_version = provider_data
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .filter(|version| u32::try_from(*version).is_ok())?;
    let value = provider_data.get("value")?;
    let mut limits = Map::new();
    if let Some(buckets) =
        canonical_buckets(value.get("rateLimitsByLimitId"), canonical_limit_bucket)
    {
        limits.insert("rateLimitsByLimitId".to_string(), buckets);
    }
    if let Some(types) = canonical_buckets(value.get("rateLimitsByType"), canonical_limit_type) {
        limits.insert("rateLimitsByType".to_string(), types);
    }
    if limits.is_empty() {
        return None;
    }
    Some(object([
        ("schemaVersion", Value::Number(Number::from(schema_version))),
        ("value", Value::Object(limits)),
    ]))
}

/// The entries of a limits map that keep a window, under bounded keys, in key order so the
/// same limits always keep the same buckets and serialize the same way.
fn canonical_buckets(map: Option<&Value>, canonical: fn(&Value) -> Option<Value>) -> Option<Value> {
    let sorted: BTreeMap<&String, &Value> = map?.as_object()?.iter().collect();
    let buckets: Map<String, Value> = sorted
        .into_iter()
        .filter(|(key, _)| {
            let chars = key.chars().count();
            (1..=MAX_LIMIT_KEY_CHARS).contains(&chars) && !key.contains('\0')
        })
        .filter_map(|(key, bucket)| Some((key.clone(), canonical(bucket)?)))
        .take(MAX_LIMIT_BUCKETS)
        .collect();
    (!buckets.is_empty()).then_some(Value::Object(buckets))
}

/// A bucket of the limit id shape: its names and the windows with a used share.
fn canonical_limit_bucket(bucket: &Value) -> Option<Value> {
    let mut kept = Map::new();
    for name in ["limitName", "limitId"] {
        if let Some(text) = bucket
            .get(name)
            .and_then(|value| bounded_text(value, MAX_LIMIT_TEXT_CHARS))
        {
            kept.insert(name.to_string(), Value::String(text));
        }
    }
    let mut windows = 0;
    for name in LIMIT_WINDOWS {
        if let Some(window) = bucket.get(name).and_then(canonical_window) {
            kept.insert(name.to_string(), window);
            windows += 1;
        }
    }
    (windows > 0).then_some(Value::Object(kept))
}

fn canonical_window(window: &Value) -> Option<Value> {
    let mut kept = Map::new();
    kept.insert(
        "usedPercent".to_string(),
        number(window.get("usedPercent"))?,
    );
    for name in ["windowDurationMins", "resetsAt"] {
        if let Some(value) = number(window.get(name)) {
            kept.insert(name.to_string(), value);
        }
    }
    Some(Value::Object(kept))
}

/// An entry of the limit type shape: its utilization, state, and reset time.
fn canonical_limit_type(entry: &Value) -> Option<Value> {
    let mut kept = Map::new();
    if let Some(utilization) = number(entry.get("utilization")) {
        kept.insert("utilization".to_string(), utilization);
    }
    if let Some(status) = entry
        .get("status")
        .and_then(|value| bounded_text(value, MAX_LIMIT_TEXT_CHARS))
    {
        kept.insert("status".to_string(), Value::String(status));
    }
    if kept.is_empty() {
        return None;
    }
    if let Some(resets_at) = number(entry.get("resetsAt")) {
        kept.insert("resetsAt".to_string(), resets_at);
    }
    Some(Value::Object(kept))
}

fn number(value: Option<&Value>) -> Option<Value> {
    value
        .and_then(Value::as_number)
        .map(|number| Value::Number(number.clone()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn provider_status_key_names_the_device_owner_tag() {
        let key = provider_status_key("device-a", "codex").expect("valid key");
        let tag = device_owner_tag("device-a");
        assert!(
            key.as_str()
                .starts_with(&format!("{PROVIDER_STATUS_OWNER_PREFIX}{tag}:"))
        );
        assert_ne!(
            key,
            provider_status_key("device-b", "codex").expect("valid key")
        );
        assert_ne!(
            key,
            provider_status_key("device-a", "claude").expect("valid key")
        );
    }

    #[test]
    fn catalog_keeps_names_and_option_labels_only() {
        let catalog = json!({
            "providerInstanceId": "instance-1",
            "fetchedAt": "2026-11-19T10:00:00Z",
            "models": [{
                "id": "gpt-5",
                "displayName": "GPT-5",
                "description": "Account user@example.com",
                "contextLimit": 400000,
                "availability": "available",
                "capabilities": {"images": true},
                "options": [
                    {
                        "kind": "choice",
                        "key": "reasoning_effort",
                        "label": "Reasoning",
                        "description": "How hard it thinks",
                        "defaultValue": "medium",
                        "options": [
                            {"value": "low", "label": "Low", "description": "Fast"},
                            {"value": "medium", "label": "Medium"}
                        ]
                    },
                    {"kind": "boolean", "key": "fast", "label": "Fast mode", "defaultValue": false},
                    {"kind": "text", "key": "path", "label": "Path"}
                ]
            }]
        });
        assert_eq!(
            canonical_catalog(&catalog),
            json!({"models": [{
                "id": "gpt-5",
                "displayName": "GPT-5",
                "availability": "available",
                "options": [
                    {
                        "kind": "choice",
                        "key": "reasoning_effort",
                        "label": "Reasoning",
                        "defaultValue": "medium",
                        "options": [
                            {"value": "low", "label": "Low"},
                            {"value": "medium", "label": "Medium"}
                        ]
                    },
                    {"kind": "boolean", "key": "fast", "label": "Fast mode", "defaultValue": false}
                ]
            }]})
        );
    }

    #[test]
    fn catalog_drops_out_of_bounds_entries_and_fits_its_bound() {
        let long = "x".repeat(MAX_CATALOG_TEXT_CHARS + 1);
        let model = |index: usize| {
            json!({
                "id": format!("model-{index}"),
                "displayName": "y".repeat(MAX_CATALOG_TEXT_CHARS),
                "availability": "retired",
                "options": (0..MAX_MODEL_OPTIONS + 2).map(|option| json!({
                    "kind": "choice",
                    "key": format!("option-{option}"),
                    "label": "z".repeat(MAX_CATALOG_TEXT_CHARS),
                    "options": (0..MAX_OPTION_CHOICES + 2).map(|choice| json!({
                        "value": format!("choice-{choice}"),
                        "label": "w".repeat(MAX_CATALOG_TEXT_CHARS),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        };
        let mut models: Vec<Value> = (0..MAX_CATALOG_MODELS + 4).map(model).collect();
        models.insert(0, json!({"id": long, "displayName": "Too long"}));
        models.insert(1, json!({"id": "", "displayName": "Empty"}));
        let canonical = canonical_catalog(&json!({ "models": models }));
        let listed = canonical["models"].as_array().expect("models");
        assert!(!listed.is_empty());
        assert!(listed.len() <= MAX_CATALOG_MODELS);
        assert_eq!(listed[0]["id"], "model-0");
        assert_eq!(listed[0]["availability"], "unknown");
        let options = listed[0]["options"].as_array().expect("options");
        assert_eq!(options.len(), MAX_MODEL_OPTIONS);
        assert_eq!(
            options[0]["options"].as_array().expect("choices").len(),
            MAX_OPTION_CHOICES
        );
        let data = canonical.to_string();
        assert!(data.chars().count() <= MAX_CATALOG_DATA_CHARS);
        assert!(is_canonical_catalog(&data));
    }

    #[test]
    fn canonical_catalog_is_idempotent_and_refuses_extra_fields() {
        let canonical = canonical_catalog(&json!({"models": [{
            "id": "m", "displayName": "M", "availability": "stale", "options": []
        }]}));
        assert_eq!(canonical_catalog(&canonical), canonical);
        assert!(is_canonical_catalog(&canonical.to_string()));
        assert!(is_canonical_catalog(r#"{"models":[]}"#));
        assert!(!is_canonical_catalog(
            r#"{"models":[{"id":"m","displayName":"M","availability":"stale","options":[],"description":"x"}]}"#
        ));
        assert!(!is_canonical_catalog(
            r#"{"models":[],"path":"/home/user"}"#
        ));
        assert!(!is_canonical_catalog("[]"));
        assert!(!is_canonical_catalog("not json"));
    }

    #[test]
    fn limits_keep_usage_windows_and_drop_detail() {
        let status = json!({
            "limited": true,
            "resetsAt": "2026-11-19T12:00:00Z",
            "detail": "Signed in as user@example.com",
            "providerData": {
                "schemaVersion": 1,
                "value": {
                    "account": {"email": "user@example.com"},
                    "rateLimitsByLimitId": {
                        "codex": {
                            "limitName": "Codex",
                            "limitId": "codex",
                            "planType": "pro",
                            "primary": {"usedPercent": 42.5, "windowDurationMins": 300, "resetsAt": 1763560000},
                            "secondary": {"windowDurationMins": 10080}
                        },
                        "empty": {"limitName": "Nothing"}
                    },
                    "rateLimitsByType": {
                        "five_hour": {"utilization": 0.4, "status": "allowed", "resetsAt": 1763560000, "raw": "x"}
                    }
                }
            }
        });
        assert_eq!(
            canonical_limits(&status),
            json!({
                "limited": true,
                "resetsAt": "2026-11-19T12:00:00Z",
                "providerData": {
                    "schemaVersion": 1,
                    "value": {
                        "rateLimitsByLimitId": {
                            "codex": {
                                "limitName": "Codex",
                                "limitId": "codex",
                                "primary": {"usedPercent": 42.5, "windowDurationMins": 300, "resetsAt": 1763560000}
                            }
                        },
                        "rateLimitsByType": {
                            "five_hour": {"utilization": 0.4, "status": "allowed", "resetsAt": 1763560000}
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn limits_without_windows_keep_only_the_limited_state() {
        let canonical = canonical_limits(&json!({
            "limited": false,
            "providerData": {"schemaVersion": 1, "value": {"plan": "free"}}
        }));
        assert_eq!(
            canonical,
            json!({"limited": false, "resetsAt": null, "providerData": null})
        );
        assert_eq!(canonical_limits(&canonical), canonical);
        assert!(is_canonical_limits(&canonical.to_string()));
        assert!(!is_canonical_limits(
            r#"{"limited":false,"resetsAt":null,"providerData":null,"detail":"x"}"#
        ));
        assert!(!is_canonical_limits(&"x".repeat(MAX_LIMITS_DATA_CHARS + 1)));
    }

    #[test]
    fn limit_buckets_are_bounded() {
        let buckets: Map<String, Value> = (0..MAX_LIMIT_BUCKETS + 4)
            .map(|index| {
                (
                    format!("bucket-{index:02}"),
                    json!({"primary": {"usedPercent": index}}),
                )
            })
            .chain([(
                "k".repeat(MAX_LIMIT_KEY_CHARS + 1),
                json!({"primary": {"usedPercent": 1}}),
            )])
            .collect();
        let canonical = canonical_limits(&json!({
            "limited": false,
            "providerData": {"schemaVersion": 2, "value": {"rateLimitsByLimitId": buckets}}
        }));
        let kept = canonical["providerData"]["value"]["rateLimitsByLimitId"]
            .as_object()
            .expect("buckets");
        assert_eq!(kept.len(), MAX_LIMIT_BUCKETS);
        assert!(kept.keys().all(|key| key.starts_with("bucket-")));
        assert!(is_canonical_limits(&canonical.to_string()));
    }
}
