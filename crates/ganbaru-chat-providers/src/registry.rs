use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ProviderCapability, ProviderFamilyId,
    ProviderFamilyMetadataRead, ProviderImplementationStatus, ProviderInstanceConfig,
    ProviderMaturity, ProviderVersionAdmission,
};
use crate::{
    ProviderDriver, ProviderDriverFactory, UnsupportedProviderDriver, claude::ClaudeProviderDriver,
    codex::CodexProviderDriver, cursor::CursorProviderDriver, opencode::OpenCodeProviderDriver,
};
use serde_json::json;

const DESKTOP_PLATFORMS: &[&str] = &["linux", "windows", "macos"];

const CODEX_CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::NativeResume,
    ProviderCapability::NativePlan,
    ProviderCapability::Steering,
    ProviderCapability::DynamicModelChange,
    ProviderCapability::Images,
    ProviderCapability::FileReferences,
    ProviderCapability::Skills,
    ProviderCapability::SlashCommands,
    ProviderCapability::Approvals,
    ProviderCapability::StructuredQuestions,
    ProviderCapability::ReasoningSummaries,
    ProviderCapability::StructuredPlans,
    ProviderCapability::ContextUsage,
    ProviderCapability::McpStatus,
    ProviderCapability::AccountStatus,
    ProviderCapability::RateLimitStatus,
    ProviderCapability::ProviderDiffs,
    ProviderCapability::TaskActivity,
];

const CLAUDE_CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::NativeResume,
    ProviderCapability::NativePlan,
    ProviderCapability::Steering,
    ProviderCapability::DynamicModelChange,
    ProviderCapability::Images,
    ProviderCapability::FileReferences,
    ProviderCapability::Skills,
    ProviderCapability::SlashCommands,
    ProviderCapability::Approvals,
    ProviderCapability::StructuredQuestions,
    ProviderCapability::ReasoningSummaries,
    ProviderCapability::StructuredPlans,
    ProviderCapability::ContextUsage,
    ProviderCapability::CostReporting,
    ProviderCapability::McpStatus,
    ProviderCapability::RateLimitStatus,
    ProviderCapability::TaskActivity,
];

const CURSOR_CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::NativeResume,
    ProviderCapability::NativePlan,
    ProviderCapability::DynamicModelChange,
    ProviderCapability::Images,
    ProviderCapability::FileReferences,
    ProviderCapability::Approvals,
    ProviderCapability::StructuredQuestions,
    ProviderCapability::ReasoningSummaries,
    ProviderCapability::StructuredPlans,
    ProviderCapability::ContextUsage,
    ProviderCapability::ProviderDiffs,
    ProviderCapability::SlashCommands,
];

const GROK_CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::NativeResume,
    ProviderCapability::NativePlan,
    ProviderCapability::DynamicModelChange,
    ProviderCapability::Images,
    ProviderCapability::FileReferences,
    ProviderCapability::Approvals,
    ProviderCapability::ReasoningSummaries,
    ProviderCapability::StructuredPlans,
    ProviderCapability::ProviderDiffs,
    ProviderCapability::SlashCommands,
];

const OPENCODE_CAPABILITIES: &[ProviderCapability] = &[
    ProviderCapability::NativeResume,
    ProviderCapability::NativeRollback,
    ProviderCapability::NativePlan,
    ProviderCapability::Steering,
    ProviderCapability::DynamicModelChange,
    ProviderCapability::Images,
    ProviderCapability::FileReferences,
    ProviderCapability::Approvals,
    ProviderCapability::StructuredQuestions,
    ProviderCapability::ReasoningSummaries,
    ProviderCapability::StructuredPlans,
    ProviderCapability::ContextUsage,
    ProviderCapability::CostReporting,
    ProviderCapability::McpStatus,
    ProviderCapability::ProviderDiffs,
    ProviderCapability::SlashCommands,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderMetadataDefinition {
    pub family_id: &'static str,
    pub display_name: &'static str,
    pub configuration_schema_version: u32,
    pub supported_platforms: &'static [&'static str],
    pub minimum_tested_cli_version: Option<&'static str>,
    pub default_executable_candidates: &'static [&'static str],
    pub potential_capabilities: &'static [ProviderCapability],
}

impl ProviderMetadataDefinition {
    pub fn to_read(self) -> ProviderFamilyMetadataRead {
        ProviderFamilyMetadataRead {
            family_id: ProviderFamilyId::new(self.family_id)
                .expect("static provider family ID must be valid"),
            display_name: self.display_name.to_string(),
            configuration_schema_version: self.configuration_schema_version,
            supported_platforms: self
                .supported_platforms
                .iter()
                .map(|platform| (*platform).to_string())
                .collect(),
            minimum_tested_cli_version: self.minimum_tested_cli_version.map(str::to_string),
            maximum_tested_cli_version: None,
            default_executable_candidates: self
                .default_executable_candidates
                .iter()
                .map(|executable| (*executable).to_string())
                .collect(),
            implementation_status: ProviderImplementationStatus::MetadataOnly,
            maturity: ProviderMaturity::Experimental,
            protocol_name: "unavailable".to_string(),
            potential_capabilities: self.potential_capabilities.to_vec(),
            unavailable_reason: Some(
                "This provider driver has not been implemented yet.".to_string(),
            ),
        }
    }
}

