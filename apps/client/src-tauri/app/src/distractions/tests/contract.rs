#[test]
fn command_adapters_are_registered_at_their_defining_modules() {
    let handlers = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/desktop.rs"
    ));
    for command in [
        "distractions::commands::distractions_open_extension_install_docs",
        "distractions::state_files::distractions_extension_status",
        "distractions::limits_read::distractions_load_usage_projection",
    ] {
        assert!(handlers.contains(command), "missing handler {command}");
    }
    for removed in [
        "distractions_close_desktop_app",
        "distractions_close_current_foreground_desktop_app",
        "distractions_list_blocked_desktop_app_matches",
        "distractions_record_usage_samples",
        "distractions_record_usage_sample",
        "distractions_list_usage_samples",
        "distractions_record_desktop_block_event",
    ] {
        assert!(
            !handlers.contains(removed),
            "frontend execution handler remains: {removed}"
        );
    }
    assert!(handlers.contains("distractions::runtime::setup(app.handle())"));
}

#[test]
fn platform_foreground_modules_keep_explicit_cfg_boundaries() {
    let module = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/distractions/foreground.rs"
    ));
    assert!(module.contains("target_os = \"linux\""));
    assert!(module.contains("target_os = \"macos\""));
    assert!(module.contains("cfg(windows)"));
}
