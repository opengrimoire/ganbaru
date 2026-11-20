//! The desktop side of approvals answered from a linked device: the answer resolves the open
//! approval of the newest run of a work assignment this device executes, through the same path
//! as the desktop approval panel.

use crate::chat::send_commands::ResolveChatApprovalCommand;
use ganbaru_chat_contracts::models::{
    ApprovalDecision, ApprovalDecisionKind, ChatCommandContext, ChatCommandId, ChatErrorCode,
    ChatRequestId, ChatThreadId, ProviderRequestId,
};
use ganbaru_handoff::protocol::ChatRemoteApprovalDecision;
use ganbaru_sync_replica::serve::live::{
    ChatApprovalAnswer, ChatApprovalQuery, ChatApprovalResolver,
};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};

/// Prefix that keeps receipts of answers from linked devices apart from desktop commands.
const REMOTE_APPROVAL_COMMAND_PREFIX: &str = "remote-approval:";

/// The resolver the sync listeners use for approvals answered from linked devices. It is managed
/// state because resolving needs the desktop runtime handle, while the listeners are generic.
pub(crate) struct RemoteApprovals(ChatApprovalResolver);

impl RemoteApprovals {
    /// Resolves approvals through the Chat runtime of `app`.
    pub(crate) fn new(app: AppHandle) -> Self {
        Self(Arc::new(move |query: ChatApprovalQuery| {
            let app = app.clone();
            Box::pin(async move { resolve(&app, query).await })
        }))
    }
}

/// The resolver for the listeners of `app`, which answers unavailable until the desktop runtime
/// has registered [`RemoteApprovals`].
pub(super) fn approval_resolver<R: Runtime>(app: &AppHandle<R>) -> ChatApprovalResolver {
    let app = app.clone();
    Arc::new(move |query: ChatApprovalQuery| {
        let resolver = app
            .try_state::<RemoteApprovals>()
            .map(|approvals| approvals.0.clone());
        Box::pin(async move {
            match resolver {
                Some(resolver) => resolver(query).await,
                None => Err("the Chat runtime is not ready".to_string()),
            }
        })
    })
}

/// The open approval a remote answer names, as the desktop panel would address it.
struct OpenApproval {
    thread_id: String,
    provider_request_id: String,
}

async fn resolve(app: &AppHandle, query: ChatApprovalQuery) -> Result<ChatApprovalAnswer, String> {
    let own_device_id = crate::sync::devices::stable_device_id(app)?;
    let pool = crate::db::connect_active_vault_for_sync(app).await?;
    let executing_device_id: Option<Option<String>> =
        sqlx::query_scalar("SELECT executing_device_id FROM chat_work_assignments WHERE id = ?")
            .bind(&query.assignment_id)
            .fetch_optional(&pool)
            .await
            .map_err(|error| format!("read the work assignment: {error}"))?;
    if executing_device_id.flatten().as_deref() != Some(own_device_id.as_str()) {
        return Ok(ChatApprovalAnswer::NotHere);
    }
    let Some(approval) = open_approval(&pool, &query).await? else {
        return Ok(ChatApprovalAnswer::Stale);
    };
    let command = resolve_command(&query, approval)?;
    let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
    match crate::chat::send::resolve_approval(app.clone(), db_url, command).await {
        Ok(_) => Ok(ChatApprovalAnswer::Resolved),
        Err(error) if error.code == ChatErrorCode::Conflict => Ok(ChatApprovalAnswer::Stale),
        Err(error) => Err(format!(
            "resolve the approval: {:?}: {}",
            error.code, error.message
        )),
    }
}

