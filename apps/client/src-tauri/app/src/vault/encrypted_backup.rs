//! Encrypted vault backups: creating a backup file, inspecting a picked one, and restoring it.
//!
//! A backup is the snapshot another installation starts from, encrypted to the vault's recovery
//! kit and, once the vault is linked, to its current key epoch. A restore lands in one of two
//! ways. It replaces the vault in place only when the backup belongs to the active space and
//! neither log enrolled another device, so no linked device holds acknowledgements the restore
//! could rewind. Every other restore is an independent copy: a new vault id and space with no
//! sync state, recovery identity, or person identity, so it never reappears in the space it
//! came from. A desktop opens the copy from a folder the user chooses; Android replaces the
//! active vault and keeps the replaced one restorable from Settings > Data.
//!
//! The picked file never travels back from the webview: inspection keeps it behind a random
//! token, and only that token selects it for the restore.

use super::backup::{
    SkippedEntries, extract_backup_archive, move_into_new_folder, recover_interrupted_restore,
    replace_vault_flagging, replace_with_other_vault, replacement_shrinks_drastically,
    validate_restored_vault,
};
use super::quiescence::{self, RunningTerminals, VaultTransition};
use super::{
    FileAccess, VaultInfo, VaultManifest, database_path, manifest_path, new_vault_id, now_utc,
    read_vault_manifest, vault_info_from_path, write_text_file_atomically,
};
use crate::sync::backup_keys::{self, LinkSummary};
use chrono::SecondsFormat;
use ganbaru_sync_contracts::backup::{BackupError, BackupHeader, FileKey};
use ganbaru_sync_contracts::{EncryptionKeyPair, RecoverySeed, SpaceId, SpaceKey};
use ganbaru_sync_replica::backup::{
    ArchiveGate, BackupFileError, BackupSealKeys, create_encrypted_backup, decrypt_backup,
    read_backup_header,
};
use serde::Serialize;
use sqlx::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Manager, Runtime};

/// File extension of encrypted backups.
pub(crate) const BACKUP_EXTENSION: &str = "ganbaru-backup";
/// Name of the backup file kind in pickers and Android Downloads.
const BACKUP_FILE_KIND: &str = "Ganbaru backup";
/// App cache directory that holds backup staging and picked backups.
const TRANSFER_ROOT: &str = "vault-backups";
/// Prefix of the private directory one inspected backup lives in; the rest is its token.
const RESTORE_TRANSFER_PREFIX: &str = "restore-";
/// Prefix of the private directory one Android backup is written to before it is saved.
#[cfg(target_os = "android")]
const SAVE_TRANSFER_PREFIX: &str = "save-";
/// The decrypted archive inside a restore transfer directory.
const RESTORED_ARCHIVE_FILE: &str = "archive.zip";
/// Folder name that holds a restored vault before it replaces the vault beside it.
const RESTORE_STAGING_NAME: &str = ".ganbaru.restore";
/// Folder name the replaced vault waits in while a restore swaps folders.
const RESTORE_ROLLBACK_NAME: &str = ".ganbaru.rollback";
/// Trigger names that keep the person identity and contact card from being deleted. An
/// independent copy recreates them unchanged after removing both rows.
const IDENTITY_TRIGGERS: [&str; 2] = [
    "contacts_local_identity_immutable_delete",
    "contact_card_prevent_delete",
];

/// Why a backup was not written or restored. The webview maps each reason to a localized
/// message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupRefusal {
    /// The vault has no recovery kit, so no backup could be opened on a new device.
    NoRecoveryKit,
    /// The file is not a Ganbaru backup.
    NotBackup,
    /// The backup was written in a format this build does not read.
    UnsupportedVersion,
    /// The file is incomplete, changed, or damaged.
    Damaged,
    /// The backup opens only with the recovery kit.
    KitRequired,
    /// The recovery kit belongs to another vault.
    WrongKit,
    /// The recovery kit code is mistyped.
    InvalidKit,
    /// This Android device is linked, so a restore would replace a vault other devices share.
    LinkedAndroid,
    /// The inspected backup is no longer held; pick it again.
    Expired,
}

impl BackupRefusal {
    /// Whether the picked file stays held, so the user can retry with a kit code.
    fn keeps_file(self) -> bool {
        matches!(self, Self::KitRequired | Self::WrongKit | Self::InvalidKit)
    }
}

/// What writing a backup did.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackupSaveOutcome {
    /// The user closed the save dialog.
    Cancelled,
    /// The backup was written under this file name.
    Saved {
        #[serde(rename = "fileName")]
        file_name: String,
    },
    /// The backup was not written.
    Refused { reason: BackupRefusal },
}

