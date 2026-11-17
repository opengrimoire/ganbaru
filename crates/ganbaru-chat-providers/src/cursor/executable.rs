//! Cursor executable, probe, launch, environment, and continuation identity.

use super::protocol::{
    CURSOR_COMPATIBILITY, CursorProviderSettings, GROK_COMPATIBILITY, MINIMUM_CURSOR_VERSION,
    protocol_error,
};
use crate::models::*;
use crate::process::{
    BoundedCommand, BoundedCommandFailure, ProviderProcessConfig, run_bounded_command,
    spawn_provider_process,
};
use crate::registry::{CoreVersion, VersionOrder, admit_version, admits_sessions, version_refusal};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const CURSOR_STDERR_LIMIT_BYTES: usize = 256 * 1024;
const VERSION_OUTPUT_LIMIT_BYTES: usize = 16 * 1024;
const ABOUT_TIMEOUT: Duration = Duration::from_secs(5);
/// Appended to the continuation group digest of confined Cursor launches only.
const CONFINED_CONTINUATION_MARKER: &[u8] = b"\0confined";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CursorVersion {
    pub date: u32,
}

impl std::fmt::Display for CursorVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let year = self.date / 10_000;
        let month = (self.date / 100) % 100;
        let day = self.date % 100;
        write!(formatter, "{year:04}.{month:02}.{day:02}")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorAbout {
    pub version: CursorVersion,
    pub account_label: Option<String>,
    pub authenticated: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcpProviderAbout {
    pub version: String,
    pub account_label: Option<String>,
    pub authenticated: Option<bool>,
}

impl From<CursorAbout> for AcpProviderAbout {
    fn from(value: CursorAbout) -> Self {
        Self {
            version: value.version.to_string(),
            account_label: value.account_label,
            authenticated: value.authenticated,
        }
    }
}

pub fn process_environment(
    configuration: &ProviderInstanceConfig,
) -> ChatResult<BTreeMap<String, String>> {
    process_environment_for(configuration, "Cursor")
}

pub fn process_environment_for(
    configuration: &ProviderInstanceConfig,
    provider_name: &str,
) -> ChatResult<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for name in inherited_environment_names() {
        if let Some(value) = crate::host_path::host_variable(name) {
            if !value.contains('\0') {
                environment.insert((*name).to_string(), value);
            }
        }
    }
    for (name, value) in &configuration.environment {
        if name.is_empty()
            || name.contains(['=', '\0'])
            || value.contains('\0')
            || matches!(name.to_ascii_uppercase().as_str(), "HOME" | "USERPROFILE")
        {
            return Err(ChatError::validation(
                "environment",
                format!("{provider_name} environment contains a prohibited entry"),
            ));
        }
        environment.insert(name.clone(), value.clone());
    }
    if configuration.family_id.as_str() == "grok" {
        if let Some(home) = grok_home_setting(configuration) {
            let path = Path::new(home);
            if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
                return Err(ChatError::validation(
                    "providerHome",
                    "Grok home must be an absolute path",
                ));
            }
            environment.insert("GROK_HOME".to_string(), home.to_string());
        }
    }
    Ok(environment)
}

/// The Grok home a launch names: the private home Ganbaru owns when one is set, otherwise the
/// configured provider home.
fn grok_home_setting(configuration: &ProviderInstanceConfig) -> Option<&str> {
    configuration
        .private_home
        .as_deref()
        .or(configuration.provider_home.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Creates the private Grok home of an API key launch, readable only by this user, since Grok
/// keeps its sessions there.
fn create_private_home(home: &Path) -> ChatResult<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(home).map_err(|error| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            format!("Grok private home could not be created: {}", error.kind()),
            true,
        )
    })
}

