//! Authorization snapshots of frozen work: granted access, folder grants, the resolved execution
//! target, and the channel sources an assignment may disclose.

use super::super::super::internal_mcp::generate_opaque_handle;
use super::super::access::{
    ChannelAgentAccess, read_reached_folders, require_working_agent_access,
};
use super::super::provider_agents::DeviceProviders;
use super::super::{i64_value, identifier_error, persistence_error, u64_value};
use ganbaru_chat::coordination::acceptance::AcceptanceGrant;
use ganbaru_chat::project_defaults::{
    current_folder_environment_id, ensure_working_folder_chat_defaults,
};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, Transaction};

/// Who asked for an assignment and what the agent may do for it on this device.
pub(super) struct AssignmentAccess {
    pub(super) requester_participant_id: ChatParticipantId,
    pub(super) channel: ChannelAgentAccess,
}

/// The access an agent would hold if an assignment ran now on this device.
pub(in crate::chat::organization) struct GrantedAccess {
    /// Stored spelling of the effective access the device resolved for the channel.
    pub(in crate::chat::organization) access_level: &'static str,
    /// The access the agent runs under here, `None` when it cannot work on this device.
    pub(in crate::chat::organization) effective: Option<ChatEffectiveAgentAccess>,
    pub(in crate::chat::organization) grants: Vec<AcceptanceGrant>,
    /// Stored approval in the agent's scratch, empty when the access allows no scratch.
    pub(in crate::chat::organization) scratch_runtime_approval: &'static str,
    /// The provider installation that runs the agent here, empty when none is configured.
    pub(in crate::chat::organization) provider_instance_id: String,
}

/// Reads the access `agent_id` would hold running the message `triggering_message_item_id`
/// in `conversation_id`, resolved as a frozen context package resolves it.
pub(in crate::chat::organization) async fn read_granted_access(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    triggering_message_item_id: &ChatConversationItemId,
    agent_id: &ChatParticipantId,
    providers: &DeviceProviders,
) -> ChatResult<GrantedAccess> {
    let access = read_assignment_access(
        transaction,
        conversation_id,
        triggering_message_item_id,
        agent_id,
        providers,
    )
    .await?;
    let grants = folder_grants(transaction, &access.channel)
        .await?
        .into_iter()
        .map(|grant| AcceptanceGrant {
            working_folder_id: grant.working_folder_id,
            capability: grant.capability,
            runtime_approval: grant.runtime_approval.to_owned(),
        })
        .collect();
    let resolved = access.channel.resolved;
    Ok(GrantedAccess {
        access_level: access.channel.effective_access_name(),
        effective: resolved.effective,
        grants,
        scratch_runtime_approval: if resolved.scratch_allowed {
            resolved.scratch_approval.as_str()
        } else {
            ""
        },
        provider_instance_id: access
            .channel
            .provider_instance_id
            .as_ref()
            .map(|instance| instance.as_str().to_owned())
            .unwrap_or_default(),
    })
}

/// One project folder the agent's access reaches, with what the agent may do in it.
#[derive(Clone)]
pub(super) struct FolderGrant {
    pub(super) working_folder_id: String,
    pub(super) capability: String,
    /// Whether the folder is the project's primary folder.
    is_default: bool,
    pub(super) runtime_approval: &'static str,
}

pub(super) struct FrozenChannelSource {
    pub(super) source_handle: String,
    pub(super) message_reference_id: String,
    pub(super) conversation_id: String,
    pub(super) label_snapshot: String,
    pub(super) lower_order_key: String,
    pub(super) lower_item_id: String,
    pub(super) high_order_key: String,
    pub(super) high_item_id: String,
    pub(super) source_revision_cutoff_id: String,
    pub(super) destination_audience_revision: u64,
}

pub(super) struct ResolvedExecutionTarget {
    pub(super) execution_environment_id: Option<String>,
    pub(super) working_folder_id: Option<String>,
    pub(super) scratch_generation_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReferencedExecutionEnvironment {
    execution_environment_id: String,
    working_folder_id: String,
}

/// The folder earlier work in a reply thread ran in, with the environment it ran in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ThreadFolder {
    working_folder_id: String,
    execution_environment_id: String,
}

