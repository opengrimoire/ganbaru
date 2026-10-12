//! Tauri adapters for the core Chat review service.

use crate::chat::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use crate::db;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use sqlx::{Row, SqlitePool};

pub use ganbaru_chat::review_engine::*;

pub(crate) struct TauriReviewAuthorizer {
    app: tauri::AppHandle,
}

impl TauriReviewAuthorizer {
    pub(crate) fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl ReviewWorkspaceAuthorizer for TauriReviewAuthorizer {
    fn authorize<'a>(
        &'a self,
        pool: &'a SqlitePool,
        request: &'a OpenChatReviewRequest,
        operation: WorkingFolderAuthorizationOperation,
    ) -> ReviewAuthorizationFuture<'a> {
        Box::pin(authorize_review_request(
            &self.app, pool, request, operation,
        ))
    }
}

impl ChatWorkspaceChangeEmitter
    for crate::chat::workspace::observer::ChatWorkspaceObserverRegistry
{
    fn invalidate_paths(
        &self,
        working_folder_id: &ProjectWorkingFolderId,
        execution_environment_id: Option<&str>,
        paths: Vec<String>,
        repository_changed: bool,
    ) {
        crate::chat::workspace::observer::ChatWorkspaceObserverRegistry::invalidate_paths(
            self,
            working_folder_id,
            execution_environment_id,
            paths,
            repository_changed,
        );
    }
}

#[tauri::command]
pub async fn chat_open_review(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatReviewRegistry>,
    db_url: String,
    request: OpenChatReviewRequest,
) -> ChatResult<ChatReviewOpenRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let database_identity = database_identity(&pool).await?;
    open_review_service(
        &TauriReviewAuthorizer::new(app),
        &state,
        &pool,
        &database_identity,
        request,
    )
    .await
}

#[tauri::command]
pub async fn chat_read_review_patches(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatReviewRegistry>,
    db_url: String,
    request: ReadChatReviewPatchesRequest,
) -> ChatResult<ChatReviewPatchesRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let database_identity = database_identity(&pool).await?;
    read_review_patches_service(
        &TauriReviewAuthorizer::new(app),
        &state,
        &pool,
        &database_identity,
        request,
    )
    .await
}

#[tauri::command]
pub async fn chat_apply_review_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, ChatReviewRegistry>,
    mutations: tauri::State<'_, crate::chat::workspace::mutation::ChatWorkspaceMutationRegistry>,
    observers: tauri::State<'_, crate::chat::workspace::observer::ChatWorkspaceObserverRegistry>,
    db_url: String,
    request: ApplyChatReviewActionRequest,
) -> ChatResult<ChatReviewActionResultRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let database_identity = database_identity(&pool).await?;
    apply_review_action_service(
        &TauriReviewAuthorizer::new(app),
        &state,
        &mutations,
        &*observers,
        &pool,
        &database_identity,
        request,
    )
    .await
}

async fn authorize_review_request(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    request: &OpenChatReviewRequest,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<(ProjectWorkingFolderId, String, AuthorizedWorkingFolder)> {
    if source_requires_thread(&request.source) {
        let thread_id = request
            .thread_id
            .as_ref()
            .ok_or_else(thread_required_error)?;
        let (working_folder_id, environment_id, authorized, _) =
            crate::chat::execution_environment::authorize_thread_environment(
                app, pool, thread_id, operation,
            )
            .await?;
        let requested_environment = resolve_requested_environment_id(
            pool,
            &request.working_folder_id,
            request.execution_environment_id.as_deref(),
        )
        .await?;
        if working_folder_id != request.working_folder_id || requested_environment != environment_id
        {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Review source belongs to another workspace or execution environment",
                false,
            ));
        }
        return Ok((working_folder_id, environment_id, authorized));
    }
    let environment_id = resolve_requested_environment_id(
        pool,
        &request.working_folder_id,
        request.execution_environment_id.as_deref(),
    )
    .await?;
    if let Some(thread_id) = request.thread_id.as_ref() {
        let row = sqlx::query(
            "SELECT working_folder_id, execution_environment_id FROM chat_threads
             WHERE id = ? AND state != 'closed'",
        )
        .bind(thread_id.as_str())
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::NotFound,
                "Active Chat thread was not found",
                true,
            )
        })?;
        let thread_working_folder: String = row
            .try_get("working_folder_id")
            .map_err(persistence_error)?;
        let thread_environment: Option<String> = row
            .try_get("execution_environment_id")
            .map_err(persistence_error)?;
        if thread_working_folder != request.working_folder_id.as_str()
            || thread_environment.as_deref() != Some(environment_id.as_str())
        {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Chat thread belongs to another workspace or execution environment",
                false,
            ));
        }
    }
    let authorized = crate::chat::workspace::commands::authorize_working_folder(
        app,
        pool,
        &request.working_folder_id,
        operation,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        app,
        pool,
        authorized,
        Some(&environment_id),
    )
    .await?;
    Ok((
        request.working_folder_id.clone(),
        environment_id,
        authorized,
    ))
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat review persistence failed",
        true,
    )
}