/// Finds the approval `query` names, open on the thread of the newest run of its assignment.
async fn open_approval(
    pool: &sqlx::SqlitePool,
    query: &ChatApprovalQuery,
) -> Result<Option<OpenApproval>, String> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT p.thread_id, p.provider_request_id
         FROM chat_pending_requests p
         JOIN chat_agent_runs r ON r.provider_thread_id = p.thread_id
         WHERE r.assignment_id = ?
           AND r.run_ordinal = (
             SELECT MAX(run_ordinal) FROM chat_agent_runs WHERE assignment_id = ?
           )
           AND p.id = ?
           AND p.request_kind = 'approval'
           AND p.resolution_state = 'open'
         LIMIT 1",
    )
    .bind(&query.assignment_id)
    .bind(&query.assignment_id)
    .bind(&query.request_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("read the open approval: {error}"))?;
    Ok(row.map(|(thread_id, provider_request_id)| OpenApproval {
        thread_id,
        provider_request_id,
    }))
}

/// The desktop approval command for a remote answer. A remote answer never edits the tool input
/// and never names a thread revision, so it resolves exactly what the provider asked.
fn resolve_command(
    query: &ChatApprovalQuery,
    approval: OpenApproval,
) -> Result<ResolveChatApprovalCommand, String> {
    let kind = match query.decision {
        ChatRemoteApprovalDecision::AllowOnce => ApprovalDecisionKind::AllowOnce,
        ChatRemoteApprovalDecision::Deny => ApprovalDecisionKind::Deny,
        ChatRemoteApprovalDecision::Cancel => ApprovalDecisionKind::Cancel,
    };
    Ok(ResolveChatApprovalCommand {
        command: ChatCommandContext {
            client_command_id: ChatCommandId::new(format!(
                "{REMOTE_APPROVAL_COMMAND_PREFIX}{}",
                query.command_id
            ))?,
            expected_thread_revision: None,
        },
        thread_id: ChatThreadId::new(approval.thread_id)?,
        request_id: ChatRequestId::new(query.request_id.clone())?,
        provider_request_id: ProviderRequestId::new(approval.provider_request_id)?,
        decision: ApprovalDecision {
            kind,
            provider_option_id: Some(query.provider_option_id.clone()),
            updated_tool_input: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(decision: ChatRemoteApprovalDecision) -> ChatApprovalQuery {
        ChatApprovalQuery {
            assignment_id: "assignment-1".to_string(),
            request_id: "request-1".to_string(),
            decision,
            provider_option_id: "allow".to_string(),
            command_id: "command-1".to_string(),
        }
    }

    fn approval() -> OpenApproval {
        OpenApproval {
            thread_id: "thread-1".to_string(),
            provider_request_id: "provider-request-1".to_string(),
        }
    }

    #[test]
    fn a_remote_answer_resolves_the_open_request_without_editing_its_input() {
        let command = resolve_command(&query(ChatRemoteApprovalDecision::AllowOnce), approval())
            .expect("command");
        assert_eq!(command.thread_id.as_str(), "thread-1");
        assert_eq!(command.provider_request_id.as_str(), "provider-request-1");
        assert_eq!(command.request_id.as_str(), "request-1");
        assert_eq!(command.decision.kind, ApprovalDecisionKind::AllowOnce);
        assert_eq!(
            command.decision.provider_option_id.as_deref(),
            Some("allow")
        );
        assert!(command.decision.updated_tool_input.is_none());
        assert_eq!(command.command.expected_thread_revision, None);
        assert_eq!(
            command.command.client_command_id.as_str(),
            "remote-approval:command-1"
        );
    }

    #[test]
    fn each_remote_decision_maps_to_the_same_desktop_decision() {
        for (remote, local) in [
            (
                ChatRemoteApprovalDecision::AllowOnce,
                ApprovalDecisionKind::AllowOnce,
            ),
            (ChatRemoteApprovalDecision::Deny, ApprovalDecisionKind::Deny),
            (
                ChatRemoteApprovalDecision::Cancel,
                ApprovalDecisionKind::Cancel,
            ),
        ] {
            let command = resolve_command(&query(remote), approval()).expect("command");
            assert_eq!(command.decision.kind, local);
        }
    }
}
