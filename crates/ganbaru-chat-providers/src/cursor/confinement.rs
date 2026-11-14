//! Kernel confinement of Grok teammates and of the commands they ask Ganbaru to run.
//!
//! An organizational Grok launch runs the Grok process under the `ganbaru-sandbox` helper. The
//! process reads the system, its own Grok home, and the working folder, writes only its session
//! store, logs, and a private temporary directory, executes only its own binary, and connects
//! only to TCP port 443 and the internal MCP endpoint. Its writes to the working folder go
//! through `fs/write_text_file`, and its commands through `terminal/create`, which Ganbaru
//! confines again with write access to the folder and no network. When the helper is missing or
//! the kernel cannot enforce a policy, folder and scratch launches are refused.

use crate::models::*;
use ganbaru_sandbox::{NetworkAccess, SandboxHelper, SandboxPolicy};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::git_guard::GitMetadataGuard;
use toolchains::{ToolchainGrants, resolve_existing, toolchain_grants};

#[cfg(test)]
mod tests;
mod toolchains;

/// System trees a confined Grok process reads: programs, libraries, configuration such as TLS
/// roots and name resolution, and the kernel views a runtime inspects.
const GROK_SYSTEM_READ: &[&str] = &[
    "/usr",
    "/etc",
    "/lib",
    "/lib64",
    "/proc",
    "/sys",
    "/dev/urandom",
];
/// Files every confined process may write without reaching user data.
const DEVICE_WRITE: &[&str] = &["/dev/null"];
/// Grok home folders a confined Grok process writes: its session store and its logs.
const GROK_HOME_WRITABLE_FOLDERS: &[&str] = &["sessions", "logs"];
/// Grok home files a confined Grok process writes when they already exist.
const GROK_HOME_WRITABLE_FILES: &[&str] = &["managed_config.lock"];
/// Port of the provider API, reached over TLS.
const PROVIDER_API_PORT: u16 = 443;
/// Grok global switch that removes its web search tool; it must precede the `agent` command.
pub(super) const GROK_DISABLE_WEB_SEARCH: &str = "--disable-web-search";
/// Grok environment that turns off fetching web pages and sending telemetry.
pub(super) const GROK_CONFINED_ENVIRONMENT: &[(&str, &str)] =
    &[("GROK_WEB_FETCH", "0"), ("GROK_TELEMETRY_ENABLED", "0")];
/// Prefix of the private temporary directory of one confined process.
pub(super) const CONFINED_TEMPORARY_PREFIX: &str = "ganbaru-confined-";

/// Folder access of one Grok launch, derived from its organizational scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AcpFolderAccess {
    /// A neutral directory with no project folder: reads only, no writes or commands.
    Conversation,
    /// Reads the working folder; no writes or commands.
    Read,
    /// Reads the working folder and writes it through `fs/write_text_file`.
    Edit,
    /// Edit access plus confined commands through `terminal/create`.
    Commands,
}

impl AcpFolderAccess {
    /// The access an organizational launch holds; `None` for a direct personal thread, which
    /// runs unconfined.
    pub(super) fn for_launch(
        internal_mcp: Option<&ProviderInternalMcpConfig>,
    ) -> ChatResult<Option<Self>> {
        match internal_mcp {
            Some(server) if server.organizational_authority => server
                .organizational_scope
                .map(|scope| Some(Self::for_scope(scope)))
                .ok_or_else(organizational_permission_error),
            _ => Ok(None),
        }
    }

