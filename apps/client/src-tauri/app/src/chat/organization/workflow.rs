//! Assignment routing, frozen context, and durable organizational messages.

#[cfg(desktop)]
use super::assignments::read_assignment_subject;
#[cfg(desktop)]
use super::common::wire_work_state;
use super::common::{
    AttachmentSource, ScheduledPostIds, assignment_input_id, conversation_item_id,
    has_thread_eligible_mention, insert_revision_attachments, json_object, message_revision_id,
    new_id, parse_participant_kind, reply_thread_id, serialization_error, wire_participant_kind,
    wire_review_outcome, work_assignment_id,
};
#[cfg(desktop)]
use super::context::{FreezeOutcome, WorkToFreeze, freeze_context_package};
use super::inputs::place_delivered_reply;
#[cfg(desktop)]
use super::now_timestamp;
use super::reads::{
    read_active_or_latest_assignment, read_assignment, read_message, read_participant,
};
use super::{
    LOCAL_PARTICIPANT_ID, ReviewedWork, StoredPostMessageReceipt, i64_value, identifier_error,
    persistence_error, u64_value,
};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, SqliteConnection, SqlitePool, Transaction};

#[derive(Clone, Debug)]
pub(super) struct ResolvedInvocation {
    pub(super) agent_id: ChatParticipantId,
    pub(super) active_assignment: Option<ChatWorkAssignmentRead>,
    pub(super) latest_assignment: Option<ChatWorkAssignmentRead>,
}

/// Where a post routed its invocation.
pub(super) struct AssignmentWrite {
    pub(super) assignment_id: Option<ChatWorkAssignmentId>,
    /// Whether the post became an input of an active assignment.
    pub(super) input_queued: bool,
    /// How the queued input reaches the active work, when the post became one.
    pub(super) routed_as: Option<ChatFollowupRouting>,
    /// Whether this device executes the assignment, so it dispatches or delivers now. Only
    /// desktop executes assignments.
    #[cfg_attr(mobile, allow(dead_code))]
    pub(super) executes_here: bool,
    /// Whether the post created work, or a pending reply of reviewed work, that starts only
    /// through local acceptance.
    #[cfg_attr(mobile, allow(dead_code))]
    pub(super) awaits_acceptance: bool,
}

impl AssignmentWrite {
    /// A post that routed to no assignment.
    pub(super) const NONE: Self = Self {
        assignment_id: None,
        input_queued: false,
        routed_as: None,
        executes_here: false,
        awaits_acceptance: false,
    };

    /// Whether the post queued the dispatch of new work on this device, the only post that
    /// carries a local gesture's acceptance of what the work runs. An input of existing work
    /// starts nothing, since that work may still wait for Start here, and work awaiting
    /// acceptance has no dispatch.
    #[cfg(desktop)]
    pub(super) fn dispatches_here(&self) -> bool {
        self.executes_here && !self.input_queued && !self.awaits_acceptance
    }
}

pub(super) fn has_authority_bearing_references(references: &[ChatMessageReference]) -> bool {
    references.iter().any(|reference| {
        matches!(
            reference,
            ChatMessageReference::Channel { .. }
                | ChatMessageReference::WorkingFolder { .. }
                | ChatMessageReference::WorkspacePath { .. }
                | ChatMessageReference::ExecutionEnvironment { .. }
        )
    })
}

pub(super) fn require_continuation_scope_is_unchanged(
    has_active_assignment: bool,
    references: &[ChatMessageReference],
) -> ChatResult<()> {
    if has_active_assignment && has_authority_bearing_references(references) {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Resource references require a new linked assignment after current work settles",
            true,
        ));
    }
    Ok(())
}

pub(super) async fn resolve_invoked_agent(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
    reply_thread_id: Option<&ChatReplyThreadId>,
    references: &[ChatMessageReference],
) -> ChatResult<Option<ResolvedInvocation>> {
    let mut ai_mentions = Vec::new();
    for reference in references {
        let ChatMessageReference::Participant {
            participant_id,
            participant_kind,
            ..
        } = reference
        else {
            continue;
        };
        let participant = read_participant(pool, participant_id).await?;
        if participant.kind != *participant_kind {
            return Err(ChatError::validation(
                "references",
                "A participant reference has stale identity data",
            ));
        }
        if participant.kind == ChatParticipantKind::AiAgent {
            ai_mentions.push(participant_id.clone());
        }
    }
    ai_mentions.sort();
    ai_mentions.dedup();
    if ai_mentions.len() > 1 {
        return Err(ChatError::validation(
            "references",
            "Assign one AI agent at a time",
        ));
    }
    let latest_assignment = match reply_thread_id {
        Some(reply_thread_id) => read_active_or_latest_assignment(pool, reply_thread_id).await?,
        None => None,
    };
    let active_assignment = latest_assignment
        .as_ref()
        .filter(|assignment| assignment.state.accepts_continuation())
        .cloned();
    let agent_id = ai_mentions.into_iter().next().or_else(|| {
        latest_assignment
            .as_ref()
            .map(|assignment| assignment.agent.id.clone())
    });
    let Some(agent_id) = agent_id else {
        return Ok(None);
    };
    if active_assignment
        .as_ref()
        .is_some_and(|assignment| assignment.agent.id != agent_id)
    {
        return Err(ChatError::validation(
            "references",
            "This reply thread already has an active AI agent",
        ));
    }
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    require_participating_agent(&mut connection, conversation_id, &agent_id).await?;
    Ok(Some(ResolvedInvocation {
        agent_id,
        active_assignment,
        latest_assignment,
    }))
}

/// The devices that can write Chat, and the providers configured on this device. An assignment
/// writes its states, messages, and runs into Chat, so only these devices execute assignments:
/// this device and the linked devices that hold an active sync writer.
#[derive(Clone, Debug, Default)]
pub(super) struct ChatWriters {
    devices: std::collections::BTreeSet<String>,
    #[cfg(desktop)]
    providers: super::provider_agents::DeviceProviders,
}

impl ChatWriters {
    /// The writers `devices`, with no provider configured on this device.
    pub(super) fn new(devices: impl IntoIterator<Item = String>) -> Self {
        Self {
            devices: devices.into_iter().collect(),
            #[cfg(desktop)]
            providers: super::provider_agents::DeviceProviders::default(),
        }
    }

    /// These writers with `providers` configured on this device.
    #[cfg(desktop)]
    pub(super) fn with_providers(self, providers: super::provider_agents::DeviceProviders) -> Self {
        Self { providers, ..self }
    }

    /// The providers configured on this device.
    #[cfg(desktop)]
    pub(super) fn providers(&self) -> &super::provider_agents::DeviceProviders {
        &self.providers
    }

    /// Whether the device `device_id` can write Chat.
    pub(super) fn includes(&self, device_id: &str) -> bool {
        self.devices.contains(device_id)
    }

    /// Whether the provider of `family` can start new work on this device. Android runs no
    /// provider.
    pub(super) fn provider_ready_here(&self, family: &str) -> bool {
        #[cfg(desktop)]
        {
            self.providers
                .get(family)
                .is_some_and(|provider| provider.ready)
        }
        #[cfg(mobile)]
        {
            let _ = family;
            false
        }
    }

    /// The writer ids as a JSON array, for queries that read them with `json_each`.
    #[cfg(desktop)]
    pub(super) fn json(&self) -> ChatResult<String> {
        serde_json::to_string(&self.devices).map_err(serialization_error)
    }
}

/// The device writing a post, which decides where the work a mention starts runs.
#[derive(Clone, Copy, Debug)]
pub(super) struct PostingDevice<'a> {
    /// Stable id of this device.
    pub(super) id: &'a str,
    /// Whether this device executes assignments; Android never does.
    pub(super) can_execute: bool,
    /// Devices that can write Chat, the only ones an assignment may run on.
    pub(super) chat_writers: &'a ChatWriters,
    /// Whether no one on this device consented to the post, so new work it creates here waits
    /// for local acceptance instead of queueing its dispatch.
    pub(super) unattended: bool,
}

