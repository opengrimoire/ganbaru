//! Desktop authority for Music file paths that arrive from the webview.
//!
//! A path is usable only when a folder or file the user granted on this device covers it:
//! a native picker result, a natively confirmed typed source, or a root binding of the
//! active vault. Grants persist in device-local app state so restored sessions keep
//! playing after a restart. Coverage is lexical, so symlinks the user placed inside a
//! granted folder keep working, while `..` segments and device namespaces are rejected
//! before any filesystem call. Network shares are reachable only through such a grant.

use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf, Prefix},
};
#[cfg(desktop)]
use tauri::Runtime;

/// Picker and confirmation grants kept on this device before the oldest one is evicted.
#[cfg(desktop)]
pub(crate) const MAX_MUSIC_PATH_GRANTS: usize = 1_024;

/// What a grant or an authorization request covers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MusicPathKind {
    File,
    Folder,
}

/// One path the user chose on this device.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPathGrantRecord {
    pub kind: MusicPathKind,
    pub path: String,
}

/// Device-local Music path grants, oldest first.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MusicPathGrantState(Vec<MusicPathGrantRecord>);

#[cfg(desktop)]
impl MusicPathGrantState {
    /// Records a user-chosen path. Invalid paths are ignored because later checks refuse them.
    fn grant(&mut self, kind: MusicPathKind, path: &Path) {
        let Ok(key) = path_key(path) else {
            return;
        };
        let Some(text) = path.to_str() else {
            return;
        };
        self.0.retain(|record| {
            record.kind != kind || path_key(Path::new(&record.path)).ok().as_ref() != Some(&key)
        });
        self.0.push(MusicPathGrantRecord {
            kind,
            path: text.to_string(),
        });
        let excess = self.0.len().saturating_sub(MAX_MUSIC_PATH_GRANTS);
        self.0.drain(..excess);
    }

    /// Valid grants, newest last, limited to the cap even when the stored file holds more.
    fn keys(&self) -> impl Iterator<Item = (MusicPathKind, PathKey)> + '_ {
        let skip = self.0.len().saturating_sub(MAX_MUSIC_PATH_GRANTS);
        self.0.iter().skip(skip).filter_map(|record| {
            path_key(Path::new(&record.path))
                .ok()
                .map(|key| (record.kind, key))
        })
    }
}

/// Path-free reasons a Music path was refused.
#[cfg(desktop)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MusicPathError {
    /// Relative, `..`, NUL, or non-UTF-8 input.
    Invalid,
    /// A Windows device namespace that is never a library location.
    DeviceNamespace,
    /// No picker grant, confirmation, or root binding covers the path.
    NotGranted,
    /// The path does not exist or cannot be inspected.
    Unavailable,
    /// The path exists but is not the expected file or folder.
    WrongKind,
}

#[cfg(desktop)]
impl MusicPathError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Invalid => "music path must be an absolute local path",
            Self::DeviceNamespace => "music path must not name a device namespace",
            Self::NotGranted => "music path is outside the folders and files chosen on this device",
            Self::Unavailable => "music path is unavailable",
            Self::WrongKind => "music path is not the expected file or folder",
        }
    }
}

#[cfg(desktop)]
impl From<MusicPathError> for String {
    fn from(error: MusicPathError) -> Self {
        error.message().to_string()
    }
}

/// Comparable form of an absolute path. The first part names the root: empty on Unix,
/// `c:` for a Windows drive, `\\server\share` for a network share, or `\\?\volume{id}`
/// for a volume mounted without a drive letter.
#[cfg(desktop)]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PathKey {
    parts: Vec<String>,
}

#[cfg(desktop)]
impl PathKey {
    fn is_within(&self, root: &PathKey) -> bool {
        self.parts.starts_with(&root.parts)
    }
}

#[cfg(windows)]
fn fold(value: &OsStr) -> Option<String> {
    value.to_str().map(str::to_lowercase)
}

#[cfg(all(desktop, not(windows)))]
fn fold(value: &OsStr) -> Option<String> {
    value.to_str().map(str::to_string)
}

/// Accepts the `Volume{GUID}` form that canonicalization yields for drive-letterless
/// volumes; every other verbatim prefix names a device object.
#[cfg(desktop)]
fn volume_root(name: &OsStr) -> Option<String> {
    let name = name.to_str()?.to_ascii_lowercase();
    let id = name.strip_prefix("volume{")?.strip_suffix('}')?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .then(|| format!("\\\\?\\{name}"))
}