    pub(super) fn for_scope(scope: ProviderOrganizationalScope) -> Self {
        match scope {
            ProviderOrganizationalScope::Conversation => Self::Conversation,
            ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Read,
            } => Self::Read,
            ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Edit,
            } => Self::Edit,
            ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Execute | ProviderFolderCapability::Publish,
            }
            | ProviderOrganizationalScope::Scratch => Self::Commands,
        }
    }

    /// Whether the launch may write the working folder through `fs/write_text_file`.
    pub(super) fn writes_files(self) -> bool {
        matches!(self, Self::Edit | Self::Commands)
    }

    /// Whether the launch may run commands through `terminal/create`.
    pub(super) fn runs_commands(self) -> bool {
        self == Self::Commands
    }

    /// Whether the launch may run without the sandbox: only a conversation, whose boundary
    /// does not depend on it.
    fn runs_unconfined(self) -> bool {
        self == Self::Conversation
    }
}

/// The sandbox helper of this installation, when it is present and the kernel enforces a
/// policy; checked once per app run.
///
/// Off Linux there is no helper, so this is always `None`.
pub(super) fn sandbox_helper() -> Option<&'static SandboxHelper> {
    static HELPER: OnceLock<Option<SandboxHelper>> = OnceLock::new();
    HELPER.get_or_init(locate_enforcing_helper).as_ref()
}

#[cfg(target_os = "linux")]
fn locate_enforcing_helper() -> Option<SandboxHelper> {
    let helper = SandboxHelper::locate().ok()?;
    ganbaru_sandbox::probe(&helper).ok()?;
    Some(helper)
}

#[cfg(not(target_os = "linux"))]
fn locate_enforcing_helper() -> Option<SandboxHelper> {
    None
}

/// Boundaries a Grok teammate holds, given whether the sandbox confines its process.
///
/// Without the sandbox only a conversation is held: Grok's subagents and workflows write
/// files directly, `agent stdio` ignores the tool deny, removal, and subagent switches, and a
/// trusted folder starts hooks outside the client callbacks. Under the sandbox those writes and
/// processes fail in the kernel, every command reaches `terminal/create`, and the network is
/// limited to the provider API and the internal MCP endpoint. Publishing stays unclaimed. The
/// command boundary is the sandbox, so person approval is not claimed as one, although every
/// safety mode but full access still holds each confined command until the person allows it.
pub(super) fn grok_authority_support(confined: bool) -> ProviderAuthoritySupport {
    ProviderAuthoritySupport {
        isolated_conversation: true,
        internal_host_tools: true,
        deny_shell: confined,
        read_only_root: confined,
        writable_root: confined,
        confined_commands: confined,
        network_boundary: confined,
        classified_publish: false,
        person_approved_commands: false,
    }
}

/// The helper a Grok launch with `access` runs under; `None` when it runs unconfined.
///
/// A direct personal thread runs unconfined, and so does a conversation when the device has no
/// enforcing helper. A folder or scratch launch without one is refused.
pub(super) fn grok_confinement(
    access: Option<AcpFolderAccess>,
    helper: Option<&'static SandboxHelper>,
) -> ChatResult<Option<&'static SandboxHelper>> {
    match (access, helper) {
        (None, _) => Ok(None),
        (Some(_), Some(helper)) => Ok(Some(helper)),
        (Some(access), None) if access.runs_unconfined() => Ok(None),
        (Some(_), None) => Err(sandbox_unavailable_error()),
    }
}

/// A Grok process ready to start under the helper.
pub(super) struct ConfinedGrokLaunch {
    pub policy: SandboxPolicy,
    /// Private temporary directory of the process, removed when the launch is dropped.
    pub temporary: crate::probe_directory::ProbeDirectory,
}

/// Prepares the policy and private temporary directory of one confined Grok process.
pub(super) fn prepare_grok_launch(
    paths: &GrokPolicyPaths<'_>,
    internal_mcp: Option<&ProviderInternalMcpConfig>,
) -> ChatResult<ConfinedGrokLaunch> {
    let internal_mcp_port = internal_mcp
        .map(|server| internal_mcp_port(&server.url))
        .transpose()?;
    prepare_grok_home(paths.home)?;
    let temporary =
        crate::probe_directory::ProbeDirectory::create_with_prefix(CONFINED_TEMPORARY_PREFIX)?;
    let policy = grok_process_policy(paths, &temporary, internal_mcp_port, |path| path.exists())?;
    Ok(ConfinedGrokLaunch { policy, temporary })
}

