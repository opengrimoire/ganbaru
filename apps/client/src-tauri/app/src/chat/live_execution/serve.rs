//! The desktop side: reads of the work this device executes, served on its sync listeners to the
//! linked devices the selected log admits.

use ganbaru_chat::repository::live_execution::{
    LiveExecutionBudget, LiveExecutionOutcome, LiveExecutionQuery, read_live_execution,
};
use ganbaru_handoff::protocol::chat_live_payload_bytes;
use ganbaru_sync_replica::serve::live::{
    ChatLiveAnswer, ChatLivePage, ChatLiveQuery, ChatLiveReader, ChatLiveRoutes,
};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::watch;

/// The encoded payload one live page may carry, kept under the protocol bound so the frame
/// around it always fits.
const CHAT_LIVE_PAGE_BYTES: usize = 600 * 1024;
const _: () =
    assert!(CHAT_LIVE_PAGE_BYTES < ganbaru_handoff::protocol::MAX_CHAT_LIVE_PAYLOAD_BYTES);

/// A counter that moves after every committed Chat change, so waiting live reads wake.
pub(crate) struct ChatLiveChanges(watch::Sender<u64>);

impl Default for ChatLiveChanges {
    fn default() -> Self {
        Self(watch::Sender::new(0))
    }
}

/// Wakes the live reads waiting on this device after a committed Chat change.
pub(crate) fn wake<R: Runtime>(app: &AppHandle<R>) {
    if let Some(changes) = app.try_state::<ChatLiveChanges>() {
        changes
            .0
            .send_modify(|count| *count = count.wrapping_add(1));
    }
}

/// The live reads a sync listener of this desktop answers. Admission is read fresh for every
/// read from the selected log and this installation's quarantine.
pub(crate) fn chat_live_routes<R: Runtime>(app: &AppHandle<R>) -> ChatLiveRoutes {
    let changes = app.state::<ChatLiveChanges>().0.subscribe();
    ChatLiveRoutes::new(
        crate::sync::member_admission_source(app),
        live_reader(app.clone()),
        changes,
    )
}

fn live_reader<R: Runtime>(app: AppHandle<R>) -> ChatLiveReader {
    Arc::new(move |query: ChatLiveQuery| {
        let app = app.clone();
        Box::pin(async move { read_page(&app, query).await })
    })
}

/// Reads one page of the work `query` names, when this device executes it.
async fn read_page<R: Runtime>(
    app: &AppHandle<R>,
    query: ChatLiveQuery,
) -> Result<ChatLiveAnswer, String> {
    let own_device_id = crate::sync::devices::stable_device_id(app)?;
    let pool = crate::db::connect_active_vault_for_sync(app).await?;
    let outcome = read_live_execution(
        &pool,
        LiveExecutionQuery {
            assignment_id: &query.assignment_id,
            own_device_id: &own_device_id,
            after_change: query.after_change,
        },
        LiveExecutionBudget {
            payload_bytes: CHAT_LIVE_PAGE_BYTES,
            measure: chat_live_payload_bytes,
        },
    )
    .await
    .map_err(|error| format!("read the live execution: {:?}", error.code))?;
    let read = match outcome {
        LiveExecutionOutcome::NotHere => return Ok(ChatLiveAnswer::NotHere),
        LiveExecutionOutcome::Read(read) => read,
    };
    let payload = serde_json::to_string(&read)
        .map_err(|error| format!("encode the live execution: {error}"))?;
    Ok(ChatLiveAnswer::Page(ChatLivePage {
        payload,
        change_cursor: read.change_cursor,
        has_more: read.has_more,
        reset_required: read.reset_required,
        settled: read.settled,
    }))
}
