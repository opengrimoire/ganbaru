//! Detection of command-bearing metadata changes made by confined agents.
//!
//! A confined command writes the working folder, and the Git directory inside it, because Git
//! itself must update refs, objects, and the index. A command could also plant a hook or a
//! command-bearing configuration key that a later unconfined Git process would run, and a
//! agent that writes files could plant a Cursor MCP server, hook, or command allowlist that a
//! later unconfined Cursor run would start. The guard records a digest of that command-bearing
//! metadata when the session starts and compares it after every confined command and file
//! write. A change is detected, never reverted: the
//! session ends with an error and the folder runs no further confined commands in it.

use crate::models::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::Notify;

/// Name of the Git directory or gitfile at the root of a working folder.
const GIT_METADATA_NAME: &str = ".git";
/// Name of the Cursor project configuration folder at the root of a working folder.
const CURSOR_METADATA_NAME: &str = ".cursor";
/// Cursor project configuration entries that can name a command: MCP servers, hooks, the
/// command allowlist, and rules that steer an unconfined Cursor run.
const CURSOR_COMMAND_BEARING: &[&str] = &["cli.json", "hooks.json", "mcp.json", "rules"];
/// Git directory files whose content can name a command or redirect Git to other metadata.
const COMMAND_BEARING_FILES: &[&str] = &["config", "config.worktree", "commondir", "gitdir"];
/// Git directory folders whose whole content is command-bearing: hooks run as programs, and
/// `info` holds attributes that select configured drivers.
const COMMAND_BEARING_FOLDERS: &[&str] = &["hooks", "info"];
/// Git directory folders that hold history and state Git rewrites in ordinary use, never read
/// as commands. They are skipped only directly inside a Git directory, so a submodule or
/// worktree folder with one of these names is still watched.
const SKIPPED_FOLDERS: &[&str] = &["objects", "refs", "logs", "lfs", "rr-cache"];
/// File every Git directory holds.
const GIT_DIRECTORY_HEAD: &str = "HEAD";
/// Files of which a Git directory holds at least one; both are watched, so creating one to
/// make a folder look like a Git directory changes the digest.
const GIT_DIRECTORY_MARKERS: &[&str] = &["config", "commondir"];
/// Most entries one digest visits before the metadata counts as too large to watch.
const MAX_VISITED_ENTRIES: usize = 20_000;
/// Most bytes one digest reads before the metadata counts as too large to watch.
const MAX_DIGESTED_BYTES: u64 = 8 * 1024 * 1024;
/// Deepest folder nesting one digest descends into.
const MAX_DEPTH: usize = 16;

/// Watches the command-bearing Git metadata at the root of one working folder.
pub(crate) struct GitMetadataGuard {
    workspace: PathBuf,
    state: Mutex<GuardState>,
    tripped: Arc<Notify>,
}

enum GuardState {
    /// The metadata matches this digest.
    Watching([u8; 32]),
    /// The metadata changed or could no longer be read; no confined command runs again.
    Tripped,
}

impl GitMetadataGuard {
    /// Records the current metadata of `workspace`; a folder whose metadata cannot be digested
    /// within the bounds runs no confined commands.
    pub(crate) fn watch(workspace: &Path) -> ChatResult<Self> {
        let digest = git_metadata_digest(workspace).map_err(|_| unwatchable_error())?;
        Ok(Self {
            workspace: workspace.to_path_buf(),
            state: Mutex::new(GuardState::Watching(digest)),
            tripped: Arc::new(Notify::new()),
        })
    }

    /// Refuses when the metadata changed since the session started, tripping the guard on the
    /// first change it sees.
    pub(crate) fn verify(&self) -> ChatResult<()> {
        let Ok(mut state) = self.state.lock() else {
            return Err(changed_error());
        };
        let GuardState::Watching(baseline) = *state else {
            return Err(changed_error());
        };
        match git_metadata_digest(&self.workspace) {
            Ok(current) if current == baseline => Ok(()),
            _ => {
                *state = GuardState::Tripped;
                drop(state);
                self.tripped.notify_one();
                Err(changed_error())
            }
        }
    }

