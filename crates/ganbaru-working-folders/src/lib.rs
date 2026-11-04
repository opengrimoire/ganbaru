use chrono::Timelike;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{collections::BTreeMap, fmt};

mod managed_folder;

pub use managed_folder::{
    MANAGED_PROJECTS_DIRECTORY, managed_folder_id, managed_folder_project,
    managed_project_relative_path, resolve_managed_project_folder, validate_managed_folder_name,
};

/// Folder ids become sync row keys, so they stay short. Managed folder ids come from
/// [`managed_folder_id`] and client folder ids are `working-folder:external:{uuid}`, which no
/// project's managed folder id can equal.
const MAX_IDENTIFIER_BYTES: usize = 64;

/// Stable identifier for a project working folder.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProjectWorkingFolderId(String);

impl ProjectWorkingFolderId {
    /// Validates and constructs a project working-folder identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    /// Returns the identifier as its wire string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the identifier and returns its wire string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl TryFrom<String> for ProjectWorkingFolderId {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Display for ProjectWorkingFolderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ProjectWorkingFolderId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProjectWorkingFolderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

fn validate_identifier(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("project working folder ID is required".to_string());
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(format!(
            "project working folder ID exceeds the {MAX_IDENTIFIER_BYTES} byte limit"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("project working folder ID contains a control character".to_string());
    }
    Ok(())
}

/// Byte position of the separator between the date and the time.
const TIMESTAMP_SEPARATOR_INDEX: usize = 10;

/// Offsets the frontend reader accepts.
const UTC_OFFSET_SUFFIXES: [&str; 3] = ["Z", "+00:00", "-00:00"];

/// Sub-second values from here on represent a leap second in chrono.
const LEAP_SECOND_NANOSECONDS: u32 = 1_000_000_000;

/// Validated RFC 3339 timestamp whose offset is UTC.
///
/// The accepted form is the one `readUtcTimestamp` in the frontend accepts, a subset of what
/// chrono parses as RFC 3339: an uppercase `T` separator, an offset of `Z`, `+00:00`, or
/// `-00:00`, and seconds at most 59. A value outside that subset would pass here and then fail
/// the whole response in the frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UtcTimestamp(String);

impl UtcTimestamp {
    /// Validates and constructs a UTC timestamp.
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let parsed = chrono::DateTime::parse_from_rfc3339(&value)
            .map_err(|_| "timestamp must use RFC 3339".to_string())?;
        if parsed.offset().local_minus_utc() != 0 {
            return Err("timestamp must use UTC".to_string());
        }
        let uppercase_separator = value.as_bytes().get(TIMESTAMP_SEPARATOR_INDEX) == Some(&b'T');
        let utc_suffix = UTC_OFFSET_SUFFIXES
            .iter()
            .any(|suffix| value.ends_with(suffix));
        let leap_second = parsed.nanosecond() >= LEAP_SECOND_NANOSECONDS;
        if !uppercase_separator || !utc_suffix || leap_second {
            return Err("timestamp must use the canonical RFC 3339 UTC form".to_string());
        }
        Ok(Self(value))
    }

    /// Returns the timestamp as its wire string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Repository implementation associated with a working folder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryKind {
    Git,
    None,
}

pub const WORKING_FOLDER_DEVICE_STATE_SCHEMA_VERSION: u32 = 1;

/// Device-local binding between a working-folder ID and filesystem path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkingFolderBindingState {
    pub canonical_path: String,
    pub filesystem_identity: String,
    pub repository_kind: RepositoryKind,
    #[serde(deserialize_with = "required_nullable")]
    pub repository_identity: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub repository_storage_identity: Option<String>,
    pub last_verified_at: UtcTimestamp,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// Device-local decision about whether a bound folder's repository programs,
/// hooks, and provider configuration may run.
///
/// The decision covers the configuration whose digests it records. When the
/// folder's configuration changes, the decision no longer applies and the
/// folder runs untrusted until the user decides again.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingFolderTrustRecord {
    /// Hexadecimal digest of repository programs and hooks.
    pub git_configuration_digest: String,
    /// Hexadecimal digest of project provider configuration.
    pub provider_configuration_digest: String,
    pub trusted: bool,
    pub decided_at: UtcTimestamp,
}

