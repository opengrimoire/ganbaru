use crate::db::connect_sqlite;
use tauri::{AppHandle, Runtime};

mod data_source_row_hierarchy;
mod database_management;
#[cfg(desktop)]
mod export_dialogs;
pub(crate) mod external_links;
mod file_assets;
mod page_cover_assets;
mod page_icon_assets;
pub(crate) mod project_history;
#[cfg(desktop)]
pub mod working_markdown;

use ganbaru_notes::{
    comments, data_sources, databases, folders, links, local_user, mention_notifications,
    page_history, reads, search, suggestions, templates, transfers, undo_state, workspace_shell,
    writes,
};

pub use data_source_row_hierarchy::*;
pub use database_management::*;
pub use file_assets::*;
pub use ganbaru_notes::models::*;
pub use page_cover_assets::*;
pub use page_icon_assets::*;
#[allow(unused_imports)]
pub use project_history::*;

/// Commit one bounded editor operation with its durable retry receipt.
#[tauri::command]
pub async fn notes_apply_compound_edit<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: writes::compound::NoteCompoundEdit,
) -> Result<project_history::NotesMutationResultDto<writes::compound::NoteCompoundEditResult>, String>
{
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::compound::apply_compound_edit(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_load_workspace_shell<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteWorkspaceShellRequest,
) -> Result<NoteWorkspaceShellDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    workspace_shell::load_workspace_shell(&pool, request).await
}

#[tauri::command]
pub async fn notes_list_trashed_pages<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: Option<NotePageSummaryWindowRequest>,
) -> Result<NotePageSummaryWindowDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    writes::purge_expired_trashed_pages(&pool).await?;
    reads::list_trashed_page_window(&pool, request.unwrap_or_default()).await
}

#[tauri::command]
pub async fn notes_list_archived_pages<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: Option<NotePageSummaryWindowRequest>,
) -> Result<NotePageSummaryWindowDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    writes::purge_expired_trashed_pages(&pool).await?;
    reads::list_archived_page_window(&pool, request.unwrap_or_default()).await
}

#[tauri::command]
pub async fn notes_list_sidebar_pages<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteSidebarPagesRequest,
) -> Result<NoteSidebarPageList, String> {
    let pool = connect_sqlite(app, db_url).await?;
    writes::purge_expired_trashed_pages(&pool).await?;
    reads::list_sidebar_pages(&pool, request).await
}

#[tauri::command]
pub async fn notes_list_folders<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<NoteFolderDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    folders::list_folders(&pool).await
}

