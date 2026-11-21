//! Checks whether a newer provider CLI release exists, and runs the provider's own update
//! command when the person asks.
//!
//! Each family's newest release comes from the source its own updater reads: the Claude
//! release channel files, the npm registry for Codex and OpenCode, and Grok's own
//! `update --check`. Cursor updates itself, so it has no check. A release at or above the
//! family's compatibility ceiling is never offered, because Ganbaru would refuse it.

use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ProviderInstanceId, ProviderUpdateRead,
    ProviderUpdateRunRead,
};
use crate::process::{BoundedCommand, BoundedCommandFailure, run_bounded_command};
use crate::registry::{CoreVersion, ProviderCompatibilityRange, VersionOrder};
use crate::{ProviderCliCommand, ProviderDriver};
use reqwest::{Client, redirect::Policy};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::OnceLock;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Largest release document read; the npm manifests of these packages are a few kilobytes.
const MAX_RELEASE_DOCUMENT_BYTES: usize = 256 * 1024;
const GROK_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const GROK_CHECK_OUTPUT_BYTES: usize = 64 * 1024;
/// Installers download whole releases, so they get minutes rather than seconds.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const UPDATE_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// Lines of a failed update kept for the person to read.
const UPDATE_OUTPUT_TAIL_LINES: usize = 12;
const MAX_CLAUDE_SETTINGS_BYTES: u64 = 1024 * 1024;

/// Where a family publishes the version its updater would install.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReleaseSource {
    /// Claude's channel file, chosen by `autoUpdatesChannel` in the user settings.
    ClaudeChannel,
    /// The `latest` tag of an npm package.
    Npm(&'static str),
    /// The CLI's own `update --check --json`.
    GrokSelfCheck,
}

fn release_source(family_id: &str) -> Option<ReleaseSource> {
    match family_id {
        "claude" => Some(ReleaseSource::ClaudeChannel),
        "codex" => Some(ReleaseSource::Npm("@openai%2Fcodex")),
        "opencode" => Some(ReleaseSource::Npm("opencode-ai")),
        "grok" => Some(ReleaseSource::GrokSelfCheck),
        _ => None,
    }
}

fn compatibility(family_id: &str) -> Option<&'static ProviderCompatibilityRange> {
    match family_id {
        "claude" => Some(&crate::claude::CLAUDE_COMPATIBILITY),
        "codex" => Some(&crate::codex::CODEX_COMPATIBILITY),
        "opencode" => Some(&crate::opencode::OPENCODE_COMPATIBILITY),
        "grok" => Some(&crate::cursor::GROK_COMPATIBILITY),
        _ => None,
    }
}

/// Newest versions already read during one round of checks, so several instances of a
/// family share one request.
#[derive(Debug, Default)]
pub struct ReleaseLookups {
    versions: HashMap<String, Option<String>>,
}

/// One instance's update check, resolved from its driver before anything waits, so the
/// check holds no driver while it reads the network or runs the CLI.
#[derive(Debug)]
pub struct UpdateCheck {
    instance_id: ProviderInstanceId,
    family_id: String,
    current_version: String,
    update_command: String,
    range: &'static ProviderCompatibilityRange,
    lookup: ReleaseLookup,
    runs_in_app: bool,
}

#[derive(Debug)]
enum ReleaseLookup {
    Fetch { source: ReleaseSource, url: String },
    Cli(ProviderCliCommand),
}

