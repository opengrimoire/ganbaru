use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri_plugin_updater::{Update, UpdaterExt};

const RELEASE_PAGE_URL: &str = "https://github.com/opengrimoire/ganbaru/releases/latest";
/// Time allowed for fetching the update feed.
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Time allowed for downloading an update package.
const UPDATE_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Prefix of the authenticated comment line in a minisign signature.
const TRUSTED_COMMENT_PREFIX: &str = "trusted comment: ";
/// Field of the trusted comment that names the signed file.
const SIGNED_FILE_FIELD: &str = "file:";

/// Update found by the last check. Rust keeps it so the webview never chooses the feed, target,
/// headers, proxy, or downgrade policy.
pub struct PendingUpdate(UpdateSlot<Update>);

impl Default for PendingUpdate {
    fn default() -> Self {
        Self(UpdateSlot::default())
    }
}

/// Lifecycle of the offered update. A check in flight carries its request number so an earlier
/// check that lands late cannot replace a newer one, and an install owns its update until it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdatePhase<T> {
    Idle,
    Checking { request: u64 },
    Offered(T),
    Installing(T),
}

#[derive(Debug)]
struct UpdateSlotState<T> {
    phase: UpdatePhase<T>,
    next_request: u64,
}

/// Mutex-guarded [`UpdatePhase`] with the only legal moves between its variants.
#[derive(Debug)]
struct UpdateSlot<T>(Mutex<UpdateSlotState<T>>);

impl<T> Default for UpdateSlot<T> {
    fn default() -> Self {
        Self(Mutex::new(UpdateSlotState {
            phase: UpdatePhase::Idle,
            next_request: 0,
        }))
    }
}

impl<T: Clone> UpdateSlot<T> {
    fn lock(&self) -> std::sync::MutexGuard<'_, UpdateSlotState<T>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a check, dropping any earlier offer. Refused while an install runs.
    fn begin_check(&self) -> Result<u64, UpdateInstallError> {
        let mut state = self.lock();
        if matches!(state.phase, UpdatePhase::Installing(_)) {
            return Err(UpdateInstallError::InstallInProgress);
        }
        state.next_request += 1;
        let request = state.next_request;
        state.phase = UpdatePhase::Checking { request };
        Ok(request)
    }

    /// Records the result of check `request`; returns false when a newer check or an install took
    /// the slot meanwhile.
    fn finish_check(&self, request: u64, update: Option<T>) -> bool {
        let mut state = self.lock();
        if !matches!(state.phase, UpdatePhase::Checking { request: current } if current == request)
        {
            return false;
        }
        state.phase = update.map_or(UpdatePhase::Idle, UpdatePhase::Offered);
        true
    }

    /// Claims the offered update for one install. The claim puts the offer back when it is dropped
    /// without [`InstallClaim::complete`], so a failed or abandoned install can be retried.
    fn begin_install(&self) -> Result<InstallClaim<'_, T>, UpdateInstallError> {
        let mut state = self.lock();
        let update = match &state.phase {
            UpdatePhase::Offered(update) => update.clone(),
            UpdatePhase::Installing(_) => return Err(UpdateInstallError::InstallInProgress),
            UpdatePhase::Idle | UpdatePhase::Checking { .. } => {
                return Err(UpdateInstallError::NoPendingUpdate);
            }
        };
        state.phase = UpdatePhase::Installing(update.clone());
        Ok(InstallClaim {
            slot: self,
            update,
            completed: false,
        })
    }

    #[cfg(test)]
    fn phase(&self) -> UpdatePhase<T> {
        self.lock().phase.clone()
    }
}

/// One install's hold on the offered update.
struct InstallClaim<'a, T: Clone> {
    slot: &'a UpdateSlot<T>,
    update: T,
    completed: bool,
}

impl<T: Clone> InstallClaim<'_, T> {
    /// Ends the install with the update applied, leaving nothing offered.
    fn complete(mut self) {
        self.completed = true;
        self.slot.lock().phase = UpdatePhase::Idle;
    }
}