    /// Signaled once when the guard trips, so the session router can end the session.
    pub(crate) fn tripped(&self) -> Arc<Notify> {
        Arc::clone(&self.tripped)
    }
}

/// Whether `path` holds a Git directory's `HEAD` and one of its watched marker files.
fn is_git_directory(path: &Path) -> bool {
    let is_file = |name: &str| {
        std::fs::symlink_metadata(path.join(name)).is_ok_and(|metadata| metadata.is_file())
    };
    is_file(GIT_DIRECTORY_HEAD) && GIT_DIRECTORY_MARKERS.iter().any(|name| is_file(name))
}

/// Code of the warning and error a session ends with after its confined commands changed the
/// Git metadata of the folder, the same wire name for every provider.
pub(crate) const GIT_METADATA_CHANGED_CODE: &str = "acp_git_metadata_changed";

/// The error a session ends with after its confined commands changed Git metadata.
pub(crate) fn changed_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "acp_git_metadata_changed: a confined agent changed the Git hooks or configuration, \
         or the Cursor configuration, of this folder, so Ganbaru stopped the session and runs \
         no more commands in it",
        false,
    )
}

fn unwatchable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Ganbaru cannot watch the Git metadata of this folder, so it runs no commands in it",
        false,
    )
}

/// Why a digest could not be taken.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum DigestError {
    /// The metadata exceeds the entry, byte, or depth bounds.
    Unbounded,
    /// An entry could not be read.
    Unreadable,
}

/// Digest of the command-bearing Git metadata at the root of `workspace`.
///
/// It covers whether `.git` is missing, a gitfile, a symbolic link, or a directory; the bytes of
/// a gitfile; and inside a Git directory, at any depth such as submodule and worktree folders,
/// the files named in [`COMMAND_BEARING_FILES`] and everything under [`COMMAND_BEARING_FOLDERS`],
/// with their types, permission bits, link targets, and bytes. History, refs, and the index are
/// left out, so commits, checkouts, and fetches do not change it. It also covers whether
/// `.cursor` is missing, a link, a file, or a folder, and inside the folder every entry named in
/// [`CURSOR_COMMAND_BEARING`].
pub(crate) fn git_metadata_digest(workspace: &Path) -> Result<[u8; 32], DigestError> {
    let mut digest = MetadataDigest::default();
    digest.entry(
        &workspace.join(GIT_METADATA_NAME),
        Path::new(GIT_METADATA_NAME),
    )?;
    digest.cursor_configuration(
        &workspace.join(CURSOR_METADATA_NAME),
        Path::new(CURSOR_METADATA_NAME),
    )?;
    Ok(digest.hasher.finalize().into())
}

#[derive(Default)]
struct MetadataDigest {
    hasher: Sha256,
    visited: usize,
    bytes: u64,
}

