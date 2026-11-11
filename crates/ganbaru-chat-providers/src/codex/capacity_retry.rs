//! Bounded retry of a Codex turn that failed because the model was at capacity.
//!
//! Codex can end a turn with `serverOverloaded` after its own retries. The session then waits
//! 2, 4, and 8 seconds and continues the same thread with an empty `turn/start`, so the user
//! message Codex already recorded is not sent twice. The chat turn stays active across retries;
//! only the last failure, an interrupt, or the session ending settles it.

use super::normalizer::{CodexEventNormalizer, CodexRouteState};
use super::protocol::{TurnStartResponse, decode_response};
use super::transport::CodexRpcClient;
use crate::events::*;
use crate::models::*;
use crate::{DriverCancellation, DriverOperationContext, ProviderEventSink};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Retries of one turn after a capacity failure. t3code allows 5; 3 bounds the spend.
pub(super) const CODEX_CAPACITY_MAX_RETRIES: u32 = 3;
/// Wait before the first retry; each later retry doubles it.
const FIRST_RETRY_BACKOFF: Duration = Duration::from_secs(2);
/// Deadline of the `turn/start` request a retry sends.
const RETRY_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// The retry armed for the active `turn/start` turn.
#[derive(Clone, Debug)]
pub struct CodexCapacityRetry {
    params: Value,
    attempts: u32,
    cancellation: DriverCancellation,
}

impl CodexCapacityRetry {
    /// Arms a retry for a turn sent with `turn_start`, keeping every setting but the input.
    pub(super) fn arm(turn_start: &Value) -> Self {
        let mut params = turn_start.clone();
        if let Some(object) = params.as_object_mut() {
            object.insert("input".to_string(), Value::Array(Vec::new()));
        }
        Self {
            params,
            attempts: 0,
            cancellation: DriverCancellation::default(),
        }
    }

    /// Whether another capacity failure may still retry.
    pub(super) fn available(&self) -> bool {
        self.attempts < CODEX_CAPACITY_MAX_RETRIES && !self.cancellation.is_cancelled()
    }

    /// Whether a retry is waiting out its backoff or starting, with no provider turn running.
    pub(super) fn retrying(&self) -> bool {
        self.attempts > 0 && !self.cancellation.is_cancelled()
    }

    /// Stops a pending backoff or retry request.
    pub(super) fn cancel(&self) {
        self.cancellation.cancel();
    }
}

/// The wait before retry number `attempt`, counted from 1.
fn backoff(attempt: u32) -> Duration {
    FIRST_RETRY_BACKOFF * 2_u32.saturating_pow(attempt.saturating_sub(1))
}

/// What a retry task needs from the live session.
#[derive(Clone)]
pub(super) struct CodexRetryResources {
    pub client: CodexRpcClient,
    pub route: Arc<Mutex<CodexRouteState>>,
    pub normalizer: Arc<CodexEventNormalizer>,
    pub sink: Arc<dyn ProviderEventSink>,
    pub expected_shutdown: Arc<AtomicBool>,
}

/// Starts the next retry when `params` is a capacity failure of the active turn and retries
/// remain. Returns true when the failure was absorbed, so the caller does not settle the turn.
pub(super) async fn begin_capacity_retry(
    resources: &CodexRetryResources,
    params: &Value,
) -> ChatResult<bool> {
    let Some(turn) = params.get("turn").and_then(Value::as_object) else {
        return Ok(false);
    };
    let failed_on_capacity = turn.get("status").and_then(Value::as_str) == Some("failed")
        && turn
            .get("error")
            .and_then(Value::as_object)
            .is_some_and(super::normalizer::CodexStopReason::is_capacity);
    if !failed_on_capacity {
        return Ok(false);
    }
    let (warning, retry, attempt, chat_turn) = {
        let mut state = resources.route.lock().map_err(|_| route_state_error())?;
        let other_thread = params
            .get("threadId")
            .and_then(Value::as_str)
            .zip(state.provider_thread_id.as_deref())
            .is_some_and(|(actual, expected)| actual != expected);
        if other_thread {
            return Ok(false);
        }
        let Some(chat_turn) = state.active_chat_turn_id.clone() else {
            return Ok(false);
        };
        let Some(retry) = state
            .capacity_retry
            .as_mut()
            .filter(|retry| retry.available())
        else {
            return Ok(false);
        };
        retry.attempts += 1;
        let attempt = retry.attempts;
        let retry = retry.clone();
        state.active_provider_turn_id = None;
        state.stream_indexes.clear();
        let provider_turn = turn
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| ProviderTurnId::new(id.to_string()).ok());
        let warning = resources.normalizer.event(
            &state,
            "turn/completed",
            Some(chat_turn.clone()),
            provider_turn,
            None,
            CanonicalEvent::RuntimeWarning(NotificationEvent {
                code: "codex_capacity_retry".to_string(),
                title: "The selected model is at capacity".to_string(),
                detail: Some(format!(
                    "Retrying in {} seconds, attempt {attempt} of {CODEX_CAPACITY_MAX_RETRIES}",
                    backoff(attempt).as_secs()
                )),
            }),
        )?;
        (warning, retry, attempt, chat_turn)
    };
    resources.sink.emit(warning).await?;
    tokio::spawn(run_capacity_retry(
        resources.clone(),
        retry,
        attempt,
        chat_turn,
    ));
    Ok(true)
}

