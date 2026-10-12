//! Cursor ACP permission and structured-question validation.

use super::protocol::{
    MAX_PROTOCOL_TEXT_BYTES, bounded_text, is_valid_identifier, object, protocol_error, safe_shape,
};
use crate::events::*;
use crate::models::*;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};

pub type PendingCursorRequests = Arc<Mutex<HashMap<ProviderRequestId, PendingCursorRequest>>>;

#[derive(Clone, Debug)]
pub struct AcpPermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug)]
pub struct CursorQuestion {
    pub id: String,
    pub option_ids: Vec<String>,
    pub multiple: bool,
}

#[derive(Clone, Debug)]
pub struct XaiQuestion {
    pub id: String,
    pub question: String,
    pub options: Vec<(String, String)>,
    pub multiple: bool,
}

#[derive(Clone, Debug)]
pub enum PendingCursorRequestKind {
    Approval { options: Vec<AcpPermissionOption> },
    UserInput { questions: Vec<CursorQuestion> },
    XaiUserInput { questions: Vec<XaiQuestion> },
}

#[derive(Clone, Debug)]
pub struct PendingCursorRequest {
    pub rpc_id: Value,
    pub kind: PendingCursorRequestKind,
}

pub struct ParsedPermission {
    pub provider_request_id: ProviderRequestId,
    pub request: RequestOpenedEvent,
    pub options: Vec<AcpPermissionOption>,
    pub safely_in_workspace_edit: bool,
}

pub struct ParsedQuestion {
    pub provider_request_id: ProviderRequestId,
    pub request: UserInputRequestedEvent,
    pub questions: Vec<CursorQuestion>,
}

pub fn parse_permission(params: &Value, workspace: &Path) -> ChatResult<ParsedPermission> {
    let object = object(params)?;
    let tool = object
        .get("toolCall")
        .and_then(Value::as_object)
        .ok_or_else(|| protocol_error("permission tool call"))?;
    let tool_call_id = text(tool, "toolCallId")
        .filter(|value| is_valid_identifier(value, 512))
        .ok_or_else(|| protocol_error("permission tool call ID"))?;
    let provider_request_id = ProviderRequestId::new(tool_call_id.to_string())
        .map_err(|_| protocol_error("permission request ID"))?;
    let tool_kind = text(tool, "kind")
        .filter(|kind| is_valid_identifier(kind, 128))
        .unwrap_or("unknown")
        .to_string();
    let options = parse_permission_options(object.get("options"))?;
    let allowed_decisions = options
        .iter()
        .map(|option| ApprovalDecisionOption {
            id: option.option_id.clone(),
            label: option.name.clone(),
            decision_kind: decision_kind(&option.kind),
            description: None,
        })
        .collect();
    let title = match tool.get("title") {
        Some(Value::String(value)) if is_valid_display_text(value, 512) => value.clone(),
        Some(_) => return Err(protocol_error("permission title")),
        None => "Tool permission".to_string(),
    };
    let detail = tool
        .get("rawInput")
        .and_then(Value::as_object)
        .and_then(|input| {
            text(input, "command")
                .or_else(|| text(input, "path"))
                .map(|value| bounded_text(value, MAX_PROTOCOL_TEXT_BYTES))
        });
    Ok(ParsedPermission {
        provider_request_id: provider_request_id.clone(),
        request: RequestOpenedEvent {
            request_id: provider_request_id,
            kind: request_kind(&tool_kind),
            title,
            detail,
            allowed_decisions,
            safe_payload: safe_shape(params),
        },
        safely_in_workspace_edit: tool_kind == "edit"
            && tool_paths_are_in_workspace(tool, workspace),
        options,
    })
}

