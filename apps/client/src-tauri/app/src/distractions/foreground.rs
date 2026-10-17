use super::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(any(windows, test))]
mod windows;

#[cfg(target_os = "linux")]
pub(super) use linux::{close_current_foreground_desktop_app, foreground_desktop_app_status};
#[cfg(target_os = "macos")]
pub(super) use macos::{close_current_foreground_desktop_app, foreground_desktop_app_status};
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) use unsupported::{close_current_foreground_desktop_app, foreground_desktop_app_status};
#[cfg(windows)]
pub(super) use windows::{close_current_foreground_desktop_app, foreground_desktop_app_status};