pub(super) struct ResolveExecutionTargetInput<'a> {
    pub(super) reply_thread_id: &'a ChatReplyThreadId,
    pub(super) destination_conversation_id: &'a ChatConversationId,
    pub(super) agent_id: &'a ChatParticipantId,
    pub(super) requester_participant_id: &'a ChatParticipantId,
    pub(super) explicit: Option<&'a ChatExecutionTarget>,
    pub(super) grants: &'a [FolderGrant],
    pub(super) referenced_folder_ids: &'a [String],
    pub(super) referenced_execution_environments: &'a [ReferencedExecutionEnvironment],
    /// The folder the reply thread already works in, when it still reaches one.
    pub(super) thread_folder: Option<&'a ThreadFolder>,
    pub(super) now: &'a UtcTimestamp,
}

struct ChannelDisclosureInput<'a> {
    destination_id: &'a ChatConversationId,
    destination_message_item_id: &'a ChatConversationItemId,
    source_id: &'a str,
    frozen_audience_revision: u64,
    requester_id: &'a ChatParticipantId,
    agent_id: &'a ChatParticipantId,
}

/// Resolves the access of the agent for the message that asks for work: the agent must be a
/// present member able to work on this device, and the message author a present member.
pub(super) async fn read_assignment_access(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    triggering_message_item_id: &ChatConversationItemId,
    agent_id: &ChatParticipantId,
    providers: &DeviceProviders,
) -> ChatResult<AssignmentAccess> {
    let not_allowed = || {
        ChatError::new(
            ChatErrorCode::Permission,
            "The agent cannot participate in this channel",
            true,
        )
    };
    let agent_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_participants WHERE id = ? AND archived_at IS NULL)",
    )
    .bind(agent_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if !agent_active {
        return Err(not_allowed());
    }
    let requester: String = sqlx::query_scalar(
        "SELECT message.author_participant_id
         FROM chat_communication_messages message
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = ?
          AND membership.participant_id = message.author_participant_id
          AND membership.removed_at IS NULL
         WHERE message.item_id = ?",
    )
    .bind(conversation_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?
    .ok_or_else(not_allowed)?;
    let channel = require_working_agent_access(
        transaction,
        conversation_id.as_str(),
        agent_id.as_str(),
        providers,
    )
    .await?;
    Ok(AssignmentAccess {
        requester_participant_id: ChatParticipantId::new(requester).map_err(identifier_error)?,
        channel,
    })
}

/// Lists the folders the agent's channel access reaches, each at the capability and approval
/// the access resolves to.
pub(super) async fn folder_grants(
    transaction: &mut Transaction<'_, Sqlite>,
    access: &ChannelAgentAccess,
) -> ChatResult<Vec<FolderGrant>> {
    let capability = access.resolved.folder_capability.as_str();
    let runtime_approval = access.resolved.runtime_approval.as_str();
    Ok(read_reached_folders(transaction, access)
        .await?
        .into_iter()
        .map(|folder| FolderGrant {
            working_folder_id: folder.working_folder_id,
            capability: capability.to_owned(),
            is_default: folder.is_primary,
            runtime_approval,
        })
        .collect())
}

/// Reads the folder the latest earlier work in the reply thread ran in, when the agent's
/// access still reaches it. Its environment is kept while it is still available, else the
/// folder's current folder environment is used.
pub(super) async fn read_thread_folder(
    transaction: &mut Transaction<'_, Sqlite>,
    reply_thread_id: &ChatReplyThreadId,
    assignment_id: &ChatWorkAssignmentId,
    grants: &[FolderGrant],
    now: &UtcTimestamp,
) -> ChatResult<Option<ThreadFolder>> {
    let row = sqlx::query(
        "SELECT authorization.working_folder_id, authorization.execution_environment_id
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.reply_thread_id = ? AND assignment.id != ?
           AND authorization.working_folder_id IS NOT NULL
           AND authorization.decision_state = 'allowed'
         ORDER BY authorization.created_at DESC, authorization.id DESC
         LIMIT 1",
    )
    .bind(reply_thread_id.as_str())
    .bind(assignment_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let working_folder_id: String = row
        .try_get("working_folder_id")
        .map_err(persistence_error)?;
    if !grants
        .iter()
        .any(|grant| grant.working_folder_id == working_folder_id)
    {
        return Ok(None);
    }
    let previous_environment: Option<String> = row
        .try_get("execution_environment_id")
        .map_err(persistence_error)?;
    let execution_environment_id = match previous_environment {
        Some(environment)
            if folder_environment_available(transaction, &working_folder_id, &environment)
                .await? =>
        {
            environment
        }
        _ => {
            let current = current_folder_environment_id(&working_folder_id);
            ensure_working_folder_chat_defaults(transaction, &working_folder_id, now).await?;
            current
        }
    };
    Ok(Some(ThreadFolder {
        working_folder_id,
        execution_environment_id,
    }))
}

pub(super) async fn read_referenced_folder_ids(
    transaction: &mut Transaction<'_, Sqlite>,
    triggering_message_item_id: &ChatConversationItemId,
) -> ChatResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT DISTINCT working_folder_id FROM (
           SELECT folder.working_folder_id
           FROM chat_communication_messages message
           JOIN chat_message_references reference
             ON reference.message_revision_id = message.current_revision_id
           JOIN chat_working_folder_reference_targets folder
             ON folder.reference_id = reference.id
           WHERE message.item_id = ?
           UNION ALL
           SELECT path.working_folder_id
           FROM chat_communication_messages message
           JOIN chat_message_references reference
             ON reference.message_revision_id = message.current_revision_id
           JOIN chat_workspace_path_reference_targets path
             ON path.reference_id = reference.id
           WHERE message.item_id = ?
           UNION ALL
           SELECT environment.working_folder_id
           FROM chat_communication_messages message
           JOIN chat_message_references reference
             ON reference.message_revision_id = message.current_revision_id
           JOIN chat_execution_environment_reference_targets target
             ON target.reference_id = reference.id
           JOIN chat_execution_environments environment
             ON environment.id = target.execution_environment_id
           WHERE message.item_id = ? AND environment.working_folder_id IS NOT NULL
         ) ORDER BY working_folder_id",
    )
    .bind(triggering_message_item_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)
}

