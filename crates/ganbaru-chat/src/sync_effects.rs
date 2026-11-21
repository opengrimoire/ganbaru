//! Device-local Chat state derived from replicated rows that sync materialized.
//!
//! Commands keep audience revisions and message search current through triggers that stand down
//! while sync applies rows. After the sync engine writes rows of one table, [`after_materialize`]
//! does the same work for the changed keys inside the engine transaction, with plain SQL and no
//! dependency on the engine, so any caller holding the connection can run it. It never writes
//! replicated rows.

use crate::attachment_files;
use crate::coordination::access::{access_ceiling, effective_access_level_sql};
use crate::coordination::revocation::{revoke_assignment_authorizations, revoke_authorization};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::repository::state_writes::sources_sql;
use crate::state::ChatDispatchJobState;
use chrono::{DateTime, SecondsFormat, Utc};
use ganbaru_chat_contracts::models::{
    ChatAgentAccess, ChatFolderCapability, ChatRuntimeApprovalPolicy,
};
use sqlx::SqliteConnection;
use std::collections::BTreeSet;
use std::time::SystemTime;

/// Replicated table of Chat participants, keyed by id.
pub const PARTICIPANTS_TABLE: &str = "chat_participants";
/// Replicated single row table of app agent settings.
pub const APP_SETTINGS_TABLE: &str = "chat_agent_app_settings";
/// Replicated table of project agent settings, keyed by project id.
pub const PROJECT_SETTINGS_TABLE: &str = "chat_project_agent_settings";
/// Replicated table of project channels, keyed by id.
pub const CHANNELS_TABLE: &str = "chat_channels";
/// Replicated table of conversations, keyed by id.
pub const CONVERSATIONS_TABLE: &str = "chat_conversations";
/// Replicated table of conversation memberships, keyed by sync key.
pub const MEMBERSHIPS_TABLE: &str = "chat_conversation_memberships";
/// Replicated table of AI channel memberships, keyed by sync key.
pub const CHANNEL_MEMBERSHIPS_TABLE: &str = "chat_agent_channel_memberships";
/// Replicated table of messages, keyed by item id.
pub const MESSAGES_TABLE: &str = "chat_communication_messages";
/// Replicated table of message revisions, keyed by id.
pub const MESSAGE_REVISIONS_TABLE: &str = "chat_communication_message_revisions";
/// Replicated table of work assignments, keyed by id.
pub const ASSIGNMENTS_TABLE: &str = "chat_work_assignments";
/// Replicated table of work assignment inputs, keyed by id.
pub const ASSIGNMENT_INPUTS_TABLE: &str = "chat_work_assignment_inputs";

/// Reason recorded on authorizations revoked because their assignment settled on another device.
pub const ASSIGNMENT_SETTLED_REASON: &str = "The work assignment settled on another device";
/// Reason recorded on authorizations revoked because the channel access or membership that
/// covered them was narrowed or removed.
pub const ACCESS_REDUCTION_REASON: &str = "Agent access was reduced";
/// Reason recorded on authorizations revoked because the agent was archived.
pub const AGENT_ARCHIVED_REASON: &str = "The agent was archived";

/// Device-local state [`after_materialize`] changed, so the caller can wake the services and
/// views that read it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChatSyncEffects {
    /// Conversations whose audience state was created or whose audience revision was bumped.
    pub audience_revised: BTreeSet<String>,
    /// Agents with authorizations revoked and access revocation jobs queued for them.
    pub revoked_agents: BTreeSet<String>,
    /// Conversations that still hold unrevoked push approvals that lapsed, because the
    /// conversation's effective access is below ask or the approved agent is no longer present
    /// or is archived. Push approvals replicate, so the hook leaves them for the caller to revoke
    /// with [`revoke_lapsed_push_approvals`] once the apply ends.
    pub lapsed_push_approvals: BTreeSet<String>,
    /// Conversations that still hold unrevoked remembered approvals that lapsed, because the
    /// conversation's effective access fell below the access an approval was granted under or
    /// the approved agent is no longer present or is archived. The caller revokes them with
    /// [`revoke_lapsed_remembered_approvals`] once the apply ends.
    pub lapsed_remembered_approvals: BTreeSet<String>,
    /// Messages whose search row was rebuilt or removed.
    pub search_rebuilt: BTreeSet<String>,
    /// Conversations the apply deleted. The delete cascades away the authorizations that
    /// target them, with their runs, so no revocation job can name those runs; the caller stops
    /// running work whose authorization no longer exists.
    pub removed_conversations: BTreeSet<String>,
    /// Attachments whose device row, created for a replicated revision, was removed because no
    /// reference on this device names them any more.
    pub reclaimed_attachments: BTreeSet<String>,
    /// Queued assignments that arrived or changed, which their executing device starts.
    pub queued_assignments: BTreeSet<String>,
    /// Started assignments that arrived or changed and are not settled. A device that started
    /// one whose concurrent creates merged onto another executing device stops its own run.
    pub started_assignments: BTreeSet<String>,
    /// Settled assignments whose local dispatch jobs ended or whose active authorizations were
    /// revoked, so the runs they granted stop.
    pub settled_assignments: BTreeSet<String>,
    /// Assignments with an arrived input still pending, for running work to receive or for work
    /// whose turn ended to continue with in a follow-up.
    pub routed_inputs: BTreeSet<String>,
    /// Assignments the apply deleted. The delete cascades away their runs and authorizations,
    /// so the caller stops running work whose authorization no longer exists.
    pub removed_assignments: BTreeSet<String>,
    /// Delegated children that arrived or changed in a state that reports an outcome, which
    /// the parent's executing device posts in the parent's reply thread.
    pub delegated_outcomes: BTreeSet<String>,
}

