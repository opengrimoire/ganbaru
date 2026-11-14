use super::{
    CredentialReferenceId, ModelCatalogSource, ModelId, ProbeState, ProviderCapability,
    ProviderFamilyId, ProviderImplementationStatus, ProviderInstanceId, ProviderMaturity,
    UtcTimestamp, VersionedJson,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Eq, PartialEq)]
pub struct ProviderInternalMcpConfig {
    pub name: String,
    pub url: String,
    pub bearer_token: String,
    /// Enables the provider's fail-closed organizational authority boundary.
    ///
    /// The internal MCP endpoint also serves personal Chat resources, so its
    /// presence alone must never imply an organizational assignment.
    pub organizational_authority: bool,
    /// Provider tool names of the internal tools an organizational run calls without an
    /// approval prompt. Empty for a direct thread, and ignored by providers without a
    /// verified per-tool approval setting.
    pub pre_approved_tools: Vec<String>,
    /// The execution target an organizational run was authorized for, set exactly when
    /// `organizational_authority` is. A driver that enforces scopes fails closed without it.
    pub organizational_scope: Option<ProviderOrganizationalScope>,
}

/// The authorized execution target of one organizational run, as a provider driver enforces it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ProviderOrganizationalScope {
    /// No native workspace: the run works from an application-managed neutral directory.
    Conversation,
    /// One project working folder at the granted capability.
    Folder {
        capability: ProviderFolderCapability,
    },
    /// A writable private scratch directory.
    Scratch,
}

/// The folder capability of an organizational folder target. Unlike
/// `ChatFolderCapability`, it cannot be `None`, because a target folder always carries a grant.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFolderCapability {
    Read,
    Edit,
    Execute,
    Publish,
}

impl ProviderFolderCapability {
    /// Parses the stored capability of an execution target folder source; `none` and unknown
    /// values have no provider scope.
    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "edit" => Some(Self::Edit),
            "execute" => Some(Self::Execute),
            "publish" => Some(Self::Publish),
            _ => None,
        }
    }
}

impl std::fmt::Debug for ProviderInternalMcpConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderInternalMcpConfig")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("bearer_token", &"[REDACTED]")
            .field("organizational_authority", &self.organizational_authority)
            .field("pre_approved_tools", &self.pre_approved_tools)
            .field("organizational_scope", &self.organizational_scope)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstanceConfig {
    pub schema_version: u32,
    pub instance_id: ProviderInstanceId,
    pub family_id: ProviderFamilyId,
    pub label: String,
    pub enabled: bool,
    pub executable: String,
    pub provider_home: Option<String>,
    pub launch_arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub credential_references: BTreeMap<String, CredentialReferenceId>,
    pub visible_model_ids: Vec<ModelId>,
    pub favorite_model_ids: Vec<ModelId>,
    pub provider_config: VersionedJson,
    #[serde(skip)]
    pub internal_mcp: Option<ProviderInternalMcpConfig>,
    #[serde(flatten)]
    pub unknown_fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilitySupport {
    pub capability: ProviderCapability,
    pub supported: bool,
    pub explanation: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub entries: Vec<ProviderCapabilitySupport>,
}

impl ProviderCapabilities {
    pub fn supports(&self, capability: ProviderCapability) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.capability == capability && entry.supported)
    }
}

/// Enforcement boundaries a provider adapter can prove for organizational work.
///
/// These declarations are distinct from protocol feature flags. A false value
/// is a hard enforcement limit, not an indication that discovery is incomplete.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuthoritySupport {
    /// The adapter can run a teammate from an application-managed neutral
    /// directory without receiving project-folder or persistent-scratch grants.
    pub isolated_conversation: bool,
    pub internal_host_tools: bool,
    pub deny_shell: bool,
    pub read_only_root: bool,
    pub writable_root: bool,
    pub confined_commands: bool,
    pub network_boundary: bool,
    pub classified_publish: bool,
    /// Every shell command the provider can run reaches Ganbaru's permission handler and is
    /// never approved by the provider itself, so a person approves each command.
    #[serde(default)]
    pub person_approved_commands: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFamilyMetadataRead {
    pub family_id: ProviderFamilyId,
    pub display_name: String,
    pub configuration_schema_version: u32,
    pub supported_platforms: Vec<String>,
    pub minimum_tested_cli_version: Option<String>,
    /// The newest CLI version the replay transcripts and focused tests cover.
    #[serde(default)]
    pub maximum_tested_cli_version: Option<String>,
    pub default_executable_candidates: Vec<String>,
    pub implementation_status: ProviderImplementationStatus,
    pub maturity: ProviderMaturity,
    pub protocol_name: String,
    pub potential_capabilities: Vec<ProviderCapability>,
    pub unavailable_reason: Option<String>,
}

