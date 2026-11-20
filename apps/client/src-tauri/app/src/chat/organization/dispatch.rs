//! Assignment dispatch and live provider delivery.

use super::assignments::read_assignment_subject;
use super::common::MAX_MESSAGE_BYTES;
use super::common::{
    native_assignment_safety_mode, new_id, parse_runtime_approval_policy, this_device_id,
};
use super::context;
use super::continuation::{self, CandidateRun, ContinuationKey, TransitionInput};
use super::delegation;
use super::inputs::place_delivered_reply;
use super::instruction_files;
use super::provider_agents::{DeviceProviders, read_device_providers};
use super::{chat_pool, identifier_error, now_timestamp, persistence_error, u64_value};
use ganbaru_chat::agent_runs::{
    AttemptReason, ContextDelivery, DeliveredSource, DeliveryKind, FullPackage, LaunchSettlement,
    RunAttempt, RunLaunch,
};
use ganbaru_chat::coordination::access::{access_ceiling, effective_access_level_sql};
use ganbaru_chat::coordination::reports::{AnsweredQuestion, read_answered_question};
use ganbaru_chat::coordination::revocation::PARENT_AUTHORITY_REVOKED_REASON;
use ganbaru_chat_contracts::model_recommendation::newest_model_of_line;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, SqlitePool};

/// Claims and runs the queued dispatch job of `assignment_id`. A dispatch that settled the
/// assignment instead of starting it (a parent that lost its authority, a refused or failed
/// launch) delivers its delegation outcome to a parent running here.
pub(super) async fn dispatch_assignment_job(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let result = claim_and_dispatch(app.clone(), db_url, &pool, &assignment_id).await;
    super::assignments::deliver_delegation_outcomes_of_assignments(
        &app,
        &pool,
        std::collections::BTreeSet::from([assignment_id.as_str().to_owned()]),
    );
    result
}

async fn claim_and_dispatch(
    app: tauri::AppHandle,
    db_url: String,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    let device_id = this_device_id(&app)?;
    let now = now_timestamp()?;
    let Some(claim) = claim_dispatch_job(
        pool,
        assignment_id,
        &device_id,
        &read_device_providers(&app),
        &now,
    )
    .await?
    else {
        return Ok(());
    };
    let mut result = dispatch_claimed_assignment(
        app.clone(),
        db_url.clone(),
        pool,
        assignment_id,
        &claim,
        false,
    )
    .await;
    if result
        .as_ref()
        .is_ok_and(|launch| launch.settlement == Some(LaunchSettlement::HandoffRequired))
    {
        // The continued native session was lost, so the next run opens a new session with the
        // full package. That run is a handoff, which never requires another one.
        result = dispatch_claimed_assignment(
            app.clone(),
            db_url.clone(),
            pool,
            assignment_id,
            &claim,
            true,
        )
        .await;
    }
    let launch = match result {
        Ok(launch) => launch,
        Err(error) => {
            return match settle_dispatch_error(pool, assignment_id, &claim.token, &error).await? {
                DispatchSettlement::Failed => Err(error),
                DispatchSettlement::Waiting => Ok(()),
            };
        }
    };
    if !mark_initial_assignment_input_delivered(pool, assignment_id, &launch).await? {
        return Ok(());
    }
    deliver_pending_assignment_inputs(app, db_url, pool, assignment_id).await
}

/// The run a dispatch launched and how its launch ended.
#[derive(Debug)]
pub(super) struct AssignmentLaunch {
    pub(super) run_id: ChatAgentRunId,
    /// Set when the provider refused or never received the turn. Unless the settlement asks for
    /// a handoff, it already failed the assignment and posted its failure message.
    pub(super) launch_error: Option<ChatError>,
    /// How the launch settled for the assignment, absent when the send replayed a receipt.
    pub(super) settlement: Option<LaunchSettlement>,
    /// The pending answers to a host question that the first turn of the run carried.
    pub(super) answered_input_ids: Vec<String>,
}

/// A claimed dispatch job and the system-level instructions its run sends, composed from the
/// subject it read at claim.
#[derive(Debug)]
pub(super) struct ClaimedDispatch {
    pub(super) token: String,
    /// The agent framing and the instructions the assignment froze.
    pub(super) instructions: String,
    /// The model the assignment froze.
    pub(super) model_selection: ChatModelSelection,
    /// The provider installation that runs the agent on this device.
    pub(super) provider_instance_id: ProviderInstanceId,
}