#[tauri::command]
pub async fn notes_create_folder<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    folder: NoteFolderCreate,
) -> Result<project_history::NotesMutationResultDto<NoteFolderDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = folders::create_folder(&pool, folder).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_folder<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    folder_id: String,
    update: NoteFolderUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteFolderDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = folders::update_folder(&pool, &folder_id, update).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_delete_folder<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    folder_id: String,
) -> Result<project_history::NotesMutationResultDto<String>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = folders::delete_folder(&pool, &folder_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_list_backlinks<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<Vec<NoteBacklinkDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    links::backlinks::list_backlinks(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_page_breadcrumb<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<Vec<NotePageBreadcrumbItemDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::page_breadcrumb(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_page_location<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<Vec<String>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::page_location(&pool, &page_id).await
}

/// Most tasks a page's mention menu offers from its project group.
const MENTION_SCOPE_TASK_LIMIT: u32 = 500;
/// Most Calendar events a page's mention menu offers from its project group.
const MENTION_SCOPE_EVENT_LIMIT: u32 = 500;

/// The tasks and Calendar events a page may mention: those of the projects in its project group.
#[derive(serde::Serialize)]
pub struct NotesMentionScopeDto {
    pub tasks: Vec<ganbaru_projects::task_summaries::ProjectTaskSummary>,
    pub events: Vec<ganbaru_calendar::reads::project_events::CalendarProjectEventSummary>,
}

/// List the tasks and Calendar events a page may mention. A page outside every project gets neither.
#[tauri::command]
pub async fn notes_list_mention_scope<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<NotesMentionScopeDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let project_ids = reads::page_project_group_project_ids(&pool, &page_id).await?;
    let tasks = ganbaru_projects::task_summaries::list_task_summaries(
        &pool,
        &project_ids,
        MENTION_SCOPE_TASK_LIMIT,
    )
    .await?;
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    let now = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let events = ganbaru_calendar::reads::project_events::list_project_event_summaries(
        &pool,
        &project_ids,
        &now,
        MENTION_SCOPE_EVENT_LIMIT,
    )
    .await?;
    Ok(NotesMentionScopeDto { tasks, events })
}

#[tauri::command]
pub async fn notes_search<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    query: String,
    page_size: Option<i64>,
    include_resolved_comments: Option<bool>,
    cursor: Option<String>,
) -> Result<NoteSearchWindowDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    search::search_window(
        &pool,
        &query,
        page_size,
        include_resolved_comments.unwrap_or(false),
        cursor.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn notes_list_page_aliases<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<Vec<NotePageAliasDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    links::list_page_aliases(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_add_page_alias<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    request: NotePageAliasCreate,
) -> Result<project_history::NotesMutationResultDto<Vec<NotePageAliasDto>>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::ensure_page_baseline_for_mutation(&pool, &page_id).await?;
    let value = links::add_page_alias(&pool, &page_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_delete_page_alias<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    alias_id: String,
) -> Result<project_history::NotesMutationResultDto<Vec<NotePageAliasDto>>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::ensure_page_baseline_for_mutation(&pool, &page_id).await?;
    let value = links::delete_page_alias(&pool, &page_id, &alias_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_list_unresolved_links<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<Vec<NoteUnresolvedLinkDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    links::list_unresolved_links(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_resolve_unresolved_link<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    link_id: String,
    request: NoteUnresolvedLinkResolve,
) -> Result<project_history::NotesMutationResultDto<Vec<NoteUnresolvedLinkDto>>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let source_page_id: Option<String> =
        sqlx::query_scalar("SELECT source_page_id FROM notes_unresolved_link_index WHERE id = ?")
            .bind(&link_id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| format!("load unresolved Notes history page: {e}"))?;
    if let Some(source_page_id) = source_page_id {
        project_history::ensure_page_baseline_for_mutation(&pool, &source_page_id).await?;
    }
    let value = links::resolve_unresolved_link(&pool, &link_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_import_html_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteHtmlImportRequest,
) -> Result<project_history::NotesMutationResultDto<NoteHtmlImportDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = transfers::html_import::import_page(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_import_notion_api<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteNotionApiImportRequest,
) -> Result<project_history::NotesMutationResultDto<NoteNotionApiImportDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = transfers::notion_api_import::import_from_api(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_import_notion_export_folder<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteNotionExportImportRequest,
) -> Result<project_history::NotesMutationResultDto<Option<NoteNotionExportImportDto>>, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let value = export_dialogs::pick_and_import_notion_export(&app, &pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_pick_and_write_html_archive<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteHtmlExportRequest,
) -> Result<NoteHtmlArchiveSaveDto, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    export_dialogs::pick_and_write_archive(&app, &pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_pick_and_write_json_graph<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteJsonGraphExportRequest,
) -> Result<NoteJsonGraphExportSaveDto, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    export_dialogs::pick_and_write_graph(&app, &pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_pick_and_write_agent_bridge<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteAgentBridgeExportRequest,
) -> Result<NoteAgentBridgeExportSaveDto, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    export_dialogs::pick_and_write_bridge(&app, &pool, request).await
}

#[tauri::command]
pub async fn notes_local_user<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<NoteLocalUserDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    local_user::load_local_user(&pool).await
}

#[tauri::command]
pub async fn notes_update_local_user<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    update: NoteLocalUserUpdate,
) -> Result<NoteLocalUserDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    local_user::update_local_user(&pool, update).await
}

#[tauri::command]
pub async fn notes_list_page_templates<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<NotePageTemplateDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    templates::list_page_templates(&pool).await
}

#[tauri::command]
pub async fn notes_create_page_template_from_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NotePageTemplateCreateFromPage,
) -> Result<NotePageTemplateDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    templates::create_page_template_from_page(&pool, request).await
}

#[tauri::command]
pub async fn notes_apply_page_template<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    template_id: String,
    request: NotePageTemplateApply,
) -> Result<NoteLoadedPage, String> {
    let pool = connect_sqlite(app, db_url).await?;
    templates::apply_page_template(&pool, &template_id, request).await
}

#[tauri::command]
pub async fn notes_update_page_template<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    template_id: String,
    update: NotePageTemplateUpdate,
) -> Result<NotePageTemplateDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    templates::update_page_template(&pool, &template_id, update).await
}

