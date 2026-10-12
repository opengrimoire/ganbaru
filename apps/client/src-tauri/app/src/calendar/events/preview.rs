//! Calendar edit preview command adapter.

use std::sync::Arc;

use ganbaru_calendar::events::preview::{
    PreviewHost, PreviewRequest, PreviewResponse, ReviewContext,
};
use ganbaru_calendar::events::scope::DeviceDate;
use sqlx::SqlitePool;

/// Binds a preview to the active vault, its authorized database, and the Focus owner clock.
#[derive(Clone)]
struct AppPreviewHost {
    app: tauri::AppHandle,
    db_url: String,
}

impl PreviewHost for AppPreviewHost {
    async fn review_context(&self) -> Result<ReviewContext, String> {
        let (vault_id, vault_generation, clock_ms) =
            crate::pomodoro::native_runtime::calendar_review_context(&self.app).await?;
        Ok(ReviewContext {
            vault_id,
            vault_generation,
            clock_ms,
        })
    }

    async fn connect(&self) -> Result<SqlitePool, String> {
        crate::db::connect_sqlite(self.app.clone(), self.db_url.clone()).await
    }

    fn active_vault_id(&self) -> Result<String, String> {
        crate::vault::active_vault_id(&self.app)
    }

    fn device_date(&self) -> Arc<dyn DeviceDate> {
        crate::calendar::device_date::source(&self.app)
    }
}

/// Preview retains no write transaction and never persists its prepared rows.
#[tauri::command]
pub(crate) async fn calendar_preview_edit(
    app: tauri::AppHandle,
    db_url: String,
    request: PreviewRequest,
) -> Result<PreviewResponse, String> {
    ganbaru_calendar::events::preview::preview_edit(AppPreviewHost { app, db_url }, request).await
}
