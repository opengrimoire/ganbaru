//! The journal that makes a rollback restore all or nothing.
//!
//! A restore moves the vault data into a new copy, then moves the chosen copy into the vault.
//! The journal is written before the first move and advances only after each move reached stable
//! storage. Recovery reads only the journal: before the chosen copy is wholly in place it moves
//! everything back, so the vault holds the data it had before the restore; after, it removes the
//! emptied copy. The database, its write-ahead log, and its shared memory always move together.

use super::{RETENTION_FILE, moved_entry_names, parse_rollback_id, prune_unchecked, rollback_root};
use crate::vault::journal_file;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Journal file inside the rollback directory. Its name is not a rollback ID, so listings and
/// retention never treat it as a copy.
const JOURNAL_FILE: &str = "restore.json";

/// Error a vault operation returns while a rollback restore or reset is left unfinished.
pub(crate) const RESTORE_RECOVERY_PENDING: &str = "an interrupted rollback restore or reset could not be finished, so the vault was left as found; see the log for the reason";

/// Serializes recoveries in this process, so two first opens never move the same entries.
static RECOVERY: Mutex<()> = Mutex::new(());

/// Where a restore stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Phase {
    /// The vault data is moving into the replaced copy; some of it may already be there.
    Replacing,
    /// The vault data is safe in the replaced copy, or there was none, and the chosen copy is
    /// moving into the vault.
    Restoring,
    /// The chosen copy is back in its directory and the vault data is moving back.
    Returning,
    /// The chosen copy is wholly in the vault and only its emptied directory remains.
    Restored,
}

/// What a restore records as it goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Event {
    /// The vault data reached the replaced copy.
    MovedOut,
    /// The chosen copy reached the vault.
    MovedIn,
    /// The chosen copy reached its directory again while recovery undoes the restore.
    CopyReturned,
}

/// The only phase changes a restore makes.
pub(super) fn transition(phase: Phase, event: Event) -> Result<Phase, String> {
    match (phase, event) {
        (Phase::Replacing, Event::MovedOut) => Ok(Phase::Restoring),
        (Phase::Restoring, Event::MovedIn) => Ok(Phase::Restored),
        (Phase::Restoring, Event::CopyReturned) => Ok(Phase::Returning),
        (phase, event) => Err(format!(
            "a rollback restore cannot record {event:?} while {phase:?}"
        )),
    }
}

/// A restore as recorded: the chosen copy `source`, the copy `replaced` that keeps the vault
/// data, `None` when the vault had no database, and whether the whole layout moves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Journal {
    pub(super) phase: Phase,
    pub(super) source: String,
    pub(super) replaced: Option<String>,
    pub(super) full: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum StoredPhase {
    Replacing,
    Restoring,
    Returning,
    Restored,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredJournal {
    pub(super) phase: StoredPhase,
    pub(super) source: String,
    pub(super) replaced: Option<String>,
    pub(super) full: bool,
}

impl From<&Journal> for StoredJournal {
    fn from(journal: &Journal) -> Self {
        Self {
            phase: match journal.phase {
                Phase::Replacing => StoredPhase::Replacing,
                Phase::Restoring => StoredPhase::Restoring,
                Phase::Returning => StoredPhase::Returning,
                Phase::Restored => StoredPhase::Restored,
            },
            source: journal.source.clone(),
            replaced: journal.replaced.clone(),
            full: journal.full,
        }
    }
}

impl TryFrom<StoredJournal> for Journal {
    type Error = String;

    /// Accepts only a journal a restore could have written: rollback IDs naming two different
    /// copies, and a replaced copy whenever the vault data was moving out.
    fn try_from(stored: StoredJournal) -> Result<Self, String> {
        parse_rollback_id(&stored.source)
            .map_err(|_| "the rollback restore journal names an unknown copy".to_string())?;
        if let Some(replaced) = &stored.replaced {
            parse_rollback_id(replaced)
                .map_err(|_| "the rollback restore journal names an unknown copy".to_string())?;
            if *replaced == stored.source {
                return Err(
                    "the rollback restore journal replaces the copy it restores".to_string()
                );
            }
        }
        let phase = match stored.phase {
            StoredPhase::Replacing if stored.replaced.is_none() => {
                return Err(
                    "the rollback restore journal moves vault data into no copy".to_string()
                );
            }
            StoredPhase::Replacing => Phase::Replacing,
            StoredPhase::Restoring => Phase::Restoring,
            StoredPhase::Returning => Phase::Returning,
            StoredPhase::Restored => Phase::Restored,
        };
        Ok(Self {
            phase,
            source: stored.source,
            replaced: stored.replaced,
            full: stored.full,
        })
    }
}

/// What recovery does for a phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Step {
    /// Move the vault data back from the replaced copy and remove that copy.
    PutBack,
    /// Move the chosen copy back into its directory.
    ReturnCopy,
    /// Remove the emptied directory of the chosen copy and apply retention.
    Finish,
}

