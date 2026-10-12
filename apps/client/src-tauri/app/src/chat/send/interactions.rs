//! Steering, approval, and structured user-input operations.

use super::persistence::persist_steer_message;
use super::support::{
    TURN_OPERATION_TIMEOUT, chat_pool, json_error, now_timestamp, operation_context,
    persistence_error, runtime_not_running_error,
};
use super::validation::{
    validate_answers, validate_approval_decision, validate_pending_request, validate_prompt,
};
use crate::chat::send_commands::{
    ResolveChatApprovalCommand, ResolveChatUserInputCommand, SteerChatTurnCommand,
};
use ganbaru_chat::driver_operations::{complete_driver_operation, replay_driver_receipt};
use ganbaru_chat::repository::receipts::{CommandReceiptClaim, claim_command_receipt};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::*;
use tauri::Manager;

pub(crate) async fn steer_turn(
    app: tauri::AppHandle,
    db_url: String,
    request: SteerChatTurnCommand,
) -> ChatResult<DriverOperationReceipt> {
    validate_prompt(&request.prompt, false)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(request.thread_id.clone())?;
    let snapshot = owner.snapshot()?;
    if !snapshot.capabilities.supports(ProviderCapability::Steering) {
        return Err(ChatError::new(
            ChatErrorCode::CapabilityUnsupported,
            "This provider does not support in-turn steering",
            true,
        ));
    }
    let session_id = snapshot.session_id.ok_or_else(runtime_not_running_error)?;
    let turn_id = snapshot
        .active_turn_id
        .ok_or_else(runtime_not_running_error)?;
    let command_id = request.command.client_command_id.clone();
    match claim_command_receipt(
        &pool,
        &command_id,
        &request.thread_id,
        "steer_turn",
        request.command.expected_thread_revision,
        &now_timestamp()?,
    )
    .await?
    {
        CommandReceiptClaim::Replay(receipt) => return replay_driver_receipt(receipt),
        CommandReceiptClaim::Claimed(_) => {}
    }
    if let Err(error) = persist_steer_message(&pool, &request, &turn_id, &now_timestamp()?).await {
        return complete_driver_operation(&pool, &command_id, Err(error)).await;
    }
    let result = owner
        .steer_turn(
            SteerTurnRequest {
                command: request.command,
                session_id,
                turn_id,
                prompt: request.prompt,
            },
            operation_context("steer-turn", TURN_OPERATION_TIMEOUT),
        )
        .await;
    complete_driver_operation(&pool, &command_id, result).await
}

pub(crate) async fn resolve_approval(
    app: tauri::AppHandle,
    db_url: String,
    request: ResolveChatApprovalCommand,
) -> ChatResult<DriverOperationReceipt> {
    let pool = chat_pool(app.clone(), db_url).await?;
    validate_pending_request(
        &pool,
        &request.thread_id,
        &request.request_id,
        &request.provider_request_id,
        "approval",
    )
    .await?;
    validate_approval_decision(&pool, &request.request_id, &request.decision).await?;
    if matches!(
        request.decision.kind,
        ApprovalDecisionKind::AllowOnce | ApprovalDecisionKind::AllowSession
    ) && organizational_conversation_is_active(&pool, &request.thread_id).await?
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Conversation assignments cannot approve provider tool execution",
            false,
        ));
    }
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(request.thread_id.clone())?;
    let session_id = owner
        .snapshot()?
        .session_id
        .ok_or_else(runtime_not_running_error)?;
    match claim_command_receipt(
        &pool,
        &request.command.client_command_id,
        &request.thread_id,
        "resolve_approval",
        request.command.expected_thread_revision,
        &now_timestamp()?,
    )
    .await?
    {
        CommandReceiptClaim::Replay(receipt) => return replay_driver_receipt(receipt),
        CommandReceiptClaim::Claimed(_) => {}
    }
    let command_id = request.command.client_command_id.clone();
    let result = owner
        .resolve_approval(
            ResolveApprovalRequest {
                command: request.command,
                session_id,
                request_id: request.request_id,
                provider_request_id: request.provider_request_id,
                decision: request.decision,
            },
            operation_context("resolve-approval", TURN_OPERATION_TIMEOUT),
        )
        .await;
    complete_driver_operation(&pool, &command_id, result).await
}

async fn organizational_conversation_is_active(
    pool: &sqlx::SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_agent_runs
            WHERE provider_thread_id = ?
              AND state IN ('starting', 'working', 'waiting')
              AND working_folder_id IS NULL
              AND scratch_generation_id IS NULL
              AND execution_environment_id IS NULL
         )",
    )
    .bind(thread_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)
}

pub(crate) async fn resolve_user_input(
    app: tauri::AppHandle,
    db_url: String,
    request: ResolveChatUserInputCommand,
) -> ChatResult<DriverOperationReceipt> {
    validate_answers(&request.answers)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    validate_pending_request(
        &pool,
        &request.thread_id,
        &request.request_id,
        &request.provider_request_id,
        "user_input",
    )
    .await?;
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(request.thread_id.clone())?;
    let session_id = owner
        .snapshot()?
        .session_id
        .ok_or_else(runtime_not_running_error)?;
    match claim_command_receipt(
        &pool,
        &request.command.client_command_id,
        &request.thread_id,
        "resolve_user_input",
        request.command.expected_thread_revision,
        &now_timestamp()?,
    )
    .await?
    {
        CommandReceiptClaim::Replay(receipt) => return replay_driver_receipt(receipt),
        CommandReceiptClaim::Claimed(_) => {}
    }
    let now = now_timestamp()?;
    let answer_persistence = sqlx::query(
        "INSERT INTO chat_user_input_drafts (request_id, answers_data, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT(request_id) DO UPDATE SET answers_data = excluded.answers_data,
             updated_at = excluded.updated_at",
    )
    .bind(request.request_id.as_str())
    .bind(serde_json::to_string(&request.answers).map_err(json_error)?)
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error);
    let command_id = request.command.client_command_id.clone();
    if let Err(error) = answer_persistence {
        return complete_driver_operation(&pool, &command_id, Err(error)).await;
    }
    let request_id = request.request_id.clone();
    let result = owner
        .resolve_user_input(
            ResolveUserInputRequest {
                command: request.command,
                session_id,
                request_id: request.request_id.clone(),
                provider_request_id: request.provider_request_id,
                answers: request.answers,
            },
            operation_context("resolve-user-input", TURN_OPERATION_TIMEOUT),
        )
        .await;
    let result = complete_driver_operation(&pool, &command_id, result).await?;
    sqlx::query("DELETE FROM chat_user_input_drafts WHERE request_id = ?")
        .bind(request_id.as_str())
        .execute(&pool)
        .await
        .map_err(persistence_error)?;
    Ok(result)
}
