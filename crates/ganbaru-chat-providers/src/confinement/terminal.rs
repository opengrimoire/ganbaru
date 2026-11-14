//! The confined terminal: commands a teammate asks Ganbaru to run, under the sandbox helper.

use super::toolchains::{ToolchainGrants, resolve_existing, toolchain_grants};
use super::{
    CONFINED_TEMPORARY_PREFIX, DEVICE_WRITE, GitMetadataGuard, existing_paths,
    sandbox_policy_error, sandbox_unavailable_error,
};
use crate::models::*;
use ganbaru_sandbox::{NetworkAccess, SandboxHelper, SandboxPolicy};
use std::path::{Path, PathBuf};

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
pub(super) const CONFINED_ENV: &str = "/usr/bin/env";

/// The sandbox that confines the commands one teammate session asks Ganbaru to run: they write
/// the working folder and a private temporary directory, execute system programs, the folder,
/// the toolchains on `PATH`, and the toolchain homes such as Cargo's, and have no network.
pub(crate) struct TerminalSandbox {
    helper: &'static SandboxHelper,
    policy: SandboxPolicy,
    environment: Vec<(String, std::ffi::OsString)>,
    guard: GitMetadataGuard,
    /// Teammate provider named in refusals.
    provider: &'static str,
    /// Private temporary and home directory of the commands, removed with the session.
    _temporary: crate::probe_directory::ProbeDirectory,
}

impl TerminalSandbox {
    /// Prepares the policy, environment, and Git metadata guard of the commands one session of
    /// `provider` runs in `workspace`.
    pub(crate) fn prepare(
        helper: &'static SandboxHelper,
        workspace: &Path,
        provider: &'static str,
    ) -> ChatResult<Self> {
        let temporary =
            crate::probe_directory::ProbeDirectory::create_with_prefix(CONFINED_TEMPORARY_PREFIX)?;
        let ConfinedCommands {
            policy,
            environment,
        } = ConfinedCommands::prepare(workspace, &temporary, &[], provider)?;
        let guard = GitMetadataGuard::watch(workspace)?;
        Ok(Self {
            helper,
            policy,
            environment,
            guard,
            provider,
            _temporary: temporary,
        })
    }

    /// The Git metadata guard of the session's working folder.
    pub(crate) fn guard(&self) -> &GitMetadataGuard {
        &self.guard
    }

    /// A command that runs `program` with `arguments` under the terminal policy, with the
    /// confined environment in place of the app environment.
    ///
    /// The requested `environment` is applied by `env` inside the sandbox, never to the helper
    /// itself, so loader variables such as `LD_DEBUG_OUTPUT` cannot act before the policy holds.
    /// A program name with `=` is refused, because `env` would read it as a variable.
    pub(crate) fn command<'a>(
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
                    .map_err(|_| sandbox_unavailable_error(self.provider))?,
            )
            .env_clear()
            .envs(self.environment.iter().map(|(name, value)| (name, value)));
        Ok(command)
    }
}

/// The `env` arguments that set `environment` and then run `program` with `arguments`.
pub(crate) fn confined_env_arguments<'a>(
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
pub(crate) fn toolchain_directories(
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
pub(crate) fn terminal_policy(
    workspace: &Path,
    temporary: &Path,
    toolchains: &[PathBuf],
    homes: &ToolchainGrants,
    exists: impl Fn(&Path) -> bool,
    provider: &str,
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
        return Err(sandbox_policy_error(provider));
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

/// The policy and environment of the commands one teammate session runs in a working folder.
pub(crate) struct ConfinedCommands {
    pub policy: SandboxPolicy,
    /// The whole environment of a command, in place of the app environment.
    pub environment: Vec<(String, std::ffi::OsString)>,
}

impl ConfinedCommands {
    /// The terminal policy for `workspace`, with `temporary` as the private home and temporary
    /// directory, plus `read` for paths a provider's shell must also read.
    pub(crate) fn prepare(
        workspace: &Path,
        temporary: &Path,
        read: &[PathBuf],
        provider: &str,
    ) -> ChatResult<Self> {
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
        let mut policy = terminal_policy(
            workspace,
            temporary,
            &toolchains,
            &homes,
            |path| path.exists(),
            provider,
        )?;
        policy.read.extend(read.iter().cloned());
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
            policy,
            environment,
        })
    }
}
