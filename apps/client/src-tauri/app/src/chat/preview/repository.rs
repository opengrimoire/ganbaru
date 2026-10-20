use super::*;

/// Inserts or updates the stored row for a preview tab owned by `read.thread_id`.
///
/// Tab ids are chosen by the caller, so an existing row that belongs to another thread is
/// never updated; that conflict returns a permission error instead.
pub(super) async fn persist_tab(pool: &SqlitePool, read: &BrowserTabRead) -> ChatResult<()> {
    let result = sqlx::query(
        "INSERT INTO chat_preview_tabs
            (id, thread_id, position, current_url, title, viewport_kind, viewport_width,
             viewport_height, visible, loading_state, created_at, updated_at)
         VALUES (?, ?, (SELECT COALESCE(MAX(position) + 1, 0) FROM chat_preview_tabs WHERE thread_id = ?),
             ?, ?, 'freeform', ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(id) DO UPDATE SET current_url = excluded.current_url, title = excluded.title,
             viewport_width = excluded.viewport_width, viewport_height = excluded.viewport_height,
             visible = excluded.visible, loading_state = excluded.loading_state,
             updated_at = excluded.updated_at
         WHERE chat_preview_tabs.thread_id = excluded.thread_id",
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
    if result.rows_affected() == 0 {
        return Err(foreign_tab_error());
    }
    Ok(())
}

/// Rejects a tab id whose stored row belongs to a thread other than `thread_id`.
pub(super) async fn require_tab_owner(
    pool: &SqlitePool,
    tab_id: &str,
    thread_id: &ChatThreadId,
) -> ChatResult<()> {
    let owner =
        sqlx::query_scalar::<_, String>("SELECT thread_id FROM chat_preview_tabs WHERE id = ?")
            .bind(tab_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| persistence_error())?;
    match owner {
        Some(owner) if owner != thread_id.as_str() => Err(foreign_tab_error()),
        _ => Ok(()),
    }
}

pub(super) fn foreign_tab_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Browser preview tab belongs to another thread",
        true,
    )
}

pub(super) async fn thread_working_folder(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<ProjectWorkingFolderId> {
    let working_folder_id =
        sqlx::query_scalar::<_, String>("SELECT working_folder_id FROM chat_threads WHERE id = ?")
            .bind(thread_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(|_| persistence_error())?
            .ok_or_else(|| {
                ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
            })?;
    ProjectWorkingFolderId::new(working_folder_id).map_err(|_| persistence_error())
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
