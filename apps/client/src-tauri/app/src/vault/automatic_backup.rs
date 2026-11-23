//! Automatic encrypted backups of the active vault, on a schedule each device sets for itself.
//!
//! The vault's device preference file holds the schedule, the chosen folder, and the outcome of
//! the last attempt, so each device backs up on its own and nothing replicates. One background
//! loop writes a backup once it is due while the app runs; a backup missed while the app was
//! closed is written soon after the next start. Vault writers keep running during the backup: the
//! database is a consistent snapshot, and the vault transition reservation keeps the vault from
//! being switched or replaced meanwhile. After each backup the folder keeps the newest
//! [`RETAINED_BACKUPS`] automatic backups of the vault and no other file is touched.
//!
//! By default desktop writes beside the vault folder, in a folder named after it with
//! [`DEFAULT_FOLDER_SUFFIX`], and Android writes to a Documents folder named the same way through
//! MediaStore. The user can pick another folder instead: a desktop folder outside the vault, or an
//! Android document tree the app keeps write access to.

use super::encrypted_backup::{
    BACKUP_EXTENSION, active_seal_keys, stamped_file_name, transfer_root,
};
use super::preferences::{
    AUTOMATIC_BACKUP_DEVICE_ENTRY, read_native_device_entry, update_native_device_entry,
};
use super::{now_utc, quiescence};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use ganbaru_sync_replica::backup::{ArchiveGate, BackupSealKeys, create_encrypted_backup};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::Emitter;
use tokio::sync::Notify;

/// Automatic backups of a vault that a folder keeps; older ones are deleted after a backup.
pub(crate) const RETAINED_BACKUPS: usize = 5;
/// Appended to the vault folder's name to name the default backup folder, as in `Ganbaru Backup`.
const DEFAULT_FOLDER_SUFFIX: &str = " Backup";
/// Start of every automatic backup file name; a tag of the vault and a timestamp follow.
const FILE_STEM: &str = "ganbaru-auto-backup";
/// Bytes of the vault identifier digest that tag its automatic backup file names, so vaults
/// sharing a folder never prune each other's backups.
const VAULT_TAG_BYTES: usize = 4;
/// Time after startup before the first scheduled backup, so it never competes with startup work.
const STARTUP_DELAY: Duration = Duration::from_secs(60);
/// Longest wait before the schedule is read again, which also bounds how late a backup runs after
/// the system clock jumps or the device sleeps.
const LONGEST_WAIT: Duration = Duration::from_secs(15 * 60);
/// Wait before a due backup is tried again after another vault operation held the vault.
const BUSY_RETRY: Duration = Duration::from_secs(5 * 60);
/// Seconds after a failed attempt before a scheduled backup is tried again.
const FAILURE_RETRY_SECONDS: i64 = 60 * 60;
/// Folder of the Android shared storage that holds documents, as file managers show it.
#[cfg(target_os = "android")]
const ANDROID_DOCUMENTS_FOLDER: &str = "Documents";
/// Event emitted whenever the automatic backup status changes; listeners read the status again.
pub(crate) const AUTOMATIC_BACKUP_CHANGED_EVENT: &str = "vault-automatic-backup-changed";

/// Whether a backup is being written now.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Wakes the scheduler after the schedule changes.
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);

/// How often automatic backups are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupInterval {
    /// Every six hours.
    SixHours,
    /// Once a day.
    #[default]
    Daily,
    /// Once a week.
    Weekly,
}

impl BackupInterval {
    fn period(self) -> TimeDelta {
        let hours = match self {
            Self::SixHours => 6,
            Self::Daily => 24,
            Self::Weekly => 7 * 24,
        };
        TimeDelta::hours(hours)
    }
}

/// A folder the user picked for automatic backups instead of the default one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum CustomFolder {
    /// A desktop folder by its absolute path.
    Folder { path: String },
    /// An Android document tree by its URI, with the name its provider reported when picked.
    Tree { uri: String, name: String },
}

/// Why the last automatic backup attempt failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// The vault has no recovery kit, so no backup could be opened on a new device.
    NoRecoveryKit,
    /// The folder is missing, inside the vault, or no longer writable by the app.
    FolderUnavailable,
    /// Writing the backup failed for another reason.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Failure {
    kind: FailureKind,
    at: DateTime<Utc>,
}

