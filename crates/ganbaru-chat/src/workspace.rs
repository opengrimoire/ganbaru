//! Project-owned working folders and device-local binding authorization.

pub mod files;
pub mod mutation;

use super::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId, RepositoryKind, UtcTimestamp,
};
use chrono::{SecondsFormat, Utc};
use ganbaru_working_folders::{ProjectWorkingFolderBindingState, WorkingFolderDeviceScope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};

const MAX_GIT_CONFIG_BYTES: u64 = 1_048_576;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingFolderBindingStatus {
    Unbound,
    Available,
    Missing,
    RepositoryMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkingFolderAuthorizationOperation {
    ProviderStart,
    FileRead,
    FileWrite,
    MentionResolution,
    TerminalStart,
    Git,
    Diff,
    Restore,
}

impl WorkingFolderAuthorizationOperation {
    pub const ALL: [Self; 8] = [
        Self::ProviderStart,
        Self::FileRead,
        Self::FileWrite,
        Self::MentionResolution,
        Self::TerminalStart,
        Self::Git,
        Self::Diff,
        Self::Restore,
    ];

    fn requires_repository_continuity(self) -> bool {
        matches!(self, Self::Git | Self::Diff | Self::Restore)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkingFolder {
    pub id: ProjectWorkingFolderId,
    pub project_id: String,
    pub display_name: String,
    pub kind: WorkingFolderKind,
    pub managed_relative_path: Option<String>,
    pub sort_order: u64,
    pub repository_kind: RepositoryKind,
    pub repository_identity: Option<String>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub archived_at: Option<UtcTimestamp>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectWorkingFolderRequest {
    pub id: ProjectWorkingFolderId,
    pub project_id: String,
    pub display_name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingFolderKind {
    Managed,
    External,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkingFolderRead {
    pub working_folder: ProjectWorkingFolder,
    pub binding_status: WorkingFolderBindingStatus,
    pub canonical_path: Option<String>,
    pub last_verified_at: Option<UtcTimestamp>,
    pub current_branch: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryProbe {
    pub kind: RepositoryKind,
    pub identity: Option<String>,
    pub compatibility_identity: Option<String>,
    pub current_branch: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedWorkingFolder {
    pub working_folder_id: ProjectWorkingFolderId,
    pub canonical_path: PathBuf,
    pub repository_kind: RepositoryKind,
    /// Logical repository identity used by persisted Chat records.
    pub repository_identity: Option<String>,
    /// Filesystem identity of the currently authorized Git common directory.
    pub repository_storage_identity: Option<String>,
}

pub fn prepare_workspace_binding(
    workspace: &ProjectWorkingFolder,
    selected_path: &Path,
) -> ChatResult<(RepositoryProbe, ProjectWorkingFolderBindingState)> {
    let canonical_path = canonical_existing_directory(selected_path)?;
    let probe = probe_repository(&canonical_path)?;
    let binding = ProjectWorkingFolderBindingState {
        canonical_path: path_to_string(&canonical_path)?,
        filesystem_identity: filesystem_identity(&canonical_path, b"working-folder")?,
        repository_kind: probe.kind,
        repository_identity: match workspace.repository_kind {
            RepositoryKind::Git => workspace.repository_identity.clone(),
            RepositoryKind::None => probe.compatibility_identity.clone(),
        },
        repository_storage_identity: probe.identity.clone(),
        last_verified_at: now_timestamp()?,
    };
    Ok((probe, binding))
}

pub fn authorize_workspace(
    workspace: &ProjectWorkingFolder,
    scope: &WorkingFolderDeviceScope,
    operation: WorkingFolderAuthorizationOperation,
) -> ChatResult<AuthorizedWorkingFolder> {
    let binding = scope.bindings.get(&workspace.id).ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "This device has no binding for the project working folder",
            true,
        )
    })?;
    let canonical_path = canonical_existing_directory(Path::new(&binding.canonical_path))?;
    if path_to_string(&canonical_path)? != binding.canonical_path {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "The project working-folder binding is stale and must be located again",
            true,
        ));
    }
    let current_filesystem_identity = filesystem_identity(&canonical_path, b"working-folder")?;
    if binding.filesystem_identity != current_filesystem_identity {
        return Err(folder_identity_mismatch_error());
    }
    let probe = match probe_repository(&canonical_path) {
        Ok(probe) => Some(probe),
        Err(_) if !operation.requires_repository_continuity() => None,
        Err(error) => return Err(error),
    };
    let repository_matches = probe
        .as_ref()
        .is_some_and(|probe| repository_matches_binding(workspace, binding, probe));
    if operation.requires_repository_continuity() && !repository_matches {
        return Err(repository_mismatch_error());
    }
    let repository_kind = probe
        .as_ref()
        .filter(|_| repository_matches)
        .map_or(RepositoryKind::None, |probe| probe.kind);
    let repository_storage_identity = probe
        .filter(|_| repository_matches)
        .and_then(|probe| probe.identity);
    Ok(AuthorizedWorkingFolder {
        working_folder_id: workspace.id.clone(),
        canonical_path,
        repository_kind,
        repository_identity: repository_matches
            .then(|| workspace.repository_identity.clone())
            .flatten(),
        repository_storage_identity,
    })
}

pub fn resolve_workspace_relative_path(
    authorized: &AuthorizedWorkingFolder,
    relative_path: &str,
) -> ChatResult<PathBuf> {
    let relative = Path::new(relative_path);
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(path_validation_error());
    }
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(path_validation_error());
        }
    }
    let resolved = fs::canonicalize(authorized.canonical_path.join(relative)).map_err(|_| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Workspace path does not exist",
            true,
        )
    })?;
    if !resolved.starts_with(&authorized.canonical_path) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace path resolves outside the bound folder",
            false,
        ));
    }
    Ok(resolved)
}

