//! Toolchain homes a confined terminal command uses in place.
//!
//! Confined commands run with a private temporary home, so a toolchain that keeps its programs,
//! caches, or settings in the home directory would find nothing there. Each toolchain home is
//! named by the variable the app inherited or found at its default place in the home
//! directory, and the command receives that variable together with read and execute access to
//! the parts it needs. Caches stay read only, because a later unconfined build would run what a
//! command wrote there; only lock files are writable, as files, so concurrent builds keep
//! their locking.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What a confined command may do inside one toolchain home.
enum ToolchainAccess {
    /// Read and execute the whole home; for homes that hold programs and caches but no secrets.
    Whole,
    /// Only the named parts, for homes that also hold credentials.
    Parts {
        /// Parts read and executed in place.
        execute: &'static [&'static str],
        /// Parts only read.
        read: &'static [&'static str],
        /// Lock files written in place; directories are never granted here.
        locks: &'static [&'static str],
    },
}

/// One toolchain home: its variable, its default place in the home directory, and its access.
struct ToolchainHome {
    variable: &'static str,
    default: &'static str,
    access: ToolchainAccess,
}

/// Toolchain homes a confined command uses in place.
///
/// Cargo's home also holds registry credentials, so only its programs, registry and Git caches,
/// configuration, and package cache locks are granted.
const TOOLCHAIN_HOMES: &[ToolchainHome] = &[
    ToolchainHome {
        variable: "CARGO_HOME",
        default: ".cargo",
        access: ToolchainAccess::Parts {
            execute: &["bin"],
            read: &["registry", "git", "config.toml", "config"],
            locks: &[".package-cache", ".package-cache-mutate"],
        },
    },
    ToolchainHome {
        variable: "RUSTUP_HOME",
        default: ".rustup",
        access: ToolchainAccess::Whole,
    },
    ToolchainHome {
        variable: "PNPM_HOME",
        default: ".local/share/pnpm",
        access: ToolchainAccess::Whole,
    },
    ToolchainHome {
        variable: "NVM_DIR",
        default: ".nvm",
        access: ToolchainAccess::Whole,
    },
];

/// Home directory folders a toolchain home may never be, hold, or lie inside.
const PROTECTED_HOME_FOLDERS: &[&str] = &[".ssh", ".gnupg"];

/// Access and environment a confined command receives for the toolchain homes it uses.
#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::cursor) struct ToolchainGrants {
    /// Folders read and executed in place.
    pub execute: Vec<PathBuf>,
    /// Folders and files only read.
    pub read: Vec<PathBuf>,
    /// Lock files written in place.
    pub write: Vec<PathBuf>,
    /// Variables that point the command at each granted home.
    pub environment: Vec<(String, OsString)>,
}

/// A path that exists, after resolving symbolic links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cursor) struct ResolvedPath {
    pub path: PathBuf,
    pub is_file: bool,
}

/// The grants for every toolchain home this device has.
///
/// `variable` reads the app environment and `resolve` resolves an existing path, so tests can
/// supply both. A home named by a relative variable falls back to its default place. A home
/// that is, holds, or lies inside the home directory, the working folder, or a protected
/// folder such as `~/.ssh` is left out, as is any part that resolves outside its home.
pub(in crate::cursor) fn toolchain_grants(
    variable: impl Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
    workspace: &Path,
    resolve: impl Fn(&Path) -> Option<ResolvedPath>,
) -> ToolchainGrants {
    let protected = home
        .map(|home| {
            PROTECTED_HOME_FOLDERS
                .iter()
                .map(|name| resolve(&home.join(name)).map_or_else(|| home.join(name), |p| p.path))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let opens_user_files = |path: &Path| {
        home.is_some_and(|home| home.starts_with(path))
            || workspace.starts_with(path)
            || path.starts_with(workspace)
            || protected
                .iter()
                .any(|folder| folder.starts_with(path) || path.starts_with(folder))
    };
    let mut grants = ToolchainGrants::default();
    for toolchain in TOOLCHAIN_HOMES {
        let named = variable(toolchain.variable)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| home.map(|home| home.join(toolchain.default)));
        let Some(root) = named.as_deref().and_then(&resolve) else {
            continue;
        };
        if root.is_file || opens_user_files(&root.path) {
            continue;
        }
        let part = |name: &str| {
            resolve(&root.path.join(name))
                .filter(|part| part.path.starts_with(&root.path) && !opens_user_files(&part.path))
        };
        match &toolchain.access {
            ToolchainAccess::Whole => grants.execute.push(root.path.clone()),
            ToolchainAccess::Parts {
                execute,
                read,
                locks,
            } => {
                grants
                    .execute
                    .extend(execute.iter().filter_map(|name| part(name)).map(|p| p.path));
                grants
                    .read
                    .extend(read.iter().filter_map(|name| part(name)).map(|p| p.path));
                grants.write.extend(
                    locks
                        .iter()
                        .filter_map(|name| part(name))
                        .filter(|lock| lock.is_file)
                        .map(|lock| lock.path),
                );
            }
        }
        grants
            .environment
            .push((toolchain.variable.to_string(), root.path.into_os_string()));
    }
    grants
}

/// Resolves `path` on this device's file system.
pub(in crate::cursor) fn resolve_existing(path: &Path) -> Option<ResolvedPath> {
    let path = std::fs::canonicalize(path).ok()?;
    let metadata = std::fs::metadata(&path).ok()?;
    Some(ResolvedPath {
        is_file: metadata.is_file(),
        path,
    })
}
