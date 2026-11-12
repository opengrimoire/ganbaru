//! ACP inbound queue and the `session/load` replay gate.
//!
//! Loading a session makes the agent replay its history as `session/update` notifications. The
//! provider already restores its own context and Ganbaru's timeline is the history the user sees,
//! so the gate drains the replay while the load is in flight and until it goes idle, and the
//! router starts on the same queue afterwards.

use super::*;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::time::Instant;

/// Quiet period after the load response that ends the replay.
const ACP_LOAD_REPLAY_IDLE: Duration = Duration::from_secs(1);
/// Non-replay messages kept for the router while a session loads.
const MAX_RETAINED_LOAD_MESSAGES: usize = 256;
/// JSON-RPC error answered to requests other than permissions while a session loads.
const LOAD_REQUEST_ERROR_CODE: i64 = -32601;
const LOAD_REQUEST_ERROR_MESSAGE: &str = "Method not supported by Ganbaru";

/// Inbound ACP messages in arrival order, with messages the load gate kept served first.
pub struct AcpInboundQueue {
    receiver: mpsc::Receiver<AcpInboundMessage>,
    retained: VecDeque<AcpInboundMessage>,
    replay_idle: Duration,
}

impl AcpInboundQueue {
    pub fn new(receiver: mpsc::Receiver<AcpInboundMessage>) -> Self {
        Self {
            receiver,
            retained: VecDeque::new(),
            replay_idle: ACP_LOAD_REPLAY_IDLE,
        }
    }

    /// Sets the quiet period that ends a load replay.
    #[cfg(test)]
    pub fn with_replay_idle(mut self, replay_idle: Duration) -> Self {
        self.replay_idle = replay_idle;
        self
    }

    /// Returns the next message, or None once the transport dropped its sender.
    pub async fn recv(&mut self) -> Option<AcpInboundMessage> {
        match self.retained.pop_front() {
            Some(message) => Some(message),
            None => self.receiver.recv().await,
        }
    }

    /// Sends `session/load` and drains its replay until the response and a quiet period.
    ///
    /// Replayed updates for the loading session are dropped, except the latest command list,
    /// which the load response does not carry. Mode and configuration updates that arrive after
    /// the response are folded into the returned response instead. Permission requests are
    /// cancelled and other requests refused; every other message is kept for the router. The quiet period is measured from
    /// the later of the response and the last replayed update, never runs past the operation
    /// deadline, and ends with `Cancelled` when the operation is cancelled.
    pub async fn load_session(
        &mut self,
        client: &AcpRpcClient,
        session_id: &str,
        params: Value,
        context: &DriverOperationContext,
    ) -> Result<Value, AcpRpcFailure> {
        let load = client.request("session/load", params, context);
        tokio::pin!(load);
        let mut receiving = true;
        let mut response = loop {
            tokio::select! {
                response = &mut load => break response?,
                message = self.receiver.recv(), if receiving => match message {
                    Some(message) => {
                        self.screen(client, session_id, message, None).await?;
                    }
                    None => receiving = false,
                },
            }
        };
        let deadline = Instant::from_std(context.deadline);
        let mut quiet_until = (Instant::now() + self.replay_idle).min(deadline);
        while receiving && !self.closed() {
            let message = tokio::select! {
                biased;
                () = context.cancellation.cancelled() => return Err(AcpRpcFailure::Cancelled),
                message = tokio::time::timeout_at(quiet_until, self.receiver.recv()) => message,
            };
            match message {
                Err(_) | Ok(None) => break,
                Ok(Some(message)) => {
                    if self
                        .screen(client, session_id, message, Some(&mut response))
                        .await?
                        == LoadMessage::Replay
                    {
                        quiet_until = (Instant::now() + self.replay_idle).min(deadline);
                    }
                }
            }
        }
        Ok(response)
    }

    fn closed(&self) -> bool {
        self.retained
            .iter()
            .any(|message| matches!(message, AcpInboundMessage::Closed))
    }