pub(super) async fn read_referenced_execution_environments(
    transaction: &mut Transaction<'_, Sqlite>,
    triggering_message_item_id: &ChatConversationItemId,
) -> ChatResult<Vec<ReferencedExecutionEnvironment>> {
    let rows = sqlx::query(
        "SELECT DISTINCT target.execution_environment_id, environment.working_folder_id
         FROM chat_communication_messages message
         JOIN chat_message_references reference
           ON reference.message_revision_id = message.current_revision_id
         JOIN chat_execution_environment_reference_targets target
           ON target.reference_id = reference.id
         JOIN chat_execution_environments environment
           ON environment.id = target.execution_environment_id
         WHERE message.item_id = ? AND environment.working_folder_id IS NOT NULL
         ORDER BY target.execution_environment_id",
    )
    .bind(triggering_message_item_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(ReferencedExecutionEnvironment {
                execution_environment_id: row
                    .try_get("execution_environment_id")
                    .map_err(persistence_error)?,
                working_folder_id: row
                    .try_get("working_folder_id")
                    .map_err(persistence_error)?,
            })
        })
        .collect()
}

/// Chooses where the work runs: an explicitly chosen target, else the folder the reply thread
/// already works in, else a folder or environment the message names, else the project's
/// primary folder. Work whose access reaches no folder has no target.
pub(super) async fn resolve_execution_target(
    transaction: &mut Transaction<'_, Sqlite>,
    input: ResolveExecutionTargetInput<'_>,
) -> ChatResult<ResolvedExecutionTarget> {
    let ResolveExecutionTargetInput {
        reply_thread_id,
        destination_conversation_id,
        agent_id,
        requester_participant_id,
        explicit,
        grants,
        referenced_folder_ids,
        referenced_execution_environments,
        thread_folder,
        now,
    } = input;
    let referenced_execution_environment =
        unique_referenced_execution_environment(referenced_execution_environments)?;
    if let Some(explicit) = explicit {
        return match explicit {
            ChatExecutionTarget::WorkingFolder {
                working_folder_id,
                execution_environment_id,
            } => {
                require_granted_folder(grants, working_folder_id.as_str())?;
                verify_folder_environment(
                    transaction,
                    working_folder_id.as_str(),
                    execution_environment_id.as_str(),
                    now,
                )
                .await?;
                Ok(ResolvedExecutionTarget {
                    execution_environment_id: Some(execution_environment_id.as_str().to_string()),
                    working_folder_id: Some(working_folder_id.as_str().to_string()),
                    scratch_generation_id: None,
                })
            }
            ChatExecutionTarget::Scratch {
                scratch_scope_id,
                scratch_generation_id,
                execution_environment_id,
            } => {
                verify_explicit_scratch(
                    transaction,
                    reply_thread_id,
                    agent_id,
                    scratch_scope_id.as_str(),
                    scratch_generation_id.as_str(),
                    execution_environment_id.as_str(),
                )
                .await?;
                if !super::super::super::scratch::generation_constraints_hold_in_connection(
                    transaction,
                    scratch_generation_id.as_str(),
                    destination_conversation_id.as_str(),
                    agent_id.as_str(),
                    requester_participant_id.as_str(),
                    None,
                )
                .await?
                {
                    return Err(ChatError::new(
                        ChatErrorCode::Permission,
                        "The selected scratch target contains context that is no longer authorized",
                        true,
                    ));
                }
                Ok(ResolvedExecutionTarget {
                    execution_environment_id: Some(execution_environment_id.as_str().to_string()),
                    working_folder_id: None,
                    scratch_generation_id: Some(scratch_generation_id.as_str().to_string()),
                })
            }
        };
    }
    if let Some(thread_folder) = thread_folder {
        verify_folder_environment(
            transaction,
            &thread_folder.working_folder_id,
            &thread_folder.execution_environment_id,
            now,
        )
        .await?;
        return Ok(ResolvedExecutionTarget {
            execution_environment_id: Some(thread_folder.execution_environment_id.clone()),
            working_folder_id: Some(thread_folder.working_folder_id.clone()),
            scratch_generation_id: None,
        });
    }
    if let Some(environment) = referenced_execution_environment {
        let grant = require_granted_folder(grants, &environment.working_folder_id)?;
        if capability_can_select_native_target(&grant.capability) {
            verify_folder_environment(
                transaction,
                &environment.working_folder_id,
                &environment.execution_environment_id,
                now,
            )
            .await?;
            return Ok(ResolvedExecutionTarget {
                execution_environment_id: Some(environment.execution_environment_id.clone()),
                working_folder_id: Some(environment.working_folder_id.clone()),
                scratch_generation_id: None,
            });
        }
    }
    let mut inferred = referenced_folder_ids
        .iter()
        .filter(|working_folder_id| {
            grants.iter().any(|grant| {
                &grant.working_folder_id == *working_folder_id
                    && capability_can_select_native_target(&grant.capability)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    inferred.sort();
    inferred.dedup();
    let selected = if inferred.len() == 1 {
        inferred.pop()
    } else {
        grants
            .iter()
            .find(|grant| grant.is_default)
            .map(|grant| grant.working_folder_id.clone())
    };
    if let Some(working_folder_id) = selected {
        let execution_environment_id = current_folder_environment_id(&working_folder_id);
        verify_folder_environment(
            transaction,
            &working_folder_id,
            &execution_environment_id,
            now,
        )
        .await?;
        return Ok(ResolvedExecutionTarget {
            execution_environment_id: Some(execution_environment_id),
            working_folder_id: Some(working_folder_id),
            scratch_generation_id: None,
        });
    }
    Ok(ResolvedExecutionTarget {
        execution_environment_id: None,
        working_folder_id: None,
        scratch_generation_id: None,
    })
}

fn capability_can_select_native_target(capability: &str) -> bool {
    matches!(capability, "execute" | "publish")
}

fn unique_referenced_execution_environment(
    environments: &[ReferencedExecutionEnvironment],
) -> ChatResult<Option<&ReferencedExecutionEnvironment>> {
    match environments {
        [] => Ok(None),
        [environment] => Ok(Some(environment)),
        _ => Err(ChatError::validation(
            "references",
            "Reference only one execution environment per assignment",
        )),
    }
}

async fn verify_explicit_scratch(
    transaction: &mut Transaction<'_, Sqlite>,
    reply_thread_id: &ChatReplyThreadId,
    agent_id: &ChatParticipantId,
    scratch_scope_id: &str,
    scratch_generation_id: &str,
    execution_environment_id: &str,
) -> ChatResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_scratch_scopes scope
           JOIN chat_scratch_generations generation
             ON generation.scratch_scope_id = scope.id
           JOIN chat_execution_environments environment
             ON environment.scratch_generation_id = generation.id
           WHERE scope.id = ? AND scope.reply_thread_id = ? AND scope.agent_id = ?
             AND generation.id = ? AND generation.lifecycle_state = 'active'
             AND environment.id = ? AND environment.kind = 'scratch'
             AND environment.lifecycle_state = 'available'
             AND scope.removed_at IS NULL AND generation.removed_at IS NULL
             AND environment.archived_at IS NULL
         )",
    )
    .bind(scratch_scope_id)
    .bind(reply_thread_id.as_str())
    .bind(agent_id.as_str())
    .bind(scratch_generation_id)
    .bind(execution_environment_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if valid {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "The selected scratch target is unavailable",
            true,
        ))
    }
}

