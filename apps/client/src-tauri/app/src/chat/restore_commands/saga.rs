use super::*;
use ganbaru_chat_contracts::models::ProviderSessionState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestoreFailurePhase {
    SafeRetry,
    ProviderRollback,
    WorkspaceRestore,
}

pub(super) async fn execute_restore(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: &ExecuteChatRestoreRequest,
    now: &UtcTimestamp,
) -> ChatResult<ChatRestoreResultRead> {
    let preview = read_preview(pool, &request.preview_id).await?;
    if preview.thread_id != request.thread_id
        || preview.expected_thread_revision
            != request
                .command
                .expected_thread_revision
                .unwrap_or(preview.expected_thread_revision)
        || preview.expires_at.as_str() < now.as_str()
    {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Checkpoint restore preview is stale",
            true,
        ));
    }
    let (_, authorized, revision) = authorize_thread(app, pool, &request.thread_id).await?;
    if revision != preview.expected_thread_revision
        || authorized.repository_identity.as_deref() != Some(&preview.repository_identity)
    {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Thread or repository changed after the restore preview",
            true,
        ));
    }
    let target = read_stored_checkpoint(pool, &preview.checkpoint_id).await?;
    let current = {
        let authorized = authorized.clone();
        let target = target.clone();
        tauri::async_runtime::spawn_blocking(move || {
            verify_checkpoint(&authorized, &target)?;
            let current = current_git_snapshot(&authorized.canonical_path)?;
            require_head_context(&current, &target)?;
            Ok::<_, ChatError>(current)
        })
        .await
        .map_err(|_| restore_worker_error())??
    };
    if current.worktree_tree_oid != preview.current.worktree_tree_oid
        || current.index_tree_oid != preview.current.index_tree_oid
        || current.index_fingerprint != preview.current.index_fingerprint
        || current.head_oid != preview.current.head_oid
        || current.head_ref != preview.current.head_ref
    {
        mark_preview_state(pool, &preview.id, "stale", now).await?;
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Workspace changed after the restore preview",
            true,
        ));
    }
    mark_preview_state(pool, &preview.id, "executing", now).await?;
    let mut failure_phase = RestoreFailurePhase::SafeRetry;
    let execution: ChatResult<ChatRestoreResultRead> = async {
    let owner = app
        .state::<ChatRuntimeRegistry>()
        .owner(request.thread_id.clone())?;
    let snapshot = owner.snapshot()?;
    if let (Some(session_id), Some(turn_id)) =
        (snapshot.session_id.clone(), snapshot.active_turn_id.clone())
    {
        owner
            .interrupt_turn(
                InterruptTurnRequest {
                    command: request.command.clone(),
                    session_id,
                    turn_id,
                },
                operation_context("restore-interrupt", Duration::from_secs(15)),
            )
            .await?;
    }
    let _mutation_guard = app
        .state::<crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry>()
        .mutation_with_timeout(&authorized.canonical_path, Duration::from_secs(30))
        .await?;
    let current = {
        let authorized = authorized.clone();
        let target = target.clone();
        tauri::async_runtime::spawn_blocking(move || {
            verify_checkpoint(&authorized, &target)?;
            let current = current_git_snapshot(&authorized.canonical_path)?;
            require_head_context(&current, &target)?;
            Ok::<_, ChatError>(current)
        })
        .await
        .map_err(|_| restore_worker_error())??
    };
    if current.worktree_tree_oid != preview.current.worktree_tree_oid
        || current.index_tree_oid != preview.current.index_tree_oid
        || current.index_fingerprint != preview.current.index_fingerprint
        || current.head_oid != preview.current.head_oid
        || current.head_ref != preview.current.head_ref
    {
        mark_preview_state(pool, &preview.id, "stale", now).await?;
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Workspace changed while the active coding agent was stopping",
            true,
        ));
    }
    let refreshed = owner.snapshot()?;
    let target_turn_id = target_turn_id(pool, &request.thread_id, target.turn_count).await?;
    let provider_cursor = match target_turn_id.as_ref() {
        Some(turn_id) => stored_provider_rollback_cursor(pool, &request.thread_id, turn_id).await?,
        None => None,
    };
    let provider_history_action = if refreshed
        .capabilities
        .supports(ProviderCapability::NativeRollback)
        && refreshed.session_id.is_some()
        && refreshed.session_state == ProviderSessionState::Ready
        && provider_cursor.is_some()
    {
        failure_phase = RestoreFailurePhase::ProviderRollback;
        owner
            .rollback(
                RollbackRequest {
                    command: request.command.clone(),
                    session_id: refreshed
                        .session_id
                        .clone()
                        .ok_or_else(restore_worker_error)?,
                    checkpoint_id: Some(target.id.clone()),
                    provider_cursor,
                    target_turn_id,
                },
                operation_context("restore-provider-rollback", Duration::from_secs(30)),
            )
            .await?;
        "rolled_back"
    } else {
        "fork_required"
    };
    if provider_history_action == "fork_required" {
        failure_phase = RestoreFailurePhase::WorkspaceRestore;
    }
    let root = authorized.canonical_path.clone();
    let current_for_restore = current.clone();
    let target_for_restore = target.clone();
    let restore_result = tauri::async_runtime::spawn_blocking(move || {
        restore_git_snapshot(&root, &current_for_restore, &target_for_restore)
    })
    .await
    .map_err(|_| restore_worker_error())?;
    if let Err(error) = restore_result {
        record_restore_operation(
            pool,
            request,
            &target,
            provider_history_action,
            "recovery_required",
            Some(&error),
            now,
        )
        .await?;
        mark_preview_state(pool, &preview.id, "failed", now).await?;
        return Err(ChatError::new(
            ChatErrorCode::Internal,
            "Workspace restore failed after provider coordination. Checkpoint refs were retained for recovery.",
            true,
        ));
    }
    if provider_history_action == "fork_required" && refreshed.session_id.is_some() {
        let _ = owner
            .stop_session(
                true,
                operation_context("restore-provider-fork", Duration::from_secs(30)),
            )
            .await;
        if owner.snapshot()?.session_id.is_some() {
            let error = ChatError::new(
                ChatErrorCode::Internal,
                "The workspace was restored, but the previous provider session could not be detached",
                true,
            );
            record_restore_operation(
                pool,
                request,
                &target,
                provider_history_action,
                "recovery_required",
                Some(&error),
                now,
            )
            .await?;
            mark_preview_state(pool, &preview.id, "failed", now).await?;
            return Err(error);
        }
    }
    let reverted_turn_ids = match persist_restore(
        pool,
        request,
        &preview,
        &target,
        provider_history_action,
        now,
    )
    .await
    {
        Ok(turn_ids) => turn_ids,
        Err(error) => {
            let _ = record_restore_operation(
                pool,
                request,
                &target,
                provider_history_action,
                "recovery_required",
                Some(&error),
                now,
            )
            .await;
            let _ = mark_preview_state(pool, &preview.id, "failed", now).await;
            return Err(ChatError::new(
                ChatErrorCode::Persistence,
                "The workspace was restored, but Chat history reconciliation failed. Checkpoint refs were retained for recovery.",
                true,
            ));
        }
    };
    mark_preview_state(pool, &preview.id, "completed", now).await?;
    let thread_revision: i64 = sqlx::query_scalar("SELECT revision FROM chat_threads WHERE id = ?")
        .bind(request.thread_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
    Ok(ChatRestoreResultRead {
        checkpoint_id: target.id.clone(),
        reverted_turn_ids,
        provider_history_action: provider_history_action.to_string(),
        recovery_state: "complete".to_string(),
        thread_revision: u64::try_from(thread_revision).map_err(|_| corrupt_data_error())?,
    })
    }
    .await;
    if let Err(error) = execution.as_ref() {
        settle_executing_preview_failure(
            pool,
            request,
            &preview,
            &target,
            failure_phase,
            error,
            now,
        )
        .await?;
    }
    execution
}
