//! Retained-reference disclosure checks for access previews and transactions.

use super::{
    ChatAccessIssueCode, ChatAccessProfileId, ChatAccessProfileRevisionId,
    ChatAccessValidationIssue, ChatChannelCapabilities, ChatChannelId, ChatConversationId,
    ChatHistoryBoundary, ChatResult,
};
use super::{ResolvedChannelAccess, history_boundary_is_expansion, issue};
use crate::chat::channels::{identifier_error, persistence_error, u64_value};
use crate::chat::organization::common::{
    from_grant_lower_order_key, parse_folder_capability, parse_history_boundary,
    parse_runtime_approval_policy,
};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::collections::BTreeMap;

pub(super) async fn retained_reference_issues(
    pool: &SqlitePool,
    proposed: &[ResolvedChannelAccess],
) -> ChatResult<Vec<ChatAccessValidationIssue>> {
    let source_access = proposed_source_access(proposed);
    let mut issues = Vec::new();
    for (index, destination) in proposed.iter().enumerate() {
        if !destination.capabilities.read_history {
            continue;
        }
        let references = sqlx::query(
            "SELECT DISTINCT target.source_conversation_id,
                    target.source_lower_order_key,
                    COALESCE(root_item.order_key, item.order_key) AS destination_root_order_key
             FROM chat_channel_reference_targets target
             JOIN chat_message_references reference
               ON reference.id = target.reference_id
             JOIN chat_communication_message_revisions revision
               ON revision.id = reference.message_revision_id
             JOIN chat_communication_messages message
               ON message.item_id = revision.message_item_id
             JOIN chat_conversation_items item
               ON item.id = message.item_id
             LEFT JOIN chat_reply_threads reply_thread
               ON reply_thread.id = item.reply_thread_id
             LEFT JOIN chat_conversation_items root_item
               ON root_item.id = reply_thread.root_item_id
             WHERE target.destination_conversation_id = ?
               AND message.deleted_at IS NULL
               AND message.current_revision_id = revision.id",
        )
        .bind(destination.conversation_id.as_str())
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
        for reference in references {
            let source_conversation_id: String = reference
                .try_get("source_conversation_id")
                .map_err(persistence_error)?;
            let Some(source_lower_order_key) = reference
                .try_get::<Option<String>, _>("source_lower_order_key")
                .map_err(persistence_error)?
            else {
                continue;
            };
            let destination_root_order_key: String = reference
                .try_get("destination_root_order_key")
                .map_err(persistence_error)?;
            if !retained_reference_is_visible(
                &destination.history_boundary,
                &destination_root_order_key,
            ) {
                continue;
            }
            if source_conversation_id != destination.conversation_id.as_str()
                && !can_read_retained_source(
                    source_access.get(source_conversation_id.as_str()),
                    &source_lower_order_key,
                )
            {
                issues.push(issue(
                    ChatAccessIssueCode::RetainedReferenceDisclosure,
                    &format!("channels[{index}].historyBoundary"),
                    "Use From access grant because earlier channel references are not readable by this teammate",
                ));
                break;
            }
        }
    }
    Ok(issues)
}

pub(super) async fn retained_reference_issues_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    proposed: &[ResolvedChannelAccess],
) -> ChatResult<Vec<ChatAccessValidationIssue>> {
    let source_access = proposed_source_access(proposed);
    let mut issues = Vec::new();
    for (index, destination) in proposed.iter().enumerate() {
        if !destination.capabilities.read_history {
            continue;
        }
        let references = sqlx::query(
            "SELECT DISTINCT target.source_conversation_id,
                    target.source_lower_order_key,
                    COALESCE(root_item.order_key, item.order_key) AS destination_root_order_key
             FROM chat_channel_reference_targets target
             JOIN chat_message_references reference
               ON reference.id = target.reference_id
             JOIN chat_communication_message_revisions revision
               ON revision.id = reference.message_revision_id
             JOIN chat_communication_messages message
               ON message.item_id = revision.message_item_id
             JOIN chat_conversation_items item
               ON item.id = message.item_id
             LEFT JOIN chat_reply_threads reply_thread
               ON reply_thread.id = item.reply_thread_id
             LEFT JOIN chat_conversation_items root_item
               ON root_item.id = reply_thread.root_item_id
             WHERE target.destination_conversation_id = ?
               AND message.deleted_at IS NULL
               AND message.current_revision_id = revision.id",
        )
        .bind(destination.conversation_id.as_str())
        .fetch_all(&mut **transaction)
        .await
        .map_err(persistence_error)?;
        for reference in references {
            let source_conversation_id: String = reference
                .try_get("source_conversation_id")
                .map_err(persistence_error)?;
            let Some(source_lower_order_key) = reference
                .try_get::<Option<String>, _>("source_lower_order_key")
                .map_err(persistence_error)?
            else {
                continue;
            };
            let destination_root_order_key: String = reference
                .try_get("destination_root_order_key")
                .map_err(persistence_error)?;
            if !retained_reference_is_visible(
                &destination.history_boundary,
                &destination_root_order_key,
            ) {
                continue;
            }
            if source_conversation_id != destination.conversation_id.as_str()
                && !can_read_retained_source(
                    source_access.get(source_conversation_id.as_str()),
                    &source_lower_order_key,
                )
            {
                issues.push(issue(
                    ChatAccessIssueCode::RetainedReferenceDisclosure,
                    &format!("channels[{index}].historyBoundary"),
                    "Use From access grant because earlier channel references are not readable by this teammate",
                ));
                break;
            }
        }
    }
    Ok(issues)
}

