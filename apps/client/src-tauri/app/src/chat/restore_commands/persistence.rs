use super::*;

pub(super) async fn persist_restore(
    pool: &SqlitePool,
    request: &ExecuteChatRestoreRequest,
    preview: &StoredRestorePreview,
    target: &StoredCheckpoint,
    provider_history_action: &str,
    now: &UtcTimestamp,
) -> ChatResult<Vec<ChatTurnId>> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let rows = sqlx::query(
        "SELECT id FROM chat_turns
         WHERE thread_id = ? AND ordinal >= ? AND invalidated_at IS NULL ORDER BY ordinal",
    )
    .bind(request.thread_id.as_str())
    .bind(i64_value(target.turn_count)?)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let reverted_turn_ids = rows
        .into_iter()
        .map(|row| {
            ChatTurnId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
                .map_err(|_| corrupt_data_error())
        })
        .collect::<ChatResult<Vec<_>>>()?;
    sqlx::query(
        "UPDATE chat_events SET invalidated_at = ?, invalidation_reason = 'checkpoint_restore'
         WHERE thread_id = ? AND invalidated_at IS NULL AND turn_id IN (
             SELECT id FROM chat_turns WHERE thread_id = ? AND ordinal >= ?
         )",
    )
    .bind(now.as_str())
    .bind(request.thread_id.as_str())
    .bind(request.thread_id.as_str())
    .bind(i64_value(target.turn_count)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_turns SET invalidated_at = ?, invalidation_reason = 'checkpoint_restore'
         WHERE thread_id = ? AND ordinal >= ? AND invalidated_at IS NULL",
    )
    .bind(now.as_str())
    .bind(request.thread_id.as_str())
    .bind(i64_value(target.turn_count)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_pending_requests SET resolution_state = 'stale', resolved_at = ?
         WHERE thread_id = ? AND resolution_state = 'open'",
    )
    .bind(now.as_str())
    .bind(request.thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_queued_followups SET state = 'cancelled', updated_at = ?
         WHERE thread_id = ? AND state = 'queued'",
    )
    .bind(now.as_str())
    .bind(request.thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    enqueue_invalidated_checkpoints(&mut transaction, &request.thread_id, target, now).await?;
    append_reverted_event(
        &mut transaction,
        request,
        target,
        &reverted_turn_ids,
        provider_history_action,
        now,
    )
    .await?;
    sqlx::query(
        "UPDATE chat_threads
         SET state = 'idle', latest_turn_state = (
                SELECT state FROM chat_turns
                WHERE thread_id = chat_threads.id AND invalidated_at IS NULL
                ORDER BY ordinal DESC LIMIT 1
             ),
             provider_thread_id = CASE WHEN ? = 'fork_required' THEN NULL ELSE provider_thread_id END,
             resume_cursor_schema_version = CASE
                 WHEN ? = 'fork_required' THEN NULL ELSE resume_cursor_schema_version END,
             resume_cursor_data = CASE
                 WHEN ? = 'fork_required' THEN NULL ELSE resume_cursor_data END,
             message_count = (
                SELECT COUNT(*) FROM chat_messages
                WHERE thread_id = chat_threads.id AND (
                    turn_id IS NULL OR EXISTS (
                        SELECT 1 FROM chat_turns
                        WHERE id = chat_messages.turn_id AND invalidated_at IS NULL
                    )
                )
             ),
             latest_preview = (
                SELECT substr(normalized_markdown, -2000) FROM chat_messages
                WHERE thread_id = chat_threads.id AND (
                    turn_id IS NULL OR EXISTS (
                        SELECT 1 FROM chat_turns
                        WHERE id = chat_messages.turn_id AND invalidated_at IS NULL
                    )
                )
                ORDER BY sequence_anchor DESC, id DESC LIMIT 1
             ),
             changed_file_summary_schema_version = NULL,
             changed_file_summary_data = NULL,
             revision = revision + 1, updated_at = ?, last_activity_at = ?
         WHERE id = ?",
    )
    .bind(provider_history_action)
    .bind(provider_history_action)
    .bind(provider_history_action)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(request.thread_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_restore_operations
            (id, thread_id, checkpoint_id, preview_id, provider_history_action,
             recovery_state, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'complete', ?, ?)",
    )
    .bind(format!(
        "restore-operation:{}",
        request.command.client_command_id.as_str()
    ))
    .bind(request.thread_id.as_str())
    .bind(target.id.as_str())
    .bind(&preview.id)
    .bind(provider_history_action)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(reverted_turn_ids)
}