pub fn parse_question(params: &Value) -> ChatResult<ParsedQuestion> {
    let object = object(params)?;
    let tool_call_id = text(object, "toolCallId")
        .filter(|value| is_valid_identifier(value, 512))
        .ok_or_else(|| protocol_error("Cursor question tool call ID"))?;
    let provider_request_id = ProviderRequestId::new(tool_call_id.to_string())
        .map_err(|_| protocol_error("Cursor question request ID"))?;
    let raw_questions = object
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error("Cursor questions"))?;
    if raw_questions.is_empty() || raw_questions.len() > 16 {
        return Err(protocol_error("Cursor questions"));
    }
    let mut questions = Vec::new();
    let mut pending = Vec::new();
    let mut question_ids = BTreeSet::new();
    for raw in raw_questions {
        let raw = raw
            .as_object()
            .ok_or_else(|| protocol_error("Cursor question"))?;
        let id = text(raw, "id")
            .filter(|value| is_valid_identifier(value, 256))
            .ok_or_else(|| protocol_error("Cursor question ID"))?
            .to_string();
        if !question_ids.insert(id.clone()) {
            return Err(protocol_error("duplicate Cursor question ID"));
        }
        let prompt = text(raw, "prompt")
            .filter(|value| is_valid_display_text(value, MAX_PROTOCOL_TEXT_BYTES))
            .ok_or_else(|| protocol_error("Cursor question prompt"))?;
        let raw_options = raw
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if raw_options.len() > 64 {
            return Err(protocol_error("Cursor question options"));
        }
        let mut option_ids = Vec::new();
        let mut seen_option_ids = BTreeSet::new();
        let options = raw_options
            .iter()
            .map(|option| {
                let option = option
                    .as_object()
                    .ok_or_else(|| protocol_error("Cursor question option"))?;
                let option_id = text(option, "id")
                    .filter(|value| is_valid_identifier(value, 256))
                    .ok_or_else(|| protocol_error("Cursor question option ID"))?;
                if !seen_option_ids.insert(option_id.to_string()) {
                    return Err(protocol_error("duplicate Cursor question option ID"));
                }
                let label = text(option, "label")
                    .filter(|value| is_valid_display_text(value, 512))
                    .ok_or_else(|| protocol_error("Cursor question option label"))?;
                option_ids.push(option_id.to_string());
                Ok(UserInputOption {
                    id: option_id.to_string(),
                    label: bounded_text(label, 512),
                    description: None,
                })
            })
            .collect::<ChatResult<Vec<_>>>()?;
        let multiple = raw
            .get("allowMultiple")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        questions.push(UserInputQuestion {
            id: id.clone(),
            header: object
                .get("title")
                .and_then(Value::as_str)
                .map(|value| bounded_text(value, 128)),
            question: bounded_text(prompt, MAX_PROTOCOL_TEXT_BYTES),
            options,
            multiple,
            free_form_allowed: true,
            required: true,
        });
        pending.push(CursorQuestion {
            id,
            option_ids,
            multiple,
        });
    }
    Ok(ParsedQuestion {
        provider_request_id: provider_request_id.clone(),
        request: UserInputRequestedEvent {
            request_id: provider_request_id,
            questions,
        },
        questions: pending,
    })
}

