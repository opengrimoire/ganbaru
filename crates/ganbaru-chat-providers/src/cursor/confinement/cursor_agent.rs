//! Kernel confinement of Cursor teammates.
//!
//! An organizational Cursor launch on a device with an enforcing helper starts Cursor's bundled
//! Node runtime directly under the `ganbaru-sandbox` helper, in place of the `cursor-agent`
//! launcher script. The process reads the system, the Cursor installation, the Cursor sign-in
//! folder, and the working folder; it writes the working folder only with edit access, plus a
//! private home, data, and temporary directory per launch and a configuration folder per
//! working folder that Ganbaru reseeds with a shell deny on every launch. It executes only the
//! bundled runtime, the bundled search tool, and the dynamic loader, so every shell, hook, and
//! MCP server the agent would start fails in the kernel. It connects only to TCP port 443 and
//! the internal MCP endpoint.
//!
//! Cursor writes files itself rather than through `fs/write_text_file`, so an edit session
//! watches the folder's Git and Cursor configuration with the metadata guard.

use super::AcpFolderAccess;
use crate::confinement::{
    DEVICE_WRITE, PROVIDER_API_PORT, existing_paths, resolver_configuration, scope_refusal,
};
use crate::cursor::executable::{CursorVersion, parse_version};
use crate::models::*;
use ganbaru_sandbox::{NetworkAccess, SandboxHelper, SandboxPolicy};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// Teammate provider named in confinement refusals.
const PROVIDER: &str = "Cursor";
/// Oldest Cursor build whose confined launch was verified: its runtime layout, environment,
/// and configuration folder match what this module grants.
pub(in crate::cursor) const MINIMUM_CONFINED_CURSOR_VERSION: CursorVersion =
    CursorVersion { date: 20261111 };
/// File name of the launcher script inside a Cursor version folder.
const CURSOR_LAUNCHER: &str = "cursor-agent";
/// Bundled Node runtime inside a Cursor version folder.
const CURSOR_RUNTIME: &str = "node";
/// Script the runtime starts inside a Cursor version folder.
const CURSOR_ENTRY: &str = "index.js";
/// Bundled search tool inside a Cursor version folder.
const CURSOR_SEARCH: &str = "rg";
/// Folder Cursor marks its running installations in, inside a Cursor version folder.
const CURSOR_RUNNING_MARKER: &str = ".running";
/// Runtime switch the launcher passes so Node trusts the system certificate store.
const CURSOR_SYSTEM_CA_SWITCH: &str = "--use-system-ca";
/// Launcher variable and value under which the launcher leaves out the system store switch.
const CURSOR_FILE_CREDENTIAL_STORE: (&str, &str) = ("AGENT_CLI_CREDENTIAL_STORE", "file");
/// System trees a confined Cursor process reads.
const CURSOR_SYSTEM_READ: &[&str] = &[
    "/usr",
    "/etc",
    "/lib",
    "/lib64",
    "/proc",
    "/sys",
    "/dev/urandom",
];
/// Dynamic loaders the bundled runtime may name; the existing ones are executable.
const LOADER_CANDIDATES: &[&str] = &[
    "/lib64/ld-linux-x86-64.so.2",
    "/lib/ld-linux-aarch64.so.1",
    "/lib/ld-linux-armhf.so.3",
];
/// Longest Cursor data folder path; Cursor places local sockets in it.
const MAX_DATA_FOLDER_BYTES: usize = 83;
/// Folder under the state directory that holds one configuration folder per working folder.
const TEAMMATE_STATE_FOLDER: &str = "ganbaru/cursor-teammates";
/// Cursor CLI configuration Ganbaru writes on every confined launch: shell commands are denied
/// without asking, and Cursor's own sandbox is off because the helper confines the process.
const CURSOR_CLI_CONFIGURATION: &str = r#"{"version":1,"permissions":{"allow":[],"deny":["Shell(*)"]},"approvalMode":"allowlist","sandbox":{"mode":"disabled"}}"#;
/// Name of the Cursor CLI configuration file inside the configuration folder.
const CURSOR_CLI_CONFIGURATION_NAME: &str = "cli-config.json";
/// Command-bearing entries removed from the configuration folder before each launch.
const REMOVED_CONFIGURATION_ENTRIES: &[&str] = &["cli.json", "hooks.json", "mcp.json"];
/// Variables Ganbaru sets for a confined launch, which the configured environment cannot name.
const RESERVED_ENVIRONMENT: &[&str] = &[
    "CURSOR_CONFIG_DIR",
    "CURSOR_DATA_DIR",
    "CURSOR_INVOKED_AS",
    "XDG_CONFIG_HOME",
    "TMPDIR",
];