pub fn workspace_read(
    workspace: ProjectWorkingFolder,
    scope: &WorkingFolderDeviceScope,
) -> ChatResult<ProjectWorkingFolderRead> {
    let binding = scope.bindings.get(&workspace.id);
    let (binding_status, current_branch) = match binding {
        None => (WorkingFolderBindingStatus::Unbound, None),
        Some(binding) => match canonical_existing_directory(Path::new(&binding.canonical_path)) {
            Err(_) => (WorkingFolderBindingStatus::Missing, None),
            Ok(path) => {
                let path_matches =
                    path_to_string(&path).ok().as_deref() == Some(binding.canonical_path.as_str());
                let filesystem_matches = filesystem_identity(&path, b"working-folder")
                    .is_ok_and(|current| current == binding.filesystem_identity);
                if path_matches && filesystem_matches {
                    let branch = probe_repository(&path).ok().and_then(|probe| {
                        repository_matches_binding(&workspace, binding, &probe)
                            .then_some(probe.current_branch)
                            .flatten()
                    });
                    (WorkingFolderBindingStatus::Available, branch)
                } else {
                    (WorkingFolderBindingStatus::RepositoryMismatch, None)
                }
            }
        },
    };
    Ok(ProjectWorkingFolderRead {
        working_folder: workspace,
        binding_status,
        canonical_path: binding.map(|value| value.canonical_path.clone()),
        last_verified_at: binding.map(|value| value.last_verified_at.clone()),
        current_branch,
    })
}

pub fn repository_matches_binding(
    workspace: &ProjectWorkingFolder,
    binding: &ProjectWorkingFolderBindingState,
    probe: &RepositoryProbe,
) -> bool {
    if binding.repository_kind != probe.kind || workspace.repository_kind != probe.kind {
        return false;
    }
    match probe.kind {
        RepositoryKind::None => {
            binding.repository_identity.is_none() && workspace.repository_identity.is_none()
        }
        RepositoryKind::Git => {
            if binding.repository_identity != workspace.repository_identity
                || workspace.repository_identity.is_none()
            {
                return false;
            }
            binding.repository_storage_identity.as_deref() == probe.identity.as_deref()
                && binding.repository_storage_identity.is_some()
        }
    }
}

pub fn initialized_repository_identity(
    workspace: &ProjectWorkingFolder,
    binding: &ProjectWorkingFolderBindingState,
    probe: &RepositoryProbe,
) -> Option<String> {
    if workspace.repository_kind != RepositoryKind::None
        || workspace.repository_identity.is_some()
        || probe.kind != RepositoryKind::Git
    {
        return None;
    }
    let transition_is_unrecorded = binding.repository_kind == RepositoryKind::None;
    let transition_was_partially_recorded = binding.repository_kind == RepositoryKind::Git
        && binding.repository_storage_identity == probe.identity;
    if !transition_is_unrecorded && !transition_was_partially_recorded {
        return None;
    }
    binding
        .repository_identity
        .clone()
        .or_else(|| probe.compatibility_identity.clone())
}

pub fn canonical_existing_directory(path: &Path) -> ChatResult<PathBuf> {
    if !path.is_absolute() {
        return Err(ChatError::validation(
            "workspacePath",
            "Project working-folder path must be absolute",
        ));
    }
    let metadata = fs::metadata(path).map_err(|_| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Project working folder is missing or unreadable",
            true,
        )
    })?;
    if !metadata.is_dir() {
        return Err(ChatError::validation(
            "workspacePath",
            "Project working-folder path must be a directory",
        ));
    }
    fs::canonicalize(path).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Permission,
            "Project working folder could not be canonicalized",
            true,
        )
    })
}

