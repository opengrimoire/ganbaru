use crate::db::connect_sqlite;
#[cfg(desktop)]
use crate::music::path_authority::{MusicPathKind, authorize};
use ganbaru_music_library::*;

fn connection_error(error: String) -> MusicLibraryError {
    MusicLibraryError::runtime("open music library", error)
}

/// Runs a library operation that opens, decodes, or walks local files on a blocking thread, so a slow or stalled path never holds an async runtime worker.
#[cfg(desktop)]
async fn run_file_work<T, F>(operation: F) -> MusicLibraryResult<T>
where
    T: Send + 'static,
    F: std::future::Future<Output = MusicLibraryResult<T>> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || tauri::async_runtime::block_on(operation))
        .await
        .map_err(|error| MusicLibraryError::runtime("run music file work", error))?
}

#[tauri::command]
pub async fn music_library_start_local_refresh(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicLocalRefreshRequest,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(connection_error)?;
    #[cfg(target_os = "android")]
    return super::mobile_refresh::start(&app, &pool, request).await;
    #[cfg(desktop)]
    {
        let request = bind_refresh_request(&app, request)?;
        let progress = ganbaru_music_library::local_refresh::prepare(&pool, &request).await?;
        let refresh_pool = pool.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = tauri::async_runtime::block_on(
                ganbaru_music_library::local_refresh::run_prepared(&refresh_pool, request),
            );
        });
        Ok(progress)
    }
}

#[tauri::command]
pub async fn music_library_refresh_progress(
    app: tauri::AppHandle,
    db_url: String,
    job_id: String,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    #[cfg(target_os = "android")]
    return super::mobile_refresh::progress(&pool, &job_id).await;
    #[cfg(desktop)]
    ganbaru_music_library::local_refresh::progress(&pool, &job_id).await
}

#[tauri::command]
pub async fn music_library_cancel_refresh(
    app: tauri::AppHandle,
    db_url: String,
    job_id: String,
    cancelled_at: i64,
) -> MusicLibraryResult<MusicRefreshJobProgress> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    #[cfg(target_os = "android")]
    return super::mobile_refresh::cancel(&pool, &job_id, cancelled_at).await;
    #[cfg(desktop)]
    ganbaru_music_library::local_refresh::cancel(&pool, &job_id, cancelled_at).await
}

#[tauri::command]
pub async fn music_library_upsert_youtube_video(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicYouTubeVideoWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::youtube::upsert_video(&pool, request).await
}

#[tauri::command]
pub async fn music_library_youtube_duplicate_count(
    app: tauri::AppHandle,
    db_url: String,
    video_ids: Vec<String>,
) -> MusicLibraryResult<i64> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::youtube::duplicate_video_count(&pool, video_ids).await
}

#[tauri::command]
pub async fn music_library_apply_youtube_playlist_snapshot(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicYouTubePlaylistSnapshotWrite,
) -> MusicLibraryResult<MusicYouTubeSnapshotResult> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::youtube::apply_playlist_snapshot(&pool, request).await
}

#[tauri::command]
pub async fn music_library_report_youtube_source_failure(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicYouTubeSourceFailureWrite,
) -> MusicLibraryResult<()> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::youtube::report_source_failure(&pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_create_relink_plan(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicRelinkPlanRequest,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    let folder = request.replacement_folder_path.clone();
    let check = app.clone();
    tauri::async_runtime::spawn_blocking(move || authorize(&check, &folder, MusicPathKind::Folder))
        .await
        .map_err(|error| MusicLibraryError::runtime("authorize relink folder", error))?
        .map_err(|error| path_error("replacementFolderPath", error))?;
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    run_file_work(async move { ganbaru_music_library::relink::create_plan(&pool, request).await })
        .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_relink_plan_entries(
    app: tauri::AppHandle,
    db_url: String,
    plan_id: String,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<MusicRelinkPlanWindow> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::relink::plan_entries(&pool, &plan_id, offset, limit).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_apply_relink_plan(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicRelinkApplyRequest,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::relink::apply_plan(&pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_cancel_relink_plan(
    app: tauri::AppHandle,
    db_url: String,
    plan_id: String,
    cancelled_at: i64,
) -> MusicLibraryResult<MusicRelinkPlanSummary> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::relink::cancel_plan(&pool, &plan_id, cancelled_at).await
}

#[tauri::command]
pub async fn music_library_source_removal_impact(
    app: tauri::AppHandle,
    db_url: String,
    collection_id: String,
) -> MusicLibraryResult<MusicSourceRemovalImpact> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::source_lifecycle::removal_impact(&pool, &collection_id).await
}

#[tauri::command]
pub async fn music_library_remove_source(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicSourceRemovalRequest,
) -> MusicLibraryResult<MusicSourceRemovalImpact> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::source_lifecycle::remove_source(&pool, request).await
}

#[tauri::command]
pub async fn music_library_create_playlist(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicPlaylistCreate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::create_playlist(&pool, request).await
}

#[tauri::command]
pub async fn music_library_update_playlist(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicPlaylistUpdate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::update_playlist(&pool, request).await
}

#[tauri::command]
pub async fn music_library_reorder_playlists(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicPlaylistsReorder,
) -> MusicLibraryResult<Vec<MusicWriteReceipt>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::playlist_edits::reorder_playlists(&pool, request).await
}

#[tauri::command]
pub async fn music_library_duplicate_playlist(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicPlaylistDuplicate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::duplicate_playlist(&pool, request).await
}

#[tauri::command]
pub async fn music_library_playlist_delete_impact(
    app: tauri::AppHandle,
    db_url: String,
    playlist_id: String,
) -> MusicLibraryResult<MusicPlaylistDeleteImpact> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::playlist_delete_impact(&pool, &playlist_id).await
}