/// Paths a confined Grok process needs beyond the system.
pub(super) struct GrokPolicyPaths<'a> {
    /// Canonical Grok home: `GROK_HOME`, or `.grok` in the home directory.
    pub home: &'a Path,
    /// Canonical Grok executable.
    pub executable: &'a Path,
    /// Canonical working folder, or the neutral directory of a conversation.
    pub workspace: &'a Path,
}

/// The policy of one Grok process.
///
/// `exists` decides which optional system and Grok home paths are named, so tests can supply
/// a fixed file system view.
pub(super) fn grok_process_policy(
    paths: &GrokPolicyPaths<'_>,
    temporary: &Path,
    internal_mcp_port: Option<u16>,
    exists: impl Fn(&Path) -> bool,
) -> ChatResult<SandboxPolicy> {
    let mut read = existing_paths(GROK_SYSTEM_READ, &exists);
    read.extend(resolver_configuration().filter(|path| !path.starts_with("/etc") && exists(path)));
    read.push(paths.home.to_path_buf());
    read.push(paths.workspace.to_path_buf());
    let mut write = GROK_HOME_WRITABLE_FOLDERS
        .iter()
        .chain(GROK_HOME_WRITABLE_FILES)
        .map(|name| paths.home.join(name))
        .filter(|path| exists(path))
        .collect::<Vec<_>>();
    write.push(temporary.to_path_buf());
    write.extend(existing_paths(DEVICE_WRITE, &exists));
    // A Grok home around the folder would also expose what surrounds it, such as the vault.
    if paths.workspace.starts_with(paths.home)
        || write
            .iter()
            .any(|path| paths.workspace.starts_with(path) || path.starts_with(paths.workspace))
    {
        return Err(sandbox_policy_error());
    }
    let mut ports = vec![PROVIDER_API_PORT];
    ports.extend(internal_mcp_port.filter(|port| *port != PROVIDER_API_PORT));
    Ok(SandboxPolicy {
        read,
        execute: vec![paths.executable.to_path_buf()],
        write,
        network: NetworkAccess::TcpConnect(ports),
    })
}

/// Creates the Grok home folders a confined process writes, so the policy can name them.
fn prepare_grok_home(home: &Path) -> ChatResult<()> {
    for name in GROK_HOME_WRITABLE_FOLDERS {
        std::fs::create_dir_all(home.join(name)).map_err(|_| sandbox_policy_error())?;
    }
    Ok(())
}

/// The canonical Grok home of a launch environment.
///
/// The confined process reads the whole Grok home, so a home that is or contains the user's
/// home directory is refused.
pub(super) fn grok_home(
    environment: &std::collections::BTreeMap<String, String>,
) -> ChatResult<PathBuf> {
    let user_home = environment.get("HOME").map(PathBuf::from);
    let home = environment
        .get("GROK_HOME")
        .map(PathBuf::from)
        .or_else(|| user_home.as_ref().map(|home| home.join(".grok")))
        .ok_or_else(sandbox_policy_error)?;
    std::fs::create_dir_all(&home).map_err(|_| sandbox_policy_error())?;
    let home = std::fs::canonicalize(home).map_err(|_| sandbox_policy_error())?;
    let user_home = user_home.and_then(|user_home| std::fs::canonicalize(user_home).ok());
    if user_home.is_some_and(|user_home| user_home.starts_with(&home)) {
        return Err(sandbox_policy_error());
    }
    Ok(home)
}

/// The loopback port of the internal MCP endpoint.
pub(super) fn internal_mcp_port(url: &str) -> ChatResult<u16> {
    let parsed = reqwest::Url::parse(url).map_err(|_| sandbox_policy_error())?;
    let loopback = parsed.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    parsed
        .port_or_known_default()
        .filter(|port| loopback && *port != 0)
        .ok_or_else(sandbox_policy_error)
}

