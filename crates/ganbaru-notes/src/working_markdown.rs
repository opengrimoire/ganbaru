//! Bounded Markdown filesystem operations for authorized working folders.

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_SCAN_DEPTH: usize = 16;
const MAX_SCAN_ENTRIES: usize = 5_000;
const MAX_RELATIVE_PATH_BYTES: usize = 4_096;
/// Largest Markdown file that project Notes lists, reads, or saves.
pub const MAX_MARKDOWN_FILE_BYTES: u64 = 4 * 1024 * 1024;
const EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    ".cache",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkingMarkdownError {
    InvalidPath,
    InvalidExtension,
    InvalidUtf8,
    NotFile,
    OutsideRoot,
    SymbolicLink,
    TooLarge,
    Io,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkingMarkdownNodeKind {
    Directory,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingMarkdownNode {
    pub kind: WorkingMarkdownNodeKind,
    pub name: String,
    pub relative_path: String,
    pub children: Vec<WorkingMarkdownNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingMarkdownScan {
    pub nodes: Vec<WorkingMarkdownNode>,
    pub truncated: bool,
}

struct ScanBudget {
    visited_entries: usize,
    truncated: bool,
}

pub fn scan_markdown(root: &Path) -> Result<WorkingMarkdownScan, WorkingMarkdownError> {
    let mut budget = ScanBudget {
        visited_entries: 0,
        truncated: false,
    };
    let nodes = scan_directory(root, Path::new(""), 0, &mut budget)?;
    Ok(WorkingMarkdownScan {
        nodes,
        truncated: budget.truncated,
    })
}

/// Normalizes a relative Markdown path without touching the filesystem.
///
/// Reads and saves pass the result to the descriptor-relative workspace file
/// helpers, which open every component without following symbolic links.
pub fn validate_markdown_path(relative_path: &str) -> Result<String, WorkingMarkdownError> {
    let normalized = normalize_relative_path(relative_path)?;
    if !is_markdown_name(
        Path::new(&normalized)
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default(),
    ) {
        return Err(WorkingMarkdownError::InvalidExtension);
    }
    Ok(normalized)
}

/// Resolves a Markdown file to a canonical path for the system opener, which
/// can only receive a path.
pub fn resolve_markdown_file(
    root: &Path,
    relative_path: &str,
) -> Result<PathBuf, WorkingMarkdownError> {
    let normalized = validate_markdown_path(relative_path)?;
    let mut candidate = root.to_path_buf();
    for component in Path::new(&normalized).components() {
        let Component::Normal(segment) = component else {
            return Err(WorkingMarkdownError::InvalidPath);
        };
        candidate.push(segment);
        let metadata = fs::symlink_metadata(&candidate).map_err(file_error)?;
        if metadata.file_type().is_symlink() {
            return Err(WorkingMarkdownError::SymbolicLink);
        }
    }
    let canonical = fs::canonicalize(&candidate).map_err(file_error)?;
    if !canonical.starts_with(root) {
        return Err(WorkingMarkdownError::OutsideRoot);
    }
    let metadata = fs::metadata(&canonical).map_err(file_error)?;
    if !metadata.is_file() {
        return Err(WorkingMarkdownError::NotFile);
    }
    Ok(canonical)
}

fn scan_directory(
    root: &Path,
    relative_directory: &Path,
    depth: usize,
    budget: &mut ScanBudget,
) -> Result<Vec<WorkingMarkdownNode>, WorkingMarkdownError> {
    if depth > MAX_SCAN_DEPTH {
        budget.truncated = true;
        return Ok(Vec::new());
    }
    let directory = root.join(relative_directory);
    let mut entries = Vec::new();
    for entry in fs::read_dir(&directory).map_err(file_error)? {
        if budget.visited_entries >= MAX_SCAN_ENTRIES {
            budget.truncated = true;
            break;
        }
        budget.visited_entries += 1;
        let entry = entry.map_err(file_error)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(file_error)?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if name.chars().any(char::is_control) {
            continue;
        }
        let relative = relative_directory.join(&name);
        let Some(relative_path) = relative.to_str().map(normalize_separator) else {
            continue;
        };
        if relative_path.len() > MAX_RELATIVE_PATH_BYTES {
            budget.truncated = true;
            continue;
        }
        if metadata.is_dir() {
            if is_excluded_directory(&name) {
                continue;
            }
            let children = scan_directory(root, &relative, depth + 1, budget)?;
            if !children.is_empty() {
                entries.push(WorkingMarkdownNode {
                    kind: WorkingMarkdownNodeKind::Directory,
                    name,
                    relative_path,
                    children,
                });
            }
        } else if is_markdown_name(&name) && metadata.len() <= MAX_MARKDOWN_FILE_BYTES {
            entries.push(WorkingMarkdownNode {
                kind: WorkingMarkdownNodeKind::File,
                name,
                relative_path,
                children: Vec::new(),
            });
        } else if is_markdown_name(&name) {
            budget.truncated = true;
        }
    }
    entries.sort_by(|left, right| {
        node_rank(&left.kind)
            .cmp(&node_rank(&right.kind))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    Ok(entries)
}

fn normalize_relative_path(relative_path: &str) -> Result<String, WorkingMarkdownError> {
    if relative_path.is_empty()
        || relative_path.len() > MAX_RELATIVE_PATH_BYTES
        || relative_path.chars().any(char::is_control)
    {
        return Err(WorkingMarkdownError::InvalidPath);
    }
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(WorkingMarkdownError::InvalidPath);
    }
    path.to_str()
        .map(normalize_separator)
        .ok_or(WorkingMarkdownError::InvalidPath)
}

fn is_markdown_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn is_excluded_directory(name: &str) -> bool {
    EXCLUDED_DIRECTORIES
        .iter()
        .any(|excluded| name.eq_ignore_ascii_case(excluded))
}

fn node_rank(kind: &WorkingMarkdownNodeKind) -> u8 {
    match kind {
        WorkingMarkdownNodeKind::Directory => 0,
        WorkingMarkdownNodeKind::File => 1,
    }
}

fn normalize_separator(path: &str) -> String {
    path.replace('\\', "/")
}

fn file_error<T>(_error: T) -> WorkingMarkdownError {
    WorkingMarkdownError::Io
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ganbaru-working-markdown-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn scan_prunes_empty_and_excluded_directories() {
        let root = test_root("pruning");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("docs/guides")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::create_dir_all(root.join("node_modules/package")).unwrap();
        fs::write(root.join("docs/guides/start.md"), "# Start\n").unwrap();
        fs::write(root.join("docs/ignore.txt"), "ignored").unwrap();
        fs::write(root.join("node_modules/package/readme.md"), "ignored").unwrap();

        let scan = scan_markdown(&root).unwrap();

        assert_eq!(scan.nodes.len(), 1);
        assert_eq!(
            scan.nodes[0].children[0].children[0].relative_path,
            "docs/guides/start.md"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn markdown_paths_are_normalized_and_require_the_markdown_extension() {
        assert_eq!(
            validate_markdown_path("docs/guide.MD"),
            Ok("docs/guide.MD".to_string())
        );
        assert_eq!(
            validate_markdown_path("docs/notes.txt"),
            Err(WorkingMarkdownError::InvalidExtension)
        );
        assert_eq!(
            validate_markdown_path("/etc/notes.md"),
            Err(WorkingMarkdownError::InvalidPath)
        );
        assert_eq!(
            validate_markdown_path("docs/../notes.md"),
            Err(WorkingMarkdownError::InvalidPath)
        );
    }

    #[test]
    fn relative_paths_reject_traversal_and_symlinks() {
        assert_eq!(
            normalize_relative_path("../secret.md"),
            Err(WorkingMarkdownError::InvalidPath)
        );
        assert_eq!(
            normalize_relative_path("docs/\0secret.md"),
            Err(WorkingMarkdownError::InvalidPath)
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let root = test_root("symlink-root");
            let outside = test_root("symlink-outside");
            let _ = fs::remove_dir_all(&root);
            let _ = fs::remove_dir_all(&outside);
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&outside).unwrap();
            fs::write(outside.join("outside.md"), "secret").unwrap();
            symlink(outside.join("outside.md"), root.join("linked.md")).unwrap();
            assert_eq!(
                resolve_markdown_file(&root, "linked.md"),
                Err(WorkingMarkdownError::SymbolicLink)
            );
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
        }
    }
}
