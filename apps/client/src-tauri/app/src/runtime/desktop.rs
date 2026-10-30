use crate::*;
use std::path::PathBuf;
use std::process::Stdio;
use tauri::Manager;

static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static MAIN_WINDOW_FRONTEND_READY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
const DELAYED_RELAUNCH_MS_ENV: &str = "GANBARU_DELAYED_RELAUNCH_MS";
const DELAYED_RELAUNCH_MAX_MS: u64 = 10 * 60 * 1000;
const MAIN_WINDOW_REVEAL_FALLBACK_MS: u64 = 15_000;
const EXIT_CLEANUP_IDLE: u8 = 0;
const EXIT_CLEANUP_RUNNING: u8 = 1;
const EXIT_CLEANUP_COMPLETE: u8 = 2;

struct DeferredExitCompletion {
    app: tauri::AppHandle,
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    exit_code: i32,
}

impl Drop for DeferredExitCompletion {
    fn drop(&mut self) {
        self.state
            .store(EXIT_CLEANUP_COMPLETE, std::sync::atomic::Ordering::Release);
        self.app.exit(self.exit_code);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn raise_window_for_second_launch(app: &tauri::AppHandle) {
    if !MAIN_WINDOW_FRONTEND_READY.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    crate::app_windows::raise_app_window(app);
}

#[tauri::command]
fn reveal_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    MAIN_WINDOW_FRONTEND_READY.store(true, std::sync::atomic::Ordering::Release);
    window.set_focus().map_err(|error| error.to_string())
}

fn schedule_main_window_reveal_fallback(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(
            MAIN_WINDOW_REVEAL_FALLBACK_MS,
        ));
        if MAIN_WINDOW_FRONTEND_READY.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        eprintln!("frontend readiness timed out; revealing the main window");
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
}

#[tauri::command]
fn startup_elapsed_ms() -> u64 {
    PROCESS_START
        .get()
        .map(|start| start.elapsed().as_millis() as u64)
        .unwrap_or(0)
}

fn clear_distractions_enforcement_state_best_effort(app: &tauri::AppHandle, context: &str) {
    if let Err(error) = distractions::clear_distractions_enforcement_state(app) {
        eprintln!("failed to clear distraction enforcement state {context}: {error}");
    }
}