/// The file `/etc/resolv.conf` points to, which name resolution reads.
fn resolver_configuration() -> Option<PathBuf> {
    std::fs::canonicalize("/etc/resolv.conf").ok()
}

fn existing_paths(paths: &[&str], exists: &impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    paths
        .iter()
        .map(PathBuf::from)
        .filter(|path| exists(path))
        .collect()
}

/// Helper arguments as provider process arguments; a path that is not valid Unicode is refused.
pub(super) fn helper_arguments(
    helper: &SandboxHelper,
    policy: &SandboxPolicy,
    program: &Path,
    arguments: &[String],
) -> ChatResult<Vec<String>> {
    helper
        .arguments(policy, program, arguments)
        .map_err(|_| sandbox_unavailable_error())?
        .into_iter()
        .map(|argument| argument.into_string().map_err(|_| sandbox_policy_error()))
        .collect()
}

pub(super) fn sandbox_unavailable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Grok needs the Ganbaru sandbox to work in a folder, and it is unavailable on this device",
        false,
    )
}

fn sandbox_policy_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Ganbaru could not prepare the sandbox for this Grok session",
        false,
    )
}

fn organizational_permission_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Grok could not prove the required organizational authority boundary",
        false,
    )
}

/// System trees a confined terminal command reads.
const TERMINAL_SYSTEM_READ: &[&str] =
    &["/etc", "/proc", "/dev/urandom", "/dev/random", "/dev/zero"];
/// System trees a confined terminal command executes from, the dynamic loader among them.
const TERMINAL_SYSTEM_EXECUTE: &[&str] = &[
    "/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/opt",
];
/// App environment variables a confined terminal command keeps, apart from `PATH`.
const TERMINAL_KEPT_ENVIRONMENT: &[&str] = &["LANG", "LANGUAGE", "LC_ALL", "LC_CTYPE", "TERM"];
/// `PATH` of a confined terminal command when the app has none.
const TERMINAL_DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
/// The `env` program that applies requested variables inside the sandbox.
const CONFINED_ENV: &str = "/usr/bin/env";

/// What the client callbacks of one organizational Grok session may do.
pub(super) struct AcpCallbackConfinement {
    access: AcpFolderAccess,
    /// Confines terminal commands; present only when the access runs commands.
    terminals: Option<TerminalSandbox>,
}

impl AcpCallbackConfinement {
    /// The confinement of a session, or `None` when its callbacks keep the plain workspace
    /// boundary: every flavor but Grok, direct personal threads, and conversations whose Grok
    /// process runs unconfined because the device has no enforcing helper.
    pub(super) fn for_session(
        flavor: super::driver::AcpProviderFlavor,
        internal_mcp: Option<&ProviderInternalMcpConfig>,
        workspace: &Path,
    ) -> ChatResult<Option<Self>> {
        Self::with_helper(flavor, internal_mcp, workspace, sandbox_helper())
    }

    /// [`Self::for_session`] with an explicit helper in place of the one this device found.
    pub(super) fn with_helper(
        flavor: super::driver::AcpProviderFlavor,
        internal_mcp: Option<&ProviderInternalMcpConfig>,
        workspace: &Path,
        helper: Option<&'static SandboxHelper>,
    ) -> ChatResult<Option<Self>> {
        if flavor != super::driver::AcpProviderFlavor::Grok {
            return Ok(None);
        }
        let Some(access) = AcpFolderAccess::for_launch(internal_mcp)? else {
            return Ok(None);
        };
        let Some(helper) = grok_confinement(Some(access), helper)? else {
            return Ok(None);
        };
        let terminals = if access.runs_commands() {
            Some(TerminalSandbox::prepare(helper, workspace)?)
        } else {
            None
        };
        Ok(Some(Self { access, terminals }))
    }