/// Claims the queued dispatch job of `assignment_id` when the device `device_id` executes the
/// assignment. An assignment without an executing device runs on the device that holds its
/// device-local dispatch job.
///
/// When the subject cannot be read (as for an archived agent) or names no provider installation
/// here, the job is dropped so the assignment stays queued until recovery can start it again.
/// Narrowed access needs no check here: it revokes the frozen authorization, which cancels the
/// queued work and refuses its launch.
pub(super) async fn claim_dispatch_job(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device_id: &str,
    providers: &DeviceProviders,
    now: &UtcTimestamp,
) -> ChatResult<Option<ClaimedDispatch>> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    let queued: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_assignment_dispatch_jobs job
           JOIN chat_work_assignments assignment ON assignment.id = job.assignment_id
           WHERE job.assignment_id = ?1 AND job.state = 'queued' AND job.available_at <= ?2
             AND coalesce(assignment.executing_device_id, ?3) = ?3
         )",
    )
    .bind(assignment_id.as_str())
    .bind(now.as_str())
    .bind(device_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if !queued {
        return Ok(None);
    }
    let subject = match read_assignment_subject(&mut transaction, assignment_id, providers).await {
        Ok(subject) => Some(subject),
        Err(error) if error.code == ChatErrorCode::Persistence => return Err(error),
        Err(_) => None,
    };
    let Some((subject, provider_instance_id)) = subject.and_then(|subject| {
        let instance = ProviderInstanceId::new(subject.provider_instance_id()).ok()?;
        Some((subject, instance))
    }) else {
        sqlx::query("DELETE FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?")
            .bind(assignment_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(None);
    };
    if delegated_authority_lost(&mut transaction, assignment_id).await? {
        delegation::cancel_for_revoked_parent(&mut transaction, assignment_id.as_str(), now)
            .await?;
        sqlx::query("DELETE FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?")
            .bind(assignment_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        ganbaru_chat::coordination::revocation::revoke_assignment_authorizations(
            &mut transaction,
            assignment_id.as_str(),
            now.as_str(),
            PARENT_AUTHORITY_REVOKED_REASON,
        )
        .await?;
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(None);
    }
    let claim_token = new_id("dispatch-claim")?;
    sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs
         SET state = 'claimed', claimed_at = ?1, claim_token = ?2,
             attempt_count = attempt_count + 1, updated_at = ?1
         WHERE assignment_id = ?3",
    )
    .bind(now.as_str())
    .bind(&claim_token)
    .bind(assignment_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(Some(ClaimedDispatch {
        token: claim_token,
        instructions: continuation::compose_system_instructions(
            &subject.display_name,
            &subject.instructions,
        ),
        model_selection: subject.model_selection,
        provider_instance_id,
    }))
}

/// Whether `assignment_id` was delegated and no longer fits its ancestors' authority: an
/// ancestor was revoked or cancelled, or its frozen authorization now exceeds what they hold.
async fn delegated_authority_lost(
    connection: &mut sqlx::SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<bool> {
    match delegation::read_delegation_bound(connection, assignment_id.as_str()).await? {
        delegation::DelegationBound::Undelegated => Ok(false),
        delegation::DelegationBound::Revoked => Ok(true),
        delegation::DelegationBound::Bound(ceiling) => {
            delegation::frozen_authority_exceeds(connection, assignment_id.as_str(), &ceiling).await
        }
    }
}

/// Marks the trigger and follow-up inputs of `assignment_id`, which its context package carries,
/// and the answers the first turn carried, delivered once `launch` started its run. Returns
/// whether the run started.
///
/// The delivered revision is the one the package froze, not the message's current revision, so
/// a reply edited after the freeze reads as edited after delivery; an input the package does not
/// carry records no revision. A failed launch, or a run that settled before it started,
/// delivered nothing, so its inputs stay pending for Retry.
async fn mark_initial_assignment_input_delivered(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    launch: &AssignmentLaunch,
) -> ChatResult<bool> {
    if launch.launch_error.is_some() {
        return Ok(false);
    }
    let started: Option<bool> = sqlx::query_scalar(
        "SELECT started_at IS NOT NULL FROM chat_agent_runs WHERE id = ? AND assignment_id = ?",
    )
    .bind(launch.run_id.as_str())
    .bind(assignment_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    if started != Some(true) {
        return Ok(false);
    }
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_work_assignment_inputs
         SET delivery_state = 'delivered', delivered_at = ?,
             delivered_revision_id = (
               SELECT revision.id
               FROM chat_assignment_context_packages package
               JOIN chat_assignment_context_sources source
                 ON source.context_package_id = package.id
                AND source.source_id = chat_work_assignment_inputs.message_item_id
                AND source.source_kind IN ('trigger', 'thread_reply')
               JOIN chat_communication_message_revisions_numbered revision
                 ON revision.message_item_id = source.source_id
                AND revision.revision = source.source_revision
               WHERE package.assignment_id = chat_work_assignment_inputs.assignment_id
                 AND package.revision = 1
               ORDER BY source.source_kind = 'trigger' DESC
               LIMIT 1
             )
         WHERE assignment_id = ? AND routing_kind IN ('trigger', 'follow_up')
           AND delivery_state = 'pending'",
    )
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for message_item_id in &launch.answered_input_ids {
        let delivered = sqlx::query(
            "UPDATE chat_work_assignment_inputs
             SET delivery_state = 'delivered', delivered_at = ?
             WHERE assignment_id = ? AND message_item_id = ?
               AND routing_kind IN ('steer', 'queued_continuation')
               AND delivery_state = 'pending'",
        )
        .bind(now.as_str())
        .bind(assignment_id.as_str())
        .bind(message_item_id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?
        .rows_affected();
        if delivered > 0 {
            place_delivered_reply(&mut transaction, message_item_id, now.as_str()).await?;
        }
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(true)
}

pub(super) async fn deliver_pending_assignment_inputs(
    app: tauri::AppHandle,
    db_url: String,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    for message_id in pending_live_input_ids(pool, assignment_id).await? {
        deliver_assignment_input(
            app.clone(),
            db_url.clone(),
            assignment_id.clone(),
            message_id,
        )
        .await?;
    }
    Ok(())
}

/// Pending steer inputs of `assignment_id` that no one holds, in queue order. These reach a run
/// only while it works, unlike the trigger and follow-ups its package carries; a queued continuation waits
/// for the turn to end, when follow-up recovery continues the work with it.
async fn pending_live_input_ids(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Vec<ChatConversationItemId>> {
    sqlx::query_scalar::<_, String>(
        "SELECT message_item_id FROM chat_work_assignment_inputs
         WHERE assignment_id = ? AND routing_kind = 'steer'
           AND delivery_state = 'pending' AND held = 0
         ORDER BY ordinal, id",
    )
    .bind(assignment_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?
    .into_iter()
    .map(|message_id| ChatConversationItemId::new(message_id).map_err(identifier_error))
    .collect()
}

async fn dispatch_claimed_assignment(
    app: tauri::AppHandle,
    db_url: String,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    claim: &ClaimedDispatch,
    native_session_lost: bool,
) -> ChatResult<AssignmentLaunch> {
    let row = read_dispatch_row(pool, assignment_id).await?;
    if row
        .try_get::<String, _>("claim_token")
        .map_err(persistence_error)?
        != claim.token
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
            "The agent is no longer available for organizational work",
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
    let agent_id = ChatParticipantId::new(
        row.try_get::<String, _>("agent_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let frozen_runtime_approval: String = row
        .try_get("resolved_runtime_approval_policy")
        .map_err(persistence_error)?;
    let resolved_runtime_approval = parse_runtime_approval_policy(&frozen_runtime_approval)?;
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
            agent_id.as_str(),
            working_folder_id.as_str(),
        )
        .await?;
    }
    if let Some(scratch_generation_id) = scratch_generation_id.as_deref() {
        if !super::super::scratch::generation_constraints_hold(
            pool,
            scratch_generation_id,
            &destination_conversation_id,
            agent_id.as_str(),
            &requester_participant_id,
            Some(&frozen_runtime_approval),
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
    let run_ordinal: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(run_ordinal), 0) + 1 FROM chat_agent_runs WHERE assignment_id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    let catalog = crate::chat::device_state::read_active_device_scope(&app)
        .ok()
        .and_then(|scope| {
            scope
                .provider_instances
                .get(&claim.provider_instance_id)
                .and_then(|device| device.model_catalog.clone())
        });
    let model = dispatch_model(&claim.model_selection, catalog.as_ref());
    // Instruction files join before the key is computed, so a changed file starts a fresh
    // native session that receives it. Talk work reads no files, so it joins none.
    let instructions = if has_native_target {
        instruction_files::with_instruction_files(
            &app,
            pool,
            &claim.provider_instance_id,
            working_folder_id.as_ref(),
            execution_environment_id.as_deref(),
            &claim.instructions,
        )
        .await
    } else {
        claim.instructions.clone()
    };
    let key = ContinuationKey {
        authorization_scope_digest: authorization_scope_digest.clone(),
        provider_instance_id: claim.provider_instance_id.as_str().to_string(),
        model_selection_sha256: continuation::model_selection_sha256(
            model.selection.provider_managed_model,
            model.selection.model_id.as_ref(),
            &model.selection.model_options,
        )?,
        instructions_sha256: continuation::instructions_sha256(&instructions),
    };
    let answered = if run_ordinal > 1 {
        let mut connection = pool.acquire().await.map_err(persistence_error)?;
        read_answered_question(&mut connection, assignment_id.as_str()).await?
    } else {
        None
    };
    let candidate_scope = CandidateScope {
        assignment_id,
        reply_thread_id: &reply_thread_id,
        agent_id: &agent_id,
        working_folder_id: working_folder_id.as_ref(),
        scratch_generation_id: scratch_generation_id.as_deref(),
        execution_environment_id: execution_environment_id.as_deref(),
    };
    // Work that asked a question and completed its run continues that run's session, which holds
    // the question, so the answer reaches the session that asked it.
    let candidate = match answered.as_ref() {
        Some(answered) => {
            read_own_continuation_candidate(pool, &candidate_scope, &answered.asking_run_id).await?
        }
        None => read_continuation_candidate(pool, &candidate_scope).await?,
    };
    let decision = continuation::decide_session_transition(&TransitionInput {
        key: &key,
        unfinished_earlier_run: run_ordinal > 1 && answered.is_none(),
        candidate: candidate.as_ref().map(StoredCandidate::as_candidate),
        native_session_lost,
    });
    let (existing_thread_id, expected_thread_revision) = match (decision.reuse_thread, candidate) {
        (Some(thread_id), Some(candidate)) => (
            Some(ChatThreadId::new(thread_id).map_err(identifier_error)?),
            Some(candidate.revision),
        ),
        _ => (None, None),
    };
    let new_thread_id = match existing_thread_id {
        Some(_) => None,
        None => Some(ChatThreadId::new(new_id("provider-execution")?).map_err(identifier_error)?),
    };
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
    let developer_instructions = instructions;
    let project_id: String = row.try_get("project_id").map_err(persistence_error)?;
    let context_package_id: String = row
        .try_get("context_package_id")
        .map_err(persistence_error)?;
    let full_package: String = row.try_get("serialized_text").map_err(persistence_error)?;
    let full = FullPackage {
        delivered_sources: package_sources(pool, &context_package_id, &full_package).await?,
        prompt: full_package,
    };
    let answer = answered
        .as_ref()
        .map(frame_answered_question)
        .transpose()?
        .filter(|answer| answer.fits_after(&full.prompt));
    let delta = match (
        decision.reason,
        existing_thread_id.as_ref(),
        answer.as_ref(),
    ) {
        (AttemptReason::Continuation, Some(_), Some(answer)) => continuation::answer_delta(
            assignment_id,
            u64::try_from(full.delivered_sources.len()).unwrap_or(u64::MAX),
            &answer.answers,
            answer.sources.clone(),
        )?,
        (AttemptReason::Continuation, Some(thread_id), None) => {
            continuation::build_delta_package(
                pool,
                assignment_id,
                &context_package_id,
                &full.prompt,
                thread_id.as_str(),
            )
            .await?
        }
        _ => None,
    };
    let answered_input_ids = answer
        .as_ref()
        .map(|answer| answer.input_ids.clone())
        .unwrap_or_default();
    let full = match answer {
        Some(answer) => answer.after_package(full),
        None => full,
    };
    let delivery = choose_context_delivery(decision.reason, delta, full);
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
        new_thread_id,
        execution_environment_id: execution_environment_id.clone(),
        scratch_generation_id: scratch_generation_id.clone(),
        turn_id: ChatTurnId::new(new_id("provider-turn")?).map_err(identifier_error)?,
        message_id: ChatMessageId::new(new_id("provider-message")?).map_err(identifier_error)?,
        provider_instance_id: claim.provider_instance_id.clone(),
        provider_managed_model: model.selection.provider_managed_model,
        model_id: model.selection.model_id,
        model_options: model.selection.model_options,
        modes: assignment_turn_modes(resolved_runtime_approval, has_native_target),
        prompt: delivery.prompt,
        attachment_ids,
        mentions,
    };
    let launch = RunLaunch {
        claim_token: claim.token.clone(),
        attempt: RunAttempt {
            reason: decision.reason,
            instructions_sha256: Some(key.instructions_sha256),
            model_selection_sha256: Some(key.model_selection_sha256),
            context_delivery: delivery.context_delivery,
            delivered_context_bytes: u64::try_from(request.prompt.len())
                .map_err(|_| ChatError::validation("prompt", "Assignment context is too large"))?,
            delivered_sources: delivery.delivered_sources,
        },
        full_prompt_on_fresh_session: delivery.full_prompt_on_fresh_session,
        model_label: model.label,
    };
    let run_id = ChatAgentRunId::new(new_id("agent-run")?).map_err(identifier_error)?;
    let sent = super::super::turns::send_turn(
        app,
        db_url,
        super::super::turns::SendChatTurnInvocation {
            command: request,
            origin: ganbaru_chat::agent_runs::TurnOrigin::Assignment {
                developer_instructions,
                run: Box::new(ganbaru_chat::agent_runs::AgentRunBinding {
                    run_id: run_id.clone(),
                    assignment_id: assignment_id.clone(),
                    project_id,
                    authorization_revision_id,
                    authorization_scope_digest,
                    working_folder_id: working_folder_id
                        .as_ref()
                        .map(|value| value.as_str().to_string()),
                    scratch_generation_id,
                    execution_environment_id,
                    run_ordinal: u64_value(run_ordinal)?,
                }),
                launch: Box::new(launch),
            },
        },
    )
    .await?;
    Ok(AssignmentLaunch {
        run_id,
        launch_error: sent.launch_error,
        settlement: sent.launch_settlement,
        answered_input_ids,
    })
}

/// Reads the frozen context, latest authorization, agent access, and dispatch job that the
/// dispatch of `assignment_id` starts from. Conversation-only work has no execution
/// environment; an authorization that names one starts only while it is available.
pub(super) async fn read_dispatch_row(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<sqlx::sqlite::SqliteRow> {
    sqlx::query(
        "SELECT
            assignment.reply_thread_id,
            assignment.agent_id,
            assignment.triggering_message_item_id,
            assignment.state,
            context.id AS context_package_id,
            context.serialized_text,
            authorization.id AS authorization_revision_id,
            authorization.requester_participant_id,
            authorization.destination_conversation_id,
            authorization.working_folder_id,
            authorization.execution_environment_id,
            authorization.resolved_runtime_approval_policy,
            authorization.scope_digest,
            authorization.decision_state,
            authorization.revoked_at AS authorization_revoked_at,
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
         JOIN chat_agents agent ON agent.participant_id = assignment.agent_id
         JOIN chat_participants participant ON participant.id = assignment.agent_id
         JOIN chat_channels channel
           ON channel.conversation_id = authorization.destination_conversation_id
         JOIN projects project ON project.id = channel.project_id
         LEFT JOIN chat_execution_environments environment
           ON environment.id = authorization.execution_environment_id
          AND environment.lifecycle_state = 'available'
          AND environment.archived_at IS NULL
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = authorization.destination_conversation_id
          AND membership.participant_id = assignment.agent_id
          AND membership.removed_at IS NULL
         JOIN chat_agent_channel_memberships channel_access
           ON channel_access.conversation_id = membership.conversation_id
          AND channel_access.agent_id = membership.participant_id
          AND channel_access.removed_at IS NULL
         JOIN chat_assignment_dispatch_jobs job ON job.assignment_id = assignment.id
         WHERE assignment.id = ?
           AND (authorization.execution_environment_id IS NULL OR environment.id IS NOT NULL)",
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
    })
}

/// A person's answers to a host question, framed for the first turn of the run that continues
/// the work.
#[derive(Debug)]
struct FramedAnswer {
    /// The question as a thread reply record, which a new session needs to read the answers.
    question: String,
    /// The answers as follow-up records, in posting order.
    answers: String,
    sources: Vec<DeliveredSource>,
    input_ids: Vec<String>,
}

impl FramedAnswer {
    /// Whether the question and answers fit after `package` in one turn.
    fn fits_after(&self, package: &str) -> bool {
        package.len() + self.question.len() + self.answers.len() <= MAX_MESSAGE_BYTES
    }

    /// The full package followed by the question and the answers, as a new session receives
    /// them.
    fn after_package(self, full: FullPackage) -> FullPackage {
        let mut delivered_sources = full.delivered_sources;
        delivered_sources.extend(self.sources);
        FullPackage {
            prompt: format!("{}{}{}", full.prompt, self.question, self.answers),
            delivered_sources,
        }
    }
}

/// Frames `answered` as context records: the question once, and each answer as the follow-up a
/// queued continuation would deliver.
fn frame_answered_question(answered: &AnsweredQuestion) -> ChatResult<FramedAnswer> {
    let question = context::asked_question_context_line(
        &answered.question.item_id,
        answered.question.revision,
        answered.question.markdown.clone(),
    )?;
    let mut answers = String::new();
    let mut sources = Vec::with_capacity(answered.answers.len());
    let mut input_ids = Vec::with_capacity(answered.answers.len());
    for answer in &answered.answers {
        let framed = context::follow_up_context_line(
            &answer.item_id,
            answer.revision,
            answer.markdown.clone(),
            false,
        )?;
        answers.push_str(&framed.line);
        sources.push(DeliveredSource {
            source_id: answer.item_id.clone(),
            content_sha256: framed.content_sha256,
            kind: DeliveryKind::QueuedContinuation,
        });
        input_ids.push(answer.item_id.clone());
    }
    Ok(FramedAnswer {
        question,
        answers,
        sources,
        input_ids,
    })
}

/// What the first turn of a run sends and records as delivered.
#[derive(Debug, Eq, PartialEq)]
struct PlannedDelivery {
    prompt: String,
    context_delivery: ContextDelivery,
    delivered_sources: Vec<DeliveredSource>,
    /// The full package a continuation sends instead when its session turns out to be new.
    full_prompt_on_fresh_session: Option<FullPackage>,
}

/// Chooses between the delta and the full package. A continuation with a delta sends it, and
/// every other run, or a continuation whose delta did not fit, sends the full package. A
/// continuation keeps the full package for a session the provider opens fresh, so that run
/// records a handoff instead of failing.
fn choose_context_delivery(
    reason: AttemptReason,
    delta: Option<continuation::DeltaPackage>,
    full: FullPackage,
) -> PlannedDelivery {
    let full_prompt_on_fresh_session =
        (reason == AttemptReason::Continuation).then(|| full.clone());
    match (reason, delta) {
        (AttemptReason::Continuation, Some(delta)) => PlannedDelivery {
            prompt: delta.text,
            context_delivery: ContextDelivery::Delta,
            delivered_sources: delta.sources,
            full_prompt_on_fresh_session,
        },
        _ => PlannedDelivery {
            prompt: full.prompt,
            context_delivery: ContextDelivery::Full,
            delivered_sources: full.delivered_sources,
            full_prompt_on_fresh_session,
        },
    }
}

/// The sources of the frozen context package `context_package_id`, whose text is `serialized`,
/// in package order, as sources a launch delivers. A pointer record delivers its source as a
/// pointer.
async fn package_sources(
    pool: &SqlitePool,
    context_package_id: &str,
    serialized: &str,
) -> ChatResult<Vec<DeliveredSource>> {
    Ok(
        continuation::read_package_records(pool, context_package_id, serialized)
            .await?
            .into_iter()
            .filter_map(|record| {
                record.source.map(|source| DeliveredSource {
                    source_id: source.source_id,
                    content_sha256: source.content_sha256,
                    kind: context::record_delivery_kind(record.line),
                })
            })
            .collect(),
    )
}

/// Where an assignment run executes, which a reusable earlier run must share.
struct CandidateScope<'a> {
    assignment_id: &'a ChatWorkAssignmentId,
    reply_thread_id: &'a ChatReplyThreadId,
    agent_id: &'a ChatParticipantId,
    working_folder_id: Option<&'a ProjectWorkingFolderId>,
    scratch_generation_id: Option<&'a str>,
    execution_environment_id: Option<&'a str>,
}

/// The newest completed run of an earlier assignment that a run in `CandidateScope` could
/// continue, as stored.
struct StoredCandidate {
    thread_id: String,
    revision: u64,
    key: Option<ContinuationKey>,
    authorization_valid: bool,
}

impl StoredCandidate {
    fn as_candidate(&self) -> CandidateRun<'_> {
        CandidateRun {
            thread_id: &self.thread_id,
            key: self.key.clone(),
            authorization_valid: self.authorization_valid,
        }
    }
}

/// Reads the newest completed run of another completed assignment of the same agent in the
/// same reply thread and execution target whose provider thread is still open. Its key and
/// authorization are returned rather than filtered, so the transition records why a new
/// session starts.
async fn read_continuation_candidate(
    pool: &SqlitePool,
    scope: &CandidateScope<'_>,
) -> ChatResult<Option<StoredCandidate>> {
    let Some(row) = sqlx::query(
        "SELECT thread.id AS thread_id, thread.revision, thread.provider_instance_id,
                run.authorization_scope_digest, run.model_selection_sha256,
                run.instructions_sha256,
                previous_authorization.id IS NOT NULL AS authorization_valid
         FROM chat_agent_runs run
         JOIN chat_work_assignments previous_assignment
           ON previous_assignment.id = run.assignment_id
         LEFT JOIN chat_assignment_authorization_revisions previous_authorization
           ON previous_authorization.id = run.authorization_revision_id
          AND previous_authorization.assignment_id = run.assignment_id
          AND previous_authorization.scope_digest = run.authorization_scope_digest
          AND previous_authorization.decision_state = 'allowed'
          AND previous_authorization.revoked_at IS NULL
         JOIN chat_threads thread ON thread.id = run.provider_thread_id
         WHERE previous_assignment.reply_thread_id = ?
           AND previous_assignment.id != ?
           AND previous_assignment.agent_id = ?
           AND previous_assignment.state = 'completed'
           AND run.state = 'completed'
           AND run.working_folder_id IS ?
           AND run.scratch_generation_id IS ?
           AND run.execution_environment_id IS ?
           AND thread.archived_at IS NULL
           AND thread.state NOT IN ('closed', 'error')
         ORDER BY previous_assignment.created_at DESC, run.run_ordinal DESC
         LIMIT 1",
    )
    .bind(scope.reply_thread_id.as_str())
    .bind(scope.assignment_id.as_str())
    .bind(scope.agent_id.as_str())
    .bind(scope.working_folder_id.map(ProjectWorkingFolderId::as_str))
    .bind(scope.scratch_generation_id)
    .bind(scope.execution_environment_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    else {
        return Ok(None);
    };
    stored_candidate(&row).map(Some)
}

/// Reads `asking_run_id`, the completed run of the same assignment that asked a question, when
/// its provider thread is still open and it ran on the same execution target, so the answer
/// continues the session that asked.
async fn read_own_continuation_candidate(
    pool: &SqlitePool,
    scope: &CandidateScope<'_>,
    asking_run_id: &str,
) -> ChatResult<Option<StoredCandidate>> {
    let Some(row) = sqlx::query(
        "SELECT thread.id AS thread_id, thread.revision, thread.provider_instance_id,
                run.authorization_scope_digest, run.model_selection_sha256,
                run.instructions_sha256,
                run_authorization.id IS NOT NULL AS authorization_valid
         FROM chat_agent_runs run
         LEFT JOIN chat_assignment_authorization_revisions run_authorization
           ON run_authorization.id = run.authorization_revision_id
          AND run_authorization.assignment_id = run.assignment_id
          AND run_authorization.scope_digest = run.authorization_scope_digest
          AND run_authorization.decision_state = 'allowed'
          AND run_authorization.revoked_at IS NULL
         JOIN chat_threads thread ON thread.id = run.provider_thread_id
         WHERE run.id = ?
           AND run.assignment_id = ?
           AND run.state = 'completed'
           AND run.working_folder_id IS ?
           AND run.scratch_generation_id IS ?
           AND run.execution_environment_id IS ?
           AND thread.archived_at IS NULL
           AND thread.state NOT IN ('closed', 'error')",
    )
    .bind(asking_run_id)
    .bind(scope.assignment_id.as_str())
    .bind(scope.working_folder_id.map(ProjectWorkingFolderId::as_str))
    .bind(scope.scratch_generation_id)
    .bind(scope.execution_environment_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    else {
        return Ok(None);
    };
    stored_candidate(&row).map(Some)
}

fn stored_candidate(row: &sqlx::sqlite::SqliteRow) -> ChatResult<StoredCandidate> {
    Ok(StoredCandidate {
        thread_id: row.try_get("thread_id").map_err(persistence_error)?,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        key: ContinuationKey::stored(
            row.try_get("authorization_scope_digest")
                .map_err(persistence_error)?,
            row.try_get("provider_instance_id")
                .map_err(persistence_error)?,
            row.try_get("model_selection_sha256")
                .map_err(persistence_error)?,
            row.try_get("instructions_sha256")
                .map_err(persistence_error)?,
        ),
        authorization_valid: row
            .try_get("authorization_valid")
            .map_err(persistence_error)?,
    })
}

/// Requires the execution folder `working_folder_id` the authorization froze to still be
/// covered: the agent is still in the channel, the folder is still one of the channel's project
/// folders, and the channel's effective access still allows the frozen capability.
async fn require_live_folder_target(
    pool: &SqlitePool,
    authorization_revision_id: &str,
    destination_conversation_id: &str,
    agent_id: &str,
    working_folder_id: &str,
) -> ChatResult<()> {
    let row: Option<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT frozen.capability, {} AS access_level
         FROM chat_assignment_authorized_folder_sources frozen
         JOIN chat_conversations conversation ON conversation.id = ?
         JOIN project_working_folders folder
           ON folder.id = frozen.working_folder_id
          AND folder.project_id = conversation.project_id
          AND folder.archived_at IS NULL
         JOIN chat_agent_channel_memberships channel_access
           ON channel_access.conversation_id = conversation.id
          AND channel_access.agent_id = ?
          AND channel_access.removed_at IS NULL
         WHERE frozen.authorization_revision_id = ?
           AND frozen.working_folder_id = ?
           AND frozen.is_execution_target = 1",
        effective_access_level_sql("conversation.id"),
    )))
    .bind(destination_conversation_id)
    .bind(agent_id)
    .bind(authorization_revision_id)
    .bind(working_folder_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let covered = row.is_some_and(|(capability, level)| {
        match (
            ChatFolderCapability::parse(&capability),
            ChatAgentAccess::parse(&level),
        ) {
            (Some(capability), Some(level)) => {
                capability.rank() <= access_ceiling(level).folder_capability.rank()
            }
            _ => false,
        }
    });
    if covered {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "The execution folder is no longer authorized for this assignment",
            false,
        ))
    }
}

/// How a claimed dispatch that could not start was settled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DispatchSettlement {
    /// The assignment and its job were failed with the dispatch error.
    Failed,
    /// The job was dropped and the assignment stays queued.
    Waiting,
}

/// Settles the claim `claim_token` of `assignment_id` after its dispatch returned `error`.
///
/// When the agent was archived, for example by a linked device after this device claimed
/// the work, the still queued assignment is not failed: its job is dropped so the work waits,
/// and after a restore it is started again like any other queued work. Any other error
/// fails the assignment.
pub(super) async fn settle_dispatch_error(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    claim_token: &str,
    error: &ChatError,
) -> ChatResult<DispatchSettlement> {
    let released = sqlx::query(
        "DELETE FROM chat_assignment_dispatch_jobs
         WHERE assignment_id = ?1 AND claim_token = ?2
           AND EXISTS (
             SELECT 1 FROM chat_work_assignments assignment
             JOIN chat_participants participant ON participant.id = assignment.agent_id
             WHERE assignment.id = ?1 AND assignment.state = 'queued'
               AND participant.archived_at IS NOT NULL
           )",
    )
    .bind(assignment_id.as_str())
    .bind(claim_token)
    .execute(pool)
    .await
    .map_err(persistence_error)?
    .rows_affected()
        > 0;
    if released {
        return Ok(DispatchSettlement::Waiting);
    }
    fail_assignment_dispatch(pool, assignment_id, claim_token, error).await?;
    Ok(DispatchSettlement::Failed)
}

/// Fails the claim `claim_token` of `assignment_id` and its still queued assignment with
/// `error`, posting one failure message for the claim.
///
/// A claim that no longer holds the job, for example because recovery requeued it after the
/// claim went stale, leaves the assignment to the dispatch that holds the job now.
async fn fail_assignment_dispatch(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    claim_token: &str,
    error: &ChatError,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let job_failed = sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs
         SET state = 'failed', last_error = ?, updated_at = ?
         WHERE assignment_id = ? AND claim_token = ? AND state = 'claimed'",
    )
    .bind(&error.message)
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .bind(claim_token)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected()
        > 0;
    if !job_failed {
        return transaction.commit().await.map_err(persistence_error);
    }
    let failed = sqlx::query(
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
    .map_err(persistence_error)?
    .rows_affected()
        > 0;
    // An assignment that already left the queue, for example through a cancel applied from
    // another device, keeps its state and gets no failure message.
    if failed {
        ganbaru_chat::agent_runs::insert_assignment_failure_message(
            &mut transaction,
            assignment_id,
            ganbaru_chat::agent_runs::LaunchAttempt::Claim(claim_token),
            &error.message,
            &now,
        )
        .await?;
        ganbaru_chat::coordination::delegation::post_delegation_outcome(
            &mut transaction,
            assignment_id.as_str(),
            now.as_str(),
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)
}

/// A pending assignment input ready to steer into the run that works on it.
#[derive(Debug)]
struct PreparedAssignmentInput {
    thread_id: ChatThreadId,
    /// The working run the input steers into.
    run_id: ChatAgentRunId,
    prompt: String,
    /// The message revision the prompt frames, recorded as what the run received.
    revision_id: String,
    /// The source the run receives once the steer reaches the provider.
    delivered: DeliveredSource,
}

/// Reads the pending steer input `message_item_id` of `assignment_id` and frames it as a follow-up
/// context record for the latest working run, while that run's authorization is allowed and the
/// agent may still participate in the conversation. Returns `None` when no working run can
/// take it, so it stays pending.
async fn prepare_assignment_input(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    message_item_id: &ChatConversationItemId,
) -> ChatResult<Option<PreparedAssignmentInput>> {
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
    .fetch_one(pool)
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
        "SELECT run.id AS run_id, run.provider_thread_id, run.scratch_generation_id,
                run.authorization_revision_id, revision.id AS revision_id,
                revision.revision, revision.normalized_markdown
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
          AND membership.participant_id = assignment.agent_id
          AND membership.removed_at IS NULL
         JOIN chat_agent_channel_memberships channel_access
           ON channel_access.conversation_id = membership.conversation_id
          AND channel_access.agent_id = membership.participant_id
          AND channel_access.removed_at IS NULL
         JOIN chat_work_assignment_inputs input ON input.assignment_id = run.assignment_id
         JOIN chat_communication_messages message ON message.item_id = input.message_item_id
         JOIN chat_communication_message_revisions_numbered revision
           ON revision.id = message.current_revision_id
         WHERE run.assignment_id = ? AND input.message_item_id = ?
           AND run.state = 'working' AND input.delivery_state = 'pending' AND input.held = 0
           AND input.routing_kind = 'steer'
         ORDER BY run.run_ordinal DESC LIMIT 1",
    )
    .bind(assignment_id.as_str())
    .bind(message_item_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
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
            pool,
            &scratch_generation_id,
            &authorization_revision_id,
        )
        .await?;
    }
    let run_id = ChatAgentRunId::new(
        row.try_get::<String, _>("run_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let framed = super::context::follow_up_context_line(
        message_item_id.as_str(),
        u64_value(row.try_get("revision").map_err(persistence_error)?)?,
        row.try_get("normalized_markdown")
            .map_err(persistence_error)?,
        super::context::message_agent_authored(pool, message_item_id.as_str()).await?,
    )?;
    Ok(Some(PreparedAssignmentInput {
        thread_id,
        run_id,
        prompt: framed.line,
        revision_id: row.try_get("revision_id").map_err(persistence_error)?,
        delivered: DeliveredSource {
            source_id: message_item_id.as_str().to_string(),
            content_sha256: framed.content_sha256,
            kind: DeliveryKind::Steer,
        },
    }))
}

/// Marks the input `message_item_id` of `assignment_id` delivered after its steer reached the
/// provider, and records the source on the run that took it in the same transaction, so a later
/// continuation of the session does not send it again. An input another delivery already
/// marked records nothing. A cancel that landed after the steer was prepared, locally or by sync,
/// loses to the delivery, as the sync priority of delivery states resolves it: the provider
/// already read the text.
async fn record_assignment_input_delivered(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    message_item_id: &ChatConversationItemId,
    prepared: &PreparedAssignmentInput,
) -> ChatResult<()> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let marked = sqlx::query(
        "UPDATE chat_work_assignment_inputs
         SET delivery_state = 'delivered', delivered_at = ?, delivered_revision_id = ?
         WHERE assignment_id = ? AND message_item_id = ?
           AND delivery_state IN ('pending', 'ignored')",
    )
    .bind(now.as_str())
    .bind(&prepared.revision_id)
    .bind(assignment_id.as_str())
    .bind(message_item_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if marked > 0 {
        place_delivered_reply(&mut transaction, message_item_id.as_str(), now.as_str()).await?;
        ganbaru_chat::agent_runs::insert_delivered_sources(
            &mut transaction,
            &prepared.run_id,
            std::slice::from_ref(&prepared.delivered),
            &now,
        )
        .await?;
        // A person's answer that reached the asking run while it still works resumes the work.
        ganbaru_chat::coordination::reports::resume_answered_question(
            &mut transaction,
            assignment_id.as_str(),
            message_item_id.as_str(),
            now.as_str(),
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)
}

pub(super) async fn deliver_assignment_input(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    message_item_id: ChatConversationItemId,
) -> ChatResult<()> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let Some(prepared) = prepare_assignment_input(&pool, &assignment_id, &message_item_id).await?
    else {
        return Ok(());
    };
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
            thread_id: prepared.thread_id.clone(),
            message_id: ChatMessageId::new(new_id("provider-steer-message")?)
                .map_err(identifier_error)?,
            prompt: prepared.prompt.clone(),
        },
    )
    .await?;
    record_assignment_input_delivered(&pool, &assignment_id, &message_item_id, &prepared).await
}

