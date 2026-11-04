//! Populated submodules of a working tree and their configuration.
//!
//! Git recurses into populated submodules, for example to tell whether one
//! is dirty, and those Git processes read each submodule's own repository
//! configuration. Command-line overrides reach them, so neutralizing a
//! submodule's programs needs the keys its configuration defines. Gitlinks
//! come from the index, which Git consults whether or not `.gitmodules`
//! lists them.

use super::digest::repository_locations;
use super::{
    GitConfigEntry, GitFolderTrust, GitInheritedEnvironment, GitPolicy, GitRunError, StdoutSink,
    base_command, empty_hooks_directory, execute, hardening_for, read_effective_config,
};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Most populated submodules read for one run, across every nesting level.
pub const MAX_SUBMODULE_REPOSITORIES: usize = 512;
/// Deepest submodule nesting read.
pub const MAX_SUBMODULE_DEPTH: usize = 8;
/// Upper bound on the gitlink records kept from one index listing.
const MAX_GITLINK_LISTING_BYTES: usize = 4 * 1024 * 1024;
/// Start of an `ls-files --stage` record whose entry is a gitlink.
const GITLINK_RECORD_PREFIX: &[u8] = b"160000 ";

/// One populated submodule, listed before the submodules nested in it.
pub(super) struct Submodule {
    /// Path from the top level of the outermost working tree, `/` separated.
    pub relative: Vec<u8>,
    pub root: PathBuf,
    pub entries: Vec<GitConfigEntry>,
}

/// Reads the configuration of every populated submodule under the working
/// tree that contains `root`. `entries` and `environment` are those of the
/// coming run in `root`; nested repositories get no extra environment, as Git
/// gives them none.
pub(super) fn populated_submodules(
    root: &Path,
    policy: GitPolicy,
    entries: &[GitConfigEntry],
    environment: &[(&str, &OsStr)],
    deadline: Duration,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<Vec<Submodule>, GitRunError> {
    let started = Instant::now();
    let Some(locations) = repository_locations(root, deadline, cancel)? else {
        return Ok(Vec::new());
    };
    let mut submodules = Vec::new();
    let walk = Walk {
        policy,
        started,
        deadline,
        cancel,
    };
    walk.visit(
        Listing {
            directory: root,
            top_level: &locations.top_level,
            relative: &[],
            entries,
            environment,
            depth: 0,
        },
        &mut submodules,
    )?;
    Ok(submodules)
}

struct Walk<'a> {
    policy: GitPolicy,
    started: Instant,
    deadline: Duration,
    cancel: Option<&'a Arc<AtomicBool>>,
}

/// One repository whose index is listed.
struct Listing<'a> {
    /// Directory Git runs in.
    directory: &'a Path,
    /// Directory that index paths are relative to.
    top_level: &'a Path,
    relative: &'a [u8],
    entries: &'a [GitConfigEntry],
    environment: &'a [(&'a str, &'a OsStr)],
    depth: usize,
}