    /// Whether the session offers `terminal/create` to the agent.
    pub(super) fn offers_terminal(&self) -> bool {
        self.terminals.is_some()
    }

    /// Refuses a file write the folder access does not allow.
    pub(super) fn verify_write(&self) -> ChatResult<()> {
        if !self.access.writes_files() {
            return Err(scope_refusal(
                "acp_write_outside_scope: this teammate may not change files in this folder",
            ));
        }
        self.guard().map_or(Ok(()), GitMetadataGuard::verify)
    }

    /// Checks the Git metadata after a write or command; a change trips the guard.
    pub(super) fn verify_git_metadata(&self) -> ChatResult<()> {
        self.guard().map_or(Ok(()), GitMetadataGuard::verify)
    }

    /// The sandbox for a terminal command, after checking the folder access and Git metadata.
    pub(super) fn terminal_sandbox(&self) -> ChatResult<&TerminalSandbox> {
        if !self.access.runs_commands() {
            return Err(scope_refusal(
                "acp_command_outside_scope: this teammate may not run commands in this folder",
            ));
        }
        let sandbox = self
            .terminals
            .as_ref()
            .ok_or_else(sandbox_unavailable_error)?;
        sandbox.guard.verify()?;
        Ok(sandbox)
    }

    /// Signaled when a confined command changed Git metadata, so the session can end.
    pub(super) fn git_metadata_tripped(&self) -> Option<std::sync::Arc<tokio::sync::Notify>> {
        self.guard().map(GitMetadataGuard::tripped)
    }

    fn guard(&self) -> Option<&GitMetadataGuard> {
        self.terminals.as_ref().map(|sandbox| &sandbox.guard)
    }
}

/// The sandbox that confines the commands one Grok session asks Ganbaru to run: they write the
/// working folder and a private temporary directory, execute system programs, the folder, the
/// toolchains on `PATH`, and the toolchain homes such as Cargo's, and have no network.
pub(super) struct TerminalSandbox {
    helper: &'static SandboxHelper,
    policy: SandboxPolicy,
    environment: Vec<(String, std::ffi::OsString)>,
    guard: GitMetadataGuard,
    /// Private temporary and home directory of the commands, removed with the session.
    _temporary: crate::probe_directory::ProbeDirectory,
}

impl TerminalSandbox {
    fn prepare(helper: &'static SandboxHelper, workspace: &Path) -> ChatResult<Self> {
        let temporary =
            crate::probe_directory::ProbeDirectory::create_with_prefix(CONFINED_TEMPORARY_PREFIX)?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .and_then(|home| std::fs::canonicalize(home).ok());
        let search_path = std::env::var_os("PATH");
        let toolchains = toolchain_directories(
            search_path
                .as_deref()
                .map(std::env::split_paths)
                .into_iter()
                .flatten(),
            home.as_deref(),
            workspace,
            |path| {
                std::fs::canonicalize(path)
                    .ok()
                    .filter(|path| path.is_dir())
            },
        );
        let homes = toolchain_grants(
            |name| std::env::var_os(name),
            home.as_deref(),
            workspace,
            resolve_existing,
        );
        let policy = terminal_policy(workspace, &temporary, &toolchains, &homes, |path| {
            path.exists()
        })?;
        let guard = GitMetadataGuard::watch(workspace)?;
        let mut environment = vec![
            (
                "PATH".to_string(),
                search_path.unwrap_or_else(|| TERMINAL_DEFAULT_PATH.into()),
            ),
            ("HOME".to_string(), temporary.as_os_str().to_owned()),
            ("TMPDIR".to_string(), temporary.as_os_str().to_owned()),
        ];
        environment.extend(
            TERMINAL_KEPT_ENVIRONMENT.iter().filter_map(|name| {
                std::env::var_os(name).map(|value| ((*name).to_string(), value))
            }),
        );
        environment.extend(homes.environment);
        Ok(Self {
            helper,
            policy,
            environment,
            guard,
            _temporary: temporary,
        })
    }

