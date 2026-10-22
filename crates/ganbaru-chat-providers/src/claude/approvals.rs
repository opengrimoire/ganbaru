//! Approval subjects and session permission grants derived from Claude permission requests.
//!
//! The approval panel shows the subject and the grants built here, and an allow-for-session
//! decision forwards exactly these grants, so what the user sees is what Claude receives.

use crate::events::{ApprovalSubject, ApprovalSubjectKind, SessionPermissionGrant};
use serde_json::{Map, Value, json};

/// Largest subject text shown in an approval panel. Longer values are cut and marked.
const MAX_SUBJECT_BYTES: usize = 16 * 1024;
/// Most session grants one approval forwards. Later suggestions are not offered.
const MAX_SESSION_GRANTS: usize = 32;
/// Largest tool name, rule, or directory a session grant accepts. Longer ones are not offered.
const MAX_GRANT_TEXT_BYTES: usize = 4 * 1024;
/// The only permission mode a session grant may switch to.
const ACCEPT_EDITS_MODE: &str = "acceptEdits";

/// Builds the concrete thing a Claude tool call acts on from its input.
pub(super) fn approval_subject(tool_name: &str, input: &Value) -> Option<ApprovalSubject> {
    let input = input.as_object()?;
    let (kind, key) = match tool_name {
        "Bash" | "PowerShell" => (ApprovalSubjectKind::Command, "command"),
        "Read" | "Write" | "Edit" | "MultiEdit" => (ApprovalSubjectKind::Path, "file_path"),
        "NotebookEdit" => (ApprovalSubjectKind::Path, "notebook_path"),
        "Glob" | "Grep" => (ApprovalSubjectKind::Pattern, "pattern"),
        "WebFetch" => (ApprovalSubjectKind::Url, "url"),
        "WebSearch" => (ApprovalSubjectKind::Query, "query"),
        name if name.starts_with("mcp__") => {
            let (text, truncated) = bounded(name);
            return Some(ApprovalSubject {
                kind: ApprovalSubjectKind::Tool,
                text,
                location: None,
                truncated,
            });
        }
        _ => return None,
    };
    let (text, text_truncated) = bounded(non_empty_text(input, key)?);
    let location = (kind == ApprovalSubjectKind::Pattern)
        .then(|| non_empty_text(input, "path"))
        .flatten()
        .map(bounded);
    let location_truncated = location.as_ref().is_some_and(|(_, truncated)| *truncated);
    Some(ApprovalSubject {
        kind,
        text,
        location: location.map(|(value, _)| value),
        truncated: text_truncated || location_truncated,
    })
}

/// Reduces Claude's `permission_suggestions` to grants that stay inside the running session.
///
/// Allow rules, extra directories, and accepting edits are kept and rescoped to the session.
/// Deny and ask rules, rule replacement or removal, directory removal, and every other mode
/// are dropped, as are malformed or oversized entries. Persistent destinations would rewrite
/// the folder's Claude settings and invalidate its trust decision.
pub(super) fn session_grants(suggestions: Option<&Value>) -> Vec<SessionPermissionGrant> {
    let Some(suggestions) = suggestions.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut grants = Vec::new();
    for update in suggestions.iter().filter_map(Value::as_object) {
        for grant in update_grants(update) {
            if grants.len() == MAX_SESSION_GRANTS {
                return grants;
            }
            if !grants.contains(&grant) {
                grants.push(grant);
            }
        }
    }
    grants
}

/// Encodes grants as Claude permission updates for the running session.
pub(super) fn session_permission_updates(grants: &[SessionPermissionGrant]) -> Vec<Value> {
    grants
        .iter()
        .map(|grant| match grant {
            SessionPermissionGrant::AllowRule {
                tool_name,
                rule_content,
            } => {
                let mut rule =
                    Map::from_iter([("toolName".to_string(), Value::String(tool_name.clone()))]);
                if let Some(content) = rule_content {
                    rule.insert("ruleContent".to_string(), Value::String(content.clone()));
                }
                json!({
                    "type": "addRules",
                    "rules": [rule],
                    "behavior": "allow",
                    "destination": "session",
                })
            }
            SessionPermissionGrant::Directory { path } => json!({
                "type": "addDirectories",
                "directories": [path],
                "destination": "session",
            }),
            SessionPermissionGrant::AcceptEdits => json!({
                "type": "setMode",
                "mode": ACCEPT_EDITS_MODE,
                "destination": "session",
            }),
        })
        .collect()
}

fn update_grants(update: &Map<String, Value>) -> Vec<SessionPermissionGrant> {
    match update.get("type").and_then(Value::as_str) {
        Some("addRules") if update.get("behavior").and_then(Value::as_str) == Some("allow") => {
            update
                .get("rules")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .filter_map(allow_rule)
                .collect()
        }
        Some("addDirectories") => update
            .get("directories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|path| grant_text_fits(path))
            .map(|path| SessionPermissionGrant::Directory {
                path: path.to_string(),
            })
            .collect(),
        Some("setMode")
            if update.get("mode").and_then(Value::as_str) == Some(ACCEPT_EDITS_MODE) =>
        {
            vec![SessionPermissionGrant::AcceptEdits]
        }
        _ => Vec::new(),
    }
}

fn allow_rule(rule: &Map<String, Value>) -> Option<SessionPermissionGrant> {
    let tool_name = rule
        .get("toolName")
        .and_then(Value::as_str)
        .filter(|name| grant_text_fits(name))?;
    let rule_content = match rule.get("ruleContent") {
        None | Some(Value::Null) => None,
        Some(Value::String(content)) if grant_text_fits(content) => Some(content.clone()),
        Some(_) => return None,
    };
    Some(SessionPermissionGrant::AllowRule {
        tool_name: tool_name.to_string(),
        rule_content,
    })
}

fn grant_text_fits(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_GRANT_TEXT_BYTES
}

fn non_empty_text<'a>(input: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn bounded(value: &str) -> (String, bool) {
    if value.len() <= MAX_SUBJECT_BYTES {
        return (value.to_string(), false);
    }
    let mut end = MAX_SUBJECT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}
