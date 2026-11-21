//! Reversible vault replacement: a database reset and a handoff replacement of the
//! same vault move the previous data into a bounded set of rollback copies that the
//! user can restore from Settings.
//!
//! A reset keeps the database. A handoff replacement keeps the replaced database,
//! and when the replacement holds drastically less data, the whole replaced vault
//! layout, flagged for review on this device.
//!
//! A restore of another vault into the folder, as an Android backup restore does,
//! keeps the whole replaced vault. Its own copies stay hidden until it is restored.

use super::backup::holds_vault;
use super::{APP_SQLITE_FILE, VAULT_MANIFEST_FILE, active_writable_vault_path};
use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeDelta, Utc};
use ganbaru_atomic_files::FileAccess;
use ganbaru_sync::AuthorizationView;
use ganbaru_sync_contracts::Digest32;
use ganbaru_sync_replica::backup::BUNDLE_DIRECTORIES;
use ganbaru_sync_replica::backup::seal_keys::space_state;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

mod reset_journal;
mod restore_journal;

pub(crate) use restore_journal::{
    Recovery as RestoreRecovery, is_pending as has_unfinished_restore, recover as recover_restore,
    recover_or_refuse as recover_restore_or_refuse,
};

/// Vault-root directory that holds rollback copies. It is outside the bundle
/// layout, so backups and handoff never carry it off this device.
pub(crate) const DATABASE_ROLLBACK_DIRECTORY: &str = ".database-rollbacks";
/// Most reset and restore copies kept per vault; older copies are deleted.
pub(crate) const MAX_DATABASE_ROLLBACKS: usize = 3;
/// Most unflagged handoff replacement copies kept per vault, counted apart from
/// resets so routine refreshes never evict a reset copy.
pub(crate) const MAX_REPLACEMENT_ROLLBACKS: usize = 3;
/// Most flagged replacement copies kept per vault. The earliest copies the user
/// has not dismissed are kept first, so a later replacement never evicts the copy
/// from before an unwanted one.
pub(crate) const MAX_FLAGGED_REPLACEMENTS: usize = 2;
/// Most whole copies of another vault that a restore replaced, counted apart so a routine
/// copy never evicts one.
pub(crate) const MAX_OTHER_VAULT_ROLLBACKS: usize = 2;
/// Age after which a dismissed flagged copy is deleted.
const DISMISSED_FLAGGED_RETENTION: TimeDelta = TimeDelta::days(30);
/// File inside a rollback directory that records why the copy was kept. A copy
/// without it is a reset copy.
const RETENTION_FILE: &str = "retention.json";
/// Largest retention file read; the real file is a few dozen bytes.
const MAX_RETENTION_FILE_BYTES: u64 = 4 * 1024;
/// UTC timestamp format of rollback IDs, sortable by name.
const ROLLBACK_ID_FORMAT: &str = "%Y%m%dT%H%M%S%3fZ";
/// Length of an ID produced by [`ROLLBACK_ID_FORMAT`].
const ROLLBACK_ID_LENGTH: usize = 19;
/// Database file suffixes that move together: the main file, WAL, and shared memory.
const DATABASE_FILE_SUFFIXES: [&str; 3] = ["", "-wal", "-shm"];

/// Why a rollback copy was kept.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RollbackOrigin {
    /// A database reset, or the database a restore replaced.
    #[default]
    Reset,
    /// A handoff or backup replacement of the same vault.
    Replacement,
}

/// Why a rollback copy was kept and what it holds, decoded strictly from [`RETENTION_FILE`].
/// Copies of the vault in the folder name their `owner` once another vault replaced them
/// there, so they stay with the vault they belong to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Retention {
    /// A reset, or the data a rollback restore replaced. A `full` copy holds the whole vault
    /// layout, as the data a whole-layout restore replaced does.
    Reset { full: bool, owner: Option<String> },
    /// The database a replacement of the same vault replaced.
    Replacement { owner: Option<String> },
    /// The whole layout a replacement of the same vault replaced while holding drastically less
    /// data, shown for review until the user dismisses it.
    Flagged {
        dismissed: bool,
        owner: Option<String>,
    },
    /// The whole other vault `vault` that a restore replaced; restoring it brings that vault
    /// back in this folder.
    OtherVault { vault: String },
}

impl Default for Retention {
    /// A copy without a retention file is a reset copy.
    fn default() -> Self {
        Self::Reset {
            full: false,
            owner: None,
        }
    }
}

impl Retention {
    /// Retention of a same-vault replacement, keeping the whole layout when flagged.
    pub(crate) fn replacement(flagged: bool) -> Self {
        if flagged {
            Self::Flagged {
                dismissed: false,
                owner: None,
            }
        } else {
            Self::Replacement { owner: None }
        }
    }

    /// Retention of the whole vault `vault_id` that a restore of another vault replaced.
    fn other_vault(vault_id: &str) -> Self {
        Self::OtherVault {
            vault: vault_id.to_string(),
        }
    }

    pub(crate) fn origin(&self) -> RollbackOrigin {
        match self {
            Self::Reset { .. } => RollbackOrigin::Reset,
            Self::Replacement { .. } | Self::Flagged { .. } | Self::OtherVault { .. } => {
                RollbackOrigin::Replacement
            }
        }
    }

    pub(crate) fn flagged(&self) -> bool {
        matches!(self, Self::Flagged { .. })
    }

    pub(crate) fn dismissed(&self) -> bool {
        matches!(
            self,
            Self::Flagged {
                dismissed: true,
                ..
            }
        )
    }

    fn is_other_vault(&self) -> bool {
        matches!(self, Self::OtherVault { .. })
    }

    /// Whether the copy holds the whole vault layout, not only the database.
    fn full(&self) -> bool {
        match self {
            Self::Reset { full, .. } => *full,
            Self::Replacement { .. } => false,
            Self::Flagged { .. } | Self::OtherVault { .. } => true,
        }
    }

    /// The vault the copy belongs to, when it is recorded.
    fn vault(&self) -> Option<&str> {
        match self {
            Self::Reset { owner, .. }
            | Self::Replacement { owner }
            | Self::Flagged { owner, .. } => owner.as_deref(),
            Self::OtherVault { vault } => Some(vault),
        }
    }

    /// The same copy recorded as belonging to `vault_id`. A whole other vault already names
    /// its vault and stays as it is.
    fn claimed_by(self, vault_id: &str) -> Self {
        let owner = Some(vault_id.to_string());
        match self {
            Self::Reset { full, .. } => Self::Reset { full, owner },
            Self::Replacement { .. } => Self::Replacement { owner },
            Self::Flagged { dismissed, .. } => Self::Flagged { dismissed, owner },
            other @ Self::OtherVault { .. } => other,
        }
    }

    /// The same copy with its review notice dismissed. Only a flagged copy has one.
    fn dismissed_notice(self) -> Self {
        match self {
            Self::Flagged { owner, .. } => Self::Flagged {
                dismissed: true,
                owner,
            },
            other => other,
        }
    }
}

/// The retention record as [`RETENTION_FILE`] stores it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredRetention {
    origin: RollbackOrigin,
    flagged: bool,
    dismissed: bool,
    full: bool,
    #[serde(default)]
    other_vault: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vault: Option<String>,
}

impl From<&Retention> for StoredRetention {
    fn from(retention: &Retention) -> Self {
        Self {
            origin: retention.origin(),
            flagged: retention.flagged(),
            dismissed: retention.dismissed(),
            full: retention.full(),
            other_vault: retention.is_other_vault(),
            vault: retention.vault().map(str::to_string),
        }
    }
}

impl TryFrom<StoredRetention> for Retention {
    type Error = String;

