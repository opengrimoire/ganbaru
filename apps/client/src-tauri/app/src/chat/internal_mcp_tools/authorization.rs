//! Live authorization checks shared by host-tool calls and publication.

use super::{generic_denial_error, persistence_error, wire_folder_capability};
use crate::chat::internal_mcp::{
    InternalMcpChannelSource, InternalMcpFolderSource, InternalMcpRunScope,
};
use ganbaru_chat::coordination::access::{effective_access_level_sql, exceeds_access};
use ganbaru_chat_contracts::models::{ChatAgentAccess, ChatResult};
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
            JOIN chat_agent_channel_memberships channel_access
              ON channel_access.conversation_id = authorization.destination_conversation_id
             AND channel_access.agent_id = assignment.agent_id
             AND channel_access.removed_at IS NULL
            JOIN chat_conversation_memberships membership
              ON membership.conversation_id = channel_access.conversation_id
             AND membership.participant_id = channel_access.agent_id
             AND membership.removed_at IS NULL
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
            JOIN chat_agent_channel_memberships channel_access
              ON channel_access.conversation_id = source.conversation_id
             AND channel_access.agent_id = assignment.agent_id
             AND channel_access.removed_at IS NULL
            JOIN chat_conversation_memberships membership
              ON membership.conversation_id = channel_access.conversation_id
             AND membership.participant_id = channel_access.agent_id
             AND membership.removed_at IS NULL
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
            WHERE source.authorization_revision_id = ?
              AND source.source_handle = ?
              AND source.message_reference_id = ?
              AND source.conversation_id = ?
              AND source.lower_order_key = ?
              AND source.lower_item_id = ?
              AND source.high_order_key = ?
              AND source.high_item_id = ?
              AND source.source_revision_cutoff_id = ?
              AND source.destination_audience_revision = ?
              AND destination_audience.revision >= source.destination_audience_revision
              AND reference_item.conversation_id = authorization.destination_conversation_id
              AND (
                requester_participant.participant_kind != 'ai_agent'
                OR EXISTS (
                  SELECT 1
                  FROM chat_agent_channel_memberships requester_source_access
                  WHERE requester_source_access.conversation_id = source.conversation_id
                    AND requester_source_access.agent_id = authorization.requester_participant_id
                    AND requester_source_access.removed_at IS NULL
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
                    destination_participant.participant_kind != 'ai_agent'
                    OR EXISTS (
                      SELECT 1
                      FROM chat_agent_channel_memberships destination_ai_access
                      WHERE destination_ai_access.conversation_id = authorization.destination_conversation_id
                        AND destination_ai_access.agent_id = destination_member.participant_id
                        AND destination_ai_access.removed_at IS NULL
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
                      destination_participant.participant_kind = 'ai_agent'
                      AND NOT EXISTS (
                        SELECT 1
                        FROM chat_agent_channel_memberships source_ai_access
                        WHERE source_ai_access.conversation_id = source.conversation_id
                          AND source_ai_access.agent_id = destination_member.participant_id
                          AND source_ai_access.removed_at IS NULL
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
    .bind(&source.lower_order_key)
    .bind(&source.lower_item_id)
    .bind(&source.high_order_key)
    .bind(&source.high_item_id)
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

/// Verifies that a folder source of the run's authorization is still granted and covered.
pub(super) async fn verify_folder_source(
    pool: &SqlitePool,
    scope: &InternalMcpRunScope,
    source: &InternalMcpFolderSource,
) -> ChatResult<()> {
    verify_authorized_folder_source(pool, scope.authorization_revision_id.as_str(), source).await
}

/// Verifies that the folder source `source` of the authorization revision
/// `authorization_revision_id` is still covered: the authorization stays allowed, the agent is
/// still in the destination channel, the folder is a folder of the channel's project that is not
/// archived, and the channel's current access level still allows the frozen capability and
/// approval. The level's ceiling decides, not this device's provider, so every device reaches the
/// same answer.
pub(crate) async fn verify_authorized_folder_source(
    pool: &SqlitePool,
    authorization_revision_id: &str,
    source: &InternalMcpFolderSource,
) -> ChatResult<()> {
    let capability = wire_folder_capability(source.capability);
    let level: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {} AS access_level
            FROM chat_assignment_authorized_folder_sources source
            JOIN chat_assignment_authorization_revisions authorization
              ON authorization.id = source.authorization_revision_id
             AND authorization.decision_state = 'allowed'
             AND authorization.revoked_at IS NULL
            JOIN chat_work_assignments assignment
              ON assignment.id = authorization.assignment_id
            JOIN chat_conversation_memberships membership
              ON membership.conversation_id = authorization.destination_conversation_id
             AND membership.participant_id = assignment.agent_id
             AND membership.removed_at IS NULL
            JOIN chat_agent_channel_memberships channel_access
              ON channel_access.conversation_id = membership.conversation_id
             AND channel_access.agent_id = membership.participant_id
             AND channel_access.removed_at IS NULL
            JOIN chat_conversations conversation
              ON conversation.id = authorization.destination_conversation_id
            JOIN project_working_folders folder
              ON folder.id = source.working_folder_id
             AND folder.project_id = conversation.project_id
             AND folder.archived_at IS NULL
            WHERE source.authorization_revision_id = ?
              AND source.root_handle = ?
              AND source.working_folder_id = ?
              AND source.capability = ?
              AND source.is_execution_target = ?",
        effective_access_level_sql("authorization.destination_conversation_id"),
    )))
    .bind(authorization_revision_id)
    .bind(&source.root_handle)
    .bind(source.working_folder_id.as_str())
    .bind(capability)
    .bind(source.is_execution_target)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let level = level
        .as_deref()
        .and_then(ChatAgentAccess::parse)
        .ok_or_else(generic_denial_error)?;
    if exceeds_access(
        level,
        source.capability,
        source.runtime_approval_policy,
        false,
    ) {
        Err(generic_denial_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::verify_authorized_folder_source;
    use crate::chat::internal_mcp::InternalMcpFolderSource;
    use crate::chat::revocation::tests::pool_with_folder_runs;
    use ganbaru_chat_contracts::models::{
        ChatFolderCapability, ChatResult, ChatRuntimeApprovalPolicy, ProjectWorkingFolderId,
    };
    use sqlx::SqlitePool;

    /// Verifies the working run's execution target, `workspace-1` at execute, as frozen under
    /// `approval`.
    async fn verify_target(
        pool: &SqlitePool,
        approval: ChatRuntimeApprovalPolicy,
    ) -> ChatResult<()> {
        let source = InternalMcpFolderSource {
            root_handle: format!("{:032}", 1),
            working_folder_id: ProjectWorkingFolderId::new("workspace-1").unwrap(),
            capability: ChatFolderCapability::Execute,
            is_execution_target: true,
            runtime_approval_policy: approval,
        };
        verify_authorized_folder_source(pool, "authorization:working", &source).await
    }

    /// Verifies the working run's reference source, the external `workspace-2` at read.
    async fn verify_reference(pool: &SqlitePool) -> ChatResult<()> {
        let source = InternalMcpFolderSource {
            root_handle: format!("{:032}", 2),
            working_folder_id: ProjectWorkingFolderId::new("workspace-2").unwrap(),
            capability: ChatFolderCapability::Read,
            is_execution_target: false,
            runtime_approval_policy: ChatRuntimeApprovalPolicy::Ask,
        };
        verify_authorized_folder_source(pool, "authorization:working", &source).await
    }

    async fn set_channel_access(pool: &SqlitePool, level: Option<&str>) {
        sqlx::query("UPDATE chat_channels SET agent_access = ?")
            .bind(level)
            .execute(pool)
            .await
            .unwrap();
    }

    async fn set_project_access(pool: &SqlitePool, level: &str) {
        sqlx::query(
            "INSERT INTO chat_project_agent_settings
                (project_id, agent_access, created_at, updated_at)
             VALUES ('project-chat', ?, '2026-11-18T00:00:00.000Z', '2026-11-18T00:00:00.000Z')
             ON CONFLICT (project_id) DO UPDATE SET agent_access = excluded.agent_access",
        )
        .bind(level)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_folder_source_stays_covered_while_the_level_allows_it() {
        let pool = pool_with_folder_runs().await;
        verify_target(&pool, ChatRuntimeApprovalPolicy::Ask)
            .await
            .unwrap();
        set_channel_access(&pool, Some("own")).await;
        verify_target(&pool, ChatRuntimeApprovalPolicy::AutoApprove)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn narrowing_the_channel_denies_folder_tool_calls() {
        let pool = pool_with_folder_runs().await;
        set_channel_access(&pool, Some("own")).await;
        verify_target(&pool, ChatRuntimeApprovalPolicy::AutoApprove)
            .await
            .unwrap();
        set_channel_access(&pool, Some("ask")).await;
        assert!(
            verify_target(&pool, ChatRuntimeApprovalPolicy::AutoApprove)
                .await
                .is_err()
        );
        set_channel_access(&pool, Some("read")).await;
        assert!(
            verify_target(&pool, ChatRuntimeApprovalPolicy::Ask)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn narrowing_the_project_or_app_denies_folder_tool_calls() {
        let pool = pool_with_folder_runs().await;
        set_channel_access(&pool, None).await;
        set_project_access(&pool, "talk").await;
        assert!(
            verify_target(&pool, ChatRuntimeApprovalPolicy::Ask)
                .await
                .is_err()
        );

        let pool = pool_with_folder_runs().await;
        set_channel_access(&pool, None).await;
        sqlx::query("UPDATE chat_agent_app_settings SET agent_access = 'read'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            verify_target(&pool, ChatRuntimeApprovalPolicy::Ask)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn an_archived_folder_or_a_removed_agent_denies_folder_tool_calls() {
        let pool = pool_with_folder_runs().await;
        verify_reference(&pool).await.unwrap();
        sqlx::query(
            "UPDATE project_working_folders SET archived_at = '2026-11-18T00:00:00.000Z'
             WHERE id = 'workspace-2'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(verify_reference(&pool).await.is_err());

        let pool = pool_with_folder_runs().await;
        sqlx::query(
            "UPDATE chat_agent_channel_memberships SET removed_at = '2026-11-18T00:00:00.000Z'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            verify_target(&pool, ChatRuntimeApprovalPolicy::Ask)
                .await
                .is_err()
        );
    }
}