impl ChatSyncEffects {
    /// Returns whether no device-local state changed and nothing was found to report.
    pub fn is_empty(&self) -> bool {
        self.audience_revised.is_empty()
            && self.revoked_agents.is_empty()
            && self.lapsed_push_approvals.is_empty()
            && self.lapsed_remembered_approvals.is_empty()
            && self.search_rebuilt.is_empty()
            && self.removed_conversations.is_empty()
            && self.reclaimed_attachments.is_empty()
            && self.queued_assignments.is_empty()
            && self.started_assignments.is_empty()
            && self.settled_assignments.is_empty()
            && self.routed_inputs.is_empty()
            && self.removed_assignments.is_empty()
            && self.delegated_outcomes.is_empty()
    }
}

/// Recomputes the device-local Chat state derived from the rows of `table` whose keys are in
/// `changed_keys`, after sync inserted, updated, or deleted them.
///
/// It mirrors the gated command triggers: a conversation gets its audience state, a membership
/// bumps the audience of its conversation, and a message or revision rebuilds the message's
/// search row (a deleted message loses it). A revision also creates the attachment and resource
/// rows of the attachments it names in a working folder this device has, and a changed
/// revision, message, or deleted conversation removes such rows once nothing on this device
/// references them. A deleted conversation is reported.
///
/// An app, project, or channel settings row, a membership, an archived agent, or a deleted
/// conversation runs [`revoke_exceeding_authorizations`], so a narrowing at any scope reaches
/// every channel that inherits it and stops the work it no longer covers. The same rows report
/// the channels whose push approvals lapsed. Both compare current rows, so arrival order
/// does not matter and a widening or a repeated apply revokes nothing. A later widening does
/// not bring a revoked authorization back.
///
/// A queued work assignment is reported for its executing device to start, a settled one ends
/// its local dispatch job and revokes its active authorizations, and an input still pending
/// names its assignment for delivery. Timestamps come from the changed rows, except
/// revocations, which happen now on this device. Tables without derived state return no
/// effects.
pub async fn after_materialize(
    connection: &mut SqliteConnection,
    table: &str,
    changed_keys: &[String],
) -> ChatResult<ChatSyncEffects> {
    let mut effects = ChatSyncEffects::default();
    let keys: BTreeSet<&str> = changed_keys.iter().map(String::as_str).collect();
    match table {
        PARTICIPANTS_TABLE | APP_SETTINGS_TABLE | PROJECT_SETTINGS_TABLE | CHANNELS_TABLE => {
            sweep_exceeding_authorizations(connection, &mut effects).await?;
        }
        CONVERSATIONS_TABLE => {
            for key in keys {
                if !create_audience_state(connection, key, &mut effects).await? {
                    effects.removed_conversations.insert(key.to_string());
                }
            }
            if !effects.removed_conversations.is_empty() {
                sweep_exceeding_authorizations(connection, &mut effects).await?;
                reclaim_applied_attachments(connection, &mut effects).await?;
            }
        }
        MEMBERSHIPS_TABLE => {
            revise_audiences(connection, MEMBERSHIP_ROW_BY_KEY, &keys, &mut effects).await?;
            sweep_exceeding_authorizations(connection, &mut effects).await?;
        }
        CHANNEL_MEMBERSHIPS_TABLE => {
            revise_audiences(
                connection,
                CHANNEL_MEMBERSHIP_ROW_BY_KEY,
                &keys,
                &mut effects,
            )
            .await?;
            sweep_exceeding_authorizations(connection, &mut effects).await?;
        }
        MESSAGES_TABLE => {
            for key in keys {
                rebuild_message_search(connection, key, &mut effects).await?;
            }
            reclaim_applied_attachments(connection, &mut effects).await?;
        }
        MESSAGE_REVISIONS_TABLE => {
            reclaim_applied_attachments(connection, &mut effects).await?;
            let mut message_ids = BTreeSet::new();
            for key in keys {
                record_applied_attachments(connection, key).await?;
                let message_id: Option<String> = sqlx::query_scalar(
                    "SELECT message_item_id FROM chat_communication_message_revisions
                     WHERE id = ?",
                )
                .bind(key)
                .fetch_optional(&mut *connection)
                .await
                .map_err(persistence_error)?;
                message_ids.extend(message_id);
            }
            for message_id in message_ids {
                rebuild_message_search(connection, &message_id, &mut effects).await?;
            }
        }
        ASSIGNMENTS_TABLE => {
            for key in keys {
                follow_assignment_state(connection, key, &mut effects).await?;
            }
        }
        ASSIGNMENT_INPUTS_TABLE => {
            for key in keys {
                let assignment_id: Option<String> = sqlx::query_scalar(
                    "SELECT input.assignment_id
                     FROM chat_work_assignment_inputs input
                     JOIN chat_work_assignments assignment
                       ON assignment.id = input.assignment_id
                     WHERE input.id = ? AND input.delivery_state = 'pending'
                       AND (assignment.state NOT IN ('completed', 'failed', 'cancelled') OR (
                         assignment.state = 'completed' AND assignment.review_outcome IS NULL
                         AND NOT EXISTS (
                           SELECT 1 FROM chat_work_assignments successor
                           WHERE successor.previous_assignment_id = assignment.id
                         )
                       ))",
                )
                .bind(key)
                .fetch_optional(&mut *connection)
                .await
                .map_err(persistence_error)?;
                effects.routed_inputs.extend(assignment_id);
            }
        }
        _ => {}
    }
    Ok(effects)
}

