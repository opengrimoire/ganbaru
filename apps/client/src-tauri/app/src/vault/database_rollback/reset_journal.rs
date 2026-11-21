//! The journal that makes Reset local data all or nothing.
//!
//! A reset moves the vault database, its write-ahead log, and its shared memory into a new reset
//! copy. The journal is written before the first move and advances only once every move reached
//! stable storage. Recovery reads only the journal: before the moves are durable it moves the
//! files back, so the vault keeps the data it had; after, it finishes the reset. The database,
//! its write-ahead log, and its shared memory always end up together.

use super::restore_journal::{Recovery, flush, move_back};
use super::{moved_entry_names, parse_rollback_id, prune_unchecked, rollback_root};
use crate::vault::{VAULT_MANIFEST_FILE, journal_file};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Journal file inside the rollback directory. Its name is not a rollback ID, so listings and
/// retention never treat it as a copy.
const JOURNAL_FILE: &str = "reset.json";

/// Where a reset stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Phase {
    /// The vault database is moving into the reset copy; some of its files may already be there.
    Moving,
    /// The vault database is wholly and durably in the reset copy.
    Moved,
}

/// What a reset records as it goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Event {
    /// The vault database reached the reset copy.
    MovedOut,
}

/// The only phase change a reset makes.
pub(super) fn transition(phase: Phase, event: Event) -> Result<Phase, String> {
    match (phase, event) {
        (Phase::Moving, Event::MovedOut) => Ok(Phase::Moved),
        (phase, event) => Err(format!("a reset cannot record {event:?} while {phase:?}")),
    }
}

/// What recovery does for a phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Step {
    /// Move the database files back from the reset copy and remove that copy.
    PutBack,
    /// Keep the reset copy and apply retention.
    Finish,
}

pub(super) fn next_step(phase: Phase) -> Step {
    match phase {
        Phase::Moving => Step::PutBack,
        Phase::Moved => Step::Finish,
    }
}

/// A reset as recorded: its phase and the reset copy `copy` the database moves into.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Journal {
    pub(super) phase: Phase,
    pub(super) copy: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum StoredPhase {
    Moving,
    Moved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredJournal {
    pub(super) phase: StoredPhase,
    pub(super) copy: String,
}

impl From<&Journal> for StoredJournal {
    fn from(journal: &Journal) -> Self {
        Self {
            phase: match journal.phase {
                Phase::Moving => StoredPhase::Moving,
                Phase::Moved => StoredPhase::Moved,
            },
            copy: journal.copy.clone(),
        }
    }
}

impl TryFrom<StoredJournal> for Journal {
    type Error = String;

    /// Accepts only a journal naming a copy by its rollback ID.
    fn try_from(stored: StoredJournal) -> Result<Self, String> {
        parse_rollback_id(&stored.copy)
            .map_err(|_| "the reset journal names an unknown copy".to_string())?;
        Ok(Self {
            phase: match stored.phase {
                StoredPhase::Moving => Phase::Moving,
                StoredPhase::Moved => Phase::Moved,
            },
            copy: stored.copy,
        })
    }
}

fn journal_path(vault: &Path) -> PathBuf {
    rollback_root(vault).join(JOURNAL_FILE)
}

/// The journal of a reset of `vault`, persisted on every phase change.
pub(super) struct ResetRecord {
    vault: PathBuf,
    path: PathBuf,
    journal: Journal,
}

impl ResetRecord {
    /// Records a new reset into the existing empty copy `copy`.
    #[cfg(any(test, desktop))]
    pub(super) fn begin(vault: &Path, copy: &str) -> Result<Self, String> {
        Self::begin_at(
            vault,
            Journal {
                phase: Phase::Moving,
                copy: copy.to_string(),
            },
        )
    }

    /// Records a reset at `journal`, which tests use to leave the state of a crash.
    #[cfg(any(test, desktop))]
    pub(super) fn begin_at(vault: &Path, journal: Journal) -> Result<Self, String> {
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
            Err(error) => return Some(Err(format!("read the reset journal: {error}"))),
        };
        Some(
            serde_json::from_slice::<StoredJournal>(&bytes)
                .map_err(|error| format!("decode the reset journal: {error}"))
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
            .map_err(|error| format!("encode the reset journal: {error}"))?;
        journal_file::write(&self.path, &bytes)
            .map_err(|error| format!("write the reset journal: {error}"))
    }

    /// Records `event`, the only way a reset changes phase.
    #[cfg(any(test, desktop))]
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
            .map_err(|error| format!("remove the reset journal: {error}"))
    }

    /// Drives the reset to a settled vault from its recorded phase. Retention uses `now`.
    pub(super) fn resume(self, now: DateTime<Utc>) -> Result<Recovery, String> {
        match next_step(self.journal.phase) {
            Step::PutBack => {
                if let Some(reason) = self.put_back()? {
                    return Ok(Recovery::Pending(reason));
                }
            }
            // The marker stops recording data before the journal closes, so the next open starts
            // the vault empty instead of refusing it. The reset copy is whole, so a retention
            // failure is logged rather than keeping the vault closed.
            Step::Finish => {
                clear_recorded_data(&self.vault)?;
                if let Err(error) = prune_unchecked(&self.vault, now) {
                    eprintln!("Reset cleanup: {error}");
                }
            }
        }
        self.close()?;
        Ok(Recovery::Settled)
    }

    /// Moves the database files back from the reset copy and removes that copy. Returns a reason
    /// when the layout is not one a reset leaves.
    fn put_back(&self) -> Result<Option<String>, String> {
        let directory = rollback_root(&self.vault).join(&self.journal.copy);
        if fs::symlink_metadata(&directory).is_err() {
            return Ok(None);
        }
        if let Some(reason) = move_back(&directory, &self.vault, &moved_entry_names(false))? {
            return Ok(Some(reason));
        }
        flush(&[&self.vault, &directory])?;
        if let Err(error) = fs::remove_dir(&directory) {
            return Ok(Some(format!(
                "the reset copy holds entries a reset never moves: {error}"
            )));
        }
        flush(&[&rollback_root(&self.vault)])?;
        Ok(None)
    }
}

/// Records in the marker of `vault` that it holds no data. A folder without a marker records
/// nothing, so there is nothing to clear.
fn clear_recorded_data(vault: &Path) -> Result<(), String> {
    match fs::symlink_metadata(vault.join(VAULT_MANIFEST_FILE)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => crate::vault::record_vault_data(vault, false),
    }
}

/// Whether a reset of `vault` left its journal, so it is still unfinished or pending.
pub(super) fn is_pending(vault: &Path) -> bool {
    journal_file::exists(&journal_path(vault))
}

/// Settles an unfinished reset of `vault`, if any. Undecodable journals and layouts a reset would
/// not leave are reported as pending and left untouched. The caller serializes recoveries.
pub(super) fn recover(vault: &Path, now: DateTime<Utc>) -> Result<Recovery, String> {
    match ResetRecord::load(vault) {
        None => Ok(Recovery::Settled),
        Some(Err(error)) => Ok(Recovery::Pending(error)),
        Some(Ok(record)) => record.resume(now),
    }
}

#[cfg(test)]
#[path = "reset_journal_tests.rs"]
mod tests;
