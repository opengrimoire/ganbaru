//! Provider runtime operations initiated by interaction controls.

use super::support::{chat_pool, now_timestamp, persistence_error, require_workspace};
use crate::chat::credentials::{PlatformCredentialStore, materialize_provider_environment};
use crate::chat::device_state::{read_active_device_scope, update_active_device_scope};
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use ganbaru_chat::driver_operations::{complete_driver_operation, replay_driver_receipt};
use ganbaru_chat::repository::receipts::{CommandReceiptClaim, claim_command_receipt};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::{
    ChatCommandContext, ChatCommandId, ChatError, ChatErrorCode, ChatResult, ChatThreadId,
    ChatTurnId, CompactContextRequest, DriverOperationReceipt, InterruptTurnRequest, McpStatusRead,
    McpStatusRequest, ProjectWorkingFolderId, ProviderInstanceId,
};
use ganbaru_chat_providers::{
    DriverCancellation, DriverOperationContext, ProviderDriverFactory, ProviderDriverRegistry,
};
use sqlx::Row;
use std::time::{Duration, Instant};
use tauri::Manager;

pub(crate) async fn compact_context(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    client_command_id: ChatCommandId,
) -> ChatResult<DriverOperationReceipt> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(thread_id.clone())?;
    let session_id = owner.snapshot()?.session_id.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::InvalidStateTransition,
            "Chat session is not running",
            true,
        )
    })?;
    match claim_command_receipt(
        &pool,
        &client_command_id,
        &thread_id,
        "compact_context",
        None,
        &now_timestamp()?,
    )
    .await?
    {
        CommandReceiptClaim::Replay(receipt) => return replay_driver_receipt(receipt),
        CommandReceiptClaim::Claimed(_) => {}
    }
    let turn_id = ChatTurnId::new(format!("compact-{}", client_command_id.as_str()))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create compact turn ID", false))?;
    let result = owner
        .compact_context(
            CompactContextRequest {
                session_id,
                turn_id,
            },
            operation_context("ui-compact-context", Duration::from_secs(30)),
        )
        .await;
    complete_driver_operation(&pool, &client_command_id, result).await
}

pub(crate) async fn read_mcp_status(
    app: tauri::AppHandle,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    provider_instance_id: ProviderInstanceId,
    thread_id: Option<ChatThreadId>,
) -> ChatResult<McpStatusRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let workspace = require_workspace(
        &app,
        &pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::ProviderStart,
    )
    .await?;
    let provider = crate::chat::settings_commands::read_provider(&app, &provider_instance_id)?;
    let context = operation_context("ui-read-mcp-status", Duration::from_secs(30));

    if let Some(thread_id) = thread_id {
        let thread = sqlx::query(
            "SELECT working_folder_id, provider_instance_id FROM chat_threads
             WHERE id = ? AND archived_at IS NULL",
        )
        .bind(thread_id.as_str())
        .fetch_optional(&pool)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
        })?;
        let pinned_working_folder: String = thread
            .try_get("working_folder_id")
            .map_err(persistence_error)?;
        let pinned_provider: String = thread
            .try_get("provider_instance_id")
            .map_err(persistence_error)?;
        if working_folder_id.as_str() != pinned_working_folder
            || provider_instance_id.as_str() != pinned_provider
        {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "MCP status task does not match the selected workspace and provider",
                true,
            ));
        }
        let owner = app.state::<ChatRuntimeRegistry>().owner(thread_id)?;
        if let Some(session_id) = owner.snapshot()?.session_id {
            return owner
                .read_mcp_status(
                    McpStatusRequest {
                        session_id: Some(session_id),
                        working_directory: None,
                    },
                    context,
                )
                .await;
        }
    }

    let configuration = materialize_provider_environment(
        &provider.configuration,
        &PlatformCredentialStore::default(),
    )?;
    let mut driver = ProviderDriverRegistry.create_driver(configuration)?;
    driver
        .read_mcp_status(
            McpStatusRequest {
                session_id: None,
                working_directory: Some(workspace.canonical_path),
            },
            &context,
        )
        .await
}

pub(crate) fn set_full_access_trust(
    app: tauri::AppHandle,
    provider_instance_id: ProviderInstanceId,
    working_folder_id: ProjectWorkingFolderId,
    trusted: bool,
) -> ChatResult<bool> {
    let timestamp = now_timestamp()?;
    update_active_device_scope(&app, |scope| {
        crate::chat::device_state::set_full_access_trust(
            scope,
            provider_instance_id,
            working_folder_id,
            trusted.then_some(timestamp),
        );
        Ok(())
    })
    .map_err(device_state_error)?;
    Ok(trusted)
}

pub(crate) fn has_full_access_trust(
    app: tauri::AppHandle,
    provider_instance_id: ProviderInstanceId,
    working_folder_id: ProjectWorkingFolderId,
) -> ChatResult<bool> {
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    Ok(crate::chat::device_state::full_access_is_trusted(
        &scope,
        &provider_instance_id,
        &working_folder_id,
    ))
}

pub(crate) async fn stop_session(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    force: bool,
    client_command_id: ChatCommandId,
) -> ChatResult<DriverOperationReceipt> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(thread_id.clone())?;
    let runtime_snapshot = owner.snapshot()?;
    let interrupt_context = if force {
        None
    } else {
        Some((
            runtime_snapshot.session_id.clone().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::InvalidStateTransition,
                    "Chat session is not running",
                    true,
                )
            })?,
            runtime_snapshot.active_turn_id.clone().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::InvalidStateTransition,
                    "Chat turn is not running",
                    true,
                )
            })?,
        ))
    };
    let command_kind = if force {
        "force_stop_session"
    } else {
        "interrupt_turn"
    };
    match claim_command_receipt(
        &pool,
        &client_command_id,
        &thread_id,
        command_kind,
        None,
        &now_timestamp()?,
    )
    .await?
    {
        CommandReceiptClaim::Replay(receipt) => return replay_driver_receipt(receipt),
        CommandReceiptClaim::Claimed(_) => {}
    }
    if force {
        let mutations =
            app.state::<crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry>();
        let result = owner
            .stop_session_and_release(
                true,
                operation_context("ui-force-stop-session", Duration::from_secs(5)),
                &mutations,
            )
            .await;
        let result = complete_driver_operation(&pool, &client_command_id, result).await;
        if result.is_ok() {
            app.state::<crate::chat::internal_mcp::InternalMcpRegistry>()
                .stop_thread_endpoint(&thread_id)
                .await;
        }
        return result;
    }
    let (session_id, turn_id) = interrupt_context.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::InvalidStateTransition,
            "Chat provider session is not running",
            true,
        )
    })?;
    let result = owner
        .interrupt_turn(
            InterruptTurnRequest {
                command: ChatCommandContext {
                    client_command_id: client_command_id.clone(),
                    expected_thread_revision: None,
                },
                session_id,
                turn_id,
            },
            operation_context("ui-interrupt-turn", Duration::from_secs(15)),
        )
        .await;
    complete_driver_operation(&pool, &client_command_id, result).await
}

fn operation_context(operation_id: &str, timeout: Duration) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + timeout,
        cancellation: DriverCancellation::default(),
    }
}

fn device_state_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat device state could not be updated",
        true,
    )
}
