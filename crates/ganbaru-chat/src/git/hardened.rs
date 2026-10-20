//! Hardened Git invocation shared by every Git process Ganbaru starts.
//!
//! Repository content must not choose programs that Ganbaru runs. System and
//! global configuration belong to the user and stay in effect, so internal
//! results match the user's terminal. Command-bearing keys whose value comes
//! from local or worktree configuration (including files those include) are
//! replaced by the user's own value from another scope, or by a neutral value.

use crate::models::{ChatError, ChatErrorCode};
use ganbaru_chat_providers::process::{ProcessTree, configure_std_process_tree};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod digest;

pub use digest::{
    FolderConfigDigest, FolderConfiguration, MAX_TRUST_DIGEST_FILE_BYTES,
    MAX_TRUST_DIGEST_HOOK_FILES, MAX_TRUST_DIGEST_PROVIDER_FILES, ProviderConfigurationEntries,
    folder_configuration, folder_configuration_blocking, provider_configuration_entries,
    provider_configuration_entries_blocking,
};

/// Deadline for interactive Git reads such as status, listings, and review.
pub const GIT_READ_DEADLINE: Duration = Duration::from_secs(60);
/// Deadline for Git operations that reach a remote.
pub const GIT_NETWORK_DEADLINE: Duration = Duration::from_secs(180);
/// Deadline for checkpoint capture and restore, which hash or write the
/// whole working tree of possibly large repositories.
pub const GIT_CHECKPOINT_DEADLINE: Duration = Duration::from_secs(10 * 60);
/// Deadline for reading the effective configuration before an internal run.
pub const GIT_CONFIG_READ_DEADLINE: Duration = Duration::from_secs(20);
/// Upper bound on the configuration listing read before an internal run.
const MAX_CONFIG_LISTING_BYTES: usize = 4 * 1024 * 1024;
/// Upper bound on captured standard error.
const MAX_STDERR_BYTES: usize = 1024 * 1024;
/// Subcommands that reject literal pathspecs. Their inputs are paths, not
/// pathspecs, so no pattern magic applies.
const PATHSPEC_LITERAL_UNSUPPORTED: &[&str] = &["check-ignore"];
const POLL_INTERVAL_START: Duration = Duration::from_millis(2);
const POLL_INTERVAL_MAX: Duration = Duration::from_millis(50);

/// Environment variables that redirect Git to other repositories, inject
/// configuration, or choose programs. Internal runs remove them.
const INTERNAL_CLEARED_ENVIRONMENT: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_EXEC_PATH",
    "GIT_EXTERNAL_DIFF",
    "GIT_PAGER",
    "GIT_EDITOR",
    "GIT_SEQUENCE_EDITOR",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_ASKPASS",
    "GIT_PROXY_COMMAND",
    "GIT_TEMPLATE_DIR",
    "GIT_TRACE",
    "GIT_TRACE2",
    "GIT_TRACE2_EVENT",
    "GIT_TRACE2_PERF",
];

/// Driver commands that `git lfs install --local` writes. They name a program
/// resolved from `PATH`, not repository content, so they stay in effect.
const STANDARD_LFS_FILTER_VALUES: &[(&str, &str)] = &[
    ("clean", "git-lfs clean -- %f"),
    ("smudge", "git-lfs smudge -- %f"),
    ("smudge", "git-lfs smudge --skip -- %f"),
    ("process", "git-lfs filter-process"),
    ("process", "git-lfs filter-process --skip"),
];

/// Who started a Git run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitOrigin {
    /// Ganbaru on its own (status, listings, checkpoints, review). Hooks,
    /// fsmonitor, and signing never run, and Git ignores inherited variables
    /// that redirect it or choose programs. None of these change results.
    Internal,
    /// An operation the user requested, such as commit, push, or discard.
    User,
}

/// Whether the user trusts the folder's repository configuration on this
/// device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitFolderTrust {
    /// Repository configuration applies exactly as in the user's terminal
    /// while the folder's Git configuration digest equals the trusted one.
    /// A run that finds another digest is untrusted.
    Trusted(FolderConfigDigest),
    /// Programs named by local or worktree configuration, and repository
    /// hooks, are replaced by the user's own value or a neutral one.
    Untrusted,
}

/// Which configuration a Git run may honor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitPolicy {
    pub origin: GitOrigin,
    pub trust: GitFolderTrust,
}

impl GitPolicy {
    pub const fn internal(trust: GitFolderTrust) -> Self {
        Self {
            origin: GitOrigin::Internal,
            trust,
        }
    }

    pub const fn user(trust: GitFolderTrust) -> Self {
        Self {
            origin: GitOrigin::User,
            trust,
        }
    }