/// Verifies that `execution_environment_id` is an available environment of the working folder,
/// first creating the folder's Chat defaults when the id is its current-folder environment.
async fn verify_folder_environment(
    transaction: &mut Transaction<'_, Sqlite>,
    working_folder_id: &str,
    execution_environment_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    if execution_environment_id == current_folder_environment_id(working_folder_id) {
        ensure_working_folder_chat_defaults(transaction, working_folder_id, now).await?;
    }
    if folder_environment_available(transaction, working_folder_id, execution_environment_id)
        .await?
    {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "The selected execution target is unavailable",
            true,
        ))
    }
}

/// Whether `execution_environment_id` is an available, non-scratch environment of the
/// working folder.
async fn folder_environment_available(
    transaction: &mut Transaction<'_, Sqlite>,
    working_folder_id: &str,
    execution_environment_id: &str,
) -> ChatResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_execution_environments environment
           JOIN project_working_folders folder ON folder.id = environment.working_folder_id
           WHERE environment.id = ? AND environment.working_folder_id = ?
             AND environment.scratch_generation_id IS NULL
             AND environment.lifecycle_state = 'available'
             AND environment.archived_at IS NULL
             AND folder.archived_at IS NULL
         )",
    )
    .bind(execution_environment_id)
    .bind(working_folder_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)
}