#[tauri::command]
pub async fn notes_duplicate_page_template<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    template_id: String,
    request: NotePageTemplateDuplicate,
) -> Result<NotePageTemplateDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    templates::duplicate_page_template(&pool, &template_id, request).await
}

#[tauri::command]
pub async fn notes_delete_page_template<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    template_id: String,
) -> Result<String, String> {
    let pool = connect_sqlite(app, db_url).await?;
    templates::delete_page_template(&pool, &template_id).await
}

#[tauri::command]
pub async fn notes_load_page_history_settings<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<NotePageHistorySettingsDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    page_history::load_page_history_settings(&pool).await
}

#[cfg(test)]
pub(crate) async fn load_workspace_shell_for_first_use_contract(
    pool: &sqlx::SqlitePool,
) -> Result<NoteWorkspaceShellDto, String> {
    workspace_shell::load_workspace_shell(
        pool,
        NoteWorkspaceShellRequest {
            project_id: None,
            expanded_page_ids: Vec::new(),
            seed_page_ids: Vec::new(),
            selected_page_id: None,
            page_cursor: None,
            folder_cursor: None,
            destination_candidates: false,
            page_query: None,
            include_navigation_index: true,
        },
    )
    .await
}

#[tauri::command]
pub async fn notes_update_page_history_settings<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    update: NotePageHistorySettingsUpdate,
) -> Result<NotePageHistorySettingsDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    page_history::update_page_history_settings(&pool, update).await
}

#[tauri::command]
pub async fn notes_list_page_history_snapshots<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<Vec<NotePageHistorySnapshotDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    page_history::list_page_history_snapshots(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_load_page_history_snapshot<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    snapshot_id: String,
) -> Result<NoteLoadedPage, String> {
    let pool = connect_sqlite(app, db_url).await?;
    page_history::load_page_history_snapshot(&pool, &page_id, &snapshot_id).await
}

#[tauri::command]
pub async fn notes_restore_page_history_snapshot<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    snapshot_id: String,
) -> Result<project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::ensure_page_baseline_for_mutation(&pool, &page_id).await?;
    let value = page_history::restore_page_history_snapshot(&pool, &page_id, &snapshot_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_copy_page_history_blocks<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    snapshot_id: String,
    request: NotePageHistoryCopyBlocks,
) -> Result<project_history::NotesMutationResultDto<NotePaginatedBlockList>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::ensure_page_baseline_for_mutation(&pool, &page_id).await?;
    let value =
        page_history::copy_page_history_blocks(&pool, &page_id, &snapshot_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_list_comments<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    include_resolved: Option<bool>,
    block_ids: Option<Vec<String>>,
) -> Result<Vec<NoteCommentThreadDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    comments::list_comments_for_blocks(
        &pool,
        &page_id,
        include_resolved.unwrap_or(false),
        block_ids.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn notes_mark_comment_threads_read<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteCommentThreadReadUpdate,
) -> Result<Vec<NoteCommentThreadDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    comments::mark_comment_threads_read(&pool, request).await
}

#[tauri::command]
pub async fn notes_create_comment<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteCommentCreate,
) -> Result<project_history::NotesMutationResultDto<NoteCommentThreadDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = comments::create_comment(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_comment<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    comment_id: String,
    update: NoteCommentUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteCommentThreadDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = comments::update_comment(&pool, &comment_id, update).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_delete_comment<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    comment_id: String,
) -> Result<project_history::NotesMutationResultDto<NoteCommentThreadDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = comments::delete_comment(&pool, &comment_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_resolve_comment_thread<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    discussion_id: String,
    resolved: Option<bool>,
) -> Result<project_history::NotesMutationResultDto<NoteCommentThreadDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        comments::resolve_comment_thread(&pool, &discussion_id, resolved.unwrap_or(true)).await?;
    project_history::mutation_result(&pool, value).await
}

/// Add or remove the local user's emoji reaction on one comment.
#[tauri::command]
pub async fn notes_toggle_comment_reaction<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    comment_id: String,
    emoji: String,
) -> Result<project_history::NotesMutationResultDto<NoteCommentThreadDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = comments::toggle_comment_reaction(&pool, &comment_id, &emoji).await?;
    project_history::mutation_result(&pool, value).await
}