#[tauri::command]
pub async fn music_library_delete_playlist(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicPlaylistDelete,
) -> MusicLibraryResult<MusicPlaylistDeleteImpact> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::delete_playlist(&pool, request).await
}

#[tauri::command]
pub async fn music_library_set_review_state(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicReviewWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::set_review_state(&pool, request).await
}

#[tauri::command]
pub async fn music_library_set_metadata_overrides(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicMetadataOverrideWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::set_metadata_overrides(&pool, request).await
}

#[tauri::command]
pub async fn music_library_set_item_signals(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicItemSignalsWrite,
) -> MusicLibraryResult<Vec<MusicWriteReceipt>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::set_item_signals(&pool, request).await
}

#[tauri::command]
pub async fn music_library_upsert_memberships(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicBulkMembershipWrite,
) -> MusicLibraryResult<Vec<MusicWriteReceipt>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::upsert_memberships(&pool, request).await
}

#[tauri::command]
pub async fn music_library_bulk_edit_memberships(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicBulkMembershipEdit,
) -> MusicLibraryResult<MusicBulkMembershipResult> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::playlist_edits::bulk_edit_memberships(&pool, request).await
}

#[tauri::command]
pub async fn music_library_membership_matrix(
    app: tauri::AppHandle,
    db_url: String,
    item_ids: Vec<String>,
) -> MusicLibraryResult<Vec<MusicMembershipMatrixEntry>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::membership_matrix(&pool, item_ids).await
}

#[tauri::command]
pub async fn music_library_reorder_playlist(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicPlaylistReorder,
) -> MusicLibraryResult<MusicPlaylistReorderResult> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::playlist_edits::reorder_playlist(&pool, request).await
}

#[tauri::command]
pub async fn music_library_playlist_playback_entries(
    app: tauri::AppHandle,
    db_url: String,
    playlist_id: String,
    now_ms: i64,
) -> MusicLibraryResult<Vec<MusicPlaylistPlaybackEntry>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::playlist_playback_entries(&pool, &playlist_id, now_ms).await
}

#[tauri::command]
pub async fn music_library_context_assignments(
    app: tauri::AppHandle,
    db_url: String,
    owner_kind: MusicAssignmentOwnerKind,
    owner_id: String,
) -> MusicLibraryResult<Vec<MusicContextAssignment>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music::assignments::load_assignments(&pool, owner_kind, &owner_id).await
}

#[tauri::command]
pub async fn music_library_bulk_set_review_state(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicBulkReviewWrite,
) -> MusicLibraryResult<MusicBulkMembershipResult> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::playlist_edits::bulk_set_review_state(&pool, request).await
}

#[tauri::command]
pub async fn music_library_apply_review_selection(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicReviewSelectionWrite,
) -> MusicLibraryResult<MusicReviewSelectionResult> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::playlist_edits::apply_review_selection(&pool, request).await
}

#[tauri::command]
pub async fn music_library_bulk_snooze(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicBulkSnoozeWrite,
) -> MusicLibraryResult<MusicBulkMembershipResult> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::playlist_edits::bulk_snooze(&pool, request).await
}

