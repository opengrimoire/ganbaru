//! Live authorization checks shared by host-tool calls and publication.

use super::{generic_denial_error, persistence_error, wire_folder_capability};
use crate::chat::internal_mcp::{
    InternalMcpChannelSource, InternalMcpFolderSource, InternalMcpRunScope,
};
use ganbaru_chat_contracts::models::{ChatResult, ChatRuntimeApprovalPolicy};
use sqlx::SqlitePool;

pub(crate) async fn verify_scope(
    pool: &SqlitePool,
    thread_id: &ganbaru_chat_contracts::models::ChatThreadId,
    scope: &InternalMcpRunScope,
) -> ChatResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM chat_assignment_authorization_revisions authorization
            JOIN chat_work_assignments assignment
              ON assignment.id = authorization.assignment_id
            JOIN chat_teammate_channel_memberships channel_access
              ON channel_access.conversation_id = authorization.destination_conversation_id
             AND channel_access.teammate_id = assignment.teammate_id
            JOIN chat_conversation_memberships membership
              ON membership.conversation_id = channel_access.conversation_id
             AND membership.participant_id = channel_access.teammate_id
             AND membership.removed_at IS NULL
            JOIN chat_access_profiles profile
              ON profile.id = channel_access.access_profile_id
            JOIN chat_access_profile_revisions profile_revision
              ON profile_revision.access_profile_id = profile.id
             AND profile_revision.revision = profile.latest_revision
            JOIN chat_agent_runs run
              ON run.id = ?
             AND run.assignment_id = authorization.assignment_id
             AND run.provider_thread_id = ?
             AND run.provider_turn_id = ?
             AND run.authorization_revision_id = authorization.id
             AND run.authorization_scope_digest = authorization.scope_digest
             AND run.state IN ('starting', 'working', 'waiting')
            WHERE authorization.id = ?
              AND authorization.assignment_id = ?
              AND authorization.destination_conversation_id = ?
              AND authorization.scope_digest = ?
              AND authorization.decision_state = 'allowed'
              AND authorization.revoked_at IS NULL
              AND CASE
                    WHEN channel_access.participate_inherits_profile = 1
                      THEN profile_revision.default_participate
                    ELSE channel_access.participate AND profile_revision.default_participate
                  END = 1
        )",
    )
    .bind(scope.run_id.as_str())
    .bind(thread_id.as_str())
    .bind(scope.provider_turn_id.as_str())
    .bind(scope.authorization_revision_id.as_str())
    .bind(scope.assignment_id.as_str())
    .bind(scope.destination_conversation_id.as_str())
    .bind(&scope.authorization_scope_digest)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if !valid {
        return Err(generic_denial_error());
    }
    if let Some(scratch_generation_id) = scope.scratch_generation_id.as_deref() {
        crate::chat::scratch::require_reusable_generation(
            pool,
            scratch_generation_id,
            scope.authorization_revision_id.as_str(),
        )
        .await
        .map_err(|_| generic_denial_error())?;
    }
    Ok(())
}

pub(crate) async fn verify_publication_scope(
    pool: &SqlitePool,
    thread_id: &ganbaru_chat_contracts::models::ChatThreadId,
    scope: &InternalMcpRunScope,
) -> ChatResult<()> {
    verify_scope(pool, thread_id, scope).await?;
    for source in &scope.channel_sources {
        verify_channel_source(pool, scope, source).await?;
    }
    for source in &scope.folder_sources {
        verify_folder_source(pool, scope, source).await?;
    }
    Ok(())
}