    /// A command that runs `program` with `arguments` under the terminal policy, with the
    /// confined environment in place of the app environment.
    ///
    /// The requested `environment` is applied by `env` inside the sandbox, never to the helper
    /// itself, so loader variables such as `LD_DEBUG_OUTPUT` cannot act before the policy holds.
    /// A program name with `=` is refused, because `env` would read it as a variable.
    pub(super) fn command<'a>(
        &self,
        program: &str,
        arguments: &[String],
        environment: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> ChatResult<tokio::process::Command> {
        let confined_arguments = confined_env_arguments(program, arguments, environment)?;
        let mut command = tokio::process::Command::new(self.helper.path());
        command
            .args(
                self.helper
                    .arguments(&self.policy, CONFINED_ENV, confined_arguments)
                    .map_err(|_| sandbox_unavailable_error())?,
            )
            .env_clear()
            .envs(self.environment.iter().map(|(name, value)| (name, value)));
        Ok(command)
    }
}

/// The `env` arguments that set `environment` and then run `program` with `arguments`.
pub(super) fn confined_env_arguments<'a>(
    program: &str,
    arguments: &[String],
    environment: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> ChatResult<Vec<String>> {
    if program.contains('=') {
        return Err(ChatError::validation(
            "command",
            "ACP terminal program names cannot contain =",
        ));
    }
    let mut confined = vec!["--".to_string()];
    for (name, value) in environment {
        // `env` splits each assignment at its first `=`, so a name holding one would set
        // another variable.
        if name.is_empty() || name.contains('=') {
            return Err(ChatError::validation(
                "env",
                "ACP terminal variable names cannot be empty or contain =",
            ));
        }
        confined.push(format!("{name}={value}"));
    }
    confined.push(program.to_string());
    confined.extend(arguments.iter().cloned());
    Ok(confined)
}

/// The toolchain folders on `PATH` a confined command may execute from.
///
/// Relative entries, folders that do not resolve, and folders that contain the home directory or
/// the working folder are left out, so a broad entry such as `/` or the home directory never
/// opens the user's files.
pub(super) fn toolchain_directories(
    entries: impl IntoIterator<Item = PathBuf>,
    home: Option<&Path>,
    workspace: &Path,
    resolve: impl Fn(&Path) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = Vec::new();
    for entry in entries {
        if !entry.is_absolute() {
            continue;
        }
        let Some(directory) = resolve(&entry) else {
            continue;
        };
        let opens_user_files = home.is_some_and(|home| home.starts_with(&directory))
            || workspace.starts_with(&directory);
        if !opens_user_files && !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    directories
}

/// The policy of one confined terminal command.
pub(super) fn terminal_policy(
    workspace: &Path,
    temporary: &Path,
    toolchains: &[PathBuf],
    homes: &ToolchainGrants,
    exists: impl Fn(&Path) -> bool,
) -> ChatResult<SandboxPolicy> {
    let mut execute = existing_paths(TERMINAL_SYSTEM_EXECUTE, &exists);
    execute.push(workspace.to_path_buf());
    for path in toolchains.iter().chain(&homes.execute) {
        if !execute.contains(path) {
            execute.push(path.clone());
        }
    }
    let mut write = vec![workspace.to_path_buf(), temporary.to_path_buf()];
    write.extend(existing_paths(DEVICE_WRITE, &exists));
    write.extend(homes.write.iter().cloned());
    if temporary.starts_with(workspace) || workspace.starts_with(temporary) {
        return Err(sandbox_policy_error());
    }
    let mut read = existing_paths(TERMINAL_SYSTEM_READ, &exists);
    read.extend(homes.read.iter().cloned());
    Ok(SandboxPolicy {
        read,
        execute,
        write,
        network: NetworkAccess::Denied,
    })
}

fn scope_refusal(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::Permission, message, true)
}