#[tauri::command]
pub async fn music_library_save_advanced_membership(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicAdvancedMembershipWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::save_advanced_membership(&pool, request).await
}

#[tauri::command]
pub async fn music_library_remove_memberships(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicMembershipRemove,
) -> MusicLibraryResult<()> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::remove_memberships(&pool, request).await
}

#[tauri::command]
pub async fn music_library_remove_snooze(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicSnoozeRemove,
) -> MusicLibraryResult<()> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::remove_snooze(&pool, request).await
}

/// Parsing and serialization run on one bounded native worker, never on a WebView thread.
static MUSIC_TRANSFER_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

fn ensure_active_vault(app: &tauri::AppHandle, expected_id: &str) -> MusicLibraryResult<()> {
    let active_vault_id = crate::vault::active_vault_id(app).map_err(connection_error)?;
    if active_vault_id != expected_id {
        return Err(MusicLibraryError::conflict(
            "The active vault changed during Music transfer",
        ));
    }
    Ok(())
}

/// Loads the active vault's device-local root bindings from native state.
fn transfer_bindings(app: &tauri::AppHandle) -> MusicLibraryResult<Vec<transfer::TransferBinding>> {
    let vault_id = crate::vault::active_vault_id(app)
        .map_err(|error| MusicLibraryError::runtime("resolve transfer vault", error))?;
    let state = crate::vault::read_app_state(app)
        .map_err(|error| MusicLibraryError::runtime("read music root bindings", error))?;
    transfer::bindings(
        state
            .music_root_bindings
            .get(&vault_id)
            .into_iter()
            .flatten(),
    )
}

#[tauri::command]
pub async fn music_library_preview_transfer(
    app: tauri::AppHandle,
    db_url: String,
    vault_id: String,
    source: MusicTransferSource,
) -> MusicLibraryResult<MusicTransferPreview> {
    let permit = MUSIC_TRANSFER_WORKERS
        .try_acquire()
        .map_err(|_| MusicLibraryError::conflict("Another Music transfer is being prepared"))?;
    ensure_active_vault(&app, &vault_id)?;
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(connection_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        ensure_active_vault(&app, &vault_id)?;
        let bindings = transfer_bindings(&app)?;
        tauri::async_runtime::block_on(ganbaru_music_library::transfer::preview(
            &pool, &source, &bindings,
        ))
    })
    .await
    .map_err(|error| MusicLibraryError::runtime("prepare Music transfer", error))?
}

#[tauri::command]
pub async fn music_library_commit_transfer(
    app: tauri::AppHandle,
    db_url: String,
    vault_id: String,
    request: MusicTransferCommit,
) -> MusicLibraryResult<MusicInterchangeImportResult> {
    let permit = MUSIC_TRANSFER_WORKERS
        .try_acquire()
        .map_err(|_| MusicLibraryError::conflict("Another Music transfer is being prepared"))?;
    ensure_active_vault(&app, &vault_id)?;
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(connection_error)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        ensure_active_vault(&app, &vault_id)?;
        let bindings = transfer_bindings(&app)?;
        tauri::async_runtime::block_on(ganbaru_music_library::transfer::commit(
            &pool, request, &bindings,
        ))
    })
    .await
    .map_err(|error| MusicLibraryError::runtime("commit Music transfer", error))?
}

#[tauri::command]
pub async fn music_library_export_transfer(
    app: tauri::AppHandle,
    db_url: String,
    vault_id: String,
    request: MusicTransferExport,
) -> MusicLibraryResult<bool> {
    let permit = MUSIC_TRANSFER_WORKERS
        .try_acquire()
        .map_err(|_| MusicLibraryError::conflict("Another Music transfer is being prepared"))?;
    ensure_active_vault(&app, &vault_id)?;
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(connection_error)?;
    let format = request.format;
    let worker_app = app.clone();
    let contents = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        ensure_active_vault(&worker_app, &vault_id)?;
        let bindings = transfer_bindings(&worker_app)?;
        tauri::async_runtime::block_on(ganbaru_music_library::transfer::export(
            &pool, &request, &bindings,
        ))
    })
    .await
    .map_err(|error| MusicLibraryError::runtime("prepare Music export", error))??;
    let extension = match format {
        MusicTransferFormat::Json => "json",
        MusicTransferFormat::M3u8 => "m3u8",
    };
    crate::music::music_pick_and_write_interchange_file(
        app,
        "ganbaru-music-playlists".to_string(),
        contents,
        extension.to_string(),
    )
    .await
    .map_err(|error| MusicLibraryError::runtime("save Music export", error))
}