/// One attachment a revision names by value, as its owned reference row holds it.
#[derive(sqlx::FromRow)]
struct AppliedAttachment {
    attachment_id: String,
    working_folder_id: String,
    kind: String,
    original_display_name: String,
    mime_type: String,
    byte_size: i64,
    sha256: String,
    created_at: String,
}

/// Creates the device's attachment row and its resource for every attachment the revision
/// names whose working folder exists here, so the revision's attachments read like local ones.
/// The file arrives through the asset transfer under its digest name; an attachment row the
/// device already has is kept unchanged.
async fn record_applied_attachments(
    connection: &mut SqliteConnection,
    revision_id: &str,
) -> ChatResult<()> {
    let attachments: Vec<AppliedAttachment> = sqlx::query_as(
        "SELECT reference.attachment_id, reference.working_folder_id, reference.kind,
                reference.original_display_name, reference.mime_type, reference.byte_size,
                reference.sha256, revision.created_at
         FROM chat_communication_attachment_references reference
         JOIN chat_communication_message_revisions revision
           ON revision.id = reference.message_revision_id
         JOIN project_working_folders folder ON folder.id = reference.working_folder_id
         WHERE reference.message_revision_id = ?
         ORDER BY reference.ordinal",
    )
    .bind(revision_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    for attachment in attachments {
        let relative_path =
            attachment_files::relative_path(&attachment.sha256, &attachment.mime_type);
        let created = sqlx::query(
            "INSERT OR IGNORE INTO chat_attachments
                 (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                  sha256, managed_relative_path, signature_kind, created_at, origin)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'replicated')",
        )
        .bind(&attachment.attachment_id)
        .bind(&attachment.working_folder_id)
        .bind(&attachment.kind)
        .bind(&attachment.original_display_name)
        .bind(&attachment.mime_type)
        .bind(attachment.byte_size)
        .bind(&attachment.sha256)
        .bind(&relative_path)
        .bind(attachment_files::signature_kind(&attachment.mime_type))
        .bind(&attachment.created_at)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?
        .rows_affected();
        if created == 0 {
            continue;
        }
        sqlx::query(
            "INSERT OR IGNORE INTO chat_resources
                 (id, working_folder_id, attachment_id, resource_kind, display_name, mime_type,
                  byte_size, sha256, managed_relative_path, resource_uri, integrity_state,
                  created_at)
             VALUES (?1, ?2, ?1, ?3, ?4, ?5, ?6, ?7, ?8, 'ganbaru://chat/resource/' || ?1,
                     'verified', ?9)",
        )
        .bind(&attachment.attachment_id)
        .bind(&attachment.working_folder_id)
        .bind(&attachment.kind)
        .bind(&attachment.original_display_name)
        .bind(&attachment.mime_type)
        .bind(attachment.byte_size)
        .bind(&attachment.sha256)
        .bind(&relative_path)
        .bind(&attachment.created_at)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

/// Removes the attachment rows this device created for replicated revisions that no reference
/// on this device names any more, after an applied delete of a revision, message, or
/// conversation took the last one away. A draft, queued, message, scheduled message, terminal
/// context, or resource thread reference keeps the row. Removing the row cascades to its
/// resource. The file stays: the sync asset sweep removes it once nothing names its digest.
///
/// Assignment context packages list message sources only, never attachments. A provider turn
/// names its attachments through the message references it persists with the turn, which keep
/// the row; a launch that read an attachment just before its row was reclaimed fails on that
/// reference's foreign key instead of naming a missing row.
async fn reclaim_applied_attachments(
    connection: &mut SqliteConnection,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let reclaimed: Vec<String> = sqlx::query_scalar(
        "DELETE FROM chat_attachments
         WHERE origin = 'replicated'
           AND NOT EXISTS (SELECT 1 FROM chat_attachment_references r WHERE r.attachment_id = chat_attachments.id)
           AND NOT EXISTS (SELECT 1 FROM chat_queued_attachment_references q WHERE q.attachment_id = chat_attachments.id)
           AND NOT EXISTS (
               SELECT 1 FROM chat_communication_attachment_references c
               WHERE c.attachment_id = chat_attachments.id
           )
           AND NOT EXISTS (
               SELECT 1 FROM chat_scheduled_message_attachment_references s
               WHERE s.attachment_id = chat_attachments.id
           )
           AND NOT EXISTS (
               SELECT 1 FROM chat_terminal_attachment_contexts t
               WHERE t.attachment_id = chat_attachments.id
           )
           AND NOT EXISTS (
               SELECT 1 FROM chat_resources resource
               JOIN chat_resource_thread_references reference ON reference.resource_id = resource.id
               WHERE resource.attachment_id = chat_attachments.id
           )
         RETURNING id",
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    effects.reclaimed_attachments.extend(reclaimed);
    Ok(())
}

/// Creates the audience state of the conversation and returns whether the conversation is
/// still present.
async fn create_audience_state(
    connection: &mut SqliteConnection,
    conversation_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<bool> {
    let created = sqlx::query(
        "INSERT OR IGNORE INTO chat_conversation_audience_state (conversation_id, updated_at)
         SELECT id, updated_at FROM chat_conversations WHERE id = ?",
    )
    .bind(conversation_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if created > 0 {
        effects.audience_revised.insert(conversation_id.to_string());
        return Ok(true);
    }
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chat_conversations WHERE id = ?)")
        .bind(conversation_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(persistence_error)
}

/// Conversation and change time of the conversation membership with sync key `?`.
const MEMBERSHIP_ROW_BY_KEY: &str = "SELECT conversation_id, updated_at
     FROM chat_conversation_memberships WHERE sync_key = ?";

/// Conversation and change time of the AI channel membership with sync key `?`.
const CHANNEL_MEMBERSHIP_ROW_BY_KEY: &str = "SELECT conversation_id, updated_at
     FROM chat_agent_channel_memberships WHERE sync_key = ?";

/// Bumps the audience of the conversation of every changed membership still present, as the
/// gated membership triggers do. A deleted membership no longer names its conversation; the
/// conversation's own delete or the next membership change covers it.
async fn revise_audiences(
    connection: &mut SqliteConnection,
    row_by_key: &'static str,
    keys: &BTreeSet<&str>,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    for key in keys {
        let row: Option<(String, String)> = sqlx::query_as(row_by_key)
            .bind(key)
            .fetch_optional(&mut *connection)
            .await
            .map_err(persistence_error)?;
        let Some((conversation_id, changed_at)) = row else {
            continue;
        };
        let revised = sqlx::query(
            "UPDATE chat_conversation_audience_state
             SET revision = revision + 1, updated_at = ?
             WHERE conversation_id = ?",
        )
        .bind(&changed_at)
        .bind(&conversation_id)
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?
        .rows_affected();
        if revised > 0 {
            effects.audience_revised.insert(conversation_id);
        }
    }
    Ok(())
}

/// Revokes the authorizations [`revoke_exceeding_authorizations`] finds, records their agents,
/// and reports the conversations whose push or remembered approvals lapsed without revoking them.
async fn sweep_exceeding_authorizations(
    connection: &mut SqliteConnection,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let revoked = revoke_exceeding_authorizations(connection, &now()).await?;
    effects.revoked_agents.extend(revoked);
    effects
        .lapsed_push_approvals
        .extend(lapsed_push_approval_channels(connection).await?);
    effects
        .lapsed_remembered_approvals
        .extend(lapsed_remembered_approval_channels(connection).await?);
    Ok(())
}

/// Every folder capability, for the rank table the sweep compares capabilities with.
const FOLDER_CAPABILITIES: [ChatFolderCapability; 5] = [
    ChatFolderCapability::None,
    ChatFolderCapability::Read,
    ChatFolderCapability::Edit,
    ChatFolderCapability::Execute,
    ChatFolderCapability::Publish,
];

/// SQL common table expressions for the most each stored access level allows, from
/// [`access_ceiling`], and the rank of each stored folder capability.
fn ceiling_tables_sql() -> String {
    let ceilings = ChatAgentAccess::ALL
        .into_iter()
        .map(|level| {
            let ceiling = access_ceiling(level);
            format!(
                "('{}', {}, {}, {})",
                level.as_str(),
                ceiling.folder_capability.rank(),
                u8::from(ceiling.auto_approve),
                u8::from(ceiling.scratch_allowed)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let capabilities = FOLDER_CAPABILITIES
        .into_iter()
        .map(|capability| format!("('{}', {})", capability.as_str(), capability.rank()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "access_ceiling(level, folder_rank, auto_approve, scratch_allowed) AS (
             VALUES {ceilings}
         ),
         capability_rank(capability, rank) AS (VALUES {capabilities})"
    )
}

/// SQL condition that holds when the AI agent `agent_id` is a present member of the
/// conversation `conversation_id`, both columns of the enclosing query: its conversation
/// membership and its AI channel membership are unremoved.
fn agent_present_sql(conversation_id: &str, agent_id: &str) -> String {
    format!(
        "EXISTS (
             SELECT 1 FROM chat_conversation_memberships presence_membership
             JOIN chat_agent_channel_memberships presence_channel
               ON presence_channel.conversation_id = presence_membership.conversation_id
              AND presence_channel.agent_id = presence_membership.participant_id
              AND presence_channel.removed_at IS NULL
             WHERE presence_membership.conversation_id = {conversation_id}
               AND presence_membership.participant_id = {agent_id}
               AND presence_membership.removed_at IS NULL
         )"
    )
}

/// SQL query for the active authorizations of AI agents that current rows no longer cover, with
/// their agent and whether the agent is archived.
///
/// An authorization is uncovered when its agent is archived, when the agent is no longer a
/// present member of the destination, when a channel it disclosed no longer has the agent
/// present, when a folder it was given is outside the destination conversation's project, or
/// when its frozen authority exceeds the ceiling of the destination's live access level: a talk
/// level allows no execution environment or folder target, a level without scratch allows no
/// scratch, a folder source may not rank above the level's folder capability, and any approval
/// other than ask needs a level that auto approves. A level the ceiling table does not know
/// allows nothing.
fn exceeding_authorizations_sql() -> String {
    let tables = ceiling_tables_sql();
    let level = effective_access_level_sql("authorization.destination_conversation_id");
    let destination_present = agent_present_sql(
        "authorization.destination_conversation_id",
        "assignment.agent_id",
    );
    let source_present = agent_present_sql("source.conversation_id", "assignment.agent_id");
    let ask = ChatRuntimeApprovalPolicy::Ask.as_str();
    format!(
        "WITH {tables}
         SELECT authorization.id, assignment.agent_id,
                participant.archived_at IS NOT NULL AS archived
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         JOIN chat_participants participant ON participant.id = assignment.agent_id
         LEFT JOIN chat_conversations conversation
           ON conversation.id = authorization.destination_conversation_id
         LEFT JOIN access_ceiling ceiling ON ceiling.level = {level}
         WHERE authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL
           AND participant.participant_kind = 'ai_agent'
           AND (
               participant.archived_at IS NOT NULL
               OR conversation.id IS NULL
               OR NOT {destination_present}
               OR (coalesce(ceiling.folder_rank, 0) = 0
                   AND (authorization.execution_environment_id IS NOT NULL
                        OR authorization.working_folder_id IS NOT NULL))
               OR (coalesce(ceiling.scratch_allowed, 0) = 0
                   AND EXISTS (
                       SELECT 1 FROM chat_execution_environments environment
                       WHERE environment.id = authorization.execution_environment_id
                         AND environment.kind = 'scratch'
                   ))
               OR (coalesce(ceiling.auto_approve, 0) = 0
                   AND authorization.resolved_runtime_approval_policy IS NOT '{ask}')
               OR EXISTS (
                   SELECT 1 FROM chat_assignment_authorized_folder_sources folder_source
                   LEFT JOIN capability_rank source_rank
                     ON source_rank.capability = folder_source.capability
                   WHERE folder_source.authorization_revision_id = authorization.id
                     AND (
                         source_rank.rank IS NULL
                         OR source_rank.rank > coalesce(ceiling.folder_rank, 0)
                         OR (coalesce(ceiling.auto_approve, 0) = 0
                             AND folder_source.resolved_runtime_approval_policy IS NOT NULL
                             AND folder_source.resolved_runtime_approval_policy IS NOT '{ask}')
                         OR NOT EXISTS (
                             SELECT 1 FROM project_working_folders folder
                             WHERE folder.id = folder_source.working_folder_id
                               AND folder.project_id IS conversation.project_id
                         )
                     )
               )
               OR EXISTS (
                   SELECT 1 FROM chat_assignment_authorized_channel_sources source
                   WHERE source.authorization_revision_id = authorization.id
                     AND NOT {source_present}
               )
           )
         ORDER BY authorization.id"
    )
}

/// Revokes every active authorization of an AI agent that current rows no longer cover (see
/// [`exceeding_authorizations_sql`]), with the work delegated from it, and queues a revocation
/// job for each so the drain stops the runs they granted. Returns the agents that lost an
/// authorization.
///
/// Commands that narrow app, project, or channel access, remove a membership, or archive an
/// agent call it after their write, and the sync materialize hook calls it after an apply, so
/// both revoke the same rows. An archived agent records [`AGENT_ARCHIVED_REASON`]; anything
/// else records [`ACCESS_REDUCTION_REASON`]. Covered authorizations are kept, so a widening or
/// a repeated call revokes nothing. `now` is the revocation and job timestamp.
pub async fn revoke_exceeding_authorizations(
    connection: &mut SqliteConnection,
    now: &str,
) -> ChatResult<BTreeSet<String>> {
    let uncovered: Vec<(String, String, bool)> =
        sqlx::query_as(sqlx::AssertSqlSafe(exceeding_authorizations_sql()))
            .fetch_all(&mut *connection)
            .await
            .map_err(persistence_error)?;
    let mut agents = BTreeSet::new();
    for (authorization_id, agent_id, archived) in uncovered {
        let reason = if archived {
            AGENT_ARCHIVED_REASON
        } else {
            ACCESS_REDUCTION_REASON
        };
        if revoke_authorization(connection, &authorization_id, now, reason).await? > 0 {
            agents.insert(agent_id);
        }
    }
    Ok(agents)
}

/// SQL condition that holds when the push approval row `approval`, a table name or alias of the
/// enclosing query, has lapsed: its conversation's effective access is below ask (an unknown
/// level counts as below), its agent is no longer a present member of the conversation, or its
/// agent is archived.
fn lapsed_push_approval_sql(approval: &str) -> String {
    let at_least_ask = ChatAgentAccess::ALL
        .into_iter()
        .filter(|level| *level >= ChatAgentAccess::Ask)
        .map(|level| format!("'{}'", level.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let conversation = format!("{approval}.conversation_id");
    let agent = format!("{approval}.agent_id");
    let level = effective_access_level_sql(&conversation);
    let present = agent_present_sql(&conversation, &agent);
    format!(
        "({level} NOT IN ({at_least_ask})
          OR NOT {present}
          OR EXISTS (
              SELECT 1 FROM chat_participants lapsed_agent
              WHERE lapsed_agent.id = {agent} AND lapsed_agent.archived_at IS NOT NULL
          ))"
    )
}

/// Conversations that still hold an unrevoked push approval that has lapsed (see
/// [`lapsed_push_approval_sql`]).
async fn lapsed_push_approval_channels(
    connection: &mut SqliteConnection,
) -> ChatResult<BTreeSet<String>> {
    let lapsed = lapsed_push_approval_sql("approval");
    let conversations: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT DISTINCT approval.conversation_id FROM chat_push_approvals approval
         WHERE approval.revoked_at IS NULL AND {lapsed}
         ORDER BY approval.conversation_id"
    )))
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(conversations.into_iter().collect())
}

/// Revokes every unrevoked push approval that has lapsed, because its conversation's effective
/// access fell below ask, its agent left the conversation, or its agent was archived, and
/// returns the conversations that held them.
///
/// Push approvals replicate, so this runs outside a sync apply: after a command on this device
/// narrows app, project, or channel access, removes an agent's membership, or archives an
/// agent, and after an apply whose effects report [`ChatSyncEffects::lapsed_push_approvals`].
/// The revocation replicates like any other edit, and a repeated call revokes nothing. `now` is
/// the revocation timestamp.
pub async fn revoke_lapsed_push_approvals(
    connection: &mut SqliteConnection,
    now: &str,
) -> ChatResult<BTreeSet<String>> {
    // RETURNING cannot name a table alias, so the statement names the table itself.
    let lapsed = lapsed_push_approval_sql("chat_push_approvals");
    let conversations: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "UPDATE chat_push_approvals
         SET revoked_at = ?1, revision = revision + 1
         WHERE revoked_at IS NULL AND {lapsed}
         RETURNING conversation_id"
    )))
    .bind(now)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(conversations.into_iter().collect())
}

/// SQL expression for the rank of the access level `level`, an SQL expression, in the order
/// talk, read, ask, own, or `unknown` when it names no level.
fn access_rank_sql(level: &str, unknown: i64) -> String {
    let arms = ChatAgentAccess::ALL
        .into_iter()
        .enumerate()
        .map(|(rank, access)| format!("WHEN '{}' THEN {rank}", access.as_str()))
        .collect::<Vec<_>>()
        .join(" ");
    format!("(CASE {level} {arms} ELSE {unknown} END)")
}

/// SQL condition that holds when the remembered approval row `approval`, a table name or alias
/// of the enclosing query, has lapsed: its conversation's effective access ranks below the
/// access it was granted under (an unknown level on either side counts as below), its agent is
/// no longer a present member of the conversation, or its agent is archived.
pub(crate) fn lapsed_remembered_approval_sql(approval: &str) -> String {
    let conversation = format!("{approval}.conversation_id");
    let agent = format!("{approval}.agent_id");
    let effective = access_rank_sql(&effective_access_level_sql(&conversation), -1);
    let granted = access_rank_sql(&format!("{approval}.granted_access"), i64::MAX);
    let present = agent_present_sql(&conversation, &agent);
    format!(
        "({effective} < {granted}
          OR NOT {present}
          OR EXISTS (
              SELECT 1 FROM chat_participants lapsed_agent
              WHERE lapsed_agent.id = {agent} AND lapsed_agent.archived_at IS NOT NULL
          ))"
    )
}

/// Conversations that still hold an unrevoked remembered approval that has lapsed (see
/// [`lapsed_remembered_approval_sql`]).
async fn lapsed_remembered_approval_channels(
    connection: &mut SqliteConnection,
) -> ChatResult<BTreeSet<String>> {
    let lapsed = lapsed_remembered_approval_sql("approval");
    let conversations: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT DISTINCT approval.conversation_id FROM chat_remembered_approvals approval
         WHERE approval.revoked_at IS NULL AND {lapsed}
         ORDER BY approval.conversation_id"
    )))
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(conversations.into_iter().collect())
}

