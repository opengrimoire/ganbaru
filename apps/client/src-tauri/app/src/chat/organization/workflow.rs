//! Assignment routing, frozen context, and durable organizational messages.

mod messages;

use super::common::{
    assignment_input_id, serialization_error, wire_review_outcome, wire_work_state,
    work_assignment_id,
};
#[cfg(desktop)]
use super::common::{json_object, new_id};
#[cfg(desktop)]
use super::context::{FreezeOutcome, WorkToFreeze, freeze_context_package};
use super::inputs::place_delivered_reply;
use super::now_timestamp;
use super::reads::{read_active_or_latest_assignment, read_assignment, read_participant};
use super::{
    LOCAL_PARTICIPANT_ID, ReviewedWork, i64_value, identifier_error, persistence_error, u64_value,
};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
pub(super) use messages::{
    ChannelCopy, CommunicationMessageWrite, MessageAuthor, insert_channel_copy,
    insert_communication_message, next_item_order_key, read_message_references, read_post_receipt,
};
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
}

impl AssignmentWrite {
    /// A post that routed to no assignment.
    pub(super) const NONE: Self = Self {
        assignment_id: None,
        input_queued: false,
        routed_as: None,
        executes_here: false,
    };
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
}

impl<'a> PostingDevice<'a> {
    /// This device under its stable id `id`, with the Chat writers `chat_writers`.
    pub(super) fn this_device(id: &'a str, chat_writers: &'a ChatWriters) -> Self {
        Self {
            id,
            can_execute: cfg!(desktop),
            chat_writers,
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
}

/// The device an assignment of `agent_id` runs on when `device` creates it: the posting device
/// when it can run new work and the agent's provider is ready there, else none.
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

/// The device a post's new assignment of `agent_id` in `reply_thread_id` runs on: the posting
/// device when it can run the agent, else another linked desktop, so a mention from a phone or
/// from a desktop without the provider starts work by itself. Linked devices belong to one
/// person, so their posts start work without a confirmation on the executing desktop.
pub(super) async fn post_executing_device(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &ChatParticipantId,
    reply_thread_id: &ChatReplyThreadId,
    device: PostingDevice<'_>,
) -> ChatResult<Option<String>> {
    if let Some(here) = new_assignment_executing_device(transaction, agent_id, device).await? {
        return Ok(Some(here));
    }
    if let Some(last) = last_linked_executor(transaction, agent_id, reply_thread_id, device).await?
    {
        return Ok(Some(last));
    }
    sole_linked_desktop(transaction, device).await
}

/// The linked Chat writer other than `device` that most recently ran work of `agent_id`,
/// preferring work in the conversation of `reply_thread_id`. Only desktops run work, so a past
/// executor is a desktop that had the agent's provider.
async fn last_linked_executor(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &ChatParticipantId,
    reply_thread_id: &ChatReplyThreadId,
    device: PostingDevice<'_>,
) -> ChatResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT assignment.executing_device_id
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         WHERE assignment.agent_id = ?1
           AND assignment.state IN (
             'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review',
             'completed'
           )
           AND assignment.executing_device_id IN (SELECT value FROM json_each(?2))
           AND assignment.executing_device_id <> ?3
         ORDER BY thread.conversation_id = (
                    SELECT conversation_id FROM chat_reply_threads WHERE id = ?4
                  ) DESC,
                  assignment.created_at DESC, assignment.id DESC
         LIMIT 1",
    )
    .bind(agent_id.as_str())
    .bind(device.chat_writers.json()?)
    .bind(device.id)
    .bind(reply_thread_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)
}

/// The only linked desktop that writes Chat besides `device`, when exactly one does. With
/// several and no history of the agent, none is chosen, so the post records the mention only.
async fn sole_linked_desktop(
    transaction: &mut Transaction<'_, Sqlite>,
    device: PostingDevice<'_>,
) -> ChatResult<Option<String>> {
    let desktops: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT linked.device_id FROM sync_devices linked
         WHERE linked.kind = 'desktop' AND linked.removed_revision IS NULL
           AND linked.device_id IN (SELECT value FROM json_each(?1))
           AND linked.device_id <> ?2
         LIMIT 2",
    )
    .bind(device.chat_writers.json()?)
    .bind(device.id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(match desktops.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    })
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
        });
    }
    let latest_assignment = invocation.latest_assignment.as_ref();
    let Some(executing_device_id) =
        post_executing_device(transaction, &invocation.agent_id, reply_thread_id, device).await?
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
    Ok(AssignmentWrite {
        assignment_id: Some(assignment_id),
        input_queued: false,
        routed_as: None,
        executes_here: device.executes(Some(&executing_device_id)),
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
/// first turn so they reach it together. When this device executes it, the same transaction
/// freezes its context and queues its dispatch; otherwise the executing device does both when
/// the assignment syncs there.
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
    }
    #[cfg(desktop)]
    if device.executes(Some(assignment.executing_device_id)) {
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

/// Freezes the context of assignment `assignment_id` and queues its dispatch on this device.
/// Delegated work whose parent lost its authority is cancelled by the freeze instead, and
/// queues nothing.
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
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, available_at, created_at, updated_at)
         VALUES (?, ?, 'queued', ?, ?, ?)",
    )
    .bind(new_id("dispatch")?)
    .bind(assignment_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
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

/// Work a person cancelled: the cancelled assignment and the ids of the open work delegated
/// from it, which the cancel also ended.
#[derive(Debug)]
pub(super) struct CancelledWork {
    pub(super) assignment: ChatWorkAssignmentRead,
    #[cfg_attr(mobile, allow(dead_code))]
    pub(super) descendants: std::collections::BTreeSet<String>,
}

/// Cancels `assignment_id`, still at `expected_revision`, with the open work delegated from it.
/// Any linked device may cancel: the executing device stops the run when the settled state
/// reaches it, and this device stops runs it holds itself.
pub(super) async fn cancel_assignment_work(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    expected_revision: u64,
) -> ChatResult<CancelledWork> {
    let assignment = set_assignment_state(
        pool,
        assignment_id,
        expected_revision,
        ChatWorkAssignmentState::Cancelled,
        Some("Cancelled by the user"),
    )
    .await?;
    let descendants = cancel_delegated_work(pool, assignment_id).await?;
    Ok(CancelledWork {
        assignment,
        descendants,
    })
}

/// Cancels the open work delegated from `assignment_id`, which a person just cancelled, and
/// ends the dispatch it still holds here. Returns the cancelled ids. Their revoked
/// authorizations queue revocation jobs, which stop the runs they hold on this device.
pub(super) async fn cancel_delegated_work(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<std::collections::BTreeSet<String>> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let cancelled = ganbaru_chat::coordination::delegation::cancel_delegated_descendants(
        &mut transaction,
        assignment_id.as_str(),
        now.as_str(),
    )
    .await?;
    for descendant_id in &cancelled {
        let descendant_id =
            ChatWorkAssignmentId::new(descendant_id.clone()).map_err(identifier_error)?;
        end_assignment_dispatch_jobs(
            &mut transaction,
            &descendant_id,
            ChatWorkAssignmentState::Cancelled,
            &now,
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(cancelled.into_iter().collect())
}

/// Moves `assignment_id` to `state` when it is still at `expected_revision`. The write clears
/// any review, since a review only judges completed work and this write replaces that state.
/// Delegated work that reaches an outcome state reports it in its parent's reply thread.
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
/// settled work.
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
    Ok(())
}

pub(super) fn parse_model_selection(data: String) -> ChatResult<ChatModelSelection> {
    serde_json::from_str(&data).map_err(serialization_error)
}