#[tauri::command]
pub async fn music_library_item_window(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicItemWindowRequest,
) -> MusicLibraryResult<MusicItemWindow> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::item_window(&pool, request).await
}

#[tauri::command]
pub async fn music_library_playlist_summaries(
    app: tauri::AppHandle,
    db_url: String,
    now_ms: i64,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicPlaylistSummary>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::playlist_summaries(&pool, now_ms, offset, limit).await
}

#[tauri::command]
pub async fn music_library_source_summaries(
    app: tauri::AppHandle,
    db_url: String,
    now_ms: i64,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicSourceSummary>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::source_summaries(&pool, now_ms, offset, limit).await
}

#[tauri::command]
pub async fn music_library_issues(
    app: tauri::AppHandle,
    db_url: String,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicIssue>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::issues(&pool, offset, limit).await
}

#[tauri::command]
pub async fn music_library_inspector_detail(
    app: tauri::AppHandle,
    db_url: String,
    item_id: String,
) -> MusicLibraryResult<MusicInspectorDetail> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::inspector_detail(&pool, &item_id).await
}

#[tauri::command]
pub async fn music_library_local_roots(
    app: tauri::AppHandle,
    db_url: String,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicLocalRoot>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::local_roots(&pool, offset, limit).await
}

#[tauri::command]
pub async fn music_library_create_local_root(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicLocalRootCreate,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::create_local_root(&pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_preview_item_repair(
    app: tauri::AppHandle,
    db_url: String,
    item_id: String,
    file_path: String,
) -> MusicLibraryResult<MusicItemRepairPreview> {
    authorize_file(&app, file_path.clone(), "filePath").await?;
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    run_file_work(async move {
        ganbaru_music_library::item_repair::preview(&pool, &item_id, &file_path).await
    })
    .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_apply_item_repair(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicItemRepairApply,
) -> MusicLibraryResult<MusicWriteReceipt> {
    // Repair binds a new root to the chosen file's own folder, so the request must name
    // exactly that file's parent and name before the folder is granted.
    let folder = std::path::PathBuf::from(&request.folder_path);
    let mut names = std::path::Path::new(&request.relative_path).components();
    let (Some(std::path::Component::Normal(_)), None) = (names.next(), names.next()) else {
        return Err(MusicLibraryError::validation(
            "relativePath",
            "must be the repaired file name",
        ));
    };
    let file = folder.join(&request.relative_path);
    authorize_file(&app, file.to_string_lossy().into_owned(), "folderPath").await?;
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(connection_error)?;
    let receipt =
        run_file_work(
            async move { ganbaru_music_library::item_repair::apply(&pool, request).await },
        )
        .await?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::music::path_authority::grant_path(&app, MusicPathKind::Folder, &folder)
    })
    .await
    .map_err(|error| MusicLibraryError::runtime("grant repaired folder", error))?
    .map_err(|error| MusicLibraryError::runtime("grant repaired folder", error))?;
    Ok(receipt)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_undo_item_repair(
    app: tauri::AppHandle,
    db_url: String,
    location_id: String,
    root_id: String,
) -> MusicLibraryResult<()> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::item_repair::undo(&pool, &location_id, &root_id).await
}

#[tauri::command]
pub async fn music_library_source_collections(
    app: tauri::AppHandle,
    db_url: String,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicSourceCollection>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::source_collections(&pool, offset, limit).await
}

#[tauri::command]
pub async fn music_library_playlist_detail(
    app: tauri::AppHandle,
    db_url: String,
    playlist_id: String,
) -> MusicLibraryResult<MusicPlaylist> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::queries::playlist_detail(&pool, &playlist_id).await
}

