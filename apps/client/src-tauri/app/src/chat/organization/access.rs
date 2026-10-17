//! Teammate access previews, atomic replacement, and authority-change classification.

mod channels;
mod profiles;
mod retained_references;
mod targets;

// Preserve command paths and generated Tauri wrappers at the access facade.
pub(super) use channels::read_teammate_access;
use channels::{proposed_access_read, resolve_channel_access};
pub use profiles::*;
use retained_references::{retained_reference_issues, retained_reference_issues_in_transaction};
pub use targets::*;

use super::common::{wire_folder_capability, wire_history_boundary, wire_runtime_approval_policy};
use super::*;
use ganbaru_chat::coordination::access::history_boundary_is_expansion;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct ResolvedChannelAccess {
    channel_id: ChatChannelId,
    project_id: String,
    group_id: String,
    conversation_id: ChatConversationId,
    channel_name: String,
    access_profile_id: ChatAccessProfileId,
    access_profile_revision: u64,
    profile_default_channel_capabilities: ChatChannelCapabilities,
    profile_default_history_boundary: ChatHistoryBoundary,
    profile_maximum_folder_capability: ChatFolderCapability,
    capabilities: ChatChannelCapabilities,
    history_boundary: ChatHistoryBoundary,
    runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    scratch_runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    folder_grants: Vec<ChatFolderGrantInput>,
}

struct ValidatedTeammateConfiguration {
    display_name: String,
    role: String,
    instructions: String,
    avatar_schema_version: u32,
    avatar_data: String,
    expected_revision: u64,
}

fn validate_teammate_configuration(
    app: &tauri::AppHandle,
    teammate_id: &ChatParticipantId,
    teammate_profile: Option<&UpdateChatTeammateProfileCommand>,
    policy: Option<&ChatTeammatePolicyInput>,
) -> ChatResult<Option<ValidatedTeammateConfiguration>> {
    match (teammate_profile, policy) {
        (None, None) => Ok(None),
        (Some(profile), Some(policy)) => {
            if &profile.teammate_id != teammate_id {
                return Err(ChatError::validation(
                    "teammateProfile.teammateId",
                    "The teammate profile does not match the access draft",
                ));
            }
            let display_name = validate_display_name(&profile.display_name)?;
            let role = validate_teammate_role(&profile.role)?;
            validate_profile_text(&profile.instructions, 65_536, "instructions")?;
            validate_policy(app, policy)?;
            Ok(Some(ValidatedTeammateConfiguration {
                display_name,
                role,
                instructions: profile.instructions.trim().to_string(),
                avatar_schema_version: profile.avatar.schema_version,
                avatar_data: json_object(&profile.avatar, "avatar")?,
                expected_revision: profile.expected_revision,
            }))
        }
        _ => Err(ChatError::validation(
            "teammateProfile",
            "Teammate profile and policy changes must be saved together",
        )),
    }
}

#[tauri::command]
pub async fn chat_read_teammate_access(
    app: tauri::AppHandle,
    db_url: String,
    teammate_id: ChatParticipantId,
) -> ChatResult<ChatTeammateAccessRead> {
    read_teammate_access(&chat_pool(app, db_url).await?, &teammate_id).await
}

#[tauri::command]
pub async fn chat_read_channel_roster(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
) -> ChatResult<ChatChannelRosterRead> {
    let pool = chat_pool(app, db_url).await?;
    let channel = super::super::channels::read_channel(&pool, &channel_id).await?;
    let audience_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM chat_conversation_audience_state WHERE conversation_id = ?",
    )
    .bind(channel.conversation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatChannelRosterRead {
        channel_id,
        conversation_id: channel.conversation_id.clone(),
        audience_revision: u64_value(audience_revision)?,
        memberships: read_memberships_for_conversation(&pool, &channel.conversation_id, false)
            .await?,
    })
}

