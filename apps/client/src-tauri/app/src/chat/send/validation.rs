//! Bounded request, provider, model, and workspace validation.

use super::support::{json_error, persistence_error};
use crate::chat::send_commands::SendChatTurnCommand;
use crate::chat::workspace::AuthorizedWorkingFolder;
use ganbaru_chat_contracts::models::{
    ApprovalDecision, ApprovalDecisionKind, ChatError, ChatErrorCode, ChatRequestId, ChatResult,
    ChatThreadId, InteractionMode, ModelAvailability, ModelId, ModelOptionDefinition,
    ModelOptionSelection, ModelOptionValue, ProbeState, ProviderCapabilities, ProviderCapability,
    ProviderRequestId, SafetyMode, TurnModeSnapshot, UserInputAnswer, WorkspaceMentionReference,
};
use sqlx::SqlitePool;
use std::collections::BTreeSet;

pub(super) async fn validate_pending_request(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    request_id: &ChatRequestId,
    provider_request_id: &ProviderRequestId,
    request_kind: &str,
) -> ChatResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_pending_requests
            WHERE id = ? AND thread_id = ? AND provider_request_id = ?
              AND request_kind = ? AND resolution_state = 'open'
         )",
    )
    .bind(request_id.as_str())
    .bind(thread_id.as_str())
    .bind(provider_request_id.as_str())
    .bind(request_kind)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if valid {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Chat request is stale or belongs to another thread",
            false,
        ))
    }
}

pub(super) async fn validate_approval_decision(
    pool: &SqlitePool,
    request_id: &ChatRequestId,
    decision: &ApprovalDecision,
) -> ChatResult<()> {
    let data: String = sqlx::query_scalar(
        "SELECT allowed_decisions_data FROM chat_pending_requests
         WHERE id = ? AND resolution_state = 'open'",
    )
    .bind(request_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::Conflict, "Chat approval is stale", false))?;
    let choices: serde_json::Value = serde_json::from_str(&data).map_err(json_error)?;
    let decision_kind = match decision.kind {
        ApprovalDecisionKind::AllowOnce => "allow_once",
        ApprovalDecisionKind::AllowSession => "allow_session",
        ApprovalDecisionKind::Deny => "deny",
        ApprovalDecisionKind::Cancel => "cancel",
    };
    let Some(provider_option_id) = decision.provider_option_id.as_deref() else {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Approval decision is missing its provider option",
            false,
        ));
    };
    let offered = choices.as_array().is_some_and(|choices| {
        choices.iter().any(|choice| {
            choice.get("id").and_then(serde_json::Value::as_str) == Some(provider_option_id)
                && choice
                    .get("decisionKind")
                    .and_then(serde_json::Value::as_str)
                    == Some(decision_kind)
        })
    });
    if offered {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "Approval decision was not offered by the provider",
            false,
        ))
    }
}

pub(super) fn validate_provider_selection(
    provider: &crate::chat::settings_commands::ProviderInstanceRead,
    request: &SendChatTurnCommand,
) -> ChatResult<()> {
    if !provider.configuration.enabled
        || provider
            .last_probe
            .as_ref()
            .is_none_or(|probe| probe.state != ProbeState::Healthy)
    {
        return Err(ChatError::new(
            ChatErrorCode::DriverUnavailable,
            "Selected Chat provider is not ready",
            true,
        ));
    }
    let family = provider.configuration.family_id.as_str();
    let permission_mode_supported = match request.modes.safety_mode {
        SafetyMode::AskForApproval | SafetyMode::FullAccess => true,
        SafetyMode::ApproveForMe => matches!(family, "codex" | "claude"),
        SafetyMode::Custom => matches!(family, "codex" | "claude" | "cursor" | "grok" | "opencode"),
    };
    if !permission_mode_supported {
        return Err(ChatError::validation(
            "modes.safetyMode",
            "Selected provider cannot implement this permission mode",
        ));
    }
    if let Some(model_id) = request.model_id.as_ref() {
        let model = provider
            .model_catalog
            .as_ref()
            .and_then(|catalog| catalog.models.iter().find(|model| &model.id == model_id))
            .ok_or_else(|| ChatError::validation("modelId", "Selected model is unavailable"))?;
        if !matches!(
            model.availability,
            ModelAvailability::Available | ModelAvailability::Stale
        ) {
            return Err(ChatError::validation(
                "modelId",
                "Selected model is unavailable",
            ));
        }
        validate_model_options(&model.options, &request.model_options)?;
    } else if provider
        .model_catalog
        .as_ref()
        .is_some_and(|catalog| !catalog.models.is_empty())
    {
        return Err(ChatError::validation(
            "modelId",
            "This provider exposes explicit model choices",
        ));
    } else if !request.model_options.is_empty() {
        return Err(ChatError::validation(
            "modelOptions",
            "Provider-managed model selection cannot include model traits",
        ));
    }
    Ok(())
}