pub fn parse_xai_question(params: &Value) -> ChatResult<(ParsedQuestion, Vec<XaiQuestion>)> {
    let object = object(params)?;
    let object = if object.get("method").and_then(Value::as_str).is_some() {
        object
            .get("params")
            .and_then(Value::as_object)
            .ok_or_else(|| protocol_error("Grok question parameters"))?
    } else {
        object
    };
    let tool_call_id = text(object, "toolCallId")
        .filter(|value| is_valid_identifier(value, 512))
        .ok_or_else(|| protocol_error("Grok question tool call ID"))?;
    text(object, "sessionId")
        .filter(|value| is_valid_identifier(value, 512))
        .ok_or_else(|| protocol_error("Grok question session ID"))?;
    text(object, "mode")
        .filter(|value| matches!(*value, "default" | "plan"))
        .ok_or_else(|| protocol_error("Grok question mode"))?;
    let provider_request_id = ProviderRequestId::new(tool_call_id.to_string())
        .map_err(|_| protocol_error("Grok question request ID"))?;
    let raw_questions = object
        .get("questions")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error("Grok questions"))?;
    if raw_questions.is_empty() || raw_questions.len() > 16 {
        return Err(protocol_error("Grok questions"));
    }
    let mut canonical = Vec::new();
    let mut pending = Vec::new();
    let mut question_ids = BTreeSet::new();
    for raw in raw_questions {
        let raw = raw
            .as_object()
            .ok_or_else(|| protocol_error("Grok question"))?;
        let question = text(raw, "question")
            .filter(|value| is_valid_display_text(value, MAX_PROTOCOL_TEXT_BYTES))
            .ok_or_else(|| protocol_error("Grok question text"))?;
        let id = text(raw, "id")
            .filter(|value| is_valid_identifier(value, 256))
            .unwrap_or(question)
            .to_string();
        if !question_ids.insert(id.clone()) {
            return Err(protocol_error("duplicate Grok question ID"));
        }
        let raw_options = raw
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if raw_options.len() > 64 {
            return Err(protocol_error("Grok question options"));
        }
        let mut options = Vec::new();
        let mut canonical_options = Vec::new();
        let mut option_ids = BTreeSet::new();
        for option in raw_options {
            let option = option
                .as_object()
                .ok_or_else(|| protocol_error("Grok question option"))?;
            let label = text(option, "label")
                .filter(|value| is_valid_display_text(value, 512))
                .ok_or_else(|| protocol_error("Grok question option label"))?;
            let option_id = text(option, "id")
                .filter(|value| is_valid_identifier(value, 256))
                .unwrap_or(label)
                .to_string();
            if !option_ids.insert(option_id.clone()) {
                return Err(protocol_error("duplicate Grok question option ID"));
            }
            options.push((option_id.clone(), label.to_string()));
            canonical_options.push(UserInputOption {
                id: option_id,
                label: bounded_text(label, 512),
                description: text(option, "description").map(|value| bounded_text(value, 512)),
            });
        }
        let multiple = raw
            .get("multiSelect")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        canonical.push(UserInputQuestion {
            id: id.clone(),
            header: Some("Question".to_string()),
            question: bounded_text(question, MAX_PROTOCOL_TEXT_BYTES),
            options: canonical_options,
            multiple,
            free_form_allowed: true,
            required: true,
        });
        pending.push(XaiQuestion {
            id,
            question: question.to_string(),
            options,
            multiple,
        });
    }
    Ok((
        ParsedQuestion {
            provider_request_id: provider_request_id.clone(),
            request: UserInputRequestedEvent {
                request_id: provider_request_id,
                questions: canonical,
            },
            questions: Vec::new(),
        },
        pending,
    ))
}

pub fn auto_permission_option(
    parsed: &ParsedPermission,
    safety_mode: SafetyMode,
) -> Option<&AcpPermissionOption> {
    match safety_mode {
        SafetyMode::AskForApproval if parsed.safely_in_workspace_edit => parsed
            .options
            .iter()
            .find(|option| option.kind == "allow_once"),
        SafetyMode::AskForApproval | SafetyMode::ApproveForMe | SafetyMode::Custom => None,
        SafetyMode::FullAccess => parsed
            .options
            .iter()
            .find(|option| option.kind == "allow_always")
            .or_else(|| {
                parsed
                    .options
                    .iter()
                    .find(|option| option.kind == "allow_once")
            }),
    }
}

