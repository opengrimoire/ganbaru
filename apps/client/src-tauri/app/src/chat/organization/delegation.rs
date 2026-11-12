//! Work one teammate delegates to another, and the ceiling that keeps it within its ancestors.
//!
//! A delegated child is a separate assignment in its own reply thread, rooted at a message the
//! parent teammate posts in the channel. The child records the folder ceiling it may never
//! exceed. Every freeze and every claim of its dispatch reads the ceiling again against the
//! ancestors' current authorizations, so work whose parent lost its authority is cancelled
//! instead of starting.

use super::super::internal_mcp_tools::delegation::{
    DelegationRefusal, MAX_DELEGATION_DEPTH, MAX_OPEN_CHILDREN_PER_ASSIGNMENT,
};
use super::common::{
    chat_writers, reply_thread_id, serialization_error, stored_value_error, this_device_id,
    validate_message_request, wire_folder_capability,
};
use super::workflow::{
    ChatWriters, CommunicationMessageWrite, DelegationWrite, MessageAuthor, NewAssignment,
    PostingDevice, create_assignment, insert_communication_message,
    new_assignment_executing_device, next_item_order_key, require_participating_teammate,
};
use super::{identifier_error, now_timestamp, persistence_error};
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat::coordination::reductions::approval_rank;
use ganbaru_chat::coordination::revocation::PARENT_AUTHORITY_REVOKED_REASON;
use ganbaru_chat::ids::derived_id;
use ganbaru_chat_contracts::models::*;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::collections::BTreeMap;

/// One folder a delegated child may hold, at most at `capability`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DelegatedFolder {
    pub(crate) working_folder_id: ProjectWorkingFolderId,
    pub(crate) capability: ChatFolderCapability,
}

/// A teammate's request to hand bounded work to another teammate of its channel.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DelegationRequest<'a> {
    /// Working assignment the calling run belongs to.
    pub(crate) parent_assignment_id: &'a ChatWorkAssignmentId,
    /// Authorization the calling run holds, which must still be allowed.
    pub(crate) parent_authorization_revision_id: &'a ChatAuthorizationRevisionId,
    /// Conversation the parent works in, where the child's thread opens.
    pub(crate) conversation_id: &'a ChatConversationId,
    pub(crate) teammate_id: &'a ChatParticipantId,
    pub(crate) objective: &'a str,
    pub(crate) expected_result: &'a str,
    /// The child's folder ceiling, already within the calling run's folders.
    pub(crate) folders: &'a [DelegatedFolder],
    /// Caller-chosen id that makes a retried call return the child it created.
    pub(crate) client_request_id: &'a str,
}

/// The child a delegation created or found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DelegatedChild {
    pub(crate) assignment_id: ChatWorkAssignmentId,
    pub(crate) reply_thread_id: ChatReplyThreadId,
    /// Wire state of the child when the call returns.
    pub(crate) state: String,
}

/// Assignment states that count against the fan-out limit.
const OPEN_CHILD_STATES_SQL: &str =
    "state IN ('queued', 'working', 'waiting_for_answer', 'waiting_for_approval')";

/// Creates the child `request` describes in one transaction: the parent's root message that
/// mentions the target, its reply thread, and the queued child with its fixed delegation
/// fields. A retry with the same client request id and the same input returns the child the
/// first call created; different input is refused. The work starts like a post's: through its
/// dispatch when this device executes it, else on its executing device.
pub(crate) async fn delegate_assignment(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: DelegationRequest<'_>,
) -> ChatResult<Result<DelegatedChild, DelegationRefusal>> {
    let device_id = this_device_id(app)?;
    let writers = chat_writers(app, pool).await?;
    let created = match persist_delegation(pool, &device_id, &writers, request).await? {
        Ok(created) => created,
        Err(refusal) => return Ok(Err(refusal)),
    };
    if let Some(unattended) = created.starts_here {
        start_child(app, &created.child.assignment_id, unattended);
    }
    Ok(Ok(created.child))
}