pub fn probe_repository(path: &Path) -> ChatResult<RepositoryProbe> {
    let git_entry = path.join(".git");
    let metadata = match fs::symlink_metadata(&git_entry) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RepositoryProbe {
                kind: RepositoryKind::None,
                identity: None,
                compatibility_identity: None,
                current_branch: None,
            });
        }
        Err(_) => {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Repository metadata is unreadable",
                true,
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Repository metadata cannot be a symbolic link",
            false,
        ));
    }
    let git_directory = if metadata.is_dir() {
        fs::canonicalize(&git_entry).map_err(|_| repository_probe_error())?
    } else if metadata.is_file() {
        resolve_git_directory_file(path, &git_entry)?
    } else {
        return Err(repository_probe_error());
    };
    let common_directory = resolve_git_common_directory(&git_directory)?;
    let config_path = common_directory.join("config");
    let config_metadata = fs::metadata(&config_path).map_err(|_| repository_probe_error())?;
    if !config_metadata.is_file() || config_metadata.len() > MAX_GIT_CONFIG_BYTES {
        return Err(repository_probe_error());
    }
    let config = fs::read_to_string(&config_path).map_err(|_| repository_probe_error())?;
    let current_branch = read_current_branch(&git_directory)?;
    let remote = origin_remote(&config).map(normalize_remote_identity);
    let mut hasher = Sha256::new();
    hasher.update(b"ganbaru-chat-repository-v1\0");
    match remote.filter(|value| !value.is_empty()) {
        Some(remote) => hasher.update(remote.as_bytes()),
        None => {
            hasher.update(b"device-local\0");
            hasher.update(path.as_os_str().as_encoded_bytes());
        }
    }
    Ok(RepositoryProbe {
        kind: RepositoryKind::Git,
        identity: Some(filesystem_identity(&common_directory, b"git-storage")?),
        compatibility_identity: Some(format!("git-sha256:{}", hex_digest(hasher.finalize()))),
        current_branch,
    })
}

fn resolve_git_common_directory(git_directory: &Path) -> ChatResult<PathBuf> {
    let common_file = git_directory.join("commondir");
    let metadata = match fs::symlink_metadata(&common_file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(git_directory.to_path_buf());
        }
        Err(_) => return Err(repository_probe_error()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 4_096 {
        return Err(repository_probe_error());
    }
    let contents = fs::read_to_string(&common_file).map_err(|_| repository_probe_error())?;
    let raw_path = contents.trim();
    if raw_path.is_empty() || raw_path.chars().any(char::is_control) {
        return Err(repository_probe_error());
    }
    let configured = Path::new(raw_path);
    let candidate = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        git_directory.join(configured)
    };
    let common = fs::canonicalize(candidate).map_err(|_| repository_probe_error())?;
    let metadata = fs::metadata(&common).map_err(|_| repository_probe_error())?;
    if !metadata.is_dir() {
        return Err(repository_probe_error());
    }
    Ok(common)
}

fn read_current_branch(git_directory: &Path) -> ChatResult<Option<String>> {
    let head_path = git_directory.join("HEAD");
    let metadata = fs::metadata(&head_path).map_err(|_| repository_probe_error())?;
    if !metadata.is_file() || metadata.len() > 4_096 {
        return Err(repository_probe_error());
    }
    let head = fs::read_to_string(head_path).map_err(|_| repository_probe_error())?;
    let Some(reference) = head.trim().strip_prefix("ref: refs/heads/") else {
        return Ok(None);
    };
    if reference.is_empty() || reference.chars().any(char::is_control) {
        return Err(repository_probe_error());
    }
    Ok(Some(reference.to_string()))
}

fn resolve_git_directory_file(worktree: &Path, git_file: &Path) -> ChatResult<PathBuf> {
    let metadata = fs::metadata(git_file).map_err(|_| repository_probe_error())?;
    if metadata.len() > 4_096 {
        return Err(repository_probe_error());
    }
    let contents = fs::read_to_string(git_file).map_err(|_| repository_probe_error())?;
    let raw_path = contents
        .trim()
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(repository_probe_error)?;
    let path = Path::new(raw_path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        worktree.join(path)
    };
    fs::canonicalize(path).map_err(|_| repository_probe_error())
}

fn origin_remote(config: &str) -> Option<&str> {
    let mut in_origin = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed.eq_ignore_ascii_case("[remote \"origin\"]");
            continue;
        }
        if in_origin {
            let Some((key, value)) = trimmed.split_once('=') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("url") {
                return Some(value.trim());
            }
        }
    }
    None
}