    async fn screen(
        &mut self,
        client: &AcpRpcClient,
        session_id: &str,
        message: AcpInboundMessage,
        response: Option<&mut Value>,
    ) -> Result<LoadMessage, AcpRpcFailure> {
        match message {
            AcpInboundMessage::Notification { method, params }
                if method == "session/update" && update_session_id(&params) == Some(session_id) =>
            {
                match (update_kind(&params), response) {
                    (Some("available_commands_update"), _) => {
                        self.retained.retain(|retained| !is_command_list(retained));
                        self.retain(AcpInboundMessage::Notification { method, params })?;
                    }
                    (Some(kind), Some(response)) => fold_state_update(response, kind, &params),
                    _ => {}
                }
                Ok(LoadMessage::Replay)
            }
            AcpInboundMessage::Request { id, method, .. } => {
                if method == "session/request_permission" {
                    client
                        .respond(id, json!({ "outcome": { "outcome": "cancelled" } }))
                        .await?;
                } else {
                    client
                        .respond_error(id, LOAD_REQUEST_ERROR_CODE, LOAD_REQUEST_ERROR_MESSAGE)
                        .await?;
                }
                Ok(LoadMessage::Other)
            }
            message => {
                self.retain(message)?;
                Ok(LoadMessage::Other)
            }
        }
    }

    fn retain(&mut self, message: AcpInboundMessage) -> Result<(), AcpRpcFailure> {
        if self.retained.len() >= MAX_RETAINED_LOAD_MESSAGES {
            return Err(AcpRpcFailure::Malformed(
                "too many messages while loading the session".to_string(),
            ));
        }
        self.retained.push_back(message);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoadMessage {
    Replay,
    Other,
}

fn update_session_id(params: &Value) -> Option<&str> {
    params.get("sessionId").and_then(Value::as_str)
}

fn update_kind(params: &Value) -> Option<&str> {
    params
        .get("update")
        .and_then(|update| update.get("sessionUpdate"))
        .and_then(Value::as_str)
}

/// Folds a mode or configuration update that arrived after the load response into it.
///
/// Requested modes and model settings are applied over the parsed response, so it must hold
/// the agent's latest state; a stale value would skip a needed change or let a later router
/// update overwrite the applied one. A mode update sets the advertised current mode, or the
/// mode configuration option when the agent advertises modes that way.
fn fold_state_update(response: &mut Value, kind: &str, params: &Value) {
    let Some(update) = params.get("update") else {
        return;
    };
    match kind {
        "config_option_update" => {
            if let (Some(options), Some(response)) =
                (update.get("configOptions"), response.as_object_mut())
            {
                response.insert("configOptions".to_string(), options.clone());
            }
        }
        "current_mode_update" => {
            let Some(mode) = update
                .get("currentModeId")
                .or_else(|| update.get("modeId"))
                .filter(|mode| mode.is_string())
                .cloned()
            else {
                return;
            };
            if let Some(modes) = response.get_mut("modes").and_then(Value::as_object_mut) {
                modes.insert("currentModeId".to_string(), mode);
            } else if let Some(option) = response
                .get_mut("configOptions")
                .and_then(Value::as_array_mut)
                .and_then(|options| options.iter_mut().find(|option| is_mode_option(option)))
                .and_then(Value::as_object_mut)
            {
                option.insert("currentValue".to_string(), mode);
            }
        }
        _ => {}
    }
}

fn is_mode_option(option: &Value) -> bool {
    option
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| id.eq_ignore_ascii_case("mode"))
        || option.get("category").and_then(Value::as_str) == Some("mode")
}

fn is_command_list(message: &AcpInboundMessage) -> bool {
    matches!(
        message,
        AcpInboundMessage::Notification { method, params }
            if method == "session/update"
                && update_kind(params) == Some("available_commands_update")
    )
}