/// Builds the comparison key, rejecting every input that is not a plain absolute path.
/// Only drive, network share, and volume prefixes are accepted on Windows.
#[cfg(desktop)]
pub(crate) fn path_key(path: &Path) -> Result<PathKey, MusicPathError> {
    let text = path.to_str().ok_or(MusicPathError::Invalid)?;
    if text.contains('\0') {
        return Err(MusicPathError::Invalid);
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                    parts.push(format!("{}:", char::from(drive).to_ascii_lowercase()));
                }
                Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                    let (Some(server), Some(share)) = (fold(server), fold(share)) else {
                        return Err(MusicPathError::Invalid);
                    };
                    if server.is_empty() || share.is_empty() {
                        return Err(MusicPathError::Invalid);
                    }
                    parts.push(format!("\\\\{server}\\{share}"));
                }
                Prefix::Verbatim(name) => {
                    parts.push(volume_root(name).ok_or(MusicPathError::DeviceNamespace)?);
                }
                Prefix::DeviceNS(_) => return Err(MusicPathError::DeviceNamespace),
            },
            Component::RootDir => {
                if parts.is_empty() {
                    parts.push(String::new());
                }
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(MusicPathError::Invalid),
            Component::Normal(name) => {
                let name = fold(name).ok_or(MusicPathError::Invalid)?;
                if name == "." || name == ".." {
                    return Err(MusicPathError::Invalid);
                }
                parts.push(name);
            }
        }
    }
    if !path.is_absolute() || parts.is_empty() {
        return Err(MusicPathError::Invalid);
    }
    Ok(PathKey { parts })
}

/// Reports whether a grant list or a bound root covers the requested path.
#[cfg(desktop)]
fn is_covered(
    key: &PathKey,
    kind: MusicPathKind,
    grants: impl IntoIterator<Item = (MusicPathKind, PathKey)>,
    roots: &[PathKey],
) -> bool {
    grants
        .into_iter()
        .any(|(granted_kind, granted)| match granted_kind {
            MusicPathKind::File => kind == MusicPathKind::File && granted == *key,
            MusicPathKind::Folder => key.is_within(&granted),
        })
        || roots.iter().any(|root| key.is_within(root))
}

/// Records a picker or confirmation result in device-local app state. Callers must not hold
/// an app state update, because this takes the same lock.
#[cfg(desktop)]
pub(crate) fn grant_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
    kind: MusicPathKind,
    path: &Path,
) -> Result<(), String> {
    crate::vault::update_app_state(app, |state| {
        state.music_path_grants.grant(kind, path);
        Ok(())
    })
}

#[cfg(desktop)]
fn bound_roots(state: &crate::vault::VaultAppState, vault_id: Option<&str>) -> Vec<PathKey> {
    vault_id
        .and_then(|vault_id| state.music_root_bindings.get(vault_id))
        .into_iter()
        .flat_map(|bindings| bindings.values())
        .filter_map(|folder| path_key(Path::new(folder)).ok())
        .collect()
}

/// Reports whether this device granted the path, without touching the filesystem.
#[cfg(desktop)]
pub(crate) fn is_granted<R: Runtime>(
    app: &tauri::AppHandle<R>,
    key: &PathKey,
    kind: MusicPathKind,
) -> bool {
    let Ok(state) = crate::vault::read_app_state(app) else {
        return false;
    };
    if is_covered(key, kind, state.music_path_grants.keys(), &[]) {
        return true;
    }
    // Resolving the active vault reads its marker, so it runs only when no grant matched.
    let vault_id = crate::vault::active_vault_id(app).ok();
    is_covered(key, kind, [], &bound_roots(&state, vault_id.as_deref()))
}

/// Checks the file type with `stat`, which never opens the path and so never blocks on a
/// FIFO or device node.
#[cfg(desktop)]
pub(crate) fn require_kind(path: &Path, kind: MusicPathKind) -> Result<(), MusicPathError> {
    let metadata = fs::metadata(path).map_err(|_| MusicPathError::Unavailable)?;
    let matches = match kind {
        MusicPathKind::File => metadata.is_file(),
        MusicPathKind::Folder => metadata.is_dir(),
    };
    if matches {
        Ok(())
    } else {
        Err(MusicPathError::WrongKind)
    }
}