pub fn resolve_approval_result(
    pending: &PendingCursorRequest,
    decision: &ApprovalDecision,
) -> ChatResult<Value> {
    let PendingCursorRequestKind::Approval { options } = &pending.kind else {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "ACP provider request is not an approval",
            false,
        ));
    };
    if decision.kind == ApprovalDecisionKind::Cancel {
        return Ok(json!({ "outcome": { "outcome": "cancelled" } }));
    }
    let expected_kind = match decision.kind {
        ApprovalDecisionKind::AllowOnce => "allow_once",
        ApprovalDecisionKind::AllowSession => "allow_always",
        ApprovalDecisionKind::Deny => "reject",
        ApprovalDecisionKind::Cancel => unreachable!(),
    };
    let selected = if let Some(option_id) = decision.provider_option_id.as_deref() {
        options.iter().find(|option| {
            option.option_id == option_id
                && if expected_kind == "reject" {
                    option.kind.starts_with("reject_")
                } else {
                    option.kind == expected_kind
                }
        })
    } else {
        options.iter().find(|option| {
            if expected_kind == "reject" {
                option.kind == "reject_once"
            } else {
                option.kind == expected_kind
            }
        })
    }
    .ok_or_else(|| {
        ChatError::validation(
            "decision.providerOptionId",
            "Approval decision was not one of the provider-offered options",
        )
    })?;
    Ok(json!({
        "outcome": { "outcome": "selected", "optionId": selected.option_id }
    }))
}

pub fn resolve_question_result(
    pending: &PendingCursorRequest,
    answers: &[UserInputAnswer],
) -> ChatResult<Value> {
    let questions = match &pending.kind {
        PendingCursorRequestKind::UserInput { questions } => questions,
        PendingCursorRequestKind::XaiUserInput { questions } => {
            return resolve_xai_question_result(questions, answers);
        }
        PendingCursorRequestKind::Approval { .. } => {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "ACP request is not structured input",
                false,
            ));
        }
    };
    let mut by_id = BTreeMap::new();
    for answer in answers {
        if by_id.insert(answer.question_id.as_str(), answer).is_some() {
            return Err(ChatError::validation(
                "answers",
                "Cursor question answers must have unique question IDs",
            ));
        }
    }
    let mut result = Map::new();
    for question in questions {
        let answer = by_id.get(question.id.as_str()).ok_or_else(|| {
            ChatError::validation("answers", "Cursor requires an answer for every question")
        })?;
        if !question.multiple && answer.selected_option_ids.len() > 1 {
            return Err(ChatError::validation(
                "answers",
                "Cursor question allows only one option",
            ));
        }
        if answer
            .selected_option_ids
            .iter()
            .any(|id| !question.option_ids.contains(id))
        {
            return Err(ChatError::validation(
                "answers",
                "Cursor answer contains an option the provider did not offer",
            ));
        }
        let value =
            if let Some(text) = answer
                .free_form_text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                Value::String(bounded_text(text, MAX_PROTOCOL_TEXT_BYTES))
            } else if question.multiple {
                Value::Array(
                    answer
                        .selected_option_ids
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                )
            } else {
                Value::String(answer.selected_option_ids.first().cloned().ok_or_else(|| {
                    ChatError::validation("answers", "Cursor answer cannot be empty")
                })?)
            };
        result.insert(question.id.clone(), value);
    }
    Ok(json!({ "answers": result }))
}

fn resolve_xai_question_result(
    questions: &[XaiQuestion],
    answers: &[UserInputAnswer],
) -> ChatResult<Value> {
    let mut by_id = BTreeMap::new();
    for answer in answers {
        if by_id.insert(answer.question_id.as_str(), answer).is_some() {
            return Err(ChatError::validation(
                "answers",
                "Grok question answers must have unique question IDs",
            ));
        }
    }
    let mut result = Map::new();
    for question in questions {
        let answer = by_id.get(question.id.as_str()).ok_or_else(|| {
            ChatError::validation("answers", "Grok requires an answer for every question")
        })?;
        if !question.multiple && answer.selected_option_ids.len() > 1 {
            return Err(ChatError::validation(
                "answers",
                "Grok question allows only one option",
            ));
        }
        let labels = answer
            .selected_option_ids
            .iter()
            .map(|id| {
                question
                    .options
                    .iter()
                    .find(|(option_id, _)| option_id == id)
                    .map(|(_, label)| label.clone())
                    .ok_or_else(|| {
                        ChatError::validation(
                            "answers",
                            "Grok answer contains an option the provider did not offer",
                        )
                    })
            })
            .collect::<ChatResult<Vec<_>>>()?;
        let mut values = labels;
        if let Some(text) = answer
            .free_form_text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            values.push(bounded_text(text, MAX_PROTOCOL_TEXT_BYTES));
        }
        if values.is_empty() {
            return Err(ChatError::validation(
                "answers",
                "Grok answer cannot be empty",
            ));
        }
        result.insert(
            question.question.clone(),
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }
    Ok(json!({ "outcome": "accepted", "answers": result }))
}