/// Boundaries a Cursor teammate holds, given whether the sandbox confines its process.
///
/// Without the sandbox only a conversation is held: Cursor writes files itself without a
/// permission request or a client file callback, and a project `.cursor/cli.json` allowlist runs
/// the shell unasked. Under the sandbox writes outside the granted folders and every program
/// but Cursor's own fail in the kernel, and the network is limited to the provider API and the
/// internal MCP endpoint. Cursor runs no confined commands, so commands and publishing stay
/// unclaimed.
pub(in crate::cursor) fn cursor_authority_support(confined: bool) -> ProviderAuthoritySupport {
    ProviderAuthoritySupport {
        isolated_conversation: true,
        internal_host_tools: true,
        deny_shell: confined,
        read_only_root: confined,
        writable_root: confined,
        confined_commands: false,
        network_boundary: confined,
        classified_publish: false,
        person_approved_commands: false,
    }
}

/// The helper a Cursor launch with `access` runs under; `None` when it runs unconfined.
///
/// A direct personal thread runs unconfined, and so does a conversation when the device has no
/// enforcing helper. A read or edit launch without one is refused, and a launch that would run
/// commands is always refused.
pub(in crate::cursor) fn cursor_confinement(
    access: Option<AcpFolderAccess>,
    helper: Option<&'static SandboxHelper>,
) -> ChatResult<Option<&'static SandboxHelper>> {
    match (access, helper) {
        (None, _) => Ok(None),
        (Some(AcpFolderAccess::Commands), _) => Err(scope_refusal(
            "acp_command_outside_scope: Cursor teammates may not run commands in this folder",
        )),
        (Some(_), Some(helper)) => Ok(Some(helper)),
        (Some(AcpFolderAccess::Conversation), None) => Ok(None),
        (Some(_), None) => Err(sandbox_unavailable_error()),
    }
}

/// The files of one Cursor installation, found from its launcher script.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::cursor) struct CursorInstallation {
    /// The version folder.
    pub directory: PathBuf,
    pub runtime: PathBuf,
    pub entry: PathBuf,
    /// The bundled search tool, when the installation has one.
    pub search: Option<PathBuf>,
}

/// The installation whose launcher is the canonical `executable`.
///
/// The launcher must be `cursor-agent` in a version folder named for a build no older than
/// [`MINIMUM_CONFINED_CURSOR_VERSION`] that holds the bundled runtime and entry script; any
/// other layout is refused, because the policy names those files.
pub(in crate::cursor) fn cursor_installation(
    executable: &Path,
    is_file: impl Fn(&Path) -> bool,
) -> ChatResult<CursorInstallation> {
    let directory = executable
        .parent()
        .filter(|_| {
            executable
                .file_name()
                .is_some_and(|name| name == CURSOR_LAUNCHER)
        })
        .ok_or_else(installation_error)?;
    let version = directory
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| parse_version(name).ok())
        .ok_or_else(installation_error)?;
    if version < MINIMUM_CONFINED_CURSOR_VERSION {
        return Err(ChatError::new(
            ChatErrorCode::UnsupportedVersion,
            format!(
                "Cursor Agent {version} cannot run in the Ganbaru sandbox; version \
                 {MINIMUM_CONFINED_CURSOR_VERSION} or newer is required to work in a folder"
            ),
            false,
        ));
    }
    let runtime = directory.join(CURSOR_RUNTIME);
    let entry = directory.join(CURSOR_ENTRY);
    if !is_file(&runtime) || !is_file(&entry) {
        return Err(installation_error());
    }
    let search = Some(directory.join(CURSOR_SEARCH)).filter(|path| is_file(path));
    Ok(CursorInstallation {
        directory: directory.to_path_buf(),
        runtime,
        entry,
        search,
    })
}

