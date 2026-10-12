//! OpenCode driver capabilities, identifiers, prompts, and history helpers.

use super::http_client::{OpenCodePrompt, OpenCodePromptModel, OpenCodePromptPart};
use super::protocol::{OpenCodeCommand, protocol_error, validate_identifier};
use crate::models::*;
use reqwest::Url;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_PROMPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_ATTACHMENTS: usize = 32;
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub fn capability_kinds() -> Vec<ProviderCapability> {
    vec![
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
    ]
}

pub fn capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        entries: capability_kinds()
            .into_iter()
            .map(|capability| ProviderCapabilitySupport {
                capability,
                supported: true,
                explanation: None,
            })
            .collect(),
    }
}

pub fn provider_command(
    request: &SendTurnRequest,
    commands: &[OpenCodeCommand],
) -> Option<(String, String)> {
    if !request.attachments.is_empty()
        || !request.mentions.is_empty()
        || request.developer_instructions.is_some()
    {
        return None;
    }
    let prompt = request.prompt.trim();
    let command_text = prompt.strip_prefix('/')?;
    let (name, arguments) = command_text
        .split_once(char::is_whitespace)
        .map_or((command_text, ""), |(name, arguments)| {
            (name, arguments.trim())
        });
    commands
        .iter()
        .find(|command| command.name.eq_ignore_ascii_case(name))
        .map(|command| (command.name.clone(), arguments.to_string()))
}

pub fn canonical_workspace(workspace: &VerifiedWorkspaceContext) -> ChatResult<PathBuf> {
    let path = PathBuf::from(&workspace.canonical_path);
    if !path.is_absolute() || !path.is_dir() {
        return Err(ChatError::validation(
            "workspace.canonicalPath",
            "OpenCode workspace is unavailable",
        ));
    }
    std::fs::canonicalize(path).map_err(|_| {
        ChatError::validation(
            "workspace.canonicalPath",
            "OpenCode workspace is unavailable",
        )
    })
}

pub fn canonical_current_directory() -> ChatResult<PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                "OpenCode probe directory is unavailable",
                true,
            )
        })
}

pub fn same_directory(provider_directory: &str, workspace: &Path) -> bool {
    let provider_path = PathBuf::from(provider_directory);
    if provider_path == workspace {
        return true;
    }
    std::fs::canonicalize(provider_path)
        .ok()
        .is_some_and(|path| path == workspace)
}

pub fn session_id(value: &Value) -> ChatResult<String> {
    let id = value
        .as_object()
        .and_then(|object| object.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| protocol_error("session ID"))?;
    validate_identifier(id, "session ID")?;
    Ok(id.to_string())
}

pub fn session_directory(value: &Value) -> Option<&str> {
    value
        .as_object()
        .and_then(|object| object.get("directory"))
        .and_then(Value::as_str)
}

pub fn new_local_session_id(instance_id: &ProviderInstanceId) -> ChatResult<ProviderSessionId> {
    let created_at_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| protocol_error("local session ID"))?
        .as_nanos();
    ProviderSessionId::new(format!(
        "opencode-{}-{}-{created_at_nanos}-{}",
        instance_id.as_str(),
        std::process::id(),
        NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
    ))
    .map_err(|_| protocol_error("local session ID"))
}

pub fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .map_err(|_| protocol_error("timestamp"))
}

pub fn prompt(request: &SendTurnRequest) -> ChatResult<OpenCodePrompt> {
    prompt_parts(
        &request.prompt,
        &request.attachments,
        request.model_id.as_ref(),
        &request.model_options,
        request.developer_instructions.as_deref(),
        request.modes,
    )
}

pub fn steering_prompt(prompt: &str) -> ChatResult<OpenCodePrompt> {
    prompt_parts(
        prompt,
        &[],
        None,
        &[],
        None,
        TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        },
    )
}