/// Mute or unmute reply notifications for the local user on one discussion.
#[tauri::command]
pub async fn notes_set_comment_thread_muted<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    discussion_id: String,
    muted: bool,
) -> Result<NoteCommentThreadDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    comments::set_comment_thread_muted(&pool, &discussion_id, muted).await
}

#[tauri::command]
pub async fn notes_list_suggestions<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    include_decided: Option<bool>,
) -> Result<Vec<NoteSuggestionDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    suggestions::list_suggestions(&pool, &page_id, include_decided.unwrap_or(false)).await
}

#[tauri::command]
pub async fn notes_create_suggestion<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteSuggestionCreate,
) -> Result<project_history::NotesMutationResultDto<NoteSuggestionDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = suggestions::create_suggestion(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_accept_suggestion<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    suggestion_id: String,
) -> Result<project_history::NotesMutationResultDto<NoteSuggestionDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = suggestions::accept_suggestion(&pool, &suggestion_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_reject_suggestion<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    suggestion_id: String,
) -> Result<project_history::NotesMutationResultDto<NoteSuggestionDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = suggestions::reject_suggestion(&pool, &suggestion_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_list_pending_mention_notifications<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<NoteMentionNotificationDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    mention_notifications::list_pending(&pool).await
}

#[tauri::command]
pub async fn notes_mark_mention_notifications_delivered<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteMentionNotificationDeliveryUpdate,
) -> Result<Vec<NoteMentionNotificationDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    mention_notifications::mark_delivered(&pool, request).await
}

/// Read which comment activity on one page notifies the local user.
#[tauri::command]
pub async fn notes_get_page_notification_settings<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<NotePageNotificationSettingsDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    mention_notifications::page_notification_settings(&pool, &page_id).await
}

/// Choose which comment activity on one page notifies the local user.
#[tauri::command]
pub async fn notes_set_page_notification_level<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    level: String,
) -> Result<NotePageNotificationSettingsDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    mention_notifications::set_page_notification_level(&pool, &page_id, &level).await
}

#[tauri::command]
pub async fn notes_create_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page: NotePageCreate,
) -> Result<project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::create_page(&pool, page).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_create_child_page_from_block<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    block_id: String,
    request: NoteChildPageFromBlockCreate,
) -> Result<project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::create_child_page_from_block(&pool, &block_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_create_database<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteDatabaseCreate,
) -> Result<project_history::NotesMutationResultDto<NoteCreatedDatabaseDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = databases::create_database(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_database_reference<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    block_id: String,
) -> Result<NoteDatabaseReferenceDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    databases::database_reference(&pool, &block_id).await
}

#[tauri::command]
pub async fn notes_duplicate_database<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteDatabaseDuplicate,
) -> Result<project_history::NotesMutationResultDto<NoteCreatedDatabaseDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = databases::duplicate_database(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_create_linked_database_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteLinkedDatabaseCreate,
) -> Result<project_history::NotesMutationResultDto<NoteCreatedDatabaseDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = databases::create_linked_database_view(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_rename_database<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    database_id: String,
    update: NoteDatabaseRename,
) -> Result<project_history::NotesMutationResultDto<String>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = databases::rename_database(&pool, &database_id, update).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_list_database_views<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    database_id: String,
) -> Result<Vec<NoteDatabaseViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    databases::view_management::list_database_views(&pool, &database_id).await
}

#[tauri::command]
pub async fn notes_duplicate_database_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteDatabaseViewDuplicate,
) -> Result<project_history::NotesMutationResultDto<NoteDatabaseViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = databases::view_management::duplicate_database_view(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_rename_database_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    database_id: String,
    view_id: String,
    update: NoteDatabaseViewRename,
) -> Result<project_history::NotesMutationResultDto<NoteDatabaseViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        databases::view_management::rename_database_view(&pool, &database_id, &view_id, update)
            .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_reorder_database_views<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    database_id: String,
    view_ids: Vec<String>,
) -> Result<project_history::NotesMutationResultDto<Vec<NoteDatabaseViewDto>>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        databases::view_management::reorder_database_views(&pool, &database_id, &view_ids).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_delete_database_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    database_id: String,
    view_id: String,
) -> Result<project_history::NotesMutationResultDto<String>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        databases::view_management::delete_database_view(&pool, &database_id, &view_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_list_data_sources<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<Vec<NoteDataSourceListItemDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::schema::list_data_sources(&pool).await
}