pub(super) fn validate_model_options(
    definitions: &[ModelOptionDefinition],
    selections: &[ModelOptionSelection],
) -> ChatResult<()> {
    if selections.len() > 100 {
        return Err(ChatError::validation(
            "modelOptions",
            "Too many model traits",
        ));
    }
    let mut keys = BTreeSet::new();
    for selection in selections {
        if !keys.insert(selection.key.as_str()) {
            return Err(ChatError::validation(
                "modelOptions",
                "Model trait keys must be unique",
            ));
        }
        let definition = definitions.iter().find(|definition| match definition {
            ModelOptionDefinition::Boolean { key, .. }
            | ModelOptionDefinition::Choice { key, .. }
            | ModelOptionDefinition::MultipleChoice { key, .. }
            | ModelOptionDefinition::IntegerRange { key, .. }
            | ModelOptionDefinition::Text { key, .. }
            | ModelOptionDefinition::Unknown { key, .. } => key == &selection.key,
        });
        let valid = match (definition, &selection.value) {
            (Some(ModelOptionDefinition::Boolean { .. }), ModelOptionValue::Boolean(_)) => true,
            (
                Some(ModelOptionDefinition::Choice { options, .. }),
                ModelOptionValue::Choice(value),
            ) => options.iter().any(|option| &option.value == value),
            (
                Some(ModelOptionDefinition::MultipleChoice { options, .. }),
                ModelOptionValue::MultipleChoice(values),
            ) => {
                let unique = values.iter().collect::<BTreeSet<_>>();
                unique.len() == values.len()
                    && values
                        .iter()
                        .all(|value| options.iter().any(|option| &option.value == value))
            }
            (
                Some(ModelOptionDefinition::IntegerRange {
                    minimum,
                    maximum,
                    step,
                    ..
                }),
                ModelOptionValue::Integer(value),
            ) => *step > 0 && value >= minimum && value <= maximum && (value - minimum) % step == 0,
            (
                Some(ModelOptionDefinition::Text { allow_empty, .. }),
                ModelOptionValue::Text(value),
            ) => (*allow_empty || !value.is_empty()) && value.len() <= 100_000,
            (Some(ModelOptionDefinition::Unknown { .. }), ModelOptionValue::Unknown(_)) => true,
            (None, ModelOptionValue::Unknown(_)) => true,
            _ => false,
        };
        if !valid {
            return Err(ChatError::validation(
                "modelOptions",
                "A selected model trait is invalid or unavailable",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_modes(
    capabilities: &ProviderCapabilities,
    modes: TurnModeSnapshot,
) -> ChatResult<()> {
    if modes.interaction_mode == InteractionMode::Plan
        && !capabilities.supports(ProviderCapability::NativePlan)
    {
        return Err(ChatError::new(
            ChatErrorCode::CapabilityUnsupported,
            "Selected provider does not support native Plan mode",
            true,
        ));
    }
    Ok(())
}

pub(super) fn validate_explicit_model(
    provider_managed: bool,
    model_id: Option<&ModelId>,
) -> ChatResult<()> {
    if provider_managed == model_id.is_some() {
        return Err(ChatError::validation(
            "model",
            "Choose exactly one model or the provider-managed model state",
        ));
    }
    Ok(())
}

pub(super) fn validate_prompt(prompt: &str, has_context: bool) -> ChatResult<()> {
    if (!has_context && prompt.trim().is_empty()) || prompt.len() > 16_777_216 {
        return Err(ChatError::validation("prompt", "Chat prompt is invalid"));
    }
    Ok(())
}

pub(super) fn validate_mentions(mentions: &[WorkspaceMentionReference]) -> ChatResult<()> {
    if mentions.len() > 100
        || mentions.iter().any(|mention| {
            !matches!(mention.kind.as_str(), "file" | "directory")
                || mention.relative_path.is_empty()
                || mention.relative_path.len() > 4_096
                || crate::chat::interaction_commands::workspace_mention_is_safety_excluded(
                    &mention.relative_path,
                )
        })
    {
        return Err(ChatError::validation(
            "mentions",
            "Chat mentions are invalid",
        ));
    }
    Ok(())
}

pub(super) fn validate_send_mentions(
    authorized: &AuthorizedWorkingFolder,
    mentions: &[WorkspaceMentionReference],
) -> ChatResult<()> {
    for mention in mentions {
        let path = crate::chat::workspace::resolve_workspace_relative_path(
            authorized,
            &mention.relative_path,
        )?;
        let metadata = std::fs::metadata(path).map_err(|_| {
            ChatError::new(
                ChatErrorCode::NotFound,
                "A mentioned workspace path is missing",
                true,
            )
        })?;
        if (mention.kind == "file") != metadata.is_file()
            || (mention.kind == "directory") != metadata.is_dir()
        {
            return Err(ChatError::validation(
                "mentions",
                "A workspace mention changed type before send",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_answers(answers: &[UserInputAnswer]) -> ChatResult<()> {
    if answers.is_empty()
        || answers.len() > 100
        || answers.iter().any(|answer| {
            answer.question_id.is_empty()
                || answer.selected_option_ids.len() > 100
                || answer
                    .free_form_text
                    .as_ref()
                    .is_some_and(|text| text.len() > 100_000)
        })
    {
        return Err(ChatError::validation(
            "answers",
            "Chat input answers are invalid",
        ));
    }
    Ok(())
}

pub(super) fn prompt_title(prompt: &str) -> String {
    prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("New conversation")
        .trim()
        .chars()
        .take(80)
        .collect()
}

pub(super) fn prompt_preview(prompt: &str) -> String {
    prompt.trim().chars().take(2_000).collect()
}

pub(super) fn wire_safety(value: SafetyMode) -> &'static str {
    match value {
        SafetyMode::AskForApproval => "ask_for_approval",
        SafetyMode::ApproveForMe => "approve_for_me",
        SafetyMode::FullAccess => "full_access",
        SafetyMode::Custom => "custom",
    }
}

pub(super) fn wire_interaction(value: InteractionMode) -> &'static str {
    match value {
        InteractionMode::Build => "build",
        InteractionMode::Plan => "plan",
    }
}