impl<T: Clone> Drop for InstallClaim<'_, T> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut state = self.slot.lock();
        if matches!(state.phase, UpdatePhase::Installing(_)) {
            state.phase = UpdatePhase::Offered(self.update.clone());
        }
    }
}

/// Newer release offered by the update feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    version: String,
    current_version: String,
    body: Option<String>,
}

impl From<&Update> for AvailableUpdate {
    fn from(update: &Update) -> Self {
        Self {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
            body: update.body.clone(),
        }
    }
}

/// Download progress streamed to the webview, shaped like the updater plugin's JavaScript events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", content = "data", rename_all_fields = "camelCase")]
pub enum UpdateDownloadEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

/// Reasons an update cannot be installed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateInstallError {
    SelfUpdateUnavailable,
    NoPendingUpdate,
    InstallInProgress,
    SignatureEncoding,
    MissingSignedFile,
    VersionNotSigned { version: String, file: String },
}

impl std::fmt::Display for UpdateInstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfUpdateUnavailable => {
                formatter.write_str("this installation is updated by its package manager")
            }
            Self::NoPendingUpdate => formatter.write_str("no update is ready to install"),
            Self::InstallInProgress => formatter.write_str("an update is already being installed"),
            Self::SignatureEncoding => formatter.write_str("the update signature is malformed"),
            Self::MissingSignedFile => {
                formatter.write_str("the update signature does not name its file")
            }
            Self::VersionNotSigned { version, file } => write!(
                formatter,
                "the update signature is for {file}, not version {version}"
            ),
        }
    }
}

/// Requires the authenticated signature comment to name a file built for `version`. Release assets
/// are named `<name>_<version>_<arch>`, so feed metadata cannot relabel an older signed package.
fn verify_signed_version(signature: &str, version: &str) -> Result<(), UpdateInstallError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(signature.trim())
        .map_err(|_| UpdateInstallError::SignatureEncoding)?;
    let text = std::str::from_utf8(&decoded).map_err(|_| UpdateInstallError::SignatureEncoding)?;
    let file = text
        .lines()
        .find_map(|line| line.strip_prefix(TRUSTED_COMMENT_PREFIX))
        .and_then(|comment| {
            comment
                .split('\t')
                .find_map(|field| field.strip_prefix(SIGNED_FILE_FIELD))
        })
        .filter(|file| !file.is_empty())
        .ok_or(UpdateInstallError::MissingSignedFile)?;
    if file.contains(&format!("_{version}_")) {
        Ok(())
    } else {
        Err(UpdateInstallError::VersionNotSigned {
            version: version.to_owned(),
            file: file.to_owned(),
        })
    }
}

/// Progress is advisory, so a closed window must not abort an install that is already downloading.
fn send_download_event(channel: &Channel<UpdateDownloadEvent>, event: UpdateDownloadEvent) {
    if let Err(error) = channel.send(event) {
        eprintln!("failed to report update download progress: {error}");
    }
}

/// Check the configured update feed. Only newer versions are offered.
#[tauri::command]
pub async fn updates_check(
    app: tauri::AppHandle,
    pending: tauri::State<'_, PendingUpdate>,
) -> Result<Option<AvailableUpdate>, String> {
    let request = pending.0.begin_check().map_err(|error| error.to_string())?;
    let checked = async {
        app.updater_builder()
            .timeout(UPDATE_CHECK_TIMEOUT)
            .build()?
            .check()
            .await
    }
    .await;
    let update = match checked {
        Ok(update) => update,
        Err(error) => {
            pending.0.finish_check(request, None);
            return Err(error.to_string());
        }
    };
    let available = update.as_ref().map(AvailableUpdate::from);
    // A newer check owns the slot, so this result is not offered.
    if !pending.0.finish_check(request, update) {
        return Ok(None);
    }
    Ok(available)
}

