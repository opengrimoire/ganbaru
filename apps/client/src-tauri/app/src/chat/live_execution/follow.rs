//! The phone side: reads the live execution of work another device runs, while its reply thread
//! is open, from that device over the pinned LAN connections sync uses.

use crate::chat::channels::chat_pool;
use crate::sync::chat_live::{ChatLiveRequest, read_chat_live};
use crate::vault::handoff::protocol::{CHAT_LIVE_NOT_HERE_CODE, ControlMessage};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatLiveExecutionRead, ChatResult};
use serde::Serialize;

/// What this device can show of the live execution of work another device runs.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatLiveFollowRead {
    /// One page of the execution from the device that runs it.
    Page { read: Box<ChatLiveExecutionRead> },
    /// The executing device cannot be reached now, so the work is shown from synced rows and
    /// read again later.
    Unreachable,
    /// No other device serves this execution: the work runs nowhere yet, runs here, or the
    /// device it names does not run it.
    NotServed,
}

/// Reads one page of the live execution of `assignment_id` after `after_change` from the
/// desktop that executes it. With `wait`, the desktop holds the answer until the execution
/// changes or a short bound passes.
#[tauri::command]
pub async fn chat_read_live_execution(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: String,
    after_change: Option<u64>,
    wait: bool,
) -> ChatResult<ChatLiveFollowRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let executing: Option<Option<String>> =
        sqlx::query_scalar("SELECT executing_device_id FROM chat_work_assignments WHERE id = ?")
            .bind(&assignment_id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| {
                ChatError::new(ChatErrorCode::Persistence, "read the work assignment", true)
            })?;
    let Some(Some(executing_device_id)) = executing else {
        return Ok(ChatLiveFollowRead::NotServed);
    };
    let own_device_id = crate::sync::devices::stable_device_id(&app).map_err(|_| {
        ChatError::new(ChatErrorCode::Internal, "read this device's identity", true)
    })?;
    if executing_device_id == own_device_id {
        return Ok(ChatLiveFollowRead::NotServed);
    }
    let response = read_chat_live(
        &app,
        ChatLiveRequest {
            executing_device_id: &executing_device_id,
            assignment_id: &assignment_id,
            after_change,
            wait,
        },
    )
    .await;
    Ok(interpret(response, &assignment_id))
}

/// What the answer of the executing device means for the reader of `assignment_id`. `Ok(None)`
/// means no pinned desktop is known for that device now.
fn interpret(
    response: Result<Option<ControlMessage>, String>,
    assignment_id: &str,
) -> ChatLiveFollowRead {
    match response {
        Ok(Some(ControlMessage::Error { code, .. })) if code == CHAT_LIVE_NOT_HERE_CODE => {
            ChatLiveFollowRead::NotServed
        }
        Ok(Some(message @ ControlMessage::ChatLive { .. })) => page(message, assignment_id)
            .unwrap_or_else(|error| {
                eprintln!("chat: refused a live execution page: {error}");
                ChatLiveFollowRead::Unreachable
            }),
        Ok(Some(ControlMessage::Error { code, .. })) => {
            eprintln!("chat: the executing device refused a live read with {code}");
            ChatLiveFollowRead::Unreachable
        }
        Ok(Some(_)) => {
            eprintln!("chat: the executing device answered a live read with another message");
            ChatLiveFollowRead::Unreachable
        }
        Ok(None) => ChatLiveFollowRead::Unreachable,
        Err(error) => {
            eprintln!("chat: read the live execution: {error}");
            ChatLiveFollowRead::Unreachable
        }
    }
}

/// The page `message` carries, once its bounds hold and its payload is the execution of
/// `assignment_id` at the cursor and flags the frame states.
fn page(message: ControlMessage, assignment_id: &str) -> Result<ChatLiveFollowRead, String> {
    message.validate()?;
    let ControlMessage::ChatLive {
        payload,
        change_cursor,
        has_more,
        reset_required,
        settled,
    } = message
    else {
        return Err("not a live execution page".to_string());
    };
    let read: ChatLiveExecutionRead =
        serde_json::from_str(&payload).map_err(|error| format!("decode the page: {error}"))?;
    let matches = read.assignment_id == assignment_id
        && read.change_cursor == change_cursor
        && read.has_more == has_more
        && read.reset_required == reset_required
        && read.settled == settled;
    if !matches {
        return Err("the page does not match the work or the frame".to_string());
    }
    Ok(ChatLiveFollowRead::Page {
        read: Box::new(read),
    })
}

#[cfg(test)]
mod tests;