/// What inspecting a picked backup found.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackupInspection {
    /// The user closed the picker.
    Cancelled,
    /// The file cannot be restored here.
    Refused { reason: BackupRefusal },
    /// The file is held behind `token`; `needsKit` tells whether the restore needs the kit.
    Ready {
        token: String,
        #[serde(rename = "needsKit")]
        needs_kit: bool,
    },
}

/// How a restore placed the backup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestorePlacement {
    /// The backup replaced the active vault, which stays restorable from Settings > Data.
    InPlace,
    /// The backup became a new vault with its own id and space.
    IndependentCopy,
}

/// What a restore did.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BackupRestoreOutcome {
    /// The user closed the folder picker.
    Cancelled,
    /// The backup was not restored and the active vault is unchanged.
    Refused { reason: BackupRefusal },
    /// The backup is now the active vault.
    Restored {
        vault: VaultInfo,
        placement: RestorePlacement,
    },
}

/// The platform a restore runs on, which decides where an independent copy goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestorePlatform {
    /// The copy opens from its own folder beside the vault in use.
    Desktop,
    /// The copy replaces the one vault the app keeps.
    Android,
}

#[cfg(desktop)]
const PLATFORM: RestorePlatform = RestorePlatform::Desktop;
#[cfg(mobile)]
const PLATFORM: RestorePlatform = RestorePlatform::Android;

/// Refuses a restore on an Android device whose vault enrolled another device.
fn refuse_linked_android(
    platform: RestorePlatform,
    active: &LinkSummary,
) -> Result<(), BackupRefusal> {
    if platform == RestorePlatform::Android && active.other_devices > 0 {
        return Err(BackupRefusal::LinkedAndroid);
    }
    Ok(())
}

/// Chooses how a backup whose staged vault is summarized by `backup` restores over the active
/// vault `active`, or `None` when nothing is active.
fn plan_restore(
    platform: RestorePlatform,
    active: Option<&LinkSummary>,
    backup: &LinkSummary,
) -> Result<RestorePlacement, BackupRefusal> {
    let Some(active) = active else {
        return Ok(RestorePlacement::IndependentCopy);
    };
    refuse_linked_android(platform, active)?;
    if active.space == backup.space && active.other_devices == 0 && backup.other_devices == 0 {
        Ok(RestorePlacement::InPlace)
    } else {
        Ok(RestorePlacement::IndependentCopy)
    }
}

/// The refusal a format error means, `None` for a local failure such as the random source.
/// `with_kit` tells whether a kit code was offered, so a key that does not open the file is
/// the wrong kit rather than a missing one.
fn format_refusal(error: BackupError, with_kit: bool) -> Option<BackupRefusal> {
    match error {
        BackupError::NotBackup => Some(BackupRefusal::NotBackup),
        BackupError::Version(_) => Some(BackupRefusal::UnsupportedVersion),
        BackupError::Truncated
        | BackupError::Malformed
        | BackupError::TooLarge
        | BackupError::Authentication
        | BackupError::Finished => Some(BackupRefusal::Damaged),
        BackupError::Key | BackupError::Epoch if with_kit => Some(BackupRefusal::WrongKit),
        BackupError::Key | BackupError::Epoch => Some(BackupRefusal::KitRequired),
        BackupError::Random | BackupError::Seal => None,
    }
}

/// A restore that stopped: refused with a reason the user sees, cancelled, or failed.
#[derive(Debug)]
enum RestoreError {
    Refused(BackupRefusal),
    Cancelled,
    Failed(String),
}

impl From<String> for RestoreError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}

impl From<BackupRefusal> for RestoreError {
    fn from(reason: BackupRefusal) -> Self {
        Self::Refused(reason)
    }
}

impl RestoreError {
    fn from_file(error: BackupFileError, with_kit: bool) -> Self {
        match error {
            BackupFileError::Format(format) => match format_refusal(format, with_kit) {
                Some(reason) => Self::Refused(reason),
                None => Self::Failed(format!("open backup: {format}")),
            },
            BackupFileError::Io(message) => Self::Failed(message),
        }
    }
}

/// The key a restore opens the backup's file key with.
enum Unlock {
    /// The active space's key of the backup's epoch.
    Epoch(SpaceKey),
    /// The recovery encryption key of the kit the user entered.
    Kit(EncryptionKeyPair),
}

impl Unlock {
    /// The kit when one was entered, and else the epoch key this installation holds.
    fn choose(kit: Option<String>, epoch_key: Option<SpaceKey>) -> Result<Self, BackupRefusal> {
        if let Some(code) = kit {
            let seed =
                RecoverySeed::parse_owned_kit_code(code).map_err(|_| BackupRefusal::InvalidKit)?;
            return seed
                .recovery_encryption_key()
                .map(Self::Kit)
                .map_err(|_| BackupRefusal::InvalidKit);
        }
        epoch_key.map(Self::Epoch).ok_or(BackupRefusal::KitRequired)
    }