/// The device's automatic backup settings and last outcome for the vault, as stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Schedule {
    enabled: bool,
    interval: BackupInterval,
    /// The picked folder, `None` for the default one.
    folder: Option<CustomFolder>,
    last_backup_at: Option<DateTime<Utc>>,
    /// The last attempt's failure, cleared by a backup or a change of folder.
    failure: Option<Failure>,
}

/// Where automatic backups go, as Settings shows it.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStatus {
    /// A path or folder name to show.
    label: String,
    /// Whether the user picked the folder instead of the default one.
    custom: bool,
}

/// The last attempt's failure, as Settings shows it.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureStatus {
    kind: FailureKind,
    at: String,
}

/// The automatic backup settings and state of the active vault on this device.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticBackupStatus {
    enabled: bool,
    interval: BackupInterval,
    folder: FolderStatus,
    last_backup_at: Option<String>,
    /// When the next scheduled backup is due, `None` while automatic backups are off.
    next_backup_at: Option<String>,
    failure: Option<FailureStatus>,
    /// Whether a backup is being written now.
    running: bool,
}

/// Why a change to automatic backups was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomaticBackupRefusal {
    /// Automatic backups cannot be turned on before the vault has a recovery kit.
    NoRecoveryKit,
    /// The picked folder is inside the vault, whose backups would then hold earlier backups.
    InsideVault,
}

/// What a change to automatic backups did.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AutomaticBackupUpdate {
    /// The change was saved, or the user closed the picker; the status is current.
    Saved { status: AutomaticBackupStatus },
    /// The change was refused and nothing was saved.
    Refused { reason: AutomaticBackupRefusal },
}

/// What the folder picker returned.
#[cfg_attr(not(desktop), allow(dead_code))]
enum Picked {
    Cancelled,
    InsideVault,
    Chosen(CustomFolder),
}

/// Why writing an automatic backup failed, with the detail kept for the log.
#[derive(Debug)]
enum AttemptError {
    FolderUnavailable(String),
    Failed(String),
}

impl AttemptError {
    fn kind(&self) -> FailureKind {
        match self {
            Self::FolderUnavailable(_) => FailureKind::FolderUnavailable,
            Self::Failed(_) => FailureKind::Failed,
        }
    }

    fn detail(&self) -> &str {
        match self {
            Self::FolderUnavailable(detail) | Self::Failed(detail) => detail,
        }
    }
}

/// What triggered an attempt.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// The scheduler, which writes only a due backup while automatic backups are on.
    Scheduled,
    /// The user, who asked for a backup now.
    Requested,
}

/// What an attempt did.
#[derive(Debug, PartialEq, Eq)]
enum AttemptOutcome {
    /// A backup or its failure was recorded.
    Recorded,
    /// Nothing was due, no vault is active, or the outcome could not be recorded.
    Skipped,
}

/// The file names automatic backups of one vault use.
struct BackupNames {
    /// Passed to [`stamped_file_name`], which appends the timestamp and extension.
    stem: String,
    /// Shared by every automatic backup of the vault and by no other file.
    prefix: String,
}

impl BackupNames {
    fn for_vault(vault_id: &str) -> Self {
        let digest = Sha256::digest(vault_id.as_bytes());
        let tag: String = digest[..VAULT_TAG_BYTES]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let stem = format!("{FILE_STEM}-{tag}");
        let prefix = format!("{stem}-");
        Self { stem, prefix }
    }
}

/// Reads a stored schedule; an absent entry is the default, with automatic backups off.
fn parse_schedule(value: Option<&Value>) -> Result<Schedule, String> {
    value.map_or_else(
        || Ok(Schedule::default()),
        |value| {
            Schedule::deserialize(value)
                .map_err(|error| format!("automatic backup settings are malformed: {error}"))
        },
    )
}

fn read_schedule(app: &tauri::AppHandle) -> Result<Schedule, String> {
    parse_schedule(read_native_device_entry(app, AUTOMATIC_BACKUP_DEVICE_ENTRY)?.as_ref())
}