#[tauri::command]
pub async fn notes_data_source_schema<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
) -> Result<NoteDataSourceSchemaDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::schema::data_source_schema(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_schema<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceSchemaUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceSchemaDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::schema::update_data_source_schema(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_list_data_source_row_pages<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
) -> Result<Vec<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::rows::list_data_source_row_pages(&pool, &data_source_id).await
}

#[tauri::command]
pub async fn notes_create_data_source_row_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    request: NoteDataSourceRowPageCreate,
) -> Result<project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        data_sources::rows::create_data_source_row_page(&pool, &data_source_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_import_data_source_csv<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    request: NoteDataSourceCsvImportRequest,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceCsvImportDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::csv_import::import_csv(&pool, &data_source_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn notes_pick_and_write_data_source_csv<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    request: NoteDataSourceCsvExportRequest,
) -> Result<NoteDataSourceCsvExportSaveDto, String> {
    let pool = connect_sqlite(app.clone(), db_url).await?;
    export_dialogs::pick_and_write_csv(&app, &pool, &data_source_id, request).await
}

#[tauri::command]
pub async fn notes_list_data_source_templates<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
) -> Result<Vec<NoteDataSourceTemplateDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::templates::list_data_source_templates(&pool, &data_source_id).await
}

#[tauri::command]
pub async fn notes_create_data_source_template_from_row<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    request: NoteDataSourceTemplateCreateFromRow,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceTemplateDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::templates::create_data_source_template_from_row(
        &pool,
        &data_source_id,
        request,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_apply_data_source_template<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    template_id: String,
    request: NoteDataSourceTemplateApply,
) -> Result<project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::templates::apply_data_source_template(
        &pool,
        &data_source_id,
        &template_id,
        request,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_delete_data_source_template<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    template_id: String,
) -> Result<project_history::NotesMutationResultDto<String>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        data_sources::templates::delete_data_source_template(&pool, &data_source_id, &template_id)
            .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_table_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceTableViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::table::data_source_table_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_table_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceTableViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceTableViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::table::update_data_source_table_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_data_source_row_property<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    page_id: String,
    update: NoteDataSourceRowPropertyUpdate,
) -> Result<project_history::NotesMutationResultDto<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::table::update_data_source_row_property(
        &pool,
        &data_source_id,
        &page_id,
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_data_source_rows_property<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    page_ids: Vec<String>,
    update: NoteDataSourceRowPropertyUpdate,
) -> Result<project_history::NotesMutationResultDto<Vec<String>>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::table::update_data_source_rows_property(
        &pool,
        &data_source_id,
        &page_ids,
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_click_data_source_button<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    page_id: String,
    request: NoteDataSourceButtonClick,
) -> Result<project_history::NotesMutationResultDto<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value =
        data_sources::buttons::click_data_source_button(&pool, &data_source_id, &page_id, request)
            .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_board_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceBoardViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::board::data_source_board_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_board_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceBoardViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceBoardViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::board::update_data_source_board_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_move_data_source_board_row<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    request: NoteDataSourceBoardRowMove,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceBoardViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::board::move_data_source_board_row(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        request,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_gallery_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceGalleryViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::gallery::data_source_gallery_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_gallery_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceGalleryViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceGalleryViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::gallery::update_data_source_gallery_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_list_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceListViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::list::data_source_list_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_list_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceListViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceListViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::list::update_data_source_list_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_calendar_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceCalendarViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::calendar::data_source_calendar_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_calendar_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceCalendarViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceCalendarViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::calendar::update_data_source_calendar_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_data_source_timeline_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    window: Option<NoteDataSourceViewWindowRequest>,
) -> Result<NoteDataSourceTimelineViewDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    data_sources::layouts::timeline::data_source_timeline_view_window(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        window.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn notes_update_data_source_timeline_view<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    data_source_id: String,
    database_id: Option<String>,
    view_id: Option<String>,
    update: NoteDataSourceTimelineViewUpdate,
) -> Result<project_history::NotesMutationResultDto<NoteDataSourceTimelineViewDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = data_sources::layouts::timeline::update_data_source_timeline_view(
        &pool,
        &data_source_id,
        database_id.as_deref(),
        view_id.as_deref(),
        update,
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_duplicate_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    request: NoteDuplicatePage,
) -> Result<project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::duplicate_page(&pool, &page_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_move_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    request: NoteMovePage,
) -> Result<project_history::NotesMutationResultDto<NoteLoadedPage>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::create_safety_checkpoint_for_page(&pool, &page_id, "before_move").await?;
    let value = writes::move_page(&pool, &page_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    update: NotePageUpdate,
) -> Result<project_history::NotesMutationResultDto<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::update_page(&pool, &page_id, update).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_trash_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    in_trash: Option<bool>,
) -> Result<project_history::NotesMutationResultDto<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::create_safety_checkpoint_for_page(&pool, &page_id, "before_trash").await?;
    writes::purge_expired_trashed_pages(&pool).await?;
    let value = writes::trash_page(&pool, &page_id, in_trash.unwrap_or(true)).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_archive_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    archived: Option<bool>,
) -> Result<project_history::NotesMutationResultDto<NotePageDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::create_safety_checkpoint_for_page(&pool, &page_id, "before_archive").await?;
    let value = writes::archive_page(&pool, &page_id, archived.unwrap_or(true)).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_permanently_delete_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<project_history::NotesMutationResultDto<Vec<String>>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    project_history::create_safety_checkpoint_for_page(&pool, &page_id, "before_permanent_delete")
        .await?;
    writes::purge_expired_trashed_pages(&pool).await?;
    let value = writes::permanently_delete_page(&pool, &page_id).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_load_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<NoteLoadedPage, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::load_page(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_open_page<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<NotePageOpenDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::open_page(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_block_frontier<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    parent_ids: Vec<String>,
) -> Result<NoteBlockFrontierDto, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::block_frontier(&pool, &parent_ids).await
}

