//! Frozen assignment context and authorization snapshots.

mod authorization;
mod packing;

use authorization::{
    FolderGrant, ResolveExecutionTargetInput, capability_for_rank, capability_rank, folder_grants,
    read_assignment_access, read_channel_sources, read_referenced_execution_environments,
    read_referenced_folder_ids, read_thread_folder, require_granted_folder,
    resolve_execution_target,
};
pub(super) use authorization::{GrantedAccess, read_granted_access};
pub(super) use packing::{
    CHANNEL_NOTE_SOURCE_KIND, CONTEXT_PACKAGE_FORMAT_INSTRUCTIONS, SHORTENED_MARKER,
    asked_question_context_line, channel_note_removed_line, follow_up_context_line,
    message_agent_authored, record_delivery_kind,
};
use packing::{
    ContextSource, ContextSources, ContextTotals, count_value, pack_context_sources,
    read_channel_note_sources,
};
#[cfg(test)]
pub(super) use packing::{
    TestPackageRecord, test_package_records, test_package_records_with_notes,
};

use super::super::internal_mcp::generate_opaque_handle;
use super::common::{new_id, sha256_hex};
use super::delegation::{
    DelegationBound, DelegationCeiling, cancel_for_revoked_parent, clamp_approval,
    read_delegation_bound,
};
use super::provider_agents::DeviceProviders;
use super::{
    MAX_CHANNEL_CONTEXT_MESSAGES, MAX_THREAD_CONTEXT_REPLIES, i64_value, identifier_error,
    persistence_error, u64_value,
};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, Transaction};

/// The assignment whose context a freeze packs, with the thread, trigger, and agent it was
/// created for.
#[derive(Clone, Copy)]
pub(super) struct WorkToFreeze<'a> {
    pub(super) assignment_id: &'a ChatWorkAssignmentId,
    pub(super) reply_thread_id: &'a ChatReplyThreadId,
    pub(super) triggering_message_item_id: &'a ChatConversationItemId,
    pub(super) agent_id: &'a ChatParticipantId,
}