pub(super) fn require_granted_folder<'a>(
    grants: &'a [FolderGrant],
    working_folder_id: &str,
) -> ChatResult<&'a FolderGrant> {
    grants
        .iter()
        .find(|grant| grant.working_folder_id == working_folder_id)
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Permission,
                "The selected folder is not authorized in this channel",
                true,
            )
        })
}

pub(super) async fn read_channel_sources(
    transaction: &mut Transaction<'_, Sqlite>,
    triggering_message_item_id: &ChatConversationItemId,
    destination_conversation_id: &ChatConversationId,
    requester_id: &ChatParticipantId,
    agent_id: &ChatParticipantId,
) -> ChatResult<Vec<FrozenChannelSource>> {
    let rows = sqlx::query(
        "SELECT reference.id AS reference_id, reference.label_snapshot,
                target.source_conversation_id, target.source_lower_order_key,
                target.source_lower_item_id, target.source_high_order_key,
                target.source_high_item_id, target.source_revision_cutoff_id,
                audience.revision AS destination_audience_revision
         FROM chat_communication_messages message
         JOIN chat_message_references reference
           ON reference.message_revision_id = message.current_revision_id
          AND reference.reference_kind = 'channel'
         JOIN chat_channel_reference_targets target ON target.reference_id = reference.id
         JOIN chat_conversation_audience_state audience
           ON audience.conversation_id = target.destination_conversation_id
         WHERE message.item_id = ? AND target.destination_conversation_id = ?
           AND target.source_high_order_key IS NOT NULL
           AND target.source_revision_cutoff_id IS NOT NULL
         ORDER BY reference.ordinal",
    )
    .bind(triggering_message_item_id.as_str())
    .bind(destination_conversation_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let conversation_id: String = row
            .try_get("source_conversation_id")
            .map_err(persistence_error)?;
        let lower_order_key: String = row
            .try_get("source_lower_order_key")
            .map_err(persistence_error)?;
        let destination_audience_revision = u64_value(
            row.try_get("destination_audience_revision")
                .map_err(persistence_error)?,
        )?;
        verify_channel_disclosure(
            transaction,
            ChannelDisclosureInput {
                destination_id: destination_conversation_id,
                destination_message_item_id: triggering_message_item_id,
                source_id: &conversation_id,
                frozen_audience_revision: destination_audience_revision,
                requester_id,
                agent_id,
            },
        )
        .await?;
        result.push(FrozenChannelSource {
            source_handle: generate_opaque_handle("channel-source")?,
            message_reference_id: row.try_get("reference_id").map_err(persistence_error)?,
            conversation_id,
            label_snapshot: row.try_get("label_snapshot").map_err(persistence_error)?,
            lower_order_key,
            lower_item_id: row
                .try_get("source_lower_item_id")
                .map_err(persistence_error)?,
            high_order_key: row
                .try_get("source_high_order_key")
                .map_err(persistence_error)?,
            high_item_id: row
                .try_get("source_high_item_id")
                .map_err(persistence_error)?,
            source_revision_cutoff_id: row
                .try_get("source_revision_cutoff_id")
                .map_err(persistence_error)?,
            destination_audience_revision,
        });
    }
    Ok(result)
}