pub(super) async fn verify_channel_source(
    pool: &SqlitePool,
    scope: &InternalMcpRunScope,
    source: &InternalMcpChannelSource,
) -> ChatResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM chat_assignment_authorized_channel_sources source
            JOIN chat_assignment_authorization_revisions authorization
              ON authorization.id = source.authorization_revision_id
             AND authorization.decision_state = 'allowed'
             AND authorization.revoked_at IS NULL
            JOIN chat_work_assignments assignment
              ON assignment.id = authorization.assignment_id
            JOIN chat_teammate_channel_memberships channel_access
              ON channel_access.conversation_id = source.conversation_id
             AND channel_access.teammate_id = assignment.teammate_id
            JOIN chat_conversation_memberships membership
              ON membership.conversation_id = channel_access.conversation_id
             AND membership.participant_id = channel_access.teammate_id
             AND membership.removed_at IS NULL
            JOIN chat_access_profiles teammate_profile
              ON teammate_profile.id = channel_access.access_profile_id
            JOIN chat_access_profile_revisions teammate_profile_revision
              ON teammate_profile_revision.access_profile_id = teammate_profile.id
             AND teammate_profile_revision.revision = teammate_profile.latest_revision
            JOIN chat_conversation_memberships requester_membership
              ON requester_membership.conversation_id = source.conversation_id
             AND requester_membership.participant_id = authorization.requester_participant_id
             AND requester_membership.removed_at IS NULL
            JOIN chat_participants requester_participant
              ON requester_participant.id = requester_membership.participant_id
            JOIN chat_conversation_audience_state destination_audience
              ON destination_audience.conversation_id = authorization.destination_conversation_id
            JOIN chat_message_references reference
              ON reference.id = source.message_reference_id
            JOIN chat_communication_message_revisions reference_revision
              ON reference_revision.id = reference.message_revision_id
            JOIN chat_conversation_items reference_item
              ON reference_item.id = reference_revision.message_item_id
            LEFT JOIN chat_reply_threads reference_thread
              ON reference_thread.id = reference_item.reply_thread_id
            LEFT JOIN chat_conversation_items reference_root
              ON reference_root.id = reference_thread.root_item_id
            WHERE source.authorization_revision_id = ?
              AND source.source_handle = ?
              AND source.message_reference_id = ?
              AND source.conversation_id = ?
              AND source.lower_ordinal = ?
              AND source.high_ordinal = ?
              AND source.source_revision_cutoff_id = ?
              AND source.destination_audience_revision = ?
              AND destination_audience.revision >= source.destination_audience_revision
              AND reference_item.conversation_id = authorization.destination_conversation_id
              AND CASE
                    WHEN channel_access.read_history_inherits_profile = 1
                      THEN teammate_profile_revision.default_read_history
                    ELSE channel_access.read_history
                         AND teammate_profile_revision.default_read_history
                  END = 1
              AND (
                CASE
                  WHEN channel_access.history_boundary_inherits_profile = 1
                    THEN teammate_profile_revision.default_history_boundary
                  WHEN channel_access.history_boundary = 'from_grant'
                    OR teammate_profile_revision.default_history_boundary = 'from_grant'
                    THEN 'from_grant'
                  ELSE 'entire'
                END = 'entire'
                OR (
                  channel_access.history_from_ordinal IS NOT NULL
                  AND channel_access.history_from_ordinal <= source.lower_ordinal
                )
              )
              AND (
                requester_participant.participant_kind != 'ai_teammate'
                OR EXISTS (
                  SELECT 1
                  FROM chat_teammate_channel_memberships requester_source_access
                  JOIN chat_access_profiles requester_profile
                    ON requester_profile.id = requester_source_access.access_profile_id
                  JOIN chat_access_profile_revisions requester_profile_revision
                    ON requester_profile_revision.access_profile_id = requester_profile.id
                   AND requester_profile_revision.revision = requester_profile.latest_revision
                  WHERE requester_source_access.conversation_id = source.conversation_id
                    AND requester_source_access.teammate_id = authorization.requester_participant_id
                    AND CASE
                          WHEN requester_source_access.read_history_inherits_profile = 1
                            THEN requester_profile_revision.default_read_history
                          ELSE requester_source_access.read_history
                               AND requester_profile_revision.default_read_history
                        END = 1
                    AND (
                      CASE
                        WHEN requester_source_access.history_boundary_inherits_profile = 1
                          THEN requester_profile_revision.default_history_boundary
                        WHEN requester_source_access.history_boundary = 'from_grant'
                          OR requester_profile_revision.default_history_boundary = 'from_grant'
                          THEN 'from_grant'
                        ELSE 'entire'
                      END = 'entire'
                      OR (
                        requester_source_access.history_from_ordinal IS NOT NULL
                        AND requester_source_access.history_from_ordinal <= source.lower_ordinal
                      )
                    )
                )
              )
              AND NOT EXISTS (
                SELECT 1
                FROM chat_conversation_memberships destination_member
                JOIN chat_participants destination_participant
                  ON destination_participant.id = destination_member.participant_id
                WHERE destination_member.conversation_id = authorization.destination_conversation_id
                  AND destination_member.removed_at IS NULL
                  AND (
                    destination_participant.participant_kind != 'ai_teammate'
                    OR EXISTS (
                      SELECT 1
                      FROM chat_teammate_channel_memberships destination_ai_access
                      JOIN chat_access_profiles destination_profile
                        ON destination_profile.id = destination_ai_access.access_profile_id
                      JOIN chat_access_profile_revisions destination_profile_revision
                        ON destination_profile_revision.access_profile_id = destination_profile.id
                       AND destination_profile_revision.revision = destination_profile.latest_revision
                      WHERE destination_ai_access.conversation_id = authorization.destination_conversation_id
                        AND destination_ai_access.teammate_id = destination_member.participant_id
                        AND CASE
                              WHEN destination_ai_access.read_history_inherits_profile = 1
                                THEN destination_profile_revision.default_read_history
                              ELSE destination_ai_access.read_history
                                   AND destination_profile_revision.default_read_history
                            END = 1
                        AND (
                          CASE
                            WHEN destination_ai_access.history_boundary_inherits_profile = 1
                              THEN destination_profile_revision.default_history_boundary
                            WHEN destination_ai_access.history_boundary = 'from_grant'
                              OR destination_profile_revision.default_history_boundary = 'from_grant'
                              THEN 'from_grant'
                            ELSE 'entire'
                          END = 'entire'
                          OR (
                            destination_ai_access.history_from_ordinal IS NOT NULL
                            AND destination_ai_access.history_from_ordinal
                              <= coalesce(reference_root.ordinal, reference_item.ordinal)
                          )
                        )
                    )
                  )
                  AND (
                    NOT EXISTS (
                      SELECT 1 FROM chat_conversation_memberships source_member
                      WHERE source_member.conversation_id = source.conversation_id
                        AND source_member.participant_id = destination_member.participant_id
                        AND source_member.removed_at IS NULL
                    )
                    OR (
                      destination_participant.participant_kind = 'ai_teammate'
                      AND NOT EXISTS (
                        SELECT 1
                        FROM chat_teammate_channel_memberships source_ai_access
                        JOIN chat_access_profiles source_profile
                          ON source_profile.id = source_ai_access.access_profile_id
                        JOIN chat_access_profile_revisions source_profile_revision
                          ON source_profile_revision.access_profile_id = source_profile.id
                         AND source_profile_revision.revision = source_profile.latest_revision
                        WHERE source_ai_access.conversation_id = source.conversation_id
                          AND source_ai_access.teammate_id = destination_member.participant_id
                          AND CASE
                                WHEN source_ai_access.read_history_inherits_profile = 1
                                  THEN source_profile_revision.default_read_history
                                ELSE source_ai_access.read_history
                                     AND source_profile_revision.default_read_history
                              END = 1
                          AND (
                            CASE
                              WHEN source_ai_access.history_boundary_inherits_profile = 1
                                THEN source_profile_revision.default_history_boundary
                              WHEN source_ai_access.history_boundary = 'from_grant'
                                OR source_profile_revision.default_history_boundary = 'from_grant'
                                THEN 'from_grant'
                              ELSE 'entire'
                            END = 'entire'
                            OR (
                              source_ai_access.history_from_ordinal IS NOT NULL
                              AND source_ai_access.history_from_ordinal <= source.lower_ordinal
                            )
                          )
                      )
                    )
                  )
              )
        )",
    )
    .bind(scope.authorization_revision_id.as_str())
    .bind(&source.source_handle)
    .bind(&source.message_reference_id)
    .bind(&source.conversation_id)
    .bind(i64::try_from(source.lower_ordinal).map_err(|_| generic_denial_error())?)
    .bind(i64::try_from(source.high_ordinal).map_err(|_| generic_denial_error())?)
    .bind(&source.source_revision_cutoff_id)
    .bind(i64::try_from(source.destination_audience_revision).map_err(|_| generic_denial_error())?)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if valid {
        Ok(())
    } else {
        Err(generic_denial_error())
    }
}