pub(super) fn next_step(phase: Phase) -> Step {
    match phase {
        Phase::Replacing | Phase::Returning => Step::PutBack,
        Phase::Restoring => Step::ReturnCopy,
        Phase::Restored => Step::Finish,
    }
}

/// How recovery left a restore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Recovery {
    /// No restore of the vault is unfinished.
    Settled,
    /// A restore of the vault is left as found, for the reason given.
    Pending(String),
}

fn journal_path(vault: &Path) -> PathBuf {
    rollback_root(vault).join(JOURNAL_FILE)
}

/// The journal of a restore into `vault`, persisted on every phase change.
pub(super) struct RestoreRecord {
    vault: PathBuf,
    path: PathBuf,
    journal: Journal,
}

impl RestoreRecord {
    /// Records a new restore at its first phase.
    pub(super) fn begin(vault: &Path, journal: Journal) -> Result<Self, String> {
        let record = Self {
            vault: vault.to_path_buf(),
            path: journal_path(vault),
            journal,
        };
        record.write()?;
        Ok(record)
    }

    /// Reads the journal of `vault`: `None` when there is none, and an inner error when it
    /// cannot be read or decoded.
    fn load(vault: &Path) -> Option<Result<Self, String>> {
        let path = journal_path(vault);
        let bytes = match journal_file::read(&path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return None,
            Err(error) => return Some(Err(format!("read the rollback restore journal: {error}"))),
        };
        Some(
            serde_json::from_slice::<StoredJournal>(&bytes)
                .map_err(|error| format!("decode the rollback restore journal: {error}"))
                .and_then(Journal::try_from)
                .map(|journal| Self {
                    vault: vault.to_path_buf(),
                    path,
                    journal,
                }),
        )
    }

    fn write(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(&StoredJournal::from(&self.journal))
            .map_err(|error| format!("encode the rollback restore journal: {error}"))?;
        journal_file::write(&self.path, &bytes)
            .map_err(|error| format!("write the rollback restore journal: {error}"))
    }

    /// Records `event`, the only way a restore changes phase.
    pub(super) fn advance(&mut self, event: Event) -> Result<(), String> {
        let phase = transition(self.journal.phase, event)?;
        let previous = std::mem::replace(&mut self.journal.phase, phase);
        if let Err(error) = self.write() {
            self.journal.phase = previous;
            return Err(error);
        }
        Ok(())
    }

    fn close(self) -> Result<(), String> {
        journal_file::remove(&self.path)
            .map_err(|error| format!("remove the rollback restore journal: {error}"))
    }

    fn names(&self) -> Vec<String> {
        moved_entry_names(self.journal.full)
    }

    fn copy(&self, id: &str) -> PathBuf {
        rollback_root(&self.vault).join(id)
    }

