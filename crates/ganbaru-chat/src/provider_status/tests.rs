use ganbaru_chat_contracts::models::{
    ModelAvailability, ModelCatalogSource, ModelChoiceOption, ModelOptionDefinition,
    ProviderInstanceId, VersionedJson,
};
use serde_json::json;

use super::*;

const DEVICE: &str = "device-a";
const OTHER_DEVICE: &str = "device-b";
const FAMILY: &str = "codex";
const NOW: &str = "2026-11-19T10:00:00.000Z";
const LATER: &str = "2026-11-19T10:05:00.000Z";

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

fn at(value: &str) -> UtcTimestamp {
    UtcTimestamp::new(value).unwrap()
}

fn catalog(display_name: &str, discovered_at: &str) -> ProviderModelCatalog {
    ProviderModelCatalog {
        instance_id: ProviderInstanceId::new("instance-codex").unwrap(),
        models: vec![ProviderModel {
            id: ganbaru_chat_contracts::models::ModelId::new("gpt-5").unwrap(),
            display_name: display_name.to_string(),
            description: Some("Signed in as user@example.com".to_string()),
            context_limit: Some(400_000),
            availability: ModelAvailability::Available,
            capabilities: Vec::new(),
            options: vec![ModelOptionDefinition::Choice {
                key: "reasoning_effort".to_string(),
                label: "Reasoning".to_string(),
                description: Some("How hard it thinks".to_string()),
                options: vec![ModelChoiceOption {
                    value: "high".to_string(),
                    label: "High".to_string(),
                    description: None,
                }],
                default_value: Some("high".to_string()),
            }],
        }],
        recommended_model_id: None,
        source: ModelCatalogSource::Provider,
        discovered_at: at(discovered_at),
        stale: false,
    }
}

fn limits(used_percent: f64) -> RateLimitStatusEvent {
    RateLimitStatusEvent {
        limited: false,
        resets_at: None,
        detail: Some("Plan for user@example.com".to_string()),
        provider_data: Some(VersionedJson {
            schema_version: 1,
            value: json!({
                "account": {"email": "user@example.com"},
                "rateLimitsByLimitId": {
                    "codex": {"limitName": "Codex", "primary": {"usedPercent": used_percent}}
                }
            }),
        }),
    }
}

fn statuses(content: ProviderStatusContent) -> BTreeMap<String, ProviderStatusContent> {
    BTreeMap::from([(FAMILY.to_string(), content)])
}

async fn captured(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM sync_capture")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn clear_capture(pool: &SqlitePool) {
    sqlx::query("DELETE FROM sync_capture")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn publishing_strips_private_fields_and_reads_back() {
    let pool = migrated_pool().await;
    let catalog = catalog("GPT-5", NOW);
    let limits = limits(42.0);
    let observed = at(NOW);
    let content = ProviderStatusContent::new(true, Some(&catalog), Some((&limits, &observed)));
    assert_eq!(
        publish_provider_statuses(&pool, DEVICE, &statuses(content), &at(NOW))
            .await
            .unwrap(),
        1
    );

    let stored: (String, String) = sqlx::query_as(
        "SELECT catalog_data, limits_data FROM chat_provider_device_status",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    for data in [&stored.0, &stored.1] {
        assert!(!data.contains("user@example.com"), "{data}");
        assert!(!data.contains("instance-codex"), "{data}");
    }

    let read = read_provider_status(&pool, DEVICE, FAMILY)
        .await
        .unwrap()
        .expect("a status");
    assert!(read.available);
    assert_eq!(read.device_id, DEVICE);
    assert_eq!(read.models.len(), 1);
    assert_eq!(read.models[0].display_name, "GPT-5");
    assert_eq!(read.models[0].description, None);
    assert!(matches!(
        &read.models[0].options[0],
        ModelOptionDefinition::Choice { label, options, .. }
            if label == "Reasoning" && options[0].label == "High"
    ));
    let usage = read.usage_limits.expect("limits");
    assert_eq!(usage.observed_at, observed);
    assert_eq!(usage.status.detail, None);
    assert_eq!(
        usage.status.provider_data.expect("provider data").value,
        json!({"rateLimitsByLimitId": {"codex": {"limitName": "Codex", "primary": {"usedPercent": 42.0}}}})
    );
    assert!(
        read_provider_status(&pool, OTHER_DEVICE, FAMILY)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn republishing_the_same_content_writes_nothing() {
    let pool = migrated_pool().await;
    let first = ProviderStatusContent::new(true, Some(&catalog("GPT-5", NOW)), None);
    publish_provider_statuses(&pool, DEVICE, &statuses(first), &at(NOW))
        .await
        .unwrap();
    clear_capture(&pool).await;

    let probed_again = ProviderStatusContent::new(true, Some(&catalog("GPT-5", LATER)), None);
    assert_eq!(
        publish_provider_statuses(&pool, DEVICE, &statuses(probed_again), &at(LATER))
            .await
            .unwrap(),
        0
    );
    assert_eq!(captured(&pool).await, 0);

    let renamed = ProviderStatusContent::new(true, Some(&catalog("GPT-5.1", LATER)), None);
    assert_eq!(
        publish_provider_statuses(&pool, DEVICE, &statuses(renamed), &at(LATER))
            .await
            .unwrap(),
        1
    );
    let (status_at, catalog_at): (String, String) = sqlx::query_as(
        "SELECT status_changed_at, catalog_changed_at FROM chat_provider_device_status",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((status_at.as_str(), catalog_at.as_str()), (NOW, LATER));
}

#[tokio::test]
async fn a_removed_family_stays_unavailable_with_an_empty_catalog() {
    let pool = migrated_pool().await;
    let observed = at(NOW);
    let content = ProviderStatusContent::new(
        true,
        Some(&catalog("GPT-5", NOW)),
        Some((&limits(10.0), &observed)),
    );
    publish_provider_statuses(&pool, DEVICE, &statuses(content), &at(NOW))
        .await
        .unwrap();

    assert_eq!(
        publish_provider_statuses(&pool, DEVICE, &BTreeMap::new(), &at(LATER))
            .await
            .unwrap(),
        1
    );
    let read = read_provider_status(&pool, DEVICE, FAMILY)
        .await
        .unwrap()
        .expect("the row stays");
    assert!(!read.available);
    assert!(read.models.is_empty());
    assert!(read.usage_limits.is_none());
    assert_eq!(
        publish_provider_statuses(&pool, DEVICE, &BTreeMap::new(), &at(LATER))
            .await
            .unwrap(),
        0
    );

    let returned = ProviderStatusContent::new(true, Some(&catalog("GPT-5", LATER)), None);
    assert_eq!(
        publish_provider_statuses(&pool, DEVICE, &statuses(returned), &at(LATER))
            .await
            .unwrap(),
        1
    );
    assert!(
        read_provider_status(&pool, DEVICE, FAMILY)
            .await
            .unwrap()
            .expect("a status")
            .available
    );
}

#[tokio::test]
async fn publishing_leaves_other_devices_rows_alone() {
    let pool = migrated_pool().await;
    let content = ProviderStatusContent::new(true, Some(&catalog("GPT-5", NOW)), None);
    publish_provider_statuses(&pool, OTHER_DEVICE, &statuses(content), &at(NOW))
        .await
        .unwrap();
    publish_provider_statuses(&pool, DEVICE, &BTreeMap::new(), &at(LATER))
        .await
        .unwrap();
    assert!(
        read_provider_status(&pool, OTHER_DEVICE, FAMILY)
            .await
            .unwrap()
            .expect("a status")
            .available
    );
}