    /// Refuses every field combination that no [`Retention`] holds.
    fn try_from(stored: StoredRetention) -> Result<Self, String> {
        let StoredRetention {
            origin,
            flagged,
            dismissed,
            full,
            other_vault,
            vault,
        } = stored;
        let inconsistent =
            |detail: &str| Err(format!("rollback retention is inconsistent: {detail}"));
        if dismissed && !flagged {
            return inconsistent("only a flagged copy can be dismissed");
        }
        match (origin, other_vault, flagged) {
            (RollbackOrigin::Replacement, true, false) => match (full, vault) {
                (true, Some(vault)) => Ok(Self::OtherVault { vault }),
                (false, _) => inconsistent("a whole other vault must hold the whole layout"),
                (true, None) => inconsistent("a whole other vault must name its vault"),
            },
            (_, true, _) => inconsistent("a whole other vault is a plain replacement"),
            (RollbackOrigin::Reset, false, false) => Ok(Self::Reset { full, owner: vault }),
            (RollbackOrigin::Reset, false, true) => inconsistent("a reset copy is never flagged"),
            (RollbackOrigin::Replacement, false, false) if full => {
                inconsistent("an unflagged replacement keeps only the database")
            }
            (RollbackOrigin::Replacement, false, false) => Ok(Self::Replacement { owner: vault }),
            (RollbackOrigin::Replacement, false, true) if !full => {
                inconsistent("a flagged replacement keeps the whole layout")
            }
            (RollbackOrigin::Replacement, false, true) => Ok(Self::Flagged {
                dismissed,
                owner: vault,
            }),
        }
    }
}

impl DatabaseRollback {
    /// Whether the copy shows for the vault `current` in the folder: a whole other vault
    /// always shows, and any other copy only while the vault it belongs to is there.
    fn shows_for(&self, current: Option<&str>) -> bool {
        self.retention.is_other_vault()
            || self
                .retention
                .vault()
                .is_none_or(|vault| Some(vault) == current)
    }
}

/// One restorable copy whose retention record decoded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseRollback {
    pub id: String,
    pub created_at: String,
    pub size_bytes: u64,
    pub(crate) retention: Retention,
}

/// The copy as the webview reads it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RollbackView<'a> {
    id: &'a str,
    created_at: &'a str,
    size_bytes: u64,
    origin: RollbackOrigin,
    flagged: bool,
    dismissed: bool,
    /// The copy is a whole other vault that a restore replaced.
    other_vault: bool,
}

impl Serialize for DatabaseRollback {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RollbackView {
            id: &self.id,
            created_at: &self.created_at,
            size_bytes: self.size_bytes,
            origin: self.retention.origin(),
            flagged: self.retention.flagged(),
            dismissed: self.retention.dismissed(),
            other_vault: self.retention.is_other_vault(),
        }
        .serialize(serializer)
    }
}

fn rollback_root(vault: &Path) -> PathBuf {
    vault.join(DATABASE_ROLLBACK_DIRECTORY)
}

fn rollback_id(at: DateTime<Utc>) -> String {
    at.format(ROLLBACK_ID_FORMAT).to_string()
}

/// Parses an ID the webview sent back. Only IDs this module produces are accepted,
/// so the value can never name a path outside the rollback directory.
fn parse_rollback_id(id: &str) -> Result<DateTime<Utc>, String> {
    if id.len() != ROLLBACK_ID_LENGTH {
        return Err("unknown database rollback".to_string());
    }
    let parsed = NaiveDateTime::parse_from_str(id, ROLLBACK_ID_FORMAT)
        .map_err(|_| "unknown database rollback".to_string())?
        .and_utc();
    if rollback_id(parsed) != id {
        return Err("unknown database rollback".to_string());
    }
    Ok(parsed)
}

fn database_file(directory: &Path, suffix: &str) -> PathBuf {
    directory.join(format!("{APP_SQLITE_FILE}{suffix}"))
}

/// Entry names a copy moves: the database files, plus the vault marker and bundle
/// directories for a full copy.
fn moved_entry_names(full: bool) -> Vec<String> {
    let mut names: Vec<String> = DATABASE_FILE_SUFFIXES
        .iter()
        .map(|suffix| format!("{APP_SQLITE_FILE}{suffix}"))
        .collect();
    if full {
        names.push(VAULT_MANIFEST_FILE.to_string());
        names.extend(BUNDLE_DIRECTORIES.iter().map(|name| (*name).to_string()));
    }
    names
}

/// Moves the named entries that exist from `from` to `to`. A failure moves the
/// entries already moved back, so the data never ends up split.
fn move_entries(from: &Path, to: &Path, names: &[String]) -> Result<(), String> {
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for name in names {
        let source = from.join(name);
        if fs::symlink_metadata(&source).is_err() {
            continue;
        }
        let destination = to.join(name);
        let result = if fs::symlink_metadata(&destination).is_ok() {
            Err(format!("'{name}' already exists"))
        } else {
            fs::rename(&source, &destination).map_err(|error| error.to_string())
        };
        if let Err(error) = result {
            for (moved_source, moved_destination) in moved.iter().rev() {
                let _ = fs::rename(moved_destination, moved_source);
            }
            return Err(format!("move vault entry: {error}"));
        }
        moved.push((source, destination));
    }
    Ok(())
}

/// Reads the retention record of the copy in `directory`. A copy without one is a reset copy.
/// A record that cannot be read or decoded is an error, so the copy is never mistaken for a
/// database-only reset copy that retention may delete or a restore may move only in part.
fn read_retention(directory: &Path) -> Result<Retention, String> {
    let path = directory.join(RETENTION_FILE);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Retention::default());
        }
        Err(error) => return Err(format!("read database rollback retention: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_RETENTION_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read database rollback retention: {error}"))?;
    if bytes.len() as u64 > MAX_RETENTION_FILE_BYTES {
        return Err("database rollback retention is too large".to_string());
    }
    let stored: StoredRetention = serde_json::from_slice(&bytes)
        .map_err(|error| format!("decode database rollback retention: {error}"))?;
    Retention::try_from(stored)
}

fn write_retention(directory: &Path, retention: &Retention) -> Result<(), String> {
    let bytes = serde_json::to_vec(&StoredRetention::from(retention))
        .map_err(|error| format!("encode database rollback retention: {error}"))?;
    ganbaru_atomic_files::write_file_atomically(
        &directory.join(RETENTION_FILE),
        &bytes,
        FileAccess::OwnerOnly,
    )
    .map_err(|error| format!("write database rollback retention: {error}"))
}

/// A copy named by a rollback ID that holds a regular database file but whose retention record
/// does not decode. It is shown so the user knows it is there, and never restored, dismissed,
/// or pruned, so it stays on disk untouched.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableRollback {
    pub id: String,
    pub created_at: String,
    pub size_bytes: u64,
}

/// Every copy in a rollback folder, newest first, split by whether its retention decoded.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackListing {
    pub restorable: Vec<DatabaseRollback>,
    pub unreadable: Vec<UnreadableRollback>,
}

/// Lists rollback directories, newest first, ignoring entries that are not rollback IDs or
/// that hold no regular database file, and setting apart copies whose retention record does
/// not decode.
pub(crate) fn scan_rollbacks(vault: &Path) -> Result<RollbackListing, String> {
    let root = rollback_root(vault);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RollbackListing::default());
        }
        Err(error) => return Err(format!("read database rollbacks: {error}")),
    };
    let mut listing = RollbackListing::default();
    for entry in entries {
        let entry = entry.map_err(|error| format!("read database rollbacks: {error}"))?;
        let Some(id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(created) = parse_rollback_id(&id) else {
            continue;
        };
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(database_file(&entry.path(), "")) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let created_at = created.to_rfc3339_opts(SecondsFormat::Millis, true);
        let size_bytes = metadata.len();
        match read_retention(&entry.path()) {
            Ok(retention) => listing.restorable.push(DatabaseRollback {
                id,
                created_at,
                size_bytes,
                retention,
            }),
            Err(_) => listing.unreadable.push(UnreadableRollback {
                id,
                created_at,
                size_bytes,
            }),
        }
    }
    listing
        .restorable
        .sort_by(|left, right| right.id.cmp(&left.id));
    listing
        .unreadable
        .sort_by(|left, right| right.id.cmp(&left.id));
    Ok(listing)
}

/// The copies in `vault` whose retention decoded, newest first. A copy whose retention does
/// not decode is never shown as restorable, restored, or pruned.
pub(crate) fn list_rollbacks(vault: &Path) -> Result<Vec<DatabaseRollback>, String> {
    Ok(scan_rollbacks(vault)?.restorable)
}