/// Revokes every unrevoked remembered approval that has lapsed, because its conversation's
/// effective access fell below the access it was granted under, its agent left the
/// conversation, or its agent was archived, and returns the conversations that held them.
///
/// Remembered approvals replicate, so this runs outside a sync apply, at the same points as
/// [`revoke_lapsed_push_approvals`] and after an apply whose effects report
/// [`ChatSyncEffects::lapsed_remembered_approvals`]. A repeated call revokes nothing. `now` is
/// the revocation timestamp.
pub async fn revoke_lapsed_remembered_approvals(
    connection: &mut SqliteConnection,
    now: &str,
) -> ChatResult<BTreeSet<String>> {
    // RETURNING cannot name a table alias, so the statement names the table itself.
    let lapsed = lapsed_remembered_approval_sql("chat_remembered_approvals");
    let conversations: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "UPDATE chat_remembered_approvals
         SET revoked_at = ?1, revision = revision + 1
         WHERE revoked_at IS NULL AND {lapsed}
         RETURNING conversation_id"
    )))
    .bind(now)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(conversations.into_iter().collect())
}

/// Follows the applied state of the work assignment `assignment_id` on this device.
///
/// A queued assignment is reported so its executing device starts it, and a started one so a
/// device that runs it without being its executing device stops its run. A settled assignment
/// ends the device's queued or claimed dispatch job, drops the execution targets kept for its
/// start and its pending replies, and revokes its active authorizations with their revocation
/// jobs, so a run this device executes stops when another device settled the work. A delegated
/// child that completed, asked for review, failed, or was cancelled is reported so its parent's
/// executing device posts its outcome. A deleted assignment, whose cascade removed its local
/// runs and authorizations, is reported so the caller stops the work those runs still execute.
/// Other states change nothing.
/// Repeating it for a settled assignment revokes nothing more.
async fn follow_assignment_state(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let row: Option<(String, String, bool)> = sqlx::query_as(
        "SELECT state, agent_id, parent_assignment_id IS NOT NULL
         FROM chat_work_assignments WHERE id = ?",
    )
    .bind(assignment_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some((state, agent_id, delegated)) = row else {
        // The delete cascaded away the assignment's runs and authorizations, so no revocation
        // job can name them; the report wakes the sweep that stops running work whose
        // authorization no longer exists.
        effects
            .removed_assignments
            .insert(assignment_id.to_string());
        return Ok(());
    };
    if delegated
        && matches!(
            state.as_str(),
            "completed" | "ready_for_review" | "failed" | "cancelled"
        )
    {
        effects.delegated_outcomes.insert(assignment_id.to_string());
    }
    match state.as_str() {
        "queued" => {
            effects.queued_assignments.insert(assignment_id.to_string());
        }
        "working" | "waiting_for_answer" | "waiting_for_approval" => {
            effects
                .started_assignments
                .insert(assignment_id.to_string());
        }
        "completed" | "failed" | "cancelled" => {
            let now = now();
            let job_state = if state == "cancelled" {
                ChatDispatchJobState::Cancelled
            } else {
                ChatDispatchJobState::Failed
            };
            let end_jobs = format!(
                "UPDATE chat_assignment_dispatch_jobs SET state = ?, updated_at = ?
                 WHERE assignment_id = ? AND state IN ({})",
                sources_sql(job_state)
            );
            let ended_jobs = sqlx::query(sqlx::AssertSqlSafe(end_jobs.as_str()))
                .bind(job_state.as_str())
                .bind(&now)
                .bind(assignment_id)
                .execute(&mut *connection)
                .await
                .map_err(persistence_error)?
                .rows_affected();
            let revoked = revoke_assignment_authorizations(
                connection,
                assignment_id,
                &now,
                ASSIGNMENT_SETTLED_REASON,
            )
            .await?;
            // A claim that has not authorized yet has nothing to revoke; ending its job stops
            // the launch, which refuses a settled assignment, and the report wakes the drain
            // for a run that started meanwhile.
            if ended_jobs > 0 || revoked > 0 {
                effects.revoked_agents.insert(agent_id);
                effects
                    .settled_assignments
                    .insert(assignment_id.to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

/// The current time on this device, as stored timestamps write it.
fn now() -> String {
    let now: DateTime<Utc> = SystemTime::now().into();
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Replaces the search row of a message with its current revision, or removes it when the
/// message is gone or has no current revision yet.
async fn rebuild_message_search(
    connection: &mut SqliteConnection,
    message_item_id: &str,
    effects: &mut ChatSyncEffects,
) -> ChatResult<()> {
    let removed =
        sqlx::query("DELETE FROM chat_communication_search_fts WHERE message_item_id = ?")
            .bind(message_item_id)
            .execute(&mut *connection)
            .await
            .map_err(persistence_error)?
            .rows_affected();
    let inserted = sqlx::query(
        "INSERT INTO chat_communication_search_fts (
             message_item_id, conversation_id, reply_thread_id,
             author_display_name, normalized_markdown
         )
         SELECT message.item_id, item.conversation_id, item.reply_thread_id,
                coalesce(participant.display_name, message.author_label_snapshot),
                revision.normalized_markdown
         FROM chat_communication_messages message
         JOIN chat_conversation_items item ON item.id = message.item_id
         LEFT JOIN chat_participants participant
           ON participant.id = message.author_participant_id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE message.item_id = ?",
    )
    .bind(message_item_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    if removed > 0 || inserted > 0 {
        effects.search_rebuilt.insert(message_item_id.to_string());
    }
    Ok(())
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat state derived from synced rows could not be saved",
        true,
    )
}

#[cfg(test)]
mod tests;
