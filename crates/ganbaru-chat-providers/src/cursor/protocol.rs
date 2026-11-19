//! Validated ACP version 1 and Cursor extension protocol models.

mod config;

pub(super) use config::{confirmed_session_not_found, find_mode, resolve_configuration_updates};
use config::{
    derive_modes_from_config, ensure_select_value, model_config_option, model_option_definitions,
    validate_config_options, validate_modes,
};

use crate::models::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

pub const ACP_PROTOCOL_VERSION: u32 = 1;
pub const MINIMUM_CURSOR_VERSION: &str = "2026.04.08";
/// Cursor Agent builds with the ACP surface the Cursor driver needs, up to the build the Cursor
/// replay scenarios record. Date versions have no major line, so there is no ceiling.
pub const CURSOR_COMPATIBILITY: crate::registry::ProviderCompatibilityRange =
    crate::registry::ProviderCompatibilityRange {
        minimum: MINIMUM_CURSOR_VERSION,
        maximum_tested: "2026.11.11",
        ceiling_exclusive: None,
    };
/// Grok CLI versions whose ACP surface, `GROK_WEB_FETCH` switch, and permission modes the Grok
/// driver relies on, up to the version its focused checks cover, below the next major line.
pub const GROK_COMPATIBILITY: crate::registry::ProviderCompatibilityRange =
    crate::registry::ProviderCompatibilityRange {
        minimum: "1.0.77",
        maximum_tested: "1.0.77",
        ceiling_exclusive: Some("2.0.0"),
    };
pub const CURSOR_AUTH_METHOD: &str = "cursor_login";
pub const MAX_PROTOCOL_TEXT_BYTES: usize = 64 * 1024;
const MAX_MODELS: usize = 256;
/// Longest authentication method id an agent advertises or an instance configures.
const MAX_AUTH_METHOD_ID_BYTES: usize = 128;
/// Longest `agentInfo` name, title, or version Ganbaru keeps.
const MAX_AGENT_INFO_BYTES: usize = 256;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CursorProviderSettings {
    pub api_endpoint: Option<String>,
    /// The `authMethods` id a generic ACP agent authenticates with. Ganbaru never picks one
    /// itself, because a method can open a browser or write credentials.
    pub auth_method: Option<String>,
}

