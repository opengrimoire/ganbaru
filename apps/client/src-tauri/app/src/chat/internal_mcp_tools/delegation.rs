//! The delegation host tool: a teammate hands bounded work to another teammate in its channel.
//!
//! The child is a separate assignment in its own reply thread. Its folders and ranks never
//! exceed what the calling run holds, and the organization layer enforces depth, fan-out, the
//! target, and idempotency in one transaction.

use super::{HostToolContext, generic_denial_error, required_string};
use crate::chat::organization::{DelegatedFolder, DelegationRequest, delegate_assignment};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatFolderCapability, ChatParticipantId, ChatResult,
    ProjectWorkingFolderId,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

/// Levels of delegation below work a person started.
pub(crate) const MAX_DELEGATION_DEPTH: u8 = 2;
/// Open delegated children one assignment may hold, which also bounds its delegation calls.
pub(crate) const MAX_OPEN_CHILDREN_PER_ASSIGNMENT: u32 = 4;
/// Longest objective a teammate may delegate, in characters.
pub(crate) const MAX_DELEGATION_OBJECTIVE_CHARS: usize = 4000;
/// Longest expected result a teammate may describe, in characters.
pub(crate) const MAX_DELEGATION_EXPECTED_RESULT_CHARS: usize = 2000;
/// Longest client request id, in characters.
pub(crate) const MAX_CLIENT_REQUEST_ID_CHARS: usize = 256;

/// Why a delegation call was refused. [`DelegationRefusal::code`] is the tool error code, so
/// the teammate can tell a limit from a missing target and adjust its next call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DelegationRefusal {
    /// The child would sit deeper than [`MAX_DELEGATION_DEPTH`].
    DepthExceeded,
    /// The parent already holds [`MAX_OPEN_CHILDREN_PER_ASSIGNMENT`] open children.
    FanOutExceeded,
    /// A requested folder or rank is above what the parent holds.
    AuthorityExceeded,
    /// The target is not a participating teammate of the channel, is archived, or is the caller.
    TargetUnavailable,
    /// The client request id was used before with different input.
    RequestConflict,
    /// The parent assignment is not working.
    ParentInactive,
}

impl DelegationRefusal {
    /// The stable tool error code.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::DepthExceeded => "delegation_depth_exceeded",
            Self::FanOutExceeded => "delegation_fan_out_exceeded",
            Self::AuthorityExceeded => "delegation_authority_exceeded",
            Self::TargetUnavailable => "delegation_target_unavailable",
            Self::RequestConflict => "delegation_request_conflict",
            Self::ParentInactive => "delegation_parent_inactive",
        }
    }

    const ALL: [Self; 6] = [
        Self::DepthExceeded,
        Self::FanOutExceeded,
        Self::AuthorityExceeded,
        Self::TargetUnavailable,
        Self::RequestConflict,
        Self::ParentInactive,
    ];

    /// The refusal as the tool error the teammate receives.
    pub(crate) fn into_error(self) -> ChatError {
        ChatError::new(ChatErrorCode::Permission, self.code(), false)
    }

    /// The refusal an error made by [`DelegationRefusal::into_error`] carries, if any.
    pub(super) fn from_error(error: &ChatError) -> Option<Self> {
        if error.code != ChatErrorCode::Permission {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|refusal| refusal.code() == error.message)
    }
}

/// A delegation call failed either with a typed refusal the teammate may act on, or with an
/// error that is denied generically.
pub(super) enum DelegationCallError {
    Refused(DelegationRefusal),
    Failed(ChatError),
}

impl From<ChatError> for DelegationCallError {
    fn from(error: ChatError) -> Self {
        Self::Failed(error)
    }
}

/// The input properties of the delegation tool.
pub(super) fn properties() -> Value {
    json!({
        "teammateId": { "type": "string", "minLength": 1, "maxLength": 1024 },
        "objective": {
            "type": "string", "minLength": 1, "maxLength": MAX_DELEGATION_OBJECTIVE_CHARS
        },
        "expectedResult": {
            "type": "string", "minLength": 1, "maxLength": MAX_DELEGATION_EXPECTED_RESULT_CHARS
        },
        "folders": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "workingFolderId": { "type": "string", "minLength": 1, "maxLength": 1024 },
                    "capability": {
                        "type": "string", "enum": ["read", "edit", "execute", "publish"]
                    }
                },
                "required": ["workingFolderId", "capability"],
                "additionalProperties": false
            }
        },
        "clientRequestId": {
            "type": "string", "minLength": 1, "maxLength": MAX_CLIENT_REQUEST_ID_CHARS
        }
    })
}