/// Verifies that a channel the message references may be disclosed in the destination: the
/// requester and the agent are present members of the source, and every present member of the
/// destination is a present member of the source, so the reference shows nobody anything new.
/// Agents read a channel like members who joined it, so membership alone decides.
async fn verify_channel_disclosure(
    transaction: &mut Transaction<'_, Sqlite>,
    input: ChannelDisclosureInput<'_>,
) -> ChatResult<()> {
    let ChannelDisclosureInput {
        destination_id,
        destination_message_item_id,
        source_id,
        frozen_audience_revision,
        requester_id,
        agent_id,
    } = input;
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM chat_conversation_audience_state audience
           JOIN chat_conversation_items destination_item
             ON destination_item.id = ?
            AND destination_item.conversation_id = audience.conversation_id
           JOIN chat_conversation_memberships requester
             ON requester.conversation_id = ? AND requester.participant_id = ?
            AND requester.removed_at IS NULL
           JOIN chat_participants requester_participant
             ON requester_participant.id = requester.participant_id
           JOIN chat_conversation_memberships agent
             ON agent.conversation_id = ? AND agent.participant_id = ?
            AND agent.removed_at IS NULL
           JOIN chat_agent_channel_memberships agent_access
             ON agent_access.conversation_id = agent.conversation_id
            AND agent_access.agent_id = agent.participant_id
            AND agent_access.removed_at IS NULL
           WHERE audience.conversation_id = ? AND audience.revision >= ?
             AND (
               requester_participant.participant_kind != 'ai_agent'
               OR EXISTS (
                 SELECT 1
                 FROM chat_agent_channel_memberships requester_access
                 WHERE requester_access.conversation_id = ?
                   AND requester_access.agent_id = ?
                   AND requester_access.removed_at IS NULL
               )
             )
             AND NOT EXISTS (
               SELECT 1
               FROM chat_conversation_memberships destination_member
               JOIN chat_participants participant
                 ON participant.id = destination_member.participant_id
               WHERE destination_member.conversation_id = ?
                 AND destination_member.removed_at IS NULL
                 AND (
                   participant.participant_kind != 'ai_agent'
                   OR EXISTS (
                     SELECT 1
                     FROM chat_agent_channel_memberships destination_ai
                     WHERE destination_ai.conversation_id = ?
                       AND destination_ai.agent_id = destination_member.participant_id
                       AND destination_ai.removed_at IS NULL
                   )
                 )
                 AND (
                   NOT EXISTS (
                     SELECT 1 FROM chat_conversation_memberships source_member
                     WHERE source_member.conversation_id = ?
                       AND source_member.participant_id = destination_member.participant_id
                       AND source_member.removed_at IS NULL
                   )
                   OR (
                     participant.participant_kind = 'ai_agent'
                     AND NOT EXISTS (
                       SELECT 1
                       FROM chat_agent_channel_memberships source_ai
                       WHERE source_ai.conversation_id = ?
                         AND source_ai.agent_id = destination_member.participant_id
                         AND source_ai.removed_at IS NULL
                     )
                   )
                 )
             )
         )",
    )
    .bind(destination_message_item_id.as_str())
    .bind(source_id)
    .bind(requester_id.as_str())
    .bind(source_id)
    .bind(agent_id.as_str())
    .bind(destination_id.as_str())
    .bind(i64_value(frozen_audience_revision)?)
    .bind(source_id)
    .bind(requester_id.as_str())
    .bind(destination_id.as_str())
    .bind(destination_id.as_str())
    .bind(source_id)
    .bind(source_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if valid {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "The referenced channel cannot be disclosed to this audience",
            true,
        ))
    }
}

