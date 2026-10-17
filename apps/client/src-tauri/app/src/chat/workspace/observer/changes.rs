use super::git::is_git_metadata_path;
use super::*;

pub(super) struct WorkspaceChangeAccumulator {
    scope: ObserverScope,
    generation: u64,
    git_roots: Vec<PathBuf>,
    paths: BTreeSet<String>,
    parent_directories: BTreeSet<String>,
    renames: BTreeSet<ChatWorkspaceRename>,
    git_metadata_changed: bool,
    pub(super) overflowed: bool,
    pub(super) degraded_reason: Option<String>,
}

impl WorkspaceChangeAccumulator {
    pub(super) fn new(scope: ObserverScope, generation: u64, git_roots: Vec<PathBuf>) -> Self {
        Self {
            scope,
            generation,
            git_roots,
            paths: BTreeSet::new(),
            parent_directories: BTreeSet::new(),
            renames: BTreeSet::new(),
            git_metadata_changed: false,
            overflowed: false,
            degraded_reason: None,
        }
    }

    pub(super) fn push(&mut self, message: ObserverMessage) {
        match message {
            ObserverMessage::Event(event) => self.push_event(event),
            ObserverMessage::Error(reason) => {
                self.overflowed = true;
                if reason != "watch_error" {
                    self.degraded_reason = Some(reason);
                }
            }
            ObserverMessage::Internal {
                relative_paths,
                git_metadata_changed,
            } => {
                self.git_metadata_changed |= git_metadata_changed;
                for path in relative_paths {
                    self.push_relative_path(path);
                }
            }
            ObserverMessage::Stop => {}
        }
    }

    pub(super) fn push_event(&mut self, event: Event) {
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if matches!(event.kind, EventKind::Other) {
            self.overflowed = true;
        }
        if event.paths.iter().any(|path| path == &self.scope.root) {
            self.overflowed = true;
        }
        let relative_paths = event
            .paths
            .iter()
            .filter_map(|path| self.classify_path(path))
            .collect::<Vec<_>>();
        if relative_paths.len() == 2 && matches!(event.kind, EventKind::Modify(ModifyKind::Name(_)))
        {
            let rename = ChatWorkspaceRename {
                previous_relative_path: relative_paths[0].clone(),
                relative_path: relative_paths[1].clone(),
            };
            if self.renames.len() >= MAX_BATCH_PATHS && !self.renames.contains(&rename) {
                self.overflowed = true;
            } else {
                self.renames.insert(rename);
            }
        }
        for path in relative_paths {
            self.push_relative_path(path);
        }
    }

    pub(super) fn classify_path(&mut self, path: &Path) -> Option<String> {
        if is_git_metadata_path(path, &self.scope.root, &self.git_roots) {
            self.git_metadata_changed = true;
            return None;
        }
        safe_workspace_relative_path(&self.scope.root, path)
    }

    pub(super) fn push_relative_path(&mut self, path: String) {
        if self.paths.len() >= MAX_BATCH_PATHS && !self.paths.contains(&path) {
            self.overflowed = true;
            return;
        }
        self.parent_directories
            .insert(relative_parent_directory(&path));
        self.paths.insert(path);
    }

    pub(super) fn finish(self) -> Option<ChatWorkspaceChangeBatch> {
        if self.paths.is_empty()
            && !self.git_metadata_changed
            && !self.overflowed
            && self.degraded_reason.is_none()
        {
            return None;
        }
        Some(ChatWorkspaceChangeBatch {
            working_folder_id: self.scope.working_folder_id,
            execution_environment_id: self.scope.execution_environment_id,
            generation: self.generation,
            relative_paths: self.paths.into_iter().collect(),
            affected_parent_directories: self.parent_directories.into_iter().collect(),
            renames: self.renames.into_iter().collect(),
            git_metadata_changed: self.git_metadata_changed,
            overflowed: self.overflowed,
            degraded_reason: self.degraded_reason,
        })
    }
}

pub(super) fn safe_workspace_relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let value = relative.to_str()?.replace('\\', "/");
    if value.len() > 4_096
        || value.chars().any(char::is_control)
        || crate::chat::workspace::files::observer_excluded(&value)
        || !nearest_existing_ancestor_is_safe(root, path)
    {
        return None;
    }
    Some(value)
}

fn nearest_existing_ancestor_is_safe(root: &Path, path: &Path) -> bool {
    let mut candidate = path;
    loop {
        match fs::symlink_metadata(candidate) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return false;
                }
                return candidate
                    .canonicalize()
                    .is_ok_and(|canonical| canonical.starts_with(root));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = candidate.parent() else {
                    return false;
                };
                candidate = parent;
            }
            Err(_) => return false,
        }
    }
}

fn relative_parent_directory(path: &str) -> String {
    path.rsplit_once('/')
        .map_or_else(String::new, |(parent, _)| parent.to_string())
}
