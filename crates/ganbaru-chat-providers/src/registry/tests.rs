//! Provider version admission tests.

use super::*;

const RANGE: ProviderCompatibilityRange = ProviderCompatibilityRange {
    minimum: "0.147.0",
    maximum_tested: "0.150.2",
    ceiling_exclusive: Some("1.0.0"),
};

fn admit(version: &str) -> ProviderVersionAdmission {
    admit_version::<CoreVersion>(&RANGE, version).unwrap()
}

#[test]
fn admission_places_each_boundary() {
    for (version, admission) in [
        ("0.146.9", ProviderVersionAdmission::BelowMinimum),
        ("0.147.0", ProviderVersionAdmission::Supported),
        ("0.150.2", ProviderVersionAdmission::Supported),
        ("0.150.3", ProviderVersionAdmission::UntestedNewer),
        ("0.999.0", ProviderVersionAdmission::UntestedNewer),
        ("1.0.0", ProviderVersionAdmission::AtOrAboveCeiling),
        ("2.3.4", ProviderVersionAdmission::AtOrAboveCeiling),
    ] {
        assert_eq!(admit(version), admission, "{version}");
    }
}

#[test]
fn admission_reads_the_core_of_prefixed_and_suffixed_versions() {
    assert_eq!(
        admit("codex-cli 0.147.0"),
        ProviderVersionAdmission::Supported
    );
    assert_eq!(
        admit("0.151.0-alpha.2"),
        ProviderVersionAdmission::UntestedNewer
    );
    assert_eq!(
        admit("1.0.0-alpha.1"),
        ProviderVersionAdmission::AtOrAboveCeiling,
        "a prerelease of the ceiling orders as the ceiling"
    );
    assert_eq!(
        admit("0.147.0+build.7"),
        ProviderVersionAdmission::Supported
    );
}

#[test]
fn admission_marks_unreadable_versions_without_failing() {
    for version in ["", "codex", "0.147", "0.147.0.1", "0.x.0", "v0.147.0"] {
        assert_eq!(
            admit(version),
            ProviderVersionAdmission::Unreadable,
            "{version}"
        );
    }
}

#[test]
fn admission_without_a_ceiling_never_refuses_newer_versions() {
    let range = ProviderCompatibilityRange {
        ceiling_exclusive: None,
        ..RANGE
    };
    assert_eq!(
        admit_version::<CoreVersion>(&range, "9.0.0").unwrap(),
        ProviderVersionAdmission::UntestedNewer
    );
}

#[test]
fn an_unparseable_range_is_an_error() {
    let range = ProviderCompatibilityRange {
        minimum: "next",
        ..RANGE
    };
    assert!(admit_version::<CoreVersion>(&range, "0.147.0").is_err());
}

#[test]
fn only_refused_admissions_block_sessions() {
    assert!(admits_sessions(ProviderVersionAdmission::Supported));
    assert!(admits_sessions(ProviderVersionAdmission::UntestedNewer));
    assert!(admits_sessions(ProviderVersionAdmission::Unreadable));
    assert!(!admits_sessions(ProviderVersionAdmission::BelowMinimum));
    assert!(!admits_sessions(ProviderVersionAdmission::AtOrAboveCeiling));
}

#[test]
fn a_version_refusal_carries_its_admission_and_version() {
    let error = version_refusal(
        ProviderVersionAdmission::AtOrAboveCeiling,
        "1.2.0",
        "Provider 1.2.0 is unsupported".to_string(),
        true,
    );
    assert_eq!(error.code, ChatErrorCode::UnsupportedVersion);
    assert!(error.recoverable);
    assert_eq!(
        refused_version(&error),
        Some((
            ProviderVersionAdmission::AtOrAboveCeiling,
            "1.2.0".to_string()
        ))
    );
    let mut other = error.clone();
    other.code = ChatErrorCode::Protocol;
    assert_eq!(refused_version(&other), None);
    let plain = ChatError::new(ChatErrorCode::UnsupportedVersion, "unsupported", false);
    assert_eq!(refused_version(&plain), None);
}

#[test]
fn the_generic_acp_family_is_listed_and_creates_its_driver() {
    let registry = ProviderDriverRegistry;
    let family = ProviderFamilyId::new("acp").unwrap();
    let listed = registry
        .list_metadata()
        .into_iter()
        .find(|metadata| metadata.family_id == family)
        .unwrap();
    assert_eq!(listed, registry.metadata(&family));
    assert_eq!(listed.display_name, "ACP agent");
    assert_eq!(listed.protocol_name, "acp");
    assert_eq!(
        listed.implementation_status,
        ProviderImplementationStatus::Available
    );
    assert!(listed.default_executable_candidates.is_empty());
    assert_eq!(listed.minimum_tested_cli_version, None);
    let definition = PROVIDER_METADATA
        .iter()
        .find(|metadata| metadata.family_id == "acp")
        .unwrap();
    assert_eq!(
        listed.potential_capabilities.len(),
        definition.potential_capabilities.len()
    );
    for capability in definition.potential_capabilities {
        assert!(
            listed.potential_capabilities.contains(capability),
            "{capability:?}"
        );
    }

    let configuration: ProviderInstanceConfig = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "instanceId": "acp-instance-1",
        "familyId": "acp",
        "label": "Agent",
        "enabled": true,
        "executable": "fixture-agent",
        "providerHome": null,
        "launchArguments": [],
        "environment": {},
        "credentialReferences": {},
        "providerConfig": { "schemaVersion": 1, "value": { "authMethod": "api-key" } }
    }))
    .unwrap();
    let driver = registry.create_driver(configuration.clone()).unwrap();
    assert_eq!(driver.metadata().family_id, family);

    let mut cursor = configuration;
    cursor.family_id = ProviderFamilyId::new("cursor").unwrap();
    let error = registry.create_driver(cursor).err().unwrap();
    assert_eq!(error.field.as_deref(), Some("providerConfig.authMethod"));
}