/// Device-local working-folder bindings and project selections for one scope.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingFolderDeviceScope {
    pub bindings: BTreeMap<ProjectWorkingFolderId, ProjectWorkingFolderBindingState>,
    pub last_selected_by_project: BTreeMap<String, ProjectWorkingFolderId>,
    /// Trust decisions by folder. A folder without a record is untrusted.
    #[serde(default)]
    pub trust: BTreeMap<ProjectWorkingFolderId, WorkingFolderTrustRecord>,
}

/// Versioned working-folder device state partitioned by vault and device.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingFolderDeviceState {
    pub schema_version: u32,
    pub vaults: BTreeMap<String, BTreeMap<String, WorkingFolderDeviceScope>>,
}

impl Default for WorkingFolderDeviceState {
    fn default() -> Self {
        Self {
            schema_version: WORKING_FOLDER_DEVICE_STATE_SCHEMA_VERSION,
            vaults: BTreeMap::new(),
        }
    }
}

impl WorkingFolderDeviceState {
    /// Returns a device scope when the vault and device are present.
    pub fn scope(&self, vault_id: &str, device_id: &str) -> Option<&WorkingFolderDeviceScope> {
        self.vaults
            .get(vault_id)
            .and_then(|devices| devices.get(device_id))
    }

    /// Returns or creates the device scope for a vault and device.
    pub fn scope_mut(&mut self, vault_id: &str, device_id: &str) -> &mut WorkingFolderDeviceScope {
        self.vaults
            .entry(vault_id.to_string())
            .or_default()
            .entry(device_id.to_string())
            .or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_and_timestamp_validation_preserve_wire_contracts() {
        assert_eq!(
            ProjectWorkingFolderId::new("workspace-1").unwrap().as_str(),
            "workspace-1"
        );
        assert!(ProjectWorkingFolderId::new("workspace\n1").is_err());
        let client_id = format!("working-folder:{}", "0".repeat(36));
        assert!(ProjectWorkingFolderId::new(client_id).is_ok());
        assert!(ProjectWorkingFolderId::new("w".repeat(MAX_IDENTIFIER_BYTES)).is_ok());
        assert!(ProjectWorkingFolderId::new("w".repeat(MAX_IDENTIFIER_BYTES + 1)).is_err());
        assert!(UtcTimestamp::new("2026-07-20T12:00:00-06:00").is_err());
        assert_eq!(
            UtcTimestamp::new("2026-07-20T18:00:00Z").unwrap().as_str(),
            "2026-07-20T18:00:00Z"
        );
    }

    /// The fixture is shared with the frontend reader, so both sides refuse the same text.
    #[test]
    fn timestamp_validation_matches_the_frontend_reader() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../apps/client/test-fixtures/utc-timestamps.json"
        ))
        .unwrap();
        let cases = |key: &str| fixture[key].as_array().unwrap().clone();

        for case in cases("accepted") {
            let text = case.as_str().unwrap();
            assert_eq!(UtcTimestamp::new(text).unwrap().as_str(), text);
        }
        for case in cases("refused") {
            let text = case.as_str().unwrap();
            assert!(UtcTimestamp::new(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn binding_requires_the_current_identity_fields() {
        let current = serde_json::json!({
            "canonicalPath": "/tmp/workspace",
            "filesystemIdentity": "filesystem-1",
            "repositoryKind": "git",
            "repositoryIdentity": "repository-1",
            "repositoryStorageIdentity": null,
            "lastVerifiedAt": "2026-07-20T12:00:00Z"
        });
        assert!(
            serde_json::from_value::<ProjectWorkingFolderBindingState>(current.clone()).is_ok()
        );

        for field in [
            "filesystemIdentity",
            "repositoryIdentity",
            "repositoryStorageIdentity",
        ] {
            let mut incomplete = current.clone();
            incomplete.as_object_mut().unwrap().remove(field);
            let binding = serde_json::from_value::<ProjectWorkingFolderBindingState>(incomplete);

            assert!(binding.is_err(), "missing {field} must be rejected");
        }
    }

    #[test]
    fn scope_mut_is_partitioned_by_vault_and_device() {
        let mut state = WorkingFolderDeviceState::default();
        state
            .scope_mut("vault-1", "device-1")
            .last_selected_by_project
            .insert(
                "project-1".to_string(),
                ProjectWorkingFolderId::new("workspace-1").unwrap(),
            );

        assert!(state.scope("vault-1", "device-2").is_none());
        assert_eq!(
            state
                .scope("vault-1", "device-1")
                .unwrap()
                .last_selected_by_project["project-1"]
                .as_str(),
            "workspace-1"
        );
    }
}