pub(super) async fn retained_profile_reference_issues_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    access_profile_id: &ChatAccessProfileId,
) -> ChatResult<Vec<ChatAccessValidationIssue>> {
    let teammate_ids = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT ai.teammate_id
         FROM chat_teammate_channel_memberships ai
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = ai.conversation_id
          AND membership.participant_id = ai.teammate_id
         WHERE ai.access_profile_id = ? AND ai.removed_at IS NULL AND membership.removed_at IS NULL
         ORDER BY ai.teammate_id",
    )
    .bind(access_profile_id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    for teammate_id in teammate_ids {
        let resolved =
            read_effective_disclosure_access_in_transaction(transaction, teammate_id.as_str())
                .await?;
        if !retained_reference_issues_in_transaction(transaction, &resolved)
            .await?
            .is_empty()
        {
            return Ok(vec![issue(
                ChatAccessIssueCode::RetainedReferenceDisclosure,
                "revision.defaultHistoryBoundary",
                "This profile expansion would expose retained channel references to a teammate without source access",
            )]);
        }
    }
    Ok(Vec::new())
}

async fn read_effective_disclosure_access_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    teammate_id: &str,
) -> ChatResult<Vec<ResolvedChannelAccess>> {
    let rows = sqlx::query(
        "SELECT channel.id AS channel_id, channel.project_id, project.group_id,
                channel.conversation_id, channel.name AS channel_name,
                ai.access_profile_id, profile_revision.id AS access_profile_revision_id,
                profile_revision.revision AS access_profile_revision,
                profile_revision.default_read_history AS profile_read_history,
                profile_revision.default_participate AS profile_participate,
                profile_revision.default_history_boundary AS profile_history_boundary,
                profile_revision.maximum_folder_capability AS profile_folder_capability,
                ai.read_history, ai.read_history_inherits_profile,
                ai.participate, ai.participate_inherits_profile,
                ai.history_boundary, ai.history_boundary_inherits_profile,
                ai.history_from_order_key, ai.runtime_approval_policy,
                ai.scratch_runtime_approval_policy
         FROM chat_teammate_channel_memberships ai
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = ai.conversation_id
          AND membership.participant_id = ai.teammate_id
         JOIN chat_channels channel ON channel.conversation_id = ai.conversation_id
         JOIN projects project ON project.id = channel.project_id
         JOIN chat_access_profiles profile ON profile.id = ai.access_profile_id
         JOIN chat_access_profile_effective_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
         WHERE ai.teammate_id = ? AND ai.removed_at IS NULL AND membership.removed_at IS NULL
         ORDER BY project.group_id, channel.project_id, channel.name COLLATE NOCASE, channel.id",
    )
    .bind(teammate_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let mut resolved = Vec::with_capacity(rows.len());
    for row in rows {
        let conversation_id = ChatConversationId::new(
            row.try_get::<String, _>("conversation_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        let profile_history_boundary = parse_history_boundary(
            &row.try_get::<String, _>("profile_history_boundary")
                .map_err(persistence_error)?,
            None,
        )?;
        let stored_history_boundary = parse_history_boundary(
            &row.try_get::<String, _>("history_boundary")
                .map_err(persistence_error)?,
            row.try_get("history_from_order_key")
                .map_err(persistence_error)?,
        )?;
        let history_boundary = if row
            .try_get::<i64, _>("history_boundary_inherits_profile")
            .map_err(persistence_error)?
            != 0
        {
            match profile_history_boundary {
                ChatHistoryBoundary::Entire => ChatHistoryBoundary::Entire,
                ChatHistoryBoundary::FromGrant { .. } => ChatHistoryBoundary::FromGrant {
                    lower_order_key: match stored_history_boundary {
                        ChatHistoryBoundary::FromGrant { lower_order_key } => lower_order_key,
                        ChatHistoryBoundary::Entire => None,
                    },
                },
            }
        } else if history_boundary_is_expansion(&profile_history_boundary, &stored_history_boundary)
        {
            ChatHistoryBoundary::FromGrant {
                lower_order_key: Some(
                    from_grant_lower_order_key(transaction, conversation_id.as_str()).await?,
                ),
            }
        } else {
            stored_history_boundary
        };
        let profile_capabilities = ChatChannelCapabilities {
            read_history: row
                .try_get::<i64, _>("profile_read_history")
                .map_err(persistence_error)?
                != 0,
            participate: row
                .try_get::<i64, _>("profile_participate")
                .map_err(persistence_error)?
                != 0,
        };
        let capabilities = ChatChannelCapabilities {
            read_history: if row
                .try_get::<i64, _>("read_history_inherits_profile")
                .map_err(persistence_error)?
                != 0
            {
                profile_capabilities.read_history
            } else {
                row.try_get::<i64, _>("read_history")
                    .map_err(persistence_error)?
                    != 0
                    && profile_capabilities.read_history
            },
            participate: if row
                .try_get::<i64, _>("participate_inherits_profile")
                .map_err(persistence_error)?
                != 0
            {
                profile_capabilities.participate
            } else {
                row.try_get::<i64, _>("participate")
                    .map_err(persistence_error)?
                    != 0
                    && profile_capabilities.participate
            },
        };
        resolved.push(ResolvedChannelAccess {
            channel_id: ChatChannelId::new(
                row.try_get::<String, _>("channel_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            project_id: row.try_get("project_id").map_err(persistence_error)?,
            group_id: row.try_get("group_id").map_err(persistence_error)?,
            conversation_id,
            channel_name: row.try_get("channel_name").map_err(persistence_error)?,
            access_profile_id: ChatAccessProfileId::new(
                row.try_get::<String, _>("access_profile_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            access_profile_revision_id: ChatAccessProfileRevisionId::new(
                row.try_get::<String, _>("access_profile_revision_id")
                    .map_err(persistence_error)?,
            )
            .map_err(identifier_error)?,
            access_profile_revision: u64_value(
                row.try_get("access_profile_revision")
                    .map_err(persistence_error)?,
            )?,
            profile_default_channel_capabilities: profile_capabilities,
            profile_default_history_boundary: profile_history_boundary,
            profile_maximum_folder_capability: parse_folder_capability(
                &row.try_get::<String, _>("profile_folder_capability")
                    .map_err(persistence_error)?,
            )?,
            capabilities,
            history_boundary,
            runtime_approval_override: row
                .try_get::<Option<String>, _>("runtime_approval_policy")
                .map_err(persistence_error)?
                .as_deref()
                .map(parse_runtime_approval_policy)
                .transpose()?,
            scratch_runtime_approval_override: row
                .try_get::<Option<String>, _>("scratch_runtime_approval_policy")
                .map_err(persistence_error)?
                .as_deref()
                .map(parse_runtime_approval_policy)
                .transpose()?,
            folder_grants: Vec::new(),
        });
    }
    Ok(resolved)
}

fn proposed_source_access(
    proposed: &[ResolvedChannelAccess],
) -> BTreeMap<&str, (&ChatChannelCapabilities, &ChatHistoryBoundary)> {
    proposed
        .iter()
        .map(|channel| {
            (
                channel.conversation_id.as_str(),
                (&channel.capabilities, &channel.history_boundary),
            )
        })
        .collect()
}

fn can_read_retained_source(
    source: Option<&(&ChatChannelCapabilities, &ChatHistoryBoundary)>,
    source_lower_order_key: &str,
) -> bool {
    let Some((capabilities, boundary)) = source else {
        return false;
    };
    if !capabilities.read_history {
        return false;
    }
    match boundary {
        ChatHistoryBoundary::Entire => true,
        ChatHistoryBoundary::FromGrant { lower_order_key } => lower_order_key
            .as_deref()
            .is_some_and(|lower_order_key| lower_order_key <= source_lower_order_key),
    }
}

fn retained_reference_is_visible(
    destination_boundary: &ChatHistoryBoundary,
    destination_root_order_key: &str,
) -> bool {
    match destination_boundary {
        ChatHistoryBoundary::Entire => true,
        ChatHistoryBoundary::FromGrant { lower_order_key } => lower_order_key
            .as_deref()
            .is_none_or(|lower_order_key| lower_order_key <= destination_root_order_key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_grant_exposes_only_references_at_or_after_its_root_boundary() {
        let boundary = ChatHistoryBoundary::FromGrant {
            lower_order_key: Some("c012".to_owned()),
        };
        assert!(!retained_reference_is_visible(&boundary, "c011"));
        assert!(retained_reference_is_visible(&boundary, "c012"));
        assert!(retained_reference_is_visible(&boundary, "c012V"));
        assert!(retained_reference_is_visible(
            &ChatHistoryBoundary::Entire,
            "c000",
        ));
    }

    #[test]
    fn unresolved_from_grant_boundary_fails_closed_for_retained_references() {
        assert!(retained_reference_is_visible(
            &ChatHistoryBoundary::FromGrant {
                lower_order_key: None,
            },
            "c000",
        ));
    }
}