/// Waits out the backoff, then sends the retry `turn/start`. A cancelled retry returns quietly,
/// because whoever cancelled it settles the turn; a refused retry settles it as failed.
async fn run_capacity_retry(
    resources: CodexRetryResources,
    retry: CodexCapacityRetry,
    attempt: u32,
    chat_turn: ChatTurnId,
) {
    tokio::select! {
        biased;
        () = retry.cancellation.cancelled() => return,
        () = tokio::time::sleep(backoff(attempt)) => {}
    }
    if !retry_is_current(&resources, &chat_turn, &retry) {
        return;
    }
    let context = DriverOperationContext {
        operation_id: format!("codex-capacity-retry-{attempt}"),
        deadline: Instant::now() + RETRY_REQUEST_TIMEOUT,
        // Cancelling the retry must not abandon a request Codex may already have accepted.
        cancellation: DriverCancellation::default(),
    };
    let accepted = resources
        .client
        .request("turn/start", retry.params.clone(), &context)
        .await
        .map_err(|error| error.to_chat_error("turn retry"))
        .and_then(|response| {
            decode_response::<TurnStartResponse>(response, "turn start response")
                .map_err(|error| error.to_chat_error("turn retry"))
        })
        .and_then(|response| {
            if response.turn.status == "inProgress" {
                Ok(response.turn.id)
            } else {
                Err(ChatError::new(
                    ChatErrorCode::Protocol,
                    "Codex did not accept the retried turn as in progress",
                    true,
                ))
            }
        });
    match accepted {
        Ok(provider_turn_id) => {
            if !retry_is_current(&resources, &chat_turn, &retry) {
                // The turn settled while the request was in flight, so Codex runs a turn nobody
                // watches; stop it.
                abandon_started_retry(&resources, &retry, provider_turn_id).await;
                return;
            }
            if let Ok(mut state) = resources.route.lock() {
                // A later failure may already have absorbed this turn into the next attempt.
                let still_this_attempt = state
                    .capacity_retry
                    .as_ref()
                    .is_some_and(|current| current.attempts == attempt);
                if state.active_chat_turn_id.as_ref() == Some(&chat_turn)
                    && state.active_provider_turn_id.is_none()
                    && still_this_attempt
                {
                    state.active_provider_turn_id = Some(provider_turn_id);
                }
            }
        }
        Err(error) => {
            if retry_is_current(&resources, &chat_turn, &retry) {
                let _ = settle_retry(&resources, ChatTurnState::Failed, Some(error.message)).await;
            }
        }
    }
}

/// Interrupts a retried turn Codex accepted after the chat turn had settled. A session that is
/// shutting down ends the turn with its process instead.
async fn abandon_started_retry(
    resources: &CodexRetryResources,
    retry: &CodexCapacityRetry,
    provider_turn_id: String,
) {
    if resources.expected_shutdown.load(Ordering::Acquire) {
        return;
    }
    let context = DriverOperationContext {
        operation_id: "codex-capacity-retry-abandon".to_string(),
        deadline: Instant::now() + RETRY_REQUEST_TIMEOUT,
        cancellation: DriverCancellation::default(),
    };
    let params = serde_json::json!({
        "threadId": retry.params.get("threadId").cloned().unwrap_or(Value::Null),
        "turnId": provider_turn_id,
    });
    // A refused interrupt leaves nothing to settle: the chat turn already ended and the
    // provider turn's own completion is routed like any other.
    let _ = resources
        .client
        .request("turn/interrupt", params, &context)
        .await;
}

/// Whether the session still runs `chat_turn` with this retry armed and not cancelled.
fn retry_is_current(
    resources: &CodexRetryResources,
    chat_turn: &ChatTurnId,
    retry: &CodexCapacityRetry,
) -> bool {
    !retry.cancellation.is_cancelled()
        && !resources.expected_shutdown.load(Ordering::Acquire)
        && resources
            .route
            .lock()
            .is_ok_and(|state| state.active_chat_turn_id.as_ref() == Some(chat_turn))
}

/// Settles the active chat turn while no provider turn runs, as an interrupt during a backoff or
/// a refused retry does.
pub(super) async fn settle_retry(
    resources: &CodexRetryResources,
    turn_state: ChatTurnState,
    stop_reason: Option<String>,
) -> ChatResult<()> {
    let event = {
        let mut state = resources.route.lock().map_err(|_| route_state_error())?;
        resources
            .normalizer
            .settled_turn(&mut state, None, turn_state, stop_reason)?
    };
    resources.sink.emit(event).await
}

fn route_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Codex route state is unavailable",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn backoff_doubles_from_two_seconds() {
        assert_eq!(
            (1..=CODEX_CAPACITY_MAX_RETRIES)
                .map(|attempt| backoff(attempt).as_secs())
                .collect::<Vec<_>>(),
            vec![2, 4, 8]
        );
    }

    #[test]
    fn armed_retry_keeps_settings_and_empties_the_input() {
        let retry = CodexCapacityRetry::arm(&json!({
            "threadId": "thread-1",
            "input": [{ "type": "text", "text": "Implement it" }],
            "summary": "detailed"
        }));
        assert_eq!(retry.params["input"], json!([]));
        assert_eq!(retry.params["threadId"], "thread-1");
        assert_eq!(retry.params["summary"], "detailed");
        assert!(retry.available());
        assert!(!retry.retrying());
        retry.cancel();
        assert!(!retry.available());
    }
}