pub const PROVIDER_METADATA: [ProviderMetadataDefinition; 5] = [
    ProviderMetadataDefinition {
        family_id: "codex",
        display_name: "OpenAI",
        configuration_schema_version: 1,
        supported_platforms: DESKTOP_PLATFORMS,
        minimum_tested_cli_version: None,
        default_executable_candidates: &["codex"],
        potential_capabilities: CODEX_CAPABILITIES,
    },
    ProviderMetadataDefinition {
        family_id: "claude",
        display_name: "Anthropic",
        configuration_schema_version: 1,
        supported_platforms: DESKTOP_PLATFORMS,
        minimum_tested_cli_version: None,
        default_executable_candidates: &["claude"],
        potential_capabilities: CLAUDE_CAPABILITIES,
    },
    ProviderMetadataDefinition {
        family_id: "cursor",
        display_name: "Cursor",
        configuration_schema_version: 1,
        supported_platforms: DESKTOP_PLATFORMS,
        minimum_tested_cli_version: None,
        default_executable_candidates: &["cursor-agent", "cursor"],
        potential_capabilities: CURSOR_CAPABILITIES,
    },
    ProviderMetadataDefinition {
        family_id: "grok",
        display_name: "xAI",
        configuration_schema_version: 1,
        supported_platforms: DESKTOP_PLATFORMS,
        minimum_tested_cli_version: None,
        default_executable_candidates: &["grok"],
        potential_capabilities: GROK_CAPABILITIES,
    },
    ProviderMetadataDefinition {
        family_id: "opencode",
        display_name: "OpenCode",
        configuration_schema_version: 1,
        supported_platforms: DESKTOP_PLATFORMS,
        minimum_tested_cli_version: None,
        default_executable_candidates: &["opencode"],
        potential_capabilities: OPENCODE_CAPABILITIES,
    },
];

#[derive(Clone, Copy, Debug, Default)]
pub struct ProviderDriverRegistry;

impl ProviderDriverRegistry {
    pub fn list_metadata(self) -> Vec<ProviderFamilyMetadataRead> {
        PROVIDER_METADATA
            .iter()
            .copied()
            .map(|metadata| match metadata.family_id {
                "codex" => CodexProviderDriver::metadata_read(),
                "claude" => ClaudeProviderDriver::metadata_read(),
                "cursor" => CursorProviderDriver::metadata_read(),
                "grok" => CursorProviderDriver::grok_metadata_read(),
                "opencode" => OpenCodeProviderDriver::metadata_read(),
                _ => metadata.to_read(),
            })
            .collect()
    }

    pub fn metadata(self, family_id: &ProviderFamilyId) -> ProviderFamilyMetadataRead {
        PROVIDER_METADATA
            .iter()
            .find(|metadata| metadata.family_id == family_id.as_str())
            .copied()
            .map(|metadata| match metadata.family_id {
                "codex" => CodexProviderDriver::metadata_read(),
                "claude" => ClaudeProviderDriver::metadata_read(),
                "cursor" => CursorProviderDriver::metadata_read(),
                "grok" => CursorProviderDriver::grok_metadata_read(),
                "opencode" => OpenCodeProviderDriver::metadata_read(),
                _ => metadata.to_read(),
            })
            .unwrap_or_else(|| unsupported_metadata(family_id))
    }
}

impl ProviderDriverFactory for ProviderDriverRegistry {
    fn create_driver(
        &self,
        configuration: ProviderInstanceConfig,
    ) -> ChatResult<Box<dyn ProviderDriver>> {
        if configuration.family_id.as_str() == "codex" {
            return CodexProviderDriver::new(configuration)
                .map(|driver| Box::new(driver) as Box<dyn ProviderDriver>);
        }
        if configuration.family_id.as_str() == "claude" {
            return ClaudeProviderDriver::new(configuration)
                .map(|driver| Box::new(driver) as Box<dyn ProviderDriver>);
        }
        if configuration.family_id.as_str() == "cursor" {
            return CursorProviderDriver::new(configuration)
                .map(|driver| Box::new(driver) as Box<dyn ProviderDriver>);
        }
        if configuration.family_id.as_str() == "grok" {
            return CursorProviderDriver::new_grok(configuration)
                .map(|driver| Box::new(driver) as Box<dyn ProviderDriver>);
        }
        if configuration.family_id.as_str() == "opencode" {
            return OpenCodeProviderDriver::new(configuration)
                .map(|driver| Box::new(driver) as Box<dyn ProviderDriver>);
        }
        let metadata = self.metadata(&configuration.family_id);
        Ok(Box::new(UnsupportedProviderDriver::new(
            metadata,
            configuration,
        )))
    }
}

