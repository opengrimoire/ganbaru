//! Codex native rollback: rewinds the app-server thread to the last turn a checkpoint keeps.

use super::*;

/// The provider reference schema the normalizer writes and a rollback cursor carries.
const CODEX_ROLLBACK_CURSOR_SCHEMA_VERSION: u32 = 1;
/// Turns requested per `thread/turns/list` page.
const ROLLBACK_TURN_PAGE_LIMIT: u32 = 100;
/// Pages read before a rollback gives up looking for the kept turn.
const MAX_ROLLBACK_TURN_PAGES: usize = 64;
/// Longest Codex turn id a rollback cursor may carry.
const MAX_CODEX_TURN_ID_BYTES: usize = 512;

/// What a rollback has to do to make the kept turn the newest one in the native thread.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum CodexRewindPlan {
    /// The kept turn is already the newest turn.
    AlreadyLatest,
    /// `thread/revert` drops this turn and every later one.
    RevertBefore(String),
}

/// The Codex turn id a checkpoint restore keeps, read from the stored provider reference.
pub(super) fn rollback_turn_id(cursor: Option<&VersionedJson>) -> ChatResult<String> {
    let cursor = cursor.ok_or_else(|| {
        ChatError::validation(
            "providerCursor",
            "Codex rollback requires the native turn cursor",
        )
    })?;
    if cursor.schema_version != CODEX_ROLLBACK_CURSOR_SCHEMA_VERSION {
        return Err(ChatError::validation(
            "providerCursor.schemaVersion",
            "Codex rollback cursor schema is unsupported",
        ));
    }
    let turn_id = cursor
        .value
        .get("turnId")
        .and_then(Value::as_str)
        .filter(|turn_id| {
            !turn_id.is_empty()
                && turn_id.len() <= MAX_CODEX_TURN_ID_BYTES
                && !turn_id.chars().any(char::is_control)
        })
        .ok_or_else(|| {
            ChatError::validation("providerCursor.turnId", "Codex rollback cursor is invalid")
        })?;
    Ok(turn_id.to_string())
}

/// Pages the thread's turns from newest to oldest and returns how to make `kept_turn_id` the
/// newest turn: nothing when it already is, otherwise a revert before the turn right after it.
pub(super) async fn plan_codex_rewind(
    client: &super::super::transport::CodexRpcClient,
    provider_thread_id: &str,
    kept_turn_id: &str,
    context: &DriverOperationContext,
) -> ChatResult<CodexRewindPlan> {
    let mut cursor: Option<String> = None;
    let mut seen_cursors = BTreeSet::new();
    let mut newer_turn_id: Option<String> = None;
    for _ in 0..MAX_ROLLBACK_TURN_PAGES {
        let response = client
            .request(
                "thread/turns/list",
                json!({
                    "threadId": provider_thread_id,
                    "cursor": cursor,
                    "limit": ROLLBACK_TURN_PAGE_LIMIT,
                    "sortDirection": "desc",
                    "itemsView": "notLoaded",
                }),
                context,
            )
            .await
            .map_err(|error| error.to_chat_error("thread turns"))?;
        let turns = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| rollback_protocol_error("Codex returned an invalid turn page"))?;
        for turn in turns {
            let turn_id = turn
                .get("id")
                .and_then(Value::as_str)
                .filter(|turn_id| !turn_id.is_empty())
                .ok_or_else(|| protocol_identifier_error("turn"))?;
            if turn_id == kept_turn_id {
                return Ok(match newer_turn_id {
                    Some(newer) => CodexRewindPlan::RevertBefore(newer),
                    None => CodexRewindPlan::AlreadyLatest,
                });
            }
            newer_turn_id = Some(turn_id.to_string());
        }
        let next = response
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|next| !next.is_empty());
        let Some(next) = next else {
            return Err(ChatError::new(
                ChatErrorCode::NotFound,
                "Codex thread no longer has the checkpoint turn",
                false,
            ));
        };
        if !seen_cursors.insert(next.to_string()) {
            return Err(rollback_protocol_error("Codex repeated a turn page cursor"));
        }
        cursor = Some(next.to_string());
    }
    Err(rollback_protocol_error(
        "Codex thread has more turns than a rollback reads",
    ))
}

/// Applies a rewind plan through `thread/revert`.
pub(super) async fn apply_codex_rewind(
    client: &super::super::transport::CodexRpcClient,
    provider_thread_id: &str,
    plan: &CodexRewindPlan,
    context: &DriverOperationContext,
) -> ChatResult<&'static str> {
    let CodexRewindPlan::RevertBefore(before_turn_id) = plan else {
        return Ok("Codex thread already ends at the checkpoint turn");
    };
    client
        .request(
            "thread/revert",
            json!({ "threadId": provider_thread_id, "beforeTurnId": before_turn_id }),
            context,
        )
        .await
        .map_err(|error| error.to_chat_error("thread revert"))?;
    Ok("Codex native thread reverted")
}

fn rollback_protocol_error(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::Protocol, message, false)
}
