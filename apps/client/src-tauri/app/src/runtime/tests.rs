const MOBILE_RUNTIME: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/runtime/mobile.rs"
));
const DESKTOP_RUNTIME: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/runtime/desktop.rs"
));

/// Commands removed because they turned a webview string into file authority.
const REMOVED_PATH_AUTHORITY_COMMANDS: &[&str] = &["notes_prepare_import_file_reference"];

#[test]
fn no_runtime_registers_removed_path_authority_commands() {
    for command in REMOVED_PATH_AUTHORITY_COMMANDS {
        for (platform, runtime) in [("desktop", DESKTOP_RUNTIME), ("mobile", MOBILE_RUNTIME)] {
            assert!(
                !runtime.contains(command),
                "{platform} runtime must not register {command}"
            );
        }
    }
}

#[test]
fn mobile_runtime_registers_portable_core_commands() {
    for command in [
        "vault::vault_use_default",
        "vault::vault_pick_open",
        "calendar::reads::calendar_load_window",
        "calendar::reads::calendar_load_notification_scheduler_window",
        "projects::workspace::projects_load_workspace",
        "notes::notes_load_workspace_shell",
        "notes::notes_toggle_comment_reaction",
        "notes::notes_set_comment_thread_muted",
        "notes::notes_get_page_notification_settings",
        "notes::notes_set_page_notification_level",
        "notes::notes_get_data_source_row_page_context",
        "notes::notes_list_mention_scope",
        "pomodoro::native_runtime::focus_snapshot",
        "pomodoro::native_runtime::focus_command",
        "pomodoro::native_runtime::focus_notification_copy",
        "pomodoro::native_runtime::focus_subscribe",
        "pomodoro::native_runtime::focus_renew_subscription",
        "pomodoro::native_runtime::focus_unsubscribe",
        "crate::distractions::android::distractions_mobile_update_copy",
        "crate::distractions::android::distractions_mobile_load_usage_projection",
        "music::session::runtime::music_session_subscribe",
        "music::session::runtime::music_session_read_frame",
        "music::session::runtime::music_session_acknowledge",
        "music::session::runtime::music_session_unsubscribe",
        "quick_notes::quick_notes_list",
        "quick_notes::quick_notes_purge_expired_trash",
        "quick_notes::quick_notes_update_tag",
        "quick_notes::quick_notes_reorder_tag",
        "quick_notes::quick_notes_delete_tag",
        "quick_notes::quick_notes_conflict",
        "quick_notes::quick_notes_resolve_conflict",
        "quick_notes::quick_notes_import_image",
        "quick_notes::quick_notes_read_image",
        "quick_notes::quick_notes_download_image",
        "quick_notes::quick_notes_collect_images",
        "crate::sync::commands::sync_status",
        "crate::sync::commands::sync_subscribe",
        "crate::sync::commands::sync_unsubscribe",
        "crate::sync::commands::sync_now",
        "crate::sync::commands::sync_set_paused",
        "crate::sync::commands::sync_recovery_list",
        "crate::sync::commands::sync_recovery_restore",
        "crate::sync::commands::sync_recovery_discard",
        "contacts::contacts_list",
        "contacts::contacts_decode_card_qr",
        "themes::themes_load_all",
        "music::session::runtime::music_session_command",
        "music::library::commands::music_library_playlist_summaries",
        "music::root_bindings::music_local_root_bindings",
        "music::path_authority::music_confirm_local_sources",
        "profile_images::profile_image_asset_data_url",
        "profile_images::profile_image_save_data_url",
        "projects::icons::projects_icon_asset_data_url",
        "projects::structure_commands::projects_reorder_groups",
        "projects::structure_commands::projects_reorder_group_projects",
        "vault::vault_pick_and_read_theme_json",
        "vault::vault_pick_and_write_theme_json",
        "vault::backup::vault_backup_to_downloads",
        "vault::backup::vault_pick_and_restore_backup",
        "chat::channels::chat_list_navigation_channels",
        "chat::organization::chat_post_message",
        "chat::organization::chat_read_channel_page",
        "chat::organization::chat_read_reply_thread_page",
        "chat::settings_commands::chat_read_settings",
    ] {
        assert!(
            MOBILE_RUNTIME.contains(command),
            "missing mobile command {command}"
        );
    }
}