/// Applies `change` to the stored schedule and returns the result. A malformed entry fails the
/// update unless `replace_malformed`, which starts from the default for a change the user made.
fn update_schedule(
    app: &tauri::AppHandle,
    replace_malformed: bool,
    change: impl FnOnce(&mut Schedule),
) -> Result<Schedule, String> {
    update_native_device_entry(
        app,
        AUTOMATIC_BACKUP_DEVICE_ENTRY,
        |current| {
            let mut schedule = match parse_schedule(current) {
                Ok(schedule) => schedule,
                Err(error) if replace_malformed => {
                    eprintln!("Automatic backup settings are replaced: {error}");
                    Schedule::default()
                }
                Err(error) => return Err(error),
            };
            change(&mut schedule);
            let value = serde_json::to_value(&schedule)
                .map_err(|error| format!("encode automatic backup settings: {error}"))?;
            Ok((value, schedule))
        },
        |error| error,
    )
}

/// When the scheduler should next write a backup, `None` while automatic backups are off.
///
/// A backup is due one interval after the last one, and at once when there was none. A last
/// backup in the future, after the clock moved back, counts as written now. After a failure the
/// next attempt waits [`FAILURE_RETRY_SECONDS`], bounded the same way.
fn next_attempt(schedule: &Schedule, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if !schedule.enabled {
        return None;
    }
    let after = |at: DateTime<Utc>, delay: TimeDelta| {
        at.checked_add_signed(delay)
            .unwrap_or(now)
            .min(now.checked_add_signed(delay).unwrap_or(now))
    };
    let due = schedule
        .last_backup_at
        .map_or(now, |last| after(last, schedule.interval.period()));
    let retry = schedule
        .failure
        .map(|failure| after(failure.at, TimeDelta::seconds(FAILURE_RETRY_SECONDS)));
    Some(retry.map_or(due, |retry| due.max(retry)))
}

/// The names among `names` of automatic backups past the newest [`RETAINED_BACKUPS`], oldest
/// first. Only names with `prefix` and the backup extension count; the timestamp in each name
/// orders them.
fn expired_backups(mut names: Vec<String>, prefix: &str) -> Vec<String> {
    let extension = format!(".{BACKUP_EXTENSION}");
    names.retain(|name| name.starts_with(prefix) && name.ends_with(&extension));
    names.sort();
    names.truncate(names.len().saturating_sub(RETAINED_BACKUPS));
    names
}

/// The name of the default backup folder of the vault at `vault_root`.
fn default_folder_name(vault_root: &Path) -> Result<String, String> {
    let name = vault_root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "the vault folder has no name".to_string())?;
    Ok(format!("{name}{DEFAULT_FOLDER_SUFFIX}"))
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn status_for(
    vault_root: &Path,
    schedule: &Schedule,
    now: DateTime<Utc>,
    running: bool,
) -> Result<AutomaticBackupStatus, String> {
    let folder = match &schedule.folder {
        None => FolderStatus {
            label: default_folder_label(vault_root)?,
            custom: false,
        },
        Some(CustomFolder::Folder { path }) => FolderStatus {
            label: path.clone(),
            custom: true,
        },
        Some(CustomFolder::Tree { name, .. }) => FolderStatus {
            label: name.clone(),
            custom: true,
        },
    };
    Ok(AutomaticBackupStatus {
        enabled: schedule.enabled,
        interval: schedule.interval,
        folder,
        last_backup_at: schedule.last_backup_at.map(timestamp),
        next_backup_at: next_attempt(schedule, now).map(timestamp),
        failure: schedule.failure.map(|failure| FailureStatus {
            kind: failure.kind,
            at: timestamp(failure.at),
        }),
        running,
    })
}

fn current_status(
    app: &tauri::AppHandle,
    schedule: &Schedule,
) -> Result<AutomaticBackupStatus, String> {
    status_for(
        &super::active_vault_path(app)?,
        schedule,
        now_utc(),
        RUNNING.load(Ordering::Acquire),
    )
}

/// Tells listeners the status changed and the scheduler to read the schedule again.
fn announce_change(app: &tauri::AppHandle) {
    if let Err(error) = app.emit(AUTOMATIC_BACKUP_CHANGED_EVENT, ()) {
        eprintln!("Automatic backup status event: {error}");
    }
    WAKE.notify_one();
}

/// Marks a backup as running until dropped.
struct RunningBackup<'a> {
    app: &'a tauri::AppHandle,
}

impl<'a> RunningBackup<'a> {
    fn begin(app: &'a tauri::AppHandle) -> Self {
        RUNNING.store(true, Ordering::Release);
        announce_change(app);
        Self { app }
    }
}

