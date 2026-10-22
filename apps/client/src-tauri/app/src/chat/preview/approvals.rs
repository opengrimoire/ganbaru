//! Approvals the model waits on in the Browser panel: loopback origins a thread may navigate
//! to, and preview tool use in a session whose provider sandbox denies network access.

use super::model_access::{grant_storage_error, insert_grant, thread_grants};
use super::navigation::{loopback_approval_needed, loopback_origin_key, validate_navigation};
use super::webview::{navigate_existing, owned_tab};
use super::*;
use crate::chat::device_state::update_active_device_scope;
use std::collections::BTreeSet;
use tauri::Emitter;

/// Tells the Browser panel that a thread has a new pending approval.
pub const CHAT_BROWSER_APPROVAL_EVENT: &str = "chat://browser-approval";

/// The one approval a thread may have waiting. A newer request replaces an older one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PendingPreviewApproval {
    /// A tab tried to reach a loopback origin the thread has not allowed. The URL is the
    /// navigation target, held in Rust so the webview cannot change what the user approves.
    Navigation { tab_id: String, url: reqwest::Url },
    /// The model used preview tools in a session whose provider sandbox denies network access.
    Network,
}

/// What the Browser panel shows for a pending approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PreviewApprovalRead {
    Navigation {
        origin: String,
    },
    Network,
    /// A page asked for a capability. The user's answer is remembered per thread and origin.
    #[serde(rename_all = "camelCase")]
    Permission {
        request_id: u64,
        origin: String,
        permission: page_requests::PagePermission,
    },
    /// A page opened `alert`, `confirm`, `prompt`, or a leave-page confirmation.
    #[serde(rename_all = "camelCase")]
    Dialog {
        request_id: u64,
        origin: String,
        dialog_kind: page_requests::PageDialogKind,
        message: String,
        default_text: String,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewApprovalNotice<'a> {
    pub(super) thread_id: &'a ChatThreadId,
}

/// Outcome of checking a model navigation against the thread's loopback allowlist.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ModelNavigation {
    Ready(reqwest::Url),
    NeedsApproval { url: reqwest::Url, origin: String },
}

/// Validates a model navigation and reports whether its loopback origin still needs approval.
/// The model may only open loopback pages, so external URLs fail validation here.
pub(super) fn model_navigation(
    value: &str,
    loopback_grants: &BTreeSet<String>,
) -> ChatResult<ModelNavigation> {
    let url = validate_navigation(value, false)?;
    Ok(match loopback_approval_needed(&url, loopback_grants) {
        Some(origin) => ModelNavigation::NeedsApproval { url, origin },
        None => ModelNavigation::Ready(url),
    })
}

/// Tells the model that the user has to approve something in the Browser panel first.
pub(crate) fn approval_pending_error(subject: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        format!(
            "{subject} is waiting for the user to approve it in the Browser panel. Ask the user to approve it there, then try again"
        ),
        true,
    )
}

fn approval_read(approval: &PendingPreviewApproval) -> Option<PreviewApprovalRead> {
    match approval {
        PendingPreviewApproval::Navigation { url, .. } => {
            loopback_origin_key(url).map(|origin| PreviewApprovalRead::Navigation { origin })
        }
        PendingPreviewApproval::Network => Some(PreviewApprovalRead::Network),
    }
}

impl ChatBrowserManager {
    /// Records a pending approval and notifies the Browser panel. Repeating the same request,
    /// as subframes of one page do, neither replaces it nor emits again.
    pub(crate) fn request_approval<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        thread_id: &ChatThreadId,
        approval: PendingPreviewApproval,
    ) {
        let Ok(mut pending) = self.pending_approvals.lock() else {
            return;
        };
        if pending.get(thread_id) == Some(&approval) {
            return;
        }
        pending.insert(thread_id.clone(), approval);
        drop(pending);
        let _ = app.emit(
            CHAT_BROWSER_APPROVAL_EVENT,
            PreviewApprovalNotice { thread_id },
        );
    }

    fn take_approval(
        &self,
        thread_id: &ChatThreadId,
    ) -> ChatResult<Option<PendingPreviewApproval>> {
        Ok(self
            .pending_approvals
            .lock()
            .map_err(|_| browser_state_error())?
            .remove(thread_id))
    }
}

/// Lets the thread navigate to a loopback origin. Non-loopback URLs need no grant here.
pub(super) fn grant_loopback_origin(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
    url: &reqwest::Url,
) -> ChatResult<()> {
    let Some(origin) = loopback_origin_key(url) else {
        return Ok(());
    };
    if thread_grants(app, thread_id).contains(&origin) {
        return Ok(());
    }
    let mut outcome = Ok(());
    update_active_device_scope(app, |scope| {
        outcome = insert_grant(&mut scope.preview_model_origins, thread_id.as_str(), origin);
        Ok(())
    })
    .map_err(|_| grant_storage_error())?;
    outcome
}

/// Reads the thread's oldest pending page request, or else its pending approval.
#[tauri::command]
pub async fn chat_browser_pending_approval(
    app: tauri::AppHandle,
    thread_id: ChatThreadId,
) -> ChatResult<Option<PreviewApprovalRead>> {
    let manager = app.state::<ChatBrowserManager>();
    if let Some(request) = manager.pending_page_request(&thread_id)? {
        return Ok(Some(request));
    }
    Ok(manager
        .pending_approvals
        .lock()
        .map_err(|_| browser_state_error())?
        .get(&thread_id)
        .and_then(approval_read))
}

/// Answers a page request when `request_id` is given, otherwise allows or dismisses the
/// thread's pending approval. Allowing a navigation remembers its loopback origin for the
/// thread and opens the held URL in the tab that asked for it. `text` is what an accepted
/// `prompt` dialog returns to the page.
#[tauri::command]
pub async fn chat_browser_resolve_approval(
    app: tauri::AppHandle,
    db_url: String,
    thread_id: ChatThreadId,
    allow: bool,
    request_id: Option<u64>,
    text: Option<String>,
) -> ChatResult<Option<BrowserTabRead>> {
    if let Some(request_id) = request_id {
        page_requests::resolve_page_request(&app, &thread_id, request_id, allow, text)?;
        return Ok(None);
    }
    let Some(approval) = app
        .state::<ChatBrowserManager>()
        .take_approval(&thread_id)?
    else {
        return Ok(None);
    };
    if !allow {
        return Ok(None);
    }
    match approval {
        PendingPreviewApproval::Network => {
            app.state::<crate::chat::internal_mcp::InternalMcpRegistry>()
                .approve_preview_network(&thread_id)
                .await;
            Ok(None)
        }
        PendingPreviewApproval::Navigation { tab_id, url } => {
            grant_loopback_origin(&app, &thread_id, &url)?;
            let tab = match owned_tab(&app, &thread_id, &tab_id) {
                Ok(tab) => tab,
                Err(error) if error.code == ChatErrorCode::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            navigate_existing(&app, &tab, url.clone())?;
            let pool = chat_pool(&app, db_url).await?;
            navigated_read(&app, &pool, &tab_id, &url).await.map(Some)
        }
    }
}
