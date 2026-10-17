use super::super::workspace::{
    ProjectWorkingFolder, WorkingFolderAuthorizationOperation, WorkingFolderBindingStatus,
    WorkingFolderKind, authorize_workspace, filesystem_identity, initialized_repository_identity,
    prepare_workspace_binding, probe_repository, resolve_workspace_relative_path, workspace_read,
};
use crate::projects::working_folders::{
    ProjectWorkingFolderBindingState, WorkingFolderDeviceScope,
};
use ganbaru_chat_contracts::models::{ProjectWorkingFolderId, RepositoryKind, UtcTimestamp};
use std::fs;
use std::path::{Path, PathBuf};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let unique = format!(
            "ganbaru-chat-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test clock should follow the Unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("test directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn git_repository(label: &str, remote: &str) -> Self {
        let directory = Self::new(label);
        fs::create_dir(directory.path().join(".git"))
            .expect("Git metadata directory should be created");
        fs::write(
            directory.path().join(".git/config"),
            format!(
                "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = {remote}\n"
            ),
        )
        .expect("Git config should be written");
        fs::write(
            directory.path().join(".git/HEAD"),
            "ref: refs/heads/feat/chat\n",
        )
        .expect("Git HEAD should be written");
        directory
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn timestamp() -> UtcTimestamp {
    UtcTimestamp::new("2026-07-20T12:00:00Z").expect("timestamp should be valid")
}

fn workspace(id: &str, kind: RepositoryKind, identity: Option<String>) -> ProjectWorkingFolder {
    ProjectWorkingFolder {
        id: ProjectWorkingFolderId::new(id).expect("workspace ID should be valid"),
        project_id: "project-1".to_string(),
        display_name: "Frontend".to_string(),
        kind: WorkingFolderKind::External,
        managed_relative_path: None,
        sort_order: 10,
        repository_kind: kind,
        repository_identity: identity,
        created_at: timestamp(),
        updated_at: timestamp(),
        archived_at: None,
        revision: 1,
    }
}

fn binding(
    path: &Path,
    kind: RepositoryKind,
    identity: Option<String>,
) -> ProjectWorkingFolderBindingState {
    ProjectWorkingFolderBindingState {
        canonical_path: path
            .to_str()
            .expect("test path should be UTF-8")
            .to_string(),
        filesystem_identity: filesystem_identity(path, b"working-folder").unwrap(),
        repository_kind: kind,
        repository_identity: identity,
        repository_storage_identity: None,
        last_verified_at: timestamp(),
    }
}

#[test]
fn non_git_workspace_is_authorized_for_every_guarded_operation() {
    let directory = TestDirectory::new("non-git");
    let workspace = workspace("workspace-1", RepositoryKind::None, None);
    let mut scope = WorkingFolderDeviceScope::default();
    scope.bindings.insert(
        workspace.id.clone(),
        binding(directory.path(), RepositoryKind::None, None),
    );

    for operation in WorkingFolderAuthorizationOperation::ALL {
        let authorized = authorize_workspace(&workspace, &scope, operation).unwrap();
        assert_eq!(authorized.canonical_path, directory.path());
    }
}

#[test]
fn another_device_without_a_binding_remains_unbound() {
    let read = workspace_read(
        workspace("working-folder-1", RepositoryKind::None, None),
        &WorkingFolderDeviceScope::default(),
    )
    .unwrap();
    assert_eq!(read.binding_status, WorkingFolderBindingStatus::Unbound);
    assert_eq!(read.canonical_path, None);
}

#[test]
fn missing_and_stale_bindings_are_rejected() {
    let workspace = workspace("workspace-1", RepositoryKind::None, None);
    let mut scope = WorkingFolderDeviceScope::default();
    let missing_binding = {
        let directory = TestDirectory::new("missing");
        binding(directory.path(), RepositoryKind::None, None)
    };
    scope.bindings.insert(workspace.id.clone(), missing_binding);
    assert!(
        authorize_workspace(
            &workspace,
            &scope,
            WorkingFolderAuthorizationOperation::ProviderStart
        )
        .is_err()
    );

    let directory = TestDirectory::new("stale");
    let noncanonical = directory.path().join(".");
    scope.bindings.insert(
        workspace.id.clone(),
        binding(&noncanonical, RepositoryKind::None, None),
    );
    assert!(
        authorize_workspace(
            &workspace,
            &scope,
            WorkingFolderAuthorizationOperation::ProviderStart
        )
        .is_err()
    );
}

#[test]
fn replaced_folder_is_rejected_even_when_its_canonical_path_is_unchanged() {
    let original = TestDirectory::new("original-folder");
    let replacement = TestDirectory::new("replacement-folder");
    let workspace = workspace("workspace-1", RepositoryKind::None, None);
    let mut replaced_binding = binding(replacement.path(), RepositoryKind::None, None);
    replaced_binding.filesystem_identity =
        filesystem_identity(original.path(), b"working-folder").unwrap();
    let mut scope = WorkingFolderDeviceScope::default();
    scope
        .bindings
        .insert(workspace.id.clone(), replaced_binding);

    assert!(
        authorize_workspace(
            &workspace,
            &scope,
            WorkingFolderAuthorizationOperation::FileRead,
        )
        .is_err()
    );
}

#[test]
fn repository_storage_identity_ignores_remote_changes_and_redacts_credentials() {
    let first = TestDirectory::git_repository(
        "first-clone",
        "https://alice:secret-token@example.com/Owner/Repository.git",
    );
    let second =
        TestDirectory::git_repository("second-clone", "git@example.com:owner/repository.git");
    let first_probe = probe_repository(first.path()).unwrap();
    let second_probe = probe_repository(second.path()).unwrap();

    assert_eq!(first_probe.kind, RepositoryKind::Git);
    assert_ne!(first_probe.identity, second_probe.identity);
    assert_eq!(
        first_probe.compatibility_identity,
        second_probe.compatibility_identity
    );
    assert_eq!(first_probe.current_branch.as_deref(), Some("feat/chat"));
    let identity = first_probe.compatibility_identity.as_deref().unwrap();
    assert!(!identity.contains("alice"));
    assert!(!identity.contains("secret-token"));
    assert!(!identity.contains("example.com"));

    fs::write(
        first.path().join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = https://example.com/other/repository.git\n",
    )
    .unwrap();
    let changed_remote = probe_repository(first.path()).unwrap();
    assert_eq!(changed_remote.identity, first_probe.identity);
    assert_ne!(
        changed_remote.compatibility_identity,
        first_probe.compatibility_identity
    );
}

#[test]
fn linked_worktree_uses_the_shared_git_storage_identity() {
    let repository =
        TestDirectory::git_repository("worktree-main", "https://example.com/owner/repository.git");
    let linked = TestDirectory::new("worktree-linked");
    let worktree_git_directory = repository.path().join(".git/worktrees/linked");
    fs::create_dir_all(&worktree_git_directory).unwrap();
    fs::write(
        linked.path().join(".git"),
        format!("gitdir: {}\n", worktree_git_directory.display()),
    )
    .unwrap();
    fs::write(worktree_git_directory.join("commondir"), "../..\n").unwrap();
    fs::write(
        worktree_git_directory.join("HEAD"),
        "ref: refs/heads/linked\n",
    )
    .unwrap();

    let repository_probe = probe_repository(repository.path()).unwrap();
    let linked_probe = probe_repository(linked.path()).unwrap();
    assert_eq!(linked_probe.identity, repository_probe.identity);
    assert_eq!(
        linked_probe.compatibility_identity,
        repository_probe.compatibility_identity
    );
    assert_eq!(linked_probe.current_branch.as_deref(), Some("linked"));
}

#[test]
fn repository_replacement_blocks_git_but_not_bound_folder_access() {
    let first = TestDirectory::git_repository("repo-a", "https://example.com/owner/a.git");
    let second = TestDirectory::git_repository("repo-b", "https://example.com/owner/b.git");
    let first_probe = probe_repository(first.path()).unwrap();
    let second_probe = probe_repository(second.path()).unwrap();
    let workspace = workspace(
        "workspace-1",
        RepositoryKind::Git,
        Some("logical-repo".into()),
    );
    let mut replaced_binding = binding(
        second.path(),
        RepositoryKind::Git,
        Some("logical-repo".into()),
    );
    replaced_binding.filesystem_identity =
        filesystem_identity(second.path(), b"working-folder").unwrap();
    replaced_binding.repository_storage_identity = first_probe.identity;
    let mut scope = WorkingFolderDeviceScope::default();
    scope
        .bindings
        .insert(workspace.id.clone(), replaced_binding);

    let error = authorize_workspace(
        &workspace,
        &scope,
        WorkingFolderAuthorizationOperation::Restore,
    )
    .unwrap_err();
    assert!(error.message.contains("different repository"));
    let authorized = authorize_workspace(
        &workspace,
        &scope,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .unwrap();
    assert_eq!(authorized.repository_kind, RepositoryKind::None);
    assert_eq!(second_probe.kind, RepositoryKind::Git);
}

#[test]
fn malformed_git_metadata_does_not_block_bound_folder_access() {
    let directory = TestDirectory::new("malformed-git");
    fs::create_dir(directory.path().join(".git")).unwrap();
    let workspace = workspace("workspace-1", RepositoryKind::None, None);
    let mut current_binding = binding(directory.path(), RepositoryKind::None, None);
    current_binding.filesystem_identity =
        filesystem_identity(directory.path(), b"working-folder").unwrap();
    let mut scope = WorkingFolderDeviceScope::default();
    scope.bindings.insert(workspace.id.clone(), current_binding);

    let authorized = authorize_workspace(
        &workspace,
        &scope,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .unwrap();
    assert_eq!(authorized.repository_kind, RepositoryKind::None);
    assert!(
        authorize_workspace(&workspace, &scope, WorkingFolderAuthorizationOperation::Git,).is_err()
    );
    assert_eq!(
        workspace_read(workspace, &scope).unwrap().binding_status,
        WorkingFolderBindingStatus::Available,
    );
}

#[test]
fn prepared_binding_records_folder_and_git_storage_identity() {
    let repository = TestDirectory::git_repository("initialized", "");
    let workspace = workspace("workspace-1", RepositoryKind::None, None);
    let (probe, prepared_binding) =
        prepare_workspace_binding(&workspace, repository.path()).unwrap();

    assert_eq!(prepared_binding.filesystem_identity.len(), 82);
    assert_eq!(prepared_binding.repository_kind, RepositoryKind::Git);
    assert_eq!(prepared_binding.repository_storage_identity, probe.identity);
    assert_eq!(
        prepared_binding.repository_identity,
        probe.compatibility_identity
    );
    assert_eq!(
        initialized_repository_identity(
            &workspace,
            &binding(repository.path(), RepositoryKind::None, None),
            &probe,
        ),
        probe.compatibility_identity,
    );
}

#[test]
fn remote_configuration_changes_do_not_revoke_git_authorization() {
    let repository =
        TestDirectory::git_repository("remote-change", "https://example.com/owner/original.git");
    let initial_workspace = workspace("workspace-1", RepositoryKind::None, None);
    let (probe, prepared_binding) =
        prepare_workspace_binding(&initial_workspace, repository.path()).unwrap();
    let workspace = workspace(
        "workspace-1",
        RepositoryKind::Git,
        probe.compatibility_identity,
    );
    let mut scope = WorkingFolderDeviceScope::default();
    scope
        .bindings
        .insert(workspace.id.clone(), prepared_binding);

    fs::write(
        repository.path().join(".git/config"),
        "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = ssh://git@example.com/owner/changed.git\n",
    )
    .unwrap();

    let authorized =
        authorize_workspace(&workspace, &scope, WorkingFolderAuthorizationOperation::Git).unwrap();
    assert_eq!(authorized.repository_kind, RepositoryKind::Git);
}

#[test]
fn traversal_and_absolute_paths_are_rejected() {
    let directory = TestDirectory::new("paths");
    let authorized = super::super::workspace::AuthorizedWorkingFolder {
        working_folder_id: ProjectWorkingFolderId::new("workspace-1").unwrap(),
        canonical_path: directory.path().to_path_buf(),
        repository_kind: RepositoryKind::None,
        repository_identity: None,
        repository_storage_identity: None,
    };

    assert!(resolve_workspace_relative_path(&authorized, "../outside.txt").is_err());
    assert!(resolve_workspace_relative_path(&authorized, "/etc/passwd").is_err());
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected_at_resolution_time() {
    use std::os::unix::fs::symlink;

    let workspace_directory = TestDirectory::new("symlink-workspace");
    let outside_directory = TestDirectory::new("symlink-outside");
    fs::write(outside_directory.path().join("secret.txt"), "outside")
        .expect("outside file should be created");
    symlink(
        outside_directory.path(),
        workspace_directory.path().join("escape"),
    )
    .expect("test symlink should be created");
    let authorized = super::super::workspace::AuthorizedWorkingFolder {
        working_folder_id: ProjectWorkingFolderId::new("workspace-1").unwrap(),
        canonical_path: workspace_directory.path().to_path_buf(),
        repository_kind: RepositoryKind::None,
        repository_identity: None,
        repository_storage_identity: None,
    };

    let error = resolve_workspace_relative_path(&authorized, "escape/secret.txt").unwrap_err();
    assert_eq!(
        error.message,
        "Workspace path resolves outside the bound folder"
    );
}
