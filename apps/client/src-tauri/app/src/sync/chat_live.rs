//! Dials the desktop that executes a Chat work assignment for its live execution, over the
//! pinned connections sync already uses: the coordinator handoff connection, or a member
//! listener pinned to the certificate the selected log enrolled. Never a server or a relay.

use super::host::{AppHost, SyncHost};
use crate::vault::handoff::protocol::{ControlMessage, PROTOCOL_VERSION};
use ganbaru_sync_replica::service::peers::{ChatLiveTarget, chat_live_target_of};
use tauri::{AppHandle, Runtime};

/// The page of the live execution a device asks for.
pub(crate) struct ChatLiveRequest<'a> {
    pub executing_device_id: &'a str,
    pub assignment_id: &'a str,
    pub after_change: Option<u64>,
    pub wait: bool,
}

/// Sends one live read to the device that executes the assignment. Returns `None` when no
/// pinned, admitted desktop is known for that device in the active vault, so nothing is dialed.
pub(crate) async fn read_chat_live<R: Runtime>(
    app: &AppHandle<R>,
    request: ChatLiveRequest<'_>,
) -> Result<Option<ControlMessage>, String> {
    let host = AppHost::of(app);
    let Some((vault_id, target)) = chat_live_target_of(&host, request.executing_device_id).await?
    else {
        return Ok(None);
    };
    let manager = host.pairing();
    let message = ControlMessage::ChatLiveRead {
        protocol_version: PROTOCOL_VERSION,
        vault_id,
        device_id: manager.device_id()?,
        assignment_id: request.assignment_id.to_string(),
        after_change: request.after_change,
        wait: request.wait,
    };
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
