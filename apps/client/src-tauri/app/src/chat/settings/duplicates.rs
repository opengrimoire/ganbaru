//! Duplicate provider configurations: two configurations that launch the same program against the
//! same account on this device. Labels, models, and behavior toggles do not tell them apart.

use super::discovery::resolve_executable_identity;
use ganbaru_chat_contracts::models::{
    ChatError, CredentialReferenceId, ProviderFamilyId, ProviderInstanceConfig, ProviderInstanceId,
};
use ganbaru_chat_providers::provider_home_identity;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Provider config keys that choose models or behavior rather than the program or account.
const NON_LAUNCH_PROVIDER_CONFIG_KEYS: &[&str] = &[
    "allowCustomModels",
    "customModelIds",
    "customModelLabels",
    "refreshMcpBeforeTurn",
];

/// What makes a configuration a separate launch: the family, the program and home as this device
/// resolves them, and every value passed to the provider process.
#[derive(Debug, PartialEq)]
struct LaunchIdentity<'a> {
    family_id: &'a ProviderFamilyId,
    executable: PathBuf,
    provider_home: Option<PathBuf>,
    launch_arguments: &'a [String],
    environment: &'a BTreeMap<String, String>,
    credential_references: &'a BTreeMap<String, CredentialReferenceId>,
    connection: Value,
}

fn launch_identity<'a>(
    configuration: &'a ProviderInstanceConfig,
    search_directories: &[PathBuf],
) -> Option<LaunchIdentity<'a>> {
    Some(LaunchIdentity {
        family_id: &configuration.family_id,
        executable: resolve_executable_identity(&configuration.executable, search_directories)?,
        provider_home: provider_home_identity(configuration),
        launch_arguments: &configuration.launch_arguments,
        environment: &configuration.environment,
        credential_references: &configuration.credential_references,
        connection: connection_settings(&configuration.provider_config.value),
    })
}

fn connection_settings(value: &Value) -> Value {
    let Value::Object(entries) = value else {
        return value.clone();
    };
    let connection: Map<String, Value> = entries
        .iter()
        .filter(|(key, value)| {
            let empty =
                value.is_null() || value.as_str().is_some_and(|text| text.trim().is_empty());
            !empty && !NON_LAUNCH_PROVIDER_CONFIG_KEYS.contains(&key.as_str())
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Value::Object(connection)
}

/// The first of `others` that launches like `configuration`, skipping `configuration` itself. A
/// configuration with no executable on this device cannot launch here and duplicates nothing.
pub(crate) fn find_duplicate<'a>(
    configuration: &ProviderInstanceConfig,
    others: &'a [ProviderInstanceConfig],
    search_directories: &[PathBuf],
) -> Option<&'a ProviderInstanceConfig> {
    let identity = launch_identity(configuration, search_directories)?;
    others.iter().find(|other| {
        other.instance_id != configuration.instance_id
            && launch_identity(other, search_directories).as_ref() == Some(&identity)
    })
}

/// The earlier configuration each later one duplicates, by the later one's instance ID, so
/// Settings can point at the copy to remove while the first stays unmarked.
pub(crate) fn duplicate_configurations(
    configurations: &[ProviderInstanceConfig],
    search_directories: &[PathBuf],
) -> BTreeMap<ProviderInstanceId, ProviderInstanceId> {
    configurations
        .iter()
        .enumerate()
        .filter_map(|(index, configuration)| {
            find_duplicate(configuration, &configurations[..index], search_directories).map(
                |original| {
                    (
                        configuration.instance_id.clone(),
                        original.instance_id.clone(),
                    )
                },
            )
        })
        .collect()
}