#[tauri::command]
pub async fn notes_block_outline_frontier<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    parent_ids: Vec<String>,
) -> Result<Vec<NoteBlockOutlineDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::block_outline_frontier(&pool, &page_id, &parent_ids).await
}

#[tauri::command]
pub async fn notes_hydrate_blocks<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteBlockHydrationRequest,
) -> Result<Vec<NoteBlockDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::hydrate_blocks(&pool, request).await
}

#[tauri::command]
pub async fn notes_block_children<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    parent_id: String,
    start_cursor: Option<String>,
    page_size: Option<i64>,
) -> Result<NotePaginatedBlockList, String> {
    let pool = connect_sqlite(app, db_url).await?;
    reads::block_children(&pool, &parent_id, start_cursor.as_deref(), page_size).await
}

#[tauri::command]
pub async fn notes_append_block_children<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteAppendBlockChildren,
) -> Result<project_history::NotesMutationResultDto<NotePaginatedBlockList>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::append_block_children(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_update_block<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    block_id: String,
    update: NoteBlockUpdate,
    expected_content_revision: Option<String>,
) -> Result<project_history::NotesMutationResultDto<NoteBlockDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::update_block_if_unchanged(
        &pool,
        &block_id,
        update,
        expected_content_revision.as_deref(),
    )
    .await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_trash_block<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    block_id: String,
    in_trash: Option<bool>,
) -> Result<project_history::NotesMutationResultDto<NoteBlockDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::trash_block(&pool, &block_id, in_trash.unwrap_or(true)).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_trash_blocks<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    request: NoteTrashBlocks,
) -> Result<project_history::NotesMutationResultDto<NotePaginatedBlockList>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::trash_blocks(&pool, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_move_block<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    block_id: String,
    request: NoteMoveBlock,
) -> Result<project_history::NotesMutationResultDto<NoteBlockDto>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    let value = writes::move_block(&pool, &block_id, request).await?;
    project_history::mutation_result(&pool, value).await
}

#[tauri::command]
pub async fn notes_load_undo_state<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<Option<String>, String> {
    let pool = connect_sqlite(app, db_url).await?;
    undo_state::load_undo_state(&pool, &page_id).await
}

#[tauri::command]
pub async fn notes_save_undo_state<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
    state_json: String,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    undo_state::save_undo_state(&pool, &page_id, &state_json).await
}

#[tauri::command]
pub async fn notes_clear_undo_state<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
    page_id: String,
) -> Result<(), String> {
    let pool = connect_sqlite(app, db_url).await?;
    undo_state::clear_undo_state(&pool, &page_id).await
}