    fn is_internal(self) -> bool {
        self.origin == GitOrigin::Internal
    }

    fn neutralizes_repository(self) -> bool {
        self.trust == GitFolderTrust::Untrusted
    }

    fn untrusted(self) -> Self {
        Self {
            trust: GitFolderTrust::Untrusted,
            ..self
        }
    }
}

/// Reads the effective configuration and settles a trusted policy against
/// the folder's current digest. Returns the policy to run with and the
/// listing its hardening needs.
fn resolve_policy(
    root: &Path,
    policy: GitPolicy,
    environment: &[(&str, &OsStr)],
    deadline: Duration,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(GitPolicy, Vec<GitConfigEntry>), GitRunError> {
    let entries = read_effective_config(root, policy, environment, deadline, cancel)?;
    let GitFolderTrust::Trusted(expected) = policy.trust else {
        return Ok((policy, entries));
    };
    match digest::git_configuration(root, &entries, deadline, cancel) {
        Ok(current) if current.digest == expected => Ok((policy, entries)),
        Ok(_) | Err(GitRunError::Configuration | GitRunError::OutputTooLarge) => {
            Ok((policy.untrusted(), entries))
        }
        Err(error) => Err(error),
    }
}

/// Scope reported by `git config --list --show-scope`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitConfigScope {
    System,
    Global,
    Local,
    Worktree,
    Command,
    Unknown,
}

impl GitConfigScope {
    fn parse(value: &str) -> Self {
        match value {
            "system" => Self::System,
            "global" => Self::Global,
            "local" => Self::Local,
            "worktree" => Self::Worktree,
            "command" => Self::Command,
            _ => Self::Unknown,
        }
    }

    /// Unknown scopes are treated as repository content.
    fn is_repository(self) -> bool {
        matches!(self, Self::Local | Self::Worktree | Self::Unknown)
    }
}

/// One entry of the effective configuration, in Git's precedence order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitConfigEntry {
    pub scope: GitConfigScope,
    /// Section and variable names are lowercase; subsections keep their case.
    pub key: String,
    /// `None` for a key written without `=`, which Git reads as boolean true.
    pub value: Option<String>,
}

/// Environment values that decide how SSH, askpass, and proxy overrides apply.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitInheritedEnvironment {
    pub git_ssh_command: Option<OsString>,
    pub git_ssh: Option<OsString>,
    pub git_askpass: Option<OsString>,
    pub ssh_askpass: Option<OsString>,
    pub git_proxy_command: Option<OsString>,
}

impl GitInheritedEnvironment {
    /// Reads the variables of this process that `policy` keeps.
    pub fn current(policy: GitPolicy) -> Self {
        let read = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        if policy.is_internal() {
            return Self {
                ssh_askpass: read("SSH_ASKPASS"),
                ..Self::default()
            };
        }
        Self {
            git_ssh_command: read("GIT_SSH_COMMAND"),
            git_ssh: read("GIT_SSH"),
            git_askpass: read("GIT_ASKPASS"),
            ssh_askpass: read("SSH_ASKPASS"),
            git_proxy_command: std::env::var_os("GIT_PROXY_COMMAND"),
        }
    }
}

/// Configuration and environment overrides for one Git run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitHardening {
    /// Configuration pairs passed with `-c`, in order.
    pub config: Vec<(String, String)>,
    /// Environment variables to set.
    pub environment: Vec<(String, OsString)>,
    /// Environment variables to remove before `environment` is applied.
    pub cleared_environment: Vec<String>,
}

impl GitHardening {
    /// Appends the overrides to a `git` command before its subcommand.
    ///
    /// Keys containing `=` cannot be expressed with `-c`. Git reads
    /// `GIT_CONFIG_COUNT` before `-c`, and multi-valued keys such as
    /// credential helpers depend on order, so then every pair travels through
    /// the environment form (Git 2.31 and later).
    pub fn apply(&self, command: &mut Command) {
        for name in &self.cleared_environment {
            command.env_remove(name);
        }
        for (name, value) in &self.environment {
            command.env(name, value);
        }
        if self.config.iter().any(|(key, _)| key.contains('=')) {
            let pairs = self
                .config
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str()))
                .collect::<Vec<_>>();
            for (name, value) in config_environment(&pairs, 0) {
                command.env(name, value);
            }
            return;
        }
        for (key, value) in &self.config {
            command.arg("-c").arg(format!("{key}={value}"));
        }
    }

    /// Environment form for programs that start Git themselves, such as
    /// hosted source-control CLIs. `inherited_count` is the existing
    /// `GIT_CONFIG_COUNT` the child would otherwise see.
    pub fn child_environment(&self, inherited_count: usize) -> Vec<(String, OsString)> {
        let mut environment = self.environment.clone();
        let pairs = self
            .config
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        environment.extend(config_environment(&pairs, inherited_count));
        environment
    }
}