#[test]
fn mobile_runtime_excludes_desktop_only_commands() {
    for command in [
        "chat::send_commands::",
        "chat::terminal_commands::",
        "chat::git::",
        "chat::workspace::commands::",
        "chat::interaction_commands::",
        "chat::preview::",
        "distractions::commands::",
        "distractions::state_files::",
        "distractions::usage::",
        "distractions::limits_read::",
        "distractions::catalog::",
        "distractions::android::distractions_mobile_sync_events",
        "distractions::android::distractions_mobile_list_usage_samples",
        "notifications::desktop::show_event_notification",
        "notifications::desktop::show_project_task_notification",
        "projects::task_commands::projects_list_upcoming_task_reminders",
        "soundscape::",
        "contacts::contacts_save_card_image",
        "tray::",
        "updates::",
        "benchmark::",
        "vault::vault_pick_create",
        "notes::notes_pick_",
        "notes::working_markdown::",
        "notes::notes_import_notion_export_folder",
        "profile_images::profile_image_pick_file",
        "quick_notes::quick_notes_pick_images",
        "quick_notes::quick_notes_paste_clipboard_image",
        "quick_notes::quick_notes_import_dropped_images",
        "projects::icons::projects_icon_pick_image_file",
        "projects::icons::projects_icon_download_image_url",
        "projects::icons::projects_icon_asset_path",
        "music::music_pick_soundscape_file",
        "music::music_reveal_local_file",
        "music::library::commands::music_library_create_relink_plan",
        "music::library::commands::music_library_preview_item_repair",
        "music::library::commands::music_library_soundscapes",
        "music::library::commands::music_library_soundscape_groups",
        "music::library::commands::music_library_upsert_soundscape_group",
        "music::library::commands::music_library_remove_soundscape_group",
    ] {
        assert!(
            !MOBILE_RUNTIME.contains(command),
            "mobile runtime must exclude {command}"
        );
    }
}

const APP_COMMAND_PERMISSIONS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../permissions/app-commands.toml"
));
const DESKTOP_CAPABILITY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../capabilities/desktop.json"
));
const FOCUS_OVERLAY_CAPABILITY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../capabilities/focus-overlays.json"
));
const ANDROID_CAPABILITY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../capabilities/android.json"
));

/// Commands that only Rust calls, so no webview may invoke them.
const RUST_ONLY_COMMANDS: &[&str] = &[
    "chat_browser_snapshot",
    "chat_browser_click",
    "chat_browser_type",
    "chat_browser_press",
    "chat_browser_scroll",
    "focus_idle_status",
    "show_distractions_desktop_block_notification",
    "show_distractions_desktop_limit_notification",
];

/// Permissions that would let a webview create windows or webviews outside Rust validation.
const WINDOW_CREATION_PERMISSIONS: &[&str] = &[
    "core:default",
    "core:window:default",
    "core:webview:default",
    "core:window:allow-create",
    "core:webview:allow-create-webview",
    "core:webview:allow-create-webview-window",
];

/// Returns the command names inside a runtime's `generate_handler!` list.
fn registered_commands(runtime: &str) -> std::collections::BTreeSet<String> {
    let start = runtime
        .find("generate_handler![")
        .expect("runtime registers handlers")
        + "generate_handler![".len();
    let end = start + runtime[start..].find("])").expect("handler list ends");
    runtime[start..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("#[") && !line.starts_with("//"))
        .map(|line| {
            let path = line.trim_end_matches(',');
            path.rsplit("::").next().unwrap_or(path).to_owned()
        })
        .collect()
}