/// The rollbacks of the vault in the folder `vault`, newest first, leaving out the copies of
/// a vault that another one replaced there. Unreadable copies are all listed, as their
/// retention cannot say which vault they belong to.
pub(crate) fn visible_rollbacks(vault: &Path) -> Result<RollbackListing, String> {
    let vault_id = vault_id_at(vault)?;
    let listing = scan_rollbacks(vault)?;
    Ok(RollbackListing {
        restorable: listing
            .restorable
            .into_iter()
            .filter(|rollback| rollback.shows_for(Some(&vault_id)))
            .collect(),
        unreadable: listing.unreadable,
    })
}

/// Records `vault_id` on every copy in `vault` that is not a whole other vault and names no
/// vault yet, before another vault replaces it there, so those copies stay with the vault
/// they belong to.
fn claim_rollbacks(vault: &Path, vault_id: &str) -> Result<(), String> {
    let root = rollback_root(vault);
    for rollback in list_rollbacks(vault)? {
        if rollback.retention.vault().is_some() {
            continue;
        }
        write_retention(
            &root.join(&rollback.id),
            &rollback.retention.claimed_by(vault_id),
        )?;
    }
    Ok(())
}

/// Selects the rollbacks retention no longer keeps, given every rollback newest first and
/// the vault `current` in the folder, `None` when its marker is unreadable. Copies that show
/// for it follow [`stale_rollbacks`]; a hidden copy stays while a kept whole copy of its
/// vault could bring that vault back, or while the vault in the folder is unknown.
fn pruned_rollbacks(
    rollbacks: &[DatabaseRollback],
    current: Option<&str>,
    now: DateTime<Utc>,
) -> Vec<String> {
    let (shown, hidden): (Vec<DatabaseRollback>, Vec<DatabaseRollback>) = rollbacks
        .iter()
        .cloned()
        .partition(|rollback| rollback.shows_for(current));
    let mut stale = stale_rollbacks(&shown, now);
    if current.is_none() {
        return stale;
    }
    let kept_vaults: Vec<&str> = shown
        .iter()
        .filter(|rollback| rollback.retention.is_other_vault() && !stale.contains(&rollback.id))
        .filter_map(|rollback| rollback.retention.vault())
        .collect();
    stale.extend(
        hidden
            .iter()
            .filter(|rollback| {
                rollback
                    .retention
                    .vault()
                    .is_none_or(|vault| !kept_vaults.contains(&vault))
            })
            .map(|rollback| rollback.id.clone()),
    );
    stale
}

/// Selects the rollbacks retention no longer keeps, given every rollback newest
/// first: whole other vaults, resets, and unflagged replacements beyond their counts,
/// dismissed flagged copies past their age, and flagged copies beyond their count,
/// keeping the earliest undismissed ones first.
fn stale_rollbacks(rollbacks: &[DatabaseRollback], now: DateTime<Utc>) -> Vec<String> {
    let mut stale = Vec::new();
    let mut resets = 0;
    let mut replacements = 0;
    let mut other_vaults = 0;
    let mut flagged: Vec<(&DatabaseRollback, bool)> = Vec::new();
    for rollback in rollbacks {
        let (count, limit) = match &rollback.retention {
            Retention::OtherVault { .. } => (&mut other_vaults, MAX_OTHER_VAULT_ROLLBACKS),
            Retention::Reset { .. } => (&mut resets, MAX_DATABASE_ROLLBACKS),
            Retention::Replacement { .. } => (&mut replacements, MAX_REPLACEMENT_ROLLBACKS),
            Retention::Flagged { dismissed, .. } => {
                let expired = *dismissed
                    && parse_rollback_id(&rollback.id)
                        .is_ok_and(|created| created + DISMISSED_FLAGGED_RETENTION < now);
                if expired {
                    stale.push(rollback.id.clone());
                } else {
                    flagged.push((rollback, *dismissed));
                }
                continue;
            }
        };
        *count += 1;
        if *count > limit {
            stale.push(rollback.id.clone());
        }
    }
    flagged.sort_by(|(left, left_dismissed), (right, right_dismissed)| {
        left_dismissed
            .cmp(right_dismissed)
            .then_with(|| match left_dismissed {
                false => left.id.cmp(&right.id),
                true => right.id.cmp(&left.id),
            })
    });
    stale.extend(
        flagged
            .into_iter()
            .skip(MAX_FLAGGED_REPLACEMENTS)
            .map(|(rollback, _)| rollback.id.clone()),
    );
    stale
}

/// Deletes the rollbacks retention no longer keeps, refusing while a restore is unfinished.
fn prune_rollbacks(vault: &Path, now: DateTime<Utc>) -> Result<(), String> {
    restore_journal::refuse_pending(vault)?;
    prune_unchecked(vault, now)
}

/// [`prune_rollbacks`] for the restore that holds the journal.
fn prune_unchecked(vault: &Path, now: DateTime<Utc>) -> Result<(), String> {
    let root = rollback_root(vault);
    let current = vault_id_at(vault).ok();
    for stale in pruned_rollbacks(&list_rollbacks(vault)?, current.as_deref(), now) {
        fs::remove_dir_all(root.join(&stale))
            .map_err(|error| format!("remove old database rollback: {error}"))?;
    }
    Ok(())
}

/// Moves the vault database into a new reset copy and prunes old copies.
/// Returns `None` when the vault has no database to move.
///
/// The reset is journaled: an error or a crash before every database file durably reached the
/// copy moves them back, so the vault never holds the database apart from its log.
#[cfg(any(test, desktop))]
pub(crate) fn move_database_to_rollback(
    vault: &Path,
    at: DateTime<Utc>,
) -> Result<Option<String>, String> {
    restore_journal::refuse_pending(vault)?;
    if fs::symlink_metadata(database_file(vault, "")).is_err() {
        return Ok(None);
    }
    let (directory, id) = create_rollback_directory(vault, at)?;
    let mut record = match reset_journal::ResetRecord::begin(vault, &id) {
        Ok(record) => record,
        Err(error) => {
            let _ = fs::remove_dir(&directory);
            return Err(error);
        }
    };
    let root = rollback_root(vault);
    let moved = move_entries(vault, &directory, &moved_entry_names(false))
        .and_then(|()| restore_journal::flush(&[&directory, &root, vault]))
        .and_then(|()| record.advance(reset_journal::Event::MovedOut));
    match (moved, record.resume(at)) {
        (Ok(()), Ok(RestoreRecovery::Settled)) => Ok(Some(id)),
        // The database is durably in the copy, so the reset stands; the journal stays and the
        // next open of the vault finishes it.
        (Ok(()), Ok(RestoreRecovery::Pending(reason)) | Err(reason)) => {
            eprintln!("Reset cleanup: {reason}");
            Ok(Some(id))
        }
        (Err(error), Ok(RestoreRecovery::Settled)) => Err(error),
        (Err(error), Ok(RestoreRecovery::Pending(undo)) | Err(undo)) => {
            Err(format!("{error}; undo the reset: {undo}"))
        }
    }
}

/// Keeps the data of `replaced`, a vault folder another copy of the same vault is
/// replacing, as a rollback inside it. The rollback directory is not part of the
/// vault layout, so the replacement step carries it into the new folder. Does
/// nothing when `replaced` is not the vault `vault_id` or has no database.
pub(crate) fn retain_replaced_vault(
    replaced: &Path,
    vault_id: &str,
    flagged: bool,
    at: DateTime<Utc>,
) -> Result<Option<String>, String> {
    if !holds_vault(replaced, vault_id) {
        return Ok(None);
    }
    create_rollback(replaced, at, Retention::replacement(flagged))
}

/// Keeps the whole vault in `replaced`, which a restore of another vault is replacing, as a
/// rollback inside it. Its own copies are recorded as belonging to it first, so they stay
/// hidden while the other vault is there and come back when it is restored. Fails when
/// `replaced` holds no readable vault or no database, so the caller keeps the folder some
/// other way.
pub(crate) fn retain_other_vault(replaced: &Path, at: DateTime<Utc>) -> Result<String, String> {
    let vault_id = vault_id_at(replaced)?;
    claim_rollbacks(replaced, &vault_id)?;
    create_rollback(replaced, at, Retention::other_vault(&vault_id))?
        .ok_or_else(|| "the replaced vault has no database".to_string())
}

/// Applies retention limits to the vault at `vault` after a replacement.
pub(crate) fn prune_after_replacement(vault: &Path, at: DateTime<Utc>) -> Result<(), String> {
    prune_rollbacks(vault, at)
}

