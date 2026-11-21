//! "Allow and don't ask again" for agent approvals in a channel: the offer on an open request,
//! the rule a person's choice records, and the answer Ganbaru gives for them when a later
//! request matches a live rule.
//!
//! Every step derives the rule from the request Ganbaru persisted, never from what a client
//! sends, so the offer, the recorded rule, and the match agree.

use super::support::{json_error, now_timestamp, persistence_error};
use crate::chat::send_commands::ResolveChatApprovalCommand;
use ganbaru_chat::coordination::remembered_approvals::{
    approval_is_live, channel_run_scope, remember_approval, rememberable_approval,
};
use ganbaru_chat::runtime::{ChatRuntimeRegistry, OpenRequestKind};
use ganbaru_chat_contracts::events::ApprovalDecisionOption;
use ganbaru_chat_contracts::models::*;
use serde_json::Value;
use sqlx::SqlitePool;
use std::time::Duration;
use tauri::Manager;

/// The person every choice made on this device is recorded for.
const LOCAL_PARTICIPANT_ID: &str = "participant:local-owner";
/// Prefix of the command ids of answers Ganbaru gives for a remembered approval.
const REMEMBERED_APPROVAL_COMMAND_PREFIX: &str = "remembered-approval:";
/// How often the answer checks that the provider is waiting for the request.
const WAITING_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// How many checks the answer makes before it leaves the request to the person.
const WAITING_POLLS: u32 = 40;
/// How many times the answer tries again when the provider has not registered the request yet.
const CONFLICT_RETRIES: u32 = 4;
/// The wait before trying again after a conflict.
const CONFLICT_RETRY_DELAY: Duration = Duration::from_millis(50);

/// An open approval request as Ganbaru persisted it.
struct OpenApproval {
    provider_request_id: String,
    safe_display: Value,
    allowed_decisions: Vec<ApprovalDecisionOption>,
}

/// Reads the open approval request `request_id` of the thread `thread_id`, or `None` when it is
/// no longer open there.
async fn read_open_approval(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    request_id: &str,
) -> ChatResult<Option<OpenApproval>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT provider_request_id, safe_display_data, allowed_decisions_data
         FROM chat_pending_requests
         WHERE id = ? AND thread_id = ? AND request_kind = 'approval'
           AND resolution_state = 'open'",
    )
    .bind(request_id)
    .bind(thread_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    row.map(|(provider_request_id, safe_display, allowed)| {
        Ok(OpenApproval {
            provider_request_id,
            safe_display: serde_json::from_str(&safe_display).map_err(json_error)?,
            allowed_decisions: serde_json::from_str(&allowed).map_err(json_error)?,
        })
    })
    .transpose()
}

/// The provider option of the allow-once decision a request offers.
fn allow_once_option(allowed: &[ApprovalDecisionOption]) -> Option<&str> {
    allowed
        .iter()
        .find(|option| option.decision_kind == ApprovalDecisionKind::AllowOnce)
        .map(|option| option.id.as_str())
}

/// The rule an "Allow and don't ask again" choice would record for the open approval request
/// whose persisted display data and allowed decisions are `safe_display_data` and
/// `allowed_decisions_data`, or `None` when the request offers no allow-once choice, is not
/// part of a channel run with a folder, scratch space, or environment, or cannot be remembered.
///
/// # Errors
///
/// Returns a persistence error when a read fails or the stored data is invalid.
pub(crate) async fn offered_rule(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    safe_display_data: &str,
    allowed_decisions_data: &str,
) -> ChatResult<Option<ChatRememberableApprovalRead>> {
    let allowed: Vec<ApprovalDecisionOption> =
        serde_json::from_str(allowed_decisions_data).map_err(json_error)?;
    if allow_once_option(&allowed).is_none() {
        return Ok(None);
    }
    let safe_display: Value = serde_json::from_str(safe_display_data).map_err(json_error)?;
    let Some(rule) = rememberable_approval(&safe_display) else {
        return Ok(None);
    };
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    Ok(channel_run_scope(&mut connection, thread_id.as_str())
        .await?
        .map(|_| rule))
}