/// Returns the `commands.allow` entries of one permission in the app command manifest.
fn permission_commands(identifier: &str) -> std::collections::BTreeSet<String> {
    let marker = format!("identifier = \"{identifier}\"");
    let start = APP_COMMAND_PERMISSIONS
        .find(&marker)
        .unwrap_or_else(|| panic!("permission {identifier} exists"));
    let list_start = start
        + APP_COMMAND_PERMISSIONS[start..]
            .find("commands.allow = [")
            .expect("permission allows commands");
    let list_end = list_start
        + APP_COMMAND_PERMISSIONS[list_start..]
            .find(']')
            .expect("allow list ends");
    APP_COMMAND_PERMISSIONS[list_start..list_end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn capability(source: &str) -> serde_json::Value {
    serde_json::from_str(source).expect("capability is valid JSON")
}

fn capability_string_permissions(capability: &serde_json::Value) -> Vec<&str> {
    capability["permissions"]
        .as_array()
        .expect("capability lists permissions")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect()
}

#[test]
fn app_command_permission_matches_registered_commands() {
    let mut registered = registered_commands(DESKTOP_RUNTIME);
    registered.extend(registered_commands(MOBILE_RUNTIME));
    let allowed = permission_commands("app-commands");
    let unlisted: Vec<_> = registered.difference(&allowed).collect();
    let stale: Vec<_> = allowed.difference(&registered).collect();
    assert!(
        unlisted.is_empty(),
        "registered commands missing from permissions/app-commands.toml fail closed: {unlisted:?}"
    );
    assert!(
        stale.is_empty(),
        "permissions/app-commands.toml allows unregistered commands: {stale:?}"
    );
}

#[test]
fn rust_only_commands_are_not_registered_or_allowed() {
    let allowed = permission_commands("app-commands");
    for command in RUST_ONLY_COMMANDS {
        for (platform, runtime) in [("desktop", DESKTOP_RUNTIME), ("mobile", MOBILE_RUNTIME)] {
            assert!(
                !registered_commands(runtime).contains(*command),
                "{platform} runtime must not register {command}"
            );
        }
        assert!(!allowed.contains(*command), "{command} must not be allowed");
    }
}

#[test]
fn focus_overlay_commands_are_a_narrow_subset() {
    let all = permission_commands("app-commands");
    let overlay = permission_commands("focus-overlay-commands");
    assert!(!overlay.is_empty());
    for command in &overlay {
        assert!(
            all.contains(command),
            "overlay command {command} is registered"
        );
        assert!(
            command.starts_with("focus_")
                || command.starts_with("dismiss_pomodoro_")
                || command.starts_with("vault_read_config")
                || command.starts_with("vault_patch_config"),
            "overlay command {command} reaches beyond Focus and its window config"
        );
    }
}

#[test]
fn desktop_capabilities_target_webviews_without_window_creation() {
    for (name, source) in [
        ("desktop", DESKTOP_CAPABILITY),
        ("focus-overlays", FOCUS_OVERLAY_CAPABILITY),
    ] {
        let capability = capability(source);
        assert!(
            capability.get("windows").is_none(),
            "{name} must select webviews so Chat preview children of the main window get no grant"
        );
        let permissions = capability_string_permissions(&capability);
        for forbidden in WINDOW_CREATION_PERMISSIONS {
            assert!(
                !permissions.contains(forbidden),
                "{name} must not grant {forbidden}"
            );
        }
    }

    let desktop = capability(DESKTOP_CAPABILITY);
    assert_eq!(desktop["webviews"], serde_json::json!(["main", "view-*"]));
    assert!(capability_string_permissions(&desktop).contains(&"app-commands"));

    let overlay = capability(FOCUS_OVERLAY_CAPABILITY);
    assert_eq!(
        overlay["webviews"],
        serde_json::json!(["pomodoro-overlay-*"])
    );
    let overlay_permissions = capability_string_permissions(&overlay);
    assert!(overlay_permissions.contains(&"focus-overlay-commands"));
    assert!(!overlay_permissions.contains(&"app-commands"));
    assert_eq!(
        overlay["permissions"].as_array().map(Vec::len),
        Some(overlay_permissions.len()),
        "focus overlays need no scoped plugin permissions"
    );
    for permission in overlay_permissions {
        assert!(
            permission == "focus-overlay-commands"
                || permission.starts_with("core:event:allow-")
                || permission.starts_with("core:window:allow-set-")
                || permission == "core:window:allow-is-visible",
            "focus overlays must not receive {permission}"
        );
    }

    let android = capability(ANDROID_CAPABILITY);
    assert!(capability_string_permissions(&android).contains(&"app-commands"));
}