/// Download, verify, and install the update found by the last check. The plugin verifies the package
/// signature, and the signed file name must carry the offered version. A second install while one
/// runs is refused, and a failed install leaves the update offered.
#[tauri::command]
pub async fn updates_install(
    pending: tauri::State<'_, PendingUpdate>,
    on_event: Channel<UpdateDownloadEvent>,
) -> Result<(), String> {
    if !updater_install_context().self_updater {
        return Err(UpdateInstallError::SelfUpdateUnavailable.to_string());
    }
    let claim = pending
        .0
        .begin_install()
        .map_err(|error| error.to_string())?;
    let mut update = claim.update.clone();
    update.timeout = Some(UPDATE_DOWNLOAD_TIMEOUT);

    let mut started = false;
    let bytes = update
        .download(
            |chunk_length, content_length| {
                if !started {
                    started = true;
                    send_download_event(&on_event, UpdateDownloadEvent::Started { content_length });
                }
                send_download_event(&on_event, UpdateDownloadEvent::Progress { chunk_length });
            },
            || {
                send_download_event(&on_event, UpdateDownloadEvent::Finished);
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    verify_signed_version(&update.signature, &update.version).map_err(|error| error.to_string())?;
    update.install(bytes).map_err(|error| error.to_string())?;
    claim.complete();
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimePlatform {
    Linux,
    Windows,
    Other,
}

impl RuntimePlatform {
    fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdatePackageManager {
    Apt,
    Dnf,
    Zypper,
    AurYay,
    AurParu,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstallContext {
    self_updater: bool,
    package_manager: Option<UpdatePackageManager>,
    copy_command: Option<String>,
    release_page_url: String,
}

struct InstallDetectionInput<'a> {
    platform: RuntimePlatform,
    appimage_present: bool,
    os_release: Option<&'a str>,
    available_commands: &'a [&'a str],
}

#[tauri::command]
pub fn updater_install_context() -> UpdateInstallContext {
    let platform = RuntimePlatform::current();
    let os_release = if platform == RuntimePlatform::Linux {
        std::fs::read_to_string("/etc/os-release").ok()
    } else {
        None
    };
    let available_commands = if platform == RuntimePlatform::Linux {
        detected_aur_helpers()
    } else {
        Vec::new()
    };

    detect_update_install_context(InstallDetectionInput {
        platform,
        appimage_present: std::env::var_os("APPIMAGE").is_some(),
        os_release: os_release.as_deref(),
        available_commands: &available_commands,
    })
}

fn detect_update_install_context(input: InstallDetectionInput<'_>) -> UpdateInstallContext {
    if input.platform == RuntimePlatform::Windows
        || (input.platform == RuntimePlatform::Linux && input.appimage_present)
    {
        return UpdateInstallContext {
            self_updater: true,
            package_manager: None,
            copy_command: None,
            release_page_url: RELEASE_PAGE_URL.to_string(),
        };
    }

    let package_manager = if input.platform == RuntimePlatform::Linux {
        linux_package_manager(input.os_release, input.available_commands)
    } else {
        UpdatePackageManager::Fallback
    };
    let copy_command = copy_command_for_package_manager(package_manager).map(str::to_string);

    UpdateInstallContext {
        self_updater: false,
        package_manager: Some(package_manager),
        copy_command,
        release_page_url: RELEASE_PAGE_URL.to_string(),
    }
}

fn copy_command_for_package_manager(package_manager: UpdatePackageManager) -> Option<&'static str> {
    match package_manager {
        UpdatePackageManager::Apt => Some("sudo apt update && sudo apt install ganbaru"),
        UpdatePackageManager::Dnf => Some("sudo dnf upgrade ganbaru"),
        UpdatePackageManager::Zypper => Some("sudo zypper refresh && sudo zypper update ganbaru"),
        UpdatePackageManager::AurYay => Some("yay -Syu ganbaru-bin"),
        UpdatePackageManager::AurParu => Some("paru -Syu ganbaru-bin"),
        UpdatePackageManager::Fallback => None,
    }
}

fn linux_package_manager(
    os_release: Option<&str>,
    available_commands: &[&str],
) -> UpdatePackageManager {
    let Some(os_release) = os_release.map(parse_os_release) else {
        return UpdatePackageManager::Fallback;
    };

    if os_release.has_identifier(&["debian", "ubuntu", "linuxmint", "pop"]) {
        return UpdatePackageManager::Apt;
    }
    if os_release.has_identifier(&["fedora", "rhel", "centos", "rocky", "almalinux"]) {
        return UpdatePackageManager::Dnf;
    }
    if os_release.has_identifier(&[
        "opensuse",
        "opensuse-leap",
        "opensuse-tumbleweed",
        "suse",
        "sles",
    ]) {
        return UpdatePackageManager::Zypper;
    }
    if os_release.has_identifier(&["arch", "manjaro", "endeavouros", "garuda"]) {
        return aur_package_manager(available_commands);
    }

    UpdatePackageManager::Fallback
}

fn aur_package_manager(available_commands: &[&str]) -> UpdatePackageManager {
    if available_commands.contains(&"yay") {
        UpdatePackageManager::AurYay
    } else if available_commands.contains(&"paru") {
        UpdatePackageManager::AurParu
    } else {
        UpdatePackageManager::AurYay
    }
}

fn detected_aur_helpers() -> Vec<&'static str> {
    ["yay", "paru"]
        .into_iter()
        .filter(|command| command_exists(command))
        .collect()
}

fn command_exists(command: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&path_var).any(|directory| directory.join(command).is_file())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct OsRelease {
    id: Option<String>,
    id_like: Vec<String>,
}

impl OsRelease {
    fn has_identifier(&self, candidates: &[&str]) -> bool {
        let id_matches = self
            .id
            .as_deref()
            .is_some_and(|id| candidates.contains(&id));
        id_matches
            || self
                .id_like
                .iter()
                .any(|id_like| candidates.contains(&id_like.as_str()))
    }
}

fn parse_os_release(content: &str) -> OsRelease {
    let mut os_release = OsRelease::default();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };

        match key {
            "ID" => {
                os_release.id = Some(parse_os_release_value(value).to_ascii_lowercase());
            }
            "ID_LIKE" => {
                os_release.id_like = parse_os_release_value(value)
                    .split_whitespace()
                    .map(str::to_ascii_lowercase)
                    .collect();
            }
            _ => {}
        }
    }

    os_release
}