/// Records the rule of the request a person allows once and chose not to be asked about again,
/// under the channel's current access. Only an allow-once choice of a request that offers a
/// rule can be remembered.
///
/// # Errors
///
/// Returns a validation error when the request cannot be remembered and a persistence error
/// when a read or the write fails.
pub(super) async fn remember_choice(
    pool: &SqlitePool,
    request: &ResolveChatApprovalCommand,
) -> ChatResult<()> {
    if request.decision.kind != ApprovalDecisionKind::AllowOnce {
        return Err(not_rememberable(
            "Only an allow once choice can be remembered",
        ));
    }
    let open = read_open_approval(pool, &request.thread_id, request.request_id.as_str())
        .await?
        .ok_or_else(|| ChatError::new(ChatErrorCode::Conflict, "Chat approval is stale", false))?;
    let rule = rememberable_approval(&open.safe_display)
        .ok_or_else(|| not_rememberable("This request cannot be remembered"))?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let scope = channel_run_scope(&mut connection, request.thread_id.as_str())
        .await?
        .ok_or_else(|| {
            not_rememberable("Only requests of agents working in a channel can be remembered")
        })?;
    let now = now_timestamp()?;
    remember_approval(
        &mut connection,
        &scope,
        &rule,
        LOCAL_PARTICIPANT_ID,
        now.as_str(),
    )
    .await
}

fn not_rememberable(message: &str) -> ChatError {
    ChatError::validation("remember", message)
}

/// Answers the newly opened approval request `request_id` of the channel run on `thread_id`
/// with its allow-once choice when a live remembered approval matches it, in the background.
/// Any failure leaves the request for a person and never denies it.
pub(super) fn spawn_remembered_resolution(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: ChatThreadId,
    request_id: String,
) {
    let app = app.clone();
    let pool = pool.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = resolve_remembered(&app, &pool, &thread_id, &request_id).await {
            eprintln!(
                "A remembered approval was not applied, so the request waits for a person: {:?}",
                error.code
            );
        }
    });
}

/// The command that answers `open` with its allow-once option `option_id` on `attempt`.
fn remembered_command(
    thread_id: &ChatThreadId,
    request_id: &str,
    open: &OpenApproval,
    option_id: &str,
    attempt: u32,
) -> ChatResult<ResolveChatApprovalCommand> {
    let invalid = |_| ChatError::validation("requestId", "The approval request id is invalid");
    Ok(ResolveChatApprovalCommand {
        command: ChatCommandContext {
            client_command_id: ChatCommandId::new(format!(
                "{REMEMBERED_APPROVAL_COMMAND_PREFIX}{request_id}:{attempt}"
            ))
            .map_err(invalid)?,
            expected_thread_revision: None,
        },
        thread_id: thread_id.clone(),
        request_id: ChatRequestId::new(request_id.to_string()).map_err(invalid)?,
        provider_request_id: ProviderRequestId::new(open.provider_request_id.clone())
            .map_err(invalid)?,
        decision: ApprovalDecision {
            kind: ApprovalDecisionKind::AllowOnce,
            provider_option_id: Some(option_id.to_string()),
            updated_tool_input: None,
        },
        remember: false,
    })
}

async fn resolve_remembered(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    request_id: &str,
) -> ChatResult<()> {
    let Some(open) = read_open_approval(pool, thread_id, request_id).await? else {
        return Ok(());
    };
    if !wait_for_open_approval(app, thread_id, &open.provider_request_id).await {
        return Ok(());
    }
    let Some(option_id) = allow_once_option(&open.allowed_decisions) else {
        return Ok(());
    };
    let Some(rule) = rememberable_approval(&open.safe_display) else {
        return Ok(());
    };
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let Some(scope) = channel_run_scope(&mut connection, thread_id.as_str()).await? else {
        return Ok(());
    };
    if !approval_is_live(&mut connection, &scope, &rule).await? {
        return Ok(());
    }
    drop(connection);
    let mut attempt = 0;
    loop {
        let command = remembered_command(thread_id, request_id, &open, option_id, attempt)?;
        match super::interactions::resolve_approval_in(app, pool, command).await {
            Err(error) if error.code == ChatErrorCode::Conflict && attempt < CONFLICT_RETRIES => {
                attempt += 1;
                tokio::time::sleep(CONFLICT_RETRY_DELAY).await;
            }
            result => return result.map(|_| ()),
        }
    }
}

/// Waits until the runtime of `thread_id` holds the approval `provider_request_id` open, which
/// it does only after the event that opened the request is delivered. Returns false when the
/// request does not open in time or the session stops running.
async fn wait_for_open_approval(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
    provider_request_id: &str,
) -> bool {
    let Ok(provider_request_id) = ProviderRequestId::new(provider_request_id.to_string()) else {
        return false;
    };
    for _ in 0..WAITING_POLLS {
        match app.state::<ChatRuntimeRegistry>().snapshot(thread_id) {
            Ok(snapshot)
                if snapshot.open_request(&provider_request_id)
                    == Some(OpenRequestKind::Approval) =>
            {
                return true;
            }
            Ok(_) => tokio::time::sleep(WAITING_POLL_INTERVAL).await,
            Err(_) => return false,
        }
    }
    false
}