#[tauri::command]
pub async fn music_library_upsert_source_collection(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicCollectionWrite,
) -> MusicLibraryResult<MusicWriteReceipt> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::writes::upsert_source_collection(&pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_soundscapes(
    app: tauri::AppHandle,
    db_url: String,
) -> MusicLibraryResult<Vec<MusicSoundscapeDefinition>> {
    let (_, local_paths) = soundscape_paths(&app)?;
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    run_file_work(async move {
        ganbaru_music_library::soundscapes::definitions(&pool, &local_paths).await
    })
    .await
}

/// The active vault ID with this device's local soundscape paths for it.
#[cfg(desktop)]
fn soundscape_paths(
    app: &tauri::AppHandle,
) -> MusicLibraryResult<(String, std::collections::BTreeMap<String, String>)> {
    let vault_id = crate::vault::active_vault_id(app)
        .map_err(|error| MusicLibraryError::runtime("resolve soundscape vault", error))?;
    let paths = crate::music::soundscape_paths::paths(app, &vault_id)
        .map_err(|error| MusicLibraryError::runtime("read soundscape paths", error))?;
    Ok((vault_id, paths))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_soundscape_groups(
    app: tauri::AppHandle,
    db_url: String,
) -> MusicLibraryResult<Vec<MusicSoundscapeGroup>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscape_groups::groups(&pool).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_upsert_soundscape_group(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicSoundscapeGroupWrite,
) -> MusicLibraryResult<MusicSoundscapeGroup> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscape_groups::upsert(&pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_remove_soundscape_group(
    app: tauri::AppHandle,
    db_url: String,
    group_id: String,
    expected_version: i64,
) -> MusicLibraryResult<()> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscape_groups::remove(&pool, &group_id, expected_version).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_soundscape_presets(
    app: tauri::AppHandle,
    db_url: String,
) -> MusicLibraryResult<Vec<MusicSoundscapePreset>> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscape_presets::presets(&pool).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_upsert_soundscape_preset(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicSoundscapePresetWrite,
) -> MusicLibraryResult<MusicSoundscapePreset> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscape_presets::upsert(&pool, request).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_remove_soundscape_preset(
    app: tauri::AppHandle,
    db_url: String,
    preset_id: String,
    expected_version: i64,
) -> MusicLibraryResult<()> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscape_presets::remove(&pool, &preset_id, expected_version).await
}

/// Saves the level a preset plays at relative to the Music volume; the latest slider position wins.
#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_set_soundscape_preset_level(
    app: tauri::AppHandle,
    db_url: String,
    preset_id: String,
    level: f64,
    updated_at_ms: i64,
) -> MusicLibraryResult<MusicSoundscapePreset> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscape_presets::set_level(&pool, &preset_id, level, updated_at_ms)
        .await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_upsert_soundscape(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicSoundscapeWrite,
) -> MusicLibraryResult<MusicSoundscapeDefinition> {
    let (vault_id, local_paths) = soundscape_paths(&app)?;
    let stored_path = local_paths.get(&request.id).cloned();
    if let Some(local_path) = &request.local_path {
        if !crate::music::is_soundscape_audio_path(std::path::Path::new(local_path)) {
            return Err(MusicLibraryError::validation(
                "localPath",
                "must be a supported audio file",
            ));
        }
        authorize_file(&app, local_path.clone(), "localPath").await?;
        // Saving renews the grant so later picks do not evict a loop still in use.
        let grant_app = app.clone();
        let path = std::path::PathBuf::from(local_path);
        tauri::async_runtime::spawn_blocking(move || {
            crate::music::path_authority::grant_path(&grant_app, MusicPathKind::File, &path)
        })
        .await
        .map_err(|error| MusicLibraryError::runtime("renew background audio grant", error))?
        .map_err(|error| MusicLibraryError::runtime("renew background audio grant", error))?;
    }
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(connection_error)?;
    let previous_path = stored_path.clone();
    let saved = run_file_work(async move {
        ganbaru_music_library::soundscapes::upsert(&pool, request, stored_path.as_deref()).await
    })
    .await?;
    // The row commits first; if this write fails, the sound reads as missing until its file is
    // picked again, and no unchanged row is ever pointed at a different file.
    if let Some(path) = saved
        .local_path
        .as_deref()
        .filter(|path| previous_path.as_deref() != Some(*path))
    {
        crate::music::soundscape_paths::set_path(&app, &vault_id, &saved.id, path)
            .map_err(|error| MusicLibraryError::runtime("save soundscape path", error))?;
    }
    Ok(saved)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_remove_soundscape(
    app: tauri::AppHandle,
    db_url: String,
    soundscape_id: String,
    expected_version: i64,
) -> MusicLibraryResult<()> {
    let vault_id = crate::vault::active_vault_id(&app)
        .map_err(|error| MusicLibraryError::runtime("resolve soundscape vault", error))?;
    let pool = connect_sqlite(app.clone(), db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscapes::remove(&pool, &soundscape_id, expected_version).await?;
    crate::music::soundscape_paths::clear_path(&app, &vault_id, &soundscape_id)
        .map_err(|error| MusicLibraryError::runtime("forget soundscape path", error))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_soundscape_state(
    app: tauri::AppHandle,
    db_url: String,
) -> MusicLibraryResult<MusicSoundscapeState> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscapes::state(&pool).await
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_library_update_soundscape_state(
    app: tauri::AppHandle,
    db_url: String,
    request: MusicSoundscapeStateWrite,
) -> MusicLibraryResult<MusicSoundscapeState> {
    let pool = connect_sqlite(app, db_url)
        .await
        .map_err(connection_error)?;
    ganbaru_music_library::soundscapes::update_state(&pool, request).await
}

/// Maps a refused Music path to a path-free validation error on the named field.
#[cfg(desktop)]
fn path_error(
    field: &str,
    error: crate::music::path_authority::MusicPathError,
) -> MusicLibraryError {
    MusicLibraryError::validation(field, error.message())
}

/// Replaces the webview's refresh folders with this device's root bindings for the active
/// vault, so a scan only ever reads folders the user chose here.
#[cfg(desktop)]
fn bind_refresh_request(
    app: &tauri::AppHandle,
    request: MusicLocalRefreshRequest,
) -> MusicLibraryResult<MusicLocalRefreshRequest> {
    let vault_id = crate::vault::active_vault_id(app)
        .map_err(|error| MusicLibraryError::runtime("resolve active vault", error))?;
    let state = crate::vault::read_app_state(app)
        .map_err(|error| MusicLibraryError::runtime("read music root bindings", error))?;
    apply_native_bindings(request, state.music_root_bindings.get(&vault_id))
}

#[cfg(desktop)]
fn apply_native_bindings(
    mut request: MusicLocalRefreshRequest,
    bindings: Option<&std::collections::BTreeMap<String, String>>,
) -> MusicLibraryResult<MusicLocalRefreshRequest> {
    request.folder_path = bindings
        .and_then(|bindings| bindings.get(&request.root_id))
        .cloned()
        .ok_or_else(|| MusicLibraryError::validation("rootId", "has no folder on this device"))?;
    request.available_roots = bindings
        .into_iter()
        .flatten()
        .map(|(root_id, folder_path)| MusicAvailableRootPath {
            root_id: root_id.clone(),
            folder_path: folder_path.clone(),
        })
        .collect();
    Ok(request)
}

/// Authorizes a webview file path off the async runtime, because `stat` can block on
/// network shares.
#[cfg(desktop)]
async fn authorize_file(
    app: &tauri::AppHandle,
    path: String,
    field: &str,
) -> MusicLibraryResult<()> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || authorize(&app, &path, MusicPathKind::File))
        .await
        .map_err(|error| MusicLibraryError::runtime("authorize music file", error))?
        .map(|_| ())
        .map_err(|error| path_error(field, error))
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn request(root_id: &str, folder_path: &str) -> MusicLocalRefreshRequest {
        MusicLocalRefreshRequest {
            job_id: "job".into(),
            root_id: root_id.into(),
            collection_id: "collection".into(),
            folder_path: folder_path.into(),
            available_roots: vec![MusicAvailableRootPath {
                root_id: "forged".into(),
                folder_path: "/".into(),
            }],
            requested_at_ms: 0,
        }
    }

    #[test]
    fn refresh_scans_only_the_native_binding_whatever_the_request_names() {
        let bindings = BTreeMap::from([
            ("soundtracks".to_string(), "/music/soundtracks".to_string()),
            ("ambient".to_string(), "/music/ambient".to_string()),
        ]);
        let bound = apply_native_bindings(request("soundtracks", "/"), Some(&bindings)).unwrap();
        assert_eq!(bound.folder_path, "/music/soundtracks");
        let mut roots: Vec<_> = bound
            .available_roots
            .iter()
            .map(|root| (root.root_id.as_str(), root.folder_path.as_str()))
            .collect();
        roots.sort();
        assert_eq!(
            roots,
            [
                ("ambient", "/music/ambient"),
                ("soundtracks", "/music/soundtracks")
            ]
        );
    }

    #[test]
    fn refresh_of_a_root_unbound_on_this_device_is_refused() {
        let bindings = BTreeMap::from([("ambient".to_string(), "/music/ambient".to_string())]);
        assert!(apply_native_bindings(request("soundtracks", "/music"), Some(&bindings)).is_err());
        assert!(apply_native_bindings(request("soundtracks", "/music"), None).is_err());
    }
}
