//! Commands a provider runs through its own shell, confined by naming the helper as that shell.
//!
//! Claude and OpenCode run each command of a teammate as `SHELL -c COMMAND` or `PREFIX COMMAND`
//! in their own process. Naming the `ganbaru-sandbox` helper as the shell puts every such
//! command under the terminal policy: the helper reads its policy from a file Ganbaru writes in
//! a private session directory, outside every path the commands may write, and runs `bash -c
//! COMMAND` with the confined environment in place of the provider's own.

use super::terminal::{CONFINED_ENV, ConfinedCommands, confined_env_arguments};
use super::{
    CONFINED_TEMPORARY_PREFIX, GitMetadataGuard, sandbox_policy_error, sandbox_unavailable_error,
};
use crate::models::*;
use crate::probe_directory::ProbeDirectory;
use ganbaru_sandbox::{POLICY_FILE_VARIABLE, SandboxHelper};
use std::path::{Path, PathBuf};

/// Name of the policy file inside the session directory.
const POLICY_FILE_NAME: &str = "shell-policy";
/// Name of the commands' private home and temporary directory inside the session directory.
const TEMPORARY_NAME: &str = "tmp";
/// Shells the confined commands run with, in order of preference; provider command lines use
/// Bash syntax.
const SHELL_CANDIDATES: &[&str] = &["/usr/bin/bash", "/bin/bash"];

/// The confined shell of one provider session and the Git metadata guard of its folder.
pub(crate) struct ShellSandbox {
    /// Path of the helper, which holds no whitespace or shell syntax.
    shell: String,
    guard: GitMetadataGuard,
    /// Private session directory holding the policy file and the commands' temporary directory,
    /// removed when the sandbox is dropped.
    directory: ProbeDirectory,
}

impl ShellSandbox {
    /// Writes the policy of the commands one session of `provider` runs in `workspace`, which
    /// also read `read`, and starts watching the folder's Git metadata.
    pub(crate) fn prepare(
        helper: &'static SandboxHelper,
        workspace: &Path,
        read: &[PathBuf],
        provider: &'static str,
    ) -> ChatResult<Self> {
        let shell_path = helper
            .path()
            .to_str()
            .filter(|_| is_plain_command_path(helper.path()))
            .ok_or_else(|| sandbox_policy_error(provider))?
            .to_string();
        let directory = ProbeDirectory::create_with_prefix(CONFINED_TEMPORARY_PREFIX)?;
        let temporary = directory.join(TEMPORARY_NAME);
        std::fs::create_dir(&temporary).map_err(|_| sandbox_policy_error(provider))?;
        let commands = ConfinedCommands::prepare(workspace, &temporary, read, provider)?;
        let shell = SHELL_CANDIDATES
            .iter()
            .find(|shell| Path::new(shell).is_file())
            .ok_or_else(|| sandbox_policy_error(provider))?;
        let environment = commands
            .environment
            .iter()
            .map(|(name, value)| value.to_str().map(|value| (name.as_str(), value)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| sandbox_policy_error(provider))?;
        let mut arguments = vec!["-i".to_string()];
        arguments.extend(confined_env_arguments(
            shell,
            &["-c".to_string()],
            environment,
        )?);
        let content = helper
            .policy_file(&commands.policy, CONFINED_ENV, arguments)
            .map_err(|_| sandbox_unavailable_error(provider))?;
        write_private_file(&directory.join(POLICY_FILE_NAME), &content)
            .map_err(|_| sandbox_policy_error(provider))?;
        let guard = GitMetadataGuard::watch(workspace)?;
        Ok(Self {
            shell: shell_path,
            guard,
            directory,
        })
    }

    /// The program a provider runs as its shell: the helper. Its path holds no whitespace or
    /// shell syntax, so a provider may also place it at the front of a command line.
    pub(crate) fn shell(&self) -> &str {
        &self.shell
    }

    /// The private directory the commands use as home and temporary directory.
    pub(crate) fn temporary(&self) -> PathBuf {
        self.directory.join(TEMPORARY_NAME)
    }

    /// The variable the provider process sets so the helper finds the policy file.
    pub(crate) fn policy_variable(&self, provider: &str) -> ChatResult<(String, String)> {
        let path = self.directory.join(POLICY_FILE_NAME);
        let path = path
            .to_str()
            .ok_or_else(|| sandbox_policy_error(provider))?;
        Ok((POLICY_FILE_VARIABLE.to_string(), path.to_string()))
    }

    /// The Git metadata guard of the session's working folder.
    pub(crate) fn guard(&self) -> &GitMetadataGuard {
        &self.guard
    }
}

/// Whether `path` is absolute and made only of characters no shell treats as syntax.
pub(crate) fn is_plain_command_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|text| {
            text.chars()
                .all(|character| character.is_ascii_alphanumeric() || "/._-+".contains(character))
        })
}

/// Creates `path` readable and writable only by its owner and writes `content` to it.
fn write_private_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(content)
}