impl MetadataDigest {
    /// Adds `.git` itself: a missing entry, a link, a gitfile, or a Git directory.
    fn entry(&mut self, path: &Path, relative: &Path) -> Result<(), DigestError> {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.record(b"missing", relative, 0, &[]);
                Ok(())
            }
            Err(_) => Err(DigestError::Unreadable),
            Ok(metadata) if metadata.is_dir() => self.directory(path, relative, 0),
            Ok(metadata) => {
                self.leaf(path, relative, &metadata)?;
                match redirected_git_directory(path, &metadata) {
                    Some(target) => self.directory(&target, Path::new("redirected"), 0),
                    None => Ok(()),
                }
            }
        }
    }

    /// Adds `.cursor` itself and, when it is a folder, its command-bearing entries.
    fn cursor_configuration(&mut self, path: &Path, relative: &Path) -> Result<(), DigestError> {
        let metadata = match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.record(b"missing", relative, 0, &[]);
                return Ok(());
            }
            Err(_) => return Err(DigestError::Unreadable),
            Ok(metadata) => metadata,
        };
        if !metadata.is_dir() {
            return self.leaf(path, relative, &metadata);
        }
        self.record(b"folder", relative, permission_bits(&metadata), &[]);
        for name in CURSOR_COMMAND_BEARING {
            self.visit()?;
            let child = path.join(name);
            let child_relative = relative.join(name);
            match std::fs::symlink_metadata(&child) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.record(b"missing", &child_relative, 0, &[]);
                }
                Err(_) => return Err(DigestError::Unreadable),
                Ok(child_metadata) => self.tree(&child, &child_relative, &child_metadata, 1)?,
            }
        }
        Ok(())
    }

    /// Adds the command-bearing entries of the Git directory folder at `path`.
    ///
    /// Skipped history folders are still walked when `path` is not itself a Git directory.
    fn directory(&mut self, path: &Path, relative: &Path, depth: usize) -> Result<(), DigestError> {
        if depth > MAX_DEPTH {
            return Err(DigestError::Unbounded);
        }
        let mut entries = std::fs::read_dir(path)
            .map_err(|_| DigestError::Unreadable)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| DigestError::Unreadable)?;
        entries.sort();
        let skips_history = is_git_directory(path);
        for name in entries {
            self.visit()?;
            let child = path.join(&name);
            let child_relative = relative.join(&name);
            let metadata =
                std::fs::symlink_metadata(&child).map_err(|_| DigestError::Unreadable)?;
            let name = name.to_string_lossy();
            if COMMAND_BEARING_FOLDERS.contains(&name.as_ref()) {
                self.tree(&child, &child_relative, &metadata, depth + 1)?;
            } else if metadata.is_dir() {
                if !(skips_history && SKIPPED_FOLDERS.contains(&name.as_ref())) {
                    self.directory(&child, &child_relative, depth + 1)?;
                }
            } else if COMMAND_BEARING_FILES.contains(&name.as_ref()) {
                self.leaf(&child, &child_relative, &metadata)?;
            }
        }
        Ok(())
    }

    /// Adds every entry under a command-bearing folder.
    fn tree(
        &mut self,
        path: &Path,
        relative: &Path,
        metadata: &std::fs::Metadata,
        depth: usize,
    ) -> Result<(), DigestError> {
        if !metadata.is_dir() {
            return self.leaf(path, relative, metadata);
        }
        if depth > MAX_DEPTH {
            return Err(DigestError::Unbounded);
        }
        self.record(b"folder", relative, permission_bits(metadata), &[]);
        let mut entries = std::fs::read_dir(path)
            .map_err(|_| DigestError::Unreadable)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| DigestError::Unreadable)?;
        entries.sort();
        for name in entries {
            self.visit()?;
            let child = path.join(&name);
            let child_metadata =
                std::fs::symlink_metadata(&child).map_err(|_| DigestError::Unreadable)?;
            self.tree(&child, &relative.join(&name), &child_metadata, depth + 1)?;
        }
        Ok(())
    }

    /// Adds one file or link with its bytes or target.
    fn leaf(
        &mut self,
        path: &Path,
        relative: &Path,
        metadata: &std::fs::Metadata,
    ) -> Result<(), DigestError> {
        let mode = permission_bits(metadata);
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(path).map_err(|_| DigestError::Unreadable)?;
            self.record(
                b"link",
                relative,
                mode,
                target.as_os_str().as_encoded_bytes(),
            );
            return Ok(());
        }
        if !metadata.is_file() {
            self.record(b"special", relative, mode, &[]);
            return Ok(());
        }
        self.bytes = self.bytes.saturating_add(metadata.len());
        if self.bytes > MAX_DIGESTED_BYTES {
            return Err(DigestError::Unbounded);
        }
        let content = std::fs::read(path).map_err(|_| DigestError::Unreadable)?;
        self.record(b"file", relative, mode, &content);
        Ok(())
    }

    fn visit(&mut self) -> Result<(), DigestError> {
        self.visited += 1;
        if self.visited > MAX_VISITED_ENTRIES {
            return Err(DigestError::Unbounded);
        }
        Ok(())
    }

    /// Hashes one entry with length prefixes, so no two entry lists hash the same bytes.
    fn record(&mut self, kind: &[u8], relative: &Path, mode: u32, content: &[u8]) {
        let relative = relative.as_os_str().as_encoded_bytes();
        for part in [kind, relative, &mode.to_le_bytes(), content] {
            self.hasher.update((part.len() as u64).to_le_bytes());
            self.hasher.update(part);
        }
    }
}