/// The save error for a configuration that duplicates `existing`, naming it so the user knows
/// which one to edit instead.
pub(crate) fn duplicate_configuration_error(existing: &ProviderInstanceConfig) -> ChatError {
    let mut error = ChatError::validation(
        "configuration",
        format!(
            "This configuration launches the same program and account as {}",
            existing.label
        ),
    );
    error.details = Some(Box::new(serde_json::json!({
        "reason": "duplicate_provider",
        "existingInstanceId": existing.instance_id,
        "existingLabel": existing.label,
    })));
    error
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        root: PathBuf,
        bin: PathBuf,
        home: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("ganbaru-duplicates-{name}-{}", std::process::id()));
            let bin = root.join("bin");
            let home = root.join("home");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::create_dir_all(&home).unwrap();
            for directory in [&root, &bin, &home] {
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }
            let program = bin.join("claude");
            std::fs::write(&program, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { root, bin, home }
        }

        fn configuration(&self, id: &str, executable: &str) -> ProviderInstanceConfig {
            serde_json::from_value(serde_json::json!({
                "schemaVersion": 1,
                "instanceId": id,
                "familyId": "claude",
                "label": format!("Claude {id}"),
                "enabled": true,
                "executable": executable,
                "providerHome": self.home,
                "launchArguments": [],
                "environment": {},
                "credentialReferences": {},
                "providerConfig": {
                    "schemaVersion": 1,
                    "value": { "allowCustomModels": false, "customModelIds": [] }
                }
            }))
            .unwrap()
        }

        fn search(&self) -> Vec<PathBuf> {
            vec![self.bin.clone()]
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn a_bare_name_and_a_path_to_the_same_program_are_duplicates() {
        let fixture = Fixture::new("spelling");
        let path = fixture.bin.join("claude");
        let existing = fixture.configuration("first", path.to_str().unwrap());
        let mut draft = fixture.configuration("second", "claude");
        draft.label = "Another label".to_string();
        draft.provider_config.value = serde_json::json!({
            "allowCustomModels": true,
            "customModelIds": ["custom"],
            "customModelLabels": { "custom": "Custom" },
        });
        let others = [existing];
        let duplicate = find_duplicate(&draft, &others, &fixture.search());
        assert_eq!(
            duplicate.map(|found| found.label.as_str()),
            Some("Claude first")
        );
    }

    #[test]
    fn a_different_home_arguments_or_connection_keeps_configurations_apart() {
        let fixture = Fixture::new("apart");
        let existing = fixture.configuration("first", "claude");
        let others = [existing];

        let mut other_home = fixture.configuration("second", "claude");
        other_home.provider_home = Some(fixture.root.to_string_lossy().into_owned());
        assert!(find_duplicate(&other_home, &others, &fixture.search()).is_none());

        let mut arguments = fixture.configuration("second", "claude");
        arguments.launch_arguments = vec!["--verbose".to_string()];
        assert!(find_duplicate(&arguments, &others, &fixture.search()).is_none());

        let mut connection = fixture.configuration("second", "claude");
        connection.provider_config.value =
            serde_json::json!({ "endpoint": "https://example.test" });
        assert!(find_duplicate(&connection, &others, &fixture.search()).is_none());
    }

    #[test]
    fn an_empty_connection_value_matches_a_missing_one() {
        let fixture = Fixture::new("nulls");
        let existing = fixture.configuration("first", "claude");
        let mut draft = fixture.configuration("second", "claude");
        draft.provider_config.value = serde_json::json!({ "endpoint": null, "auth_method": " " });
        assert!(find_duplicate(&draft, &[existing], &fixture.search()).is_some());
    }

    #[test]
    fn a_configuration_never_duplicates_itself_or_launches_without_a_program() {
        let fixture = Fixture::new("self");
        let saved = fixture.configuration("first", "claude");
        assert!(find_duplicate(&saved, std::slice::from_ref(&saved), &fixture.search()).is_none());
        let unlaunchable = fixture.configuration("second", "");
        let other = fixture.configuration("third", "");
        assert!(find_duplicate(&unlaunchable, &[other], &fixture.search()).is_none());
    }

    #[test]
    fn only_later_copies_are_marked_as_duplicates() {
        let fixture = Fixture::new("marks");
        let configurations = [
            fixture.configuration("first", "claude"),
            fixture.configuration("second", "claude"),
            fixture.configuration("third", fixture.bin.join("claude").to_str().unwrap()),
        ];
        let marks = duplicate_configurations(&configurations, &fixture.search());
        let first = configurations[0].instance_id.clone();
        assert_eq!(marks.len(), 2);
        assert!(!marks.contains_key(&first));
        assert!(marks.values().all(|original| original == &first));
    }

    #[test]
    fn the_error_names_the_existing_configuration() {
        let fixture = Fixture::new("error");
        let error = duplicate_configuration_error(&fixture.configuration("first", "claude"));
        let details = error.details.unwrap();
        assert_eq!(details["reason"], "duplicate_provider");
        assert_eq!(details["existingLabel"], "Claude first");
        assert!(error.message.contains("Claude first"));
    }
}