fn parse_os_release_value(raw: &str) -> String {
    let trimmed = raw.trim();
    let unquoted = if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    let mut parsed = String::with_capacity(unquoted.len());
    let mut chars = unquoted.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                parsed.push(next);
            }
        } else {
            parsed.push(ch);
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::{
        InstallDetectionInput, OsRelease, RuntimePlatform, UpdateDownloadEvent, UpdateInstallError,
        UpdatePackageManager, UpdatePhase, UpdateSlot, copy_command_for_package_manager,
        detect_update_install_context, linux_package_manager, parse_os_release,
        verify_signed_version,
    };
    use base64::Engine as _;

    /// Linux updater signature published with release 0.1.12.
    const SIGNATURE_0_1_12: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVURU9UaFhnY1ZKRERlWkZqV1NJK1hHNXZ5L2Rwc2NYcjZ2RVhDUi9GSUdIcU5pWDdtemxDWFVhNWxHdEFVZUNjSndTM1M5N1FzRjRHMjAvUGhkZmdxN0FlQVoyK3ZNYkFzPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMjg0MDExCWZpbGU6Z2FuYmFydV8wLjEuMTJfYW1kNjQuQXBwSW1hZ2UKb0dBSVd0bUkxa2ZrdGNGNDNzSHkwSDY2RjA2cVVDbkdTZTExRklyRWNTSi9hS3NwK2txRGpPL0RSNk54Uk45QVdRRThGeCtiYmY1aDkyQnNzcVlTREE9PQo=";

    fn encode(text: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    #[test]
    fn signed_file_name_must_carry_the_offered_version() {
        assert_eq!(verify_signed_version(SIGNATURE_0_1_12, "0.1.12"), Ok(()));
        for relabeled in ["0.1.13", "0.1.1", "0.2.0", "1.12"] {
            assert_eq!(
                verify_signed_version(SIGNATURE_0_1_12, relabeled),
                Err(UpdateInstallError::VersionNotSigned {
                    version: relabeled.to_owned(),
                    file: "ganbaru_0.1.12_amd64.AppImage".to_owned(),
                })
            );
        }
    }

    #[test]
    fn signatures_without_a_signed_file_name_are_rejected() {
        assert_eq!(
            verify_signed_version("not base64!", "0.1.12"),
            Err(UpdateInstallError::SignatureEncoding)
        );
        for text in [
            "untrusted comment: file:ganbaru_0.1.12_amd64.AppImage\nsig\ntrusted comment: timestamp:1\nglobal\n",
            "untrusted comment: x\nsig\ntrusted comment: timestamp:1\tfile:\nglobal\n",
            "untrusted comment: x\nsig\n",
        ] {
            assert_eq!(
                verify_signed_version(&encode(text), "0.1.12"),
                Err(UpdateInstallError::MissingSignedFile)
            );
        }
    }

    #[test]
    fn download_events_match_the_plugin_event_shape() {
        assert_eq!(
            serde_json::to_value(UpdateDownloadEvent::Started {
                content_length: Some(10)
            })
            .unwrap(),
            serde_json::json!({ "event": "Started", "data": { "contentLength": 10 } })
        );
        assert_eq!(
            serde_json::to_value(UpdateDownloadEvent::Progress { chunk_length: 4 }).unwrap(),
            serde_json::json!({ "event": "Progress", "data": { "chunkLength": 4 } })
        );
        assert_eq!(
            serde_json::to_value(UpdateDownloadEvent::Finished).unwrap(),
            serde_json::json!({ "event": "Finished" })
        );
    }

    #[test]
    fn appimage_runtime_uses_self_updater() {
        let context = detect_update_install_context(InstallDetectionInput {
            platform: RuntimePlatform::Linux,
            appimage_present: true,
            os_release: Some("ID=ubuntu\nID_LIKE=debian\n"),
            available_commands: &[],
        });

        assert!(context.self_updater);
        assert_eq!(context.package_manager, None);
        assert_eq!(context.copy_command, None);
    }

    #[test]
    fn windows_uses_self_updater() {
        let context = detect_update_install_context(InstallDetectionInput {
            platform: RuntimePlatform::Windows,
            appimage_present: false,
            os_release: None,
            available_commands: &[],
        });

        assert!(context.self_updater);
        assert_eq!(context.package_manager, None);
        assert_eq!(context.copy_command, None);
    }

    #[test]
    fn debian_like_system_maps_to_apt_command() {
        assert_eq!(
            linux_package_manager(Some("ID=ubuntu\nID_LIKE=\"debian\"\n"), &[]),
            UpdatePackageManager::Apt,
        );
        assert_eq!(
            copy_command_for_package_manager(UpdatePackageManager::Apt),
            Some("sudo apt update && sudo apt install ganbaru"),
        );
    }

    #[test]
    fn fedora_like_system_maps_to_dnf_command() {
        assert_eq!(
            linux_package_manager(Some("ID=rocky\nID_LIKE=\"rhel fedora\"\n"), &[]),
            UpdatePackageManager::Dnf,
        );
        assert_eq!(
            copy_command_for_package_manager(UpdatePackageManager::Dnf),
            Some("sudo dnf upgrade ganbaru"),
        );
    }

    #[test]
    fn opensuse_like_system_maps_to_zypper_command() {
        assert_eq!(
            linux_package_manager(
                Some("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n"),
                &[]
            ),
            UpdatePackageManager::Zypper,
        );
        assert_eq!(
            copy_command_for_package_manager(UpdatePackageManager::Zypper),
            Some("sudo zypper refresh && sudo zypper update ganbaru"),
        );
    }

    #[test]
    fn arch_like_system_maps_to_aur_helper_command() {
        assert_eq!(
            linux_package_manager(Some("ID=arch\n"), &["paru"]),
            UpdatePackageManager::AurParu,
        );
        assert_eq!(
            linux_package_manager(Some("ID=manjaro\nID_LIKE=arch\n"), &[]),
            UpdatePackageManager::AurYay,
        );
        assert_eq!(
            copy_command_for_package_manager(UpdatePackageManager::AurYay),
            Some("yay -Syu ganbaru-bin"),
        );
        assert_eq!(
            copy_command_for_package_manager(UpdatePackageManager::AurParu),
            Some("paru -Syu ganbaru-bin"),
        );
    }

    #[test]
    fn unknown_linux_system_uses_fallback_without_command() {
        let context = detect_update_install_context(InstallDetectionInput {
            platform: RuntimePlatform::Linux,
            appimage_present: false,
            os_release: Some("ID=void\n"),
            available_commands: &[],
        });

        assert!(!context.self_updater);
        assert_eq!(
            context.package_manager,
            Some(UpdatePackageManager::Fallback)
        );
        assert_eq!(context.copy_command, None);
    }

    #[test]
    fn parses_os_release_quotes_and_id_like_values() {
        assert_eq!(
            parse_os_release("ID=\"linuxmint\"\nID_LIKE=\"ubuntu debian\"\n"),
            OsRelease {
                id: Some("linuxmint".to_string()),
                id_like: vec!["ubuntu".to_string(), "debian".to_string()],
            },
        );
    }

    fn offered(version: &str) -> UpdateSlot<String> {
        let slot = UpdateSlot::default();
        let request = slot.begin_check().expect("check starts");
        assert!(slot.finish_check(request, Some(version.to_owned())));
        slot
    }

    #[test]
    fn a_second_install_is_refused_while_one_runs() {
        let slot = offered("0.3.0");
        let claim = slot
            .begin_install()
            .expect("first install claims the offer");

        assert_eq!(
            slot.begin_install().err(),
            Some(UpdateInstallError::InstallInProgress)
        );
        assert_eq!(claim.update, "0.3.0");
        assert_eq!(slot.phase(), UpdatePhase::Installing("0.3.0".to_owned()));
    }

    #[test]
    fn a_check_is_refused_while_an_install_runs() {
        let slot = offered("0.3.0");
        let _claim = slot.begin_install().expect("install claims the offer");

        assert_eq!(
            slot.begin_check(),
            Err(UpdateInstallError::InstallInProgress)
        );
    }

    #[test]
    fn a_completed_install_leaves_nothing_offered() {
        let slot = offered("0.3.0");
        slot.begin_install()
            .expect("install claims the offer")
            .complete();

        assert_eq!(slot.phase(), UpdatePhase::Idle);
        assert_eq!(
            slot.begin_install().err(),
            Some(UpdateInstallError::NoPendingUpdate)
        );
    }

    #[test]
    fn a_failed_or_abandoned_install_offers_the_update_again() {
        let slot = offered("0.3.0");
        drop(slot.begin_install().expect("install claims the offer"));

        assert_eq!(slot.phase(), UpdatePhase::Offered("0.3.0".to_owned()));
        assert!(slot.begin_install().is_ok());
    }

    #[test]
    fn nothing_installs_before_a_check_offers_an_update() {
        let slot = UpdateSlot::<String>::default();
        assert_eq!(
            slot.begin_install().err(),
            Some(UpdateInstallError::NoPendingUpdate)
        );
        slot.begin_check().expect("check starts");
        assert_eq!(
            slot.begin_install().err(),
            Some(UpdateInstallError::NoPendingUpdate)
        );
    }

    #[test]
    fn an_earlier_check_landing_late_does_not_replace_a_newer_one() {
        let slot = UpdateSlot::default();
        let earlier = slot.begin_check().expect("earlier check starts");
        let newer = slot.begin_check().expect("newer check starts");

        assert!(slot.finish_check(newer, Some("0.3.1".to_owned())));
        assert!(!slot.finish_check(earlier, Some("0.3.0".to_owned())));
        assert_eq!(slot.phase(), UpdatePhase::Offered("0.3.1".to_owned()));
    }

    #[test]
    fn a_new_check_drops_the_previous_offer() {
        let slot = offered("0.3.0");
        let request = slot.begin_check().expect("check starts");
        assert_eq!(slot.phase(), UpdatePhase::Checking { request });
        assert!(slot.finish_check(request, None));
        assert_eq!(slot.phase(), UpdatePhase::Idle);
    }
}