fn config_environment(pairs: &[(&str, &str)], offset: usize) -> Vec<(String, OsString)> {
    let mut environment = Vec::with_capacity(pairs.len() * 2 + 1);
    for (index, (key, value)) in pairs.iter().enumerate() {
        environment.push((format!("GIT_CONFIG_KEY_{}", offset + index), (*key).into()));
        environment.push((
            format!("GIT_CONFIG_VALUE_{}", offset + index),
            (*value).into(),
        ));
    }
    environment.push((
        "GIT_CONFIG_COUNT".to_string(),
        (offset + pairs.len()).to_string().into(),
    ));
    environment
}

/// Parses `git config --list --show-scope -z` output.
pub fn parse_config_listing(bytes: &[u8]) -> Option<Vec<GitConfigEntry>> {
    let mut entries = Vec::new();
    let mut fields = bytes.split(|byte| *byte == 0);
    while let Some(scope) = fields.next() {
        if scope.is_empty() {
            if fields.next().is_none() {
                break;
            }
            return None;
        }
        let item = fields.next()?;
        if item.is_empty() {
            return None;
        }
        let scope = GitConfigScope::parse(std::str::from_utf8(scope).ok()?);
        let item = String::from_utf8_lossy(item);
        let (key, value) = match item.split_once('\n') {
            Some((key, value)) => (key.to_string(), Some(value.to_string())),
            None => (item.into_owned(), None),
        };
        entries.push(GitConfigEntry { scope, key, value });
    }
    Some(entries)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CommandKey {
    /// One value wins; the neutral value is used when no user scope sets it.
    Single(&'static str),
    /// A filter driver command; neutralizing it also relaxes `required`.
    Filter,
    /// The ordered credential helper list.
    CredentialHelper,
    SshCommand,
    AskPass,
    GitProxy,
    HooksPath,
}

fn classify_key(key: &str) -> Option<CommandKey> {
    let lower = key.to_ascii_lowercase();
    let (section, rest) = lower.split_once('.')?;
    let (subsection, name) = match rest.rsplit_once('.') {
        Some((subsection, name)) => (Some(subsection), name),
        None => (None, rest),
    };
    let kind = match (section, subsection, name) {
        ("core", None, "fsmonitor") => CommandKey::Single(""),
        ("core", None, "hookspath") => CommandKey::HooksPath,
        ("core", None, "sshcommand") => CommandKey::SshCommand,
        ("core", None, "askpass") => CommandKey::AskPass,
        ("core", None, "gitproxy") => CommandKey::GitProxy,
        ("core", None, "editor") | ("sequence", None, "editor") => CommandKey::Single(":"),
        ("core", None, "pager") | ("pager", None, _) => CommandKey::Single("cat"),
        ("core", None, "alternaterefscommand") => CommandKey::Single(""),
        ("diff", None, "external") => CommandKey::Single(""),
        ("diff", Some(_), "command" | "textconv") => CommandKey::Single(""),
        ("merge", Some(_), "driver") => CommandKey::Single(""),
        ("filter", Some(_), "clean" | "smudge" | "process") => CommandKey::Filter,
        ("credential", _, "helper") => CommandKey::CredentialHelper,
        ("gpg", None, "program") | ("gpg", Some("openpgp"), "program") => CommandKey::Single("gpg"),
        ("gpg", Some("x509"), "program") => CommandKey::Single("gpgsm"),
        ("gpg", Some("ssh"), "program") => CommandKey::Single("ssh-keygen"),
        ("gpg", Some("ssh"), "defaultkeycommand") => CommandKey::Single(""),
        ("remote", Some(_), "uploadpack") => CommandKey::Single("git-upload-pack"),
        ("remote", Some(_), "receivepack") => CommandKey::Single("git-receive-pack"),
        ("interactive", None, "difffilter") => CommandKey::Single(""),
        ("lfs", Some(subsection), "path") if subsection.starts_with("customtransfer.") => {
            CommandKey::Single("")
        }
        ("lfs", Some(subsection), "clean" | "smudge") if subsection.starts_with("extension.") => {
            CommandKey::Single("")
        }
        _ => return None,
    };
    Some(kind)
}

fn is_standard_lfs_filter(key: &str, value: Option<&str>) -> bool {
    let lower = key.to_ascii_lowercase();
    let Some(name) = lower.strip_prefix("filter.lfs.") else {
        return false;
    };
    let Some(value) = value else { return false };
    STANDARD_LFS_FILTER_VALUES
        .iter()
        .any(|(expected_name, expected_value)| *expected_name == name && *expected_value == value)
}

/// Directory used as `core.hooksPath` when hooks must not run. It never
/// contains files.
pub fn empty_hooks_directory() -> &'static Path {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY.get_or_init(|| {
        #[cfg(unix)]
        {
            PathBuf::from("/dev/null")
        }
        #[cfg(not(unix))]
        {
            create_private_empty_directory()
        }
    })
}