    fn with_kit(&self) -> bool {
        matches!(self, Self::Kit(_))
    }

    fn open(&self, header: &BackupHeader) -> Result<FileKey, BackupError> {
        match self {
            Self::Epoch(key) => header.open_with_epoch_key(key),
            Self::Kit(pair) => header.open_with_recovery(pair),
        }
    }
}

/// A picked backup held between inspection and restore.
struct PendingBackup {
    token: String,
    /// The backup file: the user's file on a desktop, a private copy on Android.
    source: PathBuf,
    /// The private directory removed with everything in it when the backup is let go.
    transfer: PathBuf,
}

static PENDING_BACKUP: Mutex<Option<PendingBackup>> = Mutex::new(None);

fn pending_slot() -> Result<std::sync::MutexGuard<'static, Option<PendingBackup>>, String> {
    PENDING_BACKUP
        .lock()
        .map_err(|_| "the picked backup is unavailable".to_string())
}

/// Removes the private directory of a backup that is no longer held.
fn discard(pending: PendingBackup) {
    if let Err(error) = fs::remove_dir_all(&pending.transfer)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("Backup restore cleanup: {error}");
    }
}

/// Holds `next` as the picked backup, letting go of any earlier one.
fn hold_pending(next: Option<PendingBackup>) -> Result<(), String> {
    let previous = std::mem::replace(&mut *pending_slot()?, next);
    if let Some(previous) = previous {
        discard(previous);
    }
    Ok(())
}

/// Takes the picked backup held behind `token`, leaving any other one held.
fn take_pending(token: &str) -> Result<Option<PendingBackup>, String> {
    let mut slot = pending_slot()?;
    if slot.as_ref().is_some_and(|pending| pending.token == token) {
        Ok(slot.take())
    } else {
        Ok(None)
    }
}

/// The app cache directory backups are staged in.
fn transfer_root<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("find app cache directory: {error}"))?
        .join(TRANSFER_ROOT);
    ganbaru_atomic_files::create_private_directories(&root)
        .map_err(|error| format!("create backup staging: {error}"))?;
    Ok(root)
}

/// Creates the private directory a picked backup is held in, after removing the ones earlier
/// inspections left. Returns the token and the directory.
fn new_restore_transfer<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(String, PathBuf), String> {
    let root = transfer_root(app)?;
    remove_stale_transfers(&root, RESTORE_TRANSFER_PREFIX);
    let directory =
        ganbaru_atomic_files::create_private_temporary_directory(&root, RESTORE_TRANSFER_PREFIX)
            .map_err(|error| format!("create backup staging: {error}"))?;
    let token = directory
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(RESTORE_TRANSFER_PREFIX))
        .ok_or_else(|| "create backup staging: unexpected directory name".to_string())?
        .to_string();
    Ok((token, directory))
}

/// Removes directories under `root` named with `prefix` that an exit left behind. The caller
/// holds the vault transition, so none of them is in use.
fn remove_stale_transfers(root: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(prefix));
        if stale && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Removes a folder a restore staged when the restore stops before using it.
struct StagedFolder(Option<PathBuf>);

impl StagedFolder {
    fn new(path: &Path) -> Self {
        Self(Some(path.to_path_buf()))
    }

    /// Keeps the folder, which the restore moved into place.
    fn keep(mut self) {
        self.0 = None;
    }
}

impl Drop for StagedFolder {
    fn drop(&mut self) {
        if let Some(path) = self.0.take()
            && let Err(error) = fs::remove_dir_all(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("Backup restore cleanup: {error}");
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("backup worker failed: {error}"))
}

/// The active vault as a restore sees it.
struct ActiveVault {
    root: PathBuf,
    summary: LinkSummary,
    /// The active space's key of the backup's epoch, when this installation holds it.
    epoch_key: Option<SpaceKey>,
}

/// Reads the active vault's link summary and, for a backup of its space, the epoch key.
/// `None` when no vault is selected yet, as during first-run setup.
async fn active_vault_for(
    app: &tauri::AppHandle,
    header: &BackupHeader,
) -> Result<Option<ActiveVault>, String> {
    if super::read_app_state(app)?.active_vault_path.is_none() {
        return Ok(None);
    }
    let root = super::active_vault_path(app)?;
    let vault_id = super::active_vault_id(app)?;
    let device_id = super::ensure_device_id(app)?;
    let pool = crate::db::connect_vault(app).await?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("open the vault database: {error}"))?;
    let summary = backup_keys::link_summary(&mut conn, &vault_id, &device_id).await?;
    let epoch_key =
        backup_keys::active_epoch_key(app, &mut conn, &vault_id, &header.space, header.epoch)
            .await?;
    Ok(Some(ActiveVault {
        root,
        summary,
        epoch_key,
    }))
}