fn prompt_parts(
    text: &str,
    attachments: &[PromptAttachmentReference],
    model_id: Option<&ModelId>,
    options: &[ModelOptionSelection],
    system: Option<&str>,
    modes: TurnModeSnapshot,
) -> ChatResult<OpenCodePrompt> {
    if text.len() > MAX_PROMPT_BYTES || text.contains('\0') {
        return Err(ChatError::validation(
            "prompt",
            "OpenCode prompt is invalid or too large",
        ));
    }
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(ChatError::validation(
            "attachments",
            "OpenCode prompt has too many attachments",
        ));
    }
    let mut parts = Vec::new();
    if !text.trim().is_empty() {
        parts.push(OpenCodePromptPart::Text {
            text: text.to_string(),
        });
    }
    for attachment in attachments {
        if attachment.byte_size > MAX_ATTACHMENT_BYTES {
            return Err(ChatError::validation(
                "attachments",
                "OpenCode attachment exceeds the supported size",
            ));
        }
        let path = attachment.local_path.as_deref().ok_or_else(|| {
            ChatError::validation(
                "attachments",
                "OpenCode attachment does not have a verified local path",
            )
        })?;
        let path = std::fs::canonicalize(path).map_err(|_| {
            ChatError::validation("attachments", "OpenCode attachment is unavailable")
        })?;
        if !path.is_file() {
            return Err(ChatError::validation(
                "attachments",
                "OpenCode attachment is not a file",
            ));
        }
        let url = Url::from_file_path(&path).map_err(|_| {
            ChatError::validation("attachments", "OpenCode attachment path is invalid")
        })?;
        parts.push(OpenCodePromptPart::File {
            url: url.to_string(),
            mime: attachment
                .mime_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            filename: attachment.display_name.clone(),
        });
    }
    if parts.is_empty() {
        return Err(ChatError::validation(
            "prompt",
            "OpenCode requires text or at least one attachment",
        ));
    }
    let model = model_id.map(parse_model).transpose()?;
    let mut agent = string_option(options, "agent")?;
    if agent.is_none() && modes.interaction_mode == InteractionMode::Plan {
        agent = Some("plan".to_string());
    }
    Ok(OpenCodePrompt {
        model,
        agent,
        variant: string_option(options, "variant")?,
        system: system.map(str::to_string),
        parts,
    })
}

fn parse_model(model: &ModelId) -> ChatResult<OpenCodePromptModel> {
    let (provider_id, model_id) = model.as_str().split_once('/').ok_or_else(|| {
        ChatError::validation("modelId", "OpenCode model must use provider/model format")
    })?;
    validate_identifier(provider_id, "model provider ID")?;
    validate_identifier(model_id, "model ID")?;
    Ok(OpenCodePromptModel {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

fn string_option(options: &[ModelOptionSelection], key: &str) -> ChatResult<Option<String>> {
    let Some(option) = options.iter().find(|option| option.key == key) else {
        return Ok(None);
    };
    let value = match &option.value {
        ModelOptionValue::Choice(value) | ModelOptionValue::Text(value) => value,
        _ => {
            return Err(ChatError::validation(
                "modelOptions",
                format!("OpenCode {key} option must be text"),
            ));
        }
    };
    validate_identifier(value, key)?;
    Ok(Some(value.clone()))
}

pub fn history_item(value: Value) -> ChatResult<ProviderHistoryItem> {
    let info = value
        .as_object()
        .and_then(|object| object.get("info"))
        .and_then(Value::as_object)
        .ok_or_else(|| protocol_error("history message"))?;
    let id = info.get("id").and_then(Value::as_str);
    if let Some(id) = id {
        validate_identifier(id, "message ID")?;
    }
    let role = info
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    Ok(ProviderHistoryItem {
        provider_item_id: id.and_then(|id| ProviderItemId::new(id.to_string()).ok()),
        provider_turn_id: (role == "assistant")
            .then(|| id.and_then(|id| ProviderTurnId::new(id.to_string()).ok()))
            .flatten(),
        kind: format!("opencode_{role}_message"),
        data: VersionedJson {
            schema_version: 1,
            value,
        },
    })
}

pub fn operation_receipt(operation_id: &str, detail: &str) -> DriverOperationReceipt {
    DriverOperationReceipt {
        accepted: true,
        operation_id: operation_id.to_string(),
        detail: Some(detail.to_string()),
    }
}

pub fn driver_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "OpenCode driver state is unavailable",
        false,
    )
}