/// Refuses a custom Cursor endpoint the network boundary would not reach.
pub(in crate::cursor) fn ensure_confined_endpoint(endpoint: Option<&str>) -> ChatResult<()> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    let reachable = reqwest::Url::parse(endpoint).is_ok_and(|url| {
        url.scheme() == "https" && url.port_or_known_default() == Some(PROVIDER_API_PORT)
    });
    if reachable {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Permission,
            "A Cursor teammate in a folder can only use an HTTPS endpoint on port 443",
            false,
        ))
    }
}

/// Paths a confined Cursor process needs beyond the system.
pub(in crate::cursor) struct CursorPolicyPaths<'a> {
    pub installation: &'a CursorInstallation,
    /// Canonical working folder, or the neutral directory of a conversation.
    pub workspace: &'a Path,
    /// Whether the process writes the working folder.
    pub writes_workspace: bool,
    /// Canonical Cursor sign-in folder, when it exists.
    pub credentials: Option<&'a Path>,
    /// Canonical configuration folder of this working folder.
    pub configuration: &'a Path,
    /// Private home, data, and temporary directories of this launch.
    pub home: &'a Path,
    pub data: &'a Path,
    pub temporary: &'a Path,
    /// Canonical dynamic loaders.
    pub loaders: &'a [PathBuf],
}

/// The policy of one Cursor process.
///
/// `exists` decides which optional system paths are named, so tests can supply a fixed file
/// system view. A write path that holds or lies inside another granted folder is refused,
/// because it would widen the folder or let the process replace a program it executes.
pub(in crate::cursor) fn cursor_process_policy(
    paths: &CursorPolicyPaths<'_>,
    internal_mcp_port: Option<u16>,
    exists: impl Fn(&Path) -> bool,
) -> ChatResult<SandboxPolicy> {
    let installation = paths.installation;
    let mut read = existing_paths(CURSOR_SYSTEM_READ, &exists);
    read.extend(resolver_configuration().filter(|path| !path.starts_with("/etc") && exists(path)));
    read.extend(paths.credentials.map(Path::to_path_buf));
    read.push(installation.directory.clone());
    let mut execute = vec![installation.runtime.clone()];
    execute.extend(installation.search.iter().cloned());
    execute.extend(paths.loaders.iter().cloned());
    let running = installation.directory.join(CURSOR_RUNNING_MARKER);
    let private = [paths.configuration, paths.home, paths.data, paths.temporary];
    let overlaps = |a: &Path, b: &Path| a.starts_with(b) || b.starts_with(a);
    // No write reaches a program the process executes, its entry script, or the sign-in, and
    // the working folder stays apart from every other granted folder.
    let mut protected = execute.clone();
    protected.push(installation.entry.clone());
    protected.extend(paths.credentials.map(Path::to_path_buf));
    let workspace_apart = !private
        .iter()
        .chain([&installation.directory.as_path()])
        .chain(paths.credentials.as_ref())
        .any(|path| overlaps(path, paths.workspace));
    let writes_apart = !private
        .iter()
        .copied()
        .chain([running.as_path()])
        .any(|path| protected.iter().any(|protected| overlaps(path, protected)));
    if !workspace_apart || !writes_apart {
        return Err(sandbox_policy_error());
    }
    let mut write = private
        .iter()
        .map(|path| path.to_path_buf())
        .collect::<Vec<_>>();
    write.push(running);
    write.extend(existing_paths(DEVICE_WRITE, &exists));
    if paths.writes_workspace {
        write.push(paths.workspace.to_path_buf());
    } else {
        read.push(paths.workspace.to_path_buf());
    }
    let mut ports = vec![PROVIDER_API_PORT];
    ports.extend(internal_mcp_port.filter(|port| *port != PROVIDER_API_PORT));
    Ok(SandboxPolicy {
        read,
        execute,
        write,
        network: NetworkAccess::TcpConnect(ports),
    })
}