/// Refuses a restore on an Android device whose active vault enrolled another device.
fn refuse_linked_active(active: Option<&ActiveVault>) -> Result<(), BackupRefusal> {
    match active {
        Some(active) => refuse_linked_android(PLATFORM, &active.summary),
        None => Ok(()),
    }
}

/// The keys a backup of the active vault is sealed to, `None` without a recovery kit.
async fn active_seal_keys(
    app: &tauri::AppHandle,
    vault_id: &str,
) -> Result<Option<BackupSealKeys>, String> {
    let pool = crate::db::connect_vault(app).await?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("open the vault database: {error}"))?;
    backup_keys::backup_seal_keys(app, &mut conn, vault_id).await
}

/// Writes an encrypted backup of the vault at `vault_root` to `destination` while vault
/// writers are excluded, then resumes them.
async fn write_backup(
    app: &tauri::AppHandle,
    transition: VaultTransition,
    vault_root: &Path,
    keys: &BackupSealKeys,
    destination: &Path,
) -> Result<u64, String> {
    let staging = transfer_root(app)?;
    let quiescence =
        quiescence::begin_reserved_quiescence(app, transition, RunningTerminals::Continue).await?;
    let written = create_encrypted_backup(
        &ArchiveGate::process(),
        vault_root,
        &staging,
        keys,
        destination,
    )
    .await
    .map_err(|error| format!("write encrypted backup: {error}"));
    let resumed = blocking(move || quiescence.finish()).await.and_then(|resumed| resumed);
    match (written, resumed) {
        (Ok(bytes), Ok(())) => Ok(bytes),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(resume)) => Err(format!("{error}; resuming runtimes also failed: {resume}")),
    }
}

/// A file name for a new backup, stamped with the current time.
fn backup_file_name() -> String {
    let timestamp = now_utc()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(':', "-");
    format!("ganbaru-backup-{timestamp}.{BACKUP_EXTENSION}")
}

/// Writes an encrypted backup of the active vault to a file the user chooses.
#[cfg(desktop)]
#[tauri::command]
pub async fn vault_backup_to_file(app: tauri::AppHandle) -> Result<BackupSaveOutcome, String> {
    use super::documents::{existing_downloads_directory, pick_save_path, require_extension};
    let transition = quiescence::reserve_vault_transition()?;
    let vault_root = super::active_vault_path(&app)?;
    let vault_id = super::active_vault_id(&app)?;
    let Some(keys) = active_seal_keys(&app, &vault_id).await? else {
        return Ok(BackupSaveOutcome::Refused {
            reason: BackupRefusal::NoRecoveryKit,
        });
    };
    let default_name = backup_file_name();
    let start_directory = existing_downloads_directory(&app);
    let dialog_app = app.clone();
    let Some(destination) = crate::native_dialogs::on_dialog_thread(move || {
        pick_save_path(
            &dialog_app,
            "Save encrypted backup",
            &default_name,
            BACKUP_FILE_KIND,
            &[BACKUP_EXTENSION],
            start_directory,
        )
    })
    .await??
    else {
        return Ok(BackupSaveOutcome::Cancelled);
    };
    require_extension(&destination, &[BACKUP_EXTENSION], "backup")?;
    let file_name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    write_backup(&app, transition, &vault_root, &keys, &destination).await?;
    Ok(BackupSaveOutcome::Saved { file_name })
}

/// Writes an encrypted backup of the active vault to Android Downloads.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_backup_to_downloads(
    app: tauri::AppHandle,
) -> Result<BackupSaveOutcome, String> {
    use ganbaru_mobile_documents::MobileDocumentsExt;
    let transition = quiescence::reserve_vault_transition()?;
    let vault_root = super::active_vault_path(&app)?;
    let vault_id = super::active_vault_id(&app)?;
    let Some(keys) = active_seal_keys(&app, &vault_id).await? else {
        return Ok(BackupSaveOutcome::Refused {
            reason: BackupRefusal::NoRecoveryKit,
        });
    };
    let root = transfer_root(&app)?;
    remove_stale_transfers(&root, SAVE_TRANSFER_PREFIX);
    let transfer =
        ganbaru_atomic_files::create_private_temporary_directory(&root, SAVE_TRANSFER_PREFIX)
            .map_err(|error| format!("create backup staging: {error}"))?;
    let destination = transfer.join(format!("backup.{BACKUP_EXTENSION}"));
    let result = async {
        write_backup(&app, transition, &vault_root, &keys, &destination).await?;
        let source = super::path_to_string(&destination, "backup file")?;
        let file_name = backup_file_name();
        let save_app = app.clone();
        let saved = blocking(move || {
            save_app.mobile_documents().save_file_download(
                &source,
                &file_name,
                ganbaru_sync_contracts::backup::MAX_BACKUP_FILE_BYTES,
                &[BACKUP_EXTENSION],
                "application/octet-stream",
                BACKUP_FILE_KIND,
            )
        })
        .await??;
        Ok(BackupSaveOutcome::Saved { file_name: saved })
    }
    .await;
    if let Err(error) = fs::remove_dir_all(&transfer) {
        eprintln!("Backup cleanup: {error}");
    }
    result
}