pub async fn probe_grok_about(
    executable: &Path,
    working_directory: &Path,
    environment: BTreeMap<String, String>,
) -> ChatResult<AcpProviderAbout> {
    let output = run_bounded_command(BoundedCommand {
        executable,
        arguments: &["--version"],
        working_directory,
        environment: &environment,
        output_limit_bytes: VERSION_OUTPUT_LIMIT_BYTES,
        timeout: ABOUT_TIMEOUT,
    })
    .await
    .map_err(|failure| match failure {
        BoundedCommandFailure::Unavailable => grok_executable_missing_error(),
        BoundedCommandFailure::TimedOut => {
            ChatError::new(ChatErrorCode::Timeout, "Grok version probe timed out", true)
        }
        BoundedCommandFailure::Oversized => protocol_error("Grok version output"),
    })?;
    if !output.successful {
        return Err(ChatError::new(
            ChatErrorCode::UnsupportedVersion,
            "Grok CLI did not report a supported version",
            true,
        ));
    }
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let version = combined
        .split_whitespace()
        .find(|part| part.chars().any(|character| character.is_ascii_digit()))
        .map(|part| {
            part.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '.' && character != '-'
            })
        })
        .filter(|part| !part.is_empty() && part.len() <= 128)
        .ok_or_else(|| protocol_error("Grok CLI version"))?;
    Ok(AcpProviderAbout {
        version: version.to_string(),
        account_label: None,
        authenticated: None,
    })
}

pub fn resolve_executable(
    configured: &str,
    environment: &BTreeMap<String, String>,
) -> ChatResult<PathBuf> {
    let configured = configured.trim();
    if configured.is_empty() || configured.contains('\0') {
        return Err(ChatError::validation(
            "executable",
            "ACP provider executable is required",
        ));
    }
    let candidate = if path_has_separator(configured) {
        let path = PathBuf::from(configured);
        if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
            return Err(ChatError::validation(
                "executable",
                "ACP provider executable path must be absolute",
            ));
        }
        path
    } else {
        find_on_path(configured, environment).ok_or_else(executable_missing_error)?
    };
    let resolved = fs::canonicalize(candidate).map_err(|_| executable_missing_error())?;
    if !resolved.is_file() || !is_executable(&resolved) {
        return Err(executable_missing_error());
    }
    if matches!(
        resolved.extension().and_then(|value| value.to_str()),
        Some("cmd" | "bat" | "ps1")
    ) {
        return Err(ChatError::unsupported(
            "ACP command shims are unsupported; configure the native executable",
        ));
    }
    Ok(resolved)
}

pub async fn probe_about(
    executable: &Path,
    working_directory: &Path,
    environment: BTreeMap<String, String>,
) -> ChatResult<CursorAbout> {
    let output = run_bounded_command(BoundedCommand {
        executable,
        arguments: &["about", "--json"],
        working_directory,
        environment: &environment,
        output_limit_bytes: VERSION_OUTPUT_LIMIT_BYTES,
        timeout: ABOUT_TIMEOUT,
    })
    .await
    .map_err(|failure| match failure {
        BoundedCommandFailure::Unavailable => executable_missing_error(),
        BoundedCommandFailure::TimedOut => {
            ChatError::new(ChatErrorCode::Timeout, "Cursor about probe timed out", true)
        }
        BoundedCommandFailure::Oversized => protocol_error("Cursor about output"),
    })?;
    parse_about(&output.stdout, &output.stderr, output.successful)
}

pub fn parse_about(stdout: &[u8], stderr: &[u8], success: bool) -> ChatResult<CursorAbout> {
    let stdout = String::from_utf8_lossy(stdout);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&stdout) {
        let version = value
            .get("cliVersion")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| protocol_error("Cursor CLI version"))?;
        let account = value
            .get("userEmail")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let authenticated = if value
            .get("userEmail")
            .is_some_and(serde_json::Value::is_null)
        {
            Some(false)
        } else {
            account.as_ref().map(|_| true)
        };
        return Ok(CursorAbout {
            version: parse_version(version)?,
            account_label: account,
            authenticated,
        });
    }
    let combined = format!("{stdout}\n{}", String::from_utf8_lossy(stderr));
    let version = field(&combined, "CLI Version")
        .or_else(|| {
            combined
                .split_whitespace()
                .find(|value| parse_version(value).is_ok())
        })
        .ok_or_else(|| protocol_error("Cursor CLI version"))?;
    let account = field(&combined, "User Email").map(str::to_string);
    let lower = combined.to_ascii_lowercase();
    let authenticated = if lower.contains("not logged in")
        || lower.contains("authentication required")
        || lower.contains("login required")
    {
        Some(false)
    } else if account.is_some() {
        Some(true)
    } else if success {
        None
    } else {
        Some(false)
    };
    Ok(CursorAbout {
        version: parse_version(version)?,
        account_label: account,
        authenticated,
    })
}

