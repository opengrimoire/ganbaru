//! Assignment dispatch and live provider delivery.

use super::common::{new_id, serialization_error, sha256_hex};
use super::reads::read_policy;
use super::{chat_pool, identifier_error, now_timestamp, persistence_error, u64_value};
use ganbaru_chat_contracts::models::*;
use serde_json::json;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

pub(super) async fn dispatch_assignment_job(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let now = now_timestamp()?;
    let claim_token = new_id("dispatch-claim");
    let claimed = sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs
         SET state = 'claimed', claimed_at = ?, claim_token = ?,
             attempt_count = attempt_count + 1, updated_at = ?
         WHERE assignment_id = ? AND state = 'queued' AND available_at <= ?",
    )
    .bind(now.as_str())
    .bind(&claim_token)
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if claimed.rows_affected() != 1 {
        return Ok(());
    }
    let result = dispatch_claimed_assignment(
        app.clone(),
        db_url.clone(),
        &pool,
        &assignment_id,
        &claim_token,
    )
    .await;
    if let Err(error) = result {
        fail_assignment_dispatch(&pool, &assignment_id, &claim_token, &error).await?;
        return Err(error);
    }
    mark_initial_assignment_input_delivered(&pool, &assignment_id).await?;
    deliver_pending_assignment_inputs(app, db_url, &pool, &assignment_id).await
}