/// The check for `driver`'s instance at `current_version`, or `None` when its family has no
/// check or no update command.
///
/// # Errors
///
/// Fails when the CLI cannot be resolved for a family that checks through its own program.
pub fn prepare_update_check(
    driver: &dyn ProviderDriver,
    current_version: &str,
) -> ChatResult<Option<UpdateCheck>> {
    let metadata = driver.metadata();
    let family_id = metadata.family_id.as_str();
    let (Some(source), Some(range), Some(update_command)) = (
        release_source(family_id),
        compatibility(family_id),
        metadata.update_command,
    ) else {
        return Ok(None);
    };
    let lookup = match source {
        ReleaseSource::GrokSelfCheck => {
            ReleaseLookup::Cli(driver.cli_command(&["update", "--check", "--json"])?)
        }
        ReleaseSource::ClaudeChannel | ReleaseSource::Npm(_) => ReleaseLookup::Fetch {
            source,
            url: release_url(source, driver),
        },
    };
    Ok(Some(UpdateCheck {
        instance_id: driver.instance_configuration().instance_id.clone(),
        family_id: family_id.to_string(),
        current_version: current_version.to_string(),
        update_command,
        range,
        lookup,
        runs_in_app: driver
            .cli_command(&[])
            .is_ok_and(|command| !package_managed(&command.executable)),
    }))
}

impl UpdateCheck {
    /// The instance this check is for.
    pub fn instance_id(&self) -> &ProviderInstanceId {
        &self.instance_id
    }

    /// The update the instance can take, or `None` when it is current or the newest release
    /// is outside the supported range.
    ///
    /// # Errors
    ///
    /// Fails when the release source cannot be read or the CLI check cannot run.
    pub async fn run(self, lookups: &mut ReleaseLookups) -> ChatResult<Option<ProviderUpdateRead>> {
        let latest = match self.lookup {
            ReleaseLookup::Cli(command) => {
                let output = run_cli(&command, GROK_CHECK_OUTPUT_BYTES, GROK_CHECK_TIMEOUT)
                    .await
                    .map_err(|failure| cli_failure("Grok update check", failure))?;
                parse_grok_check(&output.stdout)
            }
            ReleaseLookup::Fetch { source, url } => match lookups.versions.get(&url) {
                Some(version) => version.clone(),
                None => {
                    let version = fetch_release_version(source, &url).await?;
                    lookups.versions.insert(url, version.clone());
                    version
                }
            },
        };
        let Some(latest) =
            latest.filter(|latest| offers_update(self.range, &self.current_version, latest))
        else {
            return Ok(None);
        };
        Ok(Some(ProviderUpdateRead {
            instance_id: self.instance_id,
            current_version: self.current_version,
            latest_version: latest.clone(),
            update_command: pinned_update_command(&self.family_id, &self.update_command, &latest),
            runs_in_app: self.runs_in_app,
        }))
    }
}

/// The family's own update command for `driver`'s instance at `current_version`, resolved
/// with the provider's executable search and environment and pinned to `target_version` when
/// the CLI takes a version.
///
/// # Errors
///
/// Fails when `target_version` is not an update the check would offer, the family has no
/// update command, a package manager owns the install, or the CLI cannot be resolved.
pub fn update_command(
    driver: &dyn ProviderDriver,
    current_version: &str,
    target_version: &str,
) -> ChatResult<ProviderCliCommand> {
    let metadata = driver.metadata();
    if !takes_target(metadata.family_id.as_str(), current_version, target_version) {
        return Err(ChatError::validation(
            "targetVersion",
            "This release is not an update this provider can take",
        ));
    }
    let arguments = metadata
        .update_command
        .as_deref()
        .map(update_arguments)
        .filter(|arguments| !arguments.is_empty())
        .ok_or_else(|| ChatError::unsupported("This provider has no update command"))?;
    let arguments = pinned_update_arguments(metadata.family_id.as_str(), arguments, target_version);
    let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    let command = driver.cli_command(&arguments)?;
    if package_managed(&command.executable) {
        return Err(ChatError::unsupported(
            "A package manager owns this install, so update it there",
        ));
    }
    Ok(command)
}

/// Runs an update `command` from [`update_command`] with a null stdin.
///
/// # Errors
///
/// Fails when the CLI cannot start, times out, or writes too much. A command that runs and
/// fails is a result, not an error.
pub async fn run_update(command: ProviderCliCommand) -> ChatResult<ProviderUpdateRunRead> {
    let output = run_cli(&command, UPDATE_OUTPUT_BYTES, UPDATE_TIMEOUT)
        .await
        .map_err(|failure| cli_failure("Provider update", failure))?;
    Ok(ProviderUpdateRunRead {
        successful: output.successful,
        output: (!output.successful)
            .then(|| output_tail(&output.stdout, &output.stderr))
            .flatten(),
    })
}