impl VersionOrder for CursorVersion {
    fn parse_version(value: &str) -> ChatResult<Self> {
        parse_version(value)
    }
}

/// Where `version` falls in `CURSOR_COMPATIBILITY`, refusing a build older than the minimum.
pub fn ensure_supported_version(version: CursorVersion) -> ChatResult<ProviderVersionAdmission> {
    let text = version.to_string();
    let admission = admit_version::<CursorVersion>(&CURSOR_COMPATIBILITY, &text)?;
    if admission == ProviderVersionAdmission::BelowMinimum {
        return Err(version_refusal(
            admission,
            &text,
            format!(
                "Cursor Agent {version} is unsupported; version {MINIMUM_CURSOR_VERSION} or newer is required"
            ),
            false,
        ));
    }
    Ok(admission)
}

/// Where the Grok `version` falls in `GROK_COMPATIBILITY`, refusing one below the minimum or at
/// or above the ceiling, since an older build may ignore the switches that keep it offline.
pub fn ensure_supported_grok_version(version: &str) -> ChatResult<ProviderVersionAdmission> {
    let admission = admit_version::<CoreVersion>(&GROK_COMPATIBILITY, version)?;
    if admits_sessions(admission) {
        return Ok(admission);
    }
    Err(version_refusal(
        admission,
        version,
        format!(
            "Grok {version} is unsupported; Ganbaru supports versions from {} and below {}",
            GROK_COMPATIBILITY.minimum,
            GROK_COMPATIBILITY.ceiling_exclusive.unwrap_or_default()
        ),
        true,
    ))
}

/// The oldest Cursor build teammate runs on this device accept, when `version` is older: only a
/// device with the sandbox helper confines teammates, and that needs a verified build.
pub fn cursor_teammate_minimum_version(version: &str, confined_commands: bool) -> Option<String> {
    let minimum = super::confinement::MINIMUM_CONFINED_CURSOR_VERSION;
    parse_version(version)
        .ok()
        .filter(|version| confined_commands && *version < minimum)
        .map(|_| minimum.to_string())
}

pub fn parse_version(value: &str) -> ChatResult<CursorVersion> {
    let prefix = value.trim().split('-').next().unwrap_or_default();
    let parts = prefix.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(protocol_error("Cursor CLI version"));
    }
    let year = parts[0].parse::<u32>().ok();
    let month = parts[1].parse::<u32>().ok();
    let day = parts[2].parse::<u32>().ok();
    match (year, month, day) {
        (Some(year), Some(month), Some(day))
            if (2020..=9999).contains(&year)
                && (1..=12).contains(&month)
                && (1..=31).contains(&day) =>
        {
            Ok(CursorVersion {
                date: year * 10_000 + month * 100 + day,
            })
        }
        _ => Err(protocol_error("Cursor CLI version")),
    }
}

pub fn launch_arguments(
    configured: &[String],
    settings: &CursorProviderSettings,
) -> ChatResult<Vec<String>> {
    if configured.iter().any(|argument| {
        let normalized = argument.trim().to_ascii_lowercase();
        argument.contains('\0')
            || matches!(
                normalized.as_str(),
                "acp" | "-e" | "--endpoint" | "--api-key" | "-a"
            )
            || normalized.starts_with("--endpoint=")
            || normalized.starts_with("--api-key=")
    }) {
        return Err(ChatError::validation(
            "launchArguments",
            "Cursor launch arguments cannot override protocol, endpoint, or credentials",
        ));
    }
    let mut arguments = configured.to_vec();
    if let Some(endpoint) = settings.api_endpoint.as_ref() {
        arguments.extend(["-e".to_string(), endpoint.clone()]);
    }
    arguments.push("acp".to_string());
    Ok(arguments)
}