/// A child [`persist_delegation`] wrote or found.
#[derive(Debug)]
pub(super) struct CreatedChild {
    pub(super) child: DelegatedChild,
    /// Whether this device starts the new child, and if so whether it starts as unattended
    /// work. A child an earlier call created was already started.
    pub(super) starts_here: Option<bool>,
}

/// Writes the child of `request` on the device `device_id`, as [`delegate_assignment`]
/// describes, without starting it.
pub(super) async fn persist_delegation(
    pool: &SqlitePool,
    device_id: &str,
    writers: &ChatWriters,
    request: DelegationRequest<'_>,
) -> ChatResult<Result<CreatedChild, DelegationRefusal>> {
    let ceiling = folder_ceiling_json(request.folders)?;
    let parts = [
        "delegation",
        request.parent_assignment_id.as_str(),
        request.client_request_id,
    ];
    let item_id = ChatConversationItemId::new(derived_id("conversation-item", &parts)?)
        .map_err(identifier_error)?;
    let thread_id = reply_thread_id(&item_id)?;
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(persistence_error)?;
    match read_existing_child(&mut transaction, &request, &item_id).await? {
        PriorRequest::None => {}
        PriorRequest::Child(existing) => {
            return Ok(existing
                .matching(&request, &ceiling, thread_id)
                .map(|child| CreatedChild {
                    child,
                    starts_here: None,
                }));
        }
        PriorRequest::OtherWork => return Ok(Err(DelegationRefusal::RequestConflict)),
    }
    let parent = match read_working_parent(&mut transaction, &request).await? {
        Ok(parent) => parent,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let depth = parent.depth + 1;
    if depth > MAX_DELEGATION_DEPTH {
        return Ok(Err(DelegationRefusal::DepthExceeded));
    }
    if request.teammate_id == &parent.teammate_id {
        return Ok(Err(DelegationRefusal::TargetUnavailable));
    }
    match require_participating_teammate(
        &mut *transaction,
        request.conversation_id,
        request.teammate_id,
    )
    .await
    {
        Ok(()) => {}
        Err(error) if error.code == ChatErrorCode::Persistence => return Err(error),
        Err(_) => return Ok(Err(DelegationRefusal::TargetUnavailable)),
    }
    let open_children: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM chat_work_assignments
         WHERE parent_assignment_id = ? AND {OPEN_CHILD_STATES_SQL}"
    )))
    .bind(request.parent_assignment_id.as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if open_children >= i64::from(MAX_OPEN_CHILDREN_PER_ASSIGNMENT) {
        return Ok(Err(DelegationRefusal::FanOutExceeded));
    }
    let Some(target) = read_target(&mut transaction, &request).await? else {
        return Ok(Err(DelegationRefusal::TargetUnavailable));
    };
    let message = delegation_message(&request, &target, &parts)?;
    validate_message_request(&message)?;
    let now = now_timestamp()?;
    let revision_id = ChatMessageRevisionId::new(derived_id("message-revision", &parts)?)
        .map_err(identifier_error)?;
    let order_key = next_item_order_key(&mut transaction, request.conversation_id, None).await?;
    insert_communication_message(
        &mut transaction,
        CommunicationMessageWrite {
            author: MessageAuthor::Teammate(&parent.teammate_id),
            item_id: &item_id,
            revision_id: &revision_id,
            conversation_id: request.conversation_id,
            reply_thread_id: None,
            order_key: &order_key,
            request: &message,
            attachments: super::common::AttachmentSource::Device(&[]),
            invoked_teammate_id: Some(request.teammate_id),
            now: &now,
        },
    )
    .await?;
    open_reply_thread(&mut transaction, &request, &item_id, &thread_id, &now).await?;
    let device = PostingDevice::this_device(device_id, writers);
    let device = if parent.unattended {
        device.unattended()
    } else {
        device
    };
    let Some(executing_device_id) =
        new_assignment_executing_device(&mut transaction, request.teammate_id, device).await?
    else {
        return Ok(Err(DelegationRefusal::TargetUnavailable));
    };
    let assignment_id = create_assignment(
        &mut transaction,
        NewAssignment {
            reply_thread_id: &thread_id,
            message_item_id: &item_id,
            teammate_id: request.teammate_id,
            latest_assignment: None,
            executing_device_id: &executing_device_id,
            execution_target: None,
            poster_participant_id: parent.teammate_id.as_str(),
            delegation: Some(DelegationWrite {
                parent_assignment_id: request.parent_assignment_id,
                depth,
                objective: request.objective,
                expected_result: request.expected_result,
                folder_ceiling: &ceiling,
            }),
            model_override: None,
        },
        device,
        &now,
    )
    .await?;
    let state = read_state(&mut transaction, &assignment_id).await?;
    transaction.commit().await.map_err(persistence_error)?;
    let starts_here = (device.executes(Some(&executing_device_id)) && state == "queued")
        .then_some(device.unattended);
    Ok(Ok(CreatedChild {
        child: DelegatedChild {
            assignment_id,
            reply_thread_id: thread_id,
            state,
        },
        starts_here,
    }))
}