/// The Git directory a `.git` link or gitfile names, when it lies inside the working folder that
/// holds `.git`, where a confined command could write it. A directory outside the folder is out
/// of a confined command's reach, so only the link or gitfile bytes are watched.
fn redirected_git_directory(path: &Path, metadata: &std::fs::Metadata) -> Option<PathBuf> {
    let workspace = path.parent()?;
    let target = if metadata.file_type().is_symlink() {
        path.to_path_buf()
    } else {
        let content = std::fs::read_to_string(path).ok()?;
        let named = content.lines().next()?.strip_prefix("gitdir:")?.trim();
        workspace.join(named)
    };
    let target = std::fs::canonicalize(target).ok()?;
    (target.is_dir() && target.starts_with(workspace)).then_some(target)
}

#[cfg(unix)]
fn permission_bits(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn permission_bits(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe_directory::ProbeDirectory;
    use std::fs;

    fn repository() -> ProbeDirectory {
        let workspace = ProbeDirectory::create().unwrap();
        let git = workspace.join(".git");
        for folder in ["hooks", "info", "objects/ab", "refs/heads", "logs"] {
            fs::create_dir_all(git.join(folder)).unwrap();
        }
        fs::write(git.join("config"), "[core]\n\tbare = false\n").unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(git.join("hooks/pre-commit.sample"), "#!/bin/sh\n").unwrap();
        fs::write(git.join("info/exclude"), "").unwrap();
        workspace
    }

    #[test]
    fn history_refs_and_the_index_leave_the_digest_unchanged() {
        let workspace = repository();
        let before = git_metadata_digest(&workspace).unwrap();
        let git = workspace.join(".git");
        fs::write(git.join("objects/ab/cdef"), "object").unwrap();
        fs::write(git.join("refs/heads/main"), "0123").unwrap();
        fs::write(git.join("logs/HEAD"), "entry").unwrap();
        fs::write(git.join("index"), "index").unwrap();
        fs::write(git.join("HEAD"), "ref: refs/heads/other\n").unwrap();
        fs::write(workspace.join("README.md"), "text").unwrap();
        assert_eq!(git_metadata_digest(&workspace).unwrap(), before);
    }

    #[test]
    fn hooks_configuration_and_their_permissions_change_the_digest() {
        let changes: [fn(&Path); 8] = [
            |git| {
                fs::create_dir_all(git.join("modules/refs/hooks")).unwrap();
                fs::write(git.join("modules/refs/hooks/post-checkout"), "run").unwrap();
            },
            |git| {
                fs::create_dir_all(git.join("worktrees/logs")).unwrap();
                fs::write(git.join("worktrees/logs/config.worktree"), "run").unwrap();
            },
            |git| fs::write(git.join("hooks/pre-commit"), "#!/bin/sh\nrun\n").unwrap(),
            |git| fs::write(git.join("config"), "[core]\n\tfsmonitor = run\n").unwrap(),
            |git| fs::write(git.join("info/attributes"), "* filter=run\n").unwrap(),
            |git| fs::write(git.join("config.worktree"), "").unwrap(),
            |git| {
                fs::create_dir_all(git.join("modules/lib/hooks")).unwrap();
                fs::write(git.join("modules/lib/hooks/post-checkout"), "run").unwrap();
            },
            |git| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let hook = git.join("hooks/pre-commit.sample");
                    fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
                }
                #[cfg(not(unix))]
                fs::write(git.join("hooks/pre-commit.sample"), "changed").unwrap();
            },
        ];
        for (index, change) in changes.into_iter().enumerate() {
            let workspace = repository();
            let before = git_metadata_digest(&workspace).unwrap();
            change(&workspace.join(".git"));
            assert_ne!(
                git_metadata_digest(&workspace).unwrap(),
                before,
                "change {index}"
            );
        }
    }

    #[test]
    fn replacing_or_creating_the_git_entry_changes_the_digest() {
        let workspace = ProbeDirectory::create().unwrap();
        let missing = git_metadata_digest(&workspace).unwrap();
        assert_eq!(git_metadata_digest(&workspace).unwrap(), missing);
        fs::create_dir_all(workspace.join("elsewhere/hooks")).unwrap();
        fs::write(workspace.join(".git"), "gitdir: elsewhere\n").unwrap();
        let gitfile = git_metadata_digest(&workspace).unwrap();
        assert_ne!(gitfile, missing);
        fs::write(workspace.join("elsewhere/hooks/pre-push"), "run").unwrap();
        assert_ne!(git_metadata_digest(&workspace).unwrap(), gitfile);
    }

    #[test]
    fn metadata_nested_past_the_depth_bound_is_unwatchable() {
        let workspace = repository();
        let mut deep = workspace.join(".git/hooks");
        for level in 0..=MAX_DEPTH {
            deep.push(format!("level-{level}"));
        }
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(git_metadata_digest(&workspace), Err(DigestError::Unbounded));
        assert!(GitMetadataGuard::watch(&workspace).is_err());
    }

    #[test]
    fn cursor_command_configuration_changes_the_digest() {
        let changes: [fn(&Path); 6] = [
            |cursor| fs::write(cursor.join("mcp.json"), "{\"mcpServers\":{}}").unwrap(),
            |cursor| fs::write(cursor.join("hooks.json"), "{}").unwrap(),
            |cursor| fs::write(cursor.join("cli.json"), "{}").unwrap(),
            |cursor| fs::write(cursor.join("rules/style.mdc"), "changed").unwrap(),
            |cursor| fs::remove_dir_all(cursor).unwrap(),
            |cursor| {
                fs::remove_dir_all(cursor).unwrap();
                fs::write(cursor, "file").unwrap();
            },
        ];
        for (index, change) in changes.into_iter().enumerate() {
            let workspace = repository();
            let cursor = workspace.join(".cursor");
            fs::create_dir_all(cursor.join("rules")).unwrap();
            fs::write(cursor.join("rules/style.mdc"), "rule").unwrap();
            let before = git_metadata_digest(&workspace).unwrap();
            change(&cursor);
            assert_ne!(
                git_metadata_digest(&workspace).unwrap(),
                before,
                "change {index}"
            );
        }
    }

    #[test]
    fn other_cursor_files_and_a_missing_cursor_folder_leave_the_digest_unchanged() {
        let workspace = repository();
        let before = git_metadata_digest(&workspace).unwrap();
        assert_eq!(git_metadata_digest(&workspace).unwrap(), before);
        fs::create_dir_all(workspace.join("src/.cursor")).unwrap();
        fs::write(workspace.join("src/.cursor/mcp.json"), "{}").unwrap();
        assert_eq!(git_metadata_digest(&workspace).unwrap(), before);
        fs::create_dir_all(workspace.join(".cursor")).unwrap();
        let folder = git_metadata_digest(&workspace).unwrap();
        assert_ne!(folder, before);
        fs::write(workspace.join(".cursor/notes.txt"), "text").unwrap();
        assert_eq!(git_metadata_digest(&workspace).unwrap(), folder);
    }

    #[tokio::test]
    async fn a_tripped_guard_stays_tripped_and_signals_once() {
        let workspace = repository();
        let guard = GitMetadataGuard::watch(&workspace).unwrap();
        guard.verify().unwrap();
        let hook = workspace.join(".git/hooks/pre-commit");
        fs::write(&hook, "run").unwrap();
        let tripped = guard.tripped();
        assert_eq!(guard.verify().unwrap_err().code, ChatErrorCode::Permission);
        fs::remove_file(&hook).unwrap();
        assert!(
            guard.verify().is_err(),
            "a reverted change keeps the guard tripped"
        );
        tokio::time::timeout(std::time::Duration::from_secs(1), tripped.notified())
            .await
            .expect("the trip is signaled");
    }
}
