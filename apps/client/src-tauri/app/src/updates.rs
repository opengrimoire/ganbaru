use serde::Serialize;

const RELEASE_PAGE_URL: &str = "https://github.com/opengrimoire/ganbaru/releases/latest";

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
        InstallDetectionInput, OsRelease, RuntimePlatform, UpdatePackageManager,
        copy_command_for_package_manager, detect_update_install_context, linux_package_manager,
        parse_os_release,
    };

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
}