impl<'a> PostingDevice<'a> {
    /// This device under its stable id `id`, with the Chat writers `chat_writers`.
    pub(super) fn this_device(id: &'a str, chat_writers: &'a ChatWriters) -> Self {
        Self {
            id,
            can_execute: cfg!(desktop),
            chat_writers,
            unattended: false,
        }
    }

    /// This device writing a post no one on it consented to.
    pub(super) fn unattended(self) -> Self {
        Self {
            unattended: true,
            ..self
        }
    }

    /// Whether this device executes the work of `executing_device_id`.
    pub(super) fn executes(&self, executing_device_id: Option<&str>) -> bool {
        self.can_execute && executing_device_id == Some(self.id)
    }

    /// Whether this device can run new work: it executes assignments and writes Chat.
    pub(super) fn runs_new_work(&self) -> bool {
        self.can_execute && self.chat_writers.includes(self.id)
    }

    /// Whether the post must leave existing work as it is: no one on this executing device
    /// consented to it, so it steers no active work and settles no reviewed work. Android
    /// neither records consent nor executes work, so its scheduled sends route as its user's
    /// posts and the executing desktop applies its own gates.
    pub(super) fn leaves_existing_work(&self) -> bool {
        self.unattended && self.can_execute
    }
}

/// The device an assignment of `agent_id` runs on when `device` creates it: the posting device
/// when it can run new work and the agent's provider is ready there, else none, so the post
/// records the mention only and a person starts the work later on a device that can run it.
pub(super) async fn new_assignment_executing_device(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &ChatParticipantId,
    device: PostingDevice<'_>,
) -> ChatResult<Option<String>> {
    if !device.runs_new_work() {
        return Ok(None);
    }
    let family: Option<String> =
        sqlx::query_scalar("SELECT provider_family_id FROM chat_agents WHERE participant_id = ?")
            .bind(agent_id.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?;
    Ok(family
        .filter(|family| device.chat_writers.provider_ready_here(family))
        .map(|_| device.id.to_owned()))
}

/// The executing device of assignment `assignment_id`, if it names one.
pub(super) async fn read_assignment_executing_device<'e, E>(
    executor: E,
    assignment_id: &str,
) -> ChatResult<Option<String>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let executing_device_id: Option<Option<String>> =
        sqlx::query_scalar("SELECT executing_device_id FROM chat_work_assignments WHERE id = ?")
            .bind(assignment_id)
            .fetch_optional(executor)
            .await
            .map_err(persistence_error)?;
    Ok(executing_device_id.flatten())
}

/// Appends the post `message_item_id` to `assignment_id` as its next pending input.
async fn append_assignment_input(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    message_item_id: &ChatConversationItemId,
    routing_kind: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let ordinal: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(ordinal), 0) + 1
         FROM chat_work_assignment_inputs WHERE assignment_id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(assignment_input_id(message_item_id.as_str())?)
    .bind(assignment_id.as_str())
    .bind(message_item_id.as_str())
    .bind(ordinal)
    .bind(routing_kind)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// What a post asked of the work it routes to.
#[derive(Clone, Copy, Default)]
pub(super) struct PostRoutingChoices<'a> {
    /// The execution target the post chose for the work it starts.
    pub(super) execution_target: Option<&'a ChatExecutionTarget>,
    /// Whether the post steers or queues behind active work.
    pub(super) followup_routing: Option<ChatFollowupRouting>,
}

/// Routes the post `message_item_id` to the assignment of the agent it invokes: an input of
/// the active assignment, else a new assignment that runs on the agent's home device or the
/// posting desktop. A post from a device that cannot execute, for an agent without a home
/// device, records the mention only, until a Start creates its work.
///
/// An input of active work follows the `followup_routing` of `choices`: Steer reaches a working
/// run's current turn and queues for any other state, Queue always waits for the current turn
/// to end, and `None` steers working work and queues anything else.
///
/// A post no one on this executing device consented to leaves existing work as it is. It
/// records only the mention when the thread has active work. A reply to work that asked for
/// review becomes a pending reply of that work, which waits for Start here or for a local
/// acceptance of the agent before a follow-up settles the review, and this device keeps the
/// execution target the post chose for that follow-up; a mention of another
/// agent there creates that agent's new work awaiting Start here and leaves the review
/// open.
pub(super) async fn persist_assignment_routing(
    transaction: &mut Transaction<'_, Sqlite>,
    reply_thread_id: &ChatReplyThreadId,
    message_item_id: &ChatConversationItemId,
    invocation: Option<&ResolvedInvocation>,
    choices: PostRoutingChoices<'_>,
    device: PostingDevice<'_>,
    now: &UtcTimestamp,
) -> ChatResult<AssignmentWrite> {
    let PostRoutingChoices {
        execution_target,
        followup_routing,
    } = choices;
    let Some(invocation) = invocation else {
        return Ok(AssignmentWrite::NONE);
    };
    if let Some(active) = &invocation.active_assignment {
        if device.leaves_existing_work() {
            // By design an unattended reply on active work stays a plain message: it would
            // steer a run no one here agreed to, and the user can repeat it themselves.
            return Ok(AssignmentWrite::NONE);
        }
        let routed_as = active_input_routing(active.state, followup_routing);
        let routing_kind = match routed_as {
            ChatFollowupRouting::Steer => "steer",
            ChatFollowupRouting::Queue => "queued_continuation",
        };
        append_assignment_input(transaction, &active.id, message_item_id, routing_kind, now)
            .await?;
        let executing_device_id =
            read_assignment_executing_device(&mut **transaction, active.id.as_str()).await?;
        return Ok(AssignmentWrite {
            assignment_id: Some(active.id.clone()),
            input_queued: true,
            routed_as: Some(routed_as),
            executes_here: device.executes(executing_device_id.as_deref()),
            awaits_acceptance: false,
        });
    }
    let mut latest_assignment = invocation.latest_assignment.as_ref();
    let reviewed = latest_assignment
        .filter(|latest| latest.state == ChatWorkAssignmentState::ReadyForReview)
        .filter(|_| device.leaves_existing_work());
    if reviewed.is_some_and(|reviewed| reviewed.agent.id != invocation.agent_id) {
        // New work for another agent does not follow the reviewed work, so creating it
        // leaves that review open; it waits for Start here like any unattended new work.
        latest_assignment = None;
    }
    if let Some(reviewed) = reviewed.filter(|_| latest_assignment.is_some()) {
        append_assignment_input(
            transaction,
            &reviewed.id,
            message_item_id,
            "queued_continuation",
            now,
        )
        .await?;
        let executing_device_id =
            read_assignment_executing_device(&mut **transaction, reviewed.id.as_str()).await?;
        let executes_here = device.executes(executing_device_id.as_deref());
        #[cfg(desktop)]
        if let Some(target) = execution_target.filter(|_| executes_here) {
            record_input_requested_target(transaction, message_item_id, target, now).await?;
        }
        return Ok(AssignmentWrite {
            assignment_id: Some(reviewed.id.clone()),
            input_queued: true,
            routed_as: Some(ChatFollowupRouting::Queue),
            executes_here,
            awaits_acceptance: executes_here,
        });
    }
    let Some(executing_device_id) =
        new_assignment_executing_device(transaction, &invocation.agent_id, device).await?
    else {
        return Ok(AssignmentWrite::NONE);
    };
    let assignment_id = create_assignment(
        transaction,
        NewAssignment {
            reply_thread_id,
            message_item_id,
            agent_id: &invocation.agent_id,
            latest_assignment,
            executing_device_id: &executing_device_id,
            execution_target,
            poster_participant_id: LOCAL_PARTICIPANT_ID,
            delegation: None,
        },
        device,
        now,
    )
    .await?;
    let executes_here = device.executes(Some(&executing_device_id));
    Ok(AssignmentWrite {
        assignment_id: Some(assignment_id),
        input_queued: false,
        routed_as: None,
        executes_here,
        awaits_acceptance: executes_here && device.unattended,
    })
}