/// Creates the empty hooks directory with an unpredictable name. When creation fails,
/// a path inside a missing directory also runs no hooks.
#[cfg(not(unix))]
fn create_private_empty_directory() -> PathBuf {
    let base = std::env::temp_dir();
    ganbaru_atomic_files::create_private_temporary_directory(&base, "ganbaru-git-no-hooks-")
        .unwrap_or_else(|_| {
            base.join(format!(
                "ganbaru-git-no-hooks-{}-missing",
                std::process::id()
            ))
            .join("hooks")
        })
}

/// Computes the overrides for `policy` from the effective configuration.
///
/// `entries` must come from a listing taken with the same environment the run
/// will use. It is ignored for trusted folders.
pub fn hardening_for(
    policy: GitPolicy,
    entries: &[GitConfigEntry],
    inherited: &GitInheritedEnvironment,
    hooks_directory: &Path,
) -> GitHardening {
    let mut hardening = GitHardening::default();
    hardening
        .config
        .push(("protocol.ext.allow".to_string(), "never".to_string()));
    if policy.is_internal() {
        hardening.cleared_environment = INTERNAL_CLEARED_ENVIRONMENT
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        hardening
            .cleared_environment
            .extend(entries_environment_names());
    }
    let hooks_path = hooks_directory.to_string_lossy().into_owned();
    if policy.is_internal() {
        hardening.config.extend([
            ("core.fsmonitor".to_string(), String::new()),
            ("core.hooksPath".to_string(), hooks_path.clone()),
            ("commit.gpgSign".to_string(), "false".to_string()),
            ("tag.gpgSign".to_string(), "false".to_string()),
        ]);
    }
    if !policy.neutralizes_repository() {
        return hardening;
    }

    let mut repository_keys = BTreeSet::new();
    let mut user_values = BTreeMap::<String, Vec<Option<&str>>>::new();
    let mut credential_helpers = Vec::<(&str, Option<&str>)>::new();
    let mut credential_override = false;
    for entry in entries {
        let Some(kind) = classify_key(&entry.key) else {
            continue;
        };
        let canonical = canonical_key(&entry.key);
        let repository = entry.scope.is_repository()
            && !(kind == CommandKey::Filter
                && is_standard_lfs_filter(&entry.key, entry.value.as_deref()));
        if kind == CommandKey::CredentialHelper {
            if repository {
                credential_override = true;
            } else {
                credential_helpers.push((entry.key.as_str(), entry.value.as_deref()));
            }
            continue;
        }
        if repository {
            repository_keys.insert((canonical, kind, entry.key.clone()));
        } else {
            user_values
                .entry(canonical)
                .or_default()
                .push(entry.value.as_deref());
        }
    }

    // Hooks in `.git/hooks` are repository content even without a hooksPath
    // key. A relative user hooks path resolves inside the repository, so only
    // an absolute one is kept.
    if !policy.is_internal() {
        let path = user_values
            .get("core.hookspath")
            .and_then(|values| values.last().copied().flatten())
            .filter(|path| Path::new(path).is_absolute() || path.starts_with("~/"))
            .map_or(hooks_path.clone(), str::to_string);
        hardening.config.push(("core.hooksPath".to_string(), path));
    }

    let mut relaxed_filters = BTreeSet::new();
    let mut emitted = BTreeSet::new();
    for (canonical, kind, key) in &repository_keys {
        if !emitted.insert(canonical.clone()) {
            continue;
        }
        let user_value = user_values
            .get(canonical)
            .and_then(|values| values.last().copied());
        match kind {
            CommandKey::Single(neutral) => {
                if policy.is_internal() && canonical == "core.fsmonitor" {
                    continue;
                }
                // A key written without `=` is boolean true.
                let value = user_value.map_or(*neutral, |value| value.unwrap_or("true"));
                hardening.config.push((key.clone(), value.to_string()));
            }
            CommandKey::Filter => match user_value.flatten() {
                Some(value) => hardening.config.push((key.clone(), value.to_string())),
                None => {
                    hardening.config.push((key.clone(), String::new()));
                    if let Some(driver) = key.rsplit_once('.').map(|(driver, _)| driver) {
                        relaxed_filters.insert(driver.to_string());
                    }
                }
            },
            CommandKey::HooksPath => {
                // Internal and untrusted runs already set the hooks path.
            }
            CommandKey::SshCommand => {
                if inherited.git_ssh_command.is_some() {
                    continue;
                }
                if let Some(value) = user_value.flatten() {
                    hardening.config.push((key.clone(), value.to_string()));
                } else if let Some(program) = &inherited.git_ssh {
                    hardening
                        .environment
                        .push(("GIT_SSH_COMMAND".to_string(), shell_quote(program)));
                } else {
                    hardening.config.push((key.clone(), "ssh".to_string()));
                }
            }
            CommandKey::AskPass => {
                if inherited.git_askpass.is_some() {
                    continue;
                }
                let value = user_value
                    .flatten()
                    .map(str::to_string)
                    .or_else(|| {
                        inherited
                            .ssh_askpass
                            .as_ref()
                            .map(|value| value.to_string_lossy().into_owned())
                    })
                    .unwrap_or_default();
                hardening.config.push((key.clone(), value));
            }
            CommandKey::GitProxy => {
                if inherited.git_proxy_command.is_none() {
                    hardening
                        .environment
                        .push(("GIT_PROXY_COMMAND".to_string(), OsString::new()));
                }
            }
            CommandKey::CredentialHelper => {}
        }
    }
    for driver in relaxed_filters {
        hardening
            .config
            .push((format!("{driver}.required"), "false".to_string()));
    }
    if credential_override {
        hardening
            .config
            .push(("credential.helper".to_string(), String::new()));
        for (key, value) in credential_helpers {
            hardening
                .config
                .push((key.to_string(), value.unwrap_or_default().to_string()));
        }
    }
    hardening
}

