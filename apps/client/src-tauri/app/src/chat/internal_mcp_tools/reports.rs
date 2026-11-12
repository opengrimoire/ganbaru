//! The ask and report host tools: a teammate asks the person who assigned the work, or posts a
//! plan, replan, or result in the reply thread.
//!
//! Both write in one transaction through the organizational coordination services, which keep
//! the call idempotent by its client request id and refuse a run whose work already ended.

use super::delegation::{MAX_CLIENT_REQUEST_ID_CHARS, required_text};
use super::{HostToolContext, generic_denial_error, required_string};
use crate::chat::command_support::now_timestamp;
use ganbaru_chat::coordination::delegation::MAX_REPORT_BODY_BYTES;
use ganbaru_chat::coordination::reports::{
    HostCall, HostReportRefusal, MAX_QUESTION_CHARS, MAX_QUESTION_CHOICE_CHARS,
    MAX_QUESTION_CHOICES, ReportKind, record_host_question, record_host_report,
    validate_report_body,
};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use serde_json::{Map, Value, json};

/// The input properties of the ask tool.
pub(super) fn ask_properties() -> Value {
    json!({
        "question": { "type": "string", "minLength": 1, "maxLength": MAX_QUESTION_CHARS },
        "choices": {
            "type": "array",
            "maxItems": MAX_QUESTION_CHOICES,
            "items": {
                "type": "string", "minLength": 1, "maxLength": MAX_QUESTION_CHOICE_CHARS
            }
        },
        "clientRequestId": {
            "type": "string", "minLength": 1, "maxLength": MAX_CLIENT_REQUEST_ID_CHARS
        }
    })
}

/// The input properties of the report tool.
pub(super) fn report_properties() -> Value {
    json!({
        "kind": {
            "type": "string",
            "enum": ReportKind::ALL.map(ReportKind::as_str)
        },
        "body": { "type": "string", "minLength": 1, "maxLength": MAX_REPORT_BODY_BYTES },
        "clientRequestId": {
            "type": "string", "minLength": 1, "maxLength": MAX_CLIENT_REQUEST_ID_CHARS
        }
    })
}

/// Asks the person who assigned the work, moving the assignment to waiting for an answer.
pub(super) async fn ask(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let question = required_text(arguments, "question", MAX_QUESTION_CHARS)?;
    let choices = parse_choices(arguments)?;
    let client_request_id = client_request_id(arguments)?;
    let now = now_timestamp()?;
    let mut transaction = context
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    record_host_question(
        &mut transaction,
        host_call(context, client_request_id),
        &question,
        &choices,
        now.as_str(),
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(json!({ "status": "waiting" }))
}

/// Posts a plan, replan, or result report in the reply thread.
pub(super) async fn report(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let kind = required_string(arguments, "kind", 32)
        .ok()
        .and_then(ReportKind::from_name)
        .ok_or_else(|| HostReportRefusal::UnknownKind.into_error())?;
    let body = report_body(arguments)?;
    let client_request_id = client_request_id(arguments)?;
    let now = now_timestamp()?;
    let mut transaction = context
        .pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    record_host_report(
        &mut transaction,
        host_call(context, client_request_id),
        kind,
        &body,
        now.as_str(),
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(json!({ "status": "posted", "kind": kind.as_str() }))
}

fn host_call<'a>(context: &'a HostToolContext<'_>, client_request_id: &'a str) -> HostCall<'a> {
    HostCall {
        run_id: context.scope.run_id.as_str(),
        assignment_id: context.scope.assignment_id.as_str(),
        client_request_id,
    }
}

/// The trimmed report body. An empty or oversized body is a typed refusal, so the teammate can
/// shorten it; any other malformed body is denied generically.
fn report_body(arguments: &Map<String, Value>) -> ChatResult<String> {
    let body = arguments
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(generic_denial_error)?;
    let body = validate_report_body(body).map_err(HostReportRefusal::into_error)?;
    if body
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        return Err(generic_denial_error());
    }
    Ok(body.to_owned())
}

/// The answer choices: absent or at most [`MAX_QUESTION_CHOICES`] distinct single-line texts of
/// at most [`MAX_QUESTION_CHOICE_CHARS`] characters each.
fn parse_choices(arguments: &Map<String, Value>) -> ChatResult<Vec<String>> {
    let entries = match arguments.get("choices") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(entries)) if entries.len() <= MAX_QUESTION_CHOICES => entries,
        Some(_) => return Err(generic_denial_error()),
    };
    let mut choices: Vec<String> = Vec::with_capacity(entries.len());
    for entry in entries {
        let choice = entry
            .as_str()
            .map(str::trim)
            .filter(|choice| {
                !choice.is_empty()
                    && choice.chars().count() <= MAX_QUESTION_CHOICE_CHARS
                    && !choice.chars().any(char::is_control)
            })
            .ok_or_else(generic_denial_error)?;
        if choices.iter().any(|existing| existing == choice) {
            return Err(generic_denial_error());
        }
        choices.push(choice.to_owned());
    }
    Ok(choices)
}

fn client_request_id(arguments: &Map<String, Value>) -> ChatResult<&str> {
    let client_request_id = required_string(
        arguments,
        "clientRequestId",
        MAX_CLIENT_REQUEST_ID_CHARS * 4,
    )?;
    if client_request_id.trim().is_empty()
        || client_request_id.chars().count() > MAX_CLIENT_REQUEST_ID_CHARS
    {
        return Err(generic_denial_error());
    }
    Ok(client_request_id)
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat teammate report could not be saved",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn choices_are_optional_bounded_distinct_and_single_line() {
        assert!(parse_choices(&arguments(json!({}))).unwrap().is_empty());
        assert_eq!(
            parse_choices(&arguments(json!({ "choices": [" Keep ", "Drop"] }))).unwrap(),
            ["Keep", "Drop"]
        );
        for invalid in [
            json!({ "choices": ["a", "b", "c", "d", "e"] }),
            json!({ "choices": ["Keep", "Keep"] }),
            json!({ "choices": ["two\nlines"] }),
            json!({ "choices": [" "] }),
            json!({ "choices": ["x".repeat(MAX_QUESTION_CHOICE_CHARS + 1)] }),
            json!({ "choices": "Keep" }),
        ] {
            assert!(
                parse_choices(&arguments(invalid.clone())).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn report_bodies_refuse_empty_and_oversized_text_with_their_codes() {
        assert_eq!(
            report_body(&arguments(json!({ "body": " Plan\n1. Read " }))).unwrap(),
            "Plan\n1. Read"
        );
        let refusal = |value: Value| {
            HostReportRefusal::from_error(&report_body(&arguments(value)).unwrap_err())
        };
        assert_eq!(
            refusal(json!({ "body": "  " })),
            Some(HostReportRefusal::EmptyBody)
        );
        assert_eq!(
            refusal(json!({ "body": "x".repeat(MAX_REPORT_BODY_BYTES + 1) })),
            Some(HostReportRefusal::BodyTooLarge)
        );
        assert_eq!(refusal(json!({ "body": "bell\u{7}" })), None);
        assert_eq!(refusal(json!({})), None);
    }

    #[test]
    fn client_request_ids_are_bounded_and_not_blank() {
        assert_eq!(
            client_request_id(&arguments(json!({ "clientRequestId": "r-1" }))).unwrap(),
            "r-1"
        );
        for invalid in [
            json!({ "clientRequestId": " " }),
            json!({ "clientRequestId": "r".repeat(MAX_CLIENT_REQUEST_ID_CHARS + 1) }),
            json!({}),
        ] {
            assert!(client_request_id(&arguments(invalid)).is_err());
        }
    }
}