/// How a reply reaches active work in `state` when its post asked for `requested`: only a
/// working run has a turn to steer, so a steer on work in any other state queues.
pub(super) fn active_input_routing(
    state: ChatWorkAssignmentState,
    requested: Option<ChatFollowupRouting>,
) -> ChatFollowupRouting {
    match requested {
        Some(ChatFollowupRouting::Queue) => ChatFollowupRouting::Queue,
        _ if state == ChatWorkAssignmentState::Working => ChatFollowupRouting::Steer,
        _ => ChatFollowupRouting::Queue,
    }
}

/// An assignment a post or a Start action creates.
pub(super) struct NewAssignment<'a> {
    pub(super) reply_thread_id: &'a ChatReplyThreadId,
    /// Message that triggers it.
    pub(super) message_item_id: &'a ChatConversationItemId,
    pub(super) agent_id: &'a ChatParticipantId,
    /// Newest assignment of the reply thread, which the new one follows.
    pub(super) latest_assignment: Option<&'a ChatWorkAssignmentRead>,
    pub(super) executing_device_id: &'a str,
    pub(super) execution_target: Option<&'a ChatExecutionTarget>,
    /// Participant who posted the trigger, recorded as the reviewer when the new assignment
    /// follows reviewed work.
    pub(super) poster_participant_id: &'a str,
    /// The delegation that creates it, when an agent delegated it. Without one, a successor
    /// of delegated work inherits its predecessor's delegation.
    pub(super) delegation: Option<DelegationWrite<'a>>,
}

/// The fixed delegation fields of an assignment an agent delegated.
#[derive(Clone, Copy, Debug)]
pub(super) struct DelegationWrite<'a> {
    pub(super) parent_assignment_id: &'a ChatWorkAssignmentId,
    /// Levels below work a person started: 1 or 2.
    pub(super) depth: u8,
    pub(super) objective: &'a str,
    pub(super) expected_result: &'a str,
    /// JSON array of `{workingFolderId, capability}`, the most the child may ever hold.
    pub(super) folder_ceiling: &'a str,
}

/// The fixed delegation fields of a delegated assignment, owned for a successor to inherit.
struct DelegationFields {
    parent_assignment_id: ChatWorkAssignmentId,
    depth: u8,
    objective: String,
    expected_result: String,
    folder_ceiling: String,
}

impl DelegationFields {
    fn write(&self) -> DelegationWrite<'_> {
        DelegationWrite {
            parent_assignment_id: &self.parent_assignment_id,
            depth: self.depth,
            objective: &self.objective,
            expected_result: &self.expected_result,
            folder_ceiling: &self.folder_ceiling,
        }
    }
}

/// Reads the delegation fields of `assignment_id`, or none when a person started it. A
/// successor of delegated work inherits them, so it stays under the parent's ceiling.
async fn read_delegation_fields(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Option<DelegationFields>> {
    let row = sqlx::query(
        "SELECT parent_assignment_id, delegation_depth, delegation_objective,
                delegation_expected_result, delegation_folder_ceiling
         FROM chat_work_assignments WHERE id = ? AND parent_assignment_id IS NOT NULL",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let parent_assignment_id = ChatWorkAssignmentId::new(
        row.try_get::<String, _>("parent_assignment_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let depth = u8::try_from(
        row.try_get::<i64, _>("delegation_depth")
            .map_err(persistence_error)?,
    )
    .map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "The delegated assignment has an invalid depth",
            false,
        )
    })?;
    Ok(Some(DelegationFields {
        parent_assignment_id,
        depth,
        objective: row
            .try_get("delegation_objective")
            .map_err(persistence_error)?,
        expected_result: row
            .try_get("delegation_expected_result")
            .map_err(persistence_error)?,
        folder_ceiling: row
            .try_get("delegation_folder_ceiling")
            .map_err(persistence_error)?,
    }))
}