impl Drop for RunningBackup<'_> {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
        announce_change(self.app);
    }
}

/// Writes a backup of the active vault while `_transition` keeps it in place, and records the
/// outcome. A scheduled attempt writes only a due backup.
async fn attempt(
    app: &tauri::AppHandle,
    _transition: quiescence::VaultTransition,
    trigger: Trigger,
) -> AttemptOutcome {
    let (Ok(vault_root), Ok(vault_id)) =
        (super::active_vault_path(app), super::active_vault_id(app))
    else {
        return AttemptOutcome::Skipped;
    };
    let schedule = match read_schedule(app) {
        Ok(schedule) => schedule,
        Err(error) => {
            eprintln!("Automatic backup skipped: {error}");
            return AttemptOutcome::Skipped;
        }
    };
    let now = now_utc();
    if trigger == Trigger::Scheduled && next_attempt(&schedule, now).is_none_or(|at| at > now) {
        return AttemptOutcome::Skipped;
    }
    let running = RunningBackup::begin(app);
    let written = match active_seal_keys(app, &vault_id).await {
        Ok(Some(keys)) => {
            let names = BackupNames::for_vault(&vault_id);
            write_to_folder(app, &vault_root, &keys, schedule.folder.as_ref(), &names)
                .await
                .map_err(|error| {
                    eprintln!("Automatic backup failed: {}", error.detail());
                    error.kind()
                })
        }
        Ok(None) => Err(FailureKind::NoRecoveryKit),
        Err(error) => {
            eprintln!("Automatic backup failed: {error}");
            Err(FailureKind::Failed)
        }
    };
    let at = now_utc();
    let recorded = update_schedule(app, false, |schedule| match written {
        Ok(()) => {
            schedule.last_backup_at = Some(at);
            schedule.failure = None;
        }
        Err(kind) => schedule.failure = Some(Failure { kind, at }),
    });
    drop(running);
    match recorded {
        Ok(_) => AttemptOutcome::Recorded,
        Err(error) => {
            eprintln!("Automatic backup outcome is not recorded: {error}");
            AttemptOutcome::Skipped
        }
    }
}

/// How long the scheduler waits before the next due backup, `None` while none is scheduled.
fn wait_until_due(app: &tauri::AppHandle) -> Option<Duration> {
    super::active_vault_id(app).ok()?;
    let schedule = read_schedule(app)
        .inspect_err(|error| eprintln!("Automatic backups paused: {error}"))
        .ok()?;
    let now = now_utc();
    let due = next_attempt(&schedule, now)?;
    Some((due - now).to_std().unwrap_or(Duration::ZERO))
}

/// Starts the loop that writes automatic backups once they are due, for whichever vault is active.
pub(crate) fn start(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            let wait = match wait_until_due(&app) {
                Some(Duration::ZERO) => match quiescence::reserve_vault_transition() {
                    Ok(transition) => match attempt(&app, transition, Trigger::Scheduled).await {
                        AttemptOutcome::Recorded => continue,
                        AttemptOutcome::Skipped => BUSY_RETRY,
                    },
                    Err(_) => BUSY_RETRY,
                },
                Some(wait) => wait.min(LONGEST_WAIT),
                None => LONGEST_WAIT,
            };
            let _ = tokio::time::timeout(wait, WAKE.notified()).await;
        }
    });
}

/// The automatic backup settings and state of the active vault on this device.
#[tauri::command]
pub async fn vault_automatic_backup_status(
    app: tauri::AppHandle,
) -> Result<AutomaticBackupStatus, String> {
    current_status(&app, &read_schedule(&app)?)
}

/// Turns automatic backups on or off and sets how often they are written. Turning them on is
/// refused while the vault has no recovery kit.
#[tauri::command]
pub async fn vault_automatic_backup_configure(
    app: tauri::AppHandle,
    enabled: bool,
    interval: BackupInterval,
) -> Result<AutomaticBackupUpdate, String> {
    if enabled {
        let vault_id = super::active_vault_id(&app)?;
        if active_seal_keys(&app, &vault_id).await?.is_none() {
            return Ok(AutomaticBackupUpdate::Refused {
                reason: AutomaticBackupRefusal::NoRecoveryKit,
            });
        }
    }
    let schedule = update_schedule(&app, true, |schedule| {
        if enabled && !schedule.enabled {
            schedule.failure = None;
        }
        schedule.enabled = enabled;
        schedule.interval = interval;
    })?;
    announce_change(&app);
    Ok(AutomaticBackupUpdate::Saved {
        status: current_status(&app, &schedule)?,
    })
}