async fn mark_initial_assignment_input_delivered(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    sqlx::query(
        "UPDATE chat_work_assignment_inputs
         SET delivery_state = 'delivered', delivered_at = ?
         WHERE assignment_id = ? AND routing_kind IN ('trigger', 'follow_up')
           AND delivery_state = 'pending'",
    )
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn deliver_pending_assignment_inputs(
    app: tauri::AppHandle,
    db_url: String,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    let message_ids = sqlx::query_scalar::<_, String>(
        "SELECT message_item_id FROM chat_work_assignment_inputs
         WHERE assignment_id = ? AND routing_kind IN ('steer', 'queued_continuation')
           AND delivery_state = 'pending'
         ORDER BY ordinal",
    )
    .bind(assignment_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    for message_id in message_ids {
        deliver_assignment_input(
            app.clone(),
            db_url.clone(),
            assignment_id.clone(),
            ChatConversationItemId::new(message_id).map_err(identifier_error)?,
        )
        .await?;
    }
    Ok(())
}

async fn dispatch_claimed_assignment(
    app: tauri::AppHandle,
    db_url: String,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    claim_token: &str,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT
            assignment.reply_thread_id,
            assignment.teammate_id,
            assignment.triggering_message_item_id,
            assignment.state,
            context.serialized_text,
            authorization.id AS authorization_revision_id,
            authorization.teammate_policy_revision_id,
            authorization.requester_participant_id,
            authorization.destination_conversation_id,
            authorization.working_folder_id,
            authorization.execution_environment_id,
            authorization.resolved_runtime_approval_policy,
            authorization.scope_digest,
            authorization.decision_state,
            authorization.revoked_at AS authorization_revoked_at,
            policy.revision AS policy_revision,
            teammate.instructions,
            channel.project_id,
            project.status AS project_status,
            environment.scratch_generation_id,
            participant.archived_at,
            job.claim_token
         FROM chat_work_assignments assignment
         JOIN chat_assignment_context_packages context
           ON context.assignment_id = assignment.id AND context.revision = 1
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.assignment_id = assignment.id
          AND authorization.revision = (
            SELECT max(candidate.revision)
            FROM chat_assignment_authorization_revisions candidate
            WHERE candidate.assignment_id = assignment.id
          )
         JOIN chat_teammate_policy_revisions policy
           ON policy.id = authorization.teammate_policy_revision_id
         JOIN chat_teammates teammate ON teammate.participant_id = assignment.teammate_id
         JOIN chat_participants participant ON participant.id = assignment.teammate_id
         JOIN chat_channels channel
           ON channel.conversation_id = authorization.destination_conversation_id
         JOIN projects project ON project.id = channel.project_id
         JOIN chat_execution_environments environment
           ON environment.id = authorization.execution_environment_id
          AND environment.lifecycle_state = 'available'
          AND environment.archived_at IS NULL
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = authorization.destination_conversation_id
          AND membership.participant_id = assignment.teammate_id
          AND membership.removed_at IS NULL
         JOIN chat_teammate_channel_memberships channel_access
           ON channel_access.conversation_id = membership.conversation_id
          AND channel_access.teammate_id = membership.participant_id
         JOIN chat_access_profiles profile ON profile.id = channel_access.access_profile_id
         JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
         JOIN chat_assignment_dispatch_jobs job ON job.assignment_id = assignment.id
         WHERE assignment.id = ?
           AND CASE
                 WHEN channel_access.participate_inherits_profile = 1
                   THEN profile_revision.default_participate
                 ELSE channel_access.participate AND profile_revision.default_participate
               END = 1",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Work assignment was not found",
            true,
        )
    })?;
    if row
        .try_get::<String, _>("claim_token")
        .map_err(persistence_error)?
        != claim_token
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Assignment dispatch ownership changed",
            true,
        ));
    }
    if row
        .try_get::<String, _>("state")
        .map_err(persistence_error)?
        != "queued"
    {
        return Err(ChatError::new(
            ChatErrorCode::InvalidStateTransition,
            "Only queued assignments can start",
            true,
        ));
    }
    if row
        .try_get::<Option<String>, _>("archived_at")
        .map_err(persistence_error)?
        .is_some()
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The teammate is no longer available for organizational work",
            true,
        ));
    }
    if row
        .try_get::<String, _>("project_status")
        .map_err(persistence_error)?
        == "archived"
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Restore the project before starting queued AI work",
            true,
        ));
    }
    if row
        .try_get::<String, _>("decision_state")
        .map_err(persistence_error)?
        != "allowed"
        || row
            .try_get::<Option<String>, _>("authorization_revoked_at")
            .map_err(persistence_error)?
            .is_some()
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The assignment authorization decision blocks execution",
            true,
        ));
    }
    let teammate_id = ChatParticipantId::new(
        row.try_get::<String, _>("teammate_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let policy_revision = u64_value(row.try_get("policy_revision").map_err(persistence_error)?)?;
    let policy = read_policy(pool, &teammate_id, policy_revision).await?;
    let working_folder_id = row
        .try_get::<Option<String>, _>("working_folder_id")
        .map_err(persistence_error)?
        .map(ProjectWorkingFolderId::new)
        .transpose()
        .map_err(identifier_error)?;
    let scratch_generation_id: Option<String> = row
        .try_get("scratch_generation_id")
        .map_err(persistence_error)?;
    let has_native_target = working_folder_id.is_some() || scratch_generation_id.is_some();
    if working_folder_id.is_some() && scratch_generation_id.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The assignment execution target is invalid",
            false,
        ));
    }
    let execution_environment_id: Option<String> = row
        .try_get("execution_environment_id")
        .map_err(persistence_error)?;
    if has_native_target != execution_environment_id.is_some() {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The assignment execution target is invalid",
            false,
        ));
    }
    let destination_conversation_id: String = row
        .try_get("destination_conversation_id")
        .map_err(persistence_error)?;
    let requester_participant_id: String = row
        .try_get("requester_participant_id")
        .map_err(persistence_error)?;
    let authorization_revision_id = ChatAuthorizationRevisionId::new(
        row.try_get::<String, _>("authorization_revision_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    if let Some(working_folder_id) = working_folder_id.as_ref() {
        require_live_folder_target(
            pool,
            authorization_revision_id.as_str(),
            &destination_conversation_id,
            teammate_id.as_str(),
            working_folder_id.as_str(),
        )
        .await?;
    }
    if let Some(scratch_generation_id) = scratch_generation_id.as_deref() {
        if !super::super::scratch::generation_constraints_hold(
            pool,
            scratch_generation_id,
            &destination_conversation_id,
            teammate_id.as_str(),
            &requester_participant_id,
        )
        .await?
        {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Private scratch contains context that is no longer authorized",
                false,
            ));
        }
    }
    let reply_thread_id = ChatReplyThreadId::new(
        row.try_get::<String, _>("reply_thread_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let triggering_message_item_id = ChatConversationItemId::new(
        row.try_get::<String, _>("triggering_message_item_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let authorization_scope_digest: String =
        row.try_get("scope_digest").map_err(persistence_error)?;
    let continuation = sqlx::query(
        "SELECT thread.id, thread.revision
         FROM chat_agent_runs run
         JOIN chat_work_assignments previous_assignment
           ON previous_assignment.id = run.assignment_id
         JOIN chat_assignment_authorization_revisions previous_authorization
           ON previous_authorization.id = run.authorization_revision_id
          AND previous_authorization.assignment_id = run.assignment_id
          AND previous_authorization.scope_digest = run.authorization_scope_digest
          AND previous_authorization.decision_state = 'allowed'
          AND previous_authorization.revoked_at IS NULL
         JOIN chat_threads thread ON thread.id = run.provider_thread_id
         WHERE previous_assignment.reply_thread_id = ?
           AND previous_assignment.id != ?
           AND previous_assignment.state = 'completed'
           AND run.state = 'completed'
           AND run.working_folder_id IS ?
           AND run.scratch_generation_id IS ?
           AND run.execution_environment_id IS ?
           AND run.authorization_scope_digest = ?
           AND thread.provider_instance_id = ?
           AND thread.archived_at IS NULL
           AND thread.state NOT IN ('closed', 'error')
         ORDER BY previous_assignment.created_at DESC, run.run_ordinal DESC
         LIMIT 1",
    )
    .bind(reply_thread_id.as_str())
    .bind(assignment_id.as_str())
    .bind(
        working_folder_id
            .as_ref()
            .map(ProjectWorkingFolderId::as_str),
    )
    .bind(&scratch_generation_id)
    .bind(&execution_environment_id)
    .bind(&authorization_scope_digest)
    .bind(policy.provider_instance_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let existing_thread_id = continuation
        .as_ref()
        .map(|thread| thread.try_get::<String, _>("id"))
        .transpose()
        .map_err(persistence_error)?
        .map(ChatThreadId::new)
        .transpose()
        .map_err(identifier_error)?;
    let expected_thread_revision = continuation
        .as_ref()
        .map(|thread| thread.try_get::<i64, _>("revision"))
        .transpose()
        .map_err(persistence_error)?
        .map(u64_value)
        .transpose()?;
    let provider_execution_thread_id = existing_thread_id
        .clone()
        .unwrap_or(ChatThreadId::new(new_id("provider-execution")).map_err(identifier_error)?);
    let attachment_ids = sqlx::query_scalar::<_, String>(
        "SELECT attachment.attachment_id
         FROM chat_work_assignments assignment
         JOIN chat_communication_messages message
           ON message.item_id = assignment.triggering_message_item_id
         JOIN chat_communication_attachment_references attachment
           ON attachment.message_revision_id = message.current_revision_id
         WHERE assignment.id = ? ORDER BY attachment.ordinal",
    )
    .bind(assignment_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .map(ChatAttachmentId::new)
    .collect::<Result<Vec<_>, _>>()
    .map_err(identifier_error)?;
    let mentions = if let Some(working_folder_id) = working_folder_id.as_ref() {
        sqlx::query(
            "SELECT path.relative_path, path.path_kind
         FROM chat_communication_messages message
         JOIN chat_message_references reference
           ON reference.message_revision_id = message.current_revision_id
          AND reference.reference_kind = 'workspace_path'
         JOIN chat_workspace_path_reference_targets path ON path.reference_id = reference.id
         WHERE message.item_id = ? AND path.working_folder_id = ?
         ORDER BY reference.ordinal",
        )
        .bind(triggering_message_item_id.as_str())
        .bind(working_folder_id.as_str())
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?
        .into_iter()
        .map(|resource| {
            Ok(WorkspaceMentionReference {
                relative_path: resource
                    .try_get("relative_path")
                    .map_err(persistence_error)?,
                kind: resource.try_get("path_kind").map_err(persistence_error)?,
            })
        })
        .collect::<ChatResult<Vec<_>>>()?
    } else {
        Vec::new()
    };
    let run_ordinal: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(run_ordinal), 0) + 1 FROM chat_agent_runs WHERE assignment_id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    let developer_instructions: String = row.try_get("instructions").map_err(persistence_error)?;
    let project_id: String = row.try_get("project_id").map_err(persistence_error)?;
    let request = super::super::send_commands::SendChatTurnCommand {
        command: ChatCommandContext {
            client_command_id: ChatCommandId::new(format!(
                "assignment-dispatch:{}:{}",
                assignment_id.as_str(),
                run_ordinal
            ))
            .map_err(identifier_error)?,
            expected_thread_revision,
        },
        working_folder_id: working_folder_id.clone(),
        thread_id: existing_thread_id,
        new_thread_id: (continuation.is_none()).then_some(provider_execution_thread_id),
        execution_environment_id: execution_environment_id.clone(),
        scratch_generation_id: scratch_generation_id.clone(),
        turn_id: ChatTurnId::new(new_id("provider-turn")).map_err(identifier_error)?,
        message_id: ChatMessageId::new(new_id("provider-message")).map_err(identifier_error)?,
        provider_instance_id: policy.provider_instance_id,
        provider_managed_model: policy.provider_managed_model,
        model_id: policy.model_id,
        model_options: policy.model_options,
        modes: assignment_turn_modes(policy.safety_mode, has_native_target),
        prompt: row.try_get("serialized_text").map_err(persistence_error)?,
        attachment_ids,
        mentions,
    };
    super::super::turns::send_turn(
        app,
        db_url,
        super::super::turns::SendChatTurnInvocation {
            command: request,
            origin: ganbaru_chat::agent_runs::TurnOrigin::Assignment {
                developer_instructions,
                run: Box::new(ganbaru_chat::agent_runs::AgentRunBinding {
                    run_id: ChatAgentRunId::new(new_id("agent-run")).map_err(identifier_error)?,
                    assignment_id: assignment_id.clone(),
                    project_id,
                    teammate_policy_revision_id: policy.id,
                    authorization_revision_id,
                    authorization_scope_digest,
                    working_folder_id: working_folder_id
                        .as_ref()
                        .map(|value| value.as_str().to_string()),
                    scratch_generation_id,
                    execution_environment_id,
                    run_ordinal: u64_value(run_ordinal)?,
                }),
            },
        },
    )
    .await?;
    Ok(())
}

async fn require_live_folder_target(
    pool: &SqlitePool,
    authorization_revision_id: &str,
    destination_conversation_id: &str,
    teammate_id: &str,
    working_folder_id: &str,
) -> ChatResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM chat_assignment_authorized_folder_sources frozen
           JOIN chat_teammate_working_folder_grants live
             ON live.conversation_id = ?
            AND live.teammate_id = ?
            AND live.working_folder_id = frozen.working_folder_id
            AND live.revoked_at IS NULL
           JOIN chat_teammate_channel_memberships channel_access
             ON channel_access.conversation_id = live.conversation_id
            AND channel_access.teammate_id = live.teammate_id
           JOIN chat_access_profiles profile ON profile.id = channel_access.access_profile_id
           JOIN chat_access_profile_revisions profile_revision
             ON profile_revision.access_profile_id = profile.id
            AND profile_revision.revision = profile.latest_revision
           WHERE frozen.authorization_revision_id = ?
             AND frozen.working_folder_id = ?
             AND frozen.is_execution_target = 1
             AND CASE frozen.capability
                   WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                   WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 5
                 END <= CASE
                   WHEN live.capability_inherits_profile = 1 THEN
                     CASE profile_revision.maximum_folder_capability
                       WHEN 'none' THEN 0 WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                       WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 0
                     END
                   ELSE min(
                     CASE live.capability
                       WHEN 'none' THEN 0 WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                       WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 0
                     END,
                     CASE profile_revision.maximum_folder_capability
                       WHEN 'none' THEN 0 WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                       WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 0
                     END
                   )
                 END
         )",
    )
    .bind(destination_conversation_id)
    .bind(teammate_id)
    .bind(authorization_revision_id)
    .bind(working_folder_id)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if valid {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "The execution folder is no longer authorized for this assignment",
            false,
        ))
    }
}

