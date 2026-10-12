use super::*;
use std::process::Stdio;

#[cfg(target_os = "linux")]
fn open_fixed_url(url: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open browser: {e}"))
}

#[cfg(target_os = "macos")]
fn open_fixed_url(url: &str) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open browser: {e}"))
}

#[cfg(windows)]
fn open_fixed_url(url: &str) -> Result<(), String> {
    std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open browser: {e}"))
}

#[tauri::command]
pub fn distractions_open_extension_install_docs() -> Result<(), String> {
    open_fixed_url(EXTENSION_INSTALL_README_URL)
}