/// Lets the user pick the folder automatic backups go to. A folder inside the vault is refused.
#[tauri::command]
pub async fn vault_automatic_backup_choose_folder(
    app: tauri::AppHandle,
) -> Result<AutomaticBackupUpdate, String> {
    let vault_root = super::active_vault_path(&app)?;
    let current = read_schedule(&app).unwrap_or_default();
    let folder = match pick_custom_folder(&app, &vault_root, current.folder.as_ref()).await? {
        Picked::Cancelled => {
            return Ok(AutomaticBackupUpdate::Saved {
                status: current_status(&app, &current)?,
            });
        }
        Picked::InsideVault => {
            return Ok(AutomaticBackupUpdate::Refused {
                reason: AutomaticBackupRefusal::InsideVault,
            });
        }
        Picked::Chosen(folder) => folder,
    };
    let schedule = update_schedule(&app, true, |schedule| {
        schedule.folder = Some(folder);
        schedule.failure = None;
    })?;
    announce_change(&app);
    Ok(AutomaticBackupUpdate::Saved {
        status: current_status(&app, &schedule)?,
    })
}

/// Sends automatic backups to the default folder again.
#[tauri::command]
pub async fn vault_automatic_backup_reset_folder(
    app: tauri::AppHandle,
) -> Result<AutomaticBackupStatus, String> {
    let schedule = update_schedule(&app, true, |schedule| {
        schedule.folder = None;
        schedule.failure = None;
    })?;
    announce_change(&app);
    current_status(&app, &schedule)
}

/// Writes an automatic backup now, whether or not one is due, and returns the status after it.
#[tauri::command]
pub async fn vault_automatic_backup_run_now(
    app: tauri::AppHandle,
) -> Result<AutomaticBackupStatus, String> {
    let transition = quiescence::reserve_vault_transition()?;
    attempt(&app, transition, Trigger::Requested).await;
    current_status(&app, &read_schedule(&app)?)
}

/// The default backup folder beside the vault at `vault_root`.
#[cfg(desktop)]
fn default_directory(vault_root: &Path) -> Result<std::path::PathBuf, String> {
    let parent = vault_root
        .parent()
        .ok_or_else(|| "the vault folder has no parent folder".to_string())?;
    Ok(parent.join(default_folder_name(vault_root)?))
}

#[cfg(desktop)]
fn default_folder_label(vault_root: &Path) -> Result<String, String> {
    Ok(default_directory(vault_root)?.display().to_string())
}

#[cfg(target_os = "android")]
fn default_folder_label(vault_root: &Path) -> Result<String, String> {
    Ok(format!(
        "{ANDROID_DOCUMENTS_FOLDER}/{}",
        default_folder_name(vault_root)?
    ))
}

#[cfg(target_os = "ios")]
fn default_folder_label(_vault_root: &Path) -> Result<String, String> {
    Err("automatic backups are not available on this platform".to_string())
}

/// Whether `folder` is the vault at `vault_root` or inside it. A folder that cannot be resolved
/// is not, since writing to it fails anyway.
#[cfg(desktop)]
fn inside_vault(folder: &Path, vault_root: &Path) -> bool {
    match (folder.canonicalize(), vault_root.canonicalize()) {
        (Ok(folder), Ok(vault_root)) => folder.starts_with(vault_root),
        _ => false,
    }
}

#[cfg(desktop)]
async fn pick_custom_folder(
    app: &tauri::AppHandle,
    vault_root: &Path,
    current: Option<&CustomFolder>,
) -> Result<Picked, String> {
    let current = match current {
        Some(CustomFolder::Folder { path }) => Some(std::path::PathBuf::from(path)),
        _ => default_directory(vault_root).ok(),
    };
    let start_directory = current
        .filter(|path| path.is_dir())
        .or_else(|| vault_root.parent().map(Path::to_path_buf));
    let Some(path) =
        super::pick_folder(app, "Choose where automatic backups go", start_directory).await?
    else {
        return Ok(Picked::Cancelled);
    };
    if inside_vault(&path, vault_root) {
        return Ok(Picked::InsideVault);
    }
    Ok(Picked::Chosen(CustomFolder::Folder {
        path: super::path_to_string(&path, "backup folder")?,
    }))
}