fn parse_permission_options(value: Option<&Value>) -> ChatResult<Vec<AcpPermissionOption>> {
    let options = value
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error("permission options"))?;
    if options.is_empty() || options.len() > 16 {
        return Err(protocol_error("permission options"));
    }
    let mut parsed = Vec::new();
    let mut option_ids = BTreeSet::new();
    for option in options {
        let option = option
            .as_object()
            .ok_or_else(|| protocol_error("permission option"))?;
        let option_id = text(option, "optionId")
            .filter(|value| is_valid_identifier(value, 256))
            .ok_or_else(|| protocol_error("permission option ID"))?;
        if !option_ids.insert(option_id.to_string()) {
            return Err(protocol_error("duplicate permission option ID"));
        }
        let name = text(option, "name")
            .filter(|value| is_valid_display_text(value, 512))
            .ok_or_else(|| protocol_error("permission option name"))?;
        let kind = text(option, "kind")
            .filter(|kind| {
                matches!(
                    *kind,
                    "allow_once" | "allow_always" | "reject_once" | "reject_always"
                )
            })
            .ok_or_else(|| protocol_error("permission option kind"))?;
        parsed.push(AcpPermissionOption {
            option_id: option_id.to_string(),
            name: bounded_text(name, 512),
            kind: kind.to_string(),
        });
    }
    Ok(parsed)
}

fn tool_paths_are_in_workspace(tool: &Map<String, Value>, workspace: &Path) -> bool {
    let mut paths = Vec::new();
    if let Some(locations) = tool.get("locations").and_then(Value::as_array) {
        paths.extend(
            locations
                .iter()
                .filter_map(|location| location.get("path").and_then(Value::as_str)),
        );
    }
    if let Some(input) = tool.get("rawInput").and_then(Value::as_object) {
        paths.extend(
            ["path", "file", "target", "destination"]
                .into_iter()
                .filter_map(|key| input.get(key).and_then(Value::as_str)),
        );
    }
    !paths.is_empty()
        && paths
            .into_iter()
            .all(|path| path_is_in_workspace(path, workspace))
}

fn path_is_in_workspace(value: &str, workspace: &Path) -> bool {
    let path = Path::new(value);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    }) && !path.is_absolute()
    {
        return false;
    }
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let mut existing = candidate.as_path();
    while !existing.exists() {
        let Some(parent) = existing.parent() else {
            return false;
        };
        existing = parent;
    }
    std::fs::canonicalize(existing).is_ok_and(|canonical| canonical.starts_with(workspace))
}

fn request_kind(kind: &str) -> CanonicalRequestKind {
    match kind {
        "execute" => CanonicalRequestKind::CommandExecution,
        "read" => CanonicalRequestKind::FileRead,
        "edit" | "delete" | "move" => CanonicalRequestKind::FileChange,
        _ => CanonicalRequestKind::Unknown,
    }
}

fn decision_kind(kind: &str) -> ApprovalDecisionKind {
    match kind {
        "allow_once" => ApprovalDecisionKind::AllowOnce,
        "allow_always" => ApprovalDecisionKind::AllowSession,
        "reject_once" | "reject_always" => ApprovalDecisionKind::Deny,
        _ => ApprovalDecisionKind::Deny,
    }
}

fn is_valid_display_text(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_bytes
        && !value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}
