//! Retained-reference disclosure checks for access previews and transactions.

use super::{
    ChatAccessIssueCode, ChatAccessValidationIssue, ChatChannelCapabilities, ChatHistoryBoundary,
    ChatResult,
};
use super::{ResolvedChannelAccess, issue};
use crate::chat::channels::persistence_error;
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
                    "Use From access grant because earlier channel references are not readable by this agent",
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
                    "Use From access grant because earlier channel references are not readable by this agent",
                ));
                break;
            }
        }
    }
    Ok(issues)
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