async fn fail_assignment_dispatch(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    claim_token: &str,
    error: &ChatError,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs
         SET state = 'failed', last_error = ?, updated_at = ?
         WHERE assignment_id = ? AND claim_token = ?",
    )
    .bind(&error.message)
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .bind(claim_token)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'failed', state_reason = ?, settled_at = ?,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND state = 'queued'",
    )
    .bind(&error.message)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    insert_dispatch_failure_message(
        &mut transaction,
        assignment_id,
        claim_token,
        &error.message,
        &now,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)
}

async fn insert_dispatch_failure_message(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    claim_token: &str,
    message: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT assignment.reply_thread_id, assignment.teammate_id, thread.conversation_id,
                participant.display_name AS author_label_snapshot
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         JOIN chat_participants participant ON participant.id = assignment.teammate_id
         WHERE assignment.id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let reply_thread_id: String = row.try_get("reply_thread_id").map_err(persistence_error)?;
    let teammate_id: String = row.try_get("teammate_id").map_err(persistence_error)?;
    let conversation_id: String = row.try_get("conversation_id").map_err(persistence_error)?;
    let author_label_snapshot: String = row
        .try_get("author_label_snapshot")
        .map_err(persistence_error)?;
    let hash = sha256_hex(format!("{}:{claim_token}", assignment_id.as_str()).as_bytes());
    let item_id = format!("organizational-item:{hash}");
    let revision_id = format!("organizational-revision:{hash}");
    let ordinal: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(ordinal), 0) + 1 FROM chat_conversation_items
         WHERE reply_thread_id = ?",
    )
    .bind(&reply_thread_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, ordinal, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)",
    )
    .bind(&item_id)
    .bind(&conversation_id)
    .bind(&reply_thread_id)
    .bind(ordinal)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_messages
            (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&item_id)
    .bind(&teammate_id)
    .bind(&author_label_snapshot)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let rich_content = json!({
        "type": "agent_update",
        "updateKind": "failure",
        "assignmentId": assignment_id,
        "agentRunId": null,
        "payload": { "launchFailure": true },
    });
    let rich_content_data = serde_json::to_string(&rich_content).map_err(serialization_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, revision, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, 1, ?, 1, ?, ?)",
    )
    .bind(&revision_id)
    .bind(&item_id)
    .bind(message)
    .bind(&rich_content_data)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(&revision_id)
        .bind(&item_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_semantic_updates
            (item_id, assignment_id, update_kind, payload_data, created_at)
         VALUES (?, ?, 'failure', ?, ?)",
    )
    .bind(&item_id)
    .bind(assignment_id.as_str())
    .bind(&rich_content_data)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_reply_threads SET reply_count = reply_count + 1,
             last_activity_at = ?, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&reply_thread_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_conversations SET last_activity_at = ?, revision = revision + 1,
             updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&conversation_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub(super) async fn deliver_assignment_input(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    message_item_id: ChatConversationItemId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let carries_new_authority: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM chat_communication_messages message
           JOIN chat_message_references reference
             ON reference.message_revision_id = message.current_revision_id
           WHERE message.item_id = ?
             AND reference.reference_kind IN (
               'channel', 'working_folder', 'workspace_path', 'execution_environment'
             )
         )",
    )
    .bind(message_item_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    if carries_new_authority {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Resource references require a linked assignment after current work settles",
            true,
        ));
    }
    let row = sqlx::query(
        "SELECT run.provider_thread_id, run.scratch_generation_id,
                run.authorization_revision_id, revision.normalized_markdown
         FROM chat_agent_runs run
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = run.authorization_revision_id
          AND authorization.assignment_id = run.assignment_id
          AND authorization.scope_digest = run.authorization_scope_digest
          AND authorization.decision_state = 'allowed'
          AND authorization.revoked_at IS NULL
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = authorization.destination_conversation_id
          AND membership.participant_id = assignment.teammate_id
          AND membership.removed_at IS NULL
         JOIN chat_teammate_channel_memberships channel_access
           ON channel_access.conversation_id = membership.conversation_id
          AND channel_access.teammate_id = membership.participant_id
         JOIN chat_access_profiles profile ON profile.id = channel_access.access_profile_id
         JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
         JOIN chat_work_assignment_inputs input ON input.assignment_id = run.assignment_id
         JOIN chat_communication_messages message ON message.item_id = input.message_item_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE run.assignment_id = ? AND input.message_item_id = ?
           AND run.state = 'working' AND input.delivery_state = 'pending'
           AND CASE
                 WHEN channel_access.participate_inherits_profile = 1
                   THEN profile_revision.default_participate
                 ELSE channel_access.participate AND profile_revision.default_participate
               END = 1
         ORDER BY run.run_ordinal DESC LIMIT 1",
    )
    .bind(assignment_id.as_str())
    .bind(message_item_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(());
    };
    let thread_id = ChatThreadId::new(
        row.try_get::<String, _>("provider_thread_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    if let Some(scratch_generation_id) = row
        .try_get::<Option<String>, _>("scratch_generation_id")
        .map_err(persistence_error)?
    {
        let authorization_revision_id: String = row
            .try_get("authorization_revision_id")
            .map_err(persistence_error)?;
        super::super::scratch::require_reusable_generation(
            &pool,
            &scratch_generation_id,
            &authorization_revision_id,
        )
        .await?;
    }
    super::super::turns::steer_turn(
        app,
        db_url,
        super::super::send_commands::SteerChatTurnCommand {
            command: ChatCommandContext {
                client_command_id: ChatCommandId::new(format!(
                    "assignment-steer:{}:{}",
                    assignment_id.as_str(),
                    message_item_id.as_str()
                ))
                .map_err(identifier_error)?,
                expected_thread_revision: None,
            },
            thread_id,
            message_id: ChatMessageId::new(new_id("provider-steer-message"))
                .map_err(identifier_error)?,
            prompt: row
                .try_get("normalized_markdown")
                .map_err(persistence_error)?,
        },
    )
    .await?;
    sqlx::query(
        "UPDATE chat_work_assignment_inputs SET delivery_state = 'delivered', delivered_at = ?
         WHERE assignment_id = ? AND message_item_id = ? AND delivery_state = 'pending'",
    )
    .bind(now_timestamp()?.as_str())
    .bind(assignment_id.as_str())
    .bind(message_item_id.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn teammate_cli_safety_mode(value: ChatApprovalPolicy) -> SafetyMode {
    match value {
        ChatApprovalPolicy::AskForApproval => SafetyMode::AskForApproval,
        ChatApprovalPolicy::ApproveForMe => SafetyMode::ApproveForMe,
        ChatApprovalPolicy::FullAccess => SafetyMode::FullAccess,
        ChatApprovalPolicy::Custom => SafetyMode::Custom,
    }
}

fn assignment_turn_modes(
    teammate_approval: ChatApprovalPolicy,
    has_native_target: bool,
) -> TurnModeSnapshot {
    if has_native_target {
        return TurnModeSnapshot {
            safety_mode: teammate_cli_safety_mode(teammate_approval),
            interaction_mode: InteractionMode::Build,
        };
    }
    TurnModeSnapshot {
        safety_mode: SafetyMode::AskForApproval,
        interaction_mode: InteractionMode::Build,
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatApprovalPolicy, InteractionMode, SafetyMode, assignment_turn_modes};

    #[test]
    fn targetless_assignments_use_a_narrow_conversation_mode() {
        let modes = assignment_turn_modes(ChatApprovalPolicy::FullAccess, false);
        assert_eq!(modes.safety_mode, SafetyMode::AskForApproval);
        assert_eq!(modes.interaction_mode, InteractionMode::Build);
    }

    #[test]
    fn native_assignments_keep_the_teammate_cli_approval_behavior() {
        let modes = assignment_turn_modes(ChatApprovalPolicy::FullAccess, true);
        assert_eq!(modes.safety_mode, SafetyMode::FullAccess);
        assert_eq!(modes.interaction_mode, InteractionMode::Build);
    }
}