/// The arguments after the program name of a documented update command such as
/// `opencode upgrade`.
fn update_arguments(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .skip(1)
        .map(str::to_string)
        .collect()
}

/// `arguments` with the version pin of a CLI that installs a named release, so an update
/// takes exactly the release the check offered and never a newer one past the ceiling.
/// Codex and Claude update commands take no version and follow their own channel.
fn pinned_update_arguments(
    family_id: &str,
    mut arguments: Vec<String>,
    target: &str,
) -> Vec<String> {
    match family_id {
        "opencode" => arguments.push(target.to_string()),
        "grok" => arguments.extend(["--version".to_string(), target.to_string()]),
        _ => {}
    }
    arguments
}

/// The documented `command` with the version pin for `target`, as a person would type it.
fn pinned_update_command(family_id: &str, command: &str, target: &str) -> String {
    let mut words = command
        .split_whitespace()
        .take(1)
        .map(str::to_string)
        .collect::<Vec<_>>();
    words.extend(pinned_update_arguments(
        family_id,
        update_arguments(command),
        target,
    ));
    words.join(" ")
}

/// Whether `target` is a plain version the check would offer `family_id` at `current`, so an
/// update request never installs an arbitrary or unsupported release.
fn takes_target(family_id: &str, current: &str, target: &str) -> bool {
    compatibility(family_id).is_some_and(|range| {
        readable_version(target).as_deref() == Some(target) && offers_update(range, current, target)
    })
}

/// Whether `latest` is newer than `current` and still below the family's ceiling.
fn offers_update(range: &ProviderCompatibilityRange, current: &str, latest: &str) -> bool {
    let (Ok(current), Ok(latest)) = (
        CoreVersion::parse_version(current),
        CoreVersion::parse_version(latest),
    ) else {
        return false;
    };
    let below_ceiling = range
        .ceiling_exclusive
        .map(CoreVersion::parse_version)
        .transpose()
        .ok()
        .flatten()
        .is_none_or(|ceiling| latest < ceiling);
    latest > current && below_ceiling
}

/// Whether a package manager or the operating system owns the install at `executable`, so
/// the CLI's own updater would fail or fight it.
fn package_managed(executable: &Path) -> bool {
    let components = executable
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_ascii_lowercase),
            _ => None,
        })
        .collect::<Vec<_>>();
    let has = |name: &str| components.iter().any(|component| component == name);
    let managed_folder = ["cellar", "caskroom", "homebrew", ".linuxbrew", "linuxbrew"]
        .into_iter()
        .chain(["mise", ".asdf", "winget", "chocolatey", "scoop"])
        .any(has);
    let system_root = !cfg!(windows)
        && matches!(
            components.first().map(String::as_str),
            Some("usr" | "opt" | "nix" | "snap")
        );
    managed_folder || system_root
}

