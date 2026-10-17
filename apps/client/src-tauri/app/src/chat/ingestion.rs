//! Tauri change delivery for the core Chat event ingestor.

use ganbaru_chat_contracts::models::{
    ChatChangeNotification, ChatError, ChatErrorCode, ChatResult,
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
        self.app.emit(CHAT_CHANGE_EVENT, notification).map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Chat change notification delivery failed",
                true,
            )
        })
    }
}