/// Starts Cursor over ACP, under the sandbox helper for an organizational launch when the
/// device has one; see [`super::confinement`].
pub fn spawn_connection_process(
    configuration: &ProviderInstanceConfig,
    settings: &CursorProviderSettings,
    working_directory: &Path,
) -> ChatResult<crate::process::ProviderProcessHandle> {
    use super::confinement::*;

    let access = AcpFolderAccess::for_launch(configuration.internal_mcp.as_ref())?;
    let helper = cursor_confinement(access, sandbox_helper())?;
    let environment = process_environment(configuration)?;
    let executable = resolve_executable(&configuration.executable, &environment)?;
    let arguments = launch_arguments(&configuration.launch_arguments, settings)?;
    let process = |executable, arguments, environment| {
        spawn_provider_process(ProviderProcessConfig {
            executable,
            arguments,
            working_directory: working_directory.to_path_buf(),
            environment,
            stderr_limit_bytes: CURSOR_STDERR_LIMIT_BYTES,
            purpose: crate::process::ProviderProcessPurpose::Session,
            provider_instance_label: Some(configuration.label.clone()),
        })
    };
    let (Some(helper), Some(access)) = (helper, access) else {
        return process(executable, arguments, environment);
    };
    ensure_confined_endpoint(settings.api_endpoint.as_deref())?;
    let workspace =
        fs::canonicalize(working_directory).map_err(|_| cursor_sandbox_unavailable_error())?;
    let launch = prepare_cursor_launch(
        &executable,
        &workspace,
        access,
        &environment,
        &configuration.environment,
        configuration.internal_mcp.as_ref(),
    )?;
    let mut confined_arguments = launch.leading_arguments;
    confined_arguments.extend(arguments);
    let arguments = crate::confinement::helper_arguments(
        helper,
        &launch.policy,
        &launch.program,
        &confined_arguments,
        "Cursor",
    )?;
    let mut handle = process(helper.path().to_path_buf(), arguments, launch.environment)?;
    handle.retain_directory(launch.directory);
    Ok(handle)
}

pub fn spawn_acp_connection_process(
    configuration: &ProviderInstanceConfig,
    settings: &CursorProviderSettings,
    flavor: super::driver::AcpProviderFlavor,
    working_directory: &Path,
) -> ChatResult<crate::process::ProviderProcessHandle> {
    match flavor {
        super::driver::AcpProviderFlavor::Cursor => {
            spawn_connection_process(configuration, settings, working_directory)
        }
        super::driver::AcpProviderFlavor::Grok => {
            spawn_grok_connection_process(configuration, working_directory)
        }
        super::driver::AcpProviderFlavor::Generic => {
            let environment = process_environment_for(configuration, flavor.display_name())?;
            let executable = resolve_executable(&configuration.executable, &environment)?;
            let arguments = generic_launch_arguments(&configuration.launch_arguments)?;
            spawn_provider_process(ProviderProcessConfig {
                executable,
                arguments,
                working_directory: working_directory.to_path_buf(),
                environment,
                stderr_limit_bytes: CURSOR_STDERR_LIMIT_BYTES,
                purpose: crate::process::ProviderProcessPurpose::Session,
                provider_instance_label: Some(configuration.label.clone()),
            })
        }
    }
}