/// Android Downloads exist only on Android; other builds register the command for the mobile
/// handler tests.
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub async fn vault_backup_to_downloads() -> Result<BackupSaveOutcome, String> {
    Err("saving a backup to Downloads is only available on Android".to_string())
}

/// Asks the user for a backup file. A desktop keeps the user's file; Android copies the
/// picked document into `transfer`, reading no more than the largest backup.
#[cfg(desktop)]
async fn pick_backup(app: &tauri::AppHandle, _transfer: &Path) -> Result<Option<PathBuf>, String> {
    use super::documents::{existing_downloads_directory, pick_open_path};
    let start_directory = existing_downloads_directory(app);
    let dialog_app = app.clone();
    crate::native_dialogs::on_dialog_thread(move || {
        pick_open_path(
            &dialog_app,
            "Restore a Ganbaru backup",
            BACKUP_FILE_KIND,
            &[BACKUP_EXTENSION],
            start_directory,
        )
    })
    .await?
}

#[cfg(target_os = "android")]
async fn pick_backup(app: &tauri::AppHandle, transfer: &Path) -> Result<Option<PathBuf>, String> {
    use ganbaru_mobile_documents::MobileDocumentsExt;
    let destination = transfer.join(format!("picked.{BACKUP_EXTENSION}"));
    let destination_text = super::path_to_string(&destination, "picked backup")?;
    let picker_app = app.clone();
    // The picker accepts any document; the magic check below tells a backup apart, since
    // Android document providers do not report a stable type for this extension.
    let picked = blocking(move || {
        picker_app.mobile_documents().pick_document_to_path(
            &destination_text,
            ganbaru_sync_contracts::backup::MAX_BACKUP_FILE_BYTES,
            &[],
            &["*/*"],
            BACKUP_FILE_KIND,
        )
    })
    .await??;
    Ok(picked.map(|_| destination))
}

#[cfg(target_os = "ios")]
async fn pick_backup(_app: &tauri::AppHandle, _transfer: &Path) -> Result<Option<PathBuf>, String> {
    Err("backup restore is not available on this platform".to_string())
}

/// Lets the user pick a backup and reports whether it can be restored here and whether the
/// restore needs the recovery kit. The file stays held behind the returned token.
#[tauri::command]
pub async fn vault_inspect_backup(app: tauri::AppHandle) -> Result<BackupInspection, String> {
    let _transition = quiescence::reserve_vault_transition()?;
    hold_pending(None)?;
    let (token, transfer) = new_restore_transfer(&app)?;
    let pending_transfer = transfer.clone();
    let inspected = async {
        let Some(source) = pick_backup(&app, &transfer).await? else {
            return Ok(BackupInspection::Cancelled);
        };
        let header_source = source.clone();
        let header = match blocking(move || read_backup_header(&header_source)).await? {
            Ok(header) => header,
            Err(error) => {
                return match RestoreError::from_file(error, false) {
                    RestoreError::Refused(reason) => Ok(BackupInspection::Refused { reason }),
                    RestoreError::Cancelled => Ok(BackupInspection::Cancelled),
                    RestoreError::Failed(error) => Err(error),
                };
            }
        };
        let active = active_vault_for(&app, &header).await?;
        if let Err(reason) = refuse_linked_active(active.as_ref()) {
            return Ok(BackupInspection::Refused { reason });
        }
        let needs_kit = active.and_then(|active| active.epoch_key).is_none();
        hold_pending(Some(PendingBackup {
            token: token.clone(),
            source,
            transfer: pending_transfer,
        }))?;
        Ok(BackupInspection::Ready { token, needs_kit })
    }
    .await;
    if !matches!(inspected, Ok(BackupInspection::Ready { .. }))
        && let Err(error) = fs::remove_dir_all(&transfer)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("Backup restore cleanup: {error}");
    }
    inspected
}