/// Whether the vault folder at `vault` holds the rollback copy `id`.
pub(crate) fn holds_rollback(vault: &Path, id: &str) -> bool {
    parse_rollback_id(id).is_ok() && rollback_root(vault).join(id).is_dir()
}

/// The creation time of the newest rollback directory in `root`, counting every directory
/// named by a rollback ID, including copies [`list_rollbacks`] ignores.
fn newest_rollback_time(root: &Path) -> Result<Option<DateTime<Utc>>, String> {
    let entries =
        fs::read_dir(root).map_err(|error| format!("read database rollbacks: {error}"))?;
    let mut newest = None;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read database rollbacks: {error}"))?;
        let created = entry
            .file_name()
            .to_str()
            .and_then(|name| parse_rollback_id(name).ok());
        newest = newest.max(created);
    }
    Ok(newest)
}

/// Moves the vault data a copy keeps into a new rollback directory whose ID sorts
/// after every existing copy, so a clock step backwards or a pruned ID never makes
/// the new copy the oldest. Returns `None` when there is no database to move. Refuses while a
/// restore is unfinished, since that restore's recovery owns the copies.
fn create_rollback(
    vault: &Path,
    at: DateTime<Utc>,
    retention: Retention,
) -> Result<Option<String>, String> {
    restore_journal::refuse_pending(vault)?;
    if fs::symlink_metadata(database_file(vault, "")).is_err() {
        return Ok(None);
    }
    let (directory, id) = create_rollback_directory(vault, at)?;
    let root = rollback_root(vault);
    let names = moved_entry_names(retention.full());
    let kept = if retention == Retention::default() {
        Ok(())
    } else {
        write_retention(&directory, &retention)
    }
    .and_then(|()| move_entries(vault, &directory, &names));
    if let Err(error) = kept {
        return Err(match fs::remove_dir_all(&directory) {
            Ok(()) => error,
            Err(cleanup) => format!("{error}; remove empty database rollback: {cleanup}"),
        });
    }
    // A replacement deletes what is left of `vault` after this returns, so the moves must reach
    // stable storage first. A failed flush keeps the moved entries where they are.
    [directory.as_path(), root.as_path(), vault]
        .into_iter()
        .try_for_each(ganbaru_atomic_files::sync_directory_entries)
        .map_err(|error| format!("flush database rollback: {error}"))?;
    Ok(Some(id))
}

/// Creates an empty private rollback directory in `vault` whose ID sorts after every existing
/// copy, and returns it with its ID.
fn create_rollback_directory(vault: &Path, at: DateTime<Utc>) -> Result<(PathBuf, String), String> {
    let root = rollback_root(vault);
    ganbaru_atomic_files::create_private_directories(&root)
        .map_err(|error| format!("create database rollbacks: {error}"))?;
    let mut created = at;
    if let Some(newest) = newest_rollback_time(&root)? {
        created = created.max(newest + TimeDelta::milliseconds(1));
    }
    loop {
        let id = rollback_id(created);
        let candidate = root.join(&id);
        match ganbaru_atomic_files::create_private_directory(&candidate) {
            Ok(()) => return Ok((candidate, id)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                created += TimeDelta::milliseconds(1);
            }
            Err(error) => return Err(format!("create database rollback: {error}")),
        }
    }
}

/// Checks that a rollback copy is a Ganbaru database whose schema is a prefix of
/// the current migrations and that passes SQLite's integrity check. The vault
/// folder is user-writable, so a rollback-named file is not trusted by location.
async fn validate_rollback_database(path: &Path) -> Result<(), String> {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path_read_only(path).await?;
    let validation = async {
        ganbaru_db::validate_schema_catalog(&pool).await?;
        let row = sqlx::query("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .map_err(|error| format!("check database rollback: {error}"))?;
        let result: String = row
            .try_get(0)
            .map_err(|error| format!("read database rollback check: {error}"))?;
        if result != "ok" {
            return Err(format!(
                "database rollback failed its integrity check: {result}"
            ));
        }
        Ok(())
    }
    .await;
    let close_result = registry.close_all().await;
    validation?;
    close_result
}

fn vault_id_at(vault: &Path) -> Result<String, String> {
    super::vault_info_from_path(vault).map(|info| info.vault_id)
}

/// Resolves rollback `id` to its directory and retention, and validates its
/// database. A whole other vault must hold a readable vault marker; any other copy
/// must belong to the vault in `vault`, and a full one must hold that same vault.
async fn validate_rollback(vault: &Path, id: &str) -> Result<(PathBuf, Retention), String> {
    parse_rollback_id(id)?;
    let directory = rollback_root(vault).join(id);
    let is_directory = fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir());
    let is_file =
        fs::symlink_metadata(database_file(&directory, "")).is_ok_and(|meta| meta.is_file());
    if !is_directory || !is_file {
        return Err("unknown database rollback".to_string());
    }
    let retention = read_retention(&directory)?;
    if retention.is_other_vault() {
        vault_id_at(&directory)?;
    } else if retention.vault().is_some() || retention.full() {
        let current = vault_id_at(vault)?;
        let belongs = retention.vault().is_none_or(|id| id == current);
        if !belongs || (retention.full() && !holds_vault(&directory, &current)) {
            return Err("database rollback belongs to a different vault".to_string());
        }
    }
    validate_rollback_database(&database_file(&directory, "")).await?;
    Ok((directory, retention))
}

/// Restores rollback `id` as the vault data. The current data becomes a new
/// rollback first, so a restore is itself reversible. Restoring a whole other vault
/// keeps the current vault as a whole other vault in turn. Returns whether the folder
/// now holds another vault.
///
/// The restore is journaled: an error or a crash before the chosen copy is wholly in place
/// moves everything back, so the vault never holds part of either copy.
pub(crate) async fn restore_rollback(
    vault: &Path,
    id: &str,
    at: DateTime<Utc>,
) -> Result<bool, String> {
    restore_journal::refuse_pending(vault)?;
    let (directory, retention) = validate_rollback(vault, id).await?;
    let names = moved_entry_names(retention.full());
    let replaced_retention = if retention.is_other_vault() {
        let current = vault_id_at(vault)?;
        claim_rollbacks(vault, &current)?;
        Retention::other_vault(&current)
    } else {
        Retention::Reset {
            full: retention.full(),
            owner: None,
        }
    };
    let holds_database = fs::symlink_metadata(database_file(vault, "")).is_ok();
    if !holds_database
        && let Some(name) = names
            .iter()
            .find(|name| fs::symlink_metadata(vault.join(name)).is_ok())
    {
        return Err(format!("move vault entry: '{name}' already exists"));
    }
    let replaced = if holds_database {
        Some(create_rollback_directory(vault, at)?)
    } else {
        None
    };
    let journal = restore_journal::Journal {
        phase: if replaced.is_some() {
            restore_journal::Phase::Replacing
        } else {
            restore_journal::Phase::Restoring
        },
        source: id.to_string(),
        replaced: replaced.as_ref().map(|(_, id)| id.clone()),
        full: retention.full(),
    };
    let mut record = match restore_journal::RestoreRecord::begin(vault, journal) {
        Ok(record) => record,
        Err(error) => {
            if let Some((replaced, _)) = &replaced {
                let _ = fs::remove_dir(replaced);
            }
            return Err(error);
        }
    };
    let root = rollback_root(vault);
    let moved = (|| {
        if let Some((replaced, _)) = &replaced {
            if replaced_retention != Retention::default() {
                write_retention(replaced, &replaced_retention)?;
            }
            move_entries(vault, replaced, &names)?;
            restore_journal::flush(&[replaced, &root, vault])?;
            record.advance(restore_journal::Event::MovedOut)?;
        }
        move_entries(&directory, vault, &names)?;
        restore_journal::flush(&[vault, &directory, &root])?;
        record.advance(restore_journal::Event::MovedIn)
    })();
    let settled = record.resume(at);
    match (moved, settled) {
        (Ok(()), Ok(restore_journal::Recovery::Settled)) => Ok(retention.is_other_vault()),
        // The chosen copy is durably in place, so the restore stands; the journal stays and the
        // next open of the vault removes the emptied copy.
        (Ok(()), Ok(restore_journal::Recovery::Pending(reason))) => {
            eprintln!("Rollback restore cleanup: {reason}");
            Ok(retention.is_other_vault())
        }
        (Ok(()), Err(error)) => {
            eprintln!("Rollback restore cleanup: {error}");
            Ok(retention.is_other_vault())
        }
        (Err(error), Ok(restore_journal::Recovery::Settled)) => Err(error),
        (Err(error), Ok(restore_journal::Recovery::Pending(reason))) => {
            Err(format!("{error}; undo the rollback restore: {reason}"))
        }
        (Err(error), Err(undo)) => Err(format!("{error}; undo the rollback restore: {undo}")),
    }
}