/// Starts Grok over ACP, under the sandbox helper for an organizational launch when the device
/// has one; see [`super::confinement`].
fn spawn_grok_connection_process(
    configuration: &ProviderInstanceConfig,
    working_directory: &Path,
) -> ChatResult<crate::process::ProviderProcessHandle> {
    use super::confinement::*;

    let access = AcpFolderAccess::for_launch(configuration.internal_mcp.as_ref())?;
    let helper = grok_confinement(access, sandbox_helper())?;
    let mut environment = process_environment_for(configuration, "Grok")?;
    environment.insert("GROK_OAUTH2_REFERRER".to_string(), "ganbaru".to_string());
    let private_home = configuration
        .private_home
        .as_deref()
        .map(str::trim)
        .filter(|home| !home.is_empty());
    if let Some(home) = private_home {
        create_private_home(Path::new(home))?;
    }
    let executable = resolve_executable(&configuration.executable, &environment)
        .map_err(|_| grok_executable_missing_error())?;
    let process = |executable, arguments, environment| {
        spawn_provider_process(ProviderProcessConfig {
            executable,
            arguments,
            working_directory: working_directory.to_path_buf(),
            environment,
            stderr_limit_bytes: CURSOR_STDERR_LIMIT_BYTES,
            purpose: crate::process::ProviderProcessPurpose::Session,
            provider_instance_label: Some(configuration.label.clone()),
        })
    };
    let Some(helper) = helper else {
        let arguments = grok_launch_arguments(&configuration.launch_arguments)?;
        return process(executable, arguments, environment);
    };
    let home = grok_home(&environment)?;
    let workspace = fs::canonicalize(working_directory).map_err(|_| sandbox_unavailable_error())?;
    let launch = prepare_grok_launch(
        &GrokPolicyPaths {
            home: &home,
            executable: &executable,
            workspace: &workspace,
            private_home: private_home.is_some(),
        },
        configuration.internal_mcp.as_ref(),
    )?;
    let mut arguments = grok_launch_arguments(&configuration.launch_arguments)?;
    // Global switches precede the command, and this one follows the configured arguments so
    // none of them can turn web search back on.
    arguments.insert(
        arguments.len() - GROK_COMMAND_ARGUMENTS.len(),
        GROK_DISABLE_WEB_SEARCH.to_string(),
    );
    for (name, value) in GROK_CONFINED_ENVIRONMENT {
        environment.insert((*name).to_string(), (*value).to_string());
    }
    environment.insert(
        "TMPDIR".to_string(),
        launch.temporary.to_string_lossy().into_owned(),
    );
    let arguments = helper_arguments(helper, &launch.policy, &executable, &arguments)?;
    let mut handle = process(helper.path().to_path_buf(), arguments, environment)?;
    handle.retain_directory(launch.temporary);
    Ok(handle)
}

/// Passes the configured arguments of a generic ACP agent through unchanged.
///
/// The agent speaks ACP on standard input and output as launched, so Ganbaru adds no argument;
/// only arguments that cannot reach a process are refused.
pub fn generic_launch_arguments(configured: &[String]) -> ChatResult<Vec<String>> {
    if configured.iter().any(|argument| argument.contains('\0')) {
        return Err(ChatError::validation(
            "launchArguments",
            "ACP agent launch arguments cannot contain NUL characters",
        ));
    }
    Ok(configured.to_vec())
}

/// The Grok command that serves ACP on standard input and output, after any global switch.
const GROK_COMMAND_ARGUMENTS: [&str; 2] = ["agent", "stdio"];

pub fn grok_launch_arguments(configured: &[String]) -> ChatResult<Vec<String>> {
    if configured.iter().any(|argument| {
        let normalized = argument.trim().to_ascii_lowercase();
        argument.contains('\0')
            || matches!(normalized.as_str(), "agent" | "stdio" | "--api-key")
            || normalized.starts_with("--api-key=")
    }) {
        return Err(ChatError::validation(
            "launchArguments",
            "Grok launch arguments cannot override the ACP transport or credentials",
        ));
    }
    let mut arguments = configured.to_vec();
    arguments.extend(GROK_COMMAND_ARGUMENTS.map(str::to_string));
    Ok(arguments)
}

