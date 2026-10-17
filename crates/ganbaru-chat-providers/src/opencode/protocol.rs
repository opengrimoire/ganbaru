//! OpenCode resume cursors and bounded protocol helpers.

use crate::models::{ChatError, ChatErrorCode, ChatResult, VersionedJson};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const OPENCODE_RESUME_SCHEMA_VERSION: u32 = 1;
pub const MAX_HTTP_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_EVENT_DATA_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeResumeCursor {
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeRollbackCursor {
    pub message_id: String,
    #[serde(default)]
    pub part_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
}

pub fn parse_commands(value: Value) -> ChatResult<Vec<OpenCodeCommand>> {
    let entries = value
        .as_array()
        .ok_or_else(|| protocol_error("command catalog"))?;
    if entries.len() > 512 {
        return Err(protocol_error("command catalog"));
    }
    let mut seen = BTreeSet::new();
    entries
        .iter()
        .map(|value| {
            let object = value
                .as_object()
                .ok_or_else(|| protocol_error("command catalog entry"))?;
            let name = object
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
                .ok_or_else(|| protocol_error("command name"))?;
            if !seen.insert(name.to_ascii_lowercase()) {
                return Err(protocol_error("duplicate command name"));
            }
            let description = object
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 1_000
                        && !value.chars().any(char::is_control)
                })
                .map(str::to_string);
            let argument_hint = object
                .get("template")
                .and_then(Value::as_str)
                .filter(|template| template_uses_arguments(template))
                .map(|_| "[arguments]".to_string());
            Ok(OpenCodeCommand {
                name: name.to_string(),
                description,
                argument_hint,
            })
        })
        .collect()
}

fn template_uses_arguments(template: &str) -> bool {
    template.contains("$ARGUMENTS")
        || template
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b'$' && matches!(pair[1], b'1'..=b'9'))
}

pub fn resume_cursor(session_id: &str) -> ChatResult<VersionedJson> {
    validate_identifier(session_id, "session ID")?;
    Ok(VersionedJson {
        schema_version: OPENCODE_RESUME_SCHEMA_VERSION,
        value: serde_json::to_value(OpenCodeResumeCursor {
            session_id: session_id.to_string(),
        })
        .map_err(|_| protocol_error("resume cursor"))?,
    })
}

pub fn parse_resume_cursor(value: &VersionedJson) -> ChatResult<OpenCodeResumeCursor> {
    if value.schema_version != OPENCODE_RESUME_SCHEMA_VERSION {
        return Err(ChatError::validation(
            "resumeCursor.schemaVersion",
            "OpenCode resume cursor schema is unsupported",
        ));
    }
    let cursor = serde_json::from_value::<OpenCodeResumeCursor>(value.value.clone())
        .map_err(|_| ChatError::validation("resumeCursor", "OpenCode resume cursor is invalid"))?;
    validate_identifier(&cursor.session_id, "session ID")?;
    Ok(cursor)
}

pub fn parse_rollback_cursor(value: &VersionedJson) -> ChatResult<OpenCodeRollbackCursor> {
    if value.schema_version != OPENCODE_RESUME_SCHEMA_VERSION {
        return Err(ChatError::validation(
            "providerCursor.schemaVersion",
            "OpenCode rollback cursor schema is unsupported",
        ));
    }
    let cursor =
        serde_json::from_value::<OpenCodeRollbackCursor>(value.value.clone()).map_err(|_| {
            ChatError::validation("providerCursor", "OpenCode rollback cursor is invalid")
        })?;
    validate_identifier(&cursor.message_id, "message ID")?;
    if let Some(part_id) = cursor.part_id.as_deref() {
        validate_identifier(part_id, "part ID")?;
    }
    Ok(cursor)
}

pub fn validate_identifier(value: &str, label: &str) -> ChatResult<()> {
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(|character| character.is_control())
    {
        return Err(protocol_error(label));
    }
    Ok(())
}

pub fn protocol_error(detail: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("OpenCode returned an invalid {detail}"),
        false,
    )
}