/// The model a dispatch runs and the label its assignment shows. The label is `None` when the
/// provider default runs without a named catalog entry, so every device shows its own localized
/// default name instead of replicated text.
#[derive(Debug, PartialEq)]
struct DispatchModel {
    selection: ChatModelSelection,
    label: Option<String>,
}

/// Chooses the model a dispatch runs from the `frozen` selection of its assignment and the
/// `catalog` this device last discovered for the provider. A model line the catalog does not
/// offer by that id, such as `sol`, runs the line's newest model. A selection without a model,
/// or with a model the catalog does not offer, runs the catalog's recommended model under its
/// name; without
/// a catalog the named model runs as frozen, since the provider is the only authority on what it
/// offers.
fn dispatch_model(
    frozen: &ChatModelSelection,
    catalog: Option<&ProviderModelCatalog>,
) -> DispatchModel {
    let Some(model_id) = frozen.model_id.as_ref() else {
        return recommended_model(frozen.model_options.clone(), catalog);
    };
    let Some(catalog) = catalog else {
        return DispatchModel {
            selection: frozen.clone(),
            label: Some(model_id.as_str().to_string()),
        };
    };
    match catalog.models.iter().find(|model| &model.id == model_id) {
        Some(model) => DispatchModel {
            selection: frozen.clone(),
            label: Some(model.display_name.clone()),
        },
        None => match newest_model_of_line(model_id.as_str(), &catalog.models)
            .and_then(|id| catalog.models.iter().find(|model| model.id == id))
        {
            Some(model) => DispatchModel {
                selection: ChatModelSelection {
                    provider_managed_model: false,
                    model_id: Some(model.id.clone()),
                    model_options: frozen.model_options.clone(),
                },
                label: Some(model.display_name.clone()),
            },
            None => recommended_model(Vec::new(), Some(catalog)),
        },
    }
}