fn unsupported_metadata(family_id: &ProviderFamilyId) -> ProviderFamilyMetadataRead {
    ProviderFamilyMetadataRead {
        family_id: family_id.clone(),
        display_name: family_id.as_str().to_string(),
        configuration_schema_version: 0,
        supported_platforms: Vec::new(),
        minimum_tested_cli_version: None,
        maximum_tested_cli_version: None,
        default_executable_candidates: Vec::new(),
        implementation_status: ProviderImplementationStatus::Unsupported,
        maturity: ProviderMaturity::Experimental,
        protocol_name: "unavailable".to_string(),
        potential_capabilities: Vec::new(),
        unavailable_reason: Some(
            "This provider family is not supported by this build.".to_string(),
        ),
    }
}

/// The CLI versions one provider family supports. The minimum and the ceiling bound what
/// starts; the newest tested version is the newest the replay transcripts and focused tests
/// cover, and anything between it and the ceiling runs with a notice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderCompatibilityRange {
    pub minimum: &'static str,
    pub maximum_tested: &'static str,
    /// The first version of a protocol line Ganbaru has no runtime for, when the family has
    /// major lines.
    pub ceiling_exclusive: Option<&'static str>,
}

impl ProviderCompatibilityRange {
    /// The probe detail of a version newer than the tested one.
    pub fn untested_newer_detail(&self) -> String {
        format!("Newer than the tested version {}", self.maximum_tested)
    }
}

/// A provider's own version parser and ordering, which `admit_version` compares through.
pub trait VersionOrder: Ord + Sized {
    fn parse_version(value: &str) -> ChatResult<Self>;
}

/// Where `version` falls in `range`, read with the provider's parser `V`. An unreadable version
/// is `Unreadable`; an error means the range itself does not parse.
pub fn admit_version<V: VersionOrder>(
    range: &ProviderCompatibilityRange,
    version: &str,
) -> ChatResult<ProviderVersionAdmission> {
    let minimum = V::parse_version(range.minimum)?;
    let maximum_tested = V::parse_version(range.maximum_tested)?;
    let ceiling = range.ceiling_exclusive.map(V::parse_version).transpose()?;
    let Ok(version) = V::parse_version(version) else {
        return Ok(ProviderVersionAdmission::Unreadable);
    };
    Ok(if version < minimum {
        ProviderVersionAdmission::BelowMinimum
    } else if ceiling.is_some_and(|ceiling| version >= ceiling) {
        ProviderVersionAdmission::AtOrAboveCeiling
    } else if version > maximum_tested {
        ProviderVersionAdmission::UntestedNewer
    } else {
        ProviderVersionAdmission::Supported
    })
}

/// Whether `admission` lets a session start.
pub fn admits_sessions(admission: ProviderVersionAdmission) -> bool {
    !matches!(
        admission,
        ProviderVersionAdmission::BelowMinimum | ProviderVersionAdmission::AtOrAboveCeiling
    )
}

const VERSION_ADMISSION_DETAIL: &str = "versionAdmission";
const VERSION_DETAIL: &str = "version";

/// An `UnsupportedVersion` refusal that carries the admission and the refused version, so a
/// probe that fails on it can still report both.
pub fn version_refusal(
    admission: ProviderVersionAdmission,
    version: &str,
    message: String,
    recoverable: bool,
) -> ChatError {
    let mut error = ChatError::new(ChatErrorCode::UnsupportedVersion, message, recoverable);
    error.details = Some(Box::new(json!({
        VERSION_ADMISSION_DETAIL: admission,
        VERSION_DETAIL: version,
    })));
    error
}

/// The admission and version a `version_refusal` carries.
pub fn refused_version(error: &ChatError) -> Option<(ProviderVersionAdmission, String)> {
    if error.code != ChatErrorCode::UnsupportedVersion {
        return None;
    }
    let details = error.details.as_deref()?;
    let admission = serde_json::from_value(details.get(VERSION_ADMISSION_DETAIL)?.clone()).ok()?;
    let version = details.get(VERSION_DETAIL)?.as_str()?.to_string();
    Some((admission, version))
}

/// A `major.minor.patch` version, read from the first token that starts with a digit. A
/// prerelease or build suffix after `-` or `+` is ignored, so `0.148.0-alpha.2` orders as
/// `0.148.0`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CoreVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl VersionOrder for CoreVersion {
    fn parse_version(value: &str) -> ChatResult<Self> {
        let unreadable = || {
            ChatError::new(
                ChatErrorCode::UnsupportedVersion,
                format!("Provider version {value:?} is not a readable version"),
                false,
            )
        };
        let token = value
            .split_whitespace()
            .find(|token| token.starts_with(|character: char| character.is_ascii_digit()))
            .ok_or_else(unreadable)?;
        let core = token.split(['-', '+']).next().unwrap_or_default();
        let parts = core
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(unreadable)?;
        let [major, minor, patch] = parts[..] else {
            return Err(unreadable());
        };
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

#[cfg(test)]
mod tests;
