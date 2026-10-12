use super::*;

pub(super) async fn persist_browser_artifact(
    app: &tauri::AppHandle,
    db_url: String,
    tab: &RuntimeBrowserTab,
    payload: BrowserArtifactPayload<'_>,
) -> ChatResult<ChatResourceRead> {
    let pool = chat_pool(app, db_url).await?;
    let vault_root = vault::active_writable_vault_path(app).map_err(|_| persistence_error())?;
    persist_browser_artifact_with_pool(&pool, &vault_root, tab, payload).await
}

pub(super) async fn persist_browser_artifact_with_pool(
    pool: &SqlitePool,
    vault_root: &std::path::Path,
    tab: &RuntimeBrowserTab,
    payload: BrowserArtifactPayload<'_>,
) -> ChatResult<ChatResourceRead> {
    let working_folder_id =
        sqlx::query_scalar::<_, String>("SELECT working_folder_id FROM chat_threads WHERE id = ?")
            .bind(tab.read.thread_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(|_| persistence_error())?
            .ok_or_else(|| {
                ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
            })?;
    let working_folder_id =
        ProjectWorkingFolderId::new(working_folder_id).map_err(|_| persistence_error())?;
    let created_at = UtcTimestamp::new(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| persistence_error())?;
    let resource_id = new_artifact_id(&tab.read.thread_id, &tab.read.tab_id);
    resources::store_browser_artifact(
        pool,
        vault_root,
        StoreBrowserArtifact {
            resource_id: &resource_id,
            thread_id: &tab.read.thread_id,
            working_folder_id: &working_folder_id,
            preview_tab_id: &tab.read.tab_id,
            kind: payload.kind,
            display_name: payload.display_name,
            mime_type: payload.mime_type,
            source_url: &tab.read.current_url,
            viewport_width: tab.read.viewport_width,
            viewport_height: tab.read.viewport_height,
            duration_milliseconds: payload.duration_milliseconds,
            frame_count: payload.frame_count,
            created_at: &created_at,
            bytes: payload.bytes,
        },
    )
    .await
}

pub(super) fn new_artifact_id(thread_id: &ChatThreadId, tab_id: &str) -> String {
    let generation = NEXT_ARTIFACT_ID.fetch_add(1, Ordering::Relaxed);
    let seed = format!(
        "{}:{tab_id}:{}:{generation}:{}",
        thread_id.as_str(),
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    format!("browser-{:x}", Sha256::digest(seed.as_bytes()))
}