impl CursorProviderSettings {
    pub fn parse_for(
        configuration: &ProviderInstanceConfig,
        provider_name: &str,
    ) -> ChatResult<Self> {
        if configuration.provider_config.schema_version != 1 {
            return Err(ChatError::validation(
                "providerConfig.schemaVersion",
                format!("{provider_name} provider config schema is unsupported"),
            ));
        }
        let mut settings: Self =
            serde_json::from_value(configuration.provider_config.value.clone()).map_err(|_| {
                ChatError::validation(
                    "providerConfig",
                    format!("{provider_name} provider config is invalid"),
                )
            })?;
        settings.api_endpoint = settings
            .api_endpoint
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if provider_name == "Cursor" {
            if let Some(endpoint) = settings.api_endpoint.as_deref() {
                validate_endpoint(endpoint)?;
            }
        } else if settings.api_endpoint.is_some() {
            return Err(ChatError::validation(
                "providerConfig.apiEndpoint",
                format!("{provider_name} does not support a custom API endpoint"),
            ));
        }
        settings.auth_method = settings
            .auth_method
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(method) = settings.auth_method.as_deref() {
            if configuration.family_id.as_str()
                != super::driver::AcpProviderFlavor::Generic.family_id()
            {
                return Err(ChatError::validation(
                    "providerConfig.authMethod",
                    format!("{provider_name} chooses its own authentication method"),
                ));
            }
            if !is_valid_identifier(method, MAX_AUTH_METHOD_ID_BYTES) {
                return Err(ChatError::validation(
                    "providerConfig.authMethod",
                    format!("{provider_name} authentication method is invalid"),
                ));
            }
        }
        Ok(settings)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpInitializeResponse {
    pub protocol_version: u32,
    pub agent_capabilities: AcpAgentCapabilities,
    pub auth_methods: Vec<AcpAuthMethod>,
    pub agent_info: Option<AcpImplementation>,
    #[serde(rename = "_meta")]
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpAgentCapabilities {
    pub load_session: bool,
    pub prompt_capabilities: AcpPromptCapabilities,
    #[serde(deserialize_with = "tolerant_mcp_capabilities")]
    pub mcp_capabilities: AcpMcpCapabilities,
    pub session_capabilities: Value,
}

impl AcpAgentCapabilities {
    /// Whether the agent takes Ganbaru's internal HTTP MCP server, by advertisement or flavor.
    pub fn accepts_http_mcp(&self, flavor: super::driver::AcpProviderFlavor) -> bool {
        self.mcp_capabilities.http || flavor.accepts_http_mcp_by_flavor()
    }
}

/// MCP transports the agent advertised; both default to false when absent or malformed.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpMcpCapabilities {
    pub http: bool,
    /// Parsed for completeness; the internal endpoint serves HTTP only.
    pub sse: bool,
}

/// Reads `mcpCapabilities` without failing `initialize` when an agent sends an odd shape.
fn tolerant_mcp_capabilities<'de, D>(deserializer: D) -> Result<AcpMcpCapabilities, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let object = value.as_object();
    let flag = |name: &str| {
        object
            .and_then(|object| object.get(name))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    Ok(AcpMcpCapabilities {
        http: flag("http"),
        sse: flag("sse"),
    })
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpPromptCapabilities {
    pub image: bool,
    pub audio: bool,
    pub embedded_context: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpAuthMethod {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpImplementation {
    pub name: String,
    pub title: Option<String>,
    pub version: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpSessionSetup {
    pub session_id: Option<String>,
    pub config_options: Vec<AcpConfigOption>,
    pub modes: Option<AcpModeState>,
    pub models: Option<AcpModelState>,
    #[serde(rename = "_meta")]
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpModelState {
    pub current_model_id: String,
    pub available_models: Vec<AcpModelInfo>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpModelInfo {
    pub model_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpModeState {
    pub current_mode_id: String,
    pub available_modes: Vec<AcpMode>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct AcpMode {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AcpConfigOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub option_type: String,
    pub current_value: Value,
    pub options: Vec<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcpAvailableCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
}

pub fn parse_available_commands_update(
    update: &Map<String, Value>,
) -> ChatResult<Vec<AcpAvailableCommand>> {
    let commands = update
        .get("availableCommands")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error("available commands update"))?;
    if commands.len() > 512 {
        return Err(protocol_error("available commands update"));
    }
    let mut seen = BTreeSet::new();
    commands
        .iter()
        .map(|value| {
            let command = value
                .as_object()
                .ok_or_else(|| protocol_error("available command"))?;
            let name = command
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .map(|value| value.trim_start_matches('/'))
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 200
                        && !value.chars().any(char::is_control)
                        && !value.chars().any(char::is_whitespace)
                })
                .ok_or_else(|| protocol_error("available command name"))?;
            if !seen.insert(name.to_ascii_lowercase()) {
                return Err(protocol_error("duplicate available command"));
            }
            let description = command
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| value.len() <= 1_000 && !value.chars().any(char::is_control))
                .ok_or_else(|| protocol_error("available command description"))?;
            let argument_hint = command
                .get("input")
                .and_then(Value::as_object)
                .and_then(|input| input.get("hint"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| {
                    !value.is_empty() && value.len() <= 500 && !value.chars().any(char::is_control)
                })
                .map(str::to_string);
            Ok(AcpAvailableCommand {
                name: name.to_string(),
                description: description.to_string(),
                argument_hint,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CursorAvailableModelsResponse {
    pub models: Vec<CursorAvailableModel>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CursorAvailableModel {
    pub value: String,
    pub name: String,
    pub config_options: Vec<AcpConfigOption>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcpResumeCursor {
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedConfigUpdate {
    pub config_id: String,
    pub value: Value,
    pub previous_value: Value,
}

pub fn parse_initialize(value: Value) -> ChatResult<AcpInitializeResponse> {
    let mut response: AcpInitializeResponse =
        serde_json::from_value(value).map_err(|_| protocol_error("initialize response"))?;
    // `agentInfo` is informational, so an implausible one is dropped instead of failing.
    response.agent_info = response.agent_info.filter(|agent| {
        [agent.name.as_str(), agent.version.as_str()]
            .into_iter()
            .chain(agent.title.as_deref())
            .all(|text| text.len() <= MAX_AGENT_INFO_BYTES && !text.chars().any(char::is_control))
    });
    if response.protocol_version != ACP_PROTOCOL_VERSION {
        return Err(ChatError::new(
            ChatErrorCode::UnsupportedVersion,
            "The provider negotiated an unsupported ACP protocol version",
            false,
        ));
    }
    if response.auth_methods.len() > 32
        || response.auth_methods.iter().any(|method| {
            !is_valid_identifier(&method.id, MAX_AUTH_METHOD_ID_BYTES)
                || method.name.len() > 256
                || method.name.chars().any(char::is_control)
        })
    {
        return Err(protocol_error("authentication methods"));
    }
    Ok(response)
}

pub fn parse_session_setup(value: Value, fresh: bool) -> ChatResult<AcpSessionSetup> {
    let mut response: AcpSessionSetup =
        serde_json::from_value(value).map_err(|_| protocol_error("session setup response"))?;
    if fresh
        && response
            .session_id
            .as_deref()
            .is_none_or(|id| !is_valid_identifier(id, 512))
    {
        return Err(protocol_error("new session ID"));
    }
    validate_config_options(&response.config_options)?;
    if response.modes.is_none() {
        response.modes = derive_modes_from_config(&response.config_options);
    }
    if let Some(modes) = response.modes.as_ref() {
        validate_modes(modes)?;
    }
    if let Some(models) = response
        .models
        .as_ref()
        .filter(|models| !models.available_models.is_empty())
    {
        validate_acp_models(models)?;
    }
    Ok(response)
}

pub fn ensure_grok_model_state(setup: &mut AcpSessionSetup) {
    let models = setup.models.get_or_insert_with(AcpModelState::default);
    if models.available_models.is_empty() {
        models.available_models.push(AcpModelInfo {
            model_id: "grok-build".to_string(),
            name: "Grok Build".to_string(),
        });
        models.current_model_id = "grok-build".to_string();
    }
}

fn validate_acp_models(models: &AcpModelState) -> ChatResult<()> {
    if models.available_models.is_empty()
        || models.available_models.len() > MAX_MODELS
        || !is_valid_identifier(&models.current_model_id, 256)
    {
        return Err(protocol_error("ACP model state"));
    }
    let mut seen = BTreeSet::new();
    for model in &models.available_models {
        let id = model.model_id.trim();
        let name = model.name.trim();
        if !is_valid_identifier(id, 256)
            || name.is_empty()
            || name.len() > 256
            || name.chars().any(char::is_control)
            || !seen.insert(id)
        {
            return Err(protocol_error("ACP model"));
        }
    }
    if !models
        .available_models
        .iter()
        .any(|model| model.model_id == models.current_model_id)
    {
        return Err(protocol_error("current ACP model"));
    }
    Ok(())
}

pub fn parse_acp_models(setup: &AcpSessionSetup) -> ChatResult<Vec<ProviderModel>> {
    let models = setup
        .models
        .as_ref()
        .ok_or_else(|| protocol_error("ACP model catalog"))?;
    validate_acp_models(models)?;
    models
        .available_models
        .iter()
        .map(|model| {
            Ok(ProviderModel {
                id: ModelId::new(model.model_id.clone()).map_err(|_| protocol_error("model ID"))?,
                display_name: model.name.trim().to_string(),
                description: None,
                context_limit: None,
                availability: ModelAvailability::Available,
                capabilities: vec![
                    ProviderCapability::FileReferences,
                    ProviderCapability::DynamicModelChange,
                ],
                options: Vec::new(),
            })
        })
        .collect()
}

pub fn resolve_mode_configuration_update(
    config_options: &[AcpConfigOption],
    mode_id: &str,
) -> ChatResult<Option<ResolvedConfigUpdate>> {
    let Some(option) = config_options.iter().find(|option| {
        option.id.eq_ignore_ascii_case("mode") || option.category.as_deref() == Some("mode")
    }) else {
        return Ok(None);
    };
    ensure_select_value(option, mode_id)?;
    Ok(
        (option.current_value != Value::String(mode_id.to_string())).then(|| {
            ResolvedConfigUpdate {
                config_id: option.id.clone(),
                value: Value::String(mode_id.to_string()),
                previous_value: option.current_value.clone(),
            }
        }),
    )
}

pub fn parse_available_models(value: Value) -> ChatResult<Vec<ProviderModel>> {
    let response: CursorAvailableModelsResponse =
        serde_json::from_value(value).map_err(|_| protocol_error("Cursor model response"))?;
    if response.models.is_empty() || response.models.len() > MAX_MODELS {
        return Err(protocol_error("Cursor model catalog"));
    }
    let mut seen = BTreeSet::new();
    response
        .models
        .into_iter()
        .map(|model| {
            let id = model.value.trim();
            let name = model.name.trim();
            if !is_valid_identifier(id, 256)
                || name.is_empty()
                || name.len() > 256
                || !seen.insert(id.to_string())
            {
                return Err(protocol_error("Cursor model"));
            }
            validate_config_options(&model.config_options)?;
            Ok(ProviderModel {
                id: ModelId::new(id.to_string()).map_err(|_| protocol_error("model ID"))?,
                display_name: name.to_string(),
                description: None,
                context_limit: None,
                availability: ModelAvailability::Available,
                capabilities: vec![
                    ProviderCapability::FileReferences,
                    ProviderCapability::DynamicModelChange,
                ],
                options: model_option_definitions(&model.config_options),
            })
        })
        .collect()
}

pub fn parse_config_options_update(value: Value) -> ChatResult<Vec<AcpConfigOption>> {
    let options: Vec<AcpConfigOption> =
        serde_json::from_value(value).map_err(|_| protocol_error("config options update"))?;
    validate_config_options(&options)?;
    Ok(options)
}

pub fn parse_config_update_response(
    value: Value,
    expected_id: &str,
    expected_value: &Value,
) -> ChatResult<Vec<AcpConfigOption>> {
    let options = value
        .get("configOptions")
        .cloned()
        .ok_or_else(|| protocol_error("configuration response"))?;
    let options = parse_config_options_update(options)?;
    let selected = options
        .iter()
        .find(|option| option.id == expected_id)
        .ok_or_else(|| protocol_error("selected configuration response"))?;
    if &selected.current_value != expected_value {
        return Err(protocol_error("applied configuration value"));
    }
    Ok(options)
}

pub fn parse_resume_cursor(value: &VersionedJson) -> ChatResult<AcpResumeCursor> {
    if value.schema_version != 1 {
        return Err(ChatError::validation(
            "resumeCursor.schemaVersion",
            "Cursor resume cursor schema is unsupported",
        ));
    }
    let session_id = value
        .value
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| is_valid_identifier(id, 512))
        .ok_or_else(|| ChatError::validation("resumeCursor", "Cursor session ID is invalid"))?;
    Ok(AcpResumeCursor {
        session_id: session_id.to_string(),
    })
}

pub fn resume_cursor(session_id: &str) -> VersionedJson {
    VersionedJson {
        schema_version: 1,
        value: json!({ "sessionId": session_id }),
    }
}

/// Capabilities an agent of `flavor` can hold; vendor extensions only for flavors that serve
/// them.
pub fn acp_capability_kinds(flavor: super::driver::AcpProviderFlavor) -> Vec<ProviderCapability> {
    let extensions = flavor.offers_structured_extensions();
    [
        Some(ProviderCapability::NativeResume),
        Some(ProviderCapability::NativePlan),
        Some(ProviderCapability::DynamicModelChange),
        Some(ProviderCapability::Images),
        Some(ProviderCapability::FileReferences),
        Some(ProviderCapability::Approvals),
        extensions.then_some(ProviderCapability::StructuredQuestions),
        Some(ProviderCapability::ReasoningSummaries),
        extensions.then_some(ProviderCapability::StructuredPlans),
        extensions.then_some(ProviderCapability::ProviderDiffs),
        Some(ProviderCapability::SlashCommands),
        Some(ProviderCapability::ContextUsage),
        Some(ProviderCapability::CostReporting),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Capabilities an ACP agent showed by sending their data during a live session.
///
/// `initialize` cannot announce these, so they start false and turn true when the matching
/// update arrives; they reach `capability_overrides` of the next `SessionStarted`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObservedAcpCapabilities {
    /// An `available_commands_update` listed at least one command.
    pub slash_commands: bool,
    /// An `agent_thought_chunk` arrived.
    pub reasoning: bool,
    /// A `usage_update` arrived.
    pub usage: bool,
    /// A `usage_update` carried a session cost.
    pub cost: bool,
}

impl ObservedAcpCapabilities {
    /// Keeps every capability either set observed.
    pub fn merge(&mut self, other: Self) {
        self.slash_commands |= other.slash_commands;
        self.reasoning |= other.reasoning;
        self.usage |= other.usage;
        self.cost |= other.cost;
    }
}

/// Builds the capability set from what the agent negotiated and showed.
///
/// No entry is true without a source: `initialize`, the session setup, the flavor's known
/// extensions, or an observed update. Without a session setup, as in an `initialize`-only
/// probe, Plan mode and dynamic models stay potential until a session confirms them.
pub fn negotiated_capabilities_for(
    flavor: super::driver::AcpProviderFlavor,
    initialize: &AcpInitializeResponse,
    setup: Option<&AcpSessionSetup>,
    observed: ObservedAcpCapabilities,
) -> ProviderCapabilities {
    let provider_name = flavor.display_name();
    let models_are_dynamic = setup.is_none_or(|setup| match flavor {
        super::driver::AcpProviderFlavor::Cursor => {
            model_config_option(&setup.config_options).is_some()
        }
        super::driver::AcpProviderFlavor::Grok | super::driver::AcpProviderFlavor::Generic => setup
            .models
            .as_ref()
            .is_some_and(|models| !models.available_models.is_empty()),
    });
    let supports_plan = setup.is_none_or(|setup| {
        setup
            .modes
            .as_ref()
            .is_some_and(|modes| find_mode(modes, InteractionMode::Plan).is_some())
    });
    let extensions = flavor.offers_structured_extensions();
    let entries = acp_capability_kinds(flavor)
        .into_iter()
        .map(|capability| {
            let supported = match capability {
                ProviderCapability::NativeResume => initialize.agent_capabilities.load_session,
                ProviderCapability::NativePlan => supports_plan,
                ProviderCapability::Images => {
                    initialize.agent_capabilities.prompt_capabilities.image
                }
                ProviderCapability::DynamicModelChange => models_are_dynamic,
                // Every ACP agent may ask `session/request_permission`, and every agent must
                // accept `resource_link` prompt blocks.
                ProviderCapability::Approvals | ProviderCapability::FileReferences => true,
                ProviderCapability::StructuredQuestions
                | ProviderCapability::StructuredPlans
                | ProviderCapability::ProviderDiffs => extensions,
                ProviderCapability::ReasoningSummaries => {
                    flavor.streams_reasoning() || observed.reasoning
                }
                ProviderCapability::SlashCommands => observed.slash_commands,
                ProviderCapability::ContextUsage => observed.usage,
                ProviderCapability::CostReporting => observed.cost,
                _ => false,
            };
            ProviderCapabilitySupport {
                capability,
                supported,
                explanation: (!supported).then(|| match capability {
                    ProviderCapability::NativeResume => {
                        format!("{provider_name} did not advertise ACP session loading")
                    }
                    ProviderCapability::NativePlan => {
                        format!("{provider_name} did not advertise a Plan mode")
                    }
                    ProviderCapability::Images => {
                        format!("{provider_name} did not advertise ACP image prompts")
                    }
                    ProviderCapability::DynamicModelChange => {
                        format!("{provider_name} did not advertise dynamic models")
                    }
                    ProviderCapability::StructuredQuestions
                    | ProviderCapability::StructuredPlans
                    | ProviderCapability::ProviderDiffs => {
                        format!("{provider_name} has no ACP extension for this capability")
                    }
                    ProviderCapability::ReasoningSummaries => {
                        format!("{provider_name} has not streamed reasoning")
                    }
                    ProviderCapability::SlashCommands => {
                        format!("{provider_name} has not listed slash commands")
                    }
                    ProviderCapability::ContextUsage => {
                        format!("{provider_name} has not reported context usage")
                    }
                    ProviderCapability::CostReporting => {
                        format!("{provider_name} has not reported a session cost")
                    }
                    _ => format!("{provider_name} did not advertise this ACP capability"),
                }),
            }
        })
        .collect();
    ProviderCapabilities { entries }
}

fn validate_endpoint(value: &str) -> ChatResult<()> {
    let parsed = reqwest::Url::parse(value).ok();
    let valid = parsed.as_ref().is_some_and(|endpoint| {
        let loopback_http = endpoint.scheme() == "http"
            && endpoint.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|address| address.is_loopback())
            });
        (endpoint.scheme() == "https" || loopback_http)
            && endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.fragment().is_none()
    });
    if value.len() > 2_048 || value.chars().any(char::is_control) || !valid {
        return Err(ChatError::validation(
            "providerConfig.apiEndpoint",
            "Cursor API endpoint must use HTTPS or loopback HTTP",
        ));
    }
    Ok(())
}

pub fn is_valid_identifier(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

pub fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

pub fn safe_shape(value: &Value) -> VersionedJson {
    VersionedJson {
        schema_version: 1,
        value: value.as_object().map_or_else(
            || json!({ "type": value_type(value) }),
            |object| json!({ "keys": object.keys().take(32).collect::<Vec<_>>() }),
        ),
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub fn object(value: &Value) -> ChatResult<&Map<String, Value>> {
    value.as_object().ok_or_else(|| protocol_error("object"))
}

pub fn protocol_error(subject: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("ACP provider returned an invalid {subject}"),
        false,
    )
}