/// Resolves a webview path that a grant or root binding on this device covers.
#[cfg(desktop)]
pub(crate) fn authorize<R: Runtime>(
    app: &tauri::AppHandle<R>,
    path: &str,
    kind: MusicPathKind,
) -> Result<PathBuf, MusicPathError> {
    let path = PathBuf::from(path);
    let key = path_key(&path)?;
    if !is_granted(app, &key, kind) {
        return Err(MusicPathError::NotGranted);
    }
    require_kind(&path, kind)?;
    Ok(path)
}

/// Most paths one confirmation request may carry.
#[cfg(desktop)]
pub(crate) const MAX_MUSIC_CONFIRM_PATHS: usize = 64;
/// Most paths listed in one native confirmation before summarizing the rest.
#[cfg(desktop)]
const MAX_MUSIC_CONFIRM_DIALOG_ITEMS: usize = 12;

/// Localized text for the native local source confirmation.
// Android accepts the prompt to keep one command signature but never shows it.
#[cfg_attr(mobile, allow(dead_code))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLocalSourcePrompt {
    pub title: String,
    pub message: String,
    pub allow_label: String,
    pub cancel_label: String,
    /// Suffix for the count of paths not listed, with `{count}` replaced natively.
    pub more_label: String,
}

/// Returns the distinct paths no grant covers, rejecting malformed input before any
/// filesystem call so a share is never contacted before the user agrees.
#[cfg(desktop)]
fn ungranted_paths<R: Runtime>(
    app: &tauri::AppHandle<R>,
    paths: &[String],
) -> Result<Vec<PathBuf>, MusicPathError> {
    let mut seen = std::collections::HashSet::new();
    let mut pending = Vec::new();
    for path in paths {
        let path = PathBuf::from(path);
        let key = path_key(&path)?;
        if seen.insert(key.clone()) && !is_granted(app, &key, MusicPathKind::File) {
            pending.push(path);
        }
    }
    Ok(pending)
}

#[cfg(desktop)]
fn confirm_dialog_message(prompt: &MusicLocalSourcePrompt, paths: &[PathBuf]) -> String {
    let mut lines = vec![prompt.message.trim().to_string(), String::new()];
    lines.extend(
        paths
            .iter()
            .take(MAX_MUSIC_CONFIRM_DIALOG_ITEMS)
            .map(|path| path.display().to_string()),
    );
    let hidden = paths.len().saturating_sub(MAX_MUSIC_CONFIRM_DIALOG_ITEMS);
    if hidden > 0 {
        lines.push(
            prompt
                .more_label
                .trim()
                .replace("{count}", &hidden.to_string()),
        );
    }
    lines.join("\n")
}

#[cfg(desktop)]
async fn ask_confirmation(
    app: &tauri::AppHandle,
    prompt: &MusicLocalSourcePrompt,
    paths: &[PathBuf],
) -> Result<bool, String> {
    use tauri_plugin_dialog::{
        DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
    };
    let allow_label = prompt.allow_label.trim().to_string();
    let (sender, mut receiver) = tauri::async_runtime::channel(1);
    app.dialog()
        .message(confirm_dialog_message(prompt, paths))
        .title(prompt.title.trim())
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            allow_label.clone(),
            prompt.cancel_label.trim().to_string(),
        ))
        .show_with_result(move |result| {
            let allowed = match result {
                MessageDialogResult::Ok | MessageDialogResult::Yes => true,
                MessageDialogResult::Custom(label) => label == allow_label,
                _ => false,
            };
            let _ = sender.try_send(allowed);
        });
    receiver
        .recv()
        .await
        .ok_or_else(|| "local source confirmation closed without a result".to_string())
}