/// Writes the queued assignment `assignment` with its trigger input, settling a reviewed
/// predecessor as completed with changes requested by the poster, since more work was asked
/// for. A trigger message that is already an input moves to the new assignment, since
/// its input id derives from the message, and the other pending replies of the same reply
/// thread that a reviewed or completed predecessor holds follow it in order, steered into the
/// first turn so they reach it together. The execution targets kept for the predecessor's
/// pending replies go, since no reply waits on it anymore. When this device executes it and
/// someone here consented to the post, the same transaction freezes its context and queues its
/// dispatch; otherwise the executing device does both when the assignment passes its acceptance
/// check.
///
/// A successor of delegated work keeps its predecessor's delegation fields unless the caller
/// names others, so rerouted and follow-up work stays under the parent's ceiling.
pub(super) async fn create_assignment(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment: NewAssignment<'_>,
    device: PostingDevice<'_>,
    now: &UtcTimestamp,
) -> ChatResult<ChatWorkAssignmentId> {
    let previous_id = assignment.latest_assignment.map(|latest| &latest.id);
    let assignment_id =
        work_assignment_id(assignment.message_item_id, assignment.agent_id, previous_id)?;
    let inherited = match (assignment.delegation, previous_id) {
        (None, Some(previous_id)) => read_delegation_fields(transaction, previous_id).await?,
        _ => None,
    };
    let delegation = assignment
        .delegation
        .or_else(|| inherited.as_ref().map(DelegationFields::write));
    let reviewed_previous = assignment
        .latest_assignment
        .filter(|latest| latest.state == ChatWorkAssignmentState::ReadyForReview);
    if let Some(previous) = reviewed_previous {
        let settled = complete_reviewed_assignment(
            transaction,
            &previous.id,
            None,
            ChatAssignmentReviewOutcome::ChangesRequested,
            assignment.poster_participant_id,
            now,
        )
        .await?;
        if !settled {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "The reviewed assignment changed before the follow-up was created",
                true,
            ));
        }
    }
    let settings = read_new_work_settings(transaction, &assignment, previous_id).await?;
    sqlx::query(
        "INSERT INTO chat_work_assignments
            (id, reply_thread_id, agent_id, triggering_message_item_id,
             previous_assignment_id, executing_device_id, state,
             model_selection_schema_version, model_selection_data, instructions,
             created_at, updated_at, parent_assignment_id, delegation_depth,
             delegation_objective, delegation_expected_result, delegation_folder_ceiling)
         VALUES (?, ?, ?, ?, ?, ?, 'queued', 1, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(assignment_id.as_str())
    .bind(assignment.reply_thread_id.as_str())
    .bind(assignment.agent_id.as_str())
    .bind(assignment.message_item_id.as_str())
    .bind(previous_id.map(ChatWorkAssignmentId::as_str))
    .bind(assignment.executing_device_id)
    .bind(serde_json::to_string(&settings.model_selection).map_err(serialization_error)?)
    .bind(&settings.instructions)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(delegation.map(|delegation| delegation.parent_assignment_id.as_str()))
    .bind(i64::from(
        delegation.map_or(0, |delegation| delegation.depth),
    ))
    .bind(delegation.map(|delegation| delegation.objective))
    .bind(delegation.map(|delegation| delegation.expected_result))
    .bind(delegation.map(|delegation| delegation.folder_ceiling))
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let trigger_input_id = assignment_input_id(assignment.message_item_id.as_str())?;
    // A reply that waited for the previous turn enters the thread when it starts this work.
    let trigger_waited: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM chat_work_assignment_inputs
            WHERE id = ? AND routing_kind IN ('steer', 'queued_continuation')
              AND delivery_state = 'pending')",
    )
    .bind(&trigger_input_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if trigger_waited {
        place_delivered_reply(
            transaction,
            assignment.message_item_id.as_str(),
            now.as_str(),
        )
        .await?;
    }
    sqlx::query(
        "INSERT INTO chat_work_assignment_inputs
            (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
         VALUES (?, ?, ?, 1, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            assignment_id = excluded.assignment_id, ordinal = 1,
            routing_kind = excluded.routing_kind",
    )
    .bind(&trigger_input_id)
    .bind(assignment_id.as_str())
    .bind(assignment.message_item_id.as_str())
    .bind(if previous_id.is_some() {
        "follow_up"
    } else {
        "trigger"
    })
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let carrying_previous = assignment.latest_assignment.filter(|latest| {
        matches!(
            latest.state,
            ChatWorkAssignmentState::ReadyForReview | ChatWorkAssignmentState::Completed
        )
    });
    if let Some(previous) = carrying_previous {
        let carried = pending_thread_inputs(transaction, &previous.id, assignment.reply_thread_id)
            .await?
            .into_iter()
            .filter(|input| input.message_item_id != assignment.message_item_id.as_str())
            .map(|input| input.id)
            .collect::<Vec<_>>();
        move_pending_inputs(transaction, &carried, &assignment_id, Some("steer")).await?;
        drop_input_requested_targets(transaction, &previous.id).await?;
        drop_input_requested_targets(transaction, &assignment_id).await?;
    }
    #[cfg(desktop)]
    if device.executes(Some(assignment.executing_device_id)) {
        if device.unattended {
            if let Some(target) = assignment.execution_target {
                record_requested_target(transaction, &assignment_id, target, now).await?;
            }
        } else {
            queue_assignment_dispatch(
                transaction,
                WorkToFreeze {
                    assignment_id: &assignment_id,
                    reply_thread_id: assignment.reply_thread_id,
                    triggering_message_item_id: assignment.message_item_id,
                    agent_id: assignment.agent_id,
                },
                assignment.execution_target,
                device.chat_writers.providers(),
                now,
            )
            .await?;
        }
    }
    #[cfg(mobile)]
    let _ = (device, assignment.execution_target);
    Ok(assignment_id)
}

/// Resolves the model and instructions `assignment` freezes from its trigger and its channel.
async fn read_new_work_settings(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment: &NewAssignment<'_>,
    previous_id: Option<&ChatWorkAssignmentId>,
) -> ChatResult<super::model_choice::FrozenWorkSettings> {
    let (conversation_id, trigger_text): (String, String) = sqlx::query_as(
        "SELECT thread.conversation_id, coalesce(revision.normalized_markdown, '')
         FROM chat_reply_threads thread
         LEFT JOIN chat_communication_messages message ON message.item_id = ?
         LEFT JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE thread.id = ?",
    )
    .bind(assignment.message_item_id.as_str())
    .bind(assignment.reply_thread_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    super::model_choice::resolve_frozen_work_settings(
        transaction,
        &conversation_id,
        assignment.agent_id.as_str(),
        &trigger_text,
        previous_id.map(ChatWorkAssignmentId::as_str),
    )
    .await
}

/// A pending input and the message it delivers.
pub(super) struct PendingInput {
    pub(super) id: String,
    pub(super) message_item_id: String,
    /// Whether a person holds it back from delivery.
    #[cfg(desktop)]
    pub(super) held: bool,
}

/// Reads, in delivery order, the pending replies of `assignment_id` whose messages belong to
/// `reply_thread_id`, held ones included. A synced input row can name any message, so the
/// thread is checked on the message itself.
pub(super) async fn pending_thread_inputs(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    reply_thread_id: &ChatReplyThreadId,
) -> ChatResult<Vec<PendingInput>> {
    let rows = sqlx::query(
        "SELECT input.id, input.message_item_id, input.held
         FROM chat_work_assignment_inputs input
         JOIN chat_reply_threads thread ON thread.id = ?2
         JOIN chat_conversation_items item
           ON item.id = input.message_item_id
          AND item.conversation_id = thread.conversation_id
         WHERE input.assignment_id = ?1
           AND (item.reply_thread_id = thread.id OR thread.root_item_id = item.id)
           AND input.routing_kind IN ('steer', 'queued_continuation')
           AND input.delivery_state = 'pending'
         ORDER BY input.ordinal, input.id",
    )
    .bind(assignment_id.as_str())
    .bind(reply_thread_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(PendingInput {
                id: row.try_get("id").map_err(persistence_error)?,
                message_item_id: row.try_get("message_item_id").map_err(persistence_error)?,
                #[cfg(desktop)]
                held: row.try_get::<i64, _>("held").map_err(persistence_error)? != 0,
            })
        })
        .collect()
}

/// Moves the pending inputs `input_ids`, in order, behind the inputs `assignment_id` already
/// holds, routed as `routing_kind`, or keeping their routing when it is `None`. A held input
/// keeps its routing either way, so resuming it later queues it as it was.
pub(super) async fn move_pending_inputs(
    transaction: &mut Transaction<'_, Sqlite>,
    input_ids: &[String],
    assignment_id: &ChatWorkAssignmentId,
    routing_kind: Option<&str>,
) -> ChatResult<()> {
    if input_ids.is_empty() {
        return Ok(());
    }
    let last_ordinal: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(ordinal), 0) FROM chat_work_assignment_inputs
         WHERE assignment_id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for (index, input_id) in input_ids.iter().enumerate() {
        let ordinal = i64::try_from(index)
            .ok()
            .and_then(|offset| last_ordinal.checked_add(offset)?.checked_add(1))
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Validation,
                    "Too many pending follow-up messages",
                    true,
                )
            })?;
        sqlx::query(
            "UPDATE chat_work_assignment_inputs
             SET assignment_id = ?, ordinal = ?,
                 routing_kind = CASE WHEN held = 1 THEN routing_kind
                                     ELSE coalesce(?, routing_kind) END
             WHERE id = ? AND delivery_state = 'pending'",
        )
        .bind(assignment_id.as_str())
        .bind(ordinal)
        .bind(routing_kind)
        .bind(input_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

/// Keeps on this device the execution target `target` that the post creating `assignment_id`
/// chose, so the dispatch that a later acceptance queues runs where the request asked.
#[cfg(desktop)]
async fn record_requested_target(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    target: &ChatExecutionTarget,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let target_data = serde_json::to_string(target).map_err(serialization_error)?;
    sqlx::query(
        "INSERT INTO chat_assignment_requested_targets (assignment_id, target_data, created_at)
         VALUES (?, ?, ?)
         ON CONFLICT(assignment_id) DO UPDATE SET
            target_data = excluded.target_data, created_at = excluded.created_at",
    )
    .bind(assignment_id.as_str())
    .bind(target_data)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Removes and returns the execution target the post creating `assignment_id` chose on this
/// device, if it kept one.
#[cfg(desktop)]
pub(super) async fn take_requested_target(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Option<ChatExecutionTarget>> {
    let target_data: Option<String> = sqlx::query_scalar(
        "DELETE FROM chat_assignment_requested_targets WHERE assignment_id = ?
         RETURNING target_data",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    target_data
        .map(|data| serde_json::from_str(&data).map_err(serialization_error))
        .transpose()
}

/// Keeps on this device the execution target `target` that the post `message_item_id` chose
/// when it became a pending reply to work awaiting review, so the follow-up that a later
/// acceptance creates from it runs where the post asked.
#[cfg(desktop)]
async fn record_input_requested_target(
    transaction: &mut Transaction<'_, Sqlite>,
    message_item_id: &ChatConversationItemId,
    target: &ChatExecutionTarget,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let target_data = serde_json::to_string(target).map_err(serialization_error)?;
    sqlx::query(
        "INSERT INTO chat_assignment_input_requested_targets (input_id, target_data, created_at)
         VALUES (?, ?, ?)
         ON CONFLICT(input_id) DO UPDATE SET
            target_data = excluded.target_data, created_at = excluded.created_at",
    )
    .bind(assignment_input_id(message_item_id.as_str())?)
    .bind(target_data)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// The execution target this device keeps for the pending reply `input_id`, if its post chose
/// one.
#[cfg(desktop)]
pub(super) async fn read_input_requested_target(
    transaction: &mut Transaction<'_, Sqlite>,
    input_id: &str,
) -> ChatResult<Option<ChatExecutionTarget>> {
    let target_data: Option<String> = sqlx::query_scalar(
        "SELECT target_data FROM chat_assignment_input_requested_targets WHERE input_id = ?",
    )
    .bind(input_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    target_data
        .map(|data| serde_json::from_str(&data).map_err(serialization_error))
        .transpose()
}

/// Drops the execution targets this device keeps for the inputs of `assignment_id`, which only
/// serve replies still waiting on work awaiting review.
pub(super) async fn drop_input_requested_targets(
    connection: &mut sqlx::SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    sqlx::query(
        "DELETE FROM chat_assignment_input_requested_targets
         WHERE input_id IN (
           SELECT id FROM chat_work_assignment_inputs WHERE assignment_id = ?
         )",
    )
    .bind(assignment_id.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Freezes the context of assignment `assignment_id` and queues its dispatch on this device,
/// with the job consenting to the subject the post or Start agreed to run. Delegated work whose
/// parent lost its authority is cancelled by the freeze instead, and queues nothing.
#[cfg(desktop)]
pub(super) async fn queue_assignment_dispatch(
    transaction: &mut Transaction<'_, Sqlite>,
    work: WorkToFreeze<'_>,
    execution_target: Option<&ChatExecutionTarget>,
    providers: &super::provider_agents::DeviceProviders,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let assignment_id = work.assignment_id;
    let outcome =
        freeze_context_package(transaction, work, execution_target, providers, now).await?;
    if outcome == FreezeOutcome::ParentAuthorityRevoked {
        return Ok(());
    }
    let consented_digest = read_assignment_subject(transaction, assignment_id, providers)
        .await?
        .digest();
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, consented_digest, available_at, created_at, updated_at)
         VALUES (?, ?, 'queued', ?, ?, ?, ?)",
    )
    .bind(new_id("dispatch")?)
    .bind(assignment_id.as_str())
    .bind(&consented_digest)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Who writes a communication message.
#[derive(Clone, Copy, Debug)]
pub(super) enum MessageAuthor<'a> {
    /// The person using this device.
    LocalPerson,
    /// An agent, as when it delegates work in its channel.
    #[cfg_attr(mobile, allow(dead_code))]
    Agent(&'a ChatParticipantId),
}

impl MessageAuthor<'_> {
    fn participant_id(&self) -> &str {
        match self {
            Self::LocalPerson => LOCAL_PARTICIPANT_ID,
            Self::Agent(agent_id) => agent_id.as_str(),
        }
    }
}

pub(super) struct CommunicationMessageWrite<'a> {
    pub(super) author: MessageAuthor<'a>,
    pub(super) item_id: &'a ChatConversationItemId,
    pub(super) revision_id: &'a ChatMessageRevisionId,
    pub(super) conversation_id: &'a ChatConversationId,
    pub(super) reply_thread_id: Option<&'a ChatReplyThreadId>,
    pub(super) order_key: &'a str,
    pub(super) request: &'a PostChatMessageCommand,
    pub(super) attachments: AttachmentSource<'a>,
    pub(super) invoked_agent_id: Option<&'a ChatParticipantId>,
    pub(super) now: &'a UtcTimestamp,
}

pub(super) async fn insert_communication_message(
    transaction: &mut Transaction<'_, Sqlite>,
    write: CommunicationMessageWrite<'_>,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_conversation_items
            (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES (?, ?, ?, 'message', ?, ?)",
    )
    .bind(write.item_id.as_str())
    .bind(write.conversation_id.as_str())
    .bind(write.reply_thread_id.map(ChatReplyThreadId::as_str))
    .bind(write.order_key)
    .bind(write.now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_messages
            (item_id, author_participant_id, author_label_snapshot, created_at)
         SELECT ?, participant.id, participant.display_name, ?
         FROM chat_participants participant WHERE participant.id = ?",
    )
    .bind(write.item_id.as_str())
    .bind(write.now.as_str())
    .bind(write.author.participant_id())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_communication_message_revisions
            (id, message_item_id, normalized_markdown,
             rich_content_schema_version, rich_content_data, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(write.revision_id.as_str())
    .bind(write.item_id.as_str())
    .bind(write.request.normalized_markdown.trim())
    .bind(i64::from(write.request.rich_content.schema_version))
    .bind(json_object(&write.request.rich_content, "richContent")?)
    .bind(write.now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for (index, reference) in write.request.references.iter().enumerate() {
        insert_message_reference(transaction, &write, reference, index).await?;
    }
    insert_revision_attachments(transaction, write.revision_id.as_str(), write.attachments).await?;
    sqlx::query("UPDATE chat_communication_messages SET current_revision_id = ? WHERE item_id = ?")
        .bind(write.revision_id.as_str())
        .bind(write.item_id.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    Ok(())
}

/// Writes `reference` of the message `write` posts at `ordinal`.
async fn insert_message_reference(
    transaction: &mut Transaction<'_, Sqlite>,
    write: &CommunicationMessageWrite<'_>,
    reference: &ChatMessageReference,
    ordinal: usize,
) -> ChatResult<()> {
    let revision_id = write.revision_id;
    let destination_conversation_id = write.conversation_id;
    let invoked_agent_id = write.invoked_agent_id;
    let now = write.now;
    let metadata = reference.metadata();
    let reference_kind = match reference {
        ChatMessageReference::Participant { .. } => "participant",
        ChatMessageReference::Channel { .. } => "channel",
        ChatMessageReference::WorkingFolder { .. } => "working_folder",
        ChatMessageReference::WorkspacePath { .. } => "workspace_path",
        ChatMessageReference::ExecutionEnvironment { .. } => "execution_environment",
    };
    sqlx::query(
        "INSERT INTO chat_message_references
            (id, message_revision_id, reference_kind, label_snapshot,
             plain_text_projection, start_offset, end_offset, ordinal, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(metadata.reference_id.as_str())
    .bind(revision_id.as_str())
    .bind(reference_kind)
    .bind(&metadata.label_snapshot)
    .bind(&metadata.plain_text_projection)
    .bind(i64_value(metadata.start_offset)?)
    .bind(i64_value(metadata.end_offset)?)
    .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;

    match reference {
        ChatMessageReference::Participant {
            participant_id,
            participant_kind,
            ..
        } => {
            let stored_kind = sqlx::query_scalar::<_, String>(
                "SELECT participant_kind FROM chat_participants WHERE id = ?",
            )
            .bind(participant_id.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::validation("references", "Referenced participant was not found")
            })?;
            if stored_kind != wire_participant_kind(*participant_kind) {
                return Err(ChatError::validation(
                    "references",
                    "Referenced participant identity is stale",
                ));
            }
            sqlx::query(
                "INSERT INTO chat_participant_reference_targets (reference_id, participant_id)
                 VALUES (?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(participant_id.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::Channel { channel_id, .. } => {
            let source = sqlx::query(
                "SELECT channel.conversation_id, channel.archived_at
                 FROM chat_channels channel WHERE channel.id = ?",
            )
            .bind(channel_id.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::validation("references", "Referenced channel was not found")
            })?;
            if source
                .try_get::<Option<String>, _>("archived_at")
                .map_err(persistence_error)?
                .is_some()
            {
                return Err(ChatError::validation(
                    "references",
                    "Archived channels cannot be referenced",
                ));
            }
            let source_conversation_id: String = source
                .try_get("conversation_id")
                .map_err(persistence_error)?;
            let (source_high_order_key, source_high_item_id) =
                sqlx::query_as::<_, (String, String)>(
                    "SELECT item.order_key, item.id
                     FROM chat_conversation_items item
                     JOIN chat_communication_messages message ON message.item_id = item.id
                     WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
                       AND message.deleted_at IS NULL
                     ORDER BY item.order_key DESC, item.id DESC LIMIT 1",
                )
                .bind(&source_conversation_id)
                .fetch_optional(&mut **transaction)
                .await
                .map_err(persistence_error)?
                .ok_or_else(|| {
                    ChatError::validation(
                        "references",
                        "An empty channel cannot be used as an assignment context source",
                    )
                })?;
            let audience_gap: i64 = sqlx::query_scalar(
                "SELECT count(*)
                 FROM chat_conversation_memberships destination
                 JOIN chat_participants participant ON participant.id = destination.participant_id
                 LEFT JOIN chat_agent_channel_memberships destination_ai
                   ON destination_ai.conversation_id = destination.conversation_id
                  AND destination_ai.agent_id = destination.participant_id
                  AND destination_ai.removed_at IS NULL
                 WHERE destination.conversation_id = ?
                   AND destination.removed_at IS NULL
                   AND (
                     participant.participant_kind != 'ai_agent'
                     OR (
                       destination_ai.agent_id IS NOT NULL
                     )
                   )
                   AND NOT EXISTS (
                     SELECT 1
                     FROM chat_conversation_memberships source_membership
                     LEFT JOIN chat_agent_channel_memberships source_ai
                       ON source_ai.conversation_id = source_membership.conversation_id
                      AND source_ai.agent_id = source_membership.participant_id
                      AND source_ai.removed_at IS NULL
                     WHERE source_membership.conversation_id = ?
                       AND source_membership.participant_id = destination.participant_id
                       AND source_membership.removed_at IS NULL
                       AND (
                         participant.participant_kind != 'ai_agent'
                         OR (
                           source_ai.agent_id IS NOT NULL
                         )
                       )
                   )",
            )
            .bind(destination_conversation_id.as_str())
            .bind(&source_conversation_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if audience_gap > 0 {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "The destination audience cannot read the referenced channel",
                    true,
                ));
            }
            // A channel member reads the entire history, which starts at the first root item.
            let source_lower_order_key: String = sqlx::query_scalar(
                "SELECT min(order_key) FROM chat_conversation_items
                 WHERE conversation_id = ? AND reply_thread_id IS NULL",
            )
            .bind(&source_conversation_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if let Some(agent_id) = invoked_agent_id {
                let agent_in_source: bool = sqlx::query_scalar(
                    "SELECT EXISTS (
                       SELECT 1
                       FROM chat_conversation_memberships membership
                       JOIN chat_agent_channel_memberships ai_membership
                         ON ai_membership.conversation_id = membership.conversation_id
                        AND ai_membership.agent_id = membership.participant_id
                        AND ai_membership.removed_at IS NULL
                       WHERE membership.conversation_id = ?
                         AND membership.participant_id = ?
                         AND membership.removed_at IS NULL
                     )",
                )
                .bind(&source_conversation_id)
                .bind(agent_id.as_str())
                .fetch_one(&mut **transaction)
                .await
                .map_err(persistence_error)?;
                if !agent_in_source {
                    return Err(ChatError::new(
                        ChatErrorCode::Conflict,
                        "The assigned agent cannot read the referenced channel history",
                        true,
                    ));
                }
            }
            if source_lower_order_key > source_high_order_key {
                return Err(ChatError::new(
                    ChatErrorCode::Conflict,
                    "No referenced channel history is readable by the complete destination audience",
                    true,
                ));
            }
            let source_revision_cutoff_id = sqlx::query_scalar::<_, String>(
                "SELECT revision.id
                 FROM chat_conversation_items item
                 JOIN chat_communication_messages message ON message.item_id = item.id
                 JOIN chat_communication_message_revisions revision
                   ON revision.id = message.current_revision_id
                 WHERE item.conversation_id = ? AND message.deleted_at IS NULL
                 ORDER BY revision.created_at DESC, revision.id DESC
                 LIMIT 1",
            )
            .bind(&source_conversation_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .unwrap_or_else(|| revision_id.as_str().to_string());
            sqlx::query(
                "INSERT INTO chat_channel_reference_targets
                    (reference_id, channel_id, source_conversation_id,
                     destination_conversation_id, source_lower_order_key,
                     source_lower_item_id, source_high_order_key, source_high_item_id,
                     source_revision_cutoff_id)
                 VALUES (?, ?, ?, ?, ?, '', ?, ?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(channel_id.as_str())
            .bind(&source_conversation_id)
            .bind(destination_conversation_id.as_str())
            .bind(&source_lower_order_key)
            .bind(&source_high_order_key)
            .bind(&source_high_item_id)
            .bind(&source_revision_cutoff_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::WorkingFolder {
            working_folder_id, ..
        } => {
            require_destination_project_folder(
                transaction,
                destination_conversation_id,
                working_folder_id,
            )
            .await?;
            sqlx::query(
                "INSERT INTO chat_working_folder_reference_targets
                    (reference_id, working_folder_id) VALUES (?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(working_folder_id.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::WorkspacePath {
            working_folder_id,
            path_kind,
            relative_path,
            ..
        } => {
            require_destination_project_folder(
                transaction,
                destination_conversation_id,
                working_folder_id,
            )
            .await?;
            let path_kind = match path_kind {
                ChatWorkspacePathKind::File => "file",
                ChatWorkspacePathKind::Folder => "folder",
            };
            sqlx::query(
                "INSERT INTO chat_workspace_path_reference_targets
                    (reference_id, working_folder_id, path_kind, relative_path)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(working_folder_id.as_str())
            .bind(path_kind)
            .bind(relative_path)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
        ChatMessageReference::ExecutionEnvironment {
            execution_environment_id,
            ..
        } => {
            let valid: i64 = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1
                    FROM chat_execution_environments environment
                    JOIN project_working_folders folder
                      ON folder.id = environment.working_folder_id
                    JOIN chat_conversations conversation
                      ON conversation.project_id = folder.project_id
                    WHERE environment.id = ? AND conversation.id = ?
                      AND environment.kind IN ('current_folder', 'worktree')
                      AND environment.archived_at IS NULL
                      AND environment.lifecycle_state = 'available'
                 )",
            )
            .bind(execution_environment_id.as_str())
            .bind(destination_conversation_id.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if valid == 0 {
                return Err(ChatError::validation(
                    "references",
                    "Execution environment does not belong to the channel project",
                ));
            }
            sqlx::query(
                "INSERT INTO chat_execution_environment_reference_targets
                    (reference_id, execution_environment_id) VALUES (?, ?)",
            )
            .bind(metadata.reference_id.as_str())
            .bind(execution_environment_id.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
        }
    }
    Ok(())
}

async fn require_destination_project_folder(
    transaction: &mut Transaction<'_, Sqlite>,
    destination_conversation_id: &ChatConversationId,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    let valid: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM project_working_folders folder
            JOIN chat_conversations conversation ON conversation.project_id = folder.project_id
            WHERE folder.id = ? AND conversation.id = ? AND folder.archived_at IS NULL
         )",
    )
    .bind(working_folder_id.as_str())
    .bind(destination_conversation_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if valid == 0 {
        return Err(ChatError::validation(
            "references",
            "Working folder does not belong to the channel project",
        ));
    }
    Ok(())
}

/// Identity and attachment source of the channel copy of a reply.
pub(super) struct ChannelCopy<'a> {
    /// Derived ids for a scheduled send, or `None` for random ids.
    pub(super) ids: Option<ScheduledPostIds>,
    /// Where the copy's attachment references come from.
    pub(super) attachments: AttachmentSource<'a>,
}

pub(super) async fn insert_channel_copy(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    request: &PostChatMessageCommand,
    copy: ChannelCopy<'_>,
    invoked_agent_id: Option<&ChatParticipantId>,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let (item_id, revision_id) = match copy.ids {
        Some(ids) => (ids.item_id, ids.revision_id),
        None => (conversation_item_id()?, message_revision_id()?),
    };
    let order_key = next_item_order_key(transaction, conversation_id, None).await?;
    let mut copied_request = request.clone();
    for reference in &mut copied_request.references {
        reference.metadata_mut().reference_id =
            ChatMessageReferenceId::new(new_id("message-reference")?).map_err(identifier_error)?;
    }
    insert_communication_message(
        transaction,
        CommunicationMessageWrite {
            author: MessageAuthor::LocalPerson,
            item_id: &item_id,
            revision_id: &revision_id,
            conversation_id,
            reply_thread_id: None,
            order_key: &order_key,
            request: &copied_request,
            attachments: copy.attachments,
            invoked_agent_id,
            now,
        },
    )
    .await?;
    if has_thread_eligible_mention(request) {
        let thread_id = reply_thread_id(&item_id)?;
        sqlx::query(
            "INSERT INTO chat_reply_threads
                (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(thread_id.as_str())
        .bind(conversation_id.as_str())
        .bind(item_id.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

pub(super) async fn next_item_order_key(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    reply_thread_id: Option<&ChatReplyThreadId>,
) -> ChatResult<String> {
    ganbaru_chat::timeline::next_order_key(
        transaction,
        conversation_id.as_str(),
        reply_thread_id.map(ChatReplyThreadId::as_str),
    )
    .await
}

pub(super) async fn require_reply_thread(
    pool: &SqlitePool,
    reply_thread_id: &ChatReplyThreadId,
    conversation_id: &ChatConversationId,
) -> ChatResult<ChatReplyThreadId> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chat_reply_threads WHERE id = ? AND conversation_id = ?
         )",
    )
    .bind(reply_thread_id.as_str())
    .bind(conversation_id.as_str())
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if exists == 0 {
        return Err(ChatError::validation(
            "replyThreadId",
            "Reply thread does not belong to the selected channel",
        ));
    }
    Ok(reply_thread_id.clone())
}

/// Requires `agent_id` to be a present, unarchived AI agent member of the conversation, which
/// is all an agent needs to take work there: without a default model it starts on the
/// provider's default. An agent that left the channel is a conflict, and one that never
/// joined it is a validation error.
pub(super) async fn require_participating_agent(
    connection: &mut SqliteConnection,
    conversation_id: &ChatConversationId,
    agent_id: &ChatParticipantId,
) -> ChatResult<()> {
    if ganbaru_chat::coordination::agent_settings::agent_is_present(
        connection,
        conversation_id.as_str(),
        agent_id.as_str(),
    )
    .await?
    {
        return Ok(());
    }
    let was_member: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM chat_agent_channel_memberships
             WHERE conversation_id = ? AND agent_id = ?
         )",
    )
    .bind(conversation_id.as_str())
    .bind(agent_id.as_str())
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if was_member {
        Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The AI agent no longer participates in this channel",
            true,
        ))
    } else {
        Err(ChatError::validation(
            "references",
            "The AI agent is not a member of this channel",
        ))
    }
}

/// Appends an agent policy revision and points the agent's current policy at it.
#[cfg(desktop)]
pub(super) async fn insert_policy_revision(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &ChatParticipantId,
    policy: &ChatAgentPolicyInput,
    now: &UtcTimestamp,
) -> ChatResult<ChatAgentPolicyRevisionId> {
    let selection =
        serde_json::to_string(&policy.model_selection()).map_err(serialization_error)?;
    let policy_revision_id = new_id("agent-policy")?;
    sqlx::query(
        "INSERT INTO chat_agent_policy_revisions
            (id, agent_id, model_selection_schema_version, model_selection_data,
             provider_options_schema_version, provider_options_data,
             interaction_mode, created_at)
         VALUES (?, ?, 1, ?, ?, ?, 'build', ?)",
    )
    .bind(policy_revision_id.as_str())
    .bind(agent_id.as_str())
    .bind(selection)
    .bind(i64::from(policy.provider_options.schema_version))
    .bind(json_object(&policy.provider_options, "providerOptions")?)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_agents SET current_policy_revision_id = ?, updated_at = ?
         WHERE participant_id = ?",
    )
    .bind(policy_revision_id.as_str())
    .bind(now.as_str())
    .bind(agent_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    ChatAgentPolicyRevisionId::new(policy_revision_id).map_err(identifier_error)
}

pub(super) async fn read_message_references(
    pool: &SqlitePool,
    revision_id: &ChatMessageRevisionId,
) -> ChatResult<Vec<ChatMessageReference>> {
    let rows = sqlx::query(
        "SELECT id, reference_kind, label_snapshot, plain_text_projection,
                start_offset, end_offset
         FROM chat_message_references WHERE message_revision_id = ?
         ORDER BY ordinal",
    )
    .bind(revision_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut references = Vec::with_capacity(rows.len());
    for row in rows {
        let reference_id =
            ChatMessageReferenceId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
                .map_err(identifier_error)?;
        let metadata = ChatReferenceMetadata {
            reference_id: reference_id.clone(),
            label_snapshot: row.try_get("label_snapshot").map_err(persistence_error)?,
            start_offset: u64_value(row.try_get("start_offset").map_err(persistence_error)?)?,
            end_offset: u64_value(row.try_get("end_offset").map_err(persistence_error)?)?,
            plain_text_projection: row
                .try_get("plain_text_projection")
                .map_err(persistence_error)?,
        };
        let reference_kind: String = row.try_get("reference_kind").map_err(persistence_error)?;
        let reference = match reference_kind.as_str() {
            "participant" => {
                let target = sqlx::query(
                    "SELECT target.participant_id, participant.participant_kind
                     FROM chat_participant_reference_targets target
                     JOIN chat_participants participant ON participant.id = target.participant_id
                     WHERE target.reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::Participant {
                    metadata,
                    participant_id: ChatParticipantId::new(
                        target
                            .try_get::<String, _>("participant_id")
                            .map_err(persistence_error)?,
                    )
                    .map_err(identifier_error)?,
                    participant_kind: parse_participant_kind(
                        &target
                            .try_get::<String, _>("participant_kind")
                            .map_err(persistence_error)?,
                    )?,
                }
            }
            "channel" => {
                let channel_id: String = sqlx::query_scalar(
                    "SELECT channel_id FROM chat_channel_reference_targets
                     WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::Channel {
                    metadata,
                    channel_id: ChatChannelId::new(channel_id).map_err(identifier_error)?,
                }
            }
            "working_folder" => {
                let folder_id: String = sqlx::query_scalar(
                    "SELECT working_folder_id FROM chat_working_folder_reference_targets
                     WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::WorkingFolder {
                    metadata,
                    working_folder_id: ProjectWorkingFolderId::new(folder_id)
                        .map_err(identifier_error)?,
                }
            }
            "workspace_path" => {
                let target = sqlx::query(
                    "SELECT working_folder_id, path_kind, relative_path
                     FROM chat_workspace_path_reference_targets WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                let path_kind = match target
                    .try_get::<String, _>("path_kind")
                    .map_err(persistence_error)?
                    .as_str()
                {
                    "file" => ChatWorkspacePathKind::File,
                    "folder" => ChatWorkspacePathKind::Folder,
                    _ => {
                        return Err(ChatError::new(
                            ChatErrorCode::Persistence,
                            "Stored workspace path kind is invalid",
                            false,
                        ));
                    }
                };
                ChatMessageReference::WorkspacePath {
                    metadata,
                    working_folder_id: ProjectWorkingFolderId::new(
                        target
                            .try_get::<String, _>("working_folder_id")
                            .map_err(persistence_error)?,
                    )
                    .map_err(identifier_error)?,
                    path_kind,
                    relative_path: target.try_get("relative_path").map_err(persistence_error)?,
                }
            }
            "execution_environment" => {
                let environment_id: String = sqlx::query_scalar(
                    "SELECT execution_environment_id
                     FROM chat_execution_environment_reference_targets
                     WHERE reference_id = ?",
                )
                .bind(reference_id.as_str())
                .fetch_one(pool)
                .await
                .map_err(persistence_error)?;
                ChatMessageReference::ExecutionEnvironment {
                    metadata,
                    execution_environment_id: ChatExecutionEnvironmentId::new(environment_id)
                        .map_err(identifier_error)?,
                }
            }
            _ => {
                return Err(ChatError::new(
                    ChatErrorCode::Persistence,
                    "Stored message reference kind is invalid",
                    false,
                ));
            }
        };
        references.push(reference);
    }
    Ok(references)
}

pub(super) async fn read_post_receipt(
    pool: &SqlitePool,
    client_command_id: &ChatCommandId,
) -> ChatResult<Option<PostChatMessageResult>> {
    let row = sqlx::query(
        "SELECT state, result_data FROM chat_organizational_command_receipts
         WHERE client_command_id = ?",
    )
    .bind(client_command_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let state: String = row.try_get("state").map_err(persistence_error)?;
    if state != "completed" {
        return Err(ChatError::new(
            ChatErrorCode::Busy,
            "This message command is still being processed",
            true,
        ));
    }
    let data: String = row
        .try_get::<Option<String>, _>("result_data")
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "Message receipt is incomplete",
                true,
            )
        })?;
    if let Ok(result) = serde_json::from_str::<PostChatMessageResult>(&data) {
        return Ok(Some(result));
    }
    let locator =
        serde_json::from_str::<StoredPostMessageReceipt>(&data).map_err(serialization_error)?;
    let StoredPostMessageReceipt::Locator {
        message_item_id,
        reply_thread_id,
        assignment_id,
        assignment_input_queued,
        routed_as,
    } = locator;
    let message = read_message(pool, &message_item_id).await?;
    let assignment = match assignment_id.as_ref() {
        Some(assignment_id) => Some(read_assignment(pool, assignment_id).await?),
        None => match reply_thread_id.as_ref() {
            Some(reply_thread_id) => {
                read_active_or_latest_assignment(pool, reply_thread_id).await?
            }
            None => None,
        },
    };
    Ok(Some(PostChatMessageResult {
        message,
        reply_thread_id,
        assignment,
        assignment_input_queued,
        routed_as,
    }))
}

/// Refuses a review note unless `invocation` routes it to the reviewed work: that work must be
/// the latest of the reply thread, with no active work in front of it.
pub(super) fn require_reviewed_work_is_latest(
    invocation: Option<&ResolvedInvocation>,
    review: ReviewedWork<'_>,
) -> ChatResult<()> {
    let routes_to_review = invocation.is_some_and(|invocation| {
        invocation.active_assignment.is_none()
            && invocation
                .latest_assignment
                .as_ref()
                .is_some_and(|latest| &latest.id == review.assignment_id)
    });
    if routes_to_review {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The reviewed work is no longer the latest work of its reply thread",
            true,
        ))
    }
}

/// Refuses a review of `assignment_id` unless it is ready for review at `expected_revision`.
pub(super) async fn require_reviewable_assignment(
    connection: &mut sqlx::SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
    expected_revision: u64,
) -> ChatResult<()> {
    let row = sqlx::query("SELECT state, revision FROM chat_work_assignments WHERE id = ?")
        .bind(assignment_id.as_str())
        .fetch_optional(connection)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::NotFound,
                "The assignment was not found",
                false,
            )
        })?;
    let revision = u64_value(row.try_get("revision").map_err(persistence_error)?)?;
    if revision != expected_revision {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The assignment changed before the review",
            true,
        ));
    }
    let state: String = row.try_get("state").map_err(persistence_error)?;
    if state != "ready_for_review" {
        return Err(ChatError::invalid_transition(
            "Only work that is ready for review can be reviewed",
        ));
    }
    Ok(())
}

/// Error detail reason of an Accept refused because replies still wait on the reviewed work.
#[cfg(desktop)]
pub(super) const REVIEW_REFUSED_PENDING_REPLIES: &str = "pending_replies";

/// Error detail reason of a Request changes refused because no device can run the agent.
pub(super) const REVIEW_REFUSED_NO_EXECUTING_DEVICE: &str = "no_executing_device";

/// A `Conflict` that refuses a review for `reason`, one of the `REVIEW_REFUSED_*` reasons, which
/// the client shows as a localized explanation.
pub(super) fn review_refusal(reason: &str, message: &str) -> ChatError {
    let mut error = ChatError::new(ChatErrorCode::Conflict, message, true);
    error.details = Some(Box::new(serde_json::json!({ "reason": reason })));
    error
}

/// Refuses to accept `assignment_id` while replies routed to it still wait for delivery: once
/// the work completes, no run or follow-up would ever deliver them.
#[cfg(desktop)]
pub(super) async fn require_no_pending_replies(
    connection: &mut sqlx::SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    let pending = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM chat_work_assignment_inputs
         WHERE assignment_id = ?
           AND routing_kind IN ('steer', 'queued_continuation')
           AND delivery_state = 'pending'",
    )
    .bind(assignment_id.as_str())
    .fetch_one(connection)
    .await
    .map_err(persistence_error)?;
    if pending == 0 {
        Ok(())
    } else {
        Err(review_refusal(
            REVIEW_REFUSED_PENDING_REPLIES,
            "Replies still wait on this work, so it cannot be accepted yet",
        ))
    }
}

/// Completes `assignment_id` with the review `outcome` by `reviewer_participant_id`, and
/// returns whether it was still ready for review, and at `expected_revision` when one is given.
pub(super) async fn complete_reviewed_assignment(
    connection: &mut sqlx::SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
    expected_revision: Option<u64>,
    outcome: ChatAssignmentReviewOutcome,
    reviewer_participant_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    let expected_revision = expected_revision.map(i64_value).transpose()?;
    let settled = sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'completed', settled_at = ?1, review_outcome = ?2, reviewed_at = ?1,
             reviewed_by_participant_id = ?3, revision = revision + 1, updated_at = ?1
         WHERE id = ?4 AND state = 'ready_for_review' AND (?5 IS NULL OR revision = ?5)",
    )
    .bind(now.as_str())
    .bind(wire_review_outcome(outcome))
    .bind(reviewer_participant_id)
    .bind(assignment_id.as_str())
    .bind(expected_revision)
    .execute(connection)
    .await
    .map_err(persistence_error)?;
    Ok(settled.rows_affected() == 1)
}

/// Moves `assignment_id` to `state` when it is still at `expected_revision`. The write clears
/// any review, since a review only judges completed work and this write replaces that state.
/// Delegated work that reaches an outcome state reports it in its parent's reply thread.
#[cfg(desktop)]
pub(super) async fn set_assignment_state(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    expected_revision: u64,
    state: ChatWorkAssignmentState,
    reason: Option<&str>,
) -> ChatResult<ChatWorkAssignmentRead> {
    let now = now_timestamp()?;
    let updated = sqlx::query(
        "UPDATE chat_work_assignments
         SET state = ?, state_reason = ?, settled_at = ?, review_outcome = NULL,
             reviewed_at = NULL, reviewed_by_participant_id = NULL,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(wire_work_state(state))
    .bind(reason)
    .bind((!state.is_active()).then(|| now.as_str()))
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The assignment changed before the update",
            true,
        ));
    }
    if !state.is_active() {
        let mut connection = pool.acquire().await.map_err(persistence_error)?;
        end_assignment_dispatch_jobs(&mut connection, assignment_id, state, &now).await?;
    }
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    ganbaru_chat::coordination::delegation::post_delegation_outcome(
        &mut transaction,
        assignment_id.as_str(),
        now.as_str(),
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    read_assignment(pool, assignment_id).await
}

/// Ends the local dispatch jobs still queued or claimed for `assignment_id`, which settled as
/// `state`: they become cancelled when it was cancelled and failed otherwise, so no worker runs
/// settled work. The execution targets kept for its start and for its pending replies go with
/// them, since they only served work that was still waiting to run.
#[cfg(desktop)]
pub(super) async fn end_assignment_dispatch_jobs(
    connection: &mut sqlx::SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
    state: ChatWorkAssignmentState,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs SET state = ?, updated_at = ?
         WHERE assignment_id = ? AND state IN ('queued', 'claimed')",
    )
    .bind(if state == ChatWorkAssignmentState::Cancelled {
        "cancelled"
    } else {
        "failed"
    })
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query("DELETE FROM chat_assignment_requested_targets WHERE assignment_id = ?")
        .bind(assignment_id.as_str())
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    drop_input_requested_targets(connection, assignment_id).await
}

pub(super) fn parse_model_selection(data: String) -> ChatResult<ChatModelSelection> {
    serde_json::from_str(&data).map_err(serialization_error)
}