pub(super) async fn freeze_context_package(
    transaction: &mut Transaction<'_, Sqlite>,
    work: WorkToFreeze<'_>,
    explicit_execution_target: Option<&ChatExecutionTarget>,
    providers: &DeviceProviders,
    now: &UtcTimestamp,
) -> ChatResult<FreezeOutcome> {
    let WorkToFreeze {
        assignment_id,
        reply_thread_id,
        triggering_message_item_id,
        agent_id,
    } = work;
    let bound = match read_delegation_bound(transaction, assignment_id.as_str()).await? {
        DelegationBound::Revoked => {
            cancel_for_revoked_parent(transaction, assignment_id.as_str(), now).await?;
            return Ok(FreezeOutcome::ParentAuthorityRevoked);
        }
        DelegationBound::Undelegated => None,
        DelegationBound::Bound(ceiling) => Some(ceiling),
    };
    let thread_row = sqlx::query(
        "SELECT thread.conversation_id, thread.root_item_id, root_item.order_key AS root_order_key,
                channel.id AS channel_id, channel.project_id
         FROM chat_reply_threads thread
         JOIN chat_conversation_items root_item ON root_item.id = thread.root_item_id
         JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
         WHERE thread.id = ?",
    )
    .bind(reply_thread_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let conversation_id = ChatConversationId::new(
        thread_row
            .try_get::<String, _>("conversation_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let project_id: String = thread_row
        .try_get("project_id")
        .map_err(persistence_error)?;
    let channel_id: String = thread_row
        .try_get("channel_id")
        .map_err(persistence_error)?;
    let root_item_id = ChatConversationItemId::new(
        thread_row
            .try_get::<String, _>("root_item_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let access = read_assignment_access(
        transaction,
        &conversation_id,
        triggering_message_item_id,
        agent_id,
        providers,
    )
    .await?;
    // An agent reads its channel like a member who joined it: the whole thread and the
    // channel's recent top-level messages.
    let source_rows = sqlx::query(
        "SELECT item.id, revision.revision, revision.normalized_markdown,
                CASE
                  WHEN item.id = ? THEN 'trigger'
                  WHEN item.id = ? THEN 'thread_root'
                  ELSE 'thread_reply'
                END AS source_kind,
                coalesce(author.participant_kind = 'ai_agent', 0) AS agent_authored
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_communication_message_revisions_numbered revision
           ON revision.id = message.current_revision_id
         LEFT JOIN chat_participants author ON author.id = message.author_participant_id
         WHERE message.deleted_at IS NULL
           AND (item.id = ? OR item.reply_thread_id = ? OR item.id = ?)
         ORDER BY CASE WHEN item.id = ? THEN 0 WHEN item.id = ? THEN 1 ELSE 2 END,
                  item.order_key DESC, item.id DESC
         LIMIT ?",
    )
    .bind(triggering_message_item_id.as_str())
    .bind(root_item_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .bind(reply_thread_id.as_str())
    .bind(root_item_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .bind(root_item_id.as_str())
    .bind(i64::try_from(MAX_THREAD_CONTEXT_REPLIES + 2).unwrap_or(52))
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let channel_rows = sqlx::query(
        "SELECT item.id, revision.revision, revision.normalized_markdown
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_communication_message_revisions_numbered revision
           ON revision.id = message.current_revision_id
         WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
           AND item.id != ?
           AND message.deleted_at IS NULL
         ORDER BY item.order_key DESC, item.id DESC LIMIT ?",
    )
    .bind(conversation_id.as_str())
    .bind(root_item_id.as_str())
    .bind(i64::try_from(MAX_CHANNEL_CONTEXT_MESSAGES).unwrap_or(20))
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let reply_total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         WHERE item.reply_thread_id = ? AND item.id != ? AND message.deleted_at IS NULL",
    )
    .bind(reply_thread_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let channel_total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
           AND item.id != ? AND message.deleted_at IS NULL",
    )
    .bind(conversation_id.as_str())
    .bind(root_item_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let mut trigger = None;
    let mut root = None;
    let mut replies = Vec::new();
    // Rows arrive as trigger, root, then replies newest first.
    for row in source_rows {
        let source = ContextSource {
            id: row.try_get("id").map_err(persistence_error)?,
            kind: row.try_get("source_kind").map_err(persistence_error)?,
            revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
            text: row
                .try_get("normalized_markdown")
                .map_err(persistence_error)?,
            agent_authored: row.try_get("agent_authored").map_err(persistence_error)?,
            note: None,
        };
        match source.kind.as_str() {
            "trigger" => {
                trigger = Some(ContextSource {
                    text: super::model_choice::strip_model_command(&source.text),
                    ..source
                })
            }
            "thread_root" => root = Some(source),
            _ => replies.push(source),
        }
    }
    replies.truncate(MAX_THREAD_CONTEXT_REPLIES);
    replies.reverse();
    let mut channel_messages = Vec::with_capacity(channel_rows.len());
    for row in channel_rows.into_iter().rev() {
        channel_messages.push(ContextSource {
            id: row.try_get("id").map_err(persistence_error)?,
            kind: "channel_message".to_string(),
            revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
            text: row
                .try_get("normalized_markdown")
                .map_err(persistence_error)?,
            agent_authored: false,
            note: None,
        });
    }
    let notes = read_channel_note_sources(transaction, &channel_id).await?;
    let note_total = count_value(notes.len());
    let sources = ContextSources {
        notes,
        trigger,
        root,
        replies,
        channel_messages,
    };
    let packed = pack_context_sources(
        &sources,
        ContextTotals {
            replies: u64_value(reply_total)?,
            channel_messages: u64_value(channel_total)?,
            channel_notes: note_total,
        },
    )?;
    let serialized = &packed.serialized;
    let context_id = new_id("context")?;
    let context_hash = sha256_hex(serialized.as_bytes());
    sqlx::query(
        "INSERT INTO chat_assignment_context_packages
            (id, assignment_id, revision, triggering_message_item_id,
             serialized_text, serialized_bytes, excluded_thread_reply_count,
             excluded_channel_message_count, excluded_channel_note_count, sha256, created_at)
         VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&context_id)
    .bind(assignment_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .bind(serialized.as_str())
    .bind(i64::try_from(serialized.len()).unwrap_or(i64::MAX))
    .bind(i64_value(packed.excluded_replies)?)
    .bind(i64_value(packed.excluded_channel_messages)?)
    .bind(i64_value(packed.excluded_channel_notes)?)
    .bind(context_hash)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for (ordinal, included) in packed.included.iter().enumerate() {
        let source = included.source;
        sqlx::query(
            "INSERT OR IGNORE INTO chat_assignment_context_sources
                (context_package_id, source_kind, source_id, source_revision,
                 content_sha256, ordinal)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&context_id)
        .bind(&source.kind)
        .bind(&source.id)
        .bind(i64_value(source.revision)?)
        .bind(sha256_hex(included.sent_text.as_bytes()))
        .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    let grants = folder_grants(transaction, &access.channel).await?;
    let grants = match &bound {
        Some(ceiling) => bounded_grants(grants, ceiling)?,
        None => grants,
    };
    let reaches_folders = access.channel.resolved.folder_capability != ChatFolderCapability::None;
    // Under Talk the agent has no folder, so references to folders and environments in the
    // message do not select or authorize one.
    let mut referenced_folder_ids = if reaches_folders {
        read_referenced_folder_ids(transaction, triggering_message_item_id).await?
    } else {
        Vec::new()
    };
    let referenced_execution_environments = if reaches_folders {
        read_referenced_execution_environments(transaction, triggering_message_item_id).await?
    } else {
        Vec::new()
    };
    for working_folder_id in &referenced_folder_ids {
        require_granted_folder(&grants, working_folder_id)?;
    }
    if bound.is_some() {
        for grant in &grants {
            if !referenced_folder_ids.contains(&grant.working_folder_id) {
                referenced_folder_ids.push(grant.working_folder_id.clone());
            }
        }
    }
    if matches!(
        explicit_execution_target,
        Some(ChatExecutionTarget::Scratch { .. })
    ) && !access.channel.resolved.scratch_allowed
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "The agent's access in this channel does not include private scratch",
            true,
        ));
    }
    let thread_folder =
        read_thread_folder(transaction, reply_thread_id, assignment_id, &grants, now).await?;
    let requester_participant_id = bound
        .as_ref()
        .and_then(|ceiling| ceiling.requester_participant_id.clone())
        .unwrap_or_else(|| access.requester_participant_id.clone());
    let target = resolve_execution_target(
        transaction,
        ResolveExecutionTargetInput {
            reply_thread_id,
            destination_conversation_id: &conversation_id,
            agent_id,
            requester_participant_id: &requester_participant_id,
            explicit: explicit_execution_target,
            grants: &grants,
            referenced_folder_ids: &referenced_folder_ids,
            referenced_execution_environments: &referenced_execution_environments,
            thread_folder: thread_folder.as_ref(),
            now,
        },
    )
    .await?;
    let channel_sources = read_channel_sources(
        transaction,
        triggering_message_item_id,
        &conversation_id,
        &requester_participant_id,
        agent_id,
    )
    .await?;
    let mut authorized_folders = grants
        .iter()
        .filter(|grant| {
            referenced_folder_ids.contains(&grant.working_folder_id)
                || target.working_folder_id.as_deref() == Some(&grant.working_folder_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    authorized_folders.sort_by(|left, right| left.working_folder_id.cmp(&right.working_folder_id));
    let resolved = access.channel.resolved;
    let mut resolved_runtime_approval_policy = match (
        target.working_folder_id.as_deref(),
        target.scratch_generation_id.as_ref(),
    ) {
        (Some(working_folder_id), _) => grants
            .iter()
            .find(|grant| grant.working_folder_id == working_folder_id)
            .map_or(resolved.runtime_approval.as_str(), |grant| {
                grant.runtime_approval
            }),
        (None, Some(_)) => resolved.scratch_approval.as_str(),
        (None, None) => resolved.runtime_approval.as_str(),
    };
    let mut folder_runtime_approval_policies = authorized_folders
        .iter()
        .map(|grant| grant.runtime_approval)
        .collect::<Vec<_>>();
    if let Some(ceiling) = &bound {
        let target_rank = target
            .working_folder_id
            .as_deref()
            .and_then(|working_folder_id| ceiling.folders.get(working_folder_id))
            .map_or(ceiling.approval_rank, |folder| folder.approval_rank);
        resolved_runtime_approval_policy = clamp_approval(
            resolved_runtime_approval_policy,
            ceiling.approval_rank.min(target_rank),
        );
        for (grant, policy) in authorized_folders
            .iter()
            .zip(folder_runtime_approval_policies.iter_mut())
        {
            let rank = ceiling
                .folders
                .get(&grant.working_folder_id)
                .map_or(0, |folder| folder.approval_rank);
            *policy = clamp_approval(policy, rank);
        }
    }
    let instructions: String =
        sqlx::query_scalar("SELECT instructions FROM chat_work_assignments WHERE id = ?")
            .bind(assignment_id.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
    let scope_data = serde_json::json!({
        "destinationConversationId": conversation_id.as_str(),
        "requesterParticipantId": requester_participant_id.as_str(),
        "agentId": agent_id.as_str(),
        "accessLevel": access.channel.level.as_str(),
        "effectiveAccess": access.channel.effective_access_name(),
        "folderCapability": resolved.folder_capability.as_str(),
        "runtimeApproval": resolved.runtime_approval.as_str(),
        "instructionsSha256": sha256_hex(instructions.as_bytes()),
        "executionEnvironmentId": &target.execution_environment_id,
        "workingFolderId": &target.working_folder_id,
        "scratchGenerationId": &target.scratch_generation_id,
        "runtimeApprovalPolicy": &resolved_runtime_approval_policy,
        "channels": channel_sources.iter().map(|source| serde_json::json!({
            "referenceId": &source.message_reference_id,
            "conversationId": &source.conversation_id,
            "lowerOrderKey": &source.lower_order_key,
            "lowerItemId": &source.lower_item_id,
            "highOrderKey": &source.high_order_key,
            "highItemId": &source.high_item_id,
            "revisionCutoffId": &source.source_revision_cutoff_id,
            "audienceRevision": source.destination_audience_revision,
        })).collect::<Vec<_>>(),
        "folders": authorized_folders.iter().zip(&folder_runtime_approval_policies).map(|(grant, policy)| serde_json::json!({
            "workingFolderId": &grant.working_folder_id,
            "capability": &grant.capability,
            "executionTarget": target.working_folder_id.as_deref() == Some(&grant.working_folder_id),
            "runtimeApprovalPolicy": policy,
        })).collect::<Vec<_>>(),
    });
    let scope_digest = sha256_hex(
        serde_json::to_vec(&scope_data)
            .map_err(|_| {
                ChatError::new(ChatErrorCode::Internal, "encode authorization scope", true)
            })?
            .as_slice(),
    );
    let authorization_id =
        ChatAuthorizationRevisionId::new(new_id("authorization")?).map_err(identifier_error)?;
    sqlx::query(
        "INSERT INTO chat_assignment_authorization_revisions
            (id, assignment_id, revision, requester_participant_id,
             destination_conversation_id,
             execution_environment_id, working_folder_id,
             resolved_runtime_approval_policy, scope_digest, decision_state,
             reason, created_at)
         VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?, 'allowed', '', ?)",
    )
    .bind(authorization_id.as_str())
    .bind(assignment_id.as_str())
    .bind(requester_participant_id.as_str())
    .bind(conversation_id.as_str())
    .bind(&target.execution_environment_id)
    .bind(&target.working_folder_id)
    .bind(resolved_runtime_approval_policy)
    .bind(&scope_digest)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for source in &channel_sources {
        sqlx::query(
            "INSERT INTO chat_assignment_authorized_channel_sources
                (authorization_revision_id, source_handle, message_reference_id,
                 conversation_id, label_snapshot, lower_order_key, lower_item_id,
                 high_order_key, high_item_id, source_revision_cutoff_id,
                 destination_audience_revision, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(authorization_id.as_str())
        .bind(&source.source_handle)
        .bind(&source.message_reference_id)
        .bind(&source.conversation_id)
        .bind(&source.label_snapshot)
        .bind(&source.lower_order_key)
        .bind(&source.lower_item_id)
        .bind(&source.high_order_key)
        .bind(&source.high_item_id)
        .bind(&source.source_revision_cutoff_id)
        .bind(i64_value(source.destination_audience_revision)?)
        .bind(now.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    for (grant, runtime_approval_policy) in authorized_folders
        .iter()
        .zip(&folder_runtime_approval_policies)
    {
        sqlx::query(
            "INSERT INTO chat_assignment_authorized_folder_sources
                (authorization_revision_id, root_handle, working_folder_id,
                 capability, is_execution_target, created_at,
                 resolved_runtime_approval_policy)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(authorization_id.as_str())
        .bind(generate_opaque_handle("folder-root")?)
        .bind(&grant.working_folder_id)
        .bind(&grant.capability)
        .bind(target.working_folder_id.as_deref() == Some(&grant.working_folder_id))
        .bind(now.as_str())
        .bind(*runtime_approval_policy)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    let _ = (project_id, context_id);
    Ok(FreezeOutcome::Frozen)
}

/// What freezing an assignment's context did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FreezeOutcome {
    /// The context and authorization are frozen.
    Frozen,
    /// The assignment was delegated and an ancestor lost its authority, so it was cancelled
    /// instead.
    ParentAuthorityRevoked,
}

/// `grants` narrowed to a delegation ceiling: only ceiling folders, each at most at the
/// ceiling capability, without folders that end at no capability.
fn bounded_grants(
    grants: Vec<FolderGrant>,
    ceiling: &DelegationCeiling,
) -> ChatResult<Vec<FolderGrant>> {
    let mut bounded = Vec::with_capacity(grants.len());
    for mut grant in grants {
        let Some(limit) = ceiling.folders.get(&grant.working_folder_id) else {
            continue;
        };
        let rank = capability_rank(&grant.capability)?.min(limit.capability_rank);
        if rank == 0 {
            continue;
        }
        grant.capability = capability_for_rank(rank)?.to_owned();
        bounded.push(grant);
    }
    Ok(bounded)
}