/// Grants typed or imported local sources after the user confirms them natively. Paths a
/// picker or root binding already covers pass without a dialog.
#[cfg(desktop)]
#[tauri::command]
pub async fn music_confirm_local_sources(
    app: tauri::AppHandle,
    paths: Vec<String>,
    prompt: MusicLocalSourcePrompt,
) -> Result<bool, String> {
    if paths.len() > MAX_MUSIC_CONFIRM_PATHS {
        return Err(format!(
            "local source confirmation exceeds the {MAX_MUSIC_CONFIRM_PATHS} path limit"
        ));
    }
    let lookup_app = app.clone();
    let pending =
        tauri::async_runtime::spawn_blocking(move || ungranted_paths(&lookup_app, &paths))
            .await
            .map_err(|error| format!("local source lookup failed: {error}"))??;
    if pending.is_empty() {
        return Ok(true);
    }
    if !ask_confirmation(&app, &prompt, &pending).await? {
        return Ok(false);
    }
    tauri::async_runtime::spawn_blocking(move || {
        for path in &pending {
            require_kind(path, MusicPathKind::File)?;
        }
        for path in &pending {
            grant_path(&app, MusicPathKind::File, path)?;
        }
        Ok(true)
    })
    .await
    .map_err(|error| format!("local source grant failed: {error}"))?
}