fn release_url(source: ReleaseSource, driver: &dyn ProviderDriver) -> String {
    match source {
        ReleaseSource::ClaudeChannel => {
            let channel = crate::claude::resolve_claude_home(driver.instance_configuration())
                .ok()
                .and_then(|home| {
                    ganbaru_atomic_files::read_regular_file_capped(
                        &home.config_directory.join("settings.json"),
                        MAX_CLAUDE_SETTINGS_BYTES,
                        ganbaru_atomic_files::FinalSymlink::Follow,
                    )
                    .ok()
                })
                .map_or(ClaudeChannel::Latest, |bytes| claude_channel(&bytes));
            format!(
                "https://downloads.claude.ai/claude-code-releases/{}",
                channel.file_name()
            )
        }
        ReleaseSource::Npm(package) => format!("https://registry.npmjs.org/{package}/latest"),
        ReleaseSource::GrokSelfCheck => String::new(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClaudeChannel {
    Latest,
    Stable,
}

impl ClaudeChannel {
    fn file_name(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Stable => "stable",
        }
    }
}

/// The release channel a Claude settings document follows; Claude defaults to `latest`.
fn claude_channel(settings: &[u8]) -> ClaudeChannel {
    let channel = serde_json::from_slice::<Value>(settings)
        .ok()
        .and_then(|document| {
            document
                .get("autoUpdatesChannel")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    if channel.as_deref() == Some("stable") {
        ClaudeChannel::Stable
    } else {
        ClaudeChannel::Latest
    }
}

async fn fetch_release_version(source: ReleaseSource, url: &str) -> ChatResult<Option<String>> {
    let body = fetch_bounded(url).await?;
    Ok(match source {
        ReleaseSource::ClaudeChannel => parse_plain_version(&body),
        ReleaseSource::Npm(_) => parse_npm_version(&body),
        ReleaseSource::GrokSelfCheck => None,
    })
}

fn release_client() -> ChatResult<&'static Client> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .redirect(Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .user_agent("Ganbaru/0.1 provider update check")
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|_| unavailable("Provider update check client could not start"))
}

async fn fetch_bounded(url: &str) -> ChatResult<Vec<u8>> {
    let mut response = release_client()?
        .get(url)
        .send()
        .await
        .map_err(|_| unavailable("Provider release source could not be reached"))?;
    if !response.status().is_success() {
        return Err(unavailable("Provider release source refused the request"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| unavailable("Provider release source stopped responding"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RELEASE_DOCUMENT_BYTES {
            return Err(unavailable("Provider release document is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// A bare version file such as Claude's channel files.
fn parse_plain_version(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?.trim();
    readable_version(text)
}

/// The `version` of an npm `latest` manifest.
fn parse_npm_version(body: &[u8]) -> Option<String> {
    let document = serde_json::from_slice::<Value>(body).ok()?;
    readable_version(document.get("version")?.as_str()?)
}

/// The `latestVersion` of `grok update --check --json`.
fn parse_grok_check(body: &[u8]) -> Option<String> {
    let document = serde_json::from_slice::<Value>(body).ok()?;
    readable_version(document.get("latestVersion")?.as_str()?)
}

fn readable_version(text: &str) -> Option<String> {
    let text = text.trim();
    (text.len() <= 64
        && !text.contains(char::is_whitespace)
        && CoreVersion::parse_version(text).is_ok())
    .then(|| text.to_string())
}

async fn run_cli(
    command: &ProviderCliCommand,
    output_limit_bytes: usize,
    timeout: Duration,
) -> Result<crate::process::BoundedCommandOutput, BoundedCommandFailure> {
    let working_directory = crate::probe_directory::ProbeDirectory::create()
        .map_err(|_| BoundedCommandFailure::Unavailable)?;
    let arguments = command
        .arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    run_bounded_command(BoundedCommand {
        executable: &command.executable,
        arguments: &arguments,
        working_directory: &working_directory,
        environment: &command.environment,
        output_limit_bytes,
        timeout,
    })
    .await
}

fn cli_failure(action: &str, failure: BoundedCommandFailure) -> ChatError {
    match failure {
        BoundedCommandFailure::Unavailable => ChatError::new(
            ChatErrorCode::ExecutableMissing,
            format!("{action} could not start the provider CLI"),
            true,
        ),
        BoundedCommandFailure::TimedOut => {
            ChatError::new(ChatErrorCode::Timeout, format!("{action} timed out"), true)
        }
        BoundedCommandFailure::Oversized => ChatError::new(
            ChatErrorCode::Protocol,
            format!("{action} wrote more output than expected"),
            true,
        ),
    }
}

fn unavailable(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::TransportUnavailable, message, true)
}

/// The last lines of a command's output, stderr first since failures usually explain
/// themselves there.
fn output_tail(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let text = [stderr, stdout]
        .into_iter()
        .map(String::from_utf8_lossy)
        .find(|text| !text.trim().is_empty())?;
    let lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let start = lines.len().saturating_sub(UPDATE_OUTPUT_TAIL_LINES);
    Some(lines[start..].join("\n"))
}

#[cfg(test)]
mod tests;