/// Starts a child this device executes: through its queued dispatch, or, for the child of
/// unattended work, through the acceptance check of synced work.
fn start_child(app: &tauri::AppHandle, assignment_id: &ChatWorkAssignmentId, unattended: bool) {
    if unattended {
        super::assignments::start_unattended_assignment(app, assignment_id);
        return;
    }
    let app = app.clone();
    let assignment_id = assignment_id.clone();
    tauri::async_runtime::spawn(async move {
        let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
        if let Err(error) =
            super::dispatch::dispatch_assignment_job(app, db_url, assignment_id).await
        {
            eprintln!("Delegated Chat work did not start: {error}");
        }
    });
}

/// The ceiling as stored: a JSON array of `{workingFolderId, capability}` sorted by folder id.
fn folder_ceiling_json(folders: &[DelegatedFolder]) -> ChatResult<String> {
    let mut sorted = folders.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        left.working_folder_id
            .as_str()
            .cmp(right.working_folder_id.as_str())
    });
    let entries = sorted
        .into_iter()
        .map(|folder| {
            json!({
                "workingFolderId": folder.working_folder_id.as_str(),
                "capability": wire_folder_capability(folder.capability),
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&entries).map_err(serialization_error)
}

/// Parses a stored ceiling into folder ids and capability ranks. A malformed ceiling is an
/// error, since the schema admits only arrays and creation writes only valid entries.
fn parse_folder_ceiling(data: &str) -> ChatResult<BTreeMap<String, u8>> {
    let invalid = || {
        ChatError::new(
            ChatErrorCode::Persistence,
            "A stored delegation folder ceiling is invalid",
            false,
        )
    };
    let entries = serde_json::from_str::<Vec<Value>>(data).map_err(|_| invalid())?;
    let mut folders = BTreeMap::new();
    for entry in entries {
        let working_folder_id = entry
            .get("workingFolderId")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let rank = entry
            .get("capability")
            .and_then(Value::as_str)
            .and_then(capability_rank)
            .ok_or_else(invalid)?;
        if folders.insert(working_folder_id.to_owned(), rank).is_some() {
            return Err(invalid());
        }
    }
    Ok(folders)
}

fn capability_rank(capability: &str) -> Option<u8> {
    match capability {
        "none" => Some(0),
        "read" => Some(1),
        "edit" => Some(2),
        "execute" => Some(3),
        "publish" => Some(4),
        _ => None,
    }
}

/// A child an earlier call with the same client request id created.
struct ExistingChild {
    assignment_id: ChatWorkAssignmentId,
    teammate_id: String,
    objective: Option<String>,
    expected_result: Option<String>,
    ceiling: Option<String>,
    state: String,
}

impl ExistingChild {
    /// The existing child when the retry asks for exactly what created it, else a conflict.
    fn matching(
        self,
        request: &DelegationRequest<'_>,
        ceiling: &str,
        reply_thread_id: ChatReplyThreadId,
    ) -> Result<DelegatedChild, DelegationRefusal> {
        let same_ceiling = match (self.ceiling.as_deref(), parse_folder_ceiling(ceiling)) {
            (Some(stored), Ok(requested)) => {
                parse_folder_ceiling(stored).is_ok_and(|stored| stored == requested)
            }
            _ => false,
        };
        let same = self.teammate_id == request.teammate_id.as_str()
            && self.objective.as_deref() == Some(request.objective)
            && self.expected_result.as_deref() == Some(request.expected_result)
            && same_ceiling;
        if !same {
            return Err(DelegationRefusal::RequestConflict);
        }
        Ok(DelegatedChild {
            assignment_id: self.assignment_id,
            reply_thread_id,
            state: self.state,
        })
    }
}

/// What an earlier call left at the root message `item_id`.
async fn read_existing_child(
    connection: &mut SqliteConnection,
    request: &DelegationRequest<'_>,
    item_id: &ChatConversationItemId,
) -> ChatResult<PriorRequest> {
    let row = sqlx::query(
        "SELECT id, teammate_id, delegation_objective, delegation_expected_result,
                delegation_folder_ceiling, state
         FROM chat_work_assignments
         WHERE triggering_message_item_id = ? AND parent_assignment_id = ?",
    )
    .bind(item_id.as_str())
    .bind(request.parent_assignment_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if let Some(row) = row {
        return Ok(PriorRequest::Child(ExistingChild {
            assignment_id: ChatWorkAssignmentId::new(
                row.try_get::<String, _>("id").map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            teammate_id: row.try_get("teammate_id").map_err(persistence_error)?,
            objective: row
                .try_get("delegation_objective")
                .map_err(persistence_error)?,
            expected_result: row
                .try_get("delegation_expected_result")
                .map_err(persistence_error)?,
            ceiling: row
                .try_get("delegation_folder_ceiling")
                .map_err(persistence_error)?,
            state: row.try_get("state").map_err(persistence_error)?,
        }));
    }
    let item_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_conversation_items WHERE id = ?)")
            .bind(item_id.as_str())
            .fetch_one(&mut *connection)
            .await
            .map_err(persistence_error)?;
    Ok(if item_exists {
        PriorRequest::OtherWork
    } else {
        PriorRequest::None
    })
}

/// What an earlier call with the same client request id left.
enum PriorRequest {
    /// Nothing: this is the first call.
    None,
    /// The child it created.
    Child(ExistingChild),
    /// A root message without that child, so the id names other work.
    OtherWork,
}

/// The working parent of a delegation.
struct WorkingParent {
    teammate_id: ChatParticipantId,
    depth: u8,
    /// Whether the parent runs without approval prompts, so no person on this device consents
    /// to what its child runs and the child waits for a local acceptance.
    unattended: bool,
}

/// The parent of `request` when it is working in the request's conversation under the run's
/// still-allowed authorization.
async fn read_working_parent(
    connection: &mut SqliteConnection,
    request: &DelegationRequest<'_>,
) -> ChatResult<Result<WorkingParent, DelegationRefusal>> {
    let row = sqlx::query(
        "SELECT assignment.state, assignment.delegation_depth, assignment.teammate_id,
                thread.conversation_id,
                authorization.decision_state, authorization.revoked_at,
                authorization.resolved_runtime_approval_policy
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         LEFT JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = ? AND authorization.assignment_id = assignment.id
         WHERE assignment.id = ?",
    )
    .bind(request.parent_authorization_revision_id.as_str())
    .bind(request.parent_assignment_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(Err(DelegationRefusal::ParentInactive));
    };
    let state: String = row.try_get("state").map_err(persistence_error)?;
    let conversation_id: String = row.try_get("conversation_id").map_err(persistence_error)?;
    let decision: Option<String> = row.try_get("decision_state").map_err(persistence_error)?;
    let revoked_at: Option<String> = row.try_get("revoked_at").map_err(persistence_error)?;
    let approval: Option<String> = row
        .try_get("resolved_runtime_approval_policy")
        .map_err(persistence_error)?;
    if state != "working"
        || conversation_id != request.conversation_id.as_str()
        || decision.as_deref() != Some("allowed")
        || revoked_at.is_some()
    {
        return Ok(Err(DelegationRefusal::ParentInactive));
    }
    let depth = row
        .try_get::<i64, _>("delegation_depth")
        .map_err(persistence_error)?;
    if delegation_chain_length(connection, request.parent_assignment_id).await? != Some(depth) {
        return Ok(Err(DelegationRefusal::ParentInactive));
    }
    Ok(Ok(WorkingParent {
        teammate_id: ChatParticipantId::new(
            row.try_get::<String, _>("teammate_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        depth: u8::try_from(depth).map_err(|_| stored_value_error("delegation depth"))?,
        unattended: approval.as_deref() == Some("unattended"),
    }))
}

/// How many delegations separate `assignment_id` from the root of its chain, or `None` when an
/// ancestor is missing or the chain is longer than [`MAX_DELEGATION_DEPTH`].
///
/// Sync applies remote assignments without checking a child's depth against its parent, so a
/// device walks the stored chain before it trusts a depth that bounds further delegation.
async fn delegation_chain_length(
    connection: &mut SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Option<i64>> {
    let (length, complete): (Option<i64>, Option<bool>) = sqlx::query_as(
        "WITH RECURSIVE chain(id, parent_id, hops) AS (
           SELECT id, parent_assignment_id, 0 FROM chat_work_assignments WHERE id = ?
           UNION ALL
           SELECT ancestor.id, ancestor.parent_assignment_id, chain.hops + 1
           FROM chat_work_assignments ancestor
           JOIN chain ON ancestor.id = chain.parent_id
           WHERE chain.hops <= ?
         )
         SELECT max(hops), max(hops) = max(CASE WHEN parent_id IS NULL THEN hops END)
         FROM chain",
    )
    .bind(assignment_id.as_str())
    .bind(i64::from(MAX_DELEGATION_DEPTH))
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(length.filter(|hops| complete == Some(true) && *hops <= i64::from(MAX_DELEGATION_DEPTH)))
}

struct DelegationTarget {
    display_name: String,
    channel_id: ChatChannelId,
}

/// The target's display name and the request's channel, when that channel is not archived.
async fn read_target(
    connection: &mut SqliteConnection,
    request: &DelegationRequest<'_>,
) -> ChatResult<Option<DelegationTarget>> {
    let row = sqlx::query(
        "SELECT participant.display_name, channel.id AS channel_id
         FROM chat_participants participant
         JOIN chat_channels channel ON channel.conversation_id = ?
         WHERE participant.id = ? AND channel.archived_at IS NULL",
    )
    .bind(request.conversation_id.as_str())
    .bind(request.teammate_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    row.map(|row| {
        Ok(DelegationTarget {
            display_name: row.try_get("display_name").map_err(persistence_error)?,
            channel_id: ChatChannelId::new(
                row.try_get::<String, _>("channel_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
        })
    })
    .transpose()
}

/// The parent's root message: a mention of the target, the objective, and the expected result.
/// The mention is a participant reference over `@name`, so the timeline renders it as one.
fn delegation_message(
    request: &DelegationRequest<'_>,
    target: &DelegationTarget,
    parts: &[&str],
) -> ChatResult<PostChatMessageCommand> {
    let display_name = target.display_name.as_str();
    let mention = format!("@{display_name}");
    let text = format!(
        "{mention} {}\n\nExpected result: {}",
        request.objective, request.expected_result
    );
    let reference_id = ChatMessageReferenceId::new(derived_id("message-reference", parts)?)
        .map_err(identifier_error)?;
    let command_id = ChatCommandId::new(derived_id("command", parts)?).map_err(identifier_error)?;
    Ok(PostChatMessageCommand {
        client_command_id: command_id,
        channel_id: target.channel_id.clone(),
        reply_thread_id: None,
        normalized_markdown: text,
        rich_content: VersionedJson {
            schema_version: 1,
            value: json!({ "type": "doc", "content": [] }),
        },
        attachment_ids: Vec::new(),
        references: vec![ChatMessageReference::Participant {
            metadata: ChatReferenceMetadata {
                reference_id,
                label_snapshot: display_name.to_owned(),
                start_offset: 0,
                end_offset: u64::try_from(mention.len()).unwrap_or(u64::MAX),
                plain_text_projection: mention,
            },
            participant_id: request.teammate_id.clone(),
            participant_kind: ChatParticipantKind::AiTeammate,
        }],
        execution_target: None,
        also_send_to_channel: false,
        followup_routing: None,
        model_override: None,
    })
}

/// Opens the child's reply thread at the root message and records the channel's activity.
async fn open_reply_thread(
    connection: &mut SqliteConnection,
    request: &DelegationRequest<'_>,
    item_id: &ChatConversationItemId,
    thread_id: &ChatReplyThreadId,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_reply_threads
            (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(thread_id.as_str())
    .bind(request.conversation_id.as_str())
    .bind(item_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_conversations
         SET last_activity_at = ?, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(request.conversation_id.as_str())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn read_state(
    connection: &mut SqliteConnection,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<String> {
    sqlx::query_scalar("SELECT state FROM chat_work_assignments WHERE id = ?")
        .bind(assignment_id.as_str())
        .fetch_one(&mut *connection)
        .await
        .map_err(persistence_error)
}

/// The authority a delegated assignment may hold now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DelegationBound {
    /// Nobody delegated the assignment; only its own grants bound it.
    Undelegated,
    /// The assignment may hold at most this.
    Bound(DelegationCeiling),
    /// An ancestor lost the authority the assignment was delegated under.
    Revoked,
}

/// The most a delegated assignment may hold: its ceiling folders, each narrowed by what every
/// ancestor holds on this device, and the narrowest approval among those ancestors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DelegationCeiling {
    /// Folder id to its bound.
    pub(super) folders: BTreeMap<String, FolderBound>,
    /// Rank of the broadest runtime approval the assignment may run with.
    pub(super) approval_rank: u8,
    /// The person who started the root of the delegation chain. Delegated work runs on that
    /// person's behalf, not the parent teammate's.
    pub(super) requester_participant_id: Option<ChatParticipantId>,
}

/// The bound of one ceiling folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FolderBound {
    pub(super) capability_rank: u8,
    pub(super) approval_rank: u8,
}

/// Rank of the broadest approval: work may run unattended unless an ancestor narrows it.
const BROADEST_APPROVAL_RANK: u8 = 2;

/// Reads the bound of `assignment_id` by walking its ancestors.
///
/// An ancestor that is gone or cancelled revokes the bound. An ancestor whose authorization is
/// on this device must still hold it: allowed and unrevoked, or revoked only because the
/// ancestor settled or moved to another device, which ends its run but not the authority it
/// delegated. Every ceiling folder must be among that authorization's folders at a capability
/// at least the ceiling's, and its approval narrows the bound. An ancestor that runs on
/// another device has no authorization here, so its child keeps the ceiling folders and steps
/// down to asking for approval.
pub(super) async fn read_delegation_bound(
    connection: &mut SqliteConnection,
    assignment_id: &str,
) -> ChatResult<DelegationBound> {
    let row = sqlx::query(
        "SELECT parent_assignment_id, delegation_folder_ceiling
         FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let parent: Option<String> = row
        .try_get("parent_assignment_id")
        .map_err(persistence_error)?;
    let Some(mut ancestor_id) = parent else {
        return Ok(DelegationBound::Undelegated);
    };
    let ceiling: Option<String> = row
        .try_get("delegation_folder_ceiling")
        .map_err(persistence_error)?;
    let ceiling = parse_folder_ceiling(ceiling.as_deref().unwrap_or("[]"))?;
    let mut folders = ceiling
        .into_iter()
        .map(|(id, capability_rank)| {
            (
                id,
                FolderBound {
                    capability_rank,
                    approval_rank: BROADEST_APPROVAL_RANK,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut approval = BROADEST_APPROVAL_RANK;
    let mut requester = None;
    for _ in 0..MAX_DELEGATION_DEPTH {
        let Some(ancestor) = read_ancestor(connection, &ancestor_id).await? else {
            return Ok(DelegationBound::Revoked);
        };
        if ancestor.state == "cancelled" {
            return Ok(DelegationBound::Revoked);
        }
        match ancestor.authorization {
            None => approval = 0,
            Some(authorization) => {
                if !authorization.live {
                    return Ok(DelegationBound::Revoked);
                }
                approval = approval.min(authorization.approval_rank);
                for (working_folder_id, bound) in &mut folders {
                    let Some(source) = authorization.folders.get(working_folder_id) else {
                        return Ok(DelegationBound::Revoked);
                    };
                    if source.capability_rank < bound.capability_rank {
                        return Ok(DelegationBound::Revoked);
                    }
                    bound.approval_rank = bound.approval_rank.min(source.approval_rank);
                }
            }
        }
        match ancestor.parent_assignment_id {
            Some(next) => ancestor_id = next,
            None => {
                requester = Some(ancestor.trigger_author_participant_id);
                break;
            }
        }
    }
    for bound in folders.values_mut() {
        bound.approval_rank = bound.approval_rank.min(approval);
    }
    Ok(DelegationBound::Bound(DelegationCeiling {
        folders,
        approval_rank: approval,
        requester_participant_id: requester,
    }))
}

struct Ancestor {
    state: String,
    parent_assignment_id: Option<String>,
    /// Author of the message that triggered the ancestor: for work a person started, that
    /// person.
    trigger_author_participant_id: ChatParticipantId,
    authorization: Option<AncestorAuthorization>,
}

struct AncestorAuthorization {
    live: bool,
    approval_rank: u8,
    folders: BTreeMap<String, FolderBound>,
}

/// Reasons that end a run's authorization without taking back the authority it delegated.
pub(crate) fn is_lifecycle_revocation(reason: &str) -> bool {
    reason == ganbaru_chat::sync_effects::ASSIGNMENT_SETTLED_REASON
        || reason == super::assignments::MOVED_EXECUTOR_REASON
}

async fn read_ancestor(
    connection: &mut SqliteConnection,
    ancestor_id: &str,
) -> ChatResult<Option<Ancestor>> {
    let Some(row) = sqlx::query(
        "SELECT assignment.state, assignment.parent_assignment_id,
                authorization.id AS authorization_id, authorization.decision_state,
                authorization.revoked_at, authorization.reason,
                authorization.resolved_runtime_approval_policy,
                message.author_participant_id AS trigger_author_participant_id
         FROM chat_work_assignments assignment
         JOIN chat_communication_messages message
           ON message.item_id = assignment.triggering_message_item_id
         LEFT JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = (
               SELECT latest.id FROM chat_assignment_authorization_revisions latest
               WHERE latest.assignment_id = assignment.id
               ORDER BY latest.revision DESC, latest.created_at DESC LIMIT 1
           )
         WHERE assignment.id = ?",
    )
    .bind(ancestor_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?
    else {
        return Ok(None);
    };
    let authorization_id: Option<String> =
        row.try_get("authorization_id").map_err(persistence_error)?;
    let authorization = match authorization_id {
        None => None,
        Some(authorization_id) => {
            let decision: String = row.try_get("decision_state").map_err(persistence_error)?;
            let revoked_at: Option<String> =
                row.try_get("revoked_at").map_err(persistence_error)?;
            let reason: String = row.try_get("reason").map_err(persistence_error)?;
            let live = (decision == "allowed" && revoked_at.is_none())
                || (decision == "revoked" && is_lifecycle_revocation(&reason));
            let policy: String = row
                .try_get("resolved_runtime_approval_policy")
                .map_err(persistence_error)?;
            let approval_rank = approval_rank(&policy).unwrap_or(0);
            let folders = sqlx::query(
                "SELECT working_folder_id, capability, resolved_runtime_approval_policy
                 FROM chat_assignment_authorized_folder_sources
                 WHERE authorization_revision_id = ?",
            )
            .bind(&authorization_id)
            .fetch_all(&mut *connection)
            .await
            .map_err(persistence_error)?
            .into_iter()
            .map(|source| {
                let working_folder_id: String = source
                    .try_get("working_folder_id")
                    .map_err(persistence_error)?;
                let capability: String = source.try_get("capability").map_err(persistence_error)?;
                let folder_policy: Option<String> = source
                    .try_get("resolved_runtime_approval_policy")
                    .map_err(persistence_error)?;
                Ok((
                    working_folder_id,
                    FolderBound {
                        capability_rank: capability_rank(&capability).unwrap_or(0),
                        approval_rank: approval_rank_or_ask(
                            folder_policy.as_deref().unwrap_or(&policy),
                        ),
                    },
                ))
            })
            .collect::<ChatResult<BTreeMap<_, _>>>()?;
            Some(AncestorAuthorization {
                live,
                approval_rank,
                folders,
            })
        }
    };
    Ok(Some(Ancestor {
        state: row.try_get("state").map_err(persistence_error)?,
        parent_assignment_id: row
            .try_get("parent_assignment_id")
            .map_err(persistence_error)?,
        trigger_author_participant_id: ChatParticipantId::new(
            row.try_get::<String, _>("trigger_author_participant_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        authorization,
    }))
}

/// The rank of `policy`, counting a policy that cannot be ranked, such as provider-custom
/// approval, as asking.
fn approval_rank_or_ask(policy: &str) -> u8 {
    approval_rank(policy).unwrap_or(0)
}

/// `policy` narrowed to `bound_rank`. A policy that cannot be ranked steps down to asking,
/// since nothing here can show it stays within the bound.
pub(super) fn clamp_approval(policy: &str, bound_rank: u8) -> &'static str {
    let rank = approval_rank(policy).map_or(0, |rank| rank.min(bound_rank));
    match rank {
        0 => "ask",
        1 => "auto_approve",
        _ => "unattended",
    }
}

/// Cancels delegated work whose parent lost its authority, before it starts, and reports the
/// cancellation in the parent's reply thread.
pub(super) async fn cancel_for_revoked_parent(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'cancelled', state_reason = ?, settled_at = ?,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND state NOT IN ('completed', 'failed', 'cancelled')",
    )
    .bind(PARENT_AUTHORITY_REVOKED_REASON)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(assignment_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    ganbaru_chat::coordination::delegation::post_delegation_outcome(
        connection,
        assignment_id,
        now.as_str(),
    )
    .await?;
    Ok(())
}

/// Whether the frozen authorization of `assignment_id` exceeds `bound`: a broader approval, a
/// folder outside the ceiling, or a folder above its ceiling capability or approval.
pub(super) async fn frozen_authority_exceeds(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    bound: &DelegationCeiling,
) -> ChatResult<bool> {
    let Some(authorization) = read_ancestor(connection, assignment_id)
        .await?
        .and_then(|assignment| assignment.authorization)
    else {
        return Ok(false);
    };
    if authorization.approval_rank > bound.approval_rank {
        return Ok(true);
    }
    Ok(authorization
        .folders
        .iter()
        .any(|(working_folder_id, held)| {
            bound.folders.get(working_folder_id).is_none_or(|limit| {
                held.capability_rank > limit.capability_rank
                    || held.approval_rank > limit.approval_rank
            })
        }))
}

#[cfg(test)]
mod tests;