pub fn continuation_group(
    configuration: &ProviderInstanceConfig,
    settings: &CursorProviderSettings,
    account_identity: Option<&str>,
) -> ChatResult<ContinuationGroupId> {
    let mut digest = Sha256::new();
    digest.update(b"ganbaru-chat-cursor-account-v1\0");
    if let Some(home) = configuration.provider_home.as_deref() {
        digest.update(home.as_bytes());
    }
    digest.update(b"\0");
    digest.update(
        settings
            .api_endpoint
            .as_deref()
            .unwrap_or("default")
            .as_bytes(),
    );
    digest.update(b"\0");
    digest.update(account_identity.unwrap_or("default").as_bytes());
    // A confined launch keeps its sessions in a private configuration folder, so it never
    // continues an unconfined session or the reverse. Only confined groups carry the marker,
    // because the unconfined launch identity is unchanged by it.
    if super::confinement::cursor_launch_is_confined(
        configuration.internal_mcp.as_ref(),
        super::confinement::sandbox_helper(),
    ) {
        digest.update(CONFINED_CONTINUATION_MARKER);
    }
    ContinuationGroupId::new(format!("cursor-account-{:x}", digest.finalize()))
        .map_err(|_| protocol_error("continuation identity"))
}

pub fn acp_continuation_group(
    configuration: &ProviderInstanceConfig,
    settings: &CursorProviderSettings,
    flavor: super::driver::AcpProviderFlavor,
    account_identity: Option<&str>,
) -> ChatResult<ContinuationGroupId> {
    if flavor == super::driver::AcpProviderFlavor::Cursor {
        return continuation_group(configuration, settings, account_identity);
    }
    if flavor == super::driver::AcpProviderFlavor::Generic {
        return generic_continuation_group(configuration);
    }
    let mut digest = Sha256::new();
    digest.update(b"ganbaru-chat-grok-account-v1\0");
    if let Some(home) = grok_home_setting(configuration) {
        digest.update(home.as_bytes());
    }
    digest.update(b"\0");
    digest.update(account_identity.unwrap_or("default").as_bytes());
    ContinuationGroupId::new(format!("grok-account-{:x}", digest.finalize()))
        .map_err(|_| protocol_error("continuation identity"))
}

/// Names the native session store of a generic ACP agent.
///
/// A generic agent reports no account, so its sessions are grouped by the configured executable
/// and launch arguments: another agent, or the same one launched differently, cannot resume them.
fn generic_continuation_group(
    configuration: &ProviderInstanceConfig,
) -> ChatResult<ContinuationGroupId> {
    let mut digest = Sha256::new();
    digest.update(b"ganbaru-chat-acp-agent-v1\0");
    digest.update(configuration.executable.trim().as_bytes());
    for argument in &configuration.launch_arguments {
        digest.update(b"\0");
        digest.update(argument.as_bytes());
    }
    ContinuationGroupId::new(format!("acp-agent-{:x}", digest.finalize()))
        .map_err(|_| protocol_error("continuation identity"))
}

fn field<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim())
            .filter(|value| !value.is_empty())
    })
}

fn path_has_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

fn find_on_path(name: &str, environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = environment
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))?
        .1
        .as_str();
    crate::executable_search::path_search_directories(std::ffi::OsStr::new(path))
        .into_iter()
        .find_map(|directory| {
            executable_names(name)
                .into_iter()
                .map(|candidate| directory.join(candidate))
                .find(|candidate| crate::executable_search::is_safe_executable_candidate(candidate))
        })
}

fn executable_names(name: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        if Path::new(name).extension().is_some() {
            vec![name.to_string()]
        } else {
            vec![format!("{name}.exe"), name.to_string()]
        }
    }
    #[cfg(not(windows))]
    {
        vec![name.to_string()]
    }
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn inherited_environment_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[
            "PATH",
            "PATHEXT",
            "SYSTEMROOT",
            "WINDIR",
            "TEMP",
            "TMP",
            "USERPROFILE",
        ]
    }
    #[cfg(not(windows))]
    {
        &["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR"]
    }
}

fn executable_missing_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ExecutableMissing,
        "Cursor Agent executable is unavailable",
        true,
    )
}

fn grok_executable_missing_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ExecutableMissing,
        "Grok CLI executable is unavailable",
        true,
    )
}