/// Restores the backup inspection held behind `token`, opening it with the kit code `kit` or,
/// without one, with the active space's epoch key. A refusal that a kit code can fix keeps the
/// file held for another try.
#[tauri::command]
pub async fn vault_restore_backup(
    app: tauri::AppHandle,
    token: String,
    kit: Option<String>,
) -> Result<BackupRestoreOutcome, String> {
    let transition = quiescence::reserve_vault_transition()?;
    let Some(pending) = take_pending(&token)? else {
        return Ok(BackupRestoreOutcome::Refused {
            reason: BackupRefusal::Expired,
        });
    };
    match restore_pending(&app, transition, &pending, kit).await {
        Ok((vault, placement)) => {
            discard(pending);
            Ok(BackupRestoreOutcome::Restored { vault, placement })
        }
        Err(RestoreError::Refused(reason)) => {
            if reason.keeps_file() {
                hold_pending(Some(pending))?;
            } else {
                discard(pending);
            }
            Ok(BackupRestoreOutcome::Refused { reason })
        }
        Err(RestoreError::Cancelled) => {
            discard(pending);
            Ok(BackupRestoreOutcome::Cancelled)
        }
        Err(RestoreError::Failed(error)) => {
            discard(pending);
            Err(error)
        }
    }
}

/// Decrypts the held backup and restores it as this platform places it.
async fn restore_pending(
    app: &tauri::AppHandle,
    transition: VaultTransition,
    pending: &PendingBackup,
    kit: Option<String>,
) -> Result<(VaultInfo, RestorePlacement), RestoreError> {
    let source = pending.source.clone();
    let header = blocking(move || read_backup_header(&source))
        .await?
        .map_err(|error| RestoreError::from_file(error, false))?;
    let active = active_vault_for(app, &header).await?;
    refuse_linked_active(active.as_ref())?;
    let epoch_key = active.as_ref().and_then(|active| active.epoch_key.clone());
    let unlock = Unlock::choose(kit, epoch_key)?;
    let with_kit = unlock.with_kit();
    let archive = pending.transfer.join(RESTORED_ARCHIVE_FILE);
    let source = pending.source.clone();
    let decrypt_archive = archive.clone();
    let expected = header.clone();
    blocking(move || {
        if let Err(error) = fs::remove_file(&decrypt_archive)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(BackupFileError::Io(format!(
                "remove an earlier restored archive: {error}"
            )));
        }
        decrypt_backup(
            &source,
            |read| {
                // The file is read again here, so it must still be the backup inspected.
                if *read != expected {
                    return Err(BackupError::Malformed);
                }
                unlock.open(read)
            },
            &decrypt_archive,
        )
    })
    .await?
    .map_err(|error| RestoreError::from_file(error, with_kit))?;
    place_restored_backup(app, transition, &archive, &header, active.as_ref()).await
}

/// Extracts the restored archive into the new folder `staging` and checks that it is a whole
/// vault of the backup's space.
async fn stage_restored_vault(
    archive: &Path,
    staging: &Path,
    header: &BackupHeader,
) -> Result<VaultInfo, RestoreError> {
    let extract_archive = archive.to_path_buf();
    let extract_staging = staging.to_path_buf();
    let skipped: SkippedEntries = blocking(move || {
        if extract_staging.exists() {
            fs::remove_dir_all(&extract_staging)
                .map_err(|error| format!("remove stale restore staging: {error}"))?;
        }
        extract_backup_archive(&extract_archive, &extract_staging, None)
    })
    .await??;
    skipped.report("Backup restore", "outside the vault layout");
    let info = validate_restored_vault(staging).await?;
    if SpaceId::personal(&info.vault_id) != header.space {
        return Err(RestoreError::Refused(BackupRefusal::Damaged));
    }
    Ok(info)
}

/// The link summary of the staged vault at `staging`, as this device sees it.
async fn staged_summary(
    app: &tauri::AppHandle,
    staging: &Path,
    vault_id: &str,
) -> Result<LinkSummary, String> {
    let device_id = super::ensure_device_id(app)?;
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path_read_only(database_path(staging)).await?;
    let summary = async {
        let mut conn = pool
            .acquire()
            .await
            .map_err(|error| format!("open the restored database: {error}"))?;
        backup_keys::link_summary(&mut conn, vault_id, &device_id).await
    }
    .await;
    let closed = registry.close_all().await;
    let summary = summary?;
    closed?;
    Ok(summary)
}

/// Turns the staged vault at `staging` into an independent copy: a new vault id, so a new
/// space, with no sync state, recovery kit, or person identity. The next open founds a fresh
/// space and creates a new person identity, whose key belongs to the new vault id.
async fn detach_restored_copy(staging: &Path) -> Result<(), String> {
    detach_database(&database_path(staging)).await?;
    let manifest = read_vault_manifest(staging)?;
    let detached = VaultManifest {
        vault_id: new_vault_id(),
        created_at: now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
        ..manifest
    };
    let json = serde_json::to_string_pretty(&detached)
        .map_err(|error| format!("encode the restored vault marker: {error}"))?;
    write_text_file_atomically(&manifest_path(staging), &json, FileAccess::OwnerOnly)
}

