//! Dials the desktop that executes a Chat work assignment for its live execution, or with an
//! answer to an approval that work holds open, over the pinned connections sync already uses:
//! the coordinator handoff connection, or a member listener pinned to the certificate the
//! selected log enrolled. Never a server or a relay.

use super::host::{AppHost, SyncHost};
use crate::vault::handoff::protocol::{
    ChatRemoteApprovalDecision, ControlMessage, PROTOCOL_VERSION,
};
use ganbaru_sync_replica::service::peers::{ChatLiveTarget, chat_live_target_of};
use tauri::{AppHandle, Runtime};

/// The page of the live execution a device asks for.
pub(crate) struct ChatLiveRequest<'a> {
    pub executing_device_id: &'a str,
    pub assignment_id: &'a str,
    pub after_change: Option<u64>,
    pub wait: bool,
}

/// The answer a device gives to an approval the executing desktop holds open.
pub(crate) struct ChatApprovalRequest<'a> {
    pub executing_device_id: &'a str,
    pub assignment_id: &'a str,
    pub request_id: &'a str,
    pub decision: ChatRemoteApprovalDecision,
    pub provider_option_id: &'a str,
    pub command_id: &'a str,
}

/// Sends one live read to the device that executes the assignment. Returns `None` when no
/// pinned, admitted desktop is known for that device in the active vault, so nothing is dialed.
pub(crate) async fn read_chat_live<R: Runtime>(
    app: &AppHandle<R>,
    request: ChatLiveRequest<'_>,
) -> Result<Option<ControlMessage>, String> {
    send_to_executing_device(app, request.executing_device_id, |vault_id, device_id| {
        ControlMessage::ChatLiveRead {
            protocol_version: PROTOCOL_VERSION,
            vault_id,
            device_id,
            assignment_id: request.assignment_id.to_string(),
            after_change: request.after_change,
            wait: request.wait,
        }
    })
    .await
}

/// Sends one approval answer to the device that executes the assignment, with the same `None`
/// as [`read_chat_live`] when no pinned, admitted desktop is known for it.
pub(crate) async fn resolve_chat_approval<R: Runtime>(
    app: &AppHandle<R>,
    request: ChatApprovalRequest<'_>,
) -> Result<Option<ControlMessage>, String> {
    send_to_executing_device(app, request.executing_device_id, |vault_id, device_id| {
        ControlMessage::ChatApprovalResolve {
            protocol_version: PROTOCOL_VERSION,
            vault_id,
            device_id,
            assignment_id: request.assignment_id.to_string(),
            request_id: request.request_id.to_string(),
            decision: request.decision,
            provider_option_id: request.provider_option_id.to_string(),
            command_id: request.command_id.to_string(),
        }
    })
    .await
}

/// Sends the message `build` makes from the active vault and this device to the pinned desktop
/// `executing_device_id` names, and returns its answer.
async fn send_to_executing_device<R: Runtime>(
    app: &AppHandle<R>,
    executing_device_id: &str,
    build: impl FnOnce(String, String) -> ControlMessage,
) -> Result<Option<ControlMessage>, String> {
    let host = AppHost::of(app);
    let Some((vault_id, target)) = chat_live_target_of(&host, executing_device_id).await? else {
        return Ok(None);
    };
    let manager = host.pairing();
    let message = build(vault_id, manager.device_id()?);
    message.validate()?;
    let response = match target {
        ChatLiveTarget::Coordinator => {
            crate::vault::handoff::transport::sync_request(&manager, message).await?
        }
        ChatLiveTarget::Member {
            endpoints,
            fingerprint,
        } => {
            crate::vault::handoff::transport::member_sync_request(
                &manager,
                &endpoints,
                &fingerprint,
                message,
            )
            .await?
        }
    };
    Ok(Some(response))
}
