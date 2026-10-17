#[cfg(windows)]
use super::*;

#[cfg(any(windows, test))]
fn validated_window_process_id(thread_id: u32, process_id: u32) -> Option<u32> {
    (thread_id != 0 && process_id != 0).then_some(process_id)
}

#[cfg(windows)]
fn windows_foreground_window() -> Option<::windows::Win32::Foundation::HWND> {
    use ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY: This getter accepts no pointers and returns a borrowed window
    // identifier. A null result is rejected before the identifier is used.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() { None } else { Some(hwnd) }
}

#[cfg(windows)]
fn windows_foreground_process_id(hwnd: ::windows::Win32::Foundation::HWND) -> Option<u32> {
    use ::windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut process_id = 0;
    // SAFETY: `hwnd` came from Windows, and `process_id` is valid writable
    // storage for the duration of the call. Windows does not retain the pointer.
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id as *mut u32)) };
    validated_window_process_id(thread_id, process_id)
}

#[cfg(test)]
mod tests {
    use super::validated_window_process_id;

    #[test]
    fn foreground_process_identity_requires_api_success_and_nonzero_output() {
        assert_eq!(validated_window_process_id(7, 11), Some(11));
        assert_eq!(validated_window_process_id(0, 11), None);
        assert_eq!(validated_window_process_id(7, 0), None);
    }
}

#[cfg(windows)]
fn windows_process_image_path(process_id: u32) -> Option<String> {
    use ::windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use ::windows::core::{Owned, PWSTR};

    // SAFETY: No pointers are passed, inheritance is disabled, and Windows
    // returns a fresh owning process handle on success.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()? };
    // SAFETY: `OpenProcess` returned a fresh owning handle above. Ownership is
    // transferred exactly once and released on every return path.
    let handle = unsafe { Owned::new(handle) };
    let mut buffer = vec![0u16; 32_768];
    let mut size = u32::try_from(buffer.len()).ok()?;
    // SAFETY: `handle` owns a live process handle with query access. `buffer` is
    // writable for the capacity reported through `size`, and the size pointer is
    // valid for the call. Windows does not retain either pointer.
    let result = unsafe {
        QueryFullProcessImageNameW(
            *handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size as *mut u32,
        )
    };
    result.ok()?;
    let size = usize::try_from(size).ok()?;
    if size == 0 || size > buffer.len() {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..size]))
}

#[cfg(windows)]
fn windows_status_for_window(
    hwnd: ::windows::Win32::Foundation::HWND,
) -> DistractionsForegroundDesktopAppStatus {
    let Some(process_id) = windows_foreground_process_id(hwnd) else {
        return unavailable_foreground_desktop_app_status(
            "foreground window process is unavailable",
        );
    };
    let process_path = windows_process_image_path(process_id);
    let process_name = process_path
        .as_deref()
        .and_then(normalize_process_match_name)
        .or_else(|| Some(format!("process-{process_id}")));
    let app_name = process_path
        .as_deref()
        .and_then(|path| Path::new(path).file_stem().and_then(|name| name.to_str()))
        .and_then(normalize_app_candidate_name)
        .or_else(|| process_name.clone())
        .unwrap_or_else(|| format!("process-{process_id}"));
    let mut match_names = Vec::new();
    if let Some(path) = process_path {
        match_names.push(path);
    }
    foreground_status_from_parts(app_name, process_name, Some(process_id), match_names)
}

#[cfg(windows)]
pub(in crate::distractions) fn foreground_desktop_app_status()
-> DistractionsForegroundDesktopAppStatus {
    let Some(hwnd) = windows_foreground_window() else {
        return unavailable_foreground_desktop_app_status("no foreground window is active");
    };
    windows_status_for_window(hwnd)
}

#[cfg(windows)]
pub(in crate::distractions) fn close_current_foreground_desktop_app(
    expected: DistractionsForegroundDesktopAppExpectation,
    authorize: &mut dyn FnMut(&DistractionsForegroundDesktopAppStatus) -> Result<(), String>,
) -> Result<(), String> {
    use ::windows::Win32::Foundation::{LPARAM, WPARAM};
    use ::windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

    let hwnd =
        windows_foreground_window().ok_or_else(|| "no foreground window is active".to_string())?;
    let status = windows_status_for_window(hwnd);
    validate_foreground_status_is_closeable(&status)?;
    if !foreground_expectation_matches(&status, &expected) {
        return Err("foreground app changed before it could be closed".to_string());
    }
    authorize(&status)?;

    let current_hwnd = windows_foreground_window()
        .ok_or_else(|| "foreground app changed before it could be closed".to_string())?;
    let current_status = windows_status_for_window(current_hwnd);
    if current_hwnd != hwnd
        || current_status != status
        || !foreground_expectation_matches(&current_status, &expected)
    {
        return Err("foreground app changed before it could be closed".to_string());
    }

    // SAFETY: `current_hwnd` was just revalidated as the original foreground
    // window with the authorized process ID. WM_CLOSE carries only scalar values,
    // so Windows does not borrow any Rust memory after this call.
    unsafe {
        PostMessageW(Some(current_hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
            .map_err(|e| format!("close foreground window: {e}"))?;
    }
    Ok(())
}
