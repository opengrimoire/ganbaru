use super::*;

pub(super) async fn persist_tab(pool: &SqlitePool, read: &BrowserTabRead) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_preview_tabs
            (id, thread_id, position, current_url, title, viewport_kind, viewport_width,
             viewport_height, visible, loading_state, created_at, updated_at)
         VALUES (?, ?, (SELECT COALESCE(MAX(position) + 1, 0) FROM chat_preview_tabs WHERE thread_id = ?),
             ?, ?, 'freeform', ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(id) DO UPDATE SET current_url = excluded.current_url, title = excluded.title,
             viewport_width = excluded.viewport_width, viewport_height = excluded.viewport_height,
             visible = excluded.visible, loading_state = excluded.loading_state,
             updated_at = excluded.updated_at",
    )
    .bind(&read.tab_id)
    .bind(read.thread_id.as_str())
    .bind(read.thread_id.as_str())
    .bind(&read.current_url)
    .bind(&read.title)
    .bind(i64::from(read.viewport_width))
    .bind(i64::from(read.viewport_height))
    .bind(read.visible)
    .bind(if read.loading { "loading" } else { "loaded" })
    .execute(pool)
    .await
    .map_err(|_| persistence_error())?;
    Ok(())
}

pub(super) async fn require_thread(pool: &SqlitePool, thread_id: &ChatThreadId) -> ChatResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_threads WHERE id = ?)")
        .bind(thread_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(|_| persistence_error())?;
    if exists {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Chat thread was not found",
            true,
        ))
    }
}