/// Removes the person identity, contact card, recovery kit, and every engine table from the
/// database at `path`, keeping its domain rows.
async fn detach_database(path: &Path) -> Result<(), String> {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(path).await?;
    let detached = async {
        let mut conn = pool
            .acquire()
            .await
            .map_err(|error| format!("open the restored database: {error}"))?;
        remove_person_identity(&mut conn).await?;
        ganbaru_sync::local::reset_sync_state(&mut conn)
            .await
            .map_err(|error| format!("reset the restored sync state: {error}"))?;
        sqlx::query("DELETE FROM sync_recovery_kit")
            .execute(&mut *conn)
            .await
            .map_err(|error| format!("remove the restored recovery kit: {error}"))?;
        Ok::<(), String>(())
    }
    .await;
    let closed = registry.close_all().await;
    detached?;
    closed
}

/// Deletes the person identity and contact card in one transaction. The triggers that forbid
/// it are dropped for the deletion and recreated from their stored definitions.
async fn remove_person_identity(conn: &mut sqlx::SqliteConnection) -> Result<(), String> {
    let failed = |error: sqlx::Error| format!("remove the restored person identity: {error}");
    let mut tx = conn.begin().await.map_err(failed)?;
    let mut definitions = Vec::with_capacity(IDENTITY_TRIGGERS.len());
    for name in IDENTITY_TRIGGERS {
        let definition: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?")
                .bind(name)
                .fetch_one(&mut *tx)
                .await
                .map_err(failed)?;
        definitions.push(definition);
    }
    for name in IDENTITY_TRIGGERS {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TRIGGER \"{name}\"")))
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
    }
    for statement in ["DELETE FROM contacts_local_identity", "DELETE FROM contact_card"] {
        sqlx::query(statement)
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
    }
    for definition in definitions {
        sqlx::query(sqlx::AssertSqlSafe(definition))
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
    }
    tx.commit().await.map_err(failed)
}

/// The staging and swap folders a restore uses beside `target`.
fn restore_paths(target: &Path) -> Result<(PathBuf, PathBuf), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())?;
    Ok((
        parent.join(RESTORE_STAGING_NAME),
        parent.join(RESTORE_ROLLBACK_NAME),
    ))
}

/// Replaces the vault at `target` with the staged vault while every runtime is drained, then
/// selects it. `other_vault` tells whether the staged vault is an independent copy, and
/// `expected_active` is the vault the restore started from, which must still be selected.
async fn replace_active_vault(
    app: &tauri::AppHandle,
    transition: VaultTransition,
    (staging, target, rollback): (&Path, &Path, &Path),
    other_vault: bool,
    expected_active: Option<PathBuf>,
) -> Result<VaultInfo, String> {
    let flagged = !other_vault && replacement_shrinks_drastically(target, staging).await;
    let quiescence =
        quiescence::begin_reserved_quiescence(app, transition, RunningTerminals::Block).await?;
    let activation_app = app.clone();
    let (staging, target, rollback) = (
        staging.to_path_buf(),
        target.to_path_buf(),
        rollback.to_path_buf(),
    );
    blocking(move || {
        let activated = (|| {
            let active = super::read_app_state(&activation_app)?
                .active_vault_path
                .as_ref()
                .map(PathBuf::from);
            if active != expected_active {
                return Err("the active vault changed while preparing the restore".to_string());
            }
            super::ensure_vault_skeleton(&staging)?;
            super::vault_lock::process_vault_lock().replace_with(&staging, &target, || {
                if other_vault {
                    replace_with_other_vault(&staging, &target, &rollback)
                } else {
                    replace_vault_flagging(&staging, &target, &rollback, flagged)
                }
            })?;
            let info = vault_info_from_path(&target)?;
            super::select_quiesced_vault(&activation_app, &info, &quiescence)?;
            Ok(info)
        })();
        let resumed = quiescence.finish();
        match (activated, resumed) {
            (Ok(info), Ok(())) => Ok(info),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(resume)) => {
                Err(format!("{error}; resuming runtimes also failed: {resume}"))
            }
        }
    })
    .await?
}