/// Marks a flagged rollback as reviewed, so its notice no longer shows. The copy
/// stays restorable until retention removes it.
pub(crate) fn dismiss_rollback(vault: &Path, id: &str) -> Result<(), String> {
    parse_rollback_id(id)?;
    let directory = rollback_root(vault).join(id);
    if !fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir()) {
        return Err("unknown database rollback".to_string());
    }
    let retention = read_retention(&directory)?;
    if !retention.flagged() || retention.dismissed() {
        return Ok(());
    }
    write_retention(&directory, &retention.dismissed_notice())
}

/// Lists the active vault's database rollbacks, newest first, with the copies that cannot be read.
#[tauri::command]
pub async fn vault_database_rollbacks(app: tauri::AppHandle) -> Result<RollbackListing, String> {
    let (vault, _) = super::active_vault(&app)?;
    visible_rollbacks(&vault)
}

/// Dismisses the review notice of a flagged rollback of the active vault. The
/// rollback directory is device-local, so a read-only copy may dismiss it too.
#[tauri::command]
pub async fn vault_dismiss_database_rollback(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    let (vault, _) = super::active_vault(&app)?;
    tauri::async_runtime::spawn_blocking(move || dismiss_rollback(&vault, &id))
        .await
        .map_err(|error| format!("database rollback dismiss worker failed: {error}"))?
}

/// Refuses restoring the copy described by `retention` when it is another vault and this
/// Android device is `linked` now, as a restored backup is refused there.
fn refuse_other_vault_rollback(
    platform: super::encrypted_backup::RestorePlatform,
    retention: &Retention,
    linked: bool,
) -> Result<(), String> {
    if !retention.is_other_vault() {
        return Ok(());
    }
    super::encrypted_backup::refuse_other_vault_while_linked(platform, linked)
}

/// The head of a vault's selected authorization log, seen from one device.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LogHead {
    /// Entry hash of the selected head.
    entry_hash: Digest32,
    /// Whether the seeing device anchors the log.
    anchored_here: bool,
    /// Devices other than the seeing one that the log enrolled and has not removed.
    other_live_devices: usize,
}

/// The head of `view` seen from the device `device_id`.
fn log_head_of(view: &AuthorizationView, device_id: &str) -> LogHead {
    LogHead {
        entry_hash: view.entry_hash,
        anchored_here: view.anchor_device_id == device_id,
        other_live_devices: view
            .devices
            .iter()
            .filter(|device| device.device_id != device_id && device.removed_revision.is_none())
            .count(),
    }
}

/// The head of the selected log of the vault `vault_id` whose database `conn` opens, seen
/// from `device_id`, or `None` when the database holds no log.
async fn log_head(
    conn: &mut sqlx::SqliteConnection,
    vault_id: &str,
    device_id: &str,
) -> Result<Option<LogHead>, String> {
    Ok(space_state(conn, vault_id)
        .await?
        .map(|(_, view)| log_head_of(&view, device_id)))
}

/// Whether a rollback would rewind authorization a live anchor already sequenced: this
/// device anchors a log other live devices follow, and nothing shows the copy holds its head.
fn rewinds_live_anchor(live: Option<&LogHead>, copy: Option<&LogHead>) -> bool {
    live.is_some_and(|live| {
        live.anchored_here
            && live.other_live_devices > 0
            && copy.is_none_or(|copy| copy.entry_hash != live.entry_hash)
    })
}

/// The log head of the database at `path`, or `None` when it has no log. A database that
/// cannot be opened or read also answers `None`, with the cause logged.
async fn readable_log_head(path: &Path, vault_id: &str, device_id: &str) -> Option<LogHead> {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let head = async {
        let pool = registry.connect_path_read_only(path).await?;
        let mut conn = pool
            .acquire()
            .await
            .map_err(|error| format!("open database: {error}"))?;
        log_head(&mut conn, vault_id, device_id).await
    }
    .await;
    if let Err(error) = registry.close_all().await {
        eprintln!(
            "close {} after reading its log head: {error}",
            path.display()
        );
    }
    head.unwrap_or_else(|error| {
        eprintln!("read the log head of {}: {error}", path.display());
        None
    })
}

/// Error of a rollback that would rewind the authorization head of an anchor other live
/// devices follow. The Settings error formatter recognizes it and asks the user to hand the
/// anchor over or remove those devices first.
pub(super) const ANCHOR_ROLLBACK_ERROR: &str = "this device anchors other live devices and the rollback copy is behind its authorization head; hand the anchor over or remove those devices first";

/// Refuses a rollback to the database in `copy_dir` that would rewind a live anchor. A live
/// vault whose id or head cannot be read has no head to protect, so the rollback proceeds.
async fn refuse_anchor_rewind(
    vault: &Path,
    copy_dir: &Path,
    device_id: &str,
) -> Result<(), String> {
    let vault_id = match vault_id_at(vault) {
        Ok(vault_id) => vault_id,
        Err(error) => {
            eprintln!("read the vault id before a rollback: {error}");
            return Ok(());
        }
    };
    let live = readable_log_head(&database_file(vault, ""), &vault_id, device_id).await;
    if live.is_none() {
        return Ok(());
    }
    let copy = readable_log_head(&database_file(copy_dir, ""), &vault_id, device_id).await;
    if rewinds_live_anchor(live.as_ref(), copy.as_ref()) {
        return Err(ANCHOR_ROLLBACK_ERROR.to_string());
    }
    Ok(())
}