impl Walk<'_> {
    fn remaining(&self) -> Result<Duration, GitRunError> {
        let remaining = self.deadline.saturating_sub(self.started.elapsed());
        if remaining.is_zero() {
            Err(GitRunError::TimedOut)
        } else {
            Ok(remaining)
        }
    }

    fn visit(&self, listing: Listing<'_>, output: &mut Vec<Submodule>) -> Result<(), GitRunError> {
        let records = self.gitlink_records(&listing)?;
        for path in gitlink_paths(&records)? {
            let directory = listing.top_level.join(path_from_bytes(&path)?);
            if !is_populated(&directory) {
                continue;
            }
            if listing.depth >= MAX_SUBMODULE_DEPTH || output.len() >= MAX_SUBMODULE_REPOSITORIES {
                return Err(GitRunError::Configuration);
            }
            let entries = read_effective_config(
                &directory,
                self.policy,
                &[],
                self.remaining()?,
                self.cancel,
            )?;
            let mut relative = listing.relative.to_vec();
            if !relative.is_empty() {
                relative.push(b'/');
            }
            relative.extend_from_slice(&path);
            output.push(Submodule {
                relative: relative.clone(),
                root: directory.clone(),
                entries: entries.clone(),
            });
            self.visit(
                Listing {
                    directory: &directory,
                    top_level: &directory,
                    relative: &relative,
                    entries: &entries,
                    environment: &[],
                    depth: listing.depth + 1,
                },
                output,
            )?;
        }
        Ok(())
    }

    /// Lists the gitlink entries of the index, with paths from the top level.
    fn gitlink_records(&self, listing: &Listing<'_>) -> Result<Vec<u8>, GitRunError> {
        let policy = GitPolicy::internal(GitFolderTrust::Untrusted);
        let mut command = base_command(listing.directory);
        hardening_for(
            policy,
            listing.entries,
            &GitInheritedEnvironment::current(policy),
            empty_hooks_directory(),
        )
        .apply(&mut command);
        command
            .env("GIT_LITERAL_PATHSPECS", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["ls-files", "--stage", "-z", "--full-name", "--", ":(top)"]);
        for (name, value) in listing.environment {
            command.env(name, value);
        }
        let output = execute(
            command,
            None,
            StdoutSink::Gitlinks(MAX_GITLINK_LISTING_BYTES),
            self.remaining()?,
            self.cancel,
        )
        .map_err(|error| match error {
            GitRunError::OutputTooLarge => GitRunError::Configuration,
            error => error,
        })?;
        if !output.status.success() {
            return Err(GitRunError::Configuration);
        }
        Ok(output.stdout)
    }
}

/// Distinct paths of `ls-files --stage -z` gitlink records. A conflicted
/// gitlink appears once per stage.
fn gitlink_paths(records: &[u8]) -> Result<BTreeSet<Vec<u8>>, GitRunError> {
    let mut paths = BTreeSet::new();
    for record in records.split(|byte| *byte == 0) {
        if record.is_empty() {
            continue;
        }
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or(GitRunError::Configuration)?;
        let path = &record[tab + 1..];
        let valid = !path.is_empty()
            && path
                .split(|byte| *byte == b'/')
                .all(|component| !matches!(component, b"" | b"." | b".."));
        if !valid {
            return Err(GitRunError::Configuration);
        }
        paths.insert(path.to_vec());
    }
    Ok(paths)
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, GitRunError> {
    use std::os::unix::ffi::OsStrExt;
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, GitRunError> {
    std::str::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|_| GitRunError::Configuration)
}

/// Mirrors Git's check: the working directory exists and holds `.git`.
fn is_populated(directory: &Path) -> bool {
    std::fs::symlink_metadata(directory).is_ok_and(|metadata| metadata.is_dir())
        && std::fs::symlink_metadata(directory.join(".git")).is_ok()
}

/// Reads `ls-files --stage -z` output and keeps only gitlink records, so a
/// large index costs no memory beyond its submodules.
pub(super) fn read_gitlink_records<R: Read>(
    mut reader: R,
    limit: usize,
    overflow: &AtomicBool,
) -> std::io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut prefix = Vec::with_capacity(GITLINK_RECORD_PREFIX.len());
    // `None` while a record's mode is read, then whether the record is kept.
    let mut keeping = None;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(kept);
        }
        for &byte in &buffer[..read] {
            if byte == 0 {
                if keeping == Some(true) {
                    kept.push(0);
                }
                keeping = None;
                prefix.clear();
                continue;
            }
            match keeping {
                Some(true) => kept.push(byte),
                Some(false) => {}
                None => {
                    prefix.push(byte);
                    if !GITLINK_RECORD_PREFIX.starts_with(&prefix) {
                        keeping = Some(false);
                    } else if prefix.len() == GITLINK_RECORD_PREFIX.len() {
                        keeping = Some(true);
                        kept.extend_from_slice(&prefix);
                    }
                }
            }
            if kept.len() > limit {
                overflow.store(true, Ordering::Relaxed);
                return Ok(kept);
            }
        }
    }
}