/// A Cursor process ready to start under the helper.
pub(in crate::cursor) struct ConfinedCursorLaunch {
    pub policy: SandboxPolicy,
    /// The bundled runtime and its arguments, before the configured launch arguments.
    pub program: PathBuf,
    pub leading_arguments: Vec<String>,
    /// Variables set in place of the app's, over the configured environment.
    pub environment: BTreeMap<String, String>,
    /// Private home, data, and temporary directory of the process, removed when dropped.
    pub directory: crate::probe_directory::ProbeDirectory,
}

/// Prepares the policy, environment, and private folders of one confined Cursor process.
///
/// `environment` is the launch environment before confinement, `configured` the variables the
/// provider configuration adds to it, and `workspace` is canonical.
pub(in crate::cursor) fn prepare_cursor_launch(
    executable: &Path,
    workspace: &Path,
    access: AcpFolderAccess,
    environment: &BTreeMap<String, String>,
    configured: &BTreeMap<String, String>,
    internal_mcp: Option<&ProviderInternalMcpConfig>,
) -> ChatResult<ConfinedCursorLaunch> {
    if let Some(name) = RESERVED_ENVIRONMENT
        .iter()
        .find(|name| configured.contains_key(**name))
    {
        return Err(ChatError::validation(
            "environment",
            format!("A Cursor teammate in a folder cannot set {name}"),
        ));
    }
    let installation = cursor_installation(executable, Path::is_file)?;
    let internal_mcp_port = internal_mcp
        .map(|server| crate::confinement::internal_mcp_port(&server.url, PROVIDER))
        .transpose()?;
    let user_home = environment
        .get("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or_else(sandbox_policy_error)?;
    let user_folder = |variable: &str, fallback: &str| {
        environment
            .get(variable)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| user_home.join(fallback))
    };
    let configuration_home = user_folder("XDG_CONFIG_HOME", ".config");
    let credentials = std::fs::canonicalize(configuration_home.join("cursor"))
        .ok()
        .filter(|path| path.is_dir());
    let configuration =
        prepare_configuration_folder(&user_folder("XDG_STATE_HOME", ".local/state"), workspace)?;
    prepare_running_marker(&installation.directory)?;
    let directory = crate::probe_directory::ProbeDirectory::create_with_prefix(
        super::CONFINED_TEMPORARY_PREFIX,
    )?;
    let [home, data, temporary] = ["home", "data", "tmp"].map(|name| directory.join(name));
    for path in [&home, &data, &temporary] {
        std::fs::create_dir(path).map_err(|_| sandbox_policy_error())?;
    }
    if data.as_os_str().len() > MAX_DATA_FOLDER_BYTES {
        return Err(sandbox_policy_error());
    }
    let loaders = canonical_loaders();
    let policy = cursor_process_policy(
        &CursorPolicyPaths {
            installation: &installation,
            workspace,
            writes_workspace: access.writes_files(),
            credentials: credentials.as_deref(),
            configuration: &configuration,
            home: &home,
            data: &data,
            temporary: &temporary,
            loaders: &loaders,
        },
        internal_mcp_port,
        |path| path.exists(),
    )?;
    let text = |path: &Path| {
        path.to_str()
            .map(str::to_string)
            .ok_or_else(sandbox_policy_error)
    };
    let mut leading_arguments = Vec::new();
    let file_store = environment
        .get(CURSOR_FILE_CREDENTIAL_STORE.0)
        .map(String::as_str)
        == Some(CURSOR_FILE_CREDENTIAL_STORE.1);
    if !file_store {
        leading_arguments.push(CURSOR_SYSTEM_CA_SWITCH.to_string());
    }
    leading_arguments.push(text(&installation.entry)?);
    let mut confined_environment = environment.clone();
    for (name, value) in [
        ("HOME", text(&home)?),
        ("TMPDIR", text(&temporary)?),
        ("CURSOR_CONFIG_DIR", text(&configuration)?),
        ("CURSOR_DATA_DIR", text(&data)?),
        ("CURSOR_INVOKED_AS", CURSOR_LAUNCHER.to_string()),
        ("XDG_CONFIG_HOME", text(&configuration_home)?),
    ] {
        confined_environment.insert(name.to_string(), value);
    }
    Ok(ConfinedCursorLaunch {
        policy,
        program: installation.runtime,
        leading_arguments,
        environment: confined_environment,
        directory,
    })
}