/// Lowercases the section and variable name and keeps the subsection, which
/// is how Git compares keys.
fn canonical_key(key: &str) -> String {
    let Some((section, rest)) = key.split_once('.') else {
        return key.to_ascii_lowercase();
    };
    match rest.rsplit_once('.') {
        Some((subsection, name)) => format!(
            "{}.{subsection}.{}",
            section.to_ascii_lowercase(),
            name.to_ascii_lowercase()
        ),
        None => format!(
            "{}.{}",
            section.to_ascii_lowercase(),
            rest.to_ascii_lowercase()
        ),
    }
}

/// Numbered `GIT_CONFIG_KEY_n` and `GIT_CONFIG_VALUE_n` names present in this
/// process.
fn entries_environment_names() -> Vec<String> {
    std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| {
            name.strip_prefix("GIT_CONFIG_KEY_")
                .or_else(|| name.strip_prefix("GIT_CONFIG_VALUE_"))
                .is_some_and(|suffix| suffix.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .collect()
}

fn shell_quote(value: &OsStr) -> OsString {
    let text = value.to_string_lossy();
    format!("'{}'", text.replace('\'', "'\\''")).into()
}

/// Where a run writes standard output.
pub enum GitStdout<'a> {
    /// Collect at most this many bytes in memory.
    Memory(usize),
    /// Stream into a new file, failing past this many bytes.
    File(&'a Path, usize),
}

/// One bounded Git run.
pub struct GitRun<'a> {
    pub root: &'a Path,
    pub arguments: &'a [&'a OsStr],
    pub policy: GitPolicy,
    pub deadline: Duration,
    pub input: Option<&'a [u8]>,
    pub stdout: GitStdout<'a>,
    /// Variables set after hardening, such as a private index or object store.
    pub environment: &'a [(&'a str, &'a OsStr)],
    /// Set by an owner to abandon the run; the process tree is stopped.
    pub cancel: Option<Arc<AtomicBool>>,
}

/// Completed Git run.
#[derive(Debug)]
pub struct GitRunOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    /// Bytes written when standard output went to a file.
    pub stdout_bytes: u64,
    pub stderr: Vec<u8>,
}

/// Why a Git run did not complete.
#[derive(Debug)]
pub enum GitRunError {
    /// The program is not installed or not on `PATH`.
    Missing,
    /// Git could not be started or its pipes failed.
    Unavailable,
    /// The effective configuration could not be read safely.
    Configuration,
    /// The deadline passed; the process tree was stopped.
    TimedOut,
    /// Output passed its bound; the process tree was stopped.
    OutputTooLarge,
    /// The owner cancelled the run; the process tree was stopped.
    Cancelled,
}

