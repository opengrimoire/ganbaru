//! Content digests that a device-local folder trust decision covers.
//!
//! The Git digest covers what can make Git run a program: command-bearing
//! keys from local or worktree configuration (including files those include)
//! and the files of the repository hooks directory. Ordinary keys such as
//! remotes, branches, and identity are left out, so pushing a new branch or
//! adding a remote never asks again. `.gitattributes` is left out as well: it
//! only selects drivers that configuration defines, and a branch checkout that
//! touches it must not ask again.
//!
//! The provider digest covers project provider configuration that can run
//! commands: Claude project settings, project MCP servers, and the skills,
//! agents, and commands whose definitions can carry hooks or shell steps.
//! Instructions such as `CLAUDE.md` are left out. Entries are digested per
//! file so a worktree can be checked against the folder decision.

use super::{
    GIT_CONFIG_READ_DEADLINE, GitConfigEntry, GitConfigScope, GitFolderTrust, GitPolicy,
    GitRunError, GitStdout, base_command, classify_key, empty_hooks_directory, execute,
    hardening_for, read_effective_config,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Most files a digest reads from one hooks directory tree.
pub const MAX_TRUST_DIGEST_HOOK_FILES: usize = 1_024;
/// Largest single file a digest reads.
pub const MAX_TRUST_DIGEST_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Most files a digest reads from project provider directories, kept far
/// above realistic skill and agent collections.
pub const MAX_TRUST_DIGEST_PROVIDER_FILES: usize = 8_192;
/// Deepest subdirectory a digest walks.
const MAX_DIGEST_DIRECTORY_DEPTH: usize = 8;
/// Upper bound on the repository location output.
const MAX_LOCATION_OUTPUT_BYTES: usize = 64 * 1024;
/// Provider configuration files, relative to the folder root, that can run
/// commands when the provider starts there.
const PROVIDER_CONFIGURATION_FILES: &[&str] = &[
    ".claude/settings.json",
    ".claude/settings.local.json",
    ".mcp.json",
];
/// Provider directories, relative to the folder root, whose definitions can
/// carry hooks, shell steps, or MCP servers. Codex keeps its project
/// configuration, hooks, and rules under `.codex`.
const PROVIDER_CONFIGURATION_DIRECTORIES: &[&str] = &[
    ".claude/skills",
    ".claude/agents",
    ".claude/commands",
    ".codex",
];
const GIT_DIGEST_DOMAIN: &[u8] = b"ganbaru-folder-trust-git-v1";
const PROVIDER_DIGEST_DOMAIN: &[u8] = b"ganbaru-folder-trust-provider-v3";

/// SHA-256 digest of folder configuration covered by a trust decision.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FolderConfigDigest([u8; 32]);

impl FolderConfigDigest {
    /// Lowercase hexadecimal form used in device state.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Parses the form written by [`Self::to_hex`].
    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }

    #[cfg(test)]
    pub(crate) const fn zero() -> Self {
        Self([0; 32])
    }
}

/// Digests of one folder's configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FolderConfiguration {
    /// Repository programs and hooks; verified before every trusted Git run.
    pub git: FolderConfigDigest,
    /// Project provider configuration that can run commands.
    pub provider: FolderConfigDigest,
    /// True when the folder defines no repository programs or hooks, so
    /// Git has nothing to decide.
    pub git_is_empty: bool,
    /// True when the folder has no project provider configuration.
    pub provider_is_empty: bool,
}

/// Reads and digests the configuration of the folder at `root`.
pub fn folder_configuration_blocking(root: &Path) -> Result<FolderConfiguration, GitRunError> {
    let policy = GitPolicy::internal(GitFolderTrust::Untrusted);
    let entries = read_effective_config(root, policy, &[], GIT_CONFIG_READ_DEADLINE, None)?;
    let git = git_configuration(root, &entries, GIT_CONFIG_READ_DEADLINE, None)?;
    let provider = provider_configuration(root)?;
    Ok(FolderConfiguration {
        git: git.digest,
        provider: provider.digest,
        git_is_empty: git.is_empty,
        provider_is_empty: provider.is_empty,
    })
}

/// Async form of [`folder_configuration_blocking`].
pub async fn folder_configuration(root: PathBuf) -> Result<FolderConfiguration, GitRunError> {
    tokio::task::spawn_blocking(move || folder_configuration_blocking(&root))
        .await
        .map_err(|_| GitRunError::Unavailable)?
}

pub(super) struct DigestOutcome {
    pub digest: FolderConfigDigest,
    pub is_empty: bool,
}