pub(super) fn capability_rank(value: &str) -> ChatResult<u8> {
    match value {
        "none" => Ok(0),
        "read" => Ok(1),
        "edit" => Ok(2),
        "execute" => Ok(3),
        "publish" => Ok(4),
        _ => Err(ChatError::new(
            ChatErrorCode::Persistence,
            "A stored folder capability is invalid",
            false,
        )),
    }
}

pub(super) fn capability_for_rank(rank: u8) -> ChatResult<&'static str> {
    match rank {
        0 => Ok("none"),
        1 => Ok("read"),
        2 => Ok("edit"),
        3 => Ok("execute"),
        4 => Ok("publish"),
        _ => Err(ChatError::new(
            ChatErrorCode::Persistence,
            "A stored folder capability rank is invalid",
            false,
        )),
    }
}

#[cfg(test)]
mod target_inference_tests {
    use super::{
        ChatErrorCode, ReferencedExecutionEnvironment, capability_can_select_native_target,
        unique_referenced_execution_environment,
    };

    #[test]
    fn only_command_capabilities_can_change_the_native_target() {
        assert!(!capability_can_select_native_target("read"));
        assert!(!capability_can_select_native_target("edit"));
        assert!(capability_can_select_native_target("execute"));
        assert!(capability_can_select_native_target("publish"));
    }

    #[test]
    fn conflicting_execution_environment_references_are_rejected() {
        let environment = |id: &str| ReferencedExecutionEnvironment {
            execution_environment_id: id.to_string(),
            working_folder_id: "folder:test".to_string(),
        };
        assert!(
            unique_referenced_execution_environment(&[])
                .unwrap()
                .is_none()
        );
        assert_eq!(
            unique_referenced_execution_environment(&[environment("environment:one")])
                .unwrap()
                .unwrap()
                .execution_environment_id,
            "environment:one"
        );
        let error = unique_referenced_execution_environment(&[
            environment("environment:one"),
            environment("environment:two"),
        ])
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
    }
}