#[tauri::command]
pub async fn chat_preview_channel_membership_removal(
    app: tauri::AppHandle,
    db_url: String,
    request: PreviewChatChannelMembershipRemovalCommand,
) -> ChatResult<ChatChannelMembershipRemovalPreview> {
    let pool = chat_pool(app, db_url).await?;
    let channel = super::super::channels::read_channel(&pool, &request.channel_id).await?;
    let mut proposed_access = read_teammate_access(&pool, &request.teammate_id).await?;
    if proposed_access.access_revision != request.expected_access_revision {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The teammate access changed before the removal preview",
            true,
        ));
    }
    if !proposed_access
        .channels
        .iter()
        .any(|member| member.channel_id == request.channel_id)
    {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The teammate is not a member of this channel",
            true,
        ));
    }
    let active_assignment_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         WHERE assignment.teammate_id = ? AND thread.conversation_id = ?
           AND assignment.state IN (
             'queued', 'working', 'waiting_for_answer',
             'waiting_for_approval', 'ready_for_review'
           )",
    )
    .bind(request.teammate_id.as_str())
    .bind(channel.conversation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    let active_authorization_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.teammate_id = ?
           AND authorization.destination_conversation_id = ?
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL",
    )
    .bind(request.teammate_id.as_str())
    .bind(channel.conversation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    proposed_access
        .channels
        .retain(|member| member.channel_id != request.channel_id);
    proposed_access.access_revision += 1;
    Ok(ChatChannelMembershipRemovalPreview {
        teammate_id: request.teammate_id,
        channel_id: request.channel_id,
        active_assignment_count: u64_value(active_assignment_count)?,
        active_authorization_count: u64_value(active_authorization_count)?,
        will_revoke_active_work: active_assignment_count > 0 || active_authorization_count > 0,
        proposed_access,
    })
}

#[tauri::command]
pub async fn chat_preview_teammate_access(
    app: tauri::AppHandle,
    db_url: String,
    request: PreviewChatTeammateAccessCommand,
) -> ChatResult<ChatTeammateAccessPreview> {
    let validated_configuration = validate_teammate_configuration(
        &app,
        &request.teammate_id,
        request.teammate_profile.as_ref(),
        request.policy.as_ref(),
    )?;
    let pool = chat_pool(app, db_url).await?;
    if let Some(configuration) = validated_configuration {
        let current = read_teammate(&pool, &request.teammate_id).await?;
        if current.participant.revision != configuration.expected_revision {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "The teammate changed before the draft could be previewed",
                true,
            ));
        }
    }
    preview_teammate_access(
        &pool,
        &request.teammate_id,
        request.expected_access_revision,
        request.teammate_default_runtime_approval,
        &request.channels,
    )
    .await
}

