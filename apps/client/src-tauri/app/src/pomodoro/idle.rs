//! Desktop idle and webcam activity probes used for Focus admission and idle warnings.

use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
use crate::system_command::fixed_command_output;
#[cfg(target_os = "macos")]
use crate::system_command::fixed_command_status;

#[derive(Clone, Serialize, Deserialize)]
pub struct IdleStatus {
    pub idle_ms: Option<u64>,
    pub webcam_in_use: bool,
}

#[cfg(target_os = "linux")]
fn mutter_idle_time_ms() -> Option<u64> {
    let stdout = fixed_command_output(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.gnome.Mutter.IdleMonitor",
            "--object-path",
            "/org/gnome/Mutter/IdleMonitor/Core",
            "--method",
            "org.gnome.Mutter.IdleMonitor.GetIdletime",
        ],
        1024,
    )
    .ok()?;

    stdout
        .trim()
        .strip_prefix("(uint64 ")?
        .strip_suffix(",)")?
        .parse::<u64>()
        .ok()
}

#[cfg(target_os = "linux")]
fn is_webcam_in_use() -> bool {
    let own_pid = std::process::id();
    let entries = match std::fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if name_str == own_pid.to_string() {
            continue;
        }
        let fd_dir = format!("/proc/{name_str}/fd");
        let fds = match std::fs::read_dir(&fd_dir) {
            Ok(f) => f,
            Err(_) => continue,
        };
        for fd_entry in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd_entry.path()) {
                if let Some(s) = target.to_str() {
                    if s.starts_with("/dev/video") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn linux_idle_time_ms() -> Option<u64> {
    if let Some(ms) = mutter_idle_time_ms() {
        return Some(ms);
    }
    if let Ok(stdout) = fixed_command_output("xprintidle", &[], 128) {
        if let Ok(ms) = stdout.trim().parse::<u64>() {
            return Some(ms);
        }
    }
    None
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn focus_idle_status() -> IdleStatus {
    IdleStatus {
        idle_ms: linux_idle_time_ms(),
        webcam_in_use: is_webcam_in_use(),
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_idle_elapsed_ms(now: u32, last_input: u32) -> u64 {
    u64::from(now.wrapping_sub(last_input))
}

#[cfg(target_os = "windows")]
fn windows_idle_time_ms() -> Option<u64> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut last_input_info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };

    // SAFETY: `last_input_info` is a valid LASTINPUTINFO output buffer with cbSize set to
    // the struct size required by GetLastInputInfo.
    if unsafe { GetLastInputInfo(&mut last_input_info) }.as_bool() {
        // SAFETY: GetTickCount reads the current Windows uptime tick and does
        // not require any pointer or handle ownership from this process.
        let now = unsafe { GetTickCount() };
        Some(windows_idle_elapsed_ms(now, last_input_info.dwTime))
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn is_webcam_in_use_windows() -> bool {
    let output = fixed_command_output(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            r#"Get-ItemProperty -Path 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\*\*' -Name LastUsedTimeStop -ErrorAction SilentlyContinue | Where-Object { $_.LastUsedTimeStop -eq 0 } | Measure-Object | Select-Object -ExpandProperty Count"#,
        ],
        128,
    );
    output
        .ok()
        .and_then(|stdout| stdout.trim().parse::<u32>().ok())
        .unwrap_or(0)
        > 0
}

#[cfg(target_os = "windows")]
#[tauri::command]
pub fn focus_idle_status() -> IdleStatus {
    IdleStatus {
        idle_ms: windows_idle_time_ms(),
        webcam_in_use: is_webcam_in_use_windows(),
    }
}

#[cfg(target_os = "macos")]
fn macos_idle_time_ms() -> Option<u64> {
    let stdout =
        match fixed_command_output("ioreg", &["-c", "IOHIDSystem", "-d", "4", "-S"], 256 * 1024) {
            Ok(stdout) => stdout,
            Err(_) => return None,
        };
    for line in stdout.lines() {
        if let Some(pos) = line.find("\"HIDIdleTime\"") {
            if let Some(eq) = line[pos..].find('=') {
                let value = line[pos + eq + 1..].trim();
                if let Ok(ns) = value.parse::<u64>() {
                    return Some(ns / 1_000_000);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn is_webcam_in_use_macos() -> bool {
    ["VDCAssistant", "AppleCameraAssistant", "appleh13camerad"]
        .iter()
        .any(|process_name| fixed_command_status("pgrep", &["-x", process_name]))
}

#[cfg(target_os = "macos")]
#[tauri::command]
pub fn focus_idle_status() -> IdleStatus {
    IdleStatus {
        idle_ms: macos_idle_time_ms(),
        webcam_in_use: is_webcam_in_use_macos(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
#[tauri::command]
pub fn focus_idle_status() -> IdleStatus {
    IdleStatus {
        idle_ms: None,
        webcam_in_use: false,
    }
}

#[cfg(test)]
mod tests {
    use super::windows_idle_elapsed_ms;

    #[test]
    fn windows_idle_elapsed_handles_tick_wraparound() {
        assert_eq!(windows_idle_elapsed_ms(25, u32::MAX - 24), 50);
    }

    #[test]
    fn windows_idle_elapsed_preserves_the_u32_range() {
        assert_eq!(windows_idle_elapsed_ms(u32::MAX, 0), u64::from(u32::MAX));
    }
}
