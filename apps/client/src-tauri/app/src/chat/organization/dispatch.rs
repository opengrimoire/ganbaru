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
use ganbaru_chat_contracts::models::*;
use ganbaru_chat_providers::model_recommendation::newest_model_of_line;
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
/// consented subject it verified.
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
/// The job starts only what it consented to: when the current subject is not its consented
/// digest, or cannot be read (as for an archived agent), the job is dropped so the
/// assignment stays queued and waits for Start here again.
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
    let Some(consented_digest) = sqlx::query_scalar::<_, Option<String>>(
        "SELECT job.consented_digest
         FROM chat_assignment_dispatch_jobs job
         JOIN chat_work_assignments assignment ON assignment.id = job.assignment_id
         WHERE job.assignment_id = ?1 AND job.state = 'queued' AND job.available_at <= ?2
           AND coalesce(assignment.executing_device_id, ?3) = ?3",
    )
    .bind(assignment_id.as_str())
    .bind(now.as_str())
    .bind(device_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(persistence_error)?
    else {
        return Ok(None);
    };
    let subject = match read_assignment_subject(&mut transaction, assignment_id, providers).await {
        Ok(subject) => Some(subject),
        Err(error) if error.code == ChatErrorCode::Persistence => return Err(error),
        Err(_) => None,
    };
    let Some((subject, provider_instance_id)) = subject
        .filter(|subject| consented_digest.as_deref() == Some(subject.digest().as_str()))
        .and_then(|subject| {
            let instance = ProviderInstanceId::new(subject.provider_instance_id()).ok()?;
            Some((subject, instance))
        })
    else {
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
/// and after a restore it is started again like any other unaccepted work. Any other error
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
mod tests {
    use super::{
        ChatModelSelection, ChatRuntimeApprovalPolicy, DispatchModel, InteractionMode,
        ModelAvailability, ModelCatalogSource, ModelId, ProviderInstanceId, ProviderModel,
        ProviderModelCatalog, SafetyMode, UtcTimestamp, assignment_turn_modes, dispatch_model,
    };

    #[test]
    fn targetless_assignments_use_a_narrow_conversation_mode() {
        let modes = assignment_turn_modes(ChatRuntimeApprovalPolicy::AutoApprove, false);
        assert_eq!(modes.safety_mode, SafetyMode::AskForApproval);
        assert_eq!(modes.interaction_mode, InteractionMode::Build);
    }

    #[test]
    fn native_assignments_run_with_the_frozen_runtime_approval() {
        for (resolved, expected) in [
            (ChatRuntimeApprovalPolicy::Ask, SafetyMode::AskForApproval),
            (
                ChatRuntimeApprovalPolicy::AutoApprove,
                SafetyMode::ApproveForMe,
            ),
        ] {
            let modes = assignment_turn_modes(resolved, true);
            assert_eq!(modes.safety_mode, expected, "{resolved:?}");
            assert_eq!(modes.interaction_mode, InteractionMode::Build);
        }
    }

    fn named(model: &str) -> ChatModelSelection {
        ChatModelSelection {
            provider_managed_model: false,
            model_id: Some(ModelId::new(model).unwrap()),
            model_options: Vec::new(),
        }
    }

    fn catalog(models: &[(&str, &str)]) -> ProviderModelCatalog {
        ProviderModelCatalog {
            instance_id: ProviderInstanceId::new("codex").unwrap(),
            models: models
                .iter()
                .map(|(id, display_name)| ProviderModel {
                    id: ModelId::new(*id).unwrap(),
                    display_name: (*display_name).to_string(),
                    description: None,
                    context_limit: None,
                    availability: ModelAvailability::Available,
                    capabilities: Vec::new(),
                    options: Vec::new(),
                    custom: false,
                })
                .collect(),
            recommended_model_id: None,
            source: ModelCatalogSource::Provider,
            discovered_at: UtcTimestamp::new("2026-11-18T00:00:00.000Z").unwrap(),
            stale: false,
        }
    }

    #[test]
    fn a_model_the_catalog_offers_runs_under_its_display_name() {
        let catalog = catalog(&[("gpt-5", "GPT-5")]);
        assert_eq!(
            dispatch_model(&named("gpt-5"), Some(&catalog)),
            DispatchModel {
                selection: named("gpt-5"),
                label: Some("GPT-5".to_string()),
            }
        );
    }

    #[test]
    fn an_unknown_model_without_a_recommendation_runs_the_provider_configured_model() {
        let catalog = catalog(&[("gpt-5", "GPT-5")]);
        let model = dispatch_model(&named("gpt-9"), Some(&catalog));
        assert_eq!(model.selection, ChatModelSelection::provider_default());
        assert_eq!(model.label, None);
    }

    #[test]
    fn without_a_catalog_the_named_model_runs_as_frozen() {
        assert_eq!(
            dispatch_model(&named("gpt-9"), None),
            DispatchModel {
                selection: named("gpt-9"),
                label: Some("gpt-9".to_string()),
            }
        );
    }

    #[test]
    fn a_selection_without_a_model_stays_empty_when_nothing_is_recommended() {
        let default = ChatModelSelection::provider_default();
        let model = dispatch_model(&default, Some(&catalog(&[])));
        assert_eq!(model.selection, default);
        assert_eq!(model.label, None);
    }

    #[test]
    fn a_selection_without_a_known_model_runs_the_recommended_model() {
        let mut catalog = catalog(&[("sonnet", "Sonnet 9"), ("opus", "Opus 9")]);
        catalog.recommended_model_id = Some(ModelId::new("opus").unwrap());
        let expected = DispatchModel {
            selection: named("opus"),
            label: Some("Opus 9".to_string()),
        };
        assert_eq!(
            dispatch_model(&ChatModelSelection::provider_default(), Some(&catalog)),
            expected
        );
        assert_eq!(dispatch_model(&named("gpt-9"), Some(&catalog)), expected);
    }

    #[test]
    fn a_model_line_runs_its_newest_model_with_the_frozen_options() {
        use ganbaru_chat_contracts::models::{ModelOptionSelection, ModelOptionValue};
        let catalog = catalog(&[
            ("gpt-5.5-sol", "GPT-5.5-Sol"),
            ("gpt-5.6-sol", "GPT-5.6-Sol"),
            ("gpt-5.6-astra", "GPT-5.6-Astra"),
        ]);
        let effort = vec![ModelOptionSelection {
            key: "reasoningEffort".to_string(),
            value: ModelOptionValue::Choice("high".to_string()),
        }];
        let frozen = ChatModelSelection {
            model_options: effort.clone(),
            ..named("sol")
        };
        assert_eq!(
            dispatch_model(&frozen, Some(&catalog)),
            DispatchModel {
                selection: ChatModelSelection {
                    model_options: effort,
                    ..named("gpt-5.6-sol")
                },
                label: Some("GPT-5.6-Sol".to_string()),
            }
        );
    }

    /// Which earlier run a later assignment of the fixture's reply thread could continue.
    mod context_delivery {
        use super::super::{PlannedDelivery, choose_context_delivery};
        use crate::chat::organization::continuation::DeltaPackage;
        use ganbaru_chat::agent_runs::{
            AttemptReason, ContextDelivery, DeliveredSource, DeliveryKind, FullPackage,
            HandoffReason,
        };

        fn source(source_id: &str, digit: char) -> DeliveredSource {
            DeliveredSource {
                source_id: source_id.to_string(),
                content_sha256: digit.to_string().repeat(64),
                kind: DeliveryKind::Package,
            }
        }

        fn full() -> FullPackage {
            FullPackage {
                prompt: "header\nroot\nearlier\ntrigger".to_string(),
                delivered_sources: vec![
                    source("item:root", 'a'),
                    source("item:earlier", 'b'),
                    source("item:trigger", 'c'),
                ],
            }
        }

        fn delta() -> DeltaPackage {
            DeltaPackage {
                text: "continuation header\ntrigger".to_string(),
                sources: vec![source("item:trigger", 'c')],
            }
        }

        #[test]
        fn a_continuation_sends_its_delta_and_keeps_the_full_package_for_a_fresh_session() {
            assert_eq!(
                choose_context_delivery(AttemptReason::Continuation, Some(delta()), full()),
                PlannedDelivery {
                    prompt: "continuation header\ntrigger".to_string(),
                    context_delivery: ContextDelivery::Delta,
                    delivered_sources: vec![source("item:trigger", 'c')],
                    full_prompt_on_fresh_session: Some(full()),
                }
            );
        }

        #[test]
        fn a_continuation_whose_delta_did_not_fit_sends_the_full_package() {
            assert_eq!(
                choose_context_delivery(AttemptReason::Continuation, None, full()),
                PlannedDelivery {
                    prompt: full().prompt,
                    context_delivery: ContextDelivery::Full,
                    delivered_sources: full().delivered_sources,
                    full_prompt_on_fresh_session: Some(full()),
                }
            );
        }

        #[test]
        fn every_run_in_a_new_session_sends_the_full_package_without_a_fallback() {
            for reason in [
                AttemptReason::Fresh,
                AttemptReason::Retry,
                AttemptReason::Handoff(HandoffReason::NativeSessionLost),
                AttemptReason::Handoff(HandoffReason::KeyChanged),
                AttemptReason::Handoff(HandoffReason::AuthorityRevoked),
            ] {
                assert_eq!(
                    choose_context_delivery(reason, Some(delta()), full()),
                    PlannedDelivery {
                        prompt: full().prompt,
                        context_delivery: ContextDelivery::Full,
                        delivered_sources: full().delivered_sources,
                        full_prompt_on_fresh_session: None,
                    },
                    "{reason:?}"
                );
            }
        }
    }

    mod continuation_candidate {
        use super::super::{CandidateScope, StoredCandidate, read_continuation_candidate};
        use crate::chat::organization::continuation::ContinuationKey;
        use ganbaru_chat_contracts::models::*;
        use sqlx::SqlitePool;

        const MODEL_SHA: &str = "1111111111111111111111111111111111111111111111111111111111111111";
        const INSTRUCTIONS_SHA: &str =
            "2222222222222222222222222222222222222222222222222222222222222222";

        /// The fixture's working run completed under a recorded key, with its assignment
        /// completed and the queued run removed.
        async fn completed_pool() -> SqlitePool {
            let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DELETE FROM chat_agent_runs WHERE id = 'run:queued';
                 UPDATE chat_agent_runs SET state = 'starting' WHERE id = 'run:working';
                 UPDATE chat_agent_runs
                 SET model_selection_sha256 = '{MODEL_SHA}',
                     instructions_sha256 = '{INSTRUCTIONS_SHA}', state = 'completed'
                 WHERE id = 'run:working';
                 UPDATE chat_work_assignments SET state = 'completed'
                 WHERE id = 'assignment:folders';"
            )))
            .execute(&pool)
            .await
            .unwrap();
            pool
        }

        async fn candidate_for(pool: &SqlitePool, agent: &str) -> Option<StoredCandidate> {
            let environment: String = sqlx::query_scalar(
                "SELECT execution_environment_id FROM chat_agent_runs WHERE id = 'run:working'",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            read_continuation_candidate(
                pool,
                &CandidateScope {
                    assignment_id: &ChatWorkAssignmentId::new("assignment:later").unwrap(),
                    reply_thread_id: &ChatReplyThreadId::new("reply:folders").unwrap(),
                    agent_id: &ChatParticipantId::new(agent).unwrap(),
                    working_folder_id: Some(&ProjectWorkingFolderId::new("workspace-1").unwrap()),
                    scratch_generation_id: None,
                    execution_environment_id: Some(&environment),
                },
            )
            .await
            .unwrap()
        }

        #[tokio::test]
        async fn a_completed_run_of_the_same_agent_is_the_candidate_with_its_stored_key() {
            let pool = completed_pool().await;
            let provider: String = sqlx::query_scalar(
                "SELECT provider_instance_id FROM chat_threads WHERE id = 'thread-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            let candidate = candidate_for(&pool, "participant:folder-agent")
                .await
                .expect("the completed run can be continued");
            assert_eq!(candidate.thread_id, "thread-1");
            assert!(candidate.authorization_valid);
            assert_eq!(
                candidate.key,
                Some(ContinuationKey {
                    authorization_scope_digest: format!("{:064}", 0),
                    provider_instance_id: provider,
                    model_selection_sha256: MODEL_SHA.to_string(),
                    instructions_sha256: INSTRUCTIONS_SHA.to_string(),
                })
            );
        }

        #[tokio::test]
        async fn another_agent_in_the_thread_never_continues_the_session() {
            let pool = completed_pool().await;
            assert!(
                candidate_for(&pool, "participant:other-agent")
                    .await
                    .is_none()
            );
        }

        #[tokio::test]
        async fn a_revoked_authorization_or_missing_key_is_reported_not_hidden() {
            let pool = completed_pool().await;
            sqlx::query(
                "UPDATE chat_assignment_authorization_revisions
                 SET decision_state = 'revoked', revoked_at = '2026-10-20T12:05:00.000Z'
                 WHERE id = 'authorization:working'",
            )
            .execute(&pool)
            .await
            .unwrap();
            let candidate = candidate_for(&pool, "participant:folder-agent")
                .await
                .expect("a revoked candidate is still reported");
            assert!(!candidate.authorization_valid);

            let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
            sqlx::raw_sql(
                "DELETE FROM chat_agent_runs WHERE id = 'run:queued';
                 UPDATE chat_agent_runs SET state = 'completed' WHERE id = 'run:working';
                 UPDATE chat_work_assignments SET state = 'completed'
                 WHERE id = 'assignment:folders';",
            )
            .execute(&pool)
            .await
            .unwrap();
            let candidate = candidate_for(&pool, "participant:folder-agent")
                .await
                .expect("a run without a recorded key is still reported");
            assert_eq!(candidate.key, None);
        }

        #[tokio::test]
        async fn an_open_assignment_or_a_closed_thread_is_no_candidate() {
            for update in [
                "UPDATE chat_work_assignments SET state = 'working'
                 WHERE id = 'assignment:folders'",
                "UPDATE chat_threads SET state = 'closed' WHERE id = 'thread-1'",
            ] {
                let pool = completed_pool().await;
                sqlx::query(update).execute(&pool).await.unwrap();
                assert!(
                    candidate_for(&pool, "participant:folder-agent")
                        .await
                        .is_none(),
                    "{update}"
                );
            }
        }
    }

    /// Launch state the launch tests share: the fixture's working run back at `starting`, its
    /// assignment queued, and its dispatch job claimed by `claim:folders`.
    mod launch {
        use super::super::{
            AssignmentLaunch, fail_assignment_dispatch, mark_initial_assignment_input_delivered,
            pending_live_input_ids, prepare_assignment_input, record_assignment_input_delivered,
        };
        use crate::chat::organization::assignments::reclaim_stale_dispatch_claims;
        use crate::chat::organization::continuation::{
            self, CandidateRun, ContinuationKey, TransitionInput,
        };
        use ganbaru_chat::agent_runs::{
            AgentRunBinding, AttemptReason, ContextDelivery, DeliveredSource, DeliveryKind,
            HandoffReason, LaunchAttempt, LaunchSettlement, RunAttempt, RunLaunch,
            insert_assignment_failure_message, settle_launch,
        };
        use ganbaru_chat_contracts::models::*;
        use sqlx::SqlitePool;

        const CLAIMED_AT: &str = "2026-10-20T12:00:00.000Z";

        fn assignment() -> ChatWorkAssignmentId {
            ChatWorkAssignmentId::new("assignment:folders").unwrap()
        }

        fn at(timestamp: &str) -> UtcTimestamp {
            UtcTimestamp::new(timestamp.to_string()).unwrap()
        }

        fn launch_error() -> ChatError {
            ChatError::new(ChatErrorCode::DriverUnavailable, "Provider exited", true)
        }

        async fn starting_pool() -> SqlitePool {
            let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "UPDATE chat_agent_runs SET state = 'starting' WHERE id = 'run:working';
                 UPDATE chat_work_assignments SET state = 'queued'
                 WHERE id = 'assignment:folders';
                 INSERT INTO chat_assignment_dispatch_jobs
                    (id, assignment_id, state, claim_token, claimed_at, available_at,
                     created_at, updated_at)
                 VALUES ('dispatch:folders', 'assignment:folders', 'claimed', 'claim:folders',
                         '{CLAIMED_AT}', '{CLAIMED_AT}', '{CLAIMED_AT}', '{CLAIMED_AT}');"
            )))
            .execute(&pool)
            .await
            .unwrap();
            pool
        }

        async fn working_binding(pool: &SqlitePool) -> AgentRunBinding {
            let execution_environment_id: String = sqlx::query_scalar(
                "SELECT execution_environment_id FROM chat_agent_runs WHERE id = 'run:working'",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            AgentRunBinding {
                run_id: ChatAgentRunId::new("run:working").unwrap(),
                assignment_id: assignment(),
                project_id: "project-chat".to_string(),
                authorization_revision_id: ChatAuthorizationRevisionId::new(
                    "authorization:working",
                )
                .unwrap(),
                authorization_scope_digest: format!("{:064}", 0),
                working_folder_id: Some("workspace-1".to_string()),
                scratch_generation_id: None,
                execution_environment_id: Some(execution_environment_id),
                run_ordinal: 1,
            }
        }

        fn delivered(source_id: &str, digit: char) -> DeliveredSource {
            DeliveredSource {
                source_id: source_id.to_string(),
                content_sha256: digit.to_string().repeat(64),
                kind: DeliveryKind::Package,
            }
        }

        /// The launch of the fixture run under the fixture claim, delivering two sources.
        fn folder_launch() -> RunLaunch {
            RunLaunch {
                claim_token: "claim:folders".to_string(),
                attempt: RunAttempt {
                    reason: AttemptReason::Fresh,
                    instructions_sha256: None,
                    model_selection_sha256: None,
                    context_delivery: ContextDelivery::Full,
                    delivered_context_bytes: 42,
                    delivered_sources: vec![
                        delivered("item:root", 'a'),
                        delivered("item:folders", 'b'),
                    ],
                },
                full_prompt_on_fresh_session: None,
                model_label: None,
            }
        }

        async fn delivered_sources(pool: &SqlitePool) -> Vec<(String, String)> {
            sqlx::query_as(
                "SELECT source_id, delivery_kind FROM chat_agent_run_delivered_sources
                 WHERE run_id = 'run:working' ORDER BY source_id",
            )
            .fetch_all(pool)
            .await
            .unwrap()
        }

        /// Posts a message by the local owner in the assignment's reply thread and records it
        /// as an assignment input routed as `routing_kind`.
        async fn seed_input(
            pool: &SqlitePool,
            item_id: &str,
            ordinal: i64,
            routing_kind: &str,
            text: &str,
        ) {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "INSERT INTO chat_conversation_items
                    (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
                 SELECT '{item_id}', conversation_id, id, 'message', 'a{ordinal}', '{CLAIMED_AT}'
                 FROM chat_reply_threads WHERE id = 'reply:folders';
                 INSERT INTO chat_communication_messages
                    (item_id, author_participant_id, created_at)
                 VALUES ('{item_id}', 'participant:local-owner', '{CLAIMED_AT}');
                 INSERT INTO chat_communication_message_revisions
                    (id, message_item_id, normalized_markdown, created_at)
                 VALUES ('revision:{item_id}', '{item_id}', '{text}', '{CLAIMED_AT}');
                 UPDATE chat_communication_messages
                 SET current_revision_id = 'revision:{item_id}' WHERE item_id = '{item_id}';
                 INSERT INTO chat_work_assignment_inputs
                    (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
                 VALUES ('input:{item_id}', 'assignment:folders', '{item_id}', {ordinal},
                         '{routing_kind}', '{CLAIMED_AT}');"
            )))
            .execute(pool)
            .await
            .unwrap();
        }

        async fn seed_inputs_of_every_route(pool: &SqlitePool) {
            sqlx::query(
                "INSERT INTO chat_work_assignment_inputs
                    (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
                 VALUES ('input:trigger', 'assignment:folders', 'item:folders', 1, 'trigger',
                         '2026-10-20T12:00:00.000Z')",
            )
            .execute(pool)
            .await
            .unwrap();
            seed_input(
                pool,
                "item:follow-up",
                2,
                "follow_up",
                "Also check the logs",
            )
            .await;
            seed_input(pool, "item:steer", 3, "steer", "Prefer the smaller fix").await;
            seed_input(
                pool,
                "item:continuation",
                4,
                "queued_continuation",
                "Then update the docs",
            )
            .await;
        }

        async fn delivery_states(pool: &SqlitePool) -> Vec<(String, String)> {
            sqlx::query_as(
                "SELECT routing_kind, delivery_state FROM chat_work_assignment_inputs
                 WHERE assignment_id = 'assignment:folders' ORDER BY ordinal",
            )
            .fetch_all(pool)
            .await
            .unwrap()
        }

        fn states(package: &str) -> Vec<(String, String)> {
            [
                ("trigger", package),
                ("follow_up", package),
                ("steer", "pending"),
                ("queued_continuation", "pending"),
            ]
            .into_iter()
            .map(|(kind, state)| (kind.to_string(), state.to_string()))
            .collect()
        }

        async fn failure_messages(pool: &SqlitePool) -> i64 {
            sqlx::query_scalar(
                "SELECT count(*) FROM chat_work_semantic_updates
                 WHERE assignment_id = 'assignment:folders' AND update_kind = 'failure'",
            )
            .fetch_one(pool)
            .await
            .unwrap()
        }

        async fn assignment_and_job(pool: &SqlitePool) -> (String, String) {
            sqlx::query_as(
                "SELECT assignment.state, job.state
                 FROM chat_work_assignments assignment
                 JOIN chat_assignment_dispatch_jobs job ON job.assignment_id = assignment.id
                 WHERE assignment.id = 'assignment:folders'",
            )
            .fetch_one(pool)
            .await
            .unwrap()
        }

        async fn thread_and_conversation_revisions(pool: &SqlitePool) -> (i64, i64) {
            sqlx::query_as(
                "SELECT thread.revision, conversation.revision
                 FROM chat_reply_threads thread
                 JOIN chat_conversations conversation ON conversation.id = thread.conversation_id
                 WHERE thread.id = 'reply:folders'",
            )
            .fetch_one(pool)
            .await
            .unwrap()
        }

        fn pair(first: &str, second: &str) -> (String, String) {
            (first.to_string(), second.to_string())
        }

        #[tokio::test]
        async fn a_failed_launch_posts_one_failure_message_per_attempt() {
            let pool = starting_pool().await;
            let binding = working_binding(&pool).await;
            let error = launch_error();

            let settlement = settle_launch(
                &pool,
                &binding,
                &folder_launch(),
                Some(&error),
                &at(CLAIMED_AT),
            )
            .await
            .unwrap();
            assert_eq!(settlement, LaunchSettlement::Recorded);
            assert_eq!(failure_messages(&pool).await, 1);
            assert_eq!(assignment_and_job(&pool).await, pair("failed", "failed"));

            // The dispatch error path that follows a failed launch finds the job settled.
            fail_assignment_dispatch(&pool, &assignment(), "claim:folders", &error)
                .await
                .unwrap();
            assert_eq!(failure_messages(&pool).await, 1);

            let revisions = thread_and_conversation_revisions(&pool).await;
            let repeated = {
                let mut connection = pool.acquire().await.unwrap();
                insert_assignment_failure_message(
                    &mut connection,
                    &assignment(),
                    LaunchAttempt::Run(&binding.run_id),
                    &error.message,
                    &at("2026-10-20T12:01:00.000Z"),
                )
                .await
                .unwrap()
            };
            assert!(!repeated, "the same attempt posts no second message");
            assert_eq!(failure_messages(&pool).await, 1);
            assert_eq!(thread_and_conversation_revisions(&pool).await, revisions);

            let claim = {
                let mut connection = pool.acquire().await.unwrap();
                insert_assignment_failure_message(
                    &mut connection,
                    &assignment(),
                    LaunchAttempt::Claim("claim:folders"),
                    &error.message,
                    &at("2026-10-20T12:01:00.000Z"),
                )
                .await
                .unwrap()
            };
            assert!(claim, "a claim attempt is keyed apart from the run attempt");
            assert_eq!(failure_messages(&pool).await, 2);
            assert_ne!(thread_and_conversation_revisions(&pool).await, revisions);
        }

        #[tokio::test]
        async fn only_a_launch_that_starts_its_run_records_delivered_sources() {
            let error = launch_error();
            let pool = starting_pool().await;
            let binding = working_binding(&pool).await;
            settle_launch(
                &pool,
                &binding,
                &folder_launch(),
                Some(&error),
                &at(CLAIMED_AT),
            )
            .await
            .unwrap();
            assert!(delivered_sources(&pool).await.is_empty(), "failed launch");

            let pool = starting_pool().await;
            let binding = working_binding(&pool).await;
            sqlx::query(
                "UPDATE chat_work_assignments SET state = 'cancelled',
                     settled_at = '2026-10-20T12:01:00.000Z'
                 WHERE id = 'assignment:folders'",
            )
            .execute(&pool)
            .await
            .unwrap();
            let superseded =
                settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
                    .await
                    .unwrap();
            assert_eq!(superseded, LaunchSettlement::Superseded);
            assert!(
                delivered_sources(&pool).await.is_empty(),
                "superseded launch"
            );

            let pool = starting_pool().await;
            let binding = working_binding(&pool).await;
            sqlx::query("UPDATE chat_agent_runs SET state = 'failed' WHERE id = 'run:working'")
                .execute(&pool)
                .await
                .unwrap();
            let settled = settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
                .await
                .unwrap();
            assert_eq!(settled, LaunchSettlement::RunSettled);
            assert!(delivered_sources(&pool).await.is_empty(), "settled run");

            let pool = starting_pool().await;
            let binding = working_binding(&pool).await;
            let recorded = settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
                .await
                .unwrap();
            assert_eq!(recorded, LaunchSettlement::Recorded);
            let expected = vec![
                pair("item:folders", "package"),
                pair("item:root", "package"),
            ];
            assert_eq!(delivered_sources(&pool).await, expected);

            let mut connection = pool.acquire().await.unwrap();
            ganbaru_chat::agent_runs::insert_delivered_sources(
                &mut connection,
                &binding.run_id,
                &folder_launch().attempt.delivered_sources,
                &at("2026-10-20T12:01:00.000Z"),
            )
            .await
            .expect("a replayed delivery keeps its first record");
            drop(connection);
            assert_eq!(delivered_sources(&pool).await, expected);
        }

        #[tokio::test]
        async fn a_launch_whose_claim_was_lost_settles_only_its_run() {
            for error in [None, Some(launch_error())] {
                let pool = starting_pool().await;
                sqlx::query(
                    "UPDATE chat_assignment_dispatch_jobs
                     SET claim_token = 'claim:newer' WHERE assignment_id = 'assignment:folders'",
                )
                .execute(&pool)
                .await
                .unwrap();
                let binding = working_binding(&pool).await;

                let settlement = settle_launch(
                    &pool,
                    &binding,
                    &folder_launch(),
                    error.as_ref(),
                    &at("2026-10-20T12:01:00.000Z"),
                )
                .await
                .unwrap();

                assert_eq!(settlement, LaunchSettlement::ClaimLost);
                assert!(settlement.stops_session());
                assert_eq!(
                    run_and_job(&pool).await,
                    (
                        "cancelled".to_string(),
                        Some("2026-10-20T12:01:00.000Z".to_string()),
                        "claimed".to_string(),
                        None
                    )
                );
                assert_eq!(assignment_and_job(&pool).await, pair("queued", "claimed"));
                assert_eq!(failure_messages(&pool).await, 0);
                assert!(delivered_sources(&pool).await.is_empty());
            }
        }

        #[tokio::test]
        async fn a_dispatch_failure_settles_only_the_claim_that_holds_the_job() {
            let pool = starting_pool().await;
            let error = launch_error();

            fail_assignment_dispatch(&pool, &assignment(), "claim:stale", &error)
                .await
                .unwrap();
            assert_eq!(assignment_and_job(&pool).await, pair("queued", "claimed"));
            assert_eq!(failure_messages(&pool).await, 0);

            sqlx::query(
                "UPDATE chat_assignment_dispatch_jobs
                 SET state = 'queued', claim_token = NULL, claimed_at = NULL",
            )
            .execute(&pool)
            .await
            .unwrap();
            fail_assignment_dispatch(&pool, &assignment(), "claim:folders", &error)
                .await
                .unwrap();
            assert_eq!(assignment_and_job(&pool).await, pair("queued", "queued"));
            assert_eq!(failure_messages(&pool).await, 0);

            sqlx::query(
                "UPDATE chat_assignment_dispatch_jobs
                 SET state = 'claimed', claim_token = 'claim:folders'",
            )
            .execute(&pool)
            .await
            .unwrap();
            fail_assignment_dispatch(&pool, &assignment(), "claim:folders", &error)
                .await
                .unwrap();
            assert_eq!(assignment_and_job(&pool).await, pair("failed", "failed"));
            assert_eq!(failure_messages(&pool).await, 1);
        }

        #[tokio::test]
        async fn package_inputs_stay_pending_until_the_launched_run_starts() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            let run_id = ChatAgentRunId::new("run:working").unwrap();

            let failed = AssignmentLaunch {
                run_id: run_id.clone(),
                launch_error: Some(launch_error()),
                settlement: None,
                answered_input_ids: Vec::new(),
            };
            assert!(
                !mark_initial_assignment_input_delivered(&pool, &assignment(), &failed)
                    .await
                    .unwrap()
            );
            assert_eq!(delivery_states(&pool).await, states("pending"));

            let unstarted = AssignmentLaunch {
                run_id: run_id.clone(),
                launch_error: None,
                settlement: None,
                answered_input_ids: Vec::new(),
            };
            assert!(
                !mark_initial_assignment_input_delivered(&pool, &assignment(), &unstarted)
                    .await
                    .unwrap()
            );
            assert_eq!(delivery_states(&pool).await, states("pending"));

            let missing = AssignmentLaunch {
                run_id: ChatAgentRunId::new("run:missing").unwrap(),
                launch_error: None,
                settlement: None,
                answered_input_ids: Vec::new(),
            };
            assert!(
                !mark_initial_assignment_input_delivered(&pool, &assignment(), &missing)
                    .await
                    .unwrap()
            );

            let binding = working_binding(&pool).await;
            settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
                .await
                .unwrap();
            assert!(
                mark_initial_assignment_input_delivered(&pool, &assignment(), &unstarted)
                    .await
                    .unwrap()
            );
            assert_eq!(delivery_states(&pool).await, states("delivered"));
        }

        #[tokio::test]
        async fn a_package_input_records_the_revision_its_package_froze() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "INSERT INTO chat_assignment_context_packages
                    (id, assignment_id, revision, triggering_message_item_id, serialized_text,
                     serialized_bytes, sha256, created_at)
                 VALUES ('context:folders', 'assignment:folders', 1, 'item:follow-up', 'frozen',
                         6, '{sha}', '{CLAIMED_AT}');
                 INSERT INTO chat_assignment_context_sources
                    (context_package_id, source_kind, source_id, source_revision,
                     content_sha256, ordinal)
                 VALUES ('context:folders', 'trigger', 'item:follow-up', 1, '{sha}', 0);
                 INSERT INTO chat_communication_message_revisions
                    (id, message_item_id, normalized_markdown, created_at)
                 VALUES ('revision:item:follow-up:edited', 'item:follow-up',
                         'Check the logs and the metrics', '2026-10-20T12:01:00.000Z');
                 UPDATE chat_communication_messages
                 SET current_revision_id = 'revision:item:follow-up:edited'
                 WHERE item_id = 'item:follow-up';",
                sha = "c".repeat(64),
            )))
            .execute(&pool)
            .await
            .unwrap();
            let binding = working_binding(&pool).await;
            settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
                .await
                .unwrap();
            let launch = AssignmentLaunch {
                run_id: ChatAgentRunId::new("run:working").unwrap(),
                launch_error: None,
                settlement: None,
                answered_input_ids: Vec::new(),
            };

            assert!(
                mark_initial_assignment_input_delivered(&pool, &assignment(), &launch)
                    .await
                    .unwrap()
            );

            let revisions: Vec<(String, Option<String>)> = sqlx::query_as(
                "SELECT routing_kind, delivered_revision_id FROM chat_work_assignment_inputs
                 WHERE routing_kind IN ('trigger', 'follow_up') ORDER BY ordinal",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(
                revisions,
                vec![
                    ("trigger".to_string(), None),
                    (
                        "follow_up".to_string(),
                        Some("revision:item:follow-up".to_string())
                    ),
                ],
                "the edit after the freeze never reached the run"
            );
        }

        #[tokio::test]
        async fn a_retried_assignment_frames_its_pending_steer_for_the_working_run() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            let steer = ChatConversationItemId::new("item:steer").unwrap();
            let continuation = ChatConversationItemId::new("item:continuation").unwrap();

            assert_eq!(
                pending_live_input_ids(&pool, &assignment()).await.unwrap(),
                vec![steer.clone()],
                "a queued continuation waits for the turn to end"
            );
            for state in ["starting", "failed"] {
                sqlx::query("UPDATE chat_agent_runs SET state = ? WHERE id = 'run:working'")
                    .bind(state)
                    .execute(&pool)
                    .await
                    .unwrap();
                let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
                    .await
                    .unwrap();
                assert!(prepared.is_none(), "a {state} run takes no live input");
            }

            sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
                .execute(&pool)
                .await
                .unwrap();
            assert!(
                prepare_assignment_input(&pool, &assignment(), &continuation)
                    .await
                    .unwrap()
                    .is_none(),
                "the working run never takes a queued continuation"
            );
            let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
                .await
                .unwrap()
                .expect("the working run takes the pending steer");
            assert_eq!(prepared.thread_id.as_str(), "thread-1");
            assert_eq!(prepared.run_id.as_str(), "run:working");
            let framed = super::super::super::context::follow_up_context_line(
                "item:steer",
                1,
                "Prefer the smaller fix".to_string(),
                false,
            )
            .unwrap();
            assert_eq!(prepared.prompt, framed.line);
            assert_eq!(
                prepared.delivered,
                DeliveredSource {
                    source_id: "item:steer".to_string(),
                    content_sha256: framed.content_sha256,
                    kind: DeliveryKind::Steer,
                }
            );
        }

        #[tokio::test]
        async fn held_replies_wait_and_steers_reach_the_run_in_queue_order() {
            use crate::chat::organization::inputs::{
                reorder_assignment_inputs, set_assignment_input_held, steer_assignment_input,
            };
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            seed_input(&pool, "item:second", 5, "steer", "Keep the tests green").await;
            sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
                .execute(&pool)
                .await
                .unwrap();
            let item = |id: &str| ChatConversationItemId::new(id).unwrap();
            let input = |id: &str| ChatAssignmentInputId::new(format!("input:{id}")).unwrap();

            let held = set_assignment_input_held(&pool, &input("item:steer"), true)
                .await
                .unwrap();
            assert!(held.held);
            assert_eq!(
                pending_live_input_ids(&pool, &assignment()).await.unwrap(),
                vec![item("item:second")],
                "delivery takes the first reply no one holds"
            );
            assert!(
                prepare_assignment_input(&pool, &assignment(), &item("item:steer"))
                    .await
                    .unwrap()
                    .is_none(),
                "the working run never takes a held reply"
            );

            set_assignment_input_held(&pool, &input("item:steer"), false)
                .await
                .unwrap();
            assert!(
                prepare_assignment_input(&pool, &assignment(), &item("item:steer"))
                    .await
                    .unwrap()
                    .is_some(),
                "a resumed reply is eligible again"
            );
            let reordered = reorder_assignment_inputs(
                &pool,
                &assignment(),
                &[
                    input("item:second"),
                    input("item:continuation"),
                    input("item:steer"),
                ],
            )
            .await
            .unwrap();
            let order = reordered
                .iter()
                .filter(|read| read.routing_kind != "follow_up")
                .map(|read| (read.message_item_id.as_str(), read.ordinal))
                .collect::<Vec<_>>();
            assert_eq!(
                order,
                [
                    ("item:second", 3),
                    ("item:continuation", 4),
                    ("item:steer", 5)
                ],
                "the reordered replies take the slots they held"
            );
            assert_eq!(
                pending_live_input_ids(&pool, &assignment()).await.unwrap(),
                vec![item("item:second"), item("item:steer")]
            );

            set_assignment_input_held(&pool, &input("item:continuation"), true)
                .await
                .unwrap();
            let steered = steer_assignment_input(&pool, &input("item:continuation"))
                .await
                .unwrap();
            assert_eq!(
                (steered.routing_kind.as_str(), steered.held),
                ("steer", false),
                "steering now resumes a held reply"
            );
            assert_eq!(
                pending_live_input_ids(&pool, &assignment()).await.unwrap(),
                vec![
                    item("item:second"),
                    item("item:continuation"),
                    item("item:steer")
                ]
            );
        }

        #[tokio::test]
        async fn a_steered_input_records_its_delivery_on_the_run_once() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
                .execute(&pool)
                .await
                .unwrap();
            let steer = ChatConversationItemId::new("item:steer").unwrap();
            let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
                .await
                .unwrap()
                .expect("the working run takes the pending steer");
            assert_eq!(prepared.delivered.kind, DeliveryKind::Steer);

            for _ in 0..2 {
                record_assignment_input_delivered(&pool, &assignment(), &steer, &prepared)
                    .await
                    .unwrap();
            }

            let rows: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT source_id, content_sha256, delivery_kind
                 FROM chat_agent_run_delivered_sources WHERE run_id = 'run:working'",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(
                rows,
                vec![(
                    "item:steer".to_string(),
                    prepared.delivered.content_sha256.clone(),
                    "steer".to_string()
                )]
            );
            let states = delivery_states(&pool).await;
            assert!(states.contains(&("steer".to_string(), "delivered".to_string())));
        }

        #[tokio::test]
        async fn a_steer_the_provider_read_stays_delivered_over_a_later_cancel() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
                .execute(&pool)
                .await
                .unwrap();
            let steer = ChatConversationItemId::new("item:steer").unwrap();
            let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
                .await
                .unwrap()
                .expect("the working run takes the pending steer");
            sqlx::query(
                "UPDATE chat_work_assignment_inputs SET delivery_state = 'ignored'
                 WHERE message_item_id = 'item:steer'",
            )
            .execute(&pool)
            .await
            .unwrap();

            record_assignment_input_delivered(&pool, &assignment(), &steer, &prepared)
                .await
                .unwrap();

            let (state, revision): (String, Option<String>) = sqlx::query_as(
                "SELECT delivery_state, delivered_revision_id
                 FROM chat_work_assignment_inputs WHERE message_item_id = 'item:steer'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(state, "delivered");
            assert_eq!(revision.as_deref(), Some(prepared.revision_id.as_str()));
            let sources: Vec<(String,)> = sqlx::query_as(
                "SELECT source_id FROM chat_agent_run_delivered_sources WHERE run_id = 'run:working'",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(sources, vec![("item:steer".to_string(),)]);
        }

        /// Posts agent output in the assignment's reply thread after every seeded reply.
        async fn seed_agent_output(pool: &SqlitePool) {
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "INSERT INTO chat_conversation_items
                    (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
                 SELECT 'item:agent-output', conversation_id, id, 'work_update', 'a9',
                        '{CLAIMED_AT}'
                 FROM chat_reply_threads WHERE id = 'reply:folders';"
            )))
            .execute(pool)
            .await
            .unwrap();
        }

        /// The ids of the reply thread's items in thread order.
        async fn thread_order(pool: &SqlitePool) -> Vec<String> {
            sqlx::query_scalar(
                "SELECT id FROM chat_conversation_items WHERE reply_thread_id = 'reply:folders'
                 ORDER BY order_key, id",
            )
            .fetch_all(pool)
            .await
            .unwrap()
        }

        async fn shown(pool: &SqlitePool, item_id: &str) -> ChatMessageRead {
            let item_id = ChatConversationItemId::new(item_id).unwrap();
            crate::chat::organization::reads::read_message(pool, &item_id)
                .await
                .unwrap()
        }

        async fn reply_count(pool: &SqlitePool) -> u64 {
            let thread = ChatReplyThreadId::new("reply:folders").unwrap();
            crate::chat::organization::reads::read_reply_thread_summary(pool, &thread)
                .await
                .unwrap()
                .reply_count
        }

        async fn delivered_at(pool: &SqlitePool, item_id: &str) -> String {
            sqlx::query_scalar(
                "SELECT delivered_at FROM chat_work_assignment_inputs WHERE message_item_id = ?",
            )
            .bind(item_id)
            .fetch_one(pool)
            .await
            .unwrap()
        }

        #[tokio::test]
        async fn a_steer_waits_out_of_the_thread_and_enters_it_after_the_output_the_run_had() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            seed_agent_output(&pool).await;
            sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
                .execute(&pool)
                .await
                .unwrap();
            let waiting = shown(&pool, "item:steer").await;
            assert!(waiting.awaiting_delivery);
            assert!(shown(&pool, "item:continuation").await.awaiting_delivery);
            assert!(!shown(&pool, "item:follow-up").await.awaiting_delivery);
            let replies_while_waiting = reply_count(&pool).await;

            let steer = ChatConversationItemId::new("item:steer").unwrap();
            let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
                .await
                .unwrap()
                .expect("the working run takes the pending steer");
            record_assignment_input_delivered(&pool, &assignment(), &steer, &prepared)
                .await
                .unwrap();

            let entered = shown(&pool, "item:steer").await;
            assert!(!entered.awaiting_delivery);
            assert_eq!(
                entered.created_at.as_str(),
                delivered_at(&pool, "item:steer").await
            );
            assert_eq!(entered.normalized_markdown, waiting.normalized_markdown);
            assert_eq!(entered.revision_id, waiting.revision_id);
            assert_eq!(
                thread_order(&pool).await.last().map(String::as_str),
                Some("item:steer")
            );
            assert_eq!(reply_count(&pool).await, replies_while_waiting + 1);
            let posted: String = sqlx::query_scalar(
                "SELECT created_at FROM chat_communication_messages WHERE item_id = 'item:steer'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(posted, CLAIMED_AT, "the message keeps its posting time");
        }

        #[tokio::test]
        async fn a_launch_places_the_answers_it_carried_and_leaves_package_inputs_in_place() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            seed_agent_output(&pool).await;
            let before = thread_order(&pool).await;
            let binding = working_binding(&pool).await;
            settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
                .await
                .unwrap();
            let launch = AssignmentLaunch {
                run_id: ChatAgentRunId::new("run:working").unwrap(),
                launch_error: None,
                settlement: None,
                answered_input_ids: vec!["item:continuation".to_string()],
            };
            assert!(
                mark_initial_assignment_input_delivered(&pool, &assignment(), &launch)
                    .await
                    .unwrap()
            );

            let mut expected = before
                .into_iter()
                .filter(|id| id != "item:continuation")
                .collect::<Vec<_>>();
            expected.push("item:continuation".to_string());
            assert_eq!(thread_order(&pool).await, expected);
            let follow_up = shown(&pool, "item:follow-up").await;
            assert_eq!(follow_up.created_at.as_str(), CLAIMED_AT);
            assert_eq!(follow_up.order_key, "a2");
            let answer = shown(&pool, "item:continuation").await;
            assert_eq!(
                answer.created_at.as_str(),
                delivered_at(&pool, "item:continuation").await
            );
            assert!(shown(&pool, "item:steer").await.awaiting_delivery);
        }

        #[tokio::test]
        async fn held_and_cancelled_replies_stay_out_and_a_failed_work_shows_its_waiting_reply() {
            let pool = starting_pool().await;
            seed_inputs_of_every_route(&pool).await;
            let held = ChatAssignmentInputId::new("input:item:continuation").unwrap();
            crate::chat::organization::inputs::set_assignment_input_held(&pool, &held, true)
                .await
                .unwrap();
            assert!(shown(&pool, "item:continuation").await.awaiting_delivery);
            let cancelled = ChatAssignmentInputId::new("input:item:steer").unwrap();
            crate::chat::organization::inputs::cancel_assignment_input(&pool, &cancelled)
                .await
                .unwrap();
            let gone = shown(&pool, "item:steer").await;
            assert!(gone.awaiting_delivery);
            assert_eq!(gone.order_key, "a3", "a cancelled reply never moves");

            sqlx::query(
                "UPDATE chat_work_assignments SET state = 'failed'
                 WHERE id = 'assignment:folders'",
            )
            .execute(&pool)
            .await
            .unwrap();
            assert!(!shown(&pool, "item:continuation").await.awaiting_delivery);
            assert!(shown(&pool, "item:steer").await.awaiting_delivery);
        }

        async fn run_and_job(
            pool: &SqlitePool,
        ) -> (String, Option<String>, String, Option<String>) {
            sqlx::query_as(
                "SELECT run.state, run.settled_at, job.state, job.last_error
                 FROM chat_agent_runs run
                 JOIN chat_assignment_dispatch_jobs job ON job.assignment_id = run.assignment_id
                 WHERE run.id = 'run:working'",
            )
            .fetch_one(pool)
            .await
            .unwrap()
        }

        #[tokio::test]
        async fn recovery_settles_the_starting_run_of_a_stale_claim_before_requeueing_it() {
            let pool = starting_pool().await;
            let now = "2026-10-20T12:06:00.000Z";

            reclaim_stale_dispatch_claims(&pool, &at(now))
                .await
                .unwrap();

            assert_eq!(
                run_and_job(&pool).await,
                (
                    "failed".to_string(),
                    Some(now.to_string()),
                    "queued".to_string(),
                    Some("launch_interrupted".to_string())
                )
            );
            let binding = working_binding(&pool).await;
            let late = settle_launch(
                &pool,
                &binding,
                &folder_launch(),
                None,
                &at("2026-10-20T12:07:00.000Z"),
            )
            .await
            .unwrap();
            assert_eq!(late, LaunchSettlement::RunSettled);
            assert_eq!(run_and_job(&pool).await.0, "failed");
            assert_eq!(assignment_and_job(&pool).await, pair("queued", "queued"));
        }

        /// The fixture launch as a continuation of an earlier native session.
        fn continuation_launch(reason: AttemptReason) -> RunLaunch {
            let launch = folder_launch();
            RunLaunch {
                attempt: RunAttempt {
                    reason,
                    context_delivery: ContextDelivery::Delta,
                    ..launch.attempt
                },
                ..launch
            }
        }

        fn session_lost() -> ChatError {
            ChatError::new(
                ChatErrorCode::ResumeNotFound,
                "The provider session was not found",
                true,
            )
        }

        /// Opens an approval on the fixture provider thread, as a session that is later lost
        /// leaves it.
        async fn open_approval(pool: &SqlitePool) {
            sqlx::query(
                "INSERT INTO chat_pending_requests
                    (id, thread_id, turn_id, provider_request_id, request_kind,
                     safe_display_data, allowed_decisions_data, opened_sequence, opened_at)
                 VALUES ('request:detached', 'thread-1', 'turn:working', 'native-request',
                         'approval', '{}', '[]', 1, ?)",
            )
            .bind(CLAIMED_AT)
            .execute(pool)
            .await
            .unwrap();
        }

        async fn approval_state(pool: &SqlitePool) -> (String, Option<String>) {
            sqlx::query_as(
                "SELECT resolution_state, resolved_at FROM chat_pending_requests
                 WHERE id = 'request:detached'",
            )
            .fetch_one(pool)
            .await
            .unwrap()
        }

        #[tokio::test]
        async fn a_continuation_that_lost_its_session_fails_only_its_run_and_asks_for_a_handoff() {
            let pool = starting_pool().await;
            open_approval(&pool).await;
            let binding = working_binding(&pool).await;
            let now = "2026-10-20T12:01:00.000Z";

            let settlement = settle_launch(
                &pool,
                &binding,
                &continuation_launch(AttemptReason::Continuation),
                Some(&session_lost()),
                &at(now),
            )
            .await
            .unwrap();

            assert_eq!(settlement, LaunchSettlement::HandoffRequired);
            assert!(settlement.stops_session());
            assert_eq!(
                run_and_job(&pool).await,
                (
                    "failed".to_string(),
                    Some(now.to_string()),
                    "claimed".to_string(),
                    None
                )
            );
            assert_eq!(assignment_and_job(&pool).await, pair("queued", "claimed"));
            assert_eq!(failure_messages(&pool).await, 0);
            assert!(delivered_sources(&pool).await.is_empty());
            assert_eq!(
                approval_state(&pool).await,
                ("interrupted".to_string(), Some(now.to_string())),
                "an approval of the lost session is closed, never left open"
            );
        }

        #[tokio::test]
        async fn a_lost_session_fails_the_assignment_once_the_run_is_no_longer_a_continuation() {
            for reason in [
                AttemptReason::Handoff(HandoffReason::NativeSessionLost),
                AttemptReason::Retry,
                AttemptReason::Fresh,
            ] {
                let pool = starting_pool().await;
                let binding = working_binding(&pool).await;

                let settlement = settle_launch(
                    &pool,
                    &binding,
                    &continuation_launch(reason),
                    Some(&session_lost()),
                    &at("2026-10-20T12:01:00.000Z"),
                )
                .await
                .unwrap();

                assert_eq!(settlement, LaunchSettlement::Recorded, "{reason:?}");
                assert_eq!(assignment_and_job(&pool).await, pair("failed", "failed"));
                assert_eq!(failure_messages(&pool).await, 1);
            }

            let pool = starting_pool().await;
            sqlx::query(
                "UPDATE chat_work_assignments SET state = 'cancelled',
                     settled_at = '2026-10-20T12:00:30.000Z'
                 WHERE id = 'assignment:folders'",
            )
            .execute(&pool)
            .await
            .unwrap();
            let binding = working_binding(&pool).await;
            let settlement = settle_launch(
                &pool,
                &binding,
                &continuation_launch(AttemptReason::Continuation),
                Some(&session_lost()),
                &at("2026-10-20T12:01:00.000Z"),
            )
            .await
            .unwrap();
            assert_eq!(
                settlement,
                LaunchSettlement::Superseded,
                "a settled assignment never hands off"
            );
        }

        #[tokio::test]
        async fn a_restart_that_missed_the_handoff_retries_the_assignment_in_a_new_session() {
            let pool = starting_pool().await;
            let binding = working_binding(&pool).await;
            settle_launch(
                &pool,
                &binding,
                &continuation_launch(AttemptReason::Continuation),
                Some(&session_lost()),
                &at("2026-10-20T12:01:00.000Z"),
            )
            .await
            .unwrap();

            // The application stops before the handoff run starts, so the claimed job is
            // requeued once it is stale.
            reclaim_stale_dispatch_claims(&pool, &at("2026-10-20T12:06:00.000Z"))
                .await
                .unwrap();
            assert_eq!(assignment_and_job(&pool).await, pair("queued", "queued"));
            let earlier_runs: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM chat_agent_runs WHERE assignment_id = 'assignment:folders'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(earlier_runs > 0);

            let key = ContinuationKey {
                authorization_scope_digest: format!("{:064}", 0),
                provider_instance_id: "fake-instance".to_string(),
                model_selection_sha256: "d".repeat(64),
                instructions_sha256: "c".repeat(64),
            };
            let decision = continuation::decide_session_transition(&TransitionInput {
                key: &key,
                unfinished_earlier_run: earlier_runs > 0,
                candidate: Some(CandidateRun {
                    thread_id: "thread-1",
                    key: Some(key.clone()),
                    authorization_valid: true,
                }),
                native_session_lost: false,
            });
            assert_eq!(decision.reason, AttemptReason::Retry);
            assert_eq!(decision.reuse_thread, None);
        }

        #[tokio::test]
        async fn recovery_leaves_fresh_claims_and_working_runs_alone() {
            let pool = starting_pool().await;
            reclaim_stale_dispatch_claims(&pool, &at("2026-10-20T12:03:00.000Z"))
                .await
                .unwrap();
            assert_eq!(
                run_and_job(&pool).await,
                ("starting".to_string(), None, "claimed".to_string(), None)
            );

            sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
                .execute(&pool)
                .await
                .unwrap();
            reclaim_stale_dispatch_claims(&pool, &at("2026-10-20T12:06:00.000Z"))
                .await
                .unwrap();
            assert_eq!(
                run_and_job(&pool).await,
                ("working".to_string(), None, "queued".to_string(), None)
            );
        }
    }

    mod answered_question {
        use super::super::{
            AnsweredQuestion, AttemptReason, ContextDelivery, DeliveredSource, DeliveryKind,
            FullPackage, MAX_MESSAGE_BYTES, choose_context_delivery, continuation,
            frame_answered_question,
        };
        use ganbaru_chat::coordination::reports::ThreadMessage;
        use ganbaru_chat_contracts::models::ChatWorkAssignmentId;

        fn message(item_id: &str, markdown: &str) -> ThreadMessage {
            ThreadMessage {
                item_id: item_id.to_owned(),
                revision: 1,
                markdown: markdown.to_owned(),
            }
        }

        fn answered(answer: &str) -> AnsweredQuestion {
            AnsweredQuestion {
                asking_run_id: "run:1".to_owned(),
                question: message("item:question", "Keep the old page?"),
                answers: vec![message("item:answer", answer)],
            }
        }

        fn package() -> FullPackage {
            FullPackage {
                prompt: "{\"kind\":\"trigger\",\"text\":\"tidy the docs\"}\n".to_owned(),
                delivered_sources: vec![DeliveredSource {
                    source_id: "item:root".to_owned(),
                    content_sha256: "a".repeat(64),
                    kind: DeliveryKind::Package,
                }],
            }
        }

        #[test]
        fn a_continued_session_receives_only_the_answers_and_a_new_one_the_question_too() {
            let framed = frame_answered_question(&answered("Keep it")).unwrap();
            assert!(framed.fits_after(&package().prompt));
            assert_eq!(framed.input_ids, ["item:answer"]);
            let assignment_id = ChatWorkAssignmentId::new("assignment:1".to_owned()).unwrap();
            let delta = continuation::answer_delta(
                &assignment_id,
                1,
                &framed.answers,
                framed.sources.clone(),
            )
            .unwrap();
            let full = framed.after_package(package());
            let planned = choose_context_delivery(AttemptReason::Continuation, delta, full);

            assert_eq!(planned.context_delivery, ContextDelivery::Delta);
            assert!(planned.prompt.contains("Keep it"));
            assert!(
                !planned.prompt.contains("tidy the docs"),
                "the session holds the trigger"
            );
            assert!(!planned.prompt.contains("Keep the old page?"));
            assert_eq!(
                planned
                    .delivered_sources
                    .iter()
                    .map(|source| (source.source_id.as_str(), source.kind))
                    .collect::<Vec<_>>(),
                [("item:answer", DeliveryKind::QueuedContinuation)]
            );

            let fresh = planned.full_prompt_on_fresh_session.unwrap();
            let trigger = fresh.prompt.find("tidy the docs").unwrap();
            let question = fresh.prompt.find("Keep the old page?").unwrap();
            let answer = fresh.prompt.find("Keep it").unwrap();
            assert!(trigger < question && question < answer);
            assert_eq!(fresh.delivered_sources.len(), 2);
        }

        #[test]
        fn answers_that_do_not_fit_after_the_package_stay_pending() {
            let framed =
                frame_answered_question(&answered(&"x".repeat(MAX_MESSAGE_BYTES))).unwrap();
            assert!(!framed.fits_after(&package().prompt));
        }
    }
}