#[tauri::command]
pub async fn chat_replace_teammate_access(
    app: tauri::AppHandle,
    db_url: String,
    request: ReplaceChatTeammateAccessCommand,
) -> ChatResult<ChatTeammateAccessRead> {
    let validated_configuration = validate_teammate_configuration(
        &app,
        &request.teammate_id,
        request.teammate_profile.as_ref(),
        request.policy.as_ref(),
    )?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let preview = preview_teammate_access(
        &pool,
        &request.teammate_id,
        request.expected_access_revision,
        request.teammate_default_runtime_approval,
        &request.channels,
    )
    .await?;
    if let Some(issue) = preview.issues.first() {
        return Err(ChatError::validation(
            &issue.field_path,
            issue.message.clone(),
        ));
    }
    let current = read_teammate_access(&pool, &request.teammate_id).await?;
    let resolved = resolve_channel_access(&pool, &request.teammate_id, &request.channels)
        .await?
        .0;
    let has_reduction = teammate_access_has_reduction(
        &current,
        request.teammate_default_runtime_approval,
        &resolved,
    );
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let retained_issues =
        retained_reference_issues_in_transaction(&mut transaction, &resolved).await?;
    if let Some(issue) = retained_issues.first() {
        return Err(ChatError::validation(
            &issue.field_path,
            issue.message.clone(),
        ));
    }
    let updated = sqlx::query(
        "UPDATE chat_teammate_access_state
         SET access_revision = access_revision + 1,
             runtime_approval_policy = ?, updated_at = ?
         WHERE teammate_id = ? AND access_revision = ?",
    )
    .bind(wire_runtime_approval_policy(
        request.teammate_default_runtime_approval,
    ))
    .bind(now.as_str())
    .bind(request.teammate_id.as_str())
    .bind(i64_value(request.expected_access_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The teammate access changed before it could be saved",
            true,
        ));
    }
    require_expected_profile_revisions_in_transaction(&mut transaction, &request.channels).await?;

    if let Some(configuration) = validated_configuration {
        let updated = sqlx::query(
            "UPDATE chat_participants
             SET display_name = ?, avatar_schema_version = ?, avatar_data = ?,
                 revision = revision + 1, updated_at = ?
             WHERE id = ? AND participant_kind = 'ai_teammate' AND revision = ?",
        )
        .bind(configuration.display_name)
        .bind(i64::from(configuration.avatar_schema_version))
        .bind(configuration.avatar_data)
        .bind(now.as_str())
        .bind(request.teammate_id.as_str())
        .bind(i64_value(configuration.expected_revision)?)
        .execute(&mut *transaction)
        .await
        .map_err(map_teammate_write_error)?;
        if updated.rows_affected() != 1 {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "The teammate changed before the access draft could be saved",
                true,
            ));
        }
        sqlx::query(
            "UPDATE chat_teammates SET role = ?, instructions = ?, updated_at = ?
             WHERE participant_id = ?",
        )
        .bind(configuration.role)
        .bind(configuration.instructions)
        .bind(now.as_str())
        .bind(request.teammate_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        let next_policy_revision: i64 = sqlx::query_scalar(
            "SELECT latest_policy_revision + 1
             FROM chat_teammates WHERE participant_id = ?",
        )
        .bind(request.teammate_id.as_str())
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        insert_policy_revision(
            &mut transaction,
            &request.teammate_id,
            u64_value(next_policy_revision)?,
            request
                .policy
                .as_ref()
                .expect("validated teammate policy must be present"),
            &now,
        )
        .await?;
    }

    let proposed_conversations = resolved
        .iter()
        .map(|channel| channel.conversation_id.as_str())
        .collect::<BTreeSet<_>>();
    let existing_conversations = sqlx::query_scalar::<_, String>(
        "SELECT conversation_id FROM chat_teammate_channel_memberships
         WHERE teammate_id = ?",
    )
    .bind(request.teammate_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for conversation_id in existing_conversations {
        if !proposed_conversations.contains(conversation_id.as_str()) {
            sqlx::query(
                "UPDATE chat_teammate_working_folder_grants
                 SET revoked_at = ?, is_default = 0, revision = revision + 1
                 WHERE conversation_id = ? AND teammate_id = ? AND revoked_at IS NULL",
            )
            .bind(now.as_str())
            .bind(&conversation_id)
            .bind(request.teammate_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
            sqlx::query(
                "DELETE FROM chat_teammate_channel_memberships
                 WHERE conversation_id = ? AND teammate_id = ?",
            )
            .bind(&conversation_id)
            .bind(request.teammate_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
            sqlx::query(
                "UPDATE chat_conversation_memberships
                 SET removed_at = ?, revision = revision + 1, updated_at = ?
                 WHERE conversation_id = ? AND participant_id = ?",
            )
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(&conversation_id)
            .bind(request.teammate_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        }
    }

    for channel in &resolved {
        sqlx::query(
            "INSERT INTO chat_conversation_memberships
                (conversation_id, participant_id, membership_role, created_at, updated_at)
             VALUES (?, ?, 'member', ?, ?)
             ON CONFLICT(conversation_id, participant_id) DO UPDATE SET
                membership_role = 'member', removed_at = NULL,
                revision = chat_conversation_memberships.revision + 1,
                updated_at = excluded.updated_at",
        )
        .bind(channel.conversation_id.as_str())
        .bind(request.teammate_id.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        let history_from_ordinal = match channel.history_boundary {
            ChatHistoryBoundary::Entire => None,
            ChatHistoryBoundary::FromGrant { lower_ordinal } => {
                Some(i64_value(lower_ordinal.ok_or_else(|| {
                    ChatError::new(
                        ChatErrorCode::Persistence,
                        "Resolved history boundary is missing its lower ordinal",
                        false,
                    )
                })?)?)
            }
        };
        let read_history_inherits_profile = channel.capabilities.read_history
            == channel.profile_default_channel_capabilities.read_history;
        let participate_inherits_profile = channel.capabilities.participate
            == channel.profile_default_channel_capabilities.participate;
        let history_boundary_inherits_profile = wire_history_boundary(&channel.history_boundary)
            == wire_history_boundary(&channel.profile_default_history_boundary);
        sqlx::query(
            "INSERT INTO chat_teammate_channel_memberships
                (conversation_id, teammate_id, access_profile_id,
                 read_history, read_history_inherits_profile,
                 participate, participate_inherits_profile,
                 history_boundary, history_boundary_inherits_profile,
                 history_from_ordinal, runtime_approval_policy,
                 scratch_runtime_approval_policy, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(conversation_id, teammate_id) DO UPDATE SET
                access_profile_id = excluded.access_profile_id,
                read_history = excluded.read_history,
                read_history_inherits_profile = excluded.read_history_inherits_profile,
                participate = excluded.participate,
                participate_inherits_profile = excluded.participate_inherits_profile,
                history_boundary = excluded.history_boundary,
                history_boundary_inherits_profile = excluded.history_boundary_inherits_profile,
                history_from_ordinal = excluded.history_from_ordinal,
                runtime_approval_policy = excluded.runtime_approval_policy,
                scratch_runtime_approval_policy = excluded.scratch_runtime_approval_policy,
                updated_at = excluded.updated_at",
        )
        .bind(channel.conversation_id.as_str())
        .bind(request.teammate_id.as_str())
        .bind(channel.access_profile_id.as_str())
        .bind(channel.capabilities.read_history)
        .bind(read_history_inherits_profile)
        .bind(channel.capabilities.participate)
        .bind(participate_inherits_profile)
        .bind(wire_history_boundary(&channel.history_boundary))
        .bind(history_boundary_inherits_profile)
        .bind(history_from_ordinal)
        .bind(
            channel
                .runtime_approval_override
                .map(wire_runtime_approval_policy),
        )
        .bind(
            channel
                .scratch_runtime_approval_override
                .map(wire_runtime_approval_policy),
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;

        sqlx::query(
            "UPDATE chat_teammate_working_folder_grants
             SET revoked_at = ?, is_default = 0, revision = revision + 1
             WHERE conversation_id = ? AND teammate_id = ? AND revoked_at IS NULL",
        )
        .bind(now.as_str())
        .bind(channel.conversation_id.as_str())
        .bind(request.teammate_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        for grant in &channel.folder_grants {
            sqlx::query(
                "INSERT INTO chat_teammate_working_folder_grants
                    (conversation_id, teammate_id, project_id, working_folder_id,
                     capability, capability_inherits_profile,
                     is_default, runtime_approval_policy,
                     revision, created_at, revoked_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, NULL)
                 ON CONFLICT(conversation_id, teammate_id, working_folder_id) DO UPDATE SET
                    project_id = excluded.project_id,
                    capability = excluded.capability,
                    capability_inherits_profile = excluded.capability_inherits_profile,
                    is_default = excluded.is_default,
                    runtime_approval_policy = excluded.runtime_approval_policy,
                    revision = chat_teammate_working_folder_grants.revision + 1,
                    revoked_at = NULL",
            )
            .bind(channel.conversation_id.as_str())
            .bind(request.teammate_id.as_str())
            .bind(&channel.project_id)
            .bind(grant.working_folder_id.as_str())
            .bind(wire_folder_capability(grant.capability))
            .bind(grant.capability == channel.profile_maximum_folder_capability)
            .bind(grant.is_default)
            .bind(
                grant
                    .runtime_approval_override
                    .map(wire_runtime_approval_policy),
            )
            .bind(now.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        }
    }

    if has_reduction {
        revoke_teammate_authorizations(
            &mut transaction,
            &request.teammate_id,
            &now,
            "Teammate access was reduced",
        )
        .await?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    if has_reduction {
        super::super::revocation::drain_access_revocation_jobs(&app, &pool).await?;
    }
    read_teammate_access(&pool, &request.teammate_id).await
}

async fn preview_teammate_access(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
    expected_access_revision: u64,
    teammate_default_runtime_approval: ChatRuntimeApprovalPolicy,
    channels: &[ChatTeammateChannelAccessInput],
) -> ChatResult<ChatTeammateAccessPreview> {
    let current = read_teammate_access(pool, teammate_id).await?;
    if current.access_revision != expected_access_revision {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The teammate access changed before the preview",
            true,
        ));
    }
    let (resolved, mut issues) = resolve_channel_access(pool, teammate_id, channels).await?;
    issues.extend(retained_reference_issues(pool, &resolved).await?);
    let current_ids = current
        .channels
        .iter()
        .map(|channel| channel.channel_id.clone())
        .collect::<BTreeSet<_>>();
    let proposed_ids = resolved
        .iter()
        .map(|channel| channel.channel_id.clone())
        .collect::<BTreeSet<_>>();
    let added_channel_ids = proposed_ids.difference(&current_ids).cloned().collect();
    let removed_channel_ids = current_ids.difference(&proposed_ids).cloned().collect();
    let is_expansion =
        teammate_access_is_expansion(&current, teammate_default_runtime_approval, &resolved);
    let proposed = if issues.is_empty() {
        Some(proposed_access_read(
            teammate_id,
            expected_access_revision + 1,
            teammate_default_runtime_approval,
            &resolved,
        ))
    } else {
        None
    };
    Ok(ChatTeammateAccessPreview {
        proposed,
        is_expansion,
        added_channel_ids,
        removed_channel_ids,
        issues,
    })
}

async fn require_expected_profile_revisions_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    channels: &[ChatTeammateChannelAccessInput],
) -> ChatResult<()> {
    let expected = channels
        .iter()
        .map(|channel| {
            (
                channel.access_profile_id.as_str(),
                channel.access_profile_revision,
            )
        })
        .collect::<BTreeSet<_>>();
    for (access_profile_id, expected_revision) in expected {
        let latest_revision = sqlx::query_scalar::<_, i64>(
            "SELECT latest_revision FROM chat_access_profiles
             WHERE id = ? AND archived_at IS NULL",
        )
        .bind(access_profile_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(persistence_error)?;
        let Some(latest_revision) = latest_revision else {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "An access profile became unavailable before teammate access could be saved",
                true,
            ));
        };
        if u64_value(latest_revision)? != expected_revision {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "An access profile changed before teammate access could be saved",
                true,
            ));
        }
    }
    Ok(())
}

fn teammate_access_is_expansion(
    current: &ChatTeammateAccessRead,
    proposed_runtime: ChatRuntimeApprovalPolicy,
    proposed: &[ResolvedChannelAccess],
) -> bool {
    if runtime_approval_is_expansion(current.teammate_default_runtime_approval, proposed_runtime) {
        return true;
    }
    let current_by_channel = current
        .channels
        .iter()
        .map(|channel| (channel.channel_id.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    proposed.iter().any(|channel| {
        let Some(current_channel) = current_by_channel.get(channel.channel_id.as_str()) else {
            return true;
        };
        (!current_channel.capabilities.read_history && channel.capabilities.read_history)
            || (!current_channel.capabilities.participate && channel.capabilities.participate)
            || history_boundary_is_expansion(
                &current_channel.history_boundary,
                &channel.history_boundary,
            )
            || runtime_approval_is_expansion(
                resolve_runtime_approval(
                    current_channel.runtime_approval_override,
                    current.teammate_default_runtime_approval,
                ),
                resolve_runtime_approval(channel.runtime_approval_override, proposed_runtime),
            )
            || runtime_approval_is_expansion(
                resolve_nested_runtime_approval(
                    current_channel.scratch_runtime_approval_override,
                    current_channel.runtime_approval_override,
                    current.teammate_default_runtime_approval,
                ),
                resolve_nested_runtime_approval(
                    channel.scratch_runtime_approval_override,
                    channel.runtime_approval_override,
                    proposed_runtime,
                ),
            )
            || channel.folder_grants.iter().any(|grant| {
                current_channel
                    .folder_grants
                    .iter()
                    .find(|current| current.working_folder_id == grant.working_folder_id)
                    .is_none_or(|current_grant| {
                        grant.capability.rank() > current_grant.capability.rank()
                            || runtime_approval_is_expansion(
                                resolve_nested_runtime_approval(
                                    current_grant.runtime_approval_override,
                                    current_channel.runtime_approval_override,
                                    current.teammate_default_runtime_approval,
                                ),
                                resolve_nested_runtime_approval(
                                    grant.runtime_approval_override,
                                    channel.runtime_approval_override,
                                    proposed_runtime,
                                ),
                            )
                    })
            })
    })
}

fn teammate_access_has_reduction(
    current: &ChatTeammateAccessRead,
    proposed_runtime: ChatRuntimeApprovalPolicy,
    proposed: &[ResolvedChannelAccess],
) -> bool {
    if runtime_approval_is_reduction(current.teammate_default_runtime_approval, proposed_runtime) {
        return true;
    }
    let proposed_by_channel = proposed
        .iter()
        .map(|channel| (channel.channel_id.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    current.channels.iter().any(|channel| {
        let Some(proposed_channel) = proposed_by_channel.get(channel.channel_id.as_str()) else {
            return true;
        };
        (channel.capabilities.read_history && !proposed_channel.capabilities.read_history)
            || (channel.capabilities.participate && !proposed_channel.capabilities.participate)
            || history_boundary_is_expansion(
                &proposed_channel.history_boundary,
                &channel.history_boundary,
            )
            || runtime_approval_is_reduction(
                resolve_runtime_approval(
                    channel.runtime_approval_override,
                    current.teammate_default_runtime_approval,
                ),
                resolve_runtime_approval(
                    proposed_channel.runtime_approval_override,
                    proposed_runtime,
                ),
            )
            || runtime_approval_is_reduction(
                resolve_nested_runtime_approval(
                    channel.scratch_runtime_approval_override,
                    channel.runtime_approval_override,
                    current.teammate_default_runtime_approval,
                ),
                resolve_nested_runtime_approval(
                    proposed_channel.scratch_runtime_approval_override,
                    proposed_channel.runtime_approval_override,
                    proposed_runtime,
                ),
            )
            || channel.folder_grants.iter().any(|grant| {
                proposed_channel
                    .folder_grants
                    .iter()
                    .find(|proposed| proposed.working_folder_id == grant.working_folder_id)
                    .is_none_or(|proposed_grant| {
                        proposed_grant.capability.rank() < grant.capability.rank()
                            || runtime_approval_is_reduction(
                                resolve_nested_runtime_approval(
                                    grant.runtime_approval_override,
                                    channel.runtime_approval_override,
                                    current.teammate_default_runtime_approval,
                                ),
                                resolve_nested_runtime_approval(
                                    proposed_grant.runtime_approval_override,
                                    proposed_channel.runtime_approval_override,
                                    proposed_runtime,
                                ),
                            )
                    })
            })
    })
}

fn resolve_runtime_approval(
    override_policy: Option<ChatRuntimeApprovalPolicy>,
    fallback: ChatRuntimeApprovalPolicy,
) -> ChatRuntimeApprovalPolicy {
    override_policy.unwrap_or(fallback)
}

fn resolve_nested_runtime_approval(
    resource_override: Option<ChatRuntimeApprovalPolicy>,
    channel_override: Option<ChatRuntimeApprovalPolicy>,
    teammate_default: ChatRuntimeApprovalPolicy,
) -> ChatRuntimeApprovalPolicy {
    resource_override
        .or(channel_override)
        .unwrap_or(teammate_default)
}

fn runtime_approval_is_expansion(
    current: ChatRuntimeApprovalPolicy,
    proposed: ChatRuntimeApprovalPolicy,
) -> bool {
    if current == proposed {
        return false;
    }
    match (
        runtime_approval_rank(current),
        runtime_approval_rank(proposed),
    ) {
        (Some(current), Some(proposed)) => proposed > current,
        _ => true,
    }
}

fn runtime_approval_is_reduction(
    current: ChatRuntimeApprovalPolicy,
    proposed: ChatRuntimeApprovalPolicy,
) -> bool {
    if current == proposed {
        return false;
    }
    match (
        runtime_approval_rank(current),
        runtime_approval_rank(proposed),
    ) {
        (Some(current), Some(proposed)) => proposed < current,
        _ => true,
    }
}

fn runtime_approval_rank(policy: ChatRuntimeApprovalPolicy) -> Option<u8> {
    match policy {
        ChatRuntimeApprovalPolicy::Ask => Some(0),
        ChatRuntimeApprovalPolicy::AutoApprove => Some(1),
        ChatRuntimeApprovalPolicy::Unattended => Some(2),
        ChatRuntimeApprovalPolicy::ProviderCustom => None,
    }
}

async fn revoke_teammate_authorizations(
    transaction: &mut Transaction<'_, Sqlite>,
    teammate_id: &ChatParticipantId,
    now: &UtcTimestamp,
    reason: &str,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', reason = ?, revoked_at = ?
         WHERE id IN (
             SELECT authorization.id
             FROM chat_assignment_authorization_revisions authorization
             JOIN chat_work_assignments assignment
               ON assignment.id = authorization.assignment_id
             WHERE assignment.teammate_id = ?
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
         )",
    )
    .bind(reason)
    .bind(now.as_str())
    .bind(teammate_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_access_revocation_jobs
            (id, teammate_id, authorization_revision_id, reason,
             available_at, created_at, updated_at)
         SELECT 'access-revocation:' || lower(hex(randomblob(16))), ?, authorization.id, ?, ?, ?, ?
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.teammate_id = ? AND authorization.revoked_at = ?
           AND NOT EXISTS (
               SELECT 1 FROM chat_access_revocation_jobs existing
               WHERE existing.authorization_revision_id = authorization.id
                 AND existing.state IN ('queued', 'claimed')
           )",
    )
    .bind(teammate_id.as_str())
    .bind(reason)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(teammate_id.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn issue(code: ChatAccessIssueCode, field_path: &str, message: &str) -> ChatAccessValidationIssue {
    ChatAccessValidationIssue {
        code,
        field_path: field_path.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_changes_are_classified_by_effective_authority() {
        assert!(runtime_approval_is_expansion(
            ChatRuntimeApprovalPolicy::Ask,
            ChatRuntimeApprovalPolicy::AutoApprove,
        ));
        assert!(runtime_approval_is_expansion(
            ChatRuntimeApprovalPolicy::AutoApprove,
            ChatRuntimeApprovalPolicy::Unattended,
        ));
        assert!(runtime_approval_is_reduction(
            ChatRuntimeApprovalPolicy::Unattended,
            ChatRuntimeApprovalPolicy::Ask,
        ));
        assert!(!runtime_approval_is_reduction(
            ChatRuntimeApprovalPolicy::Ask,
            ChatRuntimeApprovalPolicy::AutoApprove,
        ));
    }

    #[test]
    fn provider_custom_requires_review_in_both_directions() {
        assert!(runtime_approval_is_expansion(
            ChatRuntimeApprovalPolicy::Ask,
            ChatRuntimeApprovalPolicy::ProviderCustom,
        ));
        assert!(runtime_approval_is_reduction(
            ChatRuntimeApprovalPolicy::Ask,
            ChatRuntimeApprovalPolicy::ProviderCustom,
        ));
        assert!(runtime_approval_is_expansion(
            ChatRuntimeApprovalPolicy::ProviderCustom,
            ChatRuntimeApprovalPolicy::Ask,
        ));
        assert!(runtime_approval_is_reduction(
            ChatRuntimeApprovalPolicy::ProviderCustom,
            ChatRuntimeApprovalPolicy::Ask,
        ));
    }

    #[test]
    fn resource_approval_precedes_channel_and_teammate_defaults() {
        assert_eq!(
            resolve_nested_runtime_approval(
                Some(ChatRuntimeApprovalPolicy::Ask),
                Some(ChatRuntimeApprovalPolicy::AutoApprove),
                ChatRuntimeApprovalPolicy::Unattended,
            ),
            ChatRuntimeApprovalPolicy::Ask,
        );
        assert_eq!(
            resolve_nested_runtime_approval(
                None,
                Some(ChatRuntimeApprovalPolicy::AutoApprove),
                ChatRuntimeApprovalPolicy::Unattended,
            ),
            ChatRuntimeApprovalPolicy::AutoApprove,
        );
    }
}
