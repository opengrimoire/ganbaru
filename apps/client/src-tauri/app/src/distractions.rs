//! Distraction blocking. Desktop modules enforce browser and app rules from local state files;
//! Android drives the native blocker. Contracts, application rules, budget matching, usage
//! normalization, and elapsed accounting come from `ganbaru-distractions`; linked-device
//! accounting is shared.

#[cfg(desktop)]
use crate::vault;
#[cfg(desktop)]
use chrono::{DateTime, SecondsFormat, Utc};
#[cfg(desktop)]
use serde_json::Value;
#[cfg(all(desktop, target_os = "linux"))]
use sha2::{Digest, Sha256};
#[cfg(desktop)]
use std::collections::{HashMap, HashSet};
#[cfg(desktop)]
use std::path::{Path, PathBuf};
#[cfg(all(desktop, target_os = "linux"))]
use std::process::Stdio;
#[cfg(desktop)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(desktop)]
use tauri::{Manager, Runtime};

#[cfg(any(target_os = "android", all(test, not(target_os = "ios"))))]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) mod android;
#[cfg(desktop)]
mod authorization;
#[cfg(desktop)]
pub(crate) mod catalog;
#[cfg(desktop)]
pub(crate) mod commands;
#[cfg(desktop)]
mod foreground;
#[cfg(desktop)]
pub(crate) mod limits_read;
pub(crate) mod linked;
#[cfg(desktop)]
mod process_control;
#[cfg(desktop)]
mod processes;
#[cfg(desktop)]
pub(crate) mod runtime;
#[cfg(desktop)]
pub(crate) mod state_files;

pub(crate) use ganbaru_distractions::limits;
#[cfg(desktop)]
use ganbaru_distractions::{contracts, rules, usage};

#[cfg(desktop)]
use contracts::DistractionsExtensionConnectionFile;
#[cfg(all(desktop, any(target_os = "linux", test)))]
use contracts::ObservedDesktopProcess;
#[cfg(desktop)]
#[allow(unused_imports)]
pub use contracts::{
    DistractionsCloseDesktopAppRequest, DistractionsDesktopAppCandidate,
    DistractionsDesktopAppRuleInput, DistractionsDesktopBlockEventInput,
    DistractionsDesktopRuleIdentity, DistractionsExtensionStatus,
    DistractionsForegroundDesktopAppExpectation, DistractionsForegroundDesktopAppStatus,
    DistractionsLimitState, DistractionsLimitStateItem, DistractionsRunningDesktopAppMatch,
    DistractionsRuntimeState, DistractionsUsageSampleInput, DistractionsUsageSampleRow,
};

#[cfg(desktop)]
use authorization::{load_close_authorization, validate_names_authorized};
#[cfg(desktop)]
use foreground::{close_current_foreground_desktop_app, foreground_desktop_app_status};
#[cfg(desktop)]
use processes::list_blocked_desktop_app_matches;
#[cfg(all(desktop, target_os = "linux"))]
use processes::{observe_linux_process, read_linux_process_names};
#[cfg(all(desktop, any(target_os = "linux", test)))]
use rules::desktop_rule_matchers;
#[cfg(desktop)]
use rules::{
    app_name_key, foreground_expectation_matches, foreground_status_from_parts,
    foreground_status_match_names, is_protected_desktop_app_candidate,
    is_protected_desktop_app_name, normalize_app_candidate_name, normalize_process_match_name,
    normalize_process_match_names, unavailable_foreground_desktop_app_status,
    validate_foreground_status_is_closeable,
};
#[cfg(desktop)]
pub use state_files::clear_distractions_enforcement_state;
#[cfg(desktop)]
use state_files::{limit_state_path, read_fresh_limit_state, read_fresh_runtime_state, state_path};

#[cfg(desktop)]
use limits::is_valid_local_date;

#[cfg(all(desktop, test))]
use state_files::{
    clear_enforcement_state_files, extension_status_from_file_contents, validate_limit_state,
    validate_state, write_text_file_atomically,
};

#[cfg(all(desktop, test))]
use catalog::{parse_desktop_entry, sort_and_deduplicate_candidates};

#[cfg(all(desktop, test, target_os = "linux"))]
use catalog::process_name_from_exec;

#[cfg(all(desktop, test))]
use authorization::{configured_close_authorization, read_bounded_authorization_config};

#[cfg(all(desktop, test, target_os = "linux"))]
use process_control::{
    DesktopProcessController, DesktopProcessSignal, close_desktop_process_with,
    validate_observed_close_process,
};

#[cfg(desktop)]
const STATE_FILE: &str = "distractions-state.json";
#[cfg(desktop)]
const EXTENSION_CONNECTION_FILE: &str = "distractions-extension-status.json";
#[cfg(desktop)]
const LIMIT_STATE_FILE: &str = "distractions-limit-state.json";
#[cfg(desktop)]
const VAULT_CONFIG_FILE: &str = "config.json";
#[cfg(desktop)]
const MAX_AUTHORIZATION_CONFIG_BYTES: u64 = 1024 * 1024;
#[cfg(desktop)]
const LIMIT_STATE_STALE_SECONDS: i64 = 20;
#[cfg(desktop)]
const EXTENSION_CONNECTION_STALE_SECONDS: i64 = 60;
#[cfg(desktop)]
const ACTIVE_STATE_STALE_SECONDS: i64 = 45;
#[cfg(desktop)]
static DESKTOP_APP_LIST_GENERATION: AtomicU64 = AtomicU64::new(0);
#[cfg(desktop)]
const EXTENSION_INSTALL_README_URL: &str =
    "https://github.com/opengrimoire/ganbaru/blob/dev/extensions/chromium/README.md";

#[cfg(desktop)]
fn now_utc() -> DateTime<Utc> {
    std::time::SystemTime::now().into()
}

#[cfg(desktop)]
fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(all(desktop, test))]
mod tests;
