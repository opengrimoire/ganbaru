//! Steering, approval, and structured user-input operations.

use super::persistence::{
    AttachmentFolderScope, persist_steer_message, read_attachment_references,
    read_thread_runtime_data,
};
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
        .existing_owner(&request.thread_id)?
        .ok_or_else(runtime_not_running_error)?;
    let snapshot = owner.snapshot()?;
    if !snapshot.capabilities.supports(ProviderCapability::Steering) {
        return Err(ChatError::new(
            ChatErrorCode::CapabilityUnsupported,
            "This provider does not support in-turn steering",
            true,
        ));
    }
    let session_id = snapshot
        .session_id
        .clone()
        .ok_or_else(runtime_not_running_error)?;
    let turn_id = snapshot
        .active_turn_id()
        .cloned()
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
    crate::chat::ingestion::notify_rows_changed(
        &app,
        &pool,
        &request.thread_id,
        &[ChatProjectionKey::Thread, ChatProjectionKey::Messages],
    )
    .await;
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
    resolve_approval_in(&app, &pool, request).await
}

/// Resolves an open approval through `pool`. A choice to remember the approval is checked
/// before the provider receives the choice and recorded only after the provider accepted it.
pub(super) async fn resolve_approval_in(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    request: ResolveChatApprovalCommand,
) -> ChatResult<DriverOperationReceipt> {
    let pool = pool.clone();
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
        .existing_owner(&request.thread_id)?
        .ok_or_else(runtime_not_running_error)?;
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
    let choice = if request.remember {
        match super::remembered_approvals::prepare_choice(&pool, &request).await {
            Ok(choice) => Some(choice),
            Err(error) => return complete_driver_operation(&pool, &command_id, Err(error)).await,
        }
    } else {
        None
    };
    let resolve = owner.resolve_approval(
        ResolveApprovalRequest {
            command: request.command,
            session_id,
            request_id: request.request_id,
            provider_request_id: request.provider_request_id,
            decision: request.decision,
        },
        operation_context("resolve-approval", TURN_OPERATION_TIMEOUT),
    );
    let result = super::remembered_approvals::resolve_then_remember(&pool, choice, resolve).await;
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
        .existing_owner(&request.thread_id)?
        .ok_or_else(runtime_not_running_error)?;
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
    let command_id = request.command.client_command_id.clone();
    let provider_answers = match resolve_answer_attachments(&app, &pool, &request).await {
        Ok(answers) => answers,
        Err(error) => return complete_driver_operation(&pool, &command_id, Err(error)).await,
    };
    let now = now_timestamp()?;
    if let Err(error) = persist_answer_authorization(&pool, &request, &now).await {
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
                answers: provider_answers,
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

/// Resolves the attachments of each answer in the thread's working folder and returns the
/// answers the provider receives, with one `Attached:` line per attachment.
async fn resolve_answer_attachments(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    request: &ResolveChatUserInputCommand,
) -> ChatResult<Vec<UserInputAnswer>> {
    if request
        .answers
        .iter()
        .all(|answer| answer.attachment_ids.is_empty())
    {
        return Ok(request.answers.clone());
    }
    let working_folder_id = read_thread_runtime_data(pool, &request.thread_id)
        .await?
        .working_folder_id
        .ok_or_else(|| {
            ChatError::validation(
                "answers",
                "Answer attachments need a thread with a working folder",
            )
        })?;
    let mut answers = Vec::with_capacity(request.answers.len());
    for answer in &request.answers {
        let attachments = read_attachment_references(
            app,
            pool,
            AttachmentFolderScope::Folder(Some(&working_folder_id)),
            &answer.attachment_ids,
        )
        .await?;
        answers.push(answer_with_attachment_lines(answer.clone(), &attachments));
    }
    Ok(answers)
}

/// Saves the submitted answers and exposes their attachments to the thread's internal MCP
/// resource reads in one transaction, so the provider can read every attachment it is told about.
async fn persist_answer_authorization(
    pool: &sqlx::SqlitePool,
    request: &ResolveChatUserInputCommand,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_user_input_drafts (request_id, answers_data, updated_at)
         VALUES (?, ?, ?)
         ON CONFLICT(request_id) DO UPDATE SET answers_data = excluded.answers_data,
             updated_at = excluded.updated_at",
    )
    .bind(request.request_id.as_str())
    .bind(serde_json::to_string(&request.answers).map_err(json_error)?)
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for answer in &request.answers {
        expose_attachments_to_thread(
            &mut transaction,
            &request.thread_id,
            &answer.attachment_ids,
            now,
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)
}

/// Records that a thread may read these attachments as Chat resources. A reference that already
/// exists keeps its first message.
pub(super) async fn expose_attachments_to_thread(
    connection: &mut sqlx::SqliteConnection,
    thread_id: &ChatThreadId,
    attachment_ids: &[ChatAttachmentId],
    now: &UtcTimestamp,
) -> ChatResult<()> {
    for attachment_id in attachment_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO chat_resource_thread_references
                (resource_id, thread_id, first_message_id, created_at)
             SELECT id, ?, NULL, ? FROM chat_resources WHERE attachment_id = ?",
        )
        .bind(thread_id.as_str())
        .bind(now.as_str())
        .bind(attachment_id.as_str())
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

/// Appends one `Attached: <display name> <resource URI>` line per attachment to the answer's
/// free-form text and clears its attachment list, because provider answer channels carry text
/// only. Line breaks and other control characters in a display name become spaces so each
/// attachment stays on its own line.
pub(super) fn answer_with_attachment_lines(
    mut answer: UserInputAnswer,
    attachments: &[PromptAttachmentReference],
) -> UserInputAnswer {
    answer.attachment_ids.clear();
    if attachments.is_empty() {
        return answer;
    }
    let lines = attachments
        .iter()
        .map(|attachment| {
            let name: String = attachment
                .display_name
                .chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .collect();
            format!("Attached: {} {}", name.trim(), attachment.resource_uri)
        })
        .collect::<Vec<_>>()
        .join("\n");
    answer.free_form_text = Some(match answer.free_form_text.take() {
        Some(text) if text.is_empty() => lines,
        Some(text) if text.ends_with('\n') => format!("{text}{lines}"),
        Some(text) => format!("{text}\n{lines}"),
        None => lines,
    });
    answer
}
