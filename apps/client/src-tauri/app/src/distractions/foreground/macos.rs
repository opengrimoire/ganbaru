use super::*;

#[cfg(target_os = "macos")]
fn normalized_ns_app_name(
    value: Option<objc2::rc::Retained<objc2_foundation::NSString>>,
) -> Option<String> {
    value
        .map(|value| value.to_string())
        .and_then(|value| normalize_app_candidate_name(&value))
}

#[cfg(target_os = "macos")]
pub(in crate::distractions) fn foreground_desktop_app_status()
-> DistractionsForegroundDesktopAppStatus {
    use objc2_app_kit::NSWorkspace;

    let workspace = NSWorkspace::sharedWorkspace();
    let Some(app) = workspace.frontmostApplication() else {
        return unavailable_foreground_desktop_app_status("no foreground app is active");
    };
    let app_name = normalized_ns_app_name(app.localizedName())
        .or_else(|| normalized_ns_app_name(app.bundleIdentifier()));
    let Some(app_name) = app_name else {
        return unavailable_foreground_desktop_app_status("foreground app name is unavailable");
    };
    let process_id = app.processIdentifier();
    let process_id = if process_id > 0 {
        Some(process_id as u32)
    } else {
        None
    };
    let bundle_id = normalized_ns_app_name(app.bundleIdentifier());
    let mut match_names = Vec::new();
    if let Some(bundle_id) = bundle_id {
        match_names.push(bundle_id);
    }
    foreground_status_from_parts(app_name.clone(), Some(app_name), process_id, match_names)
}

#[cfg(target_os = "macos")]
pub(in crate::distractions) fn close_current_foreground_desktop_app(
    expected: DistractionsForegroundDesktopAppExpectation,
    authorize: &mut dyn FnMut(&DistractionsForegroundDesktopAppStatus) -> Result<(), String>,
) -> Result<(), String> {
    use objc2_app_kit::NSRunningApplication;

    let status = foreground_desktop_app_status();
    validate_foreground_status_is_closeable(&status)?;
    if !foreground_expectation_matches(&status, &expected) {
        return Err("foreground app changed before it could be closed".to_string());
    }
    authorize(&status)?;
    let process_id = status
        .process_id
        .ok_or_else(|| "foreground app process is unavailable".to_string())?;
    let Some(app) =
        NSRunningApplication::runningApplicationWithProcessIdentifier(process_id as libc::pid_t)
    else {
        return Err("foreground app is no longer running".to_string());
    };
    if app.terminate() {
        Ok(())
    } else {
        Err("foreground app refused the close request".to_string())
    }
}