/// Android sources are document URIs whose access the system picker already scoped.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_confirm_local_sources(
    paths: Vec<String>,
    prompt: MusicLocalSourcePrompt,
) -> Result<bool, String> {
    let _ = (paths, prompt);
    Ok(true)
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    fn key(path: &str) -> PathKey {
        path_key(Path::new(path)).unwrap()
    }

    fn grants(records: &[(MusicPathKind, &str)]) -> MusicPathGrantState {
        let mut state = MusicPathGrantState::default();
        for (kind, path) in records {
            state.grant(*kind, Path::new(path));
        }
        state
    }

    #[cfg(unix)]
    #[test]
    fn unix_keys_reject_relative_parent_and_nul_inputs() {
        assert_eq!(key("/home/a/./Music").parts, ["", "home", "a", "Music"]);
        for rejected in ["Music/a.mp3", "/home/a/../b", "/home/a\0b", ""] {
            assert_eq!(
                path_key(Path::new(rejected)),
                Err(MusicPathError::Invalid),
                "{rejected:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn folder_grants_cover_descendants_and_file_grants_cover_one_file() {
        let state = grants(&[
            (MusicPathKind::Folder, "/music/library"),
            (MusicPathKind::File, "/downloads/loop.ogg"),
        ]);
        let covered = |path: &str, kind| is_covered(&key(path), kind, state.keys(), &[]);
        assert!(covered("/music/library/a/b.mp3", MusicPathKind::File));
        assert!(covered("/music/library", MusicPathKind::Folder));
        assert!(covered("/downloads/loop.ogg", MusicPathKind::File));
        assert!(!covered("/music/library-other/b.mp3", MusicPathKind::File));
        assert!(!covered("/downloads/loop.ogg", MusicPathKind::Folder));
        assert!(!covered("/downloads/other.ogg", MusicPathKind::File));
        assert!(!covered("/music", MusicPathKind::Folder));
    }

    #[cfg(unix)]
    #[test]
    fn bound_roots_of_the_active_vault_cover_paths_without_any_grant() {
        let mut state = crate::vault::VaultAppState::default();
        state.music_root_bindings.insert(
            "vault-1".to_string(),
            [("root".to_string(), "/mnt/nas/Music".to_string())].into(),
        );
        let file = key("/mnt/nas/Music/a.flac");
        let roots = bound_roots(&state, Some("vault-1"));
        assert!(is_covered(&file, MusicPathKind::File, [], &roots));
        assert!(!is_covered(
            &key("/mnt/nas/Pictures/a.png"),
            MusicPathKind::File,
            [],
            &roots
        ));
        let other_vault = bound_roots(&state, Some("vault-2"));
        assert!(!is_covered(&file, MusicPathKind::File, [], &other_vault));
        assert!(bound_roots(&state, None).is_empty());
    }

    #[test]
    fn grants_survive_a_round_trip_and_evict_the_oldest_entry_at_the_limit() {
        let root = std::env::temp_dir();
        let mut state = MusicPathGrantState::default();
        for index in 0..=MAX_MUSIC_PATH_GRANTS {
            state.grant(MusicPathKind::File, &root.join(format!("{index}.mp3")));
        }
        state.grant(MusicPathKind::File, &root.join("5.mp3"));
        assert_eq!(state.0.len(), MAX_MUSIC_PATH_GRANTS);

        let json = serde_json::to_string(&state).unwrap();
        let restored: MusicPathGrantState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, state);
        let first = path_key(&root.join("0.mp3")).unwrap();
        let last = path_key(&root.join(format!("{MAX_MUSIC_PATH_GRANTS}.mp3"))).unwrap();
        let regranted = path_key(&root.join("5.mp3")).unwrap();
        assert!(!is_covered(
            &first,
            MusicPathKind::File,
            restored.keys(),
            &[]
        ));
        assert!(is_covered(&last, MusicPathKind::File, restored.keys(), &[]));
        assert!(is_covered(
            &regranted,
            MusicPathKind::File,
            restored.keys(),
            &[]
        ));
    }

    #[test]
    fn app_state_without_grants_still_parses() {
        let mut value = serde_json::to_value(crate::vault::VaultAppState::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("musicPathGrants")
            .unwrap();
        let state: crate::vault::VaultAppState = serde_json::from_value(value).unwrap();
        assert_eq!(state.music_path_grants, MusicPathGrantState::default());
    }

    #[test]
    fn confirmation_lists_a_bounded_number_of_paths_and_counts_the_rest() {
        let prompt = MusicLocalSourcePrompt {
            title: "Play local files?".into(),
            message: " These files were typed or imported. ".into(),
            allow_label: "Play".into(),
            cancel_label: "Cancel".into(),
            more_label: "and {count} more".into(),
        };
        let paths: Vec<PathBuf> = (0..MAX_MUSIC_CONFIRM_DIALOG_ITEMS + 3)
            .map(|index| PathBuf::from(format!("/music/{index}.mp3")))
            .collect();
        let message = confirm_dialog_message(&prompt, &paths);
        let lines: Vec<&str> = message.lines().collect();
        assert_eq!(lines[0], "These files were typed or imported.");
        assert_eq!(lines.len(), 2 + MAX_MUSIC_CONFIRM_DIALOG_ITEMS + 1);
        assert_eq!(lines.last(), Some(&"and 3 more"));
        assert!(!message.contains(&format!("{}.mp3", MAX_MUSIC_CONFIRM_DIALOG_ITEMS)));
    }

    #[cfg(unix)]
    #[test]
    fn kind_checks_reject_fifos_and_folders_without_opening_them() {
        let root = std::env::temp_dir().join(format!("music-kind-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let fifo = root.join("pipe.mp3");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            require_kind(&fifo, MusicPathKind::File),
            Err(MusicPathError::WrongKind)
        );
        assert_eq!(
            require_kind(&root, MusicPathKind::File),
            Err(MusicPathError::WrongKind)
        );
        assert_eq!(require_kind(&root, MusicPathKind::Folder), Ok(()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_keys_accept_drives_shares_and_volumes_and_reject_device_namespaces() {
        assert_eq!(key(r"C:\Music\a.mp3"), key(r"\\?\c:\music\A.MP3"));
        assert_eq!(key(r"\\host\share\a.mp3"), key(r"\\?\UNC\HOST\share\a.mp3"));
        assert_eq!(
            key(r"\\?\Volume{0a1b2c3d-0000-1111-2222-333344445555}\Music").parts[0],
            r"\\?\volume{0a1b2c3d-0000-1111-2222-333344445555}"
        );
        for rejected in [
            r"\\.\pipe\x",
            r"\\?\GLOBALROOT\Device\Mup\host\a.mp3",
            r"\\?\Volume{x}\a.mp3",
        ] {
            assert_eq!(
                path_key(Path::new(rejected)),
                Err(MusicPathError::DeviceNamespace),
                "{rejected}"
            );
        }
        assert_eq!(
            path_key(Path::new(r"C:Music\a.mp3")),
            Err(MusicPathError::Invalid)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_shares_need_a_grant_and_a_granted_share_covers_its_files() {
        let share = key(r"\\host\share\a.mp3");
        assert!(!is_covered(&share, MusicPathKind::File, [], &[]));
        let state = grants(&[(MusicPathKind::Folder, r"\\host\share\Music")]);
        assert!(is_covered(
            &key(r"\\host\share\Music\a.mp3"),
            MusicPathKind::File,
            state.keys(),
            &[]
        ));
        assert!(!is_covered(&share, MusicPathKind::File, state.keys(), &[]));
    }
}
