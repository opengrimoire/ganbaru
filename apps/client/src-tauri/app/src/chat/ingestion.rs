//! Tauri change delivery for the core Chat event ingestor.

use ganbaru_chat_contracts::models::{
    ChatChangeNotification, ChatError, ChatErrorCode, ChatProjectionKey, ChatResult,
};
use tauri::{Emitter, Runtime};

pub use ganbaru_chat::ingestion::*;

pub struct TauriChatChangeEmitter<R: Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: Runtime> TauriChatChangeEmitter<R> {
    pub fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> ChatChangeEmitter for TauriChatChangeEmitter<R> {
    fn emit(&self, notification: &ChatChangeNotification) -> ChatResult<()> {
        if notification
            .changed_projection_keys
            .contains(&ChatProjectionKey::Organizational)
        {
            super::usage_summaries::wake(&self.app);
        }
        self.app.emit(CHAT_CHANGE_EVENT, notification).map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Chat change notification delivery failed",
                true,
            )
        })
    }
}

/// Tells views that a committed write changed a thread's rows without an event. Delivery is best
/// effort: the write already committed, and the next change or reload shows it.
pub(crate) async fn notify_rows_changed<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &sqlx::SqlitePool,
    thread_id: &ganbaru_chat_contracts::models::ChatThreadId,
    keys: &[ganbaru_chat_contracts::models::ChatProjectionKey],
) {
    let notification =
        ganbaru_chat::repository::reads::read_change_notification(pool, thread_id, keys.to_vec())
            .await;
    let delivered = notification
        .and_then(|notification| TauriChatChangeEmitter::new(app.clone()).emit(&notification));
    if let Err(error) = delivered {
        eprintln!("Chat change notification failed with code {:?}", error.code);
    }
}
