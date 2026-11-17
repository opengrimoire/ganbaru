//! Kernel confinement of Grok and Cursor agents and of the commands Grok asks Ganbaru to run.
//!
//! An organizational Grok launch runs the Grok process under the `ganbaru-sandbox` helper. The
//! process reads the system, its own Grok home, and the working folder, writes only its session
//! store and logs in a configured Grok home and a private temporary directory, executes only its
//! own binary, and connects only to TCP port 443 and the internal MCP endpoint. A launch with a
//! private Grok home, which Ganbaru owns and which holds no login, reads and writes that home
//! whole and reads the binary where it is installed. Its writes to the working folder go through
//! `fs/write_text_file`, and its commands through `terminal/create`, which Ganbaru confines again
//! with write access to the folder and no network. When the helper is missing or the kernel
//! cannot enforce a policy, folder and scratch launches are refused. Cursor launches are confined
//! as described in [`cursor_agent`].

use crate::confinement::{
    DEVICE_WRITE, GitMetadataGuard, PROVIDER_API_PORT, TerminalSandbox, existing_paths,
    resolver_configuration, scope_refusal,
};
use crate::models::*;
use ganbaru_sandbox::{NetworkAccess, SandboxHelper, SandboxPolicy};
use std::path::{Path, PathBuf};

pub(super) use crate::confinement::{CONFINED_TEMPORARY_PREFIX, sandbox_helper};

mod cursor_agent;
pub(super) use cursor_agent::MINIMUM_CONFINED_CURSOR_VERSION;
#[cfg(test)]
mod tests;

pub(super) use cursor_agent::{
    cursor_authority_support, cursor_confinement, cursor_launch_is_confined,
    ensure_confined_endpoint, prepare_cursor_launch,
    sandbox_unavailable_error as cursor_sandbox_unavailable_error,
};

/// Agent provider named in confinement refusals.
const PROVIDER: &str = "Grok";

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
/// Grok home folders a confined Grok process writes: its session store and its logs.
const GROK_HOME_WRITABLE_FOLDERS: &[&str] = &["sessions", "logs"];
/// Grok home files a confined Grok process writes when they already exist.
const GROK_HOME_WRITABLE_FILES: &[&str] = &["managed_config.lock"];
/// Grok global switch that removes its web search tool; it must precede the `agent` command.
pub(super) const GROK_DISABLE_WEB_SEARCH: &str = "--disable-web-search";
/// Grok environment that turns off fetching web pages and sending telemetry.
pub(super) const GROK_CONFINED_ENVIRONMENT: &[(&str, &str)] =
    &[("GROK_WEB_FETCH", "0"), ("GROK_TELEMETRY_ENABLED", "0")];

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

/// Boundaries a Grok agent holds, given whether the sandbox confines its process.
///
/// Without the sandbox only a conversation is held: Grok's subagents and workflows write
/// files directly, `agent stdio` ignores the tool deny, removal, and subagent switches, and a
/// trusted folder starts hooks outside the client callbacks. Under the sandbox those writes and
/// processes fail in the kernel, every command reaches `terminal/create`, and the network is
/// limited to the provider API and the internal MCP endpoint, so publication goes only through
/// the push request host tool, which a person confirms. The
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
        classified_publish: confined,
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
    if !paths.private_home {
        prepare_grok_home(paths.home)?;
    }
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
    /// Whether the home is a private one Ganbaru owns for an API key launch, which the process
    /// writes whole because no other program uses it.
    pub private_home: bool,
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
    // The executable lives outside a private home, and a self-contained build reads itself.
    if !paths.executable.starts_with(paths.home) {
        read.push(paths.executable.to_path_buf());
    }
    read.push(paths.workspace.to_path_buf());
    let mut write = if paths.private_home {
        vec![paths.home.to_path_buf()]
    } else {
        GROK_HOME_WRITABLE_FOLDERS
            .iter()
            .chain(GROK_HOME_WRITABLE_FILES)
            .map(|name| paths.home.join(name))
            .filter(|path| exists(path))
            .collect::<Vec<_>>()
    };
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
    crate::confinement::internal_mcp_port(url, PROVIDER)
}

