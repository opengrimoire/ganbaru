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
    /// which the load response does not carry. Permission requests are cancelled and other
    /// requests refused; every other message is kept for the router. The quiet period is
    /// measured from the later of the response and the last replayed update and never runs past
    /// the operation deadline.
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
        let response = loop {
            tokio::select! {
                response = &mut load => break response?,
                message = self.receiver.recv(), if receiving => match message {
                    Some(message) => {
                        self.screen(client, session_id, message).await?;
                    }
                    None => receiving = false,
                },
            }
        };
        let deadline = Instant::from_std(context.deadline);
        let mut quiet_until = (Instant::now() + self.replay_idle).min(deadline);
        while receiving && !self.closed() {
            match tokio::time::timeout_at(quiet_until, self.receiver.recv()).await {
                Err(_) | Ok(None) => break,
                Ok(Some(message)) => {
                    if self.screen(client, session_id, message).await? == LoadMessage::Replay {
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
    ) -> Result<LoadMessage, AcpRpcFailure> {
        match message {
            AcpInboundMessage::Notification { method, params }
                if method == "session/update" && update_session_id(&params) == Some(session_id) =>
            {
                if update_kind(&params) == Some("available_commands_update") {
                    self.retained.retain(|retained| !is_command_list(retained));
                    self.retain(AcpInboundMessage::Notification { method, params })?;
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

fn is_command_list(message: &AcpInboundMessage) -> bool {
    matches!(
        message,
        AcpInboundMessage::Notification { method, params }
            if method == "session/update"
                && update_kind(params) == Some("available_commands_update")
    )
}