fn normalize_remote_identity(remote: &str) -> String {
    let trimmed = remote.trim();
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, remainder)| remainder)
        .unwrap_or(trimmed);
    let without_user = without_scheme
        .rsplit_once('@')
        .map(|(_, remainder)| remainder)
        .unwrap_or(without_scheme);
    without_user
        .replace(':', "/")
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    use std::fmt::Write;
    bytes
        .as_ref()
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

#[cfg(unix)]
pub fn filesystem_identity(path: &Path, domain: &[u8]) -> ChatResult<String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW);
    let directory = options.open(path).map_err(|_| folder_identity_error())?;
    let metadata = directory.metadata().map_err(|_| folder_identity_error())?;
    Ok(hashed_filesystem_identity(
        domain,
        &metadata.dev().to_le_bytes(),
        &metadata.ino().to_le_bytes(),
    ))
}

#[cfg(windows)]
pub fn filesystem_identity(path: &Path, domain: &[u8]) -> ChatResult<String> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle,
    };

    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0);
    let directory = options.open(path).map_err(|_| folder_identity_error())?;
    let metadata = directory.metadata().map_err(|_| folder_identity_error())?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(folder_identity_error());
    }
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `directory` owns a live directory handle for the call and
    // `information` is an initialized, exclusively borrowed output buffer of
    // the exact type required. The API neither retains the pointer nor closes
    // or otherwise assumes ownership of the handle.
    unsafe { GetFileInformationByHandle(HANDLE(directory.as_raw_handle()), &mut information) }
        .map_err(|_| folder_identity_error())?;
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok(hashed_filesystem_identity(
        domain,
        &information.dwVolumeSerialNumber.to_le_bytes(),
        &file_index.to_le_bytes(),
    ))
}

#[cfg(not(any(unix, windows)))]
pub fn filesystem_identity(path: &Path, domain: &[u8]) -> ChatResult<String> {
    let canonical = fs::canonicalize(path).map_err(|_| folder_identity_error())?;
    Ok(hashed_filesystem_identity(
        domain,
        b"canonical-path",
        canonical.to_string_lossy().as_bytes(),
    ))
}

fn hashed_filesystem_identity(domain: &[u8], first: &[u8], second: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ganbaru-filesystem-identity-v1\0");
    hasher.update(domain);
    hasher.update(b"\0");
    hasher.update(first);
    hasher.update(b"\0");
    hasher.update(second);
    format!("filesystem-sha256:{}", hex_digest(hasher.finalize()))
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

fn path_to_string(path: &Path) -> ChatResult<String> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        ChatError::validation(
            "workspacePath",
            "Project working-folder path contains unsupported characters",
        )
    })
}

fn repository_mismatch_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "The selected folder belongs to a different repository. Rebind the project working folder or add it separately.",
        true,
    )
}

fn folder_identity_mismatch_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "The project working folder was replaced. Locate or recreate it before continuing.",
        true,
    )
}

fn folder_identity_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Project working-folder identity could not be verified",
        true,
    )
}

fn repository_probe_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "Git repository metadata is invalid or unreadable",
        true,
    )
}

fn path_validation_error() -> ChatError {
    ChatError::validation(
        "relativePath",
        "Workspace paths must be normalized relative paths",
    )
}

pub fn open_authorized_workspace(authorized: &AuthorizedWorkingFolder) -> ChatResult<()> {
    open_authorized_path(authorized, &authorized.canonical_path)
}

pub fn open_authorized_path(authorized: &AuthorizedWorkingFolder, path: &Path) -> ChatResult<()> {
    if !path.starts_with(&authorized.canonical_path) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Workspace path resolves outside the bound folder",
            false,
        ));
    }
    spawn_file_manager(path).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "The project working-folder path could not be opened",
            true,
        )
    })
}

#[cfg(target_os = "linux")]
fn spawn_file_manager(path: &Path) -> std::io::Result<()> {
    spawn_file_manager_command("xdg-open", [path.as_os_str()])
}

#[cfg(target_os = "macos")]
fn spawn_file_manager(path: &Path) -> std::io::Result<()> {
    spawn_file_manager_command("open", [path.as_os_str()])
}

#[cfg(windows)]
fn spawn_file_manager(path: &Path) -> std::io::Result<()> {
    spawn_file_manager_command("explorer.exe", [path.as_os_str()])
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn spawn_file_manager(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "platform is unsupported",
    ))
}

fn spawn_file_manager_command<I, S>(program: &str, arguments: I) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}