/// Helper arguments as provider process arguments; a path that is not valid Unicode is refused.
pub(super) fn helper_arguments(
    helper: &SandboxHelper,
    policy: &SandboxPolicy,
    program: &Path,
    arguments: &[String],
) -> ChatResult<Vec<String>> {
    crate::confinement::helper_arguments(helper, policy, program, arguments, PROVIDER)
}

pub(super) fn sandbox_unavailable_error() -> ChatError {
    crate::confinement::sandbox_unavailable_error(PROVIDER)
}

fn sandbox_policy_error() -> ChatError {
    crate::confinement::sandbox_policy_error(PROVIDER)
}

fn organizational_permission_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Grok could not prove the required organizational authority boundary",
        false,
    )
}

/// What the client callbacks of one confined Grok or Cursor session may do.
pub(super) struct AcpCallbackConfinement {
    access: AcpFolderAccess,
    /// Confines terminal commands; present only when a Grok access runs commands.
    terminals: Option<TerminalSandbox>,
    /// Watches the folder metadata of a Cursor edit session, whose process writes the folder
    /// itself.
    writes: Option<GitMetadataGuard>,
}

impl AcpCallbackConfinement {
    /// The confinement of a session, or `None` when its callbacks keep the plain workspace
    /// boundary: generic agents, direct personal threads, and conversations whose process runs
    /// unconfined because the device has no enforcing helper.
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
        use super::driver::AcpProviderFlavor;

        let Some(access) = AcpFolderAccess::for_launch(internal_mcp)? else {
            return Ok(None);
        };
        let confining = match flavor {
            AcpProviderFlavor::Grok => grok_confinement(Some(access), helper)?,
            AcpProviderFlavor::Cursor => cursor_confinement(Some(access), helper)?,
            AcpProviderFlavor::Generic => None,
        };
        let Some(helper) = confining else {
            return Ok(None);
        };
        let terminals = if flavor == AcpProviderFlavor::Grok && access.runs_commands() {
            Some(TerminalSandbox::prepare(helper, workspace, PROVIDER)?)
        } else {
            None
        };
        let writes = if flavor == AcpProviderFlavor::Cursor && access.writes_files() {
            Some(GitMetadataGuard::watch(workspace)?)
        } else {
            None
        };
        Ok(Some(Self {
            access,
            terminals,
            writes,
        }))
    }

    /// Whether the session offers `terminal/create` to the agent.
    pub(super) fn offers_terminal(&self) -> bool {
        self.terminals.is_some()
    }

    /// Refuses a file write the folder access does not allow.
    pub(super) fn verify_write(&self) -> ChatResult<()> {
        if !self.access.writes_files() {
            return Err(scope_refusal(
                "acp_write_outside_scope: this agent may not change files in this folder",
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
                "acp_command_outside_scope: this agent may not run commands in this folder",
            ));
        }
        let sandbox = self
            .terminals
            .as_ref()
            .ok_or_else(sandbox_unavailable_error)?;
        sandbox.guard().verify()?;
        Ok(sandbox)
    }

    /// Signaled when a confined command changed Git metadata, so the session can end.
    pub(super) fn git_metadata_tripped(&self) -> Option<std::sync::Arc<tokio::sync::Notify>> {
        self.guard().map(GitMetadataGuard::tripped)
    }

    /// Whether the agent writes the folder itself, so its tool results are followed by a
    /// metadata check.
    pub(super) fn watches_direct_writes(&self) -> bool {
        self.writes.is_some()
    }

    fn guard(&self) -> Option<&GitMetadataGuard> {
        self.terminals
            .as_ref()
            .map(TerminalSandbox::guard)
            .or(self.writes.as_ref())
    }
}
