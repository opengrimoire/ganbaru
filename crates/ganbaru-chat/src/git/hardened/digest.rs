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
//! commands, such as Claude settings with hooks.

use super::{
    GIT_CONFIG_READ_DEADLINE, GitConfigEntry, GitConfigScope, GitFolderTrust, GitPolicy,
    GitRunError, GitStdout, base_command, classify_key, empty_hooks_directory, execute,
    hardening_for, read_effective_config,
};
use sha2::{Digest, Sha256};
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
/// Deepest hooks subdirectory a digest walks.
const MAX_HOOK_DIRECTORY_DEPTH: usize = 8;
/// Upper bound on the repository location output.
const MAX_LOCATION_OUTPUT_BYTES: usize = 64 * 1024;
/// Provider configuration files, relative to the folder root, that can run
/// commands when the provider starts there.
const PROVIDER_CONFIGURATION_FILES: &[&str] =
    &[".claude/settings.json", ".claude/settings.local.json"];
const GIT_DIGEST_DOMAIN: &[u8] = b"ganbaru-folder-trust-git-v1";
const PROVIDER_DIGEST_DOMAIN: &[u8] = b"ganbaru-folder-trust-provider-v1";

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
    let mut hasher = DigestWriter::new(PROVIDER_DIGEST_DOMAIN);
    let mut is_empty = true;
    for relative in PROVIDER_CONFIGURATION_FILES {
        hasher.field(relative.as_bytes());
        match read_bounded(&root.join(relative))? {
            Some(bytes) => {
                is_empty = false;
                hasher.field(&bytes);
            }
            None => hasher.field(b"\0absent"),
        }
    }
    Ok(DigestOutcome {
        digest: hasher.finish(),
        is_empty,
    })
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
    hash_directory_level(hasher, directory, Path::new(""), 0, &mut files)?;
    if files == 0 {
        hasher.field(b"\0empty");
    }
    Ok(files)
}

fn hash_directory_level(
    hasher: &mut DigestWriter,
    directory: &Path,
    relative: &Path,
    depth: usize,
    files: &mut usize,
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
        if names.len() > MAX_TRUST_DIGEST_HOOK_FILES {
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
            if depth + 1 >= MAX_HOOK_DIRECTORY_DEPTH {
                return Err(GitRunError::Configuration);
            }
            hash_directory_level(hasher, &path, &relative, depth + 1, files)?;
            continue;
        }
        if !metadata.is_file()
            || (depth == 0
                && Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext == "sample"))
        {
            continue;
        }
        *files += 1;
        if *files > MAX_TRUST_DIGEST_HOOK_FILES {
            return Err(GitRunError::Configuration);
        }
        let Some(bytes) = read_bounded(&path)? else {
            continue;
        };
        hasher.field(relative.to_string_lossy().as_bytes());
        hasher.field(&bytes);
    }
    Ok(())
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