/// Restores a database rollback of the active vault under the shared vault
/// transition guard. The caller reloads the app afterwards so every runtime
/// reopens the restored data from a clean state.
#[tauri::command]
pub async fn vault_restore_database_rollback(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    let transition = super::quiescence::reserve_vault_transition()?;
    let writable_vault = active_writable_vault_path(&app)?;
    let (copy_dir, retention) = validate_rollback(writable_vault.as_ref(), &id).await?;
    if retention.is_other_vault() {
        refuse_other_vault_rollback(
            super::encrypted_backup::PLATFORM,
            &retention,
            super::encrypted_backup::device_linked(&app)?,
        )?;
    }
    let device_id = super::ensure_device_id(&app)?;
    let quiescence = super::quiescence::begin_reserved_quiescence(
        &app,
        transition,
        super::quiescence::RunningTerminals::Block,
    )
    .await?;
    let vault: &Path = writable_vault.as_ref();
    let restored = async {
        if !retention.is_other_vault() {
            refuse_anchor_rewind(vault, &copy_dir, &device_id).await?;
        }
        restore_rollback(vault, &id, super::now_utc()).await
    }
    .await
    .and_then(|other_vault| match other_vault {
        true => super::vault_info_from_path(vault)
            .and_then(|info| super::select_quiesced_vault(&app, &info, &quiescence)),
        false => Ok(()),
    });
    let resumed = tauri::async_runtime::spawn_blocking(move || quiescence.finish())
        .await
        .map_err(|error| format!("database rollback resume worker failed: {error}"))
        .and_then(|resumed| resumed);
    match (restored, resumed) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(resume)) => {
            Err(format!("{error}; resuming runtimes also failed: {resume}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn test_vault(name: &str) -> PathBuf {
        let path = super::super::tests::unique_path(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn at(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 18, 7, 0, second).unwrap()
    }

    const BEFORE_RESET: &str = "PRAGMA application_id = 1";
    const AFTER_RESET: &str = "PRAGMA application_id = 2";

    async fn create_database(vault: &Path, marker_pragma: &'static str) {
        ganbaru_db::test_support::write_migrated_database(database_file(vault, ""))
            .await
            .unwrap();
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry
            .connect_path(database_file(vault, ""))
            .await
            .unwrap();
        sqlx::query(marker_pragma).execute(&pool).await.unwrap();
        registry.close_all().await.unwrap();
    }

    async fn marker(vault: &Path) -> i64 {
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry
            .connect_path_read_only(database_file(vault, ""))
            .await
            .unwrap();
        let row = sqlx::query("PRAGMA application_id")
            .fetch_one(&pool)
            .await
            .unwrap();
        let value: i64 = row.try_get(0).unwrap();
        registry.close_all().await.unwrap();
        value
    }

    fn rollback(id_second: u32, retention: Retention) -> DatabaseRollback {
        DatabaseRollback {
            id: rollback_id(at(id_second)),
            created_at: String::new(),
            size_bytes: 0,
            retention,
        }
    }

    fn every_retention() -> Vec<Retention> {
        let owners = [None, Some("vault-a".to_string())];
        let mut all = vec![Retention::OtherVault {
            vault: "vault-a".to_string(),
        }];
        for owner in owners {
            for flag in [false, true] {
                all.push(Retention::Reset {
                    full: flag,
                    owner: owner.clone(),
                });
                all.push(Retention::Flagged {
                    dismissed: flag,
                    owner: owner.clone(),
                });
            }
            all.push(Retention::Replacement { owner });
        }
        all
    }

    /// The retention a stored record holds, stated from the rules each copy follows.
    fn expected(stored: &StoredRetention) -> Option<Retention> {
        let owner = stored.vault.clone();
        match stored {
            StoredRetention {
                origin: RollbackOrigin::Replacement,
                flagged: false,
                dismissed: false,
                full: true,
                other_vault: true,
                vault: Some(vault),
            } => Some(Retention::OtherVault {
                vault: vault.clone(),
            }),
            StoredRetention {
                other_vault: true, ..
            } => None,
            StoredRetention {
                origin: RollbackOrigin::Reset,
                flagged: false,
                dismissed: false,
                full,
                ..
            } => Some(Retention::Reset { full: *full, owner }),
            StoredRetention {
                origin: RollbackOrigin::Replacement,
                flagged: false,
                dismissed: false,
                full: false,
                ..
            } => Some(Retention::Replacement { owner }),
            StoredRetention {
                origin: RollbackOrigin::Replacement,
                flagged: true,
                dismissed,
                full: true,
                ..
            } => Some(Retention::Flagged {
                dismissed: *dismissed,
                owner,
            }),
            _ => None,
        }
    }

    #[test]
    fn every_stored_combination_decodes_only_to_a_retention_it_can_hold() {
        let mut decoded = 0;
        for origin in [RollbackOrigin::Reset, RollbackOrigin::Replacement] {
            for bits in 0..16u8 {
                for vault in [None, Some("vault-a".to_string())] {
                    let stored = StoredRetention {
                        origin,
                        flagged: bits & 1 != 0,
                        dismissed: bits & 2 != 0,
                        full: bits & 4 != 0,
                        other_vault: bits & 8 != 0,
                        vault,
                    };
                    let want = expected(&stored);
                    let label = format!("{stored:?}");
                    let got = Retention::try_from(stored).ok();
                    assert_eq!(got, want, "{label}");
                    decoded += usize::from(got.is_some());
                }
            }
        }
        assert_eq!(decoded, every_retention().len());
    }

    #[test]
    fn every_retention_round_trips_through_its_stored_record() {
        for retention in every_retention() {
            let bytes = serde_json::to_vec(&StoredRetention::from(&retention)).unwrap();
            let stored: StoredRetention = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(Retention::try_from(stored), Ok(retention.clone()));
        }
        let record =
            serde_json::to_value(StoredRetention::from(&Retention::other_vault("v"))).unwrap();
        assert_eq!(
            record,
            serde_json::json!({
                "origin": "replacement",
                "flagged": false,
                "dismissed": false,
                "full": true,
                "otherVault": true,
                "vault": "v",
            })
        );
    }

    #[test]
    fn claiming_and_dismissing_change_only_the_owner_and_the_notice() {
        for retention in every_retention() {
            let claimed = retention.clone().claimed_by("vault-b");
            let dismissed = retention.clone().dismissed_notice();
            for changed in [&claimed, &dismissed] {
                assert_eq!(changed.origin(), retention.origin(), "{retention:?}");
                assert_eq!(changed.full(), retention.full(), "{retention:?}");
                assert_eq!(changed.flagged(), retention.flagged(), "{retention:?}");
                assert_eq!(changed.is_other_vault(), retention.is_other_vault());
            }
            match &retention {
                Retention::OtherVault { .. } => assert_eq!(claimed, retention),
                _ => assert_eq!(claimed.vault(), Some("vault-b"), "{retention:?}"),
            }
            assert_eq!(claimed.dismissed(), retention.dismissed());
            assert_eq!(dismissed.vault(), retention.vault());
            assert_eq!(dismissed.dismissed(), retention.flagged(), "{retention:?}");
        }
    }

    #[tokio::test]
    async fn a_copy_whose_retention_does_not_decode_is_kept_and_never_pruned_or_restored() {
        let vault = test_vault("undecodable-retention");
        for second in 0..(MAX_DATABASE_ROLLBACKS as u32 + 2) {
            fs::write(database_file(&vault, ""), [second as u8]).unwrap();
            move_database_to_rollback(&vault, at(second)).unwrap();
            if second == 0 {
                let oldest = rollback_root(&vault).join(rollback_id(at(0)));
                fs::write(
                    oldest.join(RETENTION_FILE),
                    br#"{"origin":"reset","flagged":true,"dismissed":false,"full":false}"#,
                )
                .unwrap();
            }
        }
        let oldest = rollback_root(&vault).join(rollback_id(at(0)));
        assert!(
            database_file(&oldest, "").exists(),
            "the unreadable copy stays"
        );
        let listed = ids(&list_rollbacks(&vault).unwrap());
        assert_eq!(listed.len(), MAX_DATABASE_ROLLBACKS);
        assert!(!listed.contains(&rollback_id(at(0))));
        write_manifest(&vault, "vault-a");
        let shown = visible_rollbacks(&vault).unwrap();
        assert_eq!(ids(&shown.restorable), listed);
        assert_eq!(
            shown.unreadable,
            vec![UnreadableRollback {
                id: rollback_id(at(0)),
                created_at: at(0).to_rfc3339_opts(SecondsFormat::Millis, true),
                size_bytes: 1,
            }],
            "the unreadable copy shows apart from the restorable ones"
        );
        let view = serde_json::to_value(&shown).unwrap();
        assert_eq!(view["unreadable"][0]["sizeBytes"], 1);
        assert!(view["unreadable"][0].get("origin").is_none());
        assert_eq!(view["restorable"][0]["origin"], "reset");
        assert!(read_retention(&oldest).is_err());
        assert!(dismiss_rollback(&vault, &rollback_id(at(0))).is_err());
        let refused = restore_rollback(&vault, &rollback_id(at(0)), at(50)).await;
        assert!(refused.unwrap_err().contains("retention"));

        fs::write(database_file(&vault, ""), b"db").unwrap();
        let newer = create_rollback(&vault, at(0), Retention::default())
            .unwrap()
            .unwrap();
        assert!(
            parse_rollback_id(&newer).unwrap() > at(MAX_DATABASE_ROLLBACKS as u32 + 1),
            "a new copy sorts after every copy on disk"
        );
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn rollback_ids_round_trip_and_reject_anything_else() {
        let id = rollback_id(at(5));
        assert_eq!(id, "20261018T070005000Z");
        assert_eq!(parse_rollback_id(&id).unwrap(), at(5));
        for value in [
            "",
            "../ganbaru.sqlite",
            "20261018T070005000Z/..",
            "20261018T070005000z",
            "2026-10-18T07:00:05Z",
            "20261018T070005000Z ",
        ] {
            assert!(parse_rollback_id(value).is_err(), "{value}");
        }
    }

    #[test]
    fn reset_moves_the_database_and_sidecars_instead_of_deleting_them() {
        let vault = test_vault("reset-moves");
        for suffix in DATABASE_FILE_SUFFIXES {
            fs::write(database_file(&vault, suffix), suffix.as_bytes()).unwrap();
        }
        let id = move_database_to_rollback(&vault, at(1)).unwrap().unwrap();
        let rollback = rollback_root(&vault).join(&id);
        #[cfg(unix)]
        for directory in [rollback_root(&vault), rollback.clone()] {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&directory).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", directory.display());
        }
        for suffix in DATABASE_FILE_SUFFIXES {
            assert!(!database_file(&vault, suffix).exists(), "{suffix}");
            assert_eq!(
                fs::read(database_file(&rollback, suffix)).unwrap(),
                suffix.as_bytes()
            );
        }
        let listed = &list_rollbacks(&vault).unwrap()[0];
        assert_eq!(listed.id, id);
        assert_eq!(listed.retention.origin(), RollbackOrigin::Reset);
        assert!(!listed.retention.flagged());
        assert_eq!(move_database_to_rollback(&vault, at(2)).unwrap(), None);
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn reset_keeps_only_the_newest_rollbacks() {
        let vault = test_vault("reset-prunes");
        for second in 0..(MAX_DATABASE_ROLLBACKS as u32 + 2) {
            fs::write(database_file(&vault, ""), [second as u8]).unwrap();
            move_database_to_rollback(&vault, at(10)).unwrap();
        }
        let rollbacks = list_rollbacks(&vault).unwrap();
        assert_eq!(rollbacks.len(), MAX_DATABASE_ROLLBACKS);
        let newest = rollback_root(&vault).join(&rollbacks[0].id);
        assert_eq!(
            fs::read(database_file(&newest, "")).unwrap(),
            [MAX_DATABASE_ROLLBACKS as u8 + 1]
        );
        let _ = fs::remove_dir_all(vault);
    }

    #[test]
    fn retention_counts_each_origin_apart() {
        let reset = Retention::default();
        let replacement = Retention::replacement(false);
        let mut rollbacks = Vec::new();
        for second in (0..10).rev() {
            rollbacks.push(rollback(
                second,
                if second < 2 {
                    reset.clone()
                } else {
                    replacement.clone()
                },
            ));
        }
        let stale = stale_rollbacks(&rollbacks, at(20));
        let expected: Vec<String> = (2..=6)
            .rev()
            .map(|second| rollback_id(at(second)))
            .collect();
        assert_eq!(stale, expected);
    }

    #[test]
    fn retention_keeps_the_earliest_undismissed_flagged_copies() {
        let flagged = Retention::replacement(true);
        let dismissed = flagged.clone().dismissed_notice();
        let rollbacks = vec![
            rollback(9, flagged.clone()),
            rollback(8, dismissed.clone()),
            rollback(7, flagged.clone()),
            rollback(6, flagged.clone()),
        ];
        let stale = stale_rollbacks(&rollbacks, at(20));
        assert_eq!(stale, vec![rollback_id(at(9)), rollback_id(at(8))]);

        let expired = vec![rollback(1, dismissed)];
        let later = at(1) + DISMISSED_FLAGGED_RETENTION + TimeDelta::seconds(1);
        assert_eq!(stale_rollbacks(&expired, later), vec![rollback_id(at(1))]);
        assert!(stale_rollbacks(&expired, at(20)).is_empty());
        let undismissed = vec![rollback(1, flagged)];
        assert!(stale_rollbacks(&undismissed, later).is_empty());
    }

    #[test]
    fn dismiss_marks_only_flagged_copies() {
        let vault = test_vault("dismiss");
        fs::write(database_file(&vault, ""), b"db").unwrap();
        let reset = move_database_to_rollback(&vault, at(1)).unwrap().unwrap();
        dismiss_rollback(&vault, &reset).unwrap();
        assert!(!list_rollbacks(&vault).unwrap()[0].retention.dismissed());

        fs::write(database_file(&vault, ""), b"db").unwrap();
        let flagged = create_rollback(&vault, at(2), Retention::replacement(true))
            .unwrap()
            .unwrap();
        dismiss_rollback(&vault, &flagged).unwrap();
        let listed = list_rollbacks(&vault).unwrap();
        assert!(listed[0].retention.flagged() && listed[0].retention.dismissed());
        assert!(dismiss_rollback(&vault, "../ganbaru.sqlite").is_err());
        let _ = fs::remove_dir_all(vault);
    }

    #[tokio::test]
    async fn restore_round_trips_and_keeps_the_replaced_database() {
        let vault = test_vault("restore-round-trip");
        create_database(&vault, BEFORE_RESET).await;
        let before = move_database_to_rollback(&vault, at(1)).unwrap().unwrap();
        create_database(&vault, AFTER_RESET).await;

        restore_rollback(&vault, &before, at(2)).await.unwrap();
        assert_eq!(marker(&vault).await, 1);
        let rollbacks = list_rollbacks(&vault).unwrap();
        assert_eq!(rollbacks.len(), 1);
        assert_ne!(rollbacks[0].id, before);

        restore_rollback(&vault, &rollbacks[0].id, at(3))
            .await
            .unwrap();
        assert_eq!(marker(&vault).await, 2);
        let _ = fs::remove_dir_all(vault);
    }

    #[tokio::test]
    async fn restore_rejects_unknown_ids_and_invalid_databases() {
        let vault = test_vault("restore-rejects");
        create_database(&vault, AFTER_RESET).await;
        assert!(
            restore_rollback(&vault, "../ganbaru.sqlite", at(1))
                .await
                .is_err()
        );
        assert!(
            restore_rollback(&vault, &rollback_id(at(9)), at(1))
                .await
                .is_err()
        );

        let forged = rollback_root(&vault).join(rollback_id(at(5)));
        fs::create_dir_all(&forged).unwrap();
        fs::write(database_file(&forged, ""), b"not a database").unwrap();
        assert!(
            restore_rollback(&vault, &rollback_id(at(5)), at(6))
                .await
                .is_err()
        );
        assert_eq!(marker(&vault).await, 2);
        let _ = fs::remove_dir_all(vault);
    }

    fn write_manifest(vault: &Path, vault_id: &str) {
        let manifest = format!(
            r#"{{"app":"ganbaru","schemaVersion":2,"vaultId":"{vault_id}","displayName":"Vault","createdAt":"2026-10-20T00:00:00Z","hasData":true}}"#
        );
        fs::write(vault.join(VAULT_MANIFEST_FILE), manifest).unwrap();
    }

    fn ids(rollbacks: &[DatabaseRollback]) -> Vec<String> {
        rollbacks
            .iter()
            .map(|rollback| rollback.id.clone())
            .collect()
    }

    #[test]
    fn retention_keeps_hidden_copies_only_with_their_whole_vault() {
        let other = |vault: &str| Retention::other_vault(vault);
        let owned = |vault: &str| Retention::default().claimed_by(vault);
        let rollbacks = vec![
            rollback(9, other("vault-c")),
            rollback(8, other("vault-b")),
            rollback(7, other("vault-a")),
            rollback(6, owned("vault-b")),
            rollback(5, owned("vault-a")),
            rollback(4, owned("vault-current")),
            rollback(3, Retention::default()),
        ];
        let stale = pruned_rollbacks(&rollbacks, Some("vault-current"), at(20));
        assert_eq!(stale, vec![rollback_id(at(7)), rollback_id(at(5))]);
        let unknown = pruned_rollbacks(&rollbacks, None, at(20));
        assert_eq!(unknown, vec![rollback_id(at(7))]);
    }

    #[tokio::test]
    async fn another_vault_replacement_keeps_and_restores_the_whole_replaced_vault() {
        let vault = test_vault("other-vault");
        write_manifest(&vault, "vault-a");
        fs::create_dir_all(vault.join("assets")).unwrap();
        fs::write(vault.join("assets").join("a.png"), b"a").unwrap();
        create_database(&vault, BEFORE_RESET).await;
        let reset_a = move_database_to_rollback(&vault, at(1)).unwrap().unwrap();
        create_database(&vault, BEFORE_RESET).await;

        let kept_a = retain_other_vault(&vault, at(2)).unwrap();
        assert!(!vault.join(VAULT_MANIFEST_FILE).exists());
        assert!(!vault.join("assets").exists());
        write_manifest(&vault, "vault-b");
        create_database(&vault, AFTER_RESET).await;

        let shown = visible_rollbacks(&vault).unwrap().restorable;
        assert_eq!(ids(&shown), vec![kept_a.clone()]);
        assert!(shown[0].retention.is_other_vault());
        assert!(
            restore_rollback(&vault, &reset_a, at(3)).await.is_err(),
            "a copy of the replaced vault never restores into another vault"
        );
        assert_eq!(marker(&vault).await, 2);

        assert!(restore_rollback(&vault, &kept_a, at(4)).await.unwrap());
        assert_eq!(vault_id_at(&vault).unwrap(), "vault-a");
        assert_eq!(fs::read(vault.join("assets").join("a.png")).unwrap(), b"a");
        assert_eq!(marker(&vault).await, 1);
        let shown = visible_rollbacks(&vault).unwrap().restorable;
        assert_eq!(shown.len(), 2);
        assert!(shown[0].retention.is_other_vault());
        assert_eq!(shown[0].retention.vault(), Some("vault-b"));
        assert_eq!(shown[1].id, reset_a);

        assert!(restore_rollback(&vault, &shown[0].id, at(5)).await.unwrap());
        assert_eq!(vault_id_at(&vault).unwrap(), "vault-b");
        assert_eq!(marker(&vault).await, 2);
        let _ = fs::remove_dir_all(vault);
    }

    #[tokio::test]
    async fn another_vault_rollback_is_refused_while_linked_on_android() {
        use super::super::encrypted_backup::{LINKED_ANDROID_RESTORE_ERROR, RestorePlatform};
        use crate::vault::handoff::receiver::{is_linked, test_linked_phone};
        let vault = test_vault("other-vault-linked");
        write_manifest(&vault, "vault-a");
        create_database(&vault, BEFORE_RESET).await;
        let kept_a = retain_other_vault(&vault, at(2)).unwrap();
        write_manifest(&vault, "vault-b");
        create_database(&vault, AFTER_RESET).await;
        let pairing_root = test_vault("other-vault-pairing");
        let phone = test_linked_phone(&pairing_root);

        let (_, other) = validate_rollback(&vault, &kept_a).await.unwrap();
        assert!(other.is_other_vault());
        let linked = is_linked(&phone).unwrap();
        assert_eq!(
            refuse_other_vault_rollback(RestorePlatform::Android, &other, linked),
            Err(LINKED_ANDROID_RESTORE_ERROR.to_string())
        );
        assert_eq!(
            refuse_other_vault_rollback(RestorePlatform::Desktop, &other, linked),
            Ok(())
        );
        let same = Retention::default().claimed_by("vault-b");
        assert_eq!(
            refuse_other_vault_rollback(RestorePlatform::Android, &same, linked),
            Ok(()),
            "a copy of the same vault is not another vault"
        );
        phone.unlink().unwrap();
        assert_eq!(
            refuse_other_vault_rollback(
                RestorePlatform::Android,
                &other,
                is_linked(&phone).unwrap()
            ),
            Ok(()),
            "a phone that unlinked restores another vault's copy"
        );
        let _ = fs::remove_dir_all(vault);
        let _ = fs::remove_dir_all(pairing_root);
    }

    #[test]
    fn another_vault_replacement_without_a_database_is_refused() {
        let vault = test_vault("other-vault-empty");
        write_manifest(&vault, "vault-a");
        assert!(retain_other_vault(&vault, at(1)).is_err());
        assert!(vault.join(VAULT_MANIFEST_FILE).exists());
        let missing = test_vault("other-vault-unmarked");
        fs::write(database_file(&missing, ""), b"db").unwrap();
        assert!(retain_other_vault(&missing, at(1)).is_err());
        assert!(database_file(&missing, "").exists());
        let _ = fs::remove_dir_all(vault);
        let _ = fs::remove_dir_all(missing);
    }

    fn enrolled(device_id: &str, removed_revision: Option<u64>) -> ganbaru_sync::EnrolledDevice {
        ganbaru_sync::EnrolledDevice {
            device_id: device_id.to_string(),
            device_key: ganbaru_sync_contracts::DevicePublicKey::from_bytes([7; 32]),
            tls_fingerprint: [0; 32],
            label: None,
            kind: ganbaru_sync_contracts::DeviceKind::Desktop,
            role: ganbaru_sync_contracts::DeviceRole::Member,
            enrolled_revision: 1,
            removed_revision,
            held: false,
            encryption_key: None,
        }
    }

    fn authorization_view(
        anchor: &str,
        devices: Vec<ganbaru_sync::EnrolledDevice>,
    ) -> AuthorizationView {
        let admin = ganbaru_sync_contracts::AdminPublicKey::from_bytes([3; 32]);
        AuthorizationView {
            epoch: 1,
            revision: 4,
            entry_hash: [1; 32],
            anchor_device_id: anchor.to_string(),
            frozen_at: None,
            catch_up: None,
            charter: [2; 32],
            recovery_admin_key: admin,
            kit_admin_key: admin,
            retired_admin_keys: std::collections::BTreeSet::new(),
            devices,
            discovery_key: [4; 32],
            previous_discovery_key: None,
            key_epoch: 0,
            recovery_encryption_key: None,
            pending_removal: None,
            server: None,
        }
    }

    #[test]
    fn log_head_counts_only_live_other_devices() {
        let view = authorization_view(
            "device-a",
            vec![
                enrolled("device-a", None),
                enrolled("device-b", None),
                enrolled("device-c", Some(3)),
            ],
        );
        let head = log_head_of(&view, "device-a");
        assert_eq!(
            head,
            LogHead {
                entry_hash: [1; 32],
                anchored_here: true,
                other_live_devices: 1,
            },
            "a removed device does not count as live"
        );
        let member = log_head_of(&view, "device-b");
        assert!(!member.anchored_here);
        assert_eq!(member.other_live_devices, 1);
    }

    fn head(entry_hash: u8, anchored_here: bool, other_live_devices: usize) -> LogHead {
        LogHead {
            entry_hash: [entry_hash; 32],
            anchored_here,
            other_live_devices,
        }
    }

    #[test]
    fn rewinds_live_anchor_refuses_only_a_different_head_with_live_devices() {
        let live = head(1, true, 2);
        assert!(
            rewinds_live_anchor(Some(&live), Some(&head(9, true, 2))),
            "a copy behind a live anchor's head rewinds it"
        );
        assert!(
            rewinds_live_anchor(Some(&live), None),
            "a copy with no log does not show it holds the anchor's head"
        );
        assert!(
            !rewinds_live_anchor(Some(&live), Some(&head(1, true, 2))),
            "an equal head rewinds nothing"
        );
        assert!(
            !rewinds_live_anchor(Some(&head(1, true, 0)), Some(&head(9, true, 0))),
            "a sole device has nobody following its head"
        );
        assert!(
            !rewinds_live_anchor(Some(&head(1, false, 2)), Some(&head(9, false, 2))),
            "a member is repaired by its outbox and the restored writer rules"
        );
        assert!(
            !rewinds_live_anchor(None, Some(&head(9, true, 2))),
            "a live vault with no readable log has no head to protect"
        );
        assert!(!rewinds_live_anchor(None, None));
    }

    #[tokio::test]
    async fn rewinds_live_anchor_lets_an_unreadable_live_database_roll_back() {
        let vault = test_vault("anchor-rewind-unreadable");
        write_manifest(&vault, "vault-a");
        create_database(&vault, BEFORE_RESET).await;
        let copy = move_database_to_rollback(&vault, at(1)).unwrap().unwrap();
        let copy_dir = rollback_root(&vault).join(&copy);
        assert_eq!(
            readable_log_head(&database_file(&copy_dir, ""), "vault-a", "device-a").await,
            None,
            "a database with no log has no head"
        );
        fs::write(database_file(&vault, ""), b"not a database").unwrap();

        assert_eq!(
            readable_log_head(&database_file(&vault, ""), "vault-a", "device-a").await,
            None
        );
        assert_eq!(
            refuse_anchor_rewind(&vault, &copy_dir, "device-a").await,
            Ok(())
        );
        restore_rollback(&vault, &copy, at(2)).await.unwrap();
        assert_eq!(marker(&vault).await, 1);
        let _ = fs::remove_dir_all(vault);
    }
}
