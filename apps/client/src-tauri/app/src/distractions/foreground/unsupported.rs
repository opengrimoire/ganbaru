use super::*;

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(in crate::distractions) fn foreground_desktop_app_status()
-> DistractionsForegroundDesktopAppStatus {
    unavailable_foreground_desktop_app_status(
        "foreground desktop app detection is not supported on this platform",
    )
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(in crate::distractions) fn close_current_foreground_desktop_app(
    _expected: DistractionsForegroundDesktopAppExpectation,
    _authorize: &mut dyn FnMut(&DistractionsForegroundDesktopAppStatus) -> Result<(), String>,
) -> Result<(), String> {
    Err("foreground desktop app closing is not supported on this platform".to_string())
}