impl From<GitRunError> for ChatError {
    fn from(error: GitRunError) -> Self {
        match error {
            GitRunError::Missing => ChatError::new(
                ChatErrorCode::ExecutableMissing,
                "Git is not available on PATH",
                true,
            ),
            GitRunError::Unavailable => ChatError::new(
                ChatErrorCode::DriverUnavailable,
                "Git is unavailable for this workspace",
                true,
            ),
            GitRunError::Configuration => ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                "Git configuration could not be read",
                true,
            ),
            GitRunError::TimedOut => {
                ChatError::new(ChatErrorCode::Timeout, "Git operation timed out", true)
            }
            GitRunError::OutputTooLarge => ChatError::new(
                ChatErrorCode::Protocol,
                "Git output exceeds the supported limit",
                true,
            ),
            GitRunError::Cancelled => ChatError::new(
                ChatErrorCode::Cancelled,
                "Git operation was cancelled",
                true,
            ),
        }
    }
}

/// Runs Git with the hardening of `run.policy` and blocks until it finishes,
/// the deadline passes, or the run is cancelled.
pub fn run_blocking(run: GitRun<'_>) -> Result<GitRunOutput, GitRunError> {
    let started = Instant::now();
    let inherited = GitInheritedEnvironment::current(run.policy);
    let (policy, entries) = resolve_policy(
        run.root,
        run.policy,
        run.environment,
        run.deadline,
        run.cancel.as_ref(),
    )?;
    let hardening = hardening_for(policy, &entries, &inherited, empty_hooks_directory());
    let remaining = run.deadline.saturating_sub(started.elapsed());
    let mut command = base_command(run.root);
    hardening.apply(&mut command);
    command.args(run.arguments);
    if run.arguments.first().is_some_and(|subcommand| {
        PATHSPEC_LITERAL_UNSUPPORTED
            .iter()
            .any(|name| *subcommand == OsStr::new(name))
    }) {
        command.env("GIT_LITERAL_PATHSPECS", "0");
    }
    for (name, value) in run.environment {
        command.env(name, value);
    }
    execute(
        command,
        run.input,
        run.stdout,
        remaining,
        run.cancel.as_ref(),
    )
}

/// Runs an internal read with the default deadline and no optional locks,
/// collecting at most `maximum_output_bytes` of standard output.
///
/// These reads (listings, ignore checks, directory probes) do not depend on
/// repository programs, so they run untrusted and need no trust record.
pub fn internal_read_blocking(
    root: &Path,
    arguments: &[&str],
    input: Option<&[u8]>,
    maximum_output_bytes: usize,
) -> Result<GitRunOutput, GitRunError> {
    let arguments = arguments.iter().map(OsStr::new).collect::<Vec<_>>();
    run_blocking(GitRun {
        root,
        arguments: &arguments,
        policy: GitPolicy::internal(GitFolderTrust::Untrusted),
        deadline: GIT_READ_DEADLINE,
        input,
        stdout: GitStdout::Memory(maximum_output_bytes),
        environment: &[("GIT_OPTIONAL_LOCKS", OsStr::new("0"))],
        cancel: None,
    })
}

/// Async form of [`run_blocking`]. Dropping the future stops the process
/// tree.
pub async fn run(run: OwnedGitRun) -> Result<GitRunOutput, GitRunError> {
    let cancel = Arc::new(AtomicBool::new(false));
    let guard = CancelOnDrop(Arc::clone(&cancel));
    let task = tokio::task::spawn_blocking(move || {
        let arguments = run
            .arguments
            .iter()
            .map(OsString::as_os_str)
            .collect::<Vec<_>>();
        let environment = run
            .environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_os_str()))
            .collect::<Vec<_>>();
        run_blocking(GitRun {
            root: &run.root,
            arguments: &arguments,
            policy: run.policy,
            deadline: run.deadline,
            input: run.input.as_deref(),
            stdout: match &run.stdout_file {
                Some(path) => GitStdout::File(path, run.maximum_output_bytes),
                None => GitStdout::Memory(run.maximum_output_bytes),
            },
            environment: &environment,
            cancel: Some(cancel),
        })
    });
    let result = task.await.map_err(|_| GitRunError::Unavailable)?;
    drop(guard);
    result
}

/// Environment that carries a policy to a program which starts Git itself,
/// such as a hosted source-control CLI.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitChildEnvironment {
    /// Variables removed before `set` is applied.
    pub cleared: Vec<String>,
    pub set: Vec<(String, OsString)>,
}

impl GitChildEnvironment {
    pub fn apply(&self, command: &mut Command) {
        for name in &self.cleared {
            command.env_remove(name);
        }
        for (name, value) in &self.set {
            command.env(name, value);
        }
    }
}