/// The `catalog`'s recommended model with `model_options`, or no model at all, so the provider
/// runs its configured one, when the catalog recommends nothing.
fn recommended_model(
    model_options: Vec<ModelOptionSelection>,
    catalog: Option<&ProviderModelCatalog>,
) -> DispatchModel {
    let recommended = catalog.and_then(|catalog| {
        let id = catalog.recommended_model_id.as_ref()?;
        catalog.models.iter().find(|model| &model.id == id)
    });
    match recommended {
        Some(model) => DispatchModel {
            selection: ChatModelSelection {
                provider_managed_model: false,
                model_id: Some(model.id.clone()),
                model_options,
            },
            label: Some(model.display_name.clone()),
        },
        None => DispatchModel {
            selection: ChatModelSelection {
                model_options,
                ..ChatModelSelection::provider_default()
            },
            label: None,
        },
    }
}

/// Chooses the turn modes for an organizational assignment. Native targets run with the frozen
/// runtime approval its channel access resolved to; work without a target stays conversational.
fn assignment_turn_modes(
    resolved_runtime_approval: ChatRuntimeApprovalPolicy,
    has_native_target: bool,
) -> TurnModeSnapshot {
    TurnModeSnapshot {
        safety_mode: if has_native_target {
            native_assignment_safety_mode(resolved_runtime_approval)
        } else {
            SafetyMode::AskForApproval
        },
        interaction_mode: InteractionMode::Build,
    }
}

#[cfg(test)]
mod tests;