#[tauri::command]
fn toggle_devtools(window: tauri::WebviewWindow) -> Result<bool, String> {
    #[cfg(debug_assertions)]
    {
        if window.is_devtools_open() {
            window.close_devtools();
            Ok(false)
        } else {
            window.open_devtools();
            Ok(true)
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = window;
        Err("DevTools are only available in development builds".to_string())
    }
}

#[tauri::command]
fn force_quit(
    app: tauri::AppHandle,
    overlays: tauri::State<'_, pomodoro::overlay::PomodoroOverlayState>,
) {
    if overlays.is_active() {
        overlays.focus(&app);
        return;
    }
    clear_distractions_enforcement_state_best_effort(&app, "before force quit");
    app.exit(0);
}

/// Move the database files (main, WAL, SHM) into a restorable rollback copy and
/// quit the app. Resets structured data without deleting the vault.
#[tauri::command]
async fn reset_database(app: tauri::AppHandle) -> Result<(), String> {
    let _transition = vault::quiescence::reserve_vault_transition()?;
    let writable_vault = vault::active_writable_vault_path(&app)?;
    distractions::runtime::stop_for_vault_handoff(&app).await?;
    distractions::clear_distractions_enforcement_state(&app)?;
    db::close_all_sqlite_pools(&app).await?;
    vault::database_rollback::move_database_to_rollback(
        writable_vault.as_ref(),
        chrono::Utc::now(),
    )?;

    app.exit(0);
    Ok(())
}

/// Request a fresh launch after completing native overlay cleanup.
#[tauri::command(async)]
fn restart_app(
    app: tauri::AppHandle,
    overlays: tauri::State<'_, pomodoro::overlay::PomodoroOverlayState>,
) {
    overlays.shutdown(&app);
    clear_distractions_enforcement_state_best_effort(&app, "before restart");
    app.request_restart();
}

/// Exit this process and let a short-lived helper reopen the app after a
/// fixed delay. The benchmark startup harness uses this so repeated launch
/// samples do not run as instant warm restarts.
#[tauri::command]
fn restart_app_after_delay(app: tauri::AppHandle, delay_ms: u64) -> Result<(), String> {
    spawn_delayed_relaunch_helper(delay_ms)?;
    clear_distractions_enforcement_state_best_effort(&app, "before delayed restart");
    app.exit(0);
    Ok(())
}

fn spawn_delayed_relaunch_helper(delay_ms: u64) -> Result<(), String> {
    let helper_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(helper_exe)
        .env(
            DELAYED_RELAUNCH_MS_ENV,
            delay_ms.min(DELAYED_RELAUNCH_MAX_MS).to_string(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn relaunch_target_path(fallback: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Ok(appimage) = std::env::var("APPIMAGE") {
        let candidate = PathBuf::from(appimage);
        if is_valid_relaunch_target(&candidate) {
            return candidate;
        }
    }
    fallback.to_path_buf()
}

fn is_valid_relaunch_target(path: &std::path::Path) -> bool {
    path.is_absolute() && path.file_name().is_some() && path.exists()
}

fn parse_delayed_relaunch_ms(raw: &str) -> Option<u64> {
    raw.parse::<u64>()
        .ok()
        .map(|delay_ms| delay_ms.min(DELAYED_RELAUNCH_MAX_MS))
}

fn run_delayed_relaunch_helper_if_needed() -> bool {
    let Ok(delay_raw) = std::env::var(DELAYED_RELAUNCH_MS_ENV) else {
        return false;
    };
    let Some(delay_ms) = parse_delayed_relaunch_ms(&delay_raw) else {
        return false;
    };
    let Ok(helper_exe) = std::env::current_exe() else {
        return true;
    };
    let target = relaunch_target_path(&helper_exe);
    if !is_valid_relaunch_target(&target) {
        return true;
    }
    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    let _ = std::process::Command::new(target)
        .env_remove(DELAYED_RELAUNCH_MS_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    true
}

pub fn run(context: tauri::Context<tauri::Wry>) {
    crate::chat::terminal::capture_launch_environment();
    crate::install_default_tls_crypto_provider();

    if run_delayed_relaunch_helper_if_needed() {
        return;
    }
    PROCESS_START.set(std::time::Instant::now()).ok();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            raise_window_for_second_launch(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(navigation_guard::init())
        .manage(db::vault_pools().clone())
        .manage(updates::PendingUpdate::default())
        .manage(vault::ownership::VaultOwnershipManager::default())
        .manage(vault::preferences::PreferencesState::default())
        .manage(vault::handoff::pairing::PairingManager::default())
        .manage(vault::handoff::CoordinatorLifecycle::default())
        .manage(vault::handoff::MemberListenerLifecycle::default())
        .manage(vault::handoff::enrollment_approval::EnrollmentApprovals::default())
        .manage(vault::handoff::receiver::ReceiverLifecycle::default())
        .manage(vault::handoff::source::SourceLifecycle::default())
        .manage(crate::sync::SyncRuntime::default())
        .manage(crate::sync::SyncSubscribers::default())
        .manage(notes::notion_api_import::NotionApiImportCancellations::default())
        .manage(sound_effects::AppSoundState::default())
        .manage(pomodoro::overlay::PomodoroOverlayState::default())
        .manage(music::player::MediaPlayerState::default())
        .manage(music::soundscape::SoundscapeEngineState::default())
        .manage(music::sync::RootRefreshScheduler::default())
        .manage(chat::settings_commands::ChatSettingsState::default())
        .manage(chat::provider_files::ProviderFileState::default())
        .manage(chat::internal_mcp::InternalMcpRegistry::default())
        .manage(chat::preview::ChatBrowserManager::default())
        .manage(ganbaru_chat::runtime::ChatRuntimeRegistry::default())
        .manage(chat::review::engine::ChatReviewRegistry::default())
        .manage(chat::terminal::ChatTerminalRegistry::default())
        .manage(chat::terminal_access::ChatTerminalAccess::default())
        .manage(chat::workspace::mutation::ChatWorkspaceMutationRegistry::default())
        .manage(chat::workspace::commands::ProjectWorkingFolderSelections::default())
        .manage(chat::workspace::observer::ChatWorkspaceObserverRegistry::default())
        .plugin(tauri_plugin_dialog::init())
        .manage(file_drops::FileDrops::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let overlays = window.state::<pomodoro::overlay::PomodoroOverlayState>();
                if overlays.is_active() {
                    api.prevent_close();
                    overlays.focus(window);
                }
            }
        })
        // The `unstable` feature builds every page as a child webview, so file drops arrive as
        // webview events rather than window events.
        .on_webview_event(|webview, event| {
            if let tauri::WebviewEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                webview.state::<file_drops::FileDrops>().record(
                    webview.label(),
                    paths.clone(),
                    std::time::Instant::now(),
                );
            }
        })
        .invoke_handler(tauri::generate_handler![
            chat::workspace::commands::projects_list_working_folders,
            chat::workspace::commands::projects_list_working_folders_cached,
            chat::workspace::commands::projects_add_external_working_folder,
            chat::workspace::commands::projects_pick_working_folder,
            chat::workspace::commands::projects_add_selected_working_folder,
            chat::workspace::commands::projects_bind_selected_working_folder,
            chat::workspace::commands::projects_rename_working_folder,
            chat::workspace::commands::projects_locate_working_folder,
            chat::workspace::commands::projects_decide_working_folder_trust,
            chat::workspace::commands::projects_revoke_working_folder_trust,
            chat::workspace::commands::projects_rebind_working_folder,
            chat::workspace::commands::projects_unbind_working_folder,
            chat::workspace::commands::projects_archive_working_folder,
            chat::workspace::commands::projects_restore_working_folder,
            chat::workspace::commands::projects_open_working_folder,
            chat::workspace::commands::projects_remove_working_folder,
            chat::workspace::commands::projects_recreate_managed_working_folder,
            chat::workspace::commands::projects_remember_working_folder,
            chat::workspace::commands::projects_last_working_folder,
            chat::workspace::files::projects_list_working_folder_directory,
            chat::workspace::files::projects_preview_working_folder_file,
            chat::workspace::files::projects_save_working_folder_file,
            chat::workspace::files::projects_save_working_folder_file_copy,
            chat::workspace::files::projects_recreate_working_folder_file,
            chat::workspace::files::projects_open_working_folder_file,
            chat::workspace::observer::chat_watch_workspace,
            chat::workspace::observer::chat_unwatch_workspace,
            chat::execution_environment::chat_list_execution_environments,
            chat::execution_environment::chat_create_worktree_environment,
            chat::execution_environment::chat_read_thread_execution_environment,
            chat::execution_environment::chat_remove_worktree_environment,
            chat::review::chat_list_review_comments,
            chat::review::chat_create_review_comment,
            chat::review::chat_set_review_comment_resolved,
            chat::review::chat_attach_review_comment,
            chat::review::engine::chat_open_review,
            chat::review::engine::chat_read_review_patches,
            chat::review::engine::chat_apply_review_action,
            chat::terminal_commands::chat_list_terminals,
            chat::terminal_commands::chat_read_terminal_layout,
            chat::terminal_commands::chat_save_terminal_panel_layout,
            chat::terminal_commands::chat_terminal_acknowledge,
            chat::terminal_commands::chat_terminal_attach,
            chat::terminal_commands::chat_terminal_create,
            chat::terminal_commands::chat_terminal_detach,
            chat::terminal_commands::chat_terminal_snapshot,
            chat::terminal_commands::chat_terminal_input,
            chat::terminal_commands::chat_terminal_resize,
            chat::terminal_commands::chat_terminal_close,
            chat::terminal_commands::chat_reserve_draft_thread,
            chat::checkpoint_commands::chat_run_checkpoint_cleanup,
            chat::diagnostics_commands::chat_read_diagnostics,
            chat::diagnostics_commands::chat_update_diagnostic_preferences,
            chat::diagnostics_commands::chat_delete_diagnostics,
            chat::diagnostics_commands::chat_export_redacted_diagnostics,
            chat::diagnostics_commands::chat_stop_all_processes,
            chat::diagnostics_commands::chat_rebuild_projections,
            chat::restore_commands::chat_preview_checkpoint_restore,
            chat::restore_commands::chat_execute_checkpoint_restore,
            chat::settings_commands::chat_read_settings,
            chat::settings_commands::chat_discover_default_providers,
            chat::settings_commands::chat_set_last_selected_thread,
            chat::settings_commands::chat_save_provider,
            chat::settings_commands::chat_set_provider_enabled,
            chat::settings_commands::chat_remove_provider,
            chat::settings_commands::chat_test_provider,
            chat::settings_commands::chat_confirm_provider_runtime,
            chat::settings_commands::chat_probe_provider,
            chat::settings_commands::chat_refresh_all_providers,
            chat::settings_commands::chat_refresh_provider_models,
            chat::settings_commands::chat_update_provider_models,
            chat::settings_commands::chat_update_behavior,
            chat::settings_commands::chat_update_panels,
            chat::settings_commands::chat_set_working_folder_provider_preference,
            chat::settings_commands::chat_remember_composer_selection,
            chat::settings_commands::chat_replace_credential,
            chat::settings_commands::chat_remove_credential,
            chat::settings_commands::chat_pick_provider_executable,
            chat::settings_commands::chat_pick_provider_home,
            chat::provider_files::chat_read_provider_files,
            chat::provider_files::chat_save_provider_file,
            chat::channels::chat_list_channels,
            chat::conflicts::chat_conflicts,
            chat::conflicts::chat_resolve_conflict,
            chat::channels::chat_list_navigation_channels,
            chat::channels::chat_read_channel,
            chat::channels::chat_create_channel,
            chat::channels::chat_update_channel_details,
            chat::channels::chat_archive_channel,
            chat::channels::chat_restore_channel,
            chat::channels::chat_set_channel_read,
            chat::organization::chat_list_teammates,
            chat::organization::chat_read_teammate,
            chat::organization::chat_create_teammate,
            chat::organization::chat_archive_teammate,
            chat::organization::access::chat_list_access_profiles,
            chat::organization::access::chat_create_access_profile,
            chat::organization::access::chat_duplicate_access_profile,
            chat::organization::access::chat_preview_access_profile_revision,
            chat::organization::access::chat_publish_access_profile_revision,
            chat::organization::access::chat_archive_access_profile,
            chat::organization::access::chat_read_teammate_access,
            chat::organization::access::chat_read_channel_roster,
            chat::organization::access::chat_list_assignment_targets,
            chat::organization::access::chat_preview_channel_membership_removal,
            chat::organization::access::chat_preview_teammate_access,
            chat::organization::access::chat_replace_teammate_access,
            chat::scratch::commands::chat_list_scratch_scopes,
            chat::scratch::commands::chat_browse_scratch_generation,
            chat::scratch::commands::chat_promote_scratch_file,
            chat::scratch::commands::chat_preview_scratch_cleanup,
            chat::scratch::commands::chat_cleanup_scratch,
            chat::organization::chat_read_project_primary_working_folder,
            chat::organization::chat_set_project_primary_working_folder,
            chat::organization::chat_post_message,
            chat::organization::chat_schedule_message,
            chat::organization::chat_list_scheduled_messages,
            chat::organization::chat_cancel_scheduled_message,
            chat::organization::chat_retry_scheduled_message,
            chat::organization::chat_send_scheduled_message_now,
            chat::organization::chat_dispatch_due_scheduled_messages,
            chat::organization::chat_read_channel_page,
            chat::organization::chat_read_reply_thread_page,
            chat::organization::chat_search_messages,
            chat::organization::chat_cancel_assignment,
            chat::organization::chat_retry_assignment,
            chat::organization::chat_start_mentioned_assignment,
            chat::organization::chat_recover_assignment_dispatch_jobs,
            chat::threads::chat_list_thread_window,
            chat::threads::chat_read_thread_shell,
            chat::threads::chat_read_timeline_page,
            chat::threads::chat_read_timeline_turn,
            chat::threads::chat_fork_thread,
            chat::threads::chat_open_external_url,
            chat::threads::chat_rename_thread,
            chat::threads::chat_set_thread_read,
            chat::threads::chat_archive_thread,
            chat::threads::chat_restore_thread,
            chat::threads::chat_delete_thread_permanently,
            chat::git::chat_git_status,
            chat::git::chat_git_stage,
            chat::git::chat_git_unstage,
            chat::git::chat_git_commit,
            chat::git::chat_git_fetch,
            chat::git::chat_git_pull,
            chat::git::chat_git_push,
            chat::git::chat_git_initialize,
            chat::git::chat_git_clone,
            chat::git::chat_git_discard,
            chat::source_control::chat_discover_source_control,
            chat::source_control::chat_list_hosted_change_requests,
            chat::source_control::chat_create_hosted_change_request,
            chat::source_control::chat_checkout_hosted_change_request,
            chat::source_control::chat_configure_bitbucket_credential,
            chat::source_control::chat_remove_bitbucket_credential,
            chat::preview::chat_browser_status,
            chat::preview::chat_browser_discover_servers,
            chat::preview::chat_browser_open,
            chat::preview::chat_browser_navigate,
            chat::preview::chat_browser_resize,
            chat::preview::chat_browser_set_visible,
            chat::preview::chat_browser_back,
            chat::preview::chat_browser_forward,
            chat::preview::chat_browser_refresh,
            chat::preview::chat_browser_screenshot,
            chat::preview::chat_browser_recording_start,
            chat::preview::chat_browser_recording_stop,
            chat::preview::chat_browser_close,
            chat::preview::model_access::chat_browser_model_origins,
            chat::preview::model_access::chat_browser_allow_model_origin,
            chat::preview::model_access::chat_browser_revoke_model_origin,
            chat::preview::approvals::chat_browser_pending_approval,
            chat::preview::approvals::chat_browser_resolve_approval,
            chat::preview::data_store::chat_browser_clear_data,
            chat::drafts::chat_save_draft,
            chat::drafts::chat_read_draft,
            chat::drafts::chat_delete_draft,
            chat::interaction_commands::chat_import_image,
            chat::interaction_commands::chat_pick_images,
            chat::interaction_commands::chat_attachment_data_url,
            chat::interaction_commands::chat_read_attachments,
            chat::interaction_commands::chat_import_text_snippet,
            chat::interaction_commands::chat_search_working_folder_paths,
            chat::interaction_commands::chat_validate_working_folder_mentions,
            chat::interaction_commands::chat_list_prompt_catalog,
            chat::interaction_commands::chat_recover_interrupted_turns,
            chat::interaction_commands::chat_read_interaction_state,
            chat::interaction_commands::chat_compact_context,
            chat::interaction_commands::chat_read_mcp_status,
            chat::interaction_commands::chat_set_full_access_trust,
            chat::interaction_commands::chat_has_full_access_trust,
            chat::interaction_commands::chat_save_queued_followup,
            chat::interaction_commands::chat_cancel_queued_followup,
            chat::interaction_commands::chat_mark_queued_followup_dispatched,
            chat::interaction_commands::chat_read_user_input_draft,
            chat::interaction_commands::chat_save_user_input_draft,
            chat::interaction_commands::chat_stop_session,
            chat::send_commands::chat_send_turn,
            chat::send_commands::chat_steer_turn,
            chat::send_commands::chat_resolve_approval,
            chat::send_commands::chat_resolve_user_input,
            notifications::desktop::show_event_notification,
            notifications::desktop::show_notes_notification,
            notifications::desktop::show_project_task_notification,
            notifications::desktop::show_benchmark_notification,
            pomodoro::overlay::dismiss_pomodoro_completion,
            music::music_pick_artwork_file,
            music::music_pick_and_read_interchange_file,
            music::music_artwork_data_url,
            music::music_artwork_asset_data_url,
            music::music_embedded_artwork_data_url,
            music::library::commands::music_library_start_local_refresh,
            music::library::commands::music_library_refresh_progress,
            music::library::commands::music_library_cancel_refresh,
            music::library::commands::music_library_upsert_youtube_video,
            music::library::commands::music_library_youtube_duplicate_count,
            music::library::commands::music_library_apply_youtube_playlist_snapshot,
            music::library::commands::music_library_report_youtube_source_failure,
            music::library::commands::music_library_create_relink_plan,
            music::library::commands::music_library_relink_plan_entries,
            music::library::commands::music_library_apply_relink_plan,
            music::library::commands::music_library_cancel_relink_plan,
            music::library::commands::music_library_source_removal_impact,
            music::library::commands::music_library_remove_source,
            music::library::commands::music_library_create_playlist,
            music::library::commands::music_library_update_playlist,
            music::library::commands::music_library_reorder_playlists,
            music::library::commands::music_library_duplicate_playlist,
            music::library::commands::music_library_playlist_delete_impact,
            music::library::commands::music_library_delete_playlist,
            music::library::commands::music_library_set_review_state,
            music::library::commands::music_library_set_metadata_overrides,
            music::library::commands::music_library_set_item_signals,
            music::library::commands::music_library_upsert_memberships,
            music::library::commands::music_library_bulk_edit_memberships,
            music::library::commands::music_library_membership_matrix,
            music::library::commands::music_library_reorder_playlist,
            music::library::commands::music_library_playlist_playback_entries,
            music::library::commands::music_library_context_assignments,
            music::library::commands::music_library_bulk_set_review_state,
            music::library::commands::music_library_apply_review_selection,
            music::library::commands::music_library_bulk_snooze,
            music::library::commands::music_library_save_advanced_membership,
            music::library::commands::music_library_remove_memberships,
            music::library::commands::music_library_remove_snooze,
            music::library::commands::music_library_preview_transfer,
            music::library::commands::music_library_commit_transfer,
            music::library::commands::music_library_export_transfer,
            music::library::commands::music_library_item_window,
            music::library::commands::music_library_playlist_summaries,
            music::library::commands::music_library_source_summaries,
            music::library::commands::music_library_issues,
            music::library::commands::music_library_inspector_detail,
            music::library::commands::music_library_local_roots,
            music::library::commands::music_library_create_local_root,
            music::library::commands::music_library_preview_item_repair,
            music::library::commands::music_library_apply_item_repair,
            music::library::commands::music_library_undo_item_repair,
            music::library::commands::music_library_source_collections,
            music::library::commands::music_library_playlist_detail,
            music::library::commands::music_library_upsert_source_collection,
            music::library::commands::music_library_soundscapes,
            music::library::commands::music_library_soundscape_presets,
            music::library::commands::music_library_upsert_soundscape_preset,
            music::library::commands::music_library_remove_soundscape_preset,
            music::library::commands::music_library_set_soundscape_preset_level,
            music::library::commands::music_library_upsert_soundscape,
            music::library::commands::music_library_remove_soundscape,
            music::library::commands::music_library_soundscape_state,
            music::library::commands::music_library_update_soundscape_state,
            music::root_bindings::music_local_root_bindings,
            music::root_bindings::music_set_local_root_binding,
            music::root_bindings::music_clear_local_root_binding,
            music::music_pick_media_folder,
            music::music_detect_default_folder,
            music::music_pick_root_binding_folder,
            music::music_pick_media_file,
            music::music_pick_soundscape_file,
            music::media_server::music_register_embedded_artwork,
            music::media_server::music_register_artwork_asset,
            music::media_server::music_register_media_file,
            music::media_server::music_retain_hosted_media,
            music::media_server::music_unregister_hosted_media,
            music::music_reveal_local_file,
            music::session::runtime::music_session_start,
            music::session::runtime::music_session_command,
            music::session::runtime::music_session_snapshot,
            music::session::runtime::music_session_observe,
            music::session::runtime::music_session_host,
            music::session::runtime::music_session_subscribe,
            music::session::runtime::music_session_read_frame,
            music::session::runtime::music_session_acknowledge,
            music::session::runtime::music_session_unsubscribe,
            music::media_server::music_youtube_host_url,
            music::youtube::metadata::music_youtube_metadata,
            music::youtube::thumbnail::music_youtube_thumbnail,
            music::player::media_player_probe,
            music::path_authority::music_confirm_local_sources,
            music::soundscape::music_soundscape_start,
            music::soundscape::music_soundscape_pause,
            music::soundscape::music_soundscape_resume,
            music::soundscape::music_soundscape_stop,
            music::soundscape::music_soundscape_set_preset_level,
            music::soundscape::music_soundscape_set_levels,
            music::soundscape::music_soundscape_recover,
            music::soundscape::music_soundscape_snapshot,
            tray::update_music_tray,
            force_quit,
            reset_database,
            benchmark::read_benchmark_state,
            benchmark::write_benchmark_state,
            benchmark::clear_benchmark_state,
            benchmark::prepare_benchmark_db,
            benchmark::teardown_benchmark_db,
            benchmark::select_database_target,
            benchmark::seed::benchmark_seed_pomodoro_history,
            benchmark::seed::benchmark_seed_dense_music_library,
            restart_app,
            updates::updater_install_context,
            updates::updates_check,
            updates::updates_install,
            restart_app_after_delay,
            toggle_devtools,
            benchmark::memory::memory_report,
            startup_elapsed_ms,
            reveal_main_window,
            crate::app_windows::app_window_open_tab,
            vault::vault_default_location,
            vault::vault_use_default,
            vault::vault_release_starter,
            vault::vault_active_info,
            vault::database_rollback::vault_database_rollbacks,
            vault::database_rollback::vault_restore_database_rollback,
            vault::database_rollback::vault_dismiss_database_rollback,
            vault::ownership::vault_ownership_status,
            vault::vault_lock::vault_lock_status,
            vault::handoff::handoff_create_pairing_invitation,
            #[cfg(target_os = "linux")]
            vault::handoff::handoff_grant_network_access,
            #[cfg(target_os = "linux")]
            vault::handoff::handoff_revoke_network_access,
            vault::handoff::handoff_decode_pairing_qr,
            vault::handoff::handoff_enroll,
            vault::handoff::handoff_enrollment_code,
            vault::handoff::handoff_suggested_device_label,
            vault::handoff::handoff_pairing_status,
            vault::handoff::handoff_unlink,
            vault::handoff::handoff_recover_local_copy,
            vault::handoff::handoff_respond_enrollment,
            vault::handoff::handoff_request_owner_bundle,
            vault::handoff::receiver::handoff_receive_desktop_bundle,
            vault::handoff::receiver::handoff_cancel_receive,
            vault::handoff::receiver::handoff_pick_link_folder,
            vault::handoff::receiver::handoff_reset_link_folder,
            vault::vault_pick_create,
            vault::vault_pick_open,
            vault::vault_reveal_active,
            vault::vault_read_config,
            vault::vault_patch_config,
            vault::vault_pick_and_read_ics_import,
            vault::vault_pick_and_write_ics_export,
            vault::vault_pick_and_read_theme_json,
            vault::vault_pick_and_write_theme_json,
            vault::vault_pick_and_write_recovery_kit,
            vault::encrypted_backup::vault_backup_to_file,
            vault::encrypted_backup::vault_inspect_backup,
            vault::encrypted_backup::vault_restore_backup,
            calendar::reads::calendar_load_window,
            calendar::reads::calendar_load_native_window,
            calendar::reads::calendar_load_native_focus_window,
            calendar::reads::calendar_load_native_notification_window,
            calendar::reads::calendar_load_panel_event,
            calendar::reads::calendar_load_full_event,
            calendar::reads::calendar_load_export_snapshot,
            calendar::events::preview::calendar_preview_edit,
            calendar::events::commit::calendar_commit_edit,
            pomodoro::native_runtime::calendar_edit::calendar_dismiss_delete_undo,
            calendar::import::calendar_bulk_import,
            calendar::calendars::calendar_list_calendars,
            calendar::calendars::calendar_find_imported_calendar,
            calendar::calendars::calendar_count_events,
            calendar::calendars::calendar_add_calendar,
            calendar::calendars::calendar_set_visibility,
            calendar::calendars::calendar_remove_calendar,
            calendar::conflicts::calendar_conflicts,
            calendar::conflicts::calendar_resolve_conflict,
            themes::themes_load_all,
            pomodoro::pomodoro_load_segments_for_events,
            pomodoro::focus_device_activity,
            pomodoro::native_runtime::focus_snapshot,
            pomodoro::native_runtime::focus_command,
            pomodoro::native_runtime::focus_idle_overlay_visible,
            pomodoro::native_runtime::focus_subscribe,
            pomodoro::native_runtime::focus_renew_subscription,
            pomodoro::native_runtime::focus_unsubscribe,
            pomodoro::native_runtime::focus_notification_copy,
            projects::workspace::projects_load_workspace,
            projects::workspace::projects_refresh_workspace,
            projects::workspace::projects_load_task_view,
            projects::workspace::projects_load_task_detail,
            projects::workspace::projects_load_optional_data,
            projects::structure_commands::projects_create_group,
            projects::structure_commands::projects_update_group,
            projects::structure_commands::projects_delete_group,
            projects::structure_commands::projects_set_group_collapsed,
            projects::structure_commands::projects_reorder_groups,
            projects::structure_commands::projects_reorder_group_projects,
            projects::project_commands::projects_create_project,
            projects::project_commands::projects_update_project,
            projects::project_commands::projects_update_notes_settings,
            projects::structure_commands::projects_create_section,
            projects::structure_commands::projects_update_section,
            projects::structure_commands::projects_create_status,
            projects::structure_commands::projects_update_status,
            projects::structure_commands::projects_delete_status,
            projects::structure_commands::projects_create_priority,
            projects::structure_commands::projects_update_priority,
            projects::structure_commands::projects_delete_priority,
            projects::task_commands::projects_create_task,
            projects::task_commands::projects_create_checklist_item,
            projects::task_commands::projects_update_checklist_item,
            projects::task_commands::projects_delete_checklist_item,
            projects::structure_commands::projects_create_tag,
            projects::structure_commands::projects_update_tag,
            projects::structure_commands::projects_delete_tag,
            projects::relationship_commands::projects_link_task_tag,
            projects::relationship_commands::projects_unlink_task_tag,
            projects::custom_fields::projects_create_custom_field,
            projects::custom_fields::projects_update_custom_field,
            projects::custom_fields::projects_delete_custom_field,
            projects::custom_fields::projects_create_custom_field_option,
            projects::custom_fields::projects_update_custom_field_option,
            projects::custom_fields::projects_delete_custom_field_option,
            projects::custom_fields::projects_update_custom_field_value,
            projects::relationship_commands::projects_link_task_event,
            projects::relationship_commands::projects_unlink_task_event,
            projects::relationship_commands::projects_search_linkable_events,
            projects::conflicts::projects_conflicts,
            projects::conflicts::projects_resolve_conflict,
            notes::conflicts::notes_conflicts,
            notes::conflicts::notes_resolve_conflict,
            notes::conflicts::notes_preview_mismatched_value,
            notes::conflicts::notes_settle_mismatched_value,
            projects::relationship_commands::projects_create_task_dependency,
            projects::relationship_commands::projects_delete_task_dependency,
            projects::task_commands::projects_update_task,
            projects::task_commands::projects_list_upcoming_task_reminders,
            projects::task_bulk::projects_apply_task_bulk,
            projects::reorder::projects_reorder_item,
            projects::dependency_cascade::projects_preview_dependency_cascade,
            projects::dependency_cascade::projects_apply_dependency_cascade,
            projects::preferences::projects_upsert_view_preference,
            projects::views::projects_create_view,
            projects::views::projects_rename_view,
            projects::views::projects_reorder_views,
            projects::views::projects_delete_view,
            projects::emojis::projects_create_custom_emoji,
            projects::emojis::projects_delete_custom_emoji,
            notes::notes_load_workspace_shell,
            notes::notes_list_trashed_pages,
            notes::notes_list_archived_pages,
            notes::notes_list_sidebar_pages,
            notes::notes_list_folders,
            notes::working_markdown::notes_list_working_markdown,
            notes::working_markdown::notes_read_working_markdown,
            notes::working_markdown::notes_save_working_markdown,
            notes::working_markdown::notes_open_working_markdown,
            notes::notes_create_folder,
            notes::notes_update_folder,
            notes::notes_delete_folder,
            notes::notes_list_backlinks,
            notes::external_links::notes_open_external_url,
            notes::notes_page_breadcrumb,
            notes::notes_page_location,
            notes::notes_list_mention_scope,
            notes::notes_search,
            notes::notes_list_page_aliases,
            notes::notes_add_page_alias,
            notes::notes_delete_page_alias,
            notes::notes_list_unresolved_links,
            notes::notes_resolve_unresolved_link,
            notes::notes_import_html_page,
            notes::notes_import_notion_api,
            notes::notes_cancel_notion_api_import,
            notes::notes_import_notion_export_folder,
            notes::notes_pick_and_write_html_archive,
            notes::notes_pick_and_write_json_graph,
            notes::notes_pick_and_write_agent_bridge,
            notes::notes_local_user,
            notes::notes_list_page_templates,
            notes::notes_create_page_template_from_page,
            notes::notes_apply_page_template,
            notes::notes_update_page_template,
            notes::notes_duplicate_page_template,
            notes::notes_delete_page_template,
            notes::notes_load_page_history_settings,
            notes::notes_update_page_history_settings,
            notes::notes_list_page_history_snapshots,
            notes::notes_load_page_history_snapshot,
            notes::notes_restore_page_history_snapshot,
            notes::notes_copy_page_history_blocks,
            notes::project_history::schedule::notes_initialize_project_history,
            notes::project_history::schedule::notes_flush_due_project_history,
            notes::project_history::reads::notes_list_project_history_versions,
            notes::project_history::reads::notes_load_project_history_tree,
            notes::project_history::reads::notes_load_project_history_page,
            notes::project_history::retention::notes_history_retention_impact,
            notes::project_history::retention::notes_prune_project_history,
            notes::project_history::commands::notes_preview_project_history_restore,
            notes::project_history::commands::notes_restore_project_history_version,
            notes::notes_list_comments,
            notes::notes_mark_comment_threads_read,
            notes::notes_create_comment,
            notes::notes_update_comment,
            notes::notes_delete_comment,
            notes::notes_resolve_comment_thread,
            notes::notes_toggle_comment_reaction,
            notes::notes_set_comment_thread_muted,
            notes::notes_list_suggestions,
            notes::notes_create_suggestion,
            notes::notes_accept_suggestion,
            notes::notes_reject_suggestion,
            notes::notes_list_pending_mention_notifications,
            notes::notes_mark_mention_notifications_delivered,
            notes::notes_get_page_notification_settings,
            notes::notes_set_page_notification_level,
            notes::notes_create_page,
            notes::notes_create_child_page_from_block,
            notes::notes_create_database,
            notes::notes_database_reference,
            notes::notes_set_database_editing_lock,
            notes::notes_create_data_source,
            notes::notes_attach_data_source,
            notes::notes_apply_data_source_property_action,
            notes::notes_duplicate_database,
            notes::notes_create_linked_database_view,
            notes::notes_rename_database,
            notes::notes_list_database_views,
            notes::notes_duplicate_database_view,
            notes::notes_rename_database_view,
            notes::notes_reorder_database_views,
            notes::notes_delete_database_view,
            notes::notes_list_data_sources,
            notes::notes_data_source_schema,
            notes::notes_update_data_source_schema,
            notes::notes_list_data_source_row_pages,
            notes::notes_create_data_source_row_page,
            notes::notes_import_data_source_csv,
            notes::notes_pick_and_write_data_source_csv,
            notes::notes_list_data_source_templates,
            notes::notes_create_data_source_template_from_row,
            notes::notes_apply_data_source_template,
            notes::notes_delete_data_source_template,
            notes::notes_data_source_table_view,
            notes::notes_update_data_source_table_view,
            notes::notes_update_data_source_row_property,
            notes::notes_update_data_source_rows_property,
            notes::notes_create_data_source_subitem,
            notes::notes_update_data_source_row_parent,
            notes::notes_get_data_source_row_page_context,
            notes::notes_click_data_source_button,
            notes::notes_data_source_board_view,
            notes::notes_update_data_source_board_view,
            notes::notes_move_data_source_board_row,
            notes::notes_data_source_gallery_view,
            notes::notes_update_data_source_gallery_view,
            notes::notes_data_source_list_view,
            notes::notes_update_data_source_list_view,
            notes::notes_data_source_calendar_view,
            notes::notes_update_data_source_calendar_view,
            notes::notes_data_source_timeline_view,
            notes::notes_update_data_source_timeline_view,
            notes::notes_duplicate_page,
            notes::notes_move_page,
            notes::notes_update_page,
            notes::notes_pick_page_cover_file,
            notes::notes_save_page_cover_data_url,
            notes::notes_page_cover_asset_data_url,
            notes::notes_pick_file_asset,
            notes::notes_file_asset_data_url,
            notes::notes_pick_page_icon_file,
            notes::notes_save_page_icon_data_url,
            notes::notes_page_icon_asset_data_url,
            notes::notes_trash_page,
            notes::notes_archive_page,
            notes::notes_permanently_delete_page,
            notes::notes_load_page,
            notes::notes_open_page,
            notes::notes_block_frontier,
            notes::notes_block_outline_frontier,
            notes::notes_hydrate_blocks,
            notes::notes_block_children,
            notes::notes_append_block_children,
            notes::notes_update_block,
            notes::notes_apply_compound_edit,
            notes::notes_trash_block,
            notes::notes_trash_blocks,
            notes::notes_move_block,
            notes::notes_load_undo_state,
            notes::notes_save_undo_state,
            notes::notes_clear_undo_state,
            quick_notes::quick_notes_list,
            quick_notes::quick_notes_load,
            quick_notes::quick_notes_create,
            quick_notes::quick_notes_update,
            quick_notes::quick_notes_set_pinned,
            quick_notes::quick_notes_reorder,
            quick_notes::quick_notes_archive,
            quick_notes::quick_notes_unarchive,
            quick_notes::quick_notes_trash,
            quick_notes::quick_notes_restore,
            quick_notes::quick_notes_delete_permanently,
            quick_notes::quick_notes_empty_trash,
            quick_notes::quick_notes_purge_expired_trash,
            quick_notes::quick_notes_list_tags,
            quick_notes::quick_notes_create_tag,
            quick_notes::quick_notes_update_tag,
            quick_notes::quick_notes_reorder_tag,
            quick_notes::quick_notes_delete_tag,
            quick_notes::quick_notes_conflict,
            quick_notes::quick_notes_resolve_conflict,
            quick_notes::quick_notes_import_image,
            quick_notes::quick_notes_read_image,
            quick_notes::quick_notes_pick_images,
            quick_notes::quick_notes_paste_clipboard_image,
            quick_notes::quick_notes_import_dropped_images,
            quick_notes::quick_notes_download_image,
            crate::sync::commands::sync_status,
            crate::sync::commands::sync_subscribe,
            crate::sync::commands::sync_unsubscribe,
            crate::sync::commands::sync_now,
            crate::sync::commands::sync_set_paused,
            crate::sync::commands::sync_recovery_list,
            crate::sync::commands::sync_recovery_restore,
            crate::sync::commands::sync_recovery_discard,
            crate::sync::commands::sync_reset_state,
            crate::sync::commands::sync_quarantine_list,
            crate::sync::commands::sync_quarantine_device,
            crate::sync::commands::sync_quarantine_lift,
            crate::sync::commands::sync_take_anchor,
            crate::sync::commands::sync_promote_device,
            crate::sync::commands::sync_regenerate_recovery_kit,
            crate::sync::commands::sync_recovery_kit_state,
            crate::sync::commands::sync_create_recovery_kit,
            crate::sync::commands::sync_recovery_kit_saved,
            crate::sync::commands::sync_use_recovery_kit,
            crate::sync::commands::sync_force_anchor,
            crate::sync::commands::sync_contest_yield,
            crate::sync::commands::sync_contest_object,
            crate::sync::commands::sync_contests,
            crate::sync::commands::sync_records_resume,
            crate::sync::commands::sync_authority,
            crate::sync::commands::sync_outbox_list,
            crate::sync::commands::sync_late_changes,
            crate::sync::commands::sync_late_change_restore,
            crate::sync::commands::sync_late_change_discard,
            crate::sync::commands::sync_remove_device,
            contacts::contacts_list,
            contacts::contacts_local_card,
            contacts::contacts_regenerate_card,
            contacts::contacts_parse_card,
            contacts::contacts_decode_card_qr,
            contacts::contacts_send_request,
            contacts::contacts_accept_request,
            contacts::contacts_decline_request,
            contacts::contacts_cancel_request,
            contacts::contacts_block,
            contacts::contacts_unblock,
            contacts::contacts_remove,
            contacts::contacts_update_trust,
            contacts::contacts_sync_requests,
            contacts::contacts_save_card_image,
            profile_images::profile_image_pick_file,
            profile_images::profile_image_save_data_url,
            profile_images::profile_image_asset_data_url,
            profile_images::profile_image_delete_file,
            projects::icons::projects_icon_pick_image_file,
            projects::icons::projects_icon_save_image_data_url,
            projects::icons::projects_icon_download_image_url,
            projects::icons::projects_icon_asset_path,
            projects::icons::projects_icon_asset_data_url,
            projects::icons::projects_icon_delete_assets_if_unreferenced,
            distractions::state_files::distractions_extension_status,
            distractions::limits_read::distractions_load_usage_projection,
            distractions::catalog::distractions_list_desktop_apps,
            distractions::commands::distractions_open_extension_install_docs,
            distractions::rule_commands::distractions_rules_load,
            distractions::rule_commands::distractions_rules_set_mode,
            distractions::rule_commands::distractions_rules_set_enabled,
            distractions::rule_commands::distractions_rules_add_hosts,
            distractions::rule_commands::distractions_rules_add_desktop_app,
            distractions::rule_commands::distractions_rules_add_mobile_app,
            distractions::rule_commands::distractions_rules_remove,
            distractions::rule_commands::distractions_rules_save_custom_category,
            distractions::rule_commands::distractions_rules_save_usage_limit,
            distractions::rule_commands::distractions_rules_reset,
            themes::themes_insert,
            themes::themes_replace_content,
            themes::themes_delete,
            themes::themes_record_dismissal,
            themes::themes_load_dismissals,
            themes::themes_rename,
            themes::themes_reset_token_to_seed,
            themes::themes_reset_to_seed,
            themes::themes_conflict,
            themes::themes_resolve_conflict,
        ])
        .setup(|app| {
            if let Err(error) = vault::secure_app_config_dir(app.handle()) {
                eprintln!("app config directory permissions are unchanged: {error}");
            }
            vault::ownership::initialize(app.handle())?;
            vault::handoff::initialize(app.handle());
            crate::sync::setup(app.handle());
            if let Err(error) = vault::handoff::start_desktop(app.handle()) {
                eprintln!("vault handoff coordinator is unavailable: {error}");
            }
            clear_distractions_enforcement_state_best_effort(app.handle(), "during startup");
            schedule_main_window_reveal_fallback(app.handle());
            app.manage(chat::sync::RevocationWaker::new(app.handle().clone()));
            app.manage(chat::sync::AssignmentWaker::new(app.handle().clone()));
            chat::revocation::start_job_drain(app.handle());
            chat::cleanup_service::start(app.handle());
            music::setup_youtube_host(app.handle())?;
            music::session::setup(app.handle());
            distractions::runtime::setup(app.handle());
            music::media_controls::setup_media_controls(app.handle())?;
            if let Err(err) = pomodoro::overlay::shortcuts::restore_stale_shortcuts(app.handle()) {
                eprintln!("failed to restore stale Linux shortcuts: {err}");
            }
            tray::setup_tray(app.handle())?;
            pomodoro::setup(app.handle());
            contacts::requests::start_request_polling(app.handle().clone());
            Ok(())
        })
        .build(context)
        .expect("error while building tauri application");

    let exit_cleanup_state =
        std::sync::Arc::new(std::sync::atomic::AtomicU8::new(EXIT_CLEANUP_IDLE));
    app.run(move |app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            crate::sync::release_runtime_handle();
            // A restart spawns the next process before this one ends, so it must find the vault free.
            vault::vault_lock::process_vault_lock().release();
        }
        if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
            let cleanup_state = exit_cleanup_state.load(std::sync::atomic::Ordering::Acquire);
            if code != Some(tauri::RESTART_EXIT_CODE) && cleanup_state != EXIT_CLEANUP_COMPLETE {
                api.prevent_exit();
                if exit_cleanup_state
                    .compare_exchange(
                        EXIT_CLEANUP_IDLE,
                        EXIT_CLEANUP_RUNNING,
                        std::sync::atomic::Ordering::AcqRel,
                        std::sync::atomic::Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let app = app_handle.clone();
                    let state = std::sync::Arc::clone(&exit_cleanup_state);
                    // Detach cleanup so this callback returns to the event loop,
                    // which must process the main-thread work cleanup requests.
                    std::mem::drop(tauri::async_runtime::spawn_blocking(move || {
                        let _completion = DeferredExitCompletion {
                            app: app.clone(),
                            state,
                            exit_code: code.unwrap_or(0),
                        };
                        app.state::<pomodoro::overlay::PomodoroOverlayState>()
                            .shutdown(&app);
                    }));
                }
                return;
            }

            let overlays = app_handle.state::<pomodoro::overlay::PomodoroOverlayState>();
            if overlays.is_active() {
                eprintln!(
                    "Pomodoro overlay remained active during an exit request that cannot be delayed"
                );
            }
            let terminals = app_handle.state::<chat::terminal::ChatTerminalRegistry>();
            if let Err(error) = terminals.shutdown_all() {
                eprintln!("Chat terminal shutdown failed with code {:?}", error.code);
            }
            let runtime = app_handle.state::<ganbaru_chat::runtime::ChatRuntimeRegistry>();
            let mutations =
                app_handle.state::<chat::workspace::mutation::ChatWorkspaceMutationRegistry>();
            if let Err(error) = tauri::async_runtime::block_on(
                runtime.shutdown_and_wait(std::time::Duration::from_secs(4), &mutations),
            ) {
                eprintln!("Chat runtime shutdown failed with code {:?}", error.code);
            }
            app_handle
                .state::<chat::preview::ChatBrowserManager>()
                .close_all(app_handle);
            app_handle
                .state::<vault::handoff::CoordinatorLifecycle>()
                .stop();
            app_handle
                .state::<vault::handoff::MemberListenerLifecycle>()
                .stop_now();
            clear_distractions_enforcement_state_best_effort(app_handle, "before app exit");
            if let Err(error) = tauri::async_runtime::block_on(
                distractions::runtime::stop_for_vault_handoff(app_handle),
            ) {
                eprintln!("Distractions shutdown flush failed: {error}");
            }
            if let Err(error) = tauri::async_runtime::block_on(
                distractions::usage_totals::flush_active_vault(app_handle),
            ) {
                eprintln!("Distractions usage totals exit flush failed: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_relaunch_delay_is_bounded() {
        assert_eq!(parse_delayed_relaunch_ms("250"), Some(250));
        assert_eq!(
            parse_delayed_relaunch_ms(&(DELAYED_RELAUNCH_MAX_MS + 1).to_string()),
            Some(DELAYED_RELAUNCH_MAX_MS)
        );
        assert_eq!(parse_delayed_relaunch_ms("not-a-number"), None);
    }

    #[test]
    fn relaunch_target_validation_requires_existing_absolute_path() {
        assert!(!is_valid_relaunch_target(std::path::Path::new(
            "relative-binary"
        )));
        assert!(!is_valid_relaunch_target(std::path::Path::new(
            "/definitely/not/ganbaru"
        )));
        let current_exe = std::env::current_exe().expect("test executable path should exist");
        assert!(is_valid_relaunch_target(&current_exe));
    }

    #[test]
    fn project_history_commands_are_registered_at_defining_modules() {
        let handlers = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/runtime/desktop.rs"
        ));
        for command in [
            "notes::project_history::schedule::notes_initialize_project_history",
            "notes::project_history::schedule::notes_flush_due_project_history",
            "notes::project_history::reads::notes_list_project_history_versions",
            "notes::project_history::reads::notes_load_project_history_tree",
            "notes::project_history::reads::notes_load_project_history_page",
            "notes::project_history::retention::notes_history_retention_impact",
            "notes::project_history::retention::notes_prune_project_history",
            "notes::project_history::commands::notes_preview_project_history_restore",
            "notes::project_history::commands::notes_restore_project_history_version",
        ] {
            assert!(handlers.contains(command), "missing handler {command}");
        }
    }
}