/// The folder a desktop backup goes to, created when it is the default one and missing.
#[cfg(desktop)]
fn prepare_directory(
    vault_root: &Path,
    folder: Option<&CustomFolder>,
) -> Result<std::path::PathBuf, AttemptError> {
    let unavailable = AttemptError::FolderUnavailable;
    let directory = match folder {
        None => {
            let directory = default_directory(vault_root).map_err(unavailable)?;
            match std::fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(unavailable(format!("create the backup folder: {error}")));
                }
            }
            directory
        }
        Some(CustomFolder::Folder { path }) => std::path::PathBuf::from(path),
        Some(CustomFolder::Tree { .. }) => {
            return Err(unavailable(
                "an Android folder is not available here".to_string(),
            ));
        }
    };
    if !directory.is_dir() {
        return Err(unavailable(format!(
            "{} is not a folder",
            directory.display()
        )));
    }
    if inside_vault(&directory, vault_root) {
        return Err(unavailable(
            "the backup folder is inside the vault".to_string(),
        ));
    }
    Ok(directory)
}

/// Deletes the automatic backups in `directory` past the newest [`RETAINED_BACKUPS`].
#[cfg(desktop)]
fn prune_directory(directory: &Path, prefix: &str) -> Result<(), String> {
    let mut names = Vec::new();
    let entries =
        std::fs::read_dir(directory).map_err(|error| format!("list the backup folder: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("list the backup folder: {error}"))?;
        let regular = entry
            .file_type()
            .map_err(|error| format!("list the backup folder: {error}"))?
            .is_file();
        if let (true, Some(name)) = (regular, entry.file_name().to_str()) {
            names.push(name.to_owned());
        }
    }
    for name in expired_backups(names, prefix) {
        std::fs::remove_file(directory.join(&name))
            .map_err(|error| format!("delete old backup {name}: {error}"))?;
    }
    Ok(())
}

#[cfg(desktop)]
async fn write_to_folder(
    app: &tauri::AppHandle,
    vault_root: &Path,
    keys: &BackupSealKeys,
    folder: Option<&CustomFolder>,
    names: &BackupNames,
) -> Result<(), AttemptError> {
    let directory = prepare_directory(vault_root, folder)?;
    let staging = transfer_root(app).map_err(AttemptError::Failed)?;
    let destination = directory.join(stamped_file_name(&names.stem));
    create_encrypted_backup(
        &ArchiveGate::process(),
        vault_root,
        &staging,
        keys,
        &destination,
    )
    .await
    .map_err(|error| AttemptError::Failed(format!("write encrypted backup: {error}")))?;
    let prefix = names.prefix.clone();
    let pruned = super::encrypted_backup::blocking(move || prune_directory(&directory, &prefix))
        .await
        .and_then(|pruned| pruned);
    if let Err(error) = pruned {
        eprintln!("Automatic backup pruning: {error}");
    }
    Ok(())
}

/// An Android backup folder, owned so it can move to a blocking worker.
#[cfg(target_os = "android")]
#[derive(Clone)]
enum AndroidFolder {
    Documents(String),
    Tree(String),
}

#[cfg(target_os = "android")]
impl AndroidFolder {
    fn as_folder(&self) -> ganbaru_mobile_documents::BackupFolder<'_> {
        use ganbaru_mobile_documents::BackupFolder;
        match self {
            Self::Documents(subdirectory) => BackupFolder::Documents { subdirectory },
            Self::Tree(uri) => BackupFolder::Tree { uri },
        }
    }
}

#[cfg(target_os = "android")]
async fn pick_custom_folder(
    app: &tauri::AppHandle,
    _vault_root: &Path,
    _current: Option<&CustomFolder>,
) -> Result<Picked, String> {
    use ganbaru_mobile_documents::MobileDocumentsExt;
    let picker_app = app.clone();
    let picked =
        super::encrypted_backup::blocking(move || picker_app.mobile_documents().pick_backup_tree())
            .await??;
    Ok(picked.map_or(Picked::Cancelled, |tree| {
        Picked::Chosen(CustomFolder::Tree {
            uri: tree.tree_uri,
            name: tree.display_name,
        })
    }))
}