pub(super) async fn verify_folder_source(
    pool: &SqlitePool,
    scope: &InternalMcpRunScope,
    source: &InternalMcpFolderSource,
) -> ChatResult<()> {
    let capability = wire_folder_capability(source.capability);
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM chat_assignment_authorized_folder_sources source
            JOIN chat_assignment_authorization_revisions authorization
              ON authorization.id = source.authorization_revision_id
             AND authorization.decision_state = 'allowed'
             AND authorization.revoked_at IS NULL
            JOIN chat_work_assignments assignment
              ON assignment.id = authorization.assignment_id
            JOIN chat_conversation_memberships membership
              ON membership.conversation_id = authorization.destination_conversation_id
             AND membership.participant_id = assignment.teammate_id
             AND membership.removed_at IS NULL
            JOIN chat_teammate_channel_memberships channel_access
              ON channel_access.conversation_id = membership.conversation_id
             AND channel_access.teammate_id = membership.participant_id
            JOIN chat_teammate_access_state access_state
              ON access_state.teammate_id = membership.participant_id
            JOIN chat_access_profiles profile
              ON profile.id = channel_access.access_profile_id
            JOIN chat_access_profile_revisions profile_revision
              ON profile_revision.access_profile_id = profile.id
             AND profile_revision.revision = profile.latest_revision
            JOIN chat_teammate_working_folder_grants live_grant
              ON live_grant.conversation_id = authorization.destination_conversation_id
             AND live_grant.teammate_id = assignment.teammate_id
             AND live_grant.working_folder_id = source.working_folder_id
             AND live_grant.revoked_at IS NULL
            WHERE source.authorization_revision_id = ?
              AND source.root_handle = ?
              AND source.working_folder_id = ?
              AND source.capability = ?
              AND source.is_execution_target = ?
              AND (
                coalesce(live_grant.runtime_approval_policy,
                         channel_access.runtime_approval_policy,
                         access_state.runtime_approval_policy) = ?
                OR CASE coalesce(live_grant.runtime_approval_policy,
                                 channel_access.runtime_approval_policy,
                                 access_state.runtime_approval_policy)
                     WHEN 'ask' THEN 0 WHEN 'auto_approve' THEN 1
                     WHEN 'unattended' THEN 2 ELSE -1
                   END >= CASE ?
                     WHEN 'ask' THEN 0 WHEN 'auto_approve' THEN 1
                     WHEN 'unattended' THEN 2 ELSE 3
                   END
              )
              AND CASE source.capability
                    WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                    WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 5
                  END <= CASE
                    WHEN live_grant.capability_inherits_profile = 1 THEN
                      CASE profile_revision.maximum_folder_capability
                        WHEN 'none' THEN 0 WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                        WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 0
                      END
                    ELSE min(
                      CASE live_grant.capability
                        WHEN 'none' THEN 0 WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                        WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 0
                      END,
                      CASE profile_revision.maximum_folder_capability
                        WHEN 'none' THEN 0 WHEN 'read' THEN 1 WHEN 'edit' THEN 2
                        WHEN 'execute' THEN 3 WHEN 'publish' THEN 4 ELSE 0
                      END
                    )
                  END
        )",
    )
    .bind(scope.authorization_revision_id.as_str())
    .bind(&source.root_handle)
    .bind(source.working_folder_id.as_str())
    .bind(capability)
    .bind(source.is_execution_target)
    .bind(wire_runtime_approval_policy(source.runtime_approval_policy))
    .bind(wire_runtime_approval_policy(source.runtime_approval_policy))
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if valid {
        Ok(())
    } else {
        Err(generic_denial_error())
    }
}

fn wire_runtime_approval_policy(policy: ChatRuntimeApprovalPolicy) -> &'static str {
    match policy {
        ChatRuntimeApprovalPolicy::Ask => "ask",
        ChatRuntimeApprovalPolicy::AutoApprove => "auto_approve",
        ChatRuntimeApprovalPolicy::Unattended => "unattended",
        ChatRuntimeApprovalPolicy::ProviderCustom => "provider_custom",
    }
}