/// The configuration folder of `workspace`, reseeded for a launch.
///
/// It lives in the user's state directory, keyed by the working folder, so Cursor's session
/// store survives between launches. Command-bearing entries a confined process left there are
/// removed and the CLI configuration is written again, never through a link.
fn prepare_configuration_folder(state_home: &Path, workspace: &Path) -> ChatResult<PathBuf> {
    let digest = Sha256::digest(workspace.as_os_str().as_encoded_bytes());
    let key = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let folder = state_home
        .join(TEAMMATE_STATE_FOLDER)
        .join(key)
        .join("config");
    create_private_folder(&folder).map_err(|_| sandbox_policy_error())?;
    let folder = std::fs::canonicalize(folder).map_err(|_| sandbox_policy_error())?;
    for name in REMOVED_CONFIGURATION_ENTRIES
        .iter()
        .chain(&[CURSOR_CLI_CONFIGURATION_NAME])
    {
        remove_entry(&folder.join(name)).map_err(|_| sandbox_policy_error())?;
    }
    write_new_file(
        &folder.join(CURSOR_CLI_CONFIGURATION_NAME),
        CURSOR_CLI_CONFIGURATION.as_bytes(),
    )
    .map_err(|_| sandbox_policy_error())?;
    Ok(folder)
}

/// Creates `<installation>/.running` when it is missing; a link or file there is refused,
/// because the policy grants writes to it.
fn prepare_running_marker(installation: &Path) -> ChatResult<()> {
    let marker = installation.join(CURSOR_RUNNING_MARKER);
    if std::fs::symlink_metadata(&marker).is_err() {
        std::fs::create_dir(&marker).map_err(|_| sandbox_policy_error())?;
    }
    match std::fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        _ => Err(sandbox_policy_error()),
    }
}

/// The dynamic loaders on this device, resolved and without duplicates.
fn canonical_loaders() -> Vec<PathBuf> {
    let mut loaders = Vec::new();
    for candidate in LOADER_CANDIDATES {
        if let Ok(loader) = std::fs::canonicalize(candidate) {
            if !loaders.contains(&loader) {
                loaders.push(loader);
            }
        }
    }
    loaders
}

#[cfg(unix)]
fn create_private_folder(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn create_private_folder(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Removes the entry at `path` without following a link; a missing entry is fine.
fn remove_entry(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

/// Creates `path`, which must not exist, readable and writable only by its owner.
fn write_new_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(content)
}

fn installation_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "Ganbaru does not recognize this Cursor Agent installation, so it cannot run it in the \
         sandbox; configure the cursor-agent launcher of a standard installation",
        false,
    )
}

pub(in crate::cursor) fn sandbox_unavailable_error() -> ChatError {
    crate::confinement::sandbox_unavailable_error(PROVIDER)
}

fn sandbox_policy_error() -> ChatError {
    crate::confinement::sandbox_policy_error(PROVIDER)
}

/// Whether a Cursor launch with `internal_mcp` runs under the helper on this device.
pub(in crate::cursor) fn cursor_launch_is_confined(
    internal_mcp: Option<&ProviderInternalMcpConfig>,
    helper: Option<&'static SandboxHelper>,
) -> bool {
    AcpFolderAccess::for_launch(internal_mcp)
        .and_then(|access| cursor_confinement(access, helper))
        .is_ok_and(|helper| helper.is_some())
}