/// Builds the child environment of `policy` for programs started in
/// `directory`. Untrusted folders have their effective configuration read
/// first, as before a Git run.
pub fn child_environment_blocking(
    directory: &Path,
    policy: GitPolicy,
) -> Result<GitChildEnvironment, GitRunError> {
    let inherited = GitInheritedEnvironment::current(policy);
    let (policy, entries) = resolve_policy(directory, policy, &[], GIT_CONFIG_READ_DEADLINE, None)?;
    let hardening = hardening_for(policy, &entries, &inherited, empty_hooks_directory());
    let inherited_count = if hardening
        .cleared_environment
        .iter()
        .any(|name| name == "GIT_CONFIG_COUNT")
    {
        0
    } else {
        std::env::var("GIT_CONFIG_COUNT")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0)
    };
    let mut set = vec![
        ("GIT_TERMINAL_PROMPT".to_string(), OsString::from("0")),
        ("GCM_INTERACTIVE".to_string(), OsString::from("never")),
    ];
    set.extend(hardening.child_environment(inherited_count));
    Ok(GitChildEnvironment {
        cleared: hardening.cleared_environment,
        set,
    })
}

/// Async form of [`child_environment_blocking`].
pub async fn child_environment(
    directory: PathBuf,
    policy: GitPolicy,
) -> Result<GitChildEnvironment, GitRunError> {
    tokio::task::spawn_blocking(move || child_environment_blocking(&directory, policy))
        .await
        .map_err(|_| GitRunError::Unavailable)?
}

/// Runs a program that may start Git, such as a hosted source-control CLI,
/// in its own process tree with a deadline and bounded output. Dropping the
/// future stops the tree.
pub async fn run_program(
    command: Command,
    deadline: Duration,
    maximum_output_bytes: usize,
) -> Result<GitRunOutput, GitRunError> {
    let cancel = Arc::new(AtomicBool::new(false));
    let guard = CancelOnDrop(Arc::clone(&cancel));
    let task = tokio::task::spawn_blocking(move || {
        execute(
            command,
            None,
            GitStdout::Memory(maximum_output_bytes),
            deadline,
            Some(&cancel),
        )
    });
    let result = task.await.map_err(|_| GitRunError::Unavailable)?;
    drop(guard);
    result
}

/// Owned arguments for [`run`].
pub struct OwnedGitRun {
    pub root: PathBuf,
    pub arguments: Vec<OsString>,
    pub policy: GitPolicy,
    pub deadline: Duration,
    pub input: Option<Vec<u8>>,
    pub maximum_output_bytes: usize,
    pub stdout_file: Option<PathBuf>,
    pub environment: Vec<(String, OsString)>,
}

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn base_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C");
    command
}

