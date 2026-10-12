//! Typed Codex app-server request builders and response decoders.

use super::organizational::{ORGANIZATIONAL_PERMISSION_PROFILE, organizational_safety};
use super::transport::CodexRpcFailure;
use crate::models::{
    ChatError, ChatResult, InteractionMode, ModelAvailability, ModelChoiceOption, ModelId,
    ModelOptionDefinition, ModelOptionSelection, ModelOptionValue, ProviderCapability,
    ProviderModel, SafetyMode, SendTurnRequest, TurnModeSnapshot,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_PROMPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_DEVELOPER_INSTRUCTIONS_BYTES: usize = 64 * 1024;
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_MODEL_OPTIONS: usize = 32;
const STANDARD_SERVICE_TIER: &str = "standard";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub user_agent: String,
    pub codex_home: PathBuf,
    // Deserialized to validate the provider's advertised platform wire shape.
    #[allow(dead_code)]
    pub platform_family: String,
    #[allow(dead_code)]
    pub platform_os: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadOpenResponse {
    pub thread: CodexThread,
    pub model: String,
    pub approval_policy: Value,
    pub approvals_reviewer: String,
    pub sandbox: Value,
    #[serde(default)]
    pub active_permission_profile: Option<CodexActivePermissionProfile>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexActivePermissionProfile {
    pub id: String,
    #[serde(default)]
    pub extends: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfigReadResponse {
    pub config: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThread {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStartResponse {
    pub turn: CodexTurn,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurn {
    pub id: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountReadResponse {
    pub account: Option<CodexAccount>,
    pub requires_openai_auth: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccount {
    #[serde(rename = "type")]
    pub account_type: String,
    pub email: Option<String>,
    // Deserialized to validate the provider's advertised account wire shape.
    #[allow(dead_code)]
    pub plan_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelListResponse {
    pub data: Vec<CodexModel>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    pub id: String,
    pub model: String,
    pub display_name: String,
    pub description: String,
    pub hidden: bool,
    // Deserialized to validate provider model metadata even when selection is explicit.
    #[allow(dead_code)]
    pub is_default: bool,
    pub default_reasoning_effort: String,
    pub supported_reasoning_efforts: Vec<CodexReasoningEffort>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub service_tiers: Vec<CodexServiceTier>,
    pub default_service_tier: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub supports_personality: bool,
    pub upgrade: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexReasoningEffort {
    pub reasoning_effort: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CodexServiceTier {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodexSafetySettings {
    pub approval_policy: &'static str,
    pub approvals_reviewer: &'static str,
    pub sandbox: &'static str,
    pub turn_sandbox_type: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodexCustomSafetySettings {
    pub approval_policy: Value,
    pub approvals_reviewer: String,
    pub sandbox_policy: Option<Value>,
    pub permissions: Option<String>,
}

/// Resolves the current Codex configuration into turn-level permission overrides.
///
/// # Errors
///
/// Returns an error when the app-server reports an invalid permission configuration.
pub fn custom_safety_settings(
    response: ConfigReadResponse,
) -> ChatResult<CodexCustomSafetySettings> {
    let config = response
        .config
        .as_object()
        .ok_or_else(|| codex_protocol_error("Codex config response"))?;
    let approval_policy = match config.get("approval_policy") {
        None | Some(Value::Null) => Value::String("on-request".to_string()),
        Some(value) if is_valid_approval_policy(value) => value.clone(),
        Some(_) => return Err(codex_protocol_error("Codex approval policy")),
    };
    let approvals_reviewer = match config.get("approvals_reviewer") {
        None | Some(Value::Null) => "user".to_string(),
        Some(Value::String(value))
            if matches!(value.as_str(), "user" | "auto_review" | "guardian_subagent") =>
        {
            value.clone()
        }
        Some(_) => return Err(codex_protocol_error("Codex approvals reviewer")),
    };
    if let Some(Value::String(permissions)) = config.get("default_permissions") {
        if permissions.trim().is_empty()
            || permissions.len() > 256
            || permissions.chars().any(char::is_control)
        {
            return Err(codex_protocol_error("Codex permission profile"));
        }
        return Ok(CodexCustomSafetySettings {
            approval_policy,
            approvals_reviewer,
            sandbox_policy: None,
            permissions: Some(permissions.clone()),
        });
    }
    if config
        .get("default_permissions")
        .is_some_and(|value| !value.is_null())
    {
        return Err(codex_protocol_error("Codex permission profile"));
    }
    let sandbox_mode = match config.get("sandbox_mode") {
        None | Some(Value::Null) => "workspace-write",
        Some(Value::String(value)) => value.as_str(),
        Some(_) => return Err(codex_protocol_error("Codex sandbox mode")),
    };
    let sandbox_policy = match sandbox_mode {
        "danger-full-access" => json!({ "type": "dangerFullAccess" }),
        "read-only" => json!({ "type": "readOnly", "networkAccess": false }),
        "workspace-write" => workspace_write_policy(config.get("sandbox_workspace_write"))?,
        _ => return Err(codex_protocol_error("Codex sandbox mode")),
    };
    Ok(CodexCustomSafetySettings {
        approval_policy,
        approvals_reviewer,
        sandbox_policy: Some(sandbox_policy),
        permissions: None,
    })
}

fn is_valid_approval_policy(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| matches!(value, "untrusted" | "on-request" | "never"))
        || value
            .get("granular")
            .and_then(Value::as_object)
            .is_some_and(|granular| {
                !granular.is_empty() && granular.values().all(Value::is_boolean)
            })
}

fn workspace_write_policy(value: Option<&Value>) -> ChatResult<Value> {
    let settings = match value {
        None | Some(Value::Null) => None,
        Some(Value::Object(settings)) => Some(settings),
        Some(_) => return Err(codex_protocol_error("Codex workspace sandbox")),
    };
    let writable_roots = match settings.and_then(|settings| settings.get("writable_roots")) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                let path = value
                    .as_str()
                    .filter(|path| Path::new(path).is_absolute())
                    .ok_or_else(|| codex_protocol_error("Codex writable roots"))?;
                Ok(Value::String(path.to_string()))
            })
            .collect::<ChatResult<Vec<_>>>()?,
        Some(_) => return Err(codex_protocol_error("Codex writable roots")),
    };
    let boolean = |key: &str, default: bool| -> ChatResult<bool> {
        match settings.and_then(|settings| settings.get(key)) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Bool(value)) => Ok(*value),
            Some(_) => Err(codex_protocol_error("Codex workspace sandbox")),
        }
    };
    Ok(json!({
        "type": "workspaceWrite",
        "writableRoots": writable_roots,
        "networkAccess": boolean("network_access", false)?,
        "excludeTmpdirEnvVar": boolean("exclude_tmpdir_env_var", false)?,
        "excludeSlashTmp": boolean("exclude_slash_tmp", false)?,
    }))
}

pub fn safety_settings(mode: SafetyMode) -> Option<CodexSafetySettings> {
    match mode {
        SafetyMode::AskForApproval => Some(CodexSafetySettings {
            approval_policy: "on-request",
            approvals_reviewer: "user",
            sandbox: "workspace-write",
            turn_sandbox_type: "workspaceWrite",
        }),
        SafetyMode::ApproveForMe => Some(CodexSafetySettings {
            approval_policy: "on-request",
            approvals_reviewer: "auto_review",
            sandbox: "workspace-write",
            turn_sandbox_type: "workspaceWrite",
        }),
        SafetyMode::FullAccess => Some(CodexSafetySettings {
            approval_policy: "never",
            approvals_reviewer: "user",
            sandbox: "danger-full-access",
            turn_sandbox_type: "dangerFullAccess",
        }),
        SafetyMode::Custom => None,
    }
}

pub fn initialize_params() -> Value {
    json!({
        "clientInfo": {
            "name": "ganbaru_ai_desktop",
            "title": "Ganbaru AI",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "capabilities": {
            "experimentalApi": true,
        },
    })
}

pub fn thread_open_params(
    provider_thread_id: Option<&str>,
    workspace: &Path,
    modes: TurnModeSnapshot,
    model_id: Option<&ModelId>,
    developer_instructions: Option<&str>,
    organizational: bool,
) -> ChatResult<Value> {
    validate_optional_text(
        developer_instructions,
        MAX_DEVELOPER_INSTRUCTIONS_BYTES,
        "developerInstructions",
    )?;
    let mut params = Map::from_iter([(
        "cwd".to_string(),
        Value::String(workspace.to_string_lossy().into_owned()),
    )]);
    if organizational {
        let safety = organizational_safety(modes.safety_mode)?;
        params.insert(
            "approvalPolicy".to_string(),
            Value::String(safety.approval_policy.to_string()),
        );
        params.insert(
            "approvalsReviewer".to_string(),
            Value::String(safety.approvals_reviewer.to_string()),
        );
        params.insert(
            "permissions".to_string(),
            Value::String(ORGANIZATIONAL_PERMISSION_PROFILE.to_string()),
        );
        params.insert(
            "runtimeWorkspaceRoots".to_string(),
            Value::Array(vec![Value::String(
                workspace.to_string_lossy().into_owned(),
            )]),
        );
    } else if let Some(safety) = safety_settings(modes.safety_mode) {
        params.insert(
            "approvalPolicy".to_string(),
            Value::String(safety.approval_policy.to_string()),
        );
        params.insert(
            "approvalsReviewer".to_string(),
            Value::String(safety.approvals_reviewer.to_string()),
        );
        params.insert(
            "sandbox".to_string(),
            Value::String(safety.sandbox.to_string()),
        );
    }
    if let Some(provider_thread_id) = provider_thread_id {
        params.insert(
            "threadId".to_string(),
            Value::String(provider_thread_id.to_string()),
        );
    }
    if let Some(model_id) = model_id {
        validate_model_id(model_id.as_str())?;
        params.insert(
            "model".to_string(),
            Value::String(model_id.as_str().to_string()),
        );
    }
    if let Some(instructions) = developer_instructions {
        params.insert(
            "developerInstructions".to_string(),
            Value::String(instructions.to_string()),
        );
    }
    Ok(Value::Object(params))
}

pub fn turn_start_params(
    provider_thread_id: &str,
    workspace: &Path,
    fallback_model: &str,
    request: &SendTurnRequest,
    custom_safety: Option<&CodexCustomSafetySettings>,
    organizational: bool,
) -> ChatResult<Value> {
    if request.prompt.len() > MAX_PROMPT_BYTES || request.prompt.contains('\0') {
        return Err(ChatError::validation(
            "prompt",
            "Codex prompt exceeds the supported limit",
        ));
    }
    validate_optional_text(
        request.developer_instructions.as_deref(),
        MAX_DEVELOPER_INSTRUCTIONS_BYTES,
        "developerInstructions",
    )?;
    let mut input = Vec::new();
    if !request.prompt.is_empty() {
        input.push(json!({ "type": "text", "text": request.prompt }));
    }
    for attachment in &request.attachments {
        match attachment.kind.as_str() {
            "image" => {
                let path = attachment.local_path.as_deref().ok_or_else(|| {
                    ChatError::validation("attachments", "Codex image attachment is unavailable")
                })?;
                let path = Path::new(path);
                let metadata = fs::metadata(path).map_err(|_| {
                    ChatError::validation("attachments", "Codex image attachment is unavailable")
                })?;
                if !path.is_absolute() || !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
                    return Err(ChatError::validation(
                        "attachments",
                        "Codex image attachment is invalid or oversized",
                    ));
                }
                attachment
                    .mime_type
                    .as_deref()
                    .filter(|value| {
                        matches!(
                            *value,
                            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
                        )
                    })
                    .ok_or_else(|| {
                        ChatError::validation("attachments", "Codex image type is unsupported")
                    })?;
                input.push(json!({
                    "type": "localImage",
                    "path": path,
                }));
            }
            "text_snippet" => {
                let text = attachment.text_content.as_deref().ok_or_else(|| {
                    ChatError::validation("attachments", "Codex text context is unavailable")
                })?;
                if text.len() > 128 * 1024 || text.contains('\0') {
                    return Err(ChatError::validation(
                        "attachments",
                        "Codex text context exceeds the supported limit",
                    ));
                }
                input.push(json!({ "type": "text", "text": text }));
            }
            _ => {
                return Err(ChatError::unsupported(
                    "Codex prompt attachment kind is unsupported",
                ));
            }
        }
    }
    if input.is_empty() {
        return Err(ChatError::validation(
            "prompt",
            "Codex turn requires text or an image",
        ));
    }
    let (effort, service_tier) = parse_model_options(&request.model_options)?;
    let mut params = Map::from_iter([
        (
            "threadId".to_string(),
            Value::String(provider_thread_id.to_string()),
        ),
        ("input".to_string(), Value::Array(input)),
        (
            "clientUserMessageId".to_string(),
            Value::String(request.turn_id.as_str().to_string()),
        ),
    ]);
    if organizational {
        let safety = organizational_safety(request.modes.safety_mode)?;
        params.insert(
            "approvalPolicy".to_string(),
            Value::String(safety.approval_policy.to_string()),
        );
        params.insert(
            "approvalsReviewer".to_string(),
            Value::String(safety.approvals_reviewer.to_string()),
        );
        params.insert(
            "permissions".to_string(),
            Value::String(ORGANIZATIONAL_PERMISSION_PROFILE.to_string()),
        );
        params.insert(
            "runtimeWorkspaceRoots".to_string(),
            Value::Array(vec![Value::String(
                workspace.to_string_lossy().into_owned(),
            )]),
        );
    } else if let Some(safety) = safety_settings(request.modes.safety_mode) {
        params.insert(
            "approvalPolicy".to_string(),
            Value::String(safety.approval_policy.to_string()),
        );
        params.insert(
            "approvalsReviewer".to_string(),
            Value::String(safety.approvals_reviewer.to_string()),
        );
        params.insert(
            "sandboxPolicy".to_string(),
            match safety.turn_sandbox_type {
                "workspaceWrite" => json!({
                    "type": "workspaceWrite",
                    "writableRoots": [],
                    "networkAccess": false,
                    "excludeTmpdirEnvVar": false,
                    "excludeSlashTmp": false,
                }),
                "dangerFullAccess" => json!({ "type": "dangerFullAccess" }),
                _ => return Err(codex_protocol_error("Codex sandbox mode")),
            },
        );
    } else {
        let custom = custom_safety
            .ok_or_else(|| codex_protocol_error("Codex custom permission configuration"))?;
        params.insert("approvalPolicy".to_string(), custom.approval_policy.clone());
        params.insert(
            "approvalsReviewer".to_string(),
            Value::String(custom.approvals_reviewer.clone()),
        );
        if let Some(sandbox_policy) = custom.sandbox_policy.as_ref() {
            params.insert("sandboxPolicy".to_string(), sandbox_policy.clone());
        }
        if let Some(permissions) = custom.permissions.as_ref() {
            params.insert(
                "permissions".to_string(),
                Value::String(permissions.clone()),
            );
        }
    }
    if let Some(model_id) = request.model_id.as_ref() {
        validate_model_id(model_id.as_str())?;
        params.insert(
            "model".to_string(),
            Value::String(model_id.as_str().to_string()),
        );
    }
    if let Some(effort) = effort.as_deref() {
        params.insert("effort".to_string(), Value::String(effort.to_string()));
    }
    if let Some(service_tier) = service_tier.filter(|tier| tier != STANDARD_SERVICE_TIER) {
        params.insert("serviceTier".to_string(), Value::String(service_tier));
    }
    let collaboration_model = request
        .model_id
        .as_ref()
        .map(|model| model.as_str())
        .unwrap_or(fallback_model);
    validate_model_id(collaboration_model)?;
    let mut collaboration_settings = Map::from_iter([(
        "model".to_string(),
        Value::String(collaboration_model.to_string()),
    )]);
    if let Some(effort) = effort {
        collaboration_settings.insert("reasoning_effort".to_string(), Value::String(effort));
    }
    if let Some(instructions) = request.developer_instructions.as_ref() {
        collaboration_settings.insert(
            "developer_instructions".to_string(),
            Value::String(instructions.clone()),
        );
    }
    params.insert(
        "collaborationMode".to_string(),
        json!({
            "mode": match request.modes.interaction_mode {
                InteractionMode::Build => "default",
                InteractionMode::Plan => "plan",
            },
            "settings": collaboration_settings,
        }),
    );
    Ok(Value::Object(params))
}

pub fn decode_response<T>(value: Value, label: &str) -> Result<T, CodexRpcFailure>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value)
        .map_err(|_| CodexRpcFailure::Malformed(format!("{label} has an invalid shape")))
}

pub fn provider_model(model: CodexModel) -> ChatResult<ProviderModel> {
    validate_model_id(&model.id)?;
    let reasoning_options = model
        .supported_reasoning_efforts
        .into_iter()
        .map(|effort| ModelChoiceOption {
            label: effort.reasoning_effort.clone(),
            value: effort.reasoning_effort,
            description: nonempty(effort.description),
        })
        .collect::<Vec<_>>();
    let mut service_tier_options = model
        .service_tiers
        .into_iter()
        .map(|tier| ModelChoiceOption {
            value: tier.id,
            label: tier.name,
            description: nonempty(tier.description),
        })
        .collect::<Vec<_>>();
    if !service_tier_options.is_empty()
        && !service_tier_options
            .iter()
            .any(|tier| tier.value == STANDARD_SERVICE_TIER)
    {
        service_tier_options.insert(
            0,
            ModelChoiceOption {
                value: STANDARD_SERVICE_TIER.to_string(),
                label: "Standard".to_string(),
                description: Some("Default speed and usage".to_string()),
            },
        );
    }
    let mut options = Vec::new();
    if !reasoning_options.is_empty() {
        options.push(ModelOptionDefinition::Choice {
            key: "reasoning_effort".to_string(),
            label: "Reasoning effort".to_string(),
            description: None,
            options: reasoning_options,
            default_value: Some(model.default_reasoning_effort),
        });
    }
    if !service_tier_options.is_empty() {
        options.push(ModelOptionDefinition::Choice {
            key: "service_tier".to_string(),
            label: "Service tier".to_string(),
            description: None,
            options: service_tier_options,
            default_value: Some(
                model
                    .default_service_tier
                    .unwrap_or_else(|| STANDARD_SERVICE_TIER.to_string()),
            ),
        });
    }
    let mut capabilities = vec![
        ProviderCapability::NativeResume,
        ProviderCapability::NativePlan,
        ProviderCapability::DynamicModelChange,
        ProviderCapability::Approvals,
        ProviderCapability::StructuredQuestions,
        ProviderCapability::ReasoningSummaries,
        ProviderCapability::StructuredPlans,
        ProviderCapability::ContextUsage,
    ];
    if model.input_modalities.iter().any(|value| value == "image") {
        capabilities.push(ProviderCapability::Images);
    }
    Ok(ProviderModel {
        id: ModelId::new(model.id).map_err(identifier_error)?,
        display_name: if model.display_name.trim().is_empty() {
            model.model
        } else {
            model.display_name
        },
        description: nonempty(model.description),
        context_limit: None,
        availability: if model.hidden {
            ModelAvailability::Unavailable
        } else if model.upgrade.is_some() {
            ModelAvailability::Deprecated
        } else {
            ModelAvailability::Available
        },
        capabilities,
        options,
        custom: false,
    })
}

pub fn custom_provider_model(
    model_id: &str,
    display_name: Option<&str>,
) -> ChatResult<ProviderModel> {
    validate_model_id(model_id)?;
    Ok(ProviderModel {
        id: ModelId::new(model_id.to_string()).map_err(identifier_error)?,
        display_name: display_name.unwrap_or(model_id).to_string(),
        description: Some("Custom Codex model ID".to_string()),
        context_limit: None,
        availability: ModelAvailability::Unknown,
        capabilities: Vec::new(),
        options: Vec::new(),
        custom: true,
    })
}

fn parse_model_options(
    options: &[ModelOptionSelection],
) -> ChatResult<(Option<String>, Option<String>)> {
    if options.len() > MAX_MODEL_OPTIONS {
        return Err(ChatError::validation(
            "modelOptions",
            "Codex model options exceed the supported limit",
        ));
    }
    let mut effort = None;
    let mut service_tier = None;
    for option in options {
        let ModelOptionValue::Choice(value) = &option.value else {
            return Err(ChatError::validation(
                "modelOptions",
                "Codex model options must use a single choice",
            ));
        };
        validate_model_id(value)?;
        match option.key.as_str() {
            "reasoning_effort" if effort.is_none() => effort = Some(value.clone()),
            "service_tier" if service_tier.is_none() => service_tier = Some(value.clone()),
            _ => {
                return Err(ChatError::validation(
                    "modelOptions",
                    "Codex model option is unsupported or duplicated",
                ));
            }
        }
    }
    Ok((effort, service_tier))
}

fn validate_model_id(value: &str) -> ChatResult<()> {
    if value.trim().is_empty()
        || value.len() > MAX_MODEL_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "modelId",
            "Codex model identifier is invalid",
        ));
    }
    Ok(())
}

fn validate_optional_text(value: Option<&str>, max_bytes: usize, field: &str) -> ChatResult<()> {
    if value.is_some_and(|value| value.len() > max_bytes || value.contains('\0')) {
        return Err(ChatError::validation(
            field,
            "Codex text configuration exceeds the supported limit",
        ));
    }
    Ok(())
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn identifier_error(_error: String) -> ChatError {
    ChatError::new(
        crate::models::ChatErrorCode::Protocol,
        "Codex returned an invalid identifier",
        false,
    )
}

fn codex_protocol_error(detail: &str) -> ChatError {
    ChatError::new(crate::models::ChatErrorCode::Protocol, detail, false)
}