/// Creates the delegated child the call describes, or returns the child an earlier call with
/// the same client request id created.
pub(super) async fn delegate(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> Result<Value, DelegationCallError> {
    let input = parse_input(arguments)?;
    let folders = folder_ceiling(context, input.folders.as_deref())?;
    let child = delegate_assignment(
        context.app,
        context.pool,
        DelegationRequest {
            parent_assignment_id: &context.scope.assignment_id,
            parent_authorization_revision_id: &context.scope.authorization_revision_id,
            conversation_id: &context.scope.destination_conversation_id,
            teammate_id: &input.teammate_id,
            objective: &input.objective,
            expected_result: &input.expected_result,
            folders: &folders,
            client_request_id: &input.client_request_id,
        },
    )
    .await?
    .map_err(DelegationCallError::Refused)?;
    Ok(json!({
        "childAssignmentId": child.assignment_id.as_str(),
        "replyThreadId": child.reply_thread_id.as_str(),
        "state": child.state,
    }))
}

struct DelegationInput {
    teammate_id: ChatParticipantId,
    objective: String,
    expected_result: String,
    folders: Option<Vec<DelegatedFolder>>,
    client_request_id: String,
}

fn parse_input(arguments: &Map<String, Value>) -> ChatResult<DelegationInput> {
    let teammate_id = ChatParticipantId::new(required_string(arguments, "teammateId", 1024)?)
        .map_err(|_| generic_denial_error())?;
    let objective = required_text(arguments, "objective", MAX_DELEGATION_OBJECTIVE_CHARS)?;
    let expected_result = required_text(
        arguments,
        "expectedResult",
        MAX_DELEGATION_EXPECTED_RESULT_CHARS,
    )?;
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
    let folders = match arguments.get("folders") {
        None | Some(Value::Null) => None,
        Some(Value::Array(entries)) => Some(parse_folders(entries)?),
        Some(_) => return Err(generic_denial_error()),
    };
    Ok(DelegationInput {
        teammate_id,
        objective,
        expected_result,
        folders,
        client_request_id: client_request_id.to_owned(),
    })
}

/// A trimmed, non-empty text of at most `maximum_chars` characters. Line breaks and tabs are
/// allowed, since an objective is prose; other control characters are not.
fn required_text(
    arguments: &Map<String, Value>,
    name: &str,
    maximum_chars: usize,
) -> ChatResult<String> {
    let value = arguments
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.chars().count() <= maximum_chars)
        .filter(|value| {
            !value
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
        })
        .ok_or_else(generic_denial_error)?;
    Ok(value.to_owned())
}

fn parse_folders(entries: &[Value]) -> ChatResult<Vec<DelegatedFolder>> {
    let mut seen = BTreeSet::new();
    entries
        .iter()
        .map(|entry| {
            let entry = entry.as_object().ok_or_else(generic_denial_error)?;
            if entry.len() != 2 {
                return Err(generic_denial_error());
            }
            let working_folder_id =
                ProjectWorkingFolderId::new(required_string(entry, "workingFolderId", 1024)?)
                    .map_err(|_| generic_denial_error())?;
            let capability = match required_string(entry, "capability", 16)? {
                "read" => ChatFolderCapability::Read,
                "edit" => ChatFolderCapability::Edit,
                "execute" => ChatFolderCapability::Execute,
                "publish" => ChatFolderCapability::Publish,
                _ => return Err(generic_denial_error()),
            };
            if !seen.insert(working_folder_id.as_str().to_owned()) {
                return Err(generic_denial_error());
            }
            Ok(DelegatedFolder {
                working_folder_id,
                capability,
            })
        })
        .collect()
}

/// The folders the child may hold: the requested ones when each is within the run's own folder
/// sources, else every folder source of the run at its rank.
fn folder_ceiling(
    context: &HostToolContext<'_>,
    requested: Option<&[DelegatedFolder]>,
) -> Result<Vec<DelegatedFolder>, DelegationCallError> {
    let held = context
        .scope
        .folder_sources
        .iter()
        .filter(|source| source.capability.rank() > ChatFolderCapability::None.rank())
        .map(|source| DelegatedFolder {
            working_folder_id: source.working_folder_id.clone(),
            capability: source.capability,
        })
        .collect::<Vec<_>>();
    let Some(requested) = requested else {
        return Ok(held);
    };
    requested
        .iter()
        .map(|folder| {
            held.iter()
                .find(|held| held.working_folder_id == folder.working_folder_id)
                .filter(|held| held.capability.rank() >= folder.capability.rank())
                .map(|_| folder.clone())
                .ok_or(DelegationCallError::Refused(
                    DelegationRefusal::AuthorityExceeded,
                ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(value: Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn delegation_input_keeps_multiline_prose_and_refuses_duplicate_or_unknown_folders() {
        let input = parse_input(&arguments(json!({
            "teammateId": "participant:reviewer",
            "objective": "  Review the parser.\nList every bug.  ",
            "expectedResult": "A list\tof bugs",
            "clientRequestId": "request-1",
        })))
        .unwrap();
        assert_eq!(input.objective, "Review the parser.\nList every bug.");
        assert_eq!(input.expected_result, "A list\tof bugs");
        assert!(input.folders.is_none());

        let duplicate = parse_input(&arguments(json!({
            "teammateId": "participant:reviewer",
            "objective": "Review",
            "expectedResult": "Bugs",
            "clientRequestId": "request-1",
            "folders": [
                { "workingFolderId": "workspace-1", "capability": "read" },
                { "workingFolderId": "workspace-1", "capability": "edit" },
            ],
        })));
        assert!(duplicate.is_err());
        for capability in ["none", "owner"] {
            let unknown = parse_input(&arguments(json!({
                "teammateId": "participant:reviewer",
                "objective": "Review",
                "expectedResult": "Bugs",
                "clientRequestId": "request-1",
                "folders": [{ "workingFolderId": "workspace-1", "capability": capability }],
            })));
            assert!(unknown.is_err(), "{capability}");
        }
        let blank = parse_input(&arguments(json!({
            "teammateId": "participant:reviewer",
            "objective": "   ",
            "expectedResult": "Bugs",
            "clientRequestId": "request-1",
        })));
        assert!(blank.is_err());
        let control = parse_input(&arguments(json!({
            "teammateId": "participant:reviewer",
            "objective": "Review\u{7}",
            "expectedResult": "Bugs",
            "clientRequestId": "request-1",
        })));
        assert!(control.is_err());
    }

    #[test]
    fn delegation_refusals_serialize_as_stable_codes() {
        assert_eq!(
            DelegationRefusal::AuthorityExceeded.into_error().message,
            "delegation_authority_exceeded"
        );
        assert_eq!(
            DelegationRefusal::FanOutExceeded.code(),
            "delegation_fan_out_exceeded"
        );
    }
}
