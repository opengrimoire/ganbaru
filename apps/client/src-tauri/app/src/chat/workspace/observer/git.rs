use super::*;

pub(super) fn discover_git_metadata_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    for arguments in [
        ["rev-parse", "--absolute-git-dir"].as_slice(),
        ["rev-parse", "--path-format=absolute", "--git-common-dir"].as_slice(),
    ] {
        let Ok(output) = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(arguments)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
        else {
            continue;
        };
        if !output.status.success() || output.stdout.len() > 4_096 {
            continue;
        }
        let Some(value) = std::str::from_utf8(&output.stdout)
            .ok()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let candidate = PathBuf::from(value);
        if let Ok(canonical) = candidate.canonicalize() {
            roots.insert(canonical);
        }
    }
    roots.into_iter().collect()
}

pub(super) fn git_watch_paths(git_roots: &[PathBuf]) -> Vec<(PathBuf, RecursiveMode)> {
    let mut paths = BTreeSet::new();
    for root in git_roots {
        paths.insert((root.clone(), false));
        for recursive in [root.join("refs"), root.join("worktrees")] {
            if recursive.is_dir() {
                paths.insert((recursive, true));
            }
        }
    }
    paths
        .into_iter()
        .map(|(path, recursive)| {
            (
                path,
                if recursive {
                    RecursiveMode::Recursive
                } else {
                    RecursiveMode::NonRecursive
                },
            )
        })
        .collect()
}

pub(super) fn is_git_metadata_path(path: &Path, root: &Path, git_roots: &[PathBuf]) -> bool {
    if let Ok(relative) = path.strip_prefix(root) {
        let value = relative.to_string_lossy().replace('\\', "/");
        if value == ".git" {
            return true;
        }
        if let Some(git_relative) = value.strip_prefix(".git/") {
            return git_metadata_relative_is_relevant(git_relative);
        }
    }
    git_roots.iter().any(|git_root| {
        path.strip_prefix(git_root).is_ok_and(|relative| {
            let value = relative.to_string_lossy().replace('\\', "/");
            git_metadata_relative_is_relevant(&value)
        })
    })
}

pub(super) fn git_metadata_relative_is_relevant(value: &str) -> bool {
    value.is_empty()
        || matches!(
            value,
            "HEAD" | "index" | "packed-refs" | "config" | "info/exclude"
        )
        || value.starts_with("refs/")
        || value.starts_with("worktrees/")
            && matches!(
                value.rsplit('/').next(),
                Some("HEAD" | "index" | "commondir" | "gitdir")
            )
}