async fn enqueue_invalidated_checkpoints(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &ChatThreadId,
    target: &StoredCheckpoint,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let rows = sqlx::query(
        "SELECT id, hidden_ref_name, git_object_id FROM chat_checkpoints
         WHERE thread_id = ? AND turn_count > ? AND status = 'available'",
    )
    .bind(thread_id.as_str())
    .bind(i64_value(target.turn_count)?)
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for row in rows {
        let checkpoint_id: String = row.try_get("id").map_err(persistence_error)?;
        let reference: String = row.try_get("hidden_ref_name").map_err(persistence_error)?;
        let object_id: String = row.try_get("git_object_id").map_err(persistence_error)?;
        sqlx::query(
            "INSERT OR IGNORE INTO chat_cleanup_jobs
                (id, source_thread_id, cleanup_kind, exact_target, repository_identity,
                 state, available_at, created_at, updated_at, working_folder_id, expected_object_id)
             VALUES (?, ?, 'checkpoint_ref', ?, ?, 'pending', ?, ?, ?, (
                SELECT working_folder_id FROM chat_threads WHERE id = ?
             ), ?)",
        )
        .bind(format!("cleanup:checkpoint:{checkpoint_id}"))
        .bind(thread_id.as_str())
        .bind(&reference)
        .bind(&target.repository_identity)
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(thread_id.as_str())
        .bind(&object_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    sqlx::query(
        "UPDATE chat_checkpoints
         SET status = 'invalid', invalidated_at = ?, invalidated_by_checkpoint_id = ?,
             cleanup_state = 'queued'
         WHERE thread_id = ? AND turn_count > ? AND status = 'available'",
    )
    .bind(now.as_str())
    .bind(target.id.as_str())
    .bind(thread_id.as_str())
    .bind(i64_value(target.turn_count)?)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn append_reverted_event(
    transaction: &mut Transaction<'_, Sqlite>,
    request: &ExecuteChatRestoreRequest,
    target: &StoredCheckpoint,
    reverted_turn_ids: &[ChatTurnId],
    provider_history_action: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let thread = sqlx::query(
        "SELECT last_event_sequence, provider_family_id, provider_instance_id
         FROM chat_threads WHERE id = ?",
    )
    .bind(request.thread_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let sequence = u64::try_from(
        thread
            .try_get::<i64, _>("last_event_sequence")
            .map_err(persistence_error)?,
    )
    .map_err(|_| corrupt_data_error())?
    .saturating_add(1);
    let event = CanonicalEvent::ThreadReverted(ThreadRevertedEvent {
        checkpoint_id: target.id.clone(),
        reverted_turn_ids: reverted_turn_ids.to_vec(),
        provider_history_action: provider_history_action.to_string(),
    });
    sqlx::query(
        "INSERT INTO chat_events
            (id, thread_id, sequence, event_schema_version, provider_family_id,
             provider_instance_id, event_type, payload_schema_version, payload_data,
             created_at, ingested_at)
         VALUES (?, ?, ?, 1, ?, ?, 'thread_reverted', 1, ?, ?, ?)",
    )
    .bind(format!(
        "event:restore:{}",
        request.command.client_command_id.as_str()
    ))
    .bind(request.thread_id.as_str())
    .bind(i64_value(sequence)?)
    .bind(
        thread
            .try_get::<String, _>("provider_family_id")
            .map_err(persistence_error)?,
    )
    .bind(
        thread
            .try_get::<String, _>("provider_instance_id")
            .map_err(persistence_error)?,
    )
    .bind(serde_json::to_string(&event).map_err(json_error)?)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_activities
            (id, thread_id, sequence_anchor, item_kind, status, title, detail,
             safe_metadata_data, source_event_type, completed_at, created_at, updated_at)
         VALUES (?, ?, ?, 'notice', 'completed', 'thread_reverted', NULL, ?,
                 'thread_reverted', ?, ?, ?)",
    )
    .bind(format!(
        "activity:restore:{}",
        request.command.client_command_id.as_str()
    ))
    .bind(request.thread_id.as_str())
    .bind(i64_value(sequence)?)
    .bind(
        serde_json::to_string(&json!({
            "checkpointId": target.id,
            "revertedTurnCount": reverted_turn_ids.len(),
            "providerHistoryAction": provider_history_action,
        }))
        .map_err(json_error)?,
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_threads
         SET last_event_sequence = ?, last_projected_sequence = ?
         WHERE id = ?",
    )
    .bind(i64_value(sequence)?)
    .bind(i64_value(sequence)?)
    .bind(request.thread_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn read_preview(
    pool: &SqlitePool,
    preview_id: &str,
) -> ChatResult<StoredRestorePreview> {
    let row = sqlx::query(
        "SELECT id, thread_id, checkpoint_id, expected_thread_revision,
                repository_identity, head_oid, head_ref, current_worktree_tree_oid,
                current_index_tree_oid, current_index_fingerprint, expires_at
         FROM chat_restore_previews WHERE id = ? AND state = 'ready'",
    )
    .bind(preview_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Checkpoint restore preview is unavailable",
            true,
        )
    })?;
    Ok(StoredRestorePreview {
        id: row.try_get("id").map_err(persistence_error)?,
        thread_id: ChatThreadId::new(
            row.try_get::<String, _>("thread_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        checkpoint_id: ChatCheckpointId::new(
            row.try_get::<String, _>("checkpoint_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        expected_thread_revision: u64::try_from(
            row.try_get::<i64, _>("expected_thread_revision")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        repository_identity: row
            .try_get("repository_identity")
            .map_err(persistence_error)?,
        current: CurrentGitSnapshot {
            worktree_commit_oid: String::new(),
            worktree_tree_oid: row
                .try_get("current_worktree_tree_oid")
                .map_err(persistence_error)?,
            index_tree_oid: row
                .try_get("current_index_tree_oid")
                .map_err(persistence_error)?,
            index_fingerprint: row
                .try_get("current_index_fingerprint")
                .map_err(persistence_error)?,
            head_oid: row.try_get("head_oid").map_err(persistence_error)?,
            head_ref: row.try_get("head_ref").map_err(persistence_error)?,
        },
        expires_at: UtcTimestamp::new(
            row.try_get::<String, _>("expires_at")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
    })
}

pub(super) async fn mark_preview_state(
    pool: &SqlitePool,
    preview_id: &str,
    state: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query("UPDATE chat_restore_previews SET state = ?, updated_at = ? WHERE id = ?")
        .bind(state)
        .bind(now.as_str())
        .bind(preview_id)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn settle_executing_preview_failure(
    pool: &SqlitePool,
    request: &ExecuteChatRestoreRequest,
    preview: &StoredRestorePreview,
    target: &StoredCheckpoint,
    failure_phase: RestoreFailurePhase,
    error: &ChatError,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let operation_id = format!(
        "restore-operation:{}",
        request.command.client_command_id.as_str()
    );
    let completed = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_restore_operations
         WHERE id = ? AND recovery_state = 'complete'",
    )
    .bind(&operation_id)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?
        == 1;
    let state = preview_state_after_failure(completed, failure_phase);
    let transition = sqlx::query(
        "UPDATE chat_restore_previews SET state = ?, updated_at = ?
         WHERE id = ? AND state = 'executing'",
    )
    .bind(state)
    .bind(now.as_str())
    .bind(&preview.id)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if transition.rows_affected() == 0
        || completed
        || failure_phase == RestoreFailurePhase::SafeRetry
    {
        return Ok(());
    }
    let provider_action = if failure_phase == RestoreFailurePhase::ProviderRollback {
        "rolled_back"
    } else {
        "fork_required"
    };
    sqlx::query(
        "INSERT OR IGNORE INTO chat_restore_operations
            (id, thread_id, checkpoint_id, preview_id, provider_history_action,
             recovery_state, error_code, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'recovery_required', ?, ?, ?)",
    )
    .bind(operation_id)
    .bind(request.thread_id.as_str())
    .bind(target.id.as_str())
    .bind(&preview.id)
    .bind(provider_action)
    .bind(format!("{:?}", error.code).to_lowercase())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(super) fn preview_state_after_failure(
    completed_operation: bool,
    failure_phase: RestoreFailurePhase,
) -> &'static str {
    if completed_operation {
        "completed"
    } else if failure_phase == RestoreFailurePhase::SafeRetry {
        "ready"
    } else {
        "failed"
    }
}

pub(super) async fn record_restore_operation(
    pool: &SqlitePool,
    request: &ExecuteChatRestoreRequest,
    target: &StoredCheckpoint,
    provider_action: &str,
    recovery_state: &str,
    error: Option<&ChatError>,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_restore_operations
            (id, thread_id, checkpoint_id, preview_id, provider_history_action,
             recovery_state, error_code, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(format!(
        "restore-operation:{}",
        request.command.client_command_id.as_str()
    ))
    .bind(request.thread_id.as_str())
    .bind(target.id.as_str())
    .bind(&request.preview_id)
    .bind(provider_action)
    .bind(recovery_state)
    .bind(error.map(|value| format!("{:?}", value.code).to_lowercase()))
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executing_preview_failures_never_remain_executing() {
        assert_eq!(
            preview_state_after_failure(false, RestoreFailurePhase::SafeRetry),
            "ready"
        );
        assert_eq!(
            preview_state_after_failure(false, RestoreFailurePhase::ProviderRollback),
            "failed"
        );
        assert_eq!(
            preview_state_after_failure(false, RestoreFailurePhase::WorkspaceRestore),
            "failed"
        );
        assert_eq!(
            preview_state_after_failure(true, RestoreFailurePhase::WorkspaceRestore),
            "completed"
        );
    }
}
