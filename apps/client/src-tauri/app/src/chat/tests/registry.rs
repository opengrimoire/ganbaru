use ganbaru_chat_contracts::models::{ProviderImplementationStatus, ProviderInstanceConfig};
use ganbaru_chat_providers::{ProviderDriverFactory, ProviderDriverRegistry};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct CodexProbeFixture(PathBuf);

impl CodexProbeFixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-codex-registry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).expect("create isolated Codex probe home");
        Self(path)
    }

    fn configuration(&self) -> ProviderInstanceConfig {
        let home = fs::canonicalize(&self.0).expect("canonicalize Codex probe home");
        let mut configuration = configuration("codex");
        configuration.provider_home = Some(home.to_str().unwrap().to_string());
        configuration.executable = home.join("missing-codex").to_str().unwrap().to_string();
        configuration.provider_config = ganbaru_chat_contracts::models::VersionedJson {
            schema_version: 1,
            value: json!({}),
        };
        configuration
    }
}

impl Drop for CodexProbeFixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir(&self.0) {
            eprintln!("remove isolated Codex probe home: {error}");
        }
    }
}

fn configuration(family_id: &str) -> ProviderInstanceConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "instanceId": "provider-instance-1",
        "familyId": family_id,
        "label": "Local provider",
        "enabled": true,
        "executable": family_id,
        "providerHome": null,
        "launchArguments": [],
        "environment": {},
        "credentialReferences": {},
        "visibleModelIds": [],
        "favoriteModelIds": [],
        "providerConfig": { "schemaVersion": 7, "value": { "future": true } },
        "futureCommonField": { "nested": [1, 2, 3] }
    }))
    .unwrap()
}

#[test]
fn registry_lists_five_known_families_in_stable_order() {
    let metadata = ProviderDriverRegistry.list_metadata();
    let family_ids = metadata
        .iter()
        .map(|entry| entry.family_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        family_ids,
        ["codex", "claude", "cursor", "grok", "opencode"]
    );
    assert_eq!(
        metadata
            .iter()
            .map(|entry| entry.display_name.as_str())
            .collect::<Vec<_>>(),
        ["OpenAI", "Anthropic", "Cursor", "xAI", "OpenCode"]
    );
    assert_eq!(
        metadata[0].implementation_status,
        ProviderImplementationStatus::Available
    );
    assert!(metadata[0].unavailable_reason.is_none());
    assert_eq!(
        metadata[1].implementation_status,
        ProviderImplementationStatus::Available
    );
    assert!(metadata[1].unavailable_reason.is_none());
    assert_eq!(
        metadata[2].implementation_status,
        ProviderImplementationStatus::Available
    );
    assert!(metadata[2].unavailable_reason.is_none());
    assert_eq!(
        metadata[3].implementation_status,
        ProviderImplementationStatus::Available
    );
    assert!(metadata[3].unavailable_reason.is_none());
    assert_eq!(
        metadata[4].implementation_status,
        ProviderImplementationStatus::Available
    );
    assert!(metadata[4].unavailable_reason.is_none());
}

#[test]
fn opencode_driver_is_available_with_a_version_floor() {
    let mut configuration = configuration("opencode");
    configuration.provider_config = ganbaru_chat_contracts::models::VersionedJson {
        schema_version: 1,
        value: json!({ "mode": "local" }),
    };
    let driver = ProviderDriverRegistry.create_driver(configuration).unwrap();

    assert_eq!(
        driver.metadata().minimum_tested_cli_version.as_deref(),
        Some("1.14.19")
    );
    assert!(
        driver
            .capabilities()
            .supports(ganbaru_chat_contracts::models::ProviderCapability::NativePlan)
    );
    assert!(
        driver
            .capabilities()
            .entries
            .iter()
            .all(|entry| entry.supported)
    );
}

#[test]
fn cursor_driver_is_available_with_an_acp_version_floor() {
    let mut configuration = configuration("cursor");
    configuration.provider_config = ganbaru_chat_contracts::models::VersionedJson {
        schema_version: 1,
        value: json!({}),
    };
    let driver = ProviderDriverRegistry.create_driver(configuration).unwrap();

    assert_eq!(
        driver.metadata().minimum_tested_cli_version.as_deref(),
        Some("2026.04.08")
    );
    assert!(
        driver
            .capabilities()
            .entries
            .iter()
            .all(|entry| entry.supported)
    );
}