    /// Drives the restore to a settled vault from its recorded phase, flushing every move before
    /// the next phase is recorded. Retention uses `now`.
    pub(super) fn resume(mut self, now: DateTime<Utc>) -> Result<Recovery, String> {
        loop {
            match next_step(self.journal.phase) {
                Step::PutBack => {
                    if let Some(reason) = self.put_back()? {
                        return Ok(Recovery::Pending(reason));
                    }
                    self.close()?;
                    return Ok(Recovery::Settled);
                }
                Step::ReturnCopy => {
                    let source = self.copy(&self.journal.source);
                    if !fs::symlink_metadata(&source).is_ok_and(|meta| meta.is_dir()) {
                        return Ok(Recovery::Pending(
                            "the copy being restored is missing".to_string(),
                        ));
                    }
                    if let Some(reason) = move_back(&self.vault, &source, &self.names())? {
                        return Ok(Recovery::Pending(reason));
                    }
                    flush(&[&source, &rollback_root(&self.vault), &self.vault])?;
                    self.advance(Event::CopyReturned)?;
                }
                // The vault data is whole, so cleanup failures are logged rather than keeping
                // the vault closed. The emptied copy holds no database, so listings ignore it.
                Step::Finish => {
                    match fs::remove_dir_all(self.copy(&self.journal.source)) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            eprintln!("Rollback restore cleanup: remove restored rollback: {error}")
                        }
                    }
                    if let Err(error) = prune_unchecked(&self.vault, now) {
                        eprintln!("Rollback restore cleanup: {error}");
                    }
                    self.close()?;
                    return Ok(Recovery::Settled);
                }
            }
        }
    }

    /// Moves the vault data back from the replaced copy and removes that copy, which then holds
    /// at most its retention record. Returns a reason when the layout is not one a restore
    /// leaves.
    fn put_back(&self) -> Result<Option<String>, String> {
        let Some(replaced) = &self.journal.replaced else {
            return Ok(None);
        };
        let directory = self.copy(replaced);
        if fs::symlink_metadata(&directory).is_err() {
            return Ok(None);
        }
        if let Some(reason) = move_back(&directory, &self.vault, &self.names())? {
            return Ok(Some(reason));
        }
        flush(&[&self.vault, &directory])?;
        match fs::remove_file(directory.join(RETENTION_FILE)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove the replaced copy retention: {error}")),
        }
        if let Err(error) = fs::remove_dir(&directory) {
            return Ok(Some(format!(
                "the replaced copy holds entries a restore never moves: {error}"
            )));
        }
        flush(&[&rollback_root(&self.vault)])?;
        Ok(None)
    }
}

/// Moves each of `names` found in `from` to `to`. A name present in both places is a layout no
/// restore leaves, so nothing moves and the reason is returned.
pub(super) fn move_back(
    from: &Path,
    to: &Path,
    names: &[String],
) -> Result<Option<String>, String> {
    let present: Vec<&String> = names
        .iter()
        .filter(|name| fs::symlink_metadata(from.join(name)).is_ok())
        .collect();
    if let Some(name) = present
        .iter()
        .find(|name| fs::symlink_metadata(to.join(name)).is_ok())
    {
        return Ok(Some(format!(
            "'{name}' is in both the vault and the copy it moved to"
        )));
    }
    for name in present {
        fs::rename(from.join(name), to.join(name))
            .map_err(|error| format!("move vault entry back: {error}"))?;
    }
    Ok(None)
}

pub(super) fn flush(directories: &[&Path]) -> Result<(), String> {
    directories
        .iter()
        .try_for_each(|directory| ganbaru_atomic_files::sync_directory_entries(directory))
        .map_err(|error| format!("flush the rollback restore: {error}"))
}

/// Whether a restore into `vault` or a reset of it left its journal, so it is still unfinished
/// or pending.
pub(crate) fn is_pending(vault: &Path) -> bool {
    journal_file::exists(&journal_path(vault)) || super::reset_journal::is_pending(vault)
}

/// Settles an unfinished reset of `vault` and an unfinished restore into it, if any. Undecodable
/// journals and layouts neither would leave are reported as pending and left untouched.
pub(crate) fn recover(vault: &Path) -> Result<Recovery, String> {
    let _recovery = RECOVERY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = crate::vault::now_utc();
    if let Recovery::Pending(reason) = super::reset_journal::recover(vault, now)? {
        return Ok(Recovery::Pending(reason));
    }
    match RestoreRecord::load(vault) {
        None => Ok(Recovery::Settled),
        Some(Err(error)) => Ok(Recovery::Pending(error)),
        Some(Ok(record)) => record.resume(now),
    }
}

/// Settles an unfinished reset or restore of `vault`, refusing with [`RESTORE_RECOVERY_PENDING`]
/// when one is left pending or fails.
pub(crate) fn recover_or_refuse(vault: &Path) -> Result<(), String> {
    match recover(vault) {
        Ok(Recovery::Settled) => Ok(()),
        Ok(Recovery::Pending(reason)) => {
            eprintln!("Rollback recovery: {reason}");
            Err(RESTORE_RECOVERY_PENDING.to_string())
        }
        Err(error) => {
            eprintln!("Rollback recovery: {error}");
            Err(RESTORE_RECOVERY_PENDING.to_string())
        }
    }
}

/// Refuses with [`RESTORE_RECOVERY_PENDING`] while a reset or restore of `vault` is unfinished.
pub(super) fn refuse_pending(vault: &Path) -> Result<(), String> {
    if is_pending(vault) {
        return Err(RESTORE_RECOVERY_PENDING.to_string());
    }
    Ok(())
}

#[cfg(test)]
#[path = "restore_journal_tests.rs"]
mod tests;
