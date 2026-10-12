//! Frozen assignment context and authorization snapshots.

use super::super::internal_mcp::generate_opaque_handle;
use super::common::{MAX_MESSAGE_BYTES, new_id, sha256_hex, truncate_utf8};
use super::{
    MAX_CHANNEL_CONTEXT_MESSAGES, MAX_THREAD_CONTEXT_REPLIES, i64_value, identifier_error,
    persistence_error, u64_value,
};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, Transaction};

pub(super) async fn freeze_context_package(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    reply_thread_id: &ChatReplyThreadId,
    triggering_message_item_id: &ChatConversationItemId,
    teammate_id: &ChatParticipantId,
    explicit_execution_target: Option<&ChatExecutionTarget>,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let thread_row = sqlx::query(
        "SELECT thread.conversation_id, thread.root_item_id, channel.project_id
         FROM chat_reply_threads thread
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
        teammate_id,
    )
    .await?;
    let source_rows = sqlx::query(
        "SELECT item.id, item.ordinal, revision.revision, revision.normalized_markdown,
                CASE
                  WHEN item.id = ? THEN 'trigger'
                  WHEN item.id = ? THEN 'thread_root'
                  ELSE 'thread_reply'
                END AS source_kind
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE message.deleted_at IS NULL
           AND (item.reply_thread_id = ? OR item.id IN (?, ?))
         ORDER BY CASE WHEN item.id = ? THEN 0 ELSE 1 END, item.ordinal DESC
         LIMIT ?",
    )
    .bind(triggering_message_item_id.as_str())
    .bind(root_item_id.as_str())
    .bind(reply_thread_id.as_str())
    .bind(root_item_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .bind(i64::try_from(MAX_THREAD_CONTEXT_REPLIES + 2).unwrap_or(52))
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let channel_rows = if access.read_history {
        sqlx::query(
            "SELECT item.id, item.ordinal, revision.revision, revision.normalized_markdown
         FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         JOIN chat_communication_message_revisions revision
           ON revision.id = message.current_revision_id
         WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
           AND item.id != ?
           AND message.deleted_at IS NULL
           AND (? IS NULL OR item.ordinal >= ?)
         ORDER BY item.ordinal DESC LIMIT ?",
        )
        .bind(conversation_id.as_str())
        .bind(root_item_id.as_str())
        .bind(access.history_from_ordinal.map(i64_value).transpose()?)
        .bind(access.history_from_ordinal.map(i64_value).transpose()?)
        .bind(i64::try_from(MAX_CHANNEL_CONTEXT_MESSAGES).unwrap_or(20))
        .fetch_all(&mut **transaction)
        .await
        .map_err(persistence_error)?
    } else {
        Vec::new()
    };
    let thread_total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_conversation_items item
         JOIN chat_communication_messages message ON message.item_id = item.id
         WHERE item.reply_thread_id = ? AND message.deleted_at IS NULL",
    )
    .bind(reply_thread_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let channel_total: i64 = if access.read_history {
        sqlx::query_scalar(
            "SELECT count(*) FROM chat_conversation_items item
             JOIN chat_communication_messages message ON message.item_id = item.id
             WHERE item.conversation_id = ? AND item.reply_thread_id IS NULL
               AND item.id != ? AND message.deleted_at IS NULL
               AND (? IS NULL OR item.ordinal >= ?)",
        )
        .bind(conversation_id.as_str())
        .bind(root_item_id.as_str())
        .bind(access.history_from_ordinal.map(i64_value).transpose()?)
        .bind(access.history_from_ordinal.map(i64_value).transpose()?)
        .fetch_one(&mut **transaction)
        .await
        .map_err(persistence_error)?
    } else {
        0
    };
    let mut sources = Vec::new();
    for row in source_rows.into_iter().rev() {
        sources.push(ContextSource {
            id: row.try_get("id").map_err(persistence_error)?,
            kind: row.try_get("source_kind").map_err(persistence_error)?,
            revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
            text: row
                .try_get("normalized_markdown")
                .map_err(persistence_error)?,
        });
    }
    for row in channel_rows.into_iter().rev() {
        sources.push(ContextSource {
            id: row.try_get("id").map_err(persistence_error)?,
            kind: "channel_message".to_string(),
            revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
            text: row
                .try_get("normalized_markdown")
                .map_err(persistence_error)?,
        });
    }
    let mut serialized = String::new();
    for source in &sources {
        let line = format!("[{}:{}]\n{}\n\n", source.kind, source.id, source.text);
        if serialized.len() + line.len() > MAX_MESSAGE_BYTES {
            if source.id == triggering_message_item_id.as_str() {
                serialized = truncate_utf8(&line, MAX_MESSAGE_BYTES).to_string();
            }
            continue;
        }
        serialized.push_str(&line);
    }
    let context_id = new_id("context");
    let context_hash = sha256_hex(serialized.as_bytes());
    sqlx::query(
        "INSERT INTO chat_assignment_context_packages
            (id, assignment_id, revision, triggering_message_item_id,
             serialized_text, serialized_bytes, excluded_thread_reply_count,
             excluded_channel_message_count, sha256, created_at)
         VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&context_id)
    .bind(assignment_id.as_str())
    .bind(triggering_message_item_id.as_str())
    .bind(&serialized)
    .bind(i64::try_from(serialized.len()).unwrap_or(i64::MAX))
    .bind((thread_total - i64::try_from(MAX_THREAD_CONTEXT_REPLIES).unwrap_or(50)).max(0))
    .bind((channel_total - i64::try_from(MAX_CHANNEL_CONTEXT_MESSAGES).unwrap_or(20)).max(0))
    .bind(context_hash)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for (ordinal, source) in sources.iter().enumerate() {
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
        .bind(sha256_hex(source.text.as_bytes()))
        .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    let grants = read_folder_grants(
        transaction,
        &conversation_id,
        teammate_id,
        &access.maximum_folder_capability,
    )
    .await?;
    let referenced_folder_ids =
        read_referenced_folder_ids(transaction, triggering_message_item_id).await?;
    let referenced_execution_environments =
        read_referenced_execution_environments(transaction, triggering_message_item_id).await?;
    for working_folder_id in &referenced_folder_ids {
        require_granted_folder(&grants, working_folder_id)?;
    }
    let target = resolve_execution_target(
        transaction,
        ResolveExecutionTargetInput {
            reply_thread_id,
            destination_conversation_id: &conversation_id,
            teammate_id,
            requester_participant_id: &access.requester_participant_id,
            explicit: explicit_execution_target,
            grants: &grants,
            referenced_folder_ids: &referenced_folder_ids,
            referenced_execution_environments: &referenced_execution_environments,
        },
    )
    .await?;
    let channel_sources = read_channel_sources(
        transaction,
        triggering_message_item_id,
        &conversation_id,
        &access.requester_participant_id,
        teammate_id,
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
    let resolved_runtime_approval_policy = target
        .working_folder_id
        .as_deref()
        .and_then(|working_folder_id| {
            grants
                .iter()
                .find(|grant| grant.working_folder_id == working_folder_id)
                .and_then(|grant| grant.runtime_approval_policy.clone())
        })
        .or_else(|| {
            target
                .scratch_generation_id
                .as_ref()
                .and(access.scratch_runtime_approval_policy.clone())
        })
        .or_else(|| access.channel_runtime_approval_policy.clone())
        .unwrap_or_else(|| access.teammate_runtime_approval_policy.clone());
    let scope_data = serde_json::json!({
        "destinationConversationId": conversation_id.as_str(),
        "requesterParticipantId": access.requester_participant_id.as_str(),
        "teammateId": teammate_id.as_str(),
        "teammateAccessRevision": access.access_revision,
        "teammatePolicyRevisionId": access.policy_revision_id.as_str(),
        "accessProfileRevisionId": access.access_profile_revision_id.as_str(),
        "executionEnvironmentId": &target.execution_environment_id,
        "workingFolderId": &target.working_folder_id,
        "scratchGenerationId": &target.scratch_generation_id,
        "runtimeApprovalPolicy": &resolved_runtime_approval_policy,
        "channels": channel_sources.iter().map(|source| serde_json::json!({
            "referenceId": &source.message_reference_id,
            "conversationId": &source.conversation_id,
            "lowerOrdinal": source.lower_ordinal,
            "highOrdinal": source.high_ordinal,
            "revisionCutoffId": &source.source_revision_cutoff_id,
            "audienceRevision": source.destination_audience_revision,
        })).collect::<Vec<_>>(),
        "folders": authorized_folders.iter().map(|grant| serde_json::json!({
            "workingFolderId": &grant.working_folder_id,
            "capability": &grant.capability,
            "executionTarget": target.working_folder_id.as_deref() == Some(&grant.working_folder_id),
            "runtimeApprovalPolicy": grant.runtime_approval_policy.as_ref()
                .or(access.channel_runtime_approval_policy.as_ref())
                .unwrap_or(&access.teammate_runtime_approval_policy),
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
        ChatAuthorizationRevisionId::new(new_id("authorization")).map_err(identifier_error)?;
    sqlx::query(
        "INSERT INTO chat_assignment_authorization_revisions
            (id, assignment_id, revision, requester_participant_id,
             teammate_policy_revision_id, teammate_access_revision,
             destination_conversation_id, access_profile_revision_id,
             execution_environment_id, working_folder_id,
             resolved_runtime_approval_policy, scope_digest, decision_state,
             reason, created_at)
         VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'allowed', '', ?)",
    )
    .bind(authorization_id.as_str())
    .bind(assignment_id.as_str())
    .bind(access.requester_participant_id.as_str())
    .bind(access.policy_revision_id.as_str())
    .bind(i64_value(access.access_revision)?)
    .bind(conversation_id.as_str())
    .bind(access.access_profile_revision_id.as_str())
    .bind(&target.execution_environment_id)
    .bind(&target.working_folder_id)
    .bind(&resolved_runtime_approval_policy)
    .bind(&scope_digest)
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for source in &channel_sources {
        sqlx::query(
            "INSERT INTO chat_assignment_authorized_channel_sources
                (authorization_revision_id, source_handle, message_reference_id,
                 conversation_id, label_snapshot, lower_ordinal, high_ordinal,
                 source_revision_cutoff_id, destination_audience_revision, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(authorization_id.as_str())
        .bind(&source.source_handle)
        .bind(&source.message_reference_id)
        .bind(&source.conversation_id)
        .bind(&source.label_snapshot)
        .bind(i64_value(source.lower_ordinal)?)
        .bind(i64_value(source.high_ordinal)?)
        .bind(&source.source_revision_cutoff_id)
        .bind(i64_value(source.destination_audience_revision)?)
        .bind(now.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    for grant in &authorized_folders {
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
        .bind(
            grant
                .runtime_approval_policy
                .as_ref()
                .or(access.channel_runtime_approval_policy.as_ref())
                .unwrap_or(&access.teammate_runtime_approval_policy),
        )
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    let _ = (project_id, context_id);
    Ok(())
}

struct ContextSource {
    id: String,
    kind: String,
    revision: u64,
    text: String,
}

struct AssignmentAccess {
    requester_participant_id: ChatParticipantId,
    policy_revision_id: ChatTeammatePolicyRevisionId,
    access_revision: u64,
    access_profile_revision_id: ChatAccessProfileRevisionId,
    maximum_folder_capability: String,
    read_history: bool,
    history_from_ordinal: Option<u64>,
    channel_runtime_approval_policy: Option<String>,
    scratch_runtime_approval_policy: Option<String>,
    teammate_runtime_approval_policy: String,
}

#[derive(Clone)]
struct FolderGrant {
    working_folder_id: String,
    capability: String,
    is_default: bool,
    runtime_approval_policy: Option<String>,
}

struct FrozenChannelSource {
    source_handle: String,
    message_reference_id: String,
    conversation_id: String,
    label_snapshot: String,
    lower_ordinal: u64,
    high_ordinal: u64,
    source_revision_cutoff_id: String,
    destination_audience_revision: u64,
}

struct ResolvedExecutionTarget {
    execution_environment_id: Option<String>,
    working_folder_id: Option<String>,
    scratch_generation_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReferencedExecutionEnvironment {
    execution_environment_id: String,
    working_folder_id: String,
}

struct ResolveExecutionTargetInput<'a> {
    reply_thread_id: &'a ChatReplyThreadId,
    destination_conversation_id: &'a ChatConversationId,
    teammate_id: &'a ChatParticipantId,
    requester_participant_id: &'a ChatParticipantId,
    explicit: Option<&'a ChatExecutionTarget>,
    grants: &'a [FolderGrant],
    referenced_folder_ids: &'a [String],
    referenced_execution_environments: &'a [ReferencedExecutionEnvironment],
}

struct ChannelDisclosureInput<'a> {
    destination_id: &'a ChatConversationId,
    destination_message_item_id: &'a ChatConversationItemId,
    source_id: &'a str,
    lower_ordinal: u64,
    frozen_audience_revision: u64,
    requester_id: &'a ChatParticipantId,
    teammate_id: &'a ChatParticipantId,
}

async fn read_assignment_access(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    triggering_message_item_id: &ChatConversationItemId,
    teammate_id: &ChatParticipantId,
) -> ChatResult<AssignmentAccess> {
    let row = sqlx::query(
        "SELECT message.author_participant_id AS requester_participant_id,
                policy.id AS policy_revision_id, access_state.access_revision,
                profile_revision.id AS access_profile_revision_id,
                profile_revision.maximum_folder_capability,
                profile_revision.default_read_history,
                profile_revision.default_history_boundary,
                channel_access.read_history, channel_access.history_boundary,
                channel_access.read_history_inherits_profile,
                channel_access.participate_inherits_profile,
                channel_access.history_boundary_inherits_profile,
                channel_access.history_from_ordinal,
                channel_access.runtime_approval_policy AS channel_runtime_approval_policy,
                channel_access.scratch_runtime_approval_policy,
                access_state.runtime_approval_policy AS teammate_runtime_approval_policy
         FROM chat_conversation_memberships membership
         JOIN chat_teammate_channel_memberships channel_access
           ON channel_access.conversation_id = membership.conversation_id
          AND channel_access.teammate_id = membership.participant_id
         JOIN chat_teammate_access_state access_state
           ON access_state.teammate_id = membership.participant_id
          AND access_state.access_revision >= 1
         JOIN chat_access_profiles profile
           ON profile.id = channel_access.access_profile_id
          AND profile.archived_at IS NULL
         JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
          AND profile_revision.default_participate = 1
         JOIN chat_teammates teammate
           ON teammate.participant_id = membership.participant_id
         JOIN chat_teammate_policy_revisions policy
           ON policy.teammate_id = teammate.participant_id
          AND policy.revision = teammate.latest_policy_revision
         JOIN chat_communication_messages message ON message.item_id = ?
         JOIN chat_conversation_memberships requester_membership
           ON requester_membership.conversation_id = membership.conversation_id
          AND requester_membership.participant_id = message.author_participant_id
          AND requester_membership.removed_at IS NULL
         JOIN chat_participants participant ON participant.id = membership.participant_id
         WHERE membership.conversation_id = ?
          AND membership.participant_id = ?
          AND membership.removed_at IS NULL
           AND CASE
             WHEN channel_access.participate_inherits_profile = 1
               THEN profile_revision.default_participate
             ELSE channel_access.participate AND profile_revision.default_participate
           END = 1
           AND participant.archived_at IS NULL",
    )
    .bind(triggering_message_item_id.as_str())
    .bind(conversation_id.as_str())
    .bind(teammate_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Permission,
            "The teammate cannot participate in this channel",
            true,
        )
    })?;
    let history_from_ordinal = row
        .try_get::<Option<i64>, _>("history_from_ordinal")
        .map_err(persistence_error)?
        .map(u64_value)
        .transpose()?;
    let profile_read_history = row
        .try_get::<i64, _>("default_read_history")
        .map_err(persistence_error)?
        == 1;
    let read_history = if row
        .try_get::<i64, _>("read_history_inherits_profile")
        .map_err(persistence_error)?
        == 1
    {
        profile_read_history
    } else {
        row.try_get::<i64, _>("read_history")
            .map_err(persistence_error)?
            == 1
            && profile_read_history
    };
    let stored_history_boundary: String =
        row.try_get("history_boundary").map_err(persistence_error)?;
    let profile_history_boundary: String = row
        .try_get("default_history_boundary")
        .map_err(persistence_error)?;
    let history_inherits = row
        .try_get::<i64, _>("history_boundary_inherits_profile")
        .map_err(persistence_error)?
        == 1;
    let effective_history_from_ordinal = if profile_history_boundary == "from_grant"
        || (!history_inherits && stored_history_boundary == "from_grant")
    {
        history_from_ordinal
    } else {
        None
    };
    Ok(AssignmentAccess {
        requester_participant_id: ChatParticipantId::new(
            row.try_get::<String, _>("requester_participant_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        policy_revision_id: ChatTeammatePolicyRevisionId::new(
            row.try_get::<String, _>("policy_revision_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        access_revision: u64_value(row.try_get("access_revision").map_err(persistence_error)?)?,
        access_profile_revision_id: ChatAccessProfileRevisionId::new(
            row.try_get::<String, _>("access_profile_revision_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        maximum_folder_capability: row
            .try_get("maximum_folder_capability")
            .map_err(persistence_error)?,
        read_history,
        history_from_ordinal: effective_history_from_ordinal,
        channel_runtime_approval_policy: row
            .try_get("channel_runtime_approval_policy")
            .map_err(persistence_error)?,
        scratch_runtime_approval_policy: row
            .try_get("scratch_runtime_approval_policy")
            .map_err(persistence_error)?,
        teammate_runtime_approval_policy: row
            .try_get("teammate_runtime_approval_policy")
            .map_err(persistence_error)?,
    })
}

async fn read_folder_grants(
    transaction: &mut Transaction<'_, Sqlite>,
    conversation_id: &ChatConversationId,
    teammate_id: &ChatParticipantId,
    maximum_capability: &str,
) -> ChatResult<Vec<FolderGrant>> {
    let rows = sqlx::query(
        "SELECT working_folder_id, capability, capability_inherits_profile,
                is_default, runtime_approval_policy
         FROM chat_teammate_working_folder_grants
         WHERE conversation_id = ? AND teammate_id = ?
           AND revoked_at IS NULL
         ORDER BY working_folder_id",
    )
    .bind(conversation_id.as_str())
    .bind(teammate_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let grants = rows
        .into_iter()
        .map(|row| {
            let stored: String = row.try_get("capability").map_err(persistence_error)?;
            let inherits = row
                .try_get::<i64, _>("capability_inherits_profile")
                .map_err(persistence_error)?
                == 1;
            let capability = if inherits {
                maximum_capability.to_string()
            } else {
                capability_for_rank(
                    capability_rank(&stored)?.min(capability_rank(maximum_capability)?),
                )?
                .to_string()
            };
            Ok(FolderGrant {
                working_folder_id: row
                    .try_get("working_folder_id")
                    .map_err(persistence_error)?,
                capability,
                is_default: row
                    .try_get::<i64, _>("is_default")
                    .map_err(persistence_error)?
                    == 1,
                runtime_approval_policy: row
                    .try_get("runtime_approval_policy")
                    .map_err(persistence_error)?,
            })
        })
        .collect::<ChatResult<Vec<_>>>()?;
    Ok(grants
        .into_iter()
        .filter(|grant| grant.capability != "none")
        .collect())
}

async fn read_referenced_folder_ids(
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

async fn read_referenced_execution_environments(
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

async fn resolve_execution_target(
    transaction: &mut Transaction<'_, Sqlite>,
    input: ResolveExecutionTargetInput<'_>,
) -> ChatResult<ResolvedExecutionTarget> {
    let ResolveExecutionTargetInput {
        reply_thread_id,
        destination_conversation_id,
        teammate_id,
        requester_participant_id,
        explicit,
        grants,
        referenced_folder_ids,
        referenced_execution_environments,
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
                    teammate_id,
                    scratch_scope_id.as_str(),
                    scratch_generation_id.as_str(),
                    execution_environment_id.as_str(),
                )
                .await?;
                if !super::super::scratch::generation_constraints_hold_in_connection(
                    transaction,
                    scratch_generation_id.as_str(),
                    destination_conversation_id.as_str(),
                    teammate_id.as_str(),
                    requester_participant_id.as_str(),
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
    if let Some(environment) = referenced_execution_environment {
        let grant = require_granted_folder(grants, &environment.working_folder_id)?;
        if capability_can_select_native_target(&grant.capability) {
            verify_folder_environment(
                transaction,
                &environment.working_folder_id,
                &environment.execution_environment_id,
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
        let execution_environment_id = format!("current-folder:{working_folder_id}");
        verify_folder_environment(transaction, &working_folder_id, &execution_environment_id)
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
    teammate_id: &ChatParticipantId,
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
           WHERE scope.id = ? AND scope.reply_thread_id = ? AND scope.teammate_id = ?
             AND generation.id = ? AND generation.lifecycle_state = 'active'
             AND environment.id = ? AND environment.kind = 'scratch'
             AND environment.lifecycle_state = 'available'
             AND scope.removed_at IS NULL AND generation.removed_at IS NULL
             AND environment.archived_at IS NULL
         )",
    )
    .bind(scratch_scope_id)
    .bind(reply_thread_id.as_str())
    .bind(teammate_id.as_str())
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

async fn verify_folder_environment(
    transaction: &mut Transaction<'_, Sqlite>,
    working_folder_id: &str,
    execution_environment_id: &str,
) -> ChatResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_execution_environments
           WHERE id = ? AND working_folder_id = ?
             AND scratch_generation_id IS NULL
             AND lifecycle_state = 'available' AND archived_at IS NULL
         )",
    )
    .bind(execution_environment_id)
    .bind(working_folder_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if valid {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "The selected execution target is unavailable",
            true,
        ))
    }
}

fn require_granted_folder<'a>(
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

async fn read_channel_sources(
    transaction: &mut Transaction<'_, Sqlite>,
    triggering_message_item_id: &ChatConversationItemId,
    destination_conversation_id: &ChatConversationId,
    requester_id: &ChatParticipantId,
    teammate_id: &ChatParticipantId,
) -> ChatResult<Vec<FrozenChannelSource>> {
    let rows = sqlx::query(
        "SELECT reference.id AS reference_id, reference.label_snapshot,
                target.source_conversation_id, target.source_lower_ordinal,
                target.source_high_ordinal, target.source_revision_cutoff_id,
                target.destination_audience_revision
         FROM chat_communication_messages message
         JOIN chat_message_references reference
           ON reference.message_revision_id = message.current_revision_id
          AND reference.reference_kind = 'channel'
         JOIN chat_channel_reference_targets target ON target.reference_id = reference.id
         WHERE message.item_id = ? AND target.destination_conversation_id = ?
           AND target.source_high_ordinal > 0
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
        let lower_ordinal = u64_value(
            row.try_get("source_lower_ordinal")
                .map_err(persistence_error)?,
        )?;
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
                lower_ordinal,
                frozen_audience_revision: destination_audience_revision,
                requester_id,
                teammate_id,
            },
        )
        .await?;
        result.push(FrozenChannelSource {
            source_handle: generate_opaque_handle("channel-source")?,
            message_reference_id: row.try_get("reference_id").map_err(persistence_error)?,
            conversation_id,
            label_snapshot: row.try_get("label_snapshot").map_err(persistence_error)?,
            lower_ordinal,
            high_ordinal: u64_value(
                row.try_get("source_high_ordinal")
                    .map_err(persistence_error)?,
            )?,
            source_revision_cutoff_id: row
                .try_get("source_revision_cutoff_id")
                .map_err(persistence_error)?,
            destination_audience_revision,
        });
    }
    Ok(result)
}

async fn verify_channel_disclosure(
    transaction: &mut Transaction<'_, Sqlite>,
    input: ChannelDisclosureInput<'_>,
) -> ChatResult<()> {
    let ChannelDisclosureInput {
        destination_id,
        destination_message_item_id,
        source_id,
        lower_ordinal,
        frozen_audience_revision,
        requester_id,
        teammate_id,
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
           JOIN chat_conversation_memberships teammate
             ON teammate.conversation_id = ? AND teammate.participant_id = ?
            AND teammate.removed_at IS NULL
           JOIN chat_teammate_channel_memberships teammate_access
             ON teammate_access.conversation_id = teammate.conversation_id
            AND teammate_access.teammate_id = teammate.participant_id
           JOIN chat_access_profiles teammate_profile
             ON teammate_profile.id = teammate_access.access_profile_id
            AND teammate_profile.archived_at IS NULL
           JOIN chat_access_profile_revisions teammate_profile_revision
             ON teammate_profile_revision.access_profile_id = teammate_profile.id
            AND teammate_profile_revision.revision = teammate_profile.latest_revision
           WHERE audience.conversation_id = ? AND audience.revision >= ?
             AND CASE
                   WHEN teammate_access.read_history_inherits_profile = 1
                     THEN teammate_profile_revision.default_read_history
                   ELSE teammate_access.read_history
                        AND teammate_profile_revision.default_read_history
                 END = 1
             AND (
               CASE
                 WHEN teammate_access.history_boundary_inherits_profile = 1
                   THEN teammate_profile_revision.default_history_boundary
                 WHEN teammate_access.history_boundary = 'from_grant'
                   OR teammate_profile_revision.default_history_boundary = 'from_grant'
                   THEN 'from_grant'
                 ELSE 'entire'
               END = 'entire'
               OR (
                 teammate_access.history_from_ordinal IS NOT NULL
                 AND teammate_access.history_from_ordinal <= ?
               )
             )
             AND (
               requester_participant.participant_kind != 'ai_teammate'
               OR EXISTS (
                 SELECT 1
                 FROM chat_teammate_channel_memberships requester_access
                 JOIN chat_access_profiles requester_profile
                   ON requester_profile.id = requester_access.access_profile_id
                  AND requester_profile.archived_at IS NULL
                 JOIN chat_access_profile_revisions requester_profile_revision
                   ON requester_profile_revision.access_profile_id = requester_profile.id
                  AND requester_profile_revision.revision = requester_profile.latest_revision
                 WHERE requester_access.conversation_id = ?
                   AND requester_access.teammate_id = ?
                   AND CASE
                         WHEN requester_access.read_history_inherits_profile = 1
                           THEN requester_profile_revision.default_read_history
                         ELSE requester_access.read_history
                              AND requester_profile_revision.default_read_history
                       END = 1
                   AND (
                     CASE
                       WHEN requester_access.history_boundary_inherits_profile = 1
                         THEN requester_profile_revision.default_history_boundary
                       WHEN requester_access.history_boundary = 'from_grant'
                         OR requester_profile_revision.default_history_boundary = 'from_grant'
                         THEN 'from_grant'
                       ELSE 'entire'
                     END = 'entire'
                     OR (
                       requester_access.history_from_ordinal IS NOT NULL
                       AND requester_access.history_from_ordinal <= ?
                     )
                   )
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
                   participant.participant_kind != 'ai_teammate'
                   OR EXISTS (
                     SELECT 1
                     FROM chat_teammate_channel_memberships destination_ai
                     JOIN chat_access_profiles destination_profile
                       ON destination_profile.id = destination_ai.access_profile_id
                      AND destination_profile.archived_at IS NULL
                     JOIN chat_access_profile_revisions destination_profile_revision
                       ON destination_profile_revision.access_profile_id = destination_profile.id
                      AND destination_profile_revision.revision = destination_profile.latest_revision
                     WHERE destination_ai.conversation_id = ?
                       AND destination_ai.teammate_id = destination_member.participant_id
                       AND CASE
                             WHEN destination_ai.read_history_inherits_profile = 1
                               THEN destination_profile_revision.default_read_history
                             ELSE destination_ai.read_history
                                  AND destination_profile_revision.default_read_history
                           END = 1
                       AND (
                         CASE
                           WHEN destination_ai.history_boundary_inherits_profile = 1
                             THEN destination_profile_revision.default_history_boundary
                           WHEN destination_ai.history_boundary = 'from_grant'
                             OR destination_profile_revision.default_history_boundary = 'from_grant'
                             THEN 'from_grant'
                           ELSE 'entire'
                         END = 'entire'
                         OR (
                           destination_ai.history_from_ordinal IS NOT NULL
                           AND destination_ai.history_from_ordinal <= destination_item.ordinal
                         )
                       )
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
                     participant.participant_kind = 'ai_teammate'
                     AND NOT EXISTS (
                       SELECT 1
                       FROM chat_teammate_channel_memberships source_ai
                       JOIN chat_access_profiles source_profile
                         ON source_profile.id = source_ai.access_profile_id
                        AND source_profile.archived_at IS NULL
                       JOIN chat_access_profile_revisions source_profile_revision
                         ON source_profile_revision.access_profile_id = source_profile.id
                        AND source_profile_revision.revision = source_profile.latest_revision
                       WHERE source_ai.conversation_id = ?
                         AND source_ai.teammate_id = destination_member.participant_id
                         AND CASE
                               WHEN source_ai.read_history_inherits_profile = 1
                                 THEN source_profile_revision.default_read_history
                               ELSE source_ai.read_history
                                    AND source_profile_revision.default_read_history
                             END = 1
                         AND (
                           CASE
                             WHEN source_ai.history_boundary_inherits_profile = 1
                               THEN source_profile_revision.default_history_boundary
                             WHEN source_ai.history_boundary = 'from_grant'
                               OR source_profile_revision.default_history_boundary = 'from_grant'
                               THEN 'from_grant'
                             ELSE 'entire'
                           END = 'entire'
                           OR (
                             source_ai.history_from_ordinal IS NOT NULL
                             AND source_ai.history_from_ordinal <= ?
                           )
                         )
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
    .bind(teammate_id.as_str())
    .bind(destination_id.as_str())
    .bind(i64_value(frozen_audience_revision)?)
    .bind(i64_value(lower_ordinal)?)
    .bind(source_id)
    .bind(requester_id.as_str())
    .bind(i64_value(lower_ordinal)?)
    .bind(destination_id.as_str())
    .bind(destination_id.as_str())
    .bind(source_id)
    .bind(source_id)
    .bind(i64_value(lower_ordinal)?)
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

fn capability_rank(value: &str) -> ChatResult<u8> {
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

fn capability_for_rank(rank: u8) -> ChatResult<&'static str> {
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