/// Reads the effective configuration with the environment the run will use.
fn read_effective_config(
    root: &Path,
    policy: GitPolicy,
    environment: &[(&str, &OsStr)],
    deadline: Duration,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<Vec<GitConfigEntry>, GitRunError> {
    let mut command = base_command(root);
    let inherited = GitInheritedEnvironment::current(policy);
    let mut hardening = hardening_for(policy, &[], &inherited, empty_hooks_directory());
    hardening.config.clear();
    hardening.apply(&mut command);
    for (name, value) in environment {
        command.env(name, value);
    }
    command.args(["config", "--list", "--show-scope", "-z"]);
    let output = execute(
        command,
        None,
        GitStdout::Memory(MAX_CONFIG_LISTING_BYTES),
        deadline.min(GIT_CONFIG_READ_DEADLINE),
        cancel,
    )
    .map_err(|error| match error {
        GitRunError::TimedOut
        | GitRunError::Cancelled
        | GitRunError::Missing
        | GitRunError::Unavailable => error,
        _ => GitRunError::Configuration,
    })?;
    if !output.status.success() {
        return Err(GitRunError::Configuration);
    }
    parse_config_listing(&output.stdout).ok_or(GitRunError::Configuration)
}

enum StdoutResult {
    Memory(Vec<u8>),
    File(u64),
}

fn execute(
    mut command: Command,
    input: Option<&[u8]>,
    stdout_target: GitStdout<'_>,
    deadline: Duration,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<GitRunOutput, GitRunError> {
    let started = Instant::now();
    let stdout_file = match &stdout_target {
        GitStdout::File(path, _) => Some(
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .map_err(|_| GitRunError::Unavailable)?,
        ),
        GitStdout::Memory(_) => None,
    };
    let stdout_limit = match stdout_target {
        GitStdout::Memory(limit) | GitStdout::File(_, limit) => limit,
    };
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_std_process_tree(&mut command);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            GitRunError::Missing
        } else {
            GitRunError::Unavailable
        }
    })?;
    let tree = ProcessTree::claim_std(&mut child).map_err(|_| GitRunError::Unavailable)?;
    let overflow = Arc::new(AtomicBool::new(false));
    let (stdout_sender, stdout_receiver) = mpsc::channel();
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    let pipes = (
        child.stdout.take(),
        child.stderr.take(),
        input.map(|_| child.stdin.take()),
    );
    let (Some(stdout), Some(stderr)) = (pipes.0, pipes.1) else {
        tree.terminate();
        let _ = child.kill();
        let _ = child.wait();
        return Err(GitRunError::Unavailable);
    };
    {
        let overflow = Arc::clone(&overflow);
        std::thread::spawn(move || {
            let result = match stdout_file {
                Some(file) => {
                    copy_bounded(stdout, file, stdout_limit, &overflow).map(StdoutResult::File)
                }
                None => read_bounded(stdout, stdout_limit, &overflow).map(StdoutResult::Memory),
            };
            let _ = stdout_sender.send(result);
        });
    }
    {
        let overflow = Arc::clone(&overflow);
        std::thread::spawn(move || {
            let _ = stderr_sender.send(read_bounded(stderr, MAX_STDERR_BYTES, &overflow));
        });
    }
    if let Some(stdin) = pipes.2 {
        let Some(mut stdin) = stdin else {
            tree.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitRunError::Unavailable);
        };
        let bytes = input.unwrap_or_default().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }

    let mut interval = POLL_INTERVAL_START;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                stop(&tree, &mut child);
                return Err(GitRunError::Unavailable);
            }
        }
        if let Some(error) = interruption(started, deadline, &overflow, cancel) {
            stop(&tree, &mut child);
            return Err(error);
        }
        std::thread::sleep(interval);
        interval = (interval * 2).min(POLL_INTERVAL_MAX);
    };
    let stdout = receive(
        &stdout_receiver,
        started,
        deadline,
        &overflow,
        cancel,
        &tree,
    )?;
    let stderr = receive(
        &stderr_receiver,
        started,
        deadline,
        &overflow,
        cancel,
        &tree,
    )?;
    if overflow.load(Ordering::Relaxed) {
        return Err(GitRunError::OutputTooLarge);
    }
    let (stdout, stdout_bytes) = match stdout {
        StdoutResult::Memory(bytes) => {
            let length = bytes.len() as u64;
            (bytes, length)
        }
        StdoutResult::File(length) => (Vec::new(), length),
    };
    Ok(GitRunOutput {
        status,
        stdout,
        stdout_bytes,
        stderr,
    })
}

fn interruption(
    started: Instant,
    deadline: Duration,
    overflow: &AtomicBool,
    cancel: Option<&Arc<AtomicBool>>,
) -> Option<GitRunError> {
    if overflow.load(Ordering::Relaxed) {
        Some(GitRunError::OutputTooLarge)
    } else if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
        Some(GitRunError::Cancelled)
    } else if started.elapsed() >= deadline {
        Some(GitRunError::TimedOut)
    } else {
        None
    }
}

fn stop(tree: &ProcessTree, child: &mut std::process::Child) {
    tree.terminate();
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits for a reader after Git exited. A descendant that keeps the pipe open
/// past the deadline is stopped with the tree.
fn receive<T>(
    receiver: &mpsc::Receiver<std::io::Result<T>>,
    started: Instant,
    deadline: Duration,
    overflow: &AtomicBool,
    cancel: Option<&Arc<AtomicBool>>,
    tree: &ProcessTree,
) -> Result<T, GitRunError> {
    loop {
        match receiver.recv_timeout(POLL_INTERVAL_MAX) {
            Ok(result) => return result.map_err(|_| GitRunError::Unavailable),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(GitRunError::Unavailable),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(error) = interruption(started, deadline, overflow, cancel) {
                    tree.terminate();
                    return Err(error);
                }
            }
        }
    }
}

fn read_bounded<R: Read>(
    mut reader: R,
    limit: usize,
    overflow: &AtomicBool,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(bytes);
        }
        if bytes.len() + read > limit {
            overflow.store(true, Ordering::Relaxed);
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

fn copy_bounded<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    limit: usize,
    overflow: &AtomicBool,
) -> std::io::Result<u64> {
    let mut total = 0_usize;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            writer.flush()?;
            return Ok(total as u64);
        }
        total = total.saturating_add(read);
        if total > limit {
            overflow.store(true, Ordering::Relaxed);
            return Ok(total as u64);
        }
        writer.write_all(&buffer[..read])?;
    }
}

#[cfg(test)]
#[path = "hardened/tests.rs"]
mod tests;