/// Digests the repository programs and hooks of `root` from a listing taken
/// with the environment of the coming run.
pub(super) fn git_configuration(
    root: &Path,
    entries: &[GitConfigEntry],
    deadline: Duration,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<DigestOutcome, GitRunError> {
    let mut hasher = DigestWriter::new(GIT_DIGEST_DOMAIN);
    let mut is_empty = true;
    for entry in entries
        .iter()
        .filter(|entry| entry.scope.is_repository() && classify_key(&entry.key).is_some())
    {
        is_empty = false;
        hasher.field(b"config");
        hasher.field(scope_label(entry.scope).as_bytes());
        hasher.field(entry.key.as_bytes());
        match &entry.value {
            Some(value) => hasher.field(value.as_bytes()),
            None => hasher.field(b"\0valueless"),
        }
    }
    let locations = repository_locations(root, deadline, cancel)?;
    if let Some(hooks) = hooks_directory(root, entries, locations.as_ref()) {
        hasher.field(b"hooks");
        let files = hash_directory(&mut hasher, &hooks)?;
        is_empty &= files == 0;
    }
    Ok(DigestOutcome {
        digest: hasher.finish(),
        is_empty,
    })
}

fn provider_configuration(root: &Path) -> Result<DigestOutcome, GitRunError> {
    let entries = provider_configuration_entries_blocking(root)?;
    Ok(DigestOutcome {
        digest: entries.digest(),
        is_empty: entries.is_empty(),
    })
}

/// Content digests of a folder's project provider configuration, keyed by
/// path relative to the folder root.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderConfigurationEntries(BTreeMap<String, [u8; 32]>);

impl ProviderConfigurationEntries {
    /// True when the folder has no project provider configuration.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// True when every entry here exists in `other` with the same content, so
    /// a decision about `other` already covers it.
    pub fn is_covered_by(&self, other: &Self) -> bool {
        self.0
            .iter()
            .all(|(path, digest)| other.0.get(path) == Some(digest))
    }

    /// The digest a folder trust record stores for these entries.
    pub fn digest(&self) -> FolderConfigDigest {
        let mut hasher = DigestWriter::new(PROVIDER_DIGEST_DOMAIN);
        for (path, digest) in &self.0 {
            hasher.field(path.as_bytes());
            hasher.field(digest);
        }
        hasher.finish()
    }

    fn insert(&mut self, path: String, bytes: &[u8]) {
        self.0.insert(path, Sha256::digest(bytes).into());
    }

    #[cfg(test)]
    pub(crate) fn from_files(files: &[(&str, &str)]) -> Self {
        let mut entries = Self::default();
        for (path, contents) in files {
            entries.insert((*path).to_string(), contents.as_bytes());
        }
        entries
    }
}

/// Reads the project provider configuration of the folder at `root`.
pub fn provider_configuration_entries_blocking(
    root: &Path,
) -> Result<ProviderConfigurationEntries, GitRunError> {
    let mut entries = ProviderConfigurationEntries::default();
    for relative in PROVIDER_CONFIGURATION_FILES {
        if let Some(bytes) = read_bounded(&root.join(relative))? {
            entries.insert((*relative).to_string(), &bytes);
        }
    }
    let mut files = 0;
    for relative in PROVIDER_CONFIGURATION_DIRECTORIES {
        let walk = DirectoryWalk {
            limit: MAX_TRUST_DIGEST_PROVIDER_FILES,
            skip_samples: false,
        };
        walk.visit(
            &root.join(relative),
            Path::new(relative),
            0,
            &mut files,
            &mut |path, bytes| entries.insert(portable_path(path), bytes),
        )?;
    }
    Ok(entries)
}

/// Async form of [`provider_configuration_entries_blocking`].
pub async fn provider_configuration_entries(
    root: PathBuf,
) -> Result<ProviderConfigurationEntries, GitRunError> {
    tokio::task::spawn_blocking(move || provider_configuration_entries_blocking(&root))
        .await
        .map_err(|_| GitRunError::Unavailable)?
}

/// Relative path with `/` separators, so digests match across platforms.
fn portable_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

struct RepositoryLocations {
    common_directory: PathBuf,
    top_level: PathBuf,
}

/// Locates the common Git directory and the top level of the working tree.
/// Returns `None` outside a repository.
fn repository_locations(
    root: &Path,
    deadline: Duration,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<Option<RepositoryLocations>, GitRunError> {
    let policy = GitPolicy::internal(GitFolderTrust::Untrusted);
    let mut command = base_command(root);
    hardening_for(
        policy,
        &[],
        &super::GitInheritedEnvironment::current(policy),
        empty_hooks_directory(),
    )
    .apply(&mut command);
    command.args(["rev-parse", "--git-common-dir", "--show-toplevel"]);
    let output = execute(
        command,
        None,
        GitStdout::Memory(MAX_LOCATION_OUTPUT_BYTES),
        deadline.min(GIT_CONFIG_READ_DEADLINE),
        cancel,
    )?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(output.stdout).map_err(|_| GitRunError::Configuration)?;
    let mut lines = text.lines();
    let (Some(common), Some(top_level), None) = (lines.next(), lines.next(), lines.next()) else {
        return Err(GitRunError::Configuration);
    };
    let common = Path::new(common);
    Ok(Some(RepositoryLocations {
        common_directory: if common.is_absolute() {
            common.to_path_buf()
        } else {
            root.join(common)
        },
        top_level: PathBuf::from(top_level),
    }))
}

/// Resolves the hooks directory repository content controls. Hooks chosen by
/// the user's own configuration are not part of the digest.
fn hooks_directory(
    root: &Path,
    entries: &[GitConfigEntry],
    locations: Option<&RepositoryLocations>,
) -> Option<PathBuf> {
    let configured = entries
        .iter()
        .rev()
        .find(|entry| entry.key.eq_ignore_ascii_case("core.hookspath"));
    match configured {
        Some(entry) if !entry.scope.is_repository() => None,
        Some(entry) => {
            let value = entry.value.as_deref().unwrap_or_default();
            let base = locations.map_or(root, |locations| locations.top_level.as_path());
            Some(resolve_configured_path(base, value))
        }
        None => locations.map(|locations| locations.common_directory.join("hooks")),
    }
}

fn resolve_configured_path(base: &Path, value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = home_directory()
    {
        return home.join(rest);
    }
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Hashes the regular files below `directory` in name order, skipping Git's
/// `*.sample` templates. Returns how many files were hashed.
fn hash_directory(hasher: &mut DigestWriter, directory: &Path) -> Result<usize, GitRunError> {
    let mut files = 0;
    let walk = DirectoryWalk {
        limit: MAX_TRUST_DIGEST_HOOK_FILES,
        skip_samples: true,
    };
    walk.visit(
        directory,
        Path::new(""),
        0,
        &mut files,
        &mut |path, bytes| {
            hasher.field(path.to_string_lossy().as_bytes());
            hasher.field(bytes);
        },
    )?;
    if files == 0 {
        hasher.field(b"\0empty");
    }
    Ok(files)
}

/// Bounded walk over the regular files of one directory tree in name order.
struct DirectoryWalk {
    /// Most files, and most entries in one directory, before the walk fails.
    limit: usize,
    /// Skips Git's `*.sample` templates at the top level.
    skip_samples: bool,
}

impl DirectoryWalk {
    fn visit(
        &self,
        directory: &Path,
        relative: &Path,
        depth: usize,
        files: &mut usize,
        visit: &mut dyn FnMut(&Path, &[u8]),
    ) -> Result<(), GitRunError> {
        let reader = match fs::read_dir(directory) {
            Ok(reader) => reader,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => return Ok(()),
            Err(_) => return Err(GitRunError::Configuration),
        };
        let mut names = Vec::new();
        for entry in reader {
            let entry = entry.map_err(|_| GitRunError::Configuration)?;
            names.push(entry.file_name());
            if names.len() > self.limit {
                return Err(GitRunError::Configuration);
            }
        }
        names.sort();
        for name in names {
            let path = directory.join(&name);
            let relative = relative.join(&name);
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if metadata.is_dir() {
                if depth + 1 >= MAX_DIGEST_DIRECTORY_DEPTH {
                    return Err(GitRunError::Configuration);
                }
                self.visit(&path, &relative, depth + 1, files, visit)?;
                continue;
            }
            let sample = self.skip_samples
                && depth == 0
                && Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext == "sample");
            if !metadata.is_file() || sample {
                continue;
            }
            *files += 1;
            if *files > self.limit {
                return Err(GitRunError::Configuration);
            }
            if let Some(bytes) = read_bounded(&path)? {
                visit(&relative, &bytes);
            }
        }
        Ok(())
    }
}

/// Reads a whole file of at most [`MAX_TRUST_DIGEST_FILE_BYTES`]. Missing
/// files and directories read as `None`.
fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, GitRunError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(_) => return Err(GitRunError::Configuration),
    };
    let metadata = file.metadata().map_err(|_| GitRunError::Configuration)?;
    if !metadata.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(MAX_TRUST_DIGEST_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| GitRunError::Configuration)?;
    if bytes.len() as u64 > MAX_TRUST_DIGEST_FILE_BYTES {
        return Err(GitRunError::OutputTooLarge);
    }
    Ok(Some(bytes))
}

fn scope_label(scope: GitConfigScope) -> &'static str {
    match scope {
        GitConfigScope::System => "system",
        GitConfigScope::Global => "global",
        GitConfigScope::Local => "local",
        GitConfigScope::Worktree => "worktree",
        GitConfigScope::Command => "command",
        GitConfigScope::Unknown => "unknown",
    }
}

/// Length-prefixed SHA-256 writer, so adjacent fields cannot run together.
struct DigestWriter(Sha256);

impl DigestWriter {
    fn new(domain: &[u8]) -> Self {
        let mut writer = Self(Sha256::new());
        writer.field(domain);
        writer
    }

    fn field(&mut self, bytes: &[u8]) {
        self.0.update((bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
    }

    fn finish(self) -> FolderConfigDigest {
        FolderConfigDigest(self.0.finalize().into())
    }
}
