//! Provider-turn application services shared by direct and organizational Chat.

use super::send_commands::{SendChatTurnCommand, SendChatTurnResult, SteerChatTurnCommand};
use ganbaru_chat::agent_runs::TurnOrigin;
use ganbaru_chat_contracts::models::{ChatResult, DriverOperationReceipt};

pub(crate) struct SendChatTurnInvocation {
    pub command: SendChatTurnCommand,
    pub origin: TurnOrigin,
}

pub(crate) async fn send_turn(
    app: tauri::AppHandle,
    db_url: String,
    invocation: SendChatTurnInvocation,
) -> ChatResult<SendChatTurnResult> {
    super::send::send_turn(app, db_url, invocation.command, invocation.origin).await
}

pub(crate) async fn steer_turn(
    app: tauri::AppHandle,
    db_url: String,
    command: SteerChatTurnCommand,
) -> ChatResult<DriverOperationReceipt> {
    super::send::steer_turn(app, db_url, command).await
}
