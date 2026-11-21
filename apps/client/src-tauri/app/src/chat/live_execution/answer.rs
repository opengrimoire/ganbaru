//! The phone side of approvals: answers an approval that work another device runs holds open,
//! sent to that device over the pinned LAN connections sync uses.

use super::follow::remote_executing_device;
use crate::sync::chat_live::{ChatApprovalRequest, resolve_chat_approval};
use crate::vault::handoff::protocol::{
    CHAT_APPROVAL_REFUSED_CODE, CHAT_APPROVAL_STALE_CODE, CHAT_LIVE_NOT_HERE_CODE,
    ChatRemoteApprovalDecision, ControlMessage,
};
use ganbaru_chat_contracts::models::ChatResult;
use serde::Serialize;

/// What became of an approval answer sent to the device that runs the work.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatRemoteApprovalOutcome {
    /// The executing device resolved the approval with the answer.
    Resolved,
    /// The approval is no longer open there: it was answered, withdrawn, or the run moved on.
    Stale,
    /// The executing device refused the answer, such as a decision the request no longer offers,
    /// so only that device can answer it.
    Refused,
    /// No other device runs this work now, so there is nothing to answer here.
    NotServed,
    /// The executing device could not be reached or could not take the answer now.
    Unreachable,
}

/// Answers the approval `request_id` of the work `assignment_id` on the device that runs it.
/// `command_id` names this answer, so sending it again never resolves twice.
#[tauri::command]
pub async fn chat_resolve_remote_approval(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: String,
    request_id: String,
    decision: ChatRemoteApprovalDecision,
    provider_option_id: String,
    command_id: String,
) -> ChatResult<ChatRemoteApprovalOutcome> {
    let Some(executing_device_id) = remote_executing_device(&app, db_url, &assignment_id).await?
    else {
        return Ok(ChatRemoteApprovalOutcome::NotServed);
    };
    let response = resolve_chat_approval(
        &app,
        ChatApprovalRequest {
            executing_device_id: &executing_device_id,
            assignment_id: &assignment_id,
            request_id: &request_id,
            decision,
            provider_option_id: &provider_option_id,
            command_id: &command_id,
        },
    )
    .await;
    Ok(interpret(response))
}

/// What the answer of the executing device means for the approval sent to it. `Ok(None)` means
/// no pinned desktop is known for that device now.
fn interpret(response: Result<Option<ControlMessage>, String>) -> ChatRemoteApprovalOutcome {
    match response {
        Ok(Some(ControlMessage::ChatApprovalResolved)) => ChatRemoteApprovalOutcome::Resolved,
        Ok(Some(ControlMessage::Error { code, .. })) if code == CHAT_APPROVAL_STALE_CODE => {
            ChatRemoteApprovalOutcome::Stale
        }
        Ok(Some(ControlMessage::Error { code, .. })) if code == CHAT_LIVE_NOT_HERE_CODE => {
            ChatRemoteApprovalOutcome::NotServed
        }
        Ok(Some(ControlMessage::Error { code, .. })) if code == CHAT_APPROVAL_REFUSED_CODE => {
            ChatRemoteApprovalOutcome::Refused
        }
        Ok(Some(ControlMessage::Error { code, .. })) => {
            eprintln!("chat: the executing device refused an approval answer with {code}");
            ChatRemoteApprovalOutcome::Unreachable
        }
        Ok(Some(_)) => {
            eprintln!("chat: the executing device answered an approval with another message");
            ChatRemoteApprovalOutcome::Unreachable
        }
        Ok(None) => ChatRemoteApprovalOutcome::Unreachable,
        Err(error) => {
            eprintln!("chat: send an approval answer: {error}");
            ChatRemoteApprovalOutcome::Unreachable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::handoff::protocol::CHAT_LIVE_UNAVAILABLE_CODE;

    fn error(code: &str) -> ControlMessage {
        ControlMessage::Error {
            code: code.to_string(),
            message: "refused".to_string(),
            retryable: false,
        }
    }

    #[test]
    fn each_desktop_answer_becomes_what_the_phone_shows() {
        let cases = [
            (
                Ok(Some(ControlMessage::ChatApprovalResolved)),
                ChatRemoteApprovalOutcome::Resolved,
            ),
            (
                Ok(Some(error(CHAT_APPROVAL_STALE_CODE))),
                ChatRemoteApprovalOutcome::Stale,
            ),
            (
                Ok(Some(error(CHAT_LIVE_NOT_HERE_CODE))),
                ChatRemoteApprovalOutcome::NotServed,
            ),
            (
                Ok(Some(error(CHAT_APPROVAL_REFUSED_CODE))),
                ChatRemoteApprovalOutcome::Refused,
            ),
            (
                Ok(Some(error(CHAT_LIVE_UNAVAILABLE_CODE))),
                ChatRemoteApprovalOutcome::Unreachable,
            ),
            (
                Ok(Some(error("membership_revoked"))),
                ChatRemoteApprovalOutcome::Unreachable,
            ),
            (
                Ok(Some(ControlMessage::ChatLive {
                    payload: "{}".to_string(),
                    change_cursor: 0,
                    has_more: false,
                    reset_required: false,
                    settled: false,
                })),
                ChatRemoteApprovalOutcome::Unreachable,
            ),
            (Ok(None), ChatRemoteApprovalOutcome::Unreachable),
            (
                Err("connection refused".to_string()),
                ChatRemoteApprovalOutcome::Unreachable,
            ),
        ];
        for (response, outcome) in cases {
            assert_eq!(interpret(response), outcome);
        }
    }

    #[test]
    fn outcomes_serialize_as_the_kinds_the_frontend_reads() {
        assert_eq!(
            serde_json::to_value(ChatRemoteApprovalOutcome::NotServed).expect("encode"),
            serde_json::json!({ "kind": "notServed" })
        );
    }
}