/// Places the decrypted backup at `archive` as this platform restores it.
#[cfg(desktop)]
async fn place_restored_backup(
    app: &tauri::AppHandle,
    transition: VaultTransition,
    archive: &Path,
    header: &BackupHeader,
    active: Option<&ActiveVault>,
) -> Result<(VaultInfo, RestorePlacement), RestoreError> {
    if let Some(active) = active
        && header.space == active.summary.space
        && active.summary.other_devices == 0
    {
        let target = &active.root;
        let (staging, rollback) = restore_paths(target)?;
        let recover_target = target.clone();
        let recover_rollback = rollback.clone();
        blocking(move || recover_interrupted_restore(&recover_target, &recover_rollback)).await??;
        let staged = StagedFolder::new(&staging);
        let info = stage_restored_vault(archive, &staging, header).await?;
        let backup = staged_summary(app, &staging, &info.vault_id).await?;
        if plan_restore(PLATFORM, Some(&active.summary), &backup)? == RestorePlacement::InPlace {
            let restored = replace_active_vault(
                app,
                transition,
                (&staging, target, &rollback),
                false,
                Some(target.clone()),
            )
            .await?;
            staged.keep();
            return Ok((restored, RestorePlacement::InPlace));
        }
    }
    let restored = restore_in_new_folder(app, transition, archive, header).await?;
    Ok((restored, RestorePlacement::IndependentCopy))
}

/// Restores the backup as an independent copy in a folder the user chooses, then opens it.
#[cfg(desktop)]
async fn restore_in_new_folder(
    app: &tauri::AppHandle,
    transition: VaultTransition,
    archive: &Path,
    header: &BackupHeader,
) -> Result<VaultInfo, RestoreError> {
    use super::backup::{is_free_folder, resolve_link_folder, vault_folder_name};
    let start_directory = super::existing_documents_directory(app);
    let Some(chosen) =
        super::pick_folder(app, "Choose where to open the restored vault", start_directory).await?
    else {
        return Err(RestoreError::Cancelled);
    };
    let target = if is_free_folder(&chosen) {
        chosen
    } else {
        let default = chosen.join(vault_folder_name("", super::default_vault_name()));
        resolve_link_folder(None, &default, super::default_vault_name())?
    };
    let parent = target
        .parent()
        .ok_or_else(|| "the chosen folder has no parent directory".to_string())?;
    let staging_parent = ganbaru_atomic_files::create_private_temporary_directory(
        parent,
        ".ganbaru.restore-",
    )
    .map_err(|error| format!("create restore staging: {error}"))?;
    let staged = StagedFolder::new(&staging_parent);
    let staging = staging_parent.join("vault");
    stage_restored_vault(archive, &staging, header).await?;
    detach_restored_copy(&staging).await?;
    let (move_staging, move_target) = (staging.clone(), target.clone());
    blocking(move || {
        super::ensure_vault_skeleton(&move_staging)?;
        move_into_new_folder(&move_staging, &move_target)
    })
    .await??;
    drop(staged);
    let info = vault_info_from_path(&target)?;
    super::select_reserved_vault(app, &info, transition).await?;
    Ok(info)
}

#[cfg(target_os = "android")]
async fn place_restored_backup(
    app: &tauri::AppHandle,
    transition: VaultTransition,
    archive: &Path,
    header: &BackupHeader,
    active: Option<&ActiveVault>,
) -> Result<(VaultInfo, RestorePlacement), RestoreError> {
    let target = super::default_vault_path(app)?;
    let (staging, rollback) = restore_paths(&target)?;
    let recover_target = target.clone();
    let recover_rollback = rollback.clone();
    blocking(move || {
        if let Some(parent) = recover_target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create app data directory: {error}"))?;
        }
        recover_interrupted_restore(&recover_target, &recover_rollback)
    })
    .await??;
    let staged = StagedFolder::new(&staging);
    let info = stage_restored_vault(archive, &staging, header).await?;
    let backup = staged_summary(app, &staging, &info.vault_id).await?;
    let placement = plan_restore(PLATFORM, active.map(|active| &active.summary), &backup)?;
    let other_vault = placement == RestorePlacement::IndependentCopy;
    if other_vault {
        detach_restored_copy(&staging).await?;
    }
    let restored = if active.is_none() && super::backup::is_free_folder(&target) {
        let (move_staging, move_target) = (staging.clone(), target.clone());
        blocking(move || {
            super::ensure_vault_skeleton(&move_staging)?;
            move_into_new_folder(&move_staging, &move_target)
        })
        .await??;
        let info = vault_info_from_path(&target)?;
        super::select_reserved_vault(app, &info, transition).await?;
        info
    } else {
        replace_active_vault(
            app,
            transition,
            (&staging, &target, &rollback),
            other_vault,
            active.map(|active| active.root.clone()),
        )
        .await?
    };
    staged.keep();
    Ok((restored, placement))
}

#[cfg(target_os = "ios")]
async fn place_restored_backup(
    _app: &tauri::AppHandle,
    _transition: VaultTransition,
    _archive: &Path,
    _header: &BackupHeader,
    _active: Option<&ActiveVault>,
) -> Result<(VaultInfo, RestorePlacement), RestoreError> {
    Err(RestoreError::Failed(
        "backup restore is not available on this platform".to_string(),
    ))
}

#[cfg(test)]
mod tests;