#[cfg(target_os = "android")]
async fn write_to_folder(
    app: &tauri::AppHandle,
    vault_root: &Path,
    keys: &BackupSealKeys,
    folder: Option<&CustomFolder>,
    names: &BackupNames,
) -> Result<(), AttemptError> {
    use super::encrypted_backup::{
        BACKUP_FILE_KIND, SAVE_TRANSFER_PREFIX, blocking, remove_stale_transfers,
    };
    use ganbaru_mobile_documents::MobileDocumentsExt;
    let target = match folder {
        None => {
            AndroidFolder::Documents(default_folder_name(vault_root).map_err(AttemptError::Failed)?)
        }
        Some(CustomFolder::Tree { uri, .. }) => AndroidFolder::Tree(uri.clone()),
        Some(CustomFolder::Folder { .. }) => {
            return Err(AttemptError::FolderUnavailable(
                "a desktop folder is not available here".to_string(),
            ));
        }
    };
    if let AndroidFolder::Tree(uri) = &target {
        let check_app = app.clone();
        let uri = uri.clone();
        let available = blocking(move || check_app.mobile_documents().backup_tree_available(&uri))
            .await
            .and_then(|available| available)
            .map_err(AttemptError::Failed)?;
        if !available {
            return Err(AttemptError::FolderUnavailable(
                "the app no longer has access to the backup folder".to_string(),
            ));
        }
    }
    let root = transfer_root(app).map_err(AttemptError::Failed)?;
    remove_stale_transfers(&root, SAVE_TRANSFER_PREFIX);
    let transfer =
        ganbaru_atomic_files::create_private_temporary_directory(&root, SAVE_TRANSFER_PREFIX)
            .map_err(|error| AttemptError::Failed(format!("create backup staging: {error}")))?;
    let sealed = transfer.join(format!("backup.{BACKUP_EXTENSION}"));
    let saved = async {
        create_encrypted_backup(&ArchiveGate::process(), vault_root, &root, keys, &sealed)
            .await
            .map_err(|error| format!("write encrypted backup: {error}"))?;
        let source = super::path_to_string(&sealed, "backup file")?;
        let file_name = stamped_file_name(&names.stem);
        let save_app = app.clone();
        let save_target = target.clone();
        blocking(move || {
            save_app.mobile_documents().save_file_to_backup_folder(
                save_target.as_folder(),
                &source,
                &file_name,
                ganbaru_sync_contracts::backup::MAX_BACKUP_FILE_BYTES,
                &[BACKUP_EXTENSION],
                "application/octet-stream",
                BACKUP_FILE_KIND,
            )
        })
        .await?
        .map(|_| ())
    }
    .await;
    if let Err(error) = std::fs::remove_dir_all(&transfer) {
        eprintln!("Automatic backup cleanup: {error}");
    }
    saved.map_err(AttemptError::Failed)?;
    let prune_app = app.clone();
    let prefix = names.prefix.clone();
    let pruned = blocking(move || {
        let documents = prune_app.mobile_documents();
        let present = documents.list_backup_folder_files(target.as_folder(), &prefix)?;
        let expired = expired_backups(present, &prefix);
        if expired.is_empty() {
            return Ok(0);
        }
        documents.delete_backup_folder_files(target.as_folder(), &prefix, &expired)
    })
    .await
    .and_then(|pruned| pruned);
    if let Err(error) = pruned {
        eprintln!("Automatic backup pruning: {error}");
    }
    Ok(())
}

#[cfg(target_os = "ios")]
async fn pick_custom_folder(
    _app: &tauri::AppHandle,
    _vault_root: &Path,
    _current: Option<&CustomFolder>,
) -> Result<Picked, String> {
    Err("automatic backups are not available on this platform".to_string())
}

#[cfg(target_os = "ios")]
async fn write_to_folder(
    _app: &tauri::AppHandle,
    _vault_root: &Path,
    _keys: &BackupSealKeys,
    _folder: Option<&CustomFolder>,
    _names: &BackupNames,
) -> Result<(), AttemptError> {
    Err(AttemptError::Failed(
        "automatic backups are not available on this platform".to_string(),
    ))
}

#[cfg(test)]
mod tests;