#[test]
fn grok_driver_is_available_with_native_acp_models() {
    let mut configuration = configuration("grok");
    configuration.provider_config = ganbaru_chat_contracts::models::VersionedJson {
        schema_version: 1,
        value: json!({}),
    };
    let driver = ProviderDriverRegistry.create_driver(configuration).unwrap();

    assert_eq!(driver.metadata().display_name, "xAI");
    assert_eq!(driver.metadata().default_executable_candidates, ["grok"]);
    assert!(
        driver
            .capabilities()
            .supports(ganbaru_chat_contracts::models::ProviderCapability::DynamicModelChange)
    );
    assert!(
        driver
            .capabilities()
            .supports(ganbaru_chat_contracts::models::ProviderCapability::StructuredQuestions)
    );
}

#[test]
fn claude_driver_is_available_with_a_version_floor() {
    let mut configuration = configuration("claude");
    configuration.provider_config = ganbaru_chat_contracts::models::VersionedJson {
        schema_version: 1,
        value: json!({}),
    };
    let driver = ProviderDriverRegistry.create_driver(configuration).unwrap();

    assert_eq!(
        driver.metadata().minimum_tested_cli_version.as_deref(),
        Some("2.1.170")
    );
    assert!(
        driver
            .capabilities()
            .entries
            .iter()
            .all(|entry| entry.supported)
    );
}

#[test]
fn unknown_family_and_configuration_round_trip_without_loss() {
    let input = serde_json::to_value(configuration("future-provider")).unwrap();
    let parsed: ProviderInstanceConfig = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), input);

    let metadata = ProviderDriverRegistry.metadata(&parsed.family_id);
    assert_eq!(metadata.family_id.as_str(), "future-provider");
    assert_eq!(
        metadata.implementation_status,
        ProviderImplementationStatus::Unsupported
    );

    let driver = ProviderDriverRegistry.create_driver(parsed).unwrap();
    assert_eq!(
        driver.instance_configuration().family_id.as_str(),
        "future-provider"
    );
    assert_eq!(
        driver
            .instance_configuration()
            .unknown_fields
            .get("futureCommonField"),
        Some(&json!({ "nested": [1, 2, 3] }))
    );
}

#[test]
fn codex_driver_is_available_with_declared_capabilities() {
    let fixture = CodexProbeFixture::new();
    let configuration = fixture.configuration();
    let mut driver = ProviderDriverRegistry.create_driver(configuration).unwrap();
    assert!(!driver.capabilities().entries.is_empty());
    assert!(
        driver
            .capabilities()
            .entries
            .iter()
            .all(|entry| entry.supported)
    );

    let context = ganbaru_chat_providers::DriverOperationContext {
        operation_id: "probe-1".to_string(),
        deadline: Instant::now() + Duration::from_secs(1),
        cancellation: Default::default(),
    };
    let result = tauri::async_runtime::block_on(driver.probe(&context)).unwrap();
    assert_eq!(
        result.state,
        ganbaru_chat_contracts::models::ProbeState::ExecutableMissing
    );
}

#[test]
fn codex_probe_reports_invalid_home_before_missing_executable() {
    let fixture = CodexProbeFixture::new();
    let mut configuration = fixture.configuration();
    configuration.provider_home =
        Some(fixture.0.join("missing-home").to_str().unwrap().to_string());
    let mut driver = ProviderDriverRegistry.create_driver(configuration).unwrap();
    let context = ganbaru_chat_providers::DriverOperationContext {
        operation_id: "probe-invalid-home".to_string(),
        deadline: Instant::now() + Duration::from_secs(1),
        cancellation: Default::default(),
    };
    let result = tauri::async_runtime::block_on(driver.probe(&context)).unwrap();
    assert_eq!(
        result.state,
        ganbaru_chat_contracts::models::ProbeState::ConfigurationInvalid
    );
}