/// How a provider authenticates the sessions it starts, when the driver can tell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthSource {
    /// An API key the provider configuration supplies through its environment.
    ApiKey,
    /// The account login the CLI keeps in its own home.
    AccountLogin,
}

/// Where a probed CLI version falls against the provider family's compatibility range.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderVersionAdmission {
    /// At or above the minimum and at or below the newest tested version.
    Supported,
    /// Newer than the newest tested version and below any ceiling; sessions still start.
    UntestedNewer,
    /// Below the minimum; sessions are refused.
    BelowMinimum,
    /// At or above the ceiling of a protocol line Ganbaru has no runtime for; sessions are
    /// refused.
    AtOrAboveCeiling,
    /// The version output could not be read.
    Unreadable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbeResult {
    pub instance_id: ProviderInstanceId,
    pub state: ProbeState,
    pub version: Option<String>,
    pub negotiated_protocol_version: Option<String>,
    pub account_label: Option<String>,
    /// How sessions authenticate; `None` when the driver cannot tell.
    #[serde(default)]
    pub auth_source: Option<ProviderAuthSource>,
    pub capabilities: ProviderCapabilities,
    pub authority_support: ProviderAuthoritySupport,
    pub checked_at: UtcTimestamp,
    pub detail: Option<String>,
    /// The probed version against the family's compatibility range; `None` for families
    /// without a range or when no version was read.
    #[serde(default)]
    pub version_admission: Option<ProviderVersionAdmission>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoiceOption {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ModelOptionDefinition {
    Boolean {
        key: String,
        label: String,
        description: Option<String>,
        default_value: Option<bool>,
    },
    Choice {
        key: String,
        label: String,
        description: Option<String>,
        options: Vec<ModelChoiceOption>,
        default_value: Option<String>,
    },
    MultipleChoice {
        key: String,
        label: String,
        description: Option<String>,
        options: Vec<ModelChoiceOption>,
        default_value: Vec<String>,
    },
    IntegerRange {
        key: String,
        label: String,
        description: Option<String>,
        minimum: i64,
        maximum: i64,
        step: i64,
        default_value: Option<i64>,
    },
    Text {
        key: String,
        label: String,
        description: Option<String>,
        default_value: Option<String>,
        allow_empty: bool,
    },
    Unknown {
        key: String,
        label: String,
        raw_kind: String,
        schema_version: u32,
        data: Value,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ModelOptionValue {
    Boolean(bool),
    Choice(String),
    MultipleChoice(Vec<String>),
    Integer(i64),
    Text(String),
    Unknown(VersionedJson),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptionSelection {
    pub key: String,
    pub value: ModelOptionValue,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub id: ModelId,
    pub display_name: String,
    pub description: Option<String>,
    pub context_limit: Option<u64>,
    pub availability: super::ModelAvailability,
    pub capabilities: Vec<ProviderCapability>,
    pub options: Vec<ModelOptionDefinition>,
    pub custom: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelCatalog {
    pub instance_id: ProviderInstanceId,
    pub models: Vec<ProviderModel>,
    pub source: ModelCatalogSource,
    pub discovered_at: UtcTimestamp,
    pub stale: bool,
}

impl ProviderModelCatalog {
    /// Removes models that the provider has explicitly deprecated.
    ///
    /// Deprecated entries are provider migration hints, not selectable Chat
    /// models. Filtering them at the catalog boundary keeps every consumer
    /// consistent while retaining unavailable and stale model semantics.
    pub fn without_deprecated_models(mut self) -> Self {
        self.models
            .retain(|model| model.availability != super::ModelAvailability::Deprecated);
        self
    }
}
