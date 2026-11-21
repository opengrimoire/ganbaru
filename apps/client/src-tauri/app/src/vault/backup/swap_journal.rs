//! Crash-safe swap of a staged vault folder into the place of another one.
//!
//! A swap renames the target to a rollback folder beside it, then the staged folder to the
//! target, then decides what becomes of the replaced folder. Each step is recorded first in a
//! journal beside the target, outside every vault, so a crash or exit at any point resumes
//! from the recorded phase at the next start. A folder no journal names is never moved,
//! merged, or deleted.

use super::{discard_replaced_folder, finish_replacement, holds_vault, kept_previous_path};
use crate::vault::{database_rollback, vault_info_from_path};
use ganbaru_atomic_files::{FileAccess, FinalSymlink};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Name prefix of the journal and the rollback folder of a swap beside the target.
const SWAP_PREFIX: &str = ".ganbaru.swap-";
/// Largest journal read; the real file is a few hundred bytes.
const MAX_JOURNAL_BYTES: u64 = 4 * 1024;

/// Refusal of a swap while an earlier swap of the same target cannot be finished here.
pub(crate) const SWAP_RECOVERY_PENDING: &str =
    "a previous vault replacement still requires recovery";

/// What becomes of the folder a swap replaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Replacement {
    /// The same vault: its database, or its whole layout when `flagged` marks a drastic shrink,
    /// becomes a rollback copy inside the new folder.
    SameVault { flagged: bool },
    /// Another vault a restore replaces: it becomes a whole rollback copy inside the new folder.
    OtherVault,
    /// Another vault a handoff replaces: it moves whole to `folder` beside the target, and is
    /// deleted once the swap settles only with `discard`.
    Preserve { folder: PathBuf, discard: bool },
}

/// The kind of a swap as its journal records it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Replaced {
    SameVault { flagged: bool },
    OtherVault,
    Preserve { discard: bool },
}

impl Replaced {
    /// Whether the replaced folder becomes a rollback copy inside the new folder.
    fn retained(self) -> bool {
        !matches!(self, Self::Preserve { .. })
    }
}

/// How far a swap got. Each phase is durable before the step after it starts.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Phase {
    /// The renames may be partly done.
    Swapping,
    /// Both renames are on stable storage; the replaced folder is untouched.
    Activated,
    /// Keeping the replaced folder as a rollback copy started and may be partly done.
    Retaining,
    /// The rollback copy `copy` is on stable storage, or the replaced folder held no database
    /// to keep when it is `None`; what remains in the replaced folder is not vault data.
    Retained { copy: Option<String> },
}

/// A step the swap records.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Event {
    Renamed,
    RetentionStarted,
    RetentionKept { copy: Option<String> },
}

/// The phase after `event`, or why `replaced` cannot take it in `phase`.
fn transition(replaced: Replaced, phase: &Phase, event: Event) -> Result<Phase, String> {
    match (phase, event) {
        (Phase::Swapping, Event::Renamed) => Ok(Phase::Activated),
        (Phase::Activated, Event::RetentionStarted) if replaced.retained() => Ok(Phase::Retaining),
        (Phase::Retaining, Event::RetentionKept { copy }) => Ok(Phase::Retained { copy }),
        (phase, event) => Err(format!(
            "a vault swap of {replaced:?} cannot record {event:?} while {phase:?}"
        )),
    }
}

/// A swap in progress. Folder names are relative to the parent of the target.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Journal {
    phase: Phase,
    replaced: Replaced,
    /// The vault the staged folder holds.
    vault: String,
    rollback: String,
    staging: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum StoredPhase {
    Swapping,
    Activated,
    Retaining,
    Retained,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum StoredKind {
    SameVault,
    OtherVault,
    Preserve,
}

/// The journal as its file stores it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredJournal {
    phase: StoredPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    copy: Option<String>,
    replaced: StoredKind,
    flagged: bool,
    discard: bool,
    vault: String,
    rollback: String,
    staging: String,
}

impl From<&Journal> for StoredJournal {
    fn from(journal: &Journal) -> Self {
        let (phase, copy) = match &journal.phase {
            Phase::Swapping => (StoredPhase::Swapping, None),
            Phase::Activated => (StoredPhase::Activated, None),
            Phase::Retaining => (StoredPhase::Retaining, None),
            Phase::Retained { copy } => (StoredPhase::Retained, copy.clone()),
        };
        let (replaced, flagged, discard) = match journal.replaced {
            Replaced::SameVault { flagged } => (StoredKind::SameVault, flagged, false),
            Replaced::OtherVault => (StoredKind::OtherVault, false, false),
            Replaced::Preserve { discard } => (StoredKind::Preserve, false, discard),
        };
        Self {
            phase,
            copy,
            replaced,
            flagged,
            discard,
            vault: journal.vault.clone(),
            rollback: journal.rollback.clone(),
            staging: journal.staging.clone(),
        }
    }
}

impl TryFrom<StoredJournal> for Journal {
    type Error = String;

    /// Refuses every field combination no swap records.
    fn try_from(stored: StoredJournal) -> Result<Self, String> {
        let inconsistent =
            |detail: &str| Err(format!("vault swap journal is inconsistent: {detail}"));
        let replaced = match (stored.replaced, stored.flagged, stored.discard) {
            (StoredKind::SameVault, flagged, false) => Replaced::SameVault { flagged },
            (StoredKind::OtherVault, false, false) => Replaced::OtherVault,
            (StoredKind::Preserve, false, discard) => Replaced::Preserve { discard },
            (StoredKind::SameVault | StoredKind::OtherVault, _, true) => {
                return inconsistent("only a preserved folder is discarded");
            }
            (StoredKind::OtherVault | StoredKind::Preserve, true, _) => {
                return inconsistent("only a copy of the same vault is flagged");
            }
        };
        let phase = match (stored.phase, stored.copy) {
            (StoredPhase::Swapping, None) => Phase::Swapping,
            (StoredPhase::Activated, None) => Phase::Activated,
            (StoredPhase::Retaining, None) => Phase::Retaining,
            (StoredPhase::Retained, copy) => Phase::Retained { copy },
            (_, Some(_)) => return inconsistent("only a retained swap names its copy"),
        };
        if !replaced.retained() && matches!(phase, Phase::Retaining | Phase::Retained { .. }) {
            return inconsistent("a preserved folder is never retained");
        }
        if let Phase::Retained { copy: Some(copy) } = &phase
            && !is_folder_name(copy)
        {
            return inconsistent("the rollback copy is not a single name");
        }
        if stored.vault.trim().is_empty() {
            return inconsistent("the staged vault is unnamed");
        }
        if !is_folder_name(&stored.rollback) || !is_folder_name(&stored.staging) {
            return inconsistent("a folder name is not a single name");
        }
        if stored.rollback == stored.staging {
            return inconsistent("the rollback and staged folders are the same");
        }
        Ok(Self {
            phase,
            replaced,
            vault: stored.vault,
            rollback: stored.rollback,
            staging: stored.staging,
        })
    }
}

/// Whether `name` names one folder directly inside a parent.
fn is_folder_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(only)), None) if only == name
    )
}

/// What a folder a journal names holds now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Folder {
    Missing,
    /// It holds the vault the journal names.
    Staged,
    /// It exists and holds another vault, or no readable one.
    Other,
}

/// The folders of a swap as recovery finds them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Observed {
    target: Folder,
    rollback: Folder,
    staging: bool,
    /// Whether the rollback copy a retained swap names is still in the target or the replaced
    /// folder. True when the journal names none.
    copy_kept: bool,
}

/// What recovery does next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    /// Nothing is left to do; the journal goes.
    Forget,
    /// The target is missing: the rollback folder goes back in its place.
    PutBack,
    /// The replaced folder becomes a rollback copy inside itself.
    Retain,
    /// The replaced folder stays whole as a visible sibling of the target.
    KeepWhole,
    /// The rollback copy is kept; the rest of the replaced folder is finished.
    Finish,
    /// The preserved folder is deleted.
    Discard,
    /// The folders are in a state no swap leaves; everything stays and the journal remains.
    Pending,
}

/// The step that settles a swap of `replaced` recorded at `phase` whose folders are `observed`.
/// No step deletes or merges a folder unless the journal proves the swap reached the point where
/// that is the intended outcome, and the target holds the staged vault.
fn next_step(phase: &Phase, replaced: Replaced, observed: Observed) -> Step {
    let Observed {
        target,
        rollback,
        staging,
        copy_kept,
    } = observed;
    if *phase == Phase::Swapping {
        return match (target, rollback, staging) {
            (Folder::Missing, Folder::Missing, _) => Step::Pending,
            (Folder::Missing, _, _) => Step::PutBack,
            (_, Folder::Missing, _) => Step::Forget,
            (_, _, true) | (Folder::Other, _, false) => Step::Pending,
            (Folder::Staged, _, false) => next_step(&Phase::Activated, replaced, observed),
        };
    }
    if target != Folder::Staged {
        return Step::Pending;
    }
    if rollback == Folder::Missing {
        return Step::Forget;
    }
    match (phase, replaced) {
        (Phase::Swapping | Phase::Activated, Replaced::Preserve { discard: true }) => Step::Discard,
        (Phase::Swapping | Phase::Activated, Replaced::Preserve { discard: false }) => Step::Forget,
        (Phase::Swapping | Phase::Activated, Replaced::SameVault { .. }) => match rollback {
            Folder::Staged => Step::Retain,
            _ => Step::KeepWhole,
        },
        (Phase::Swapping | Phase::Activated, Replaced::OtherVault) => Step::Retain,
        (Phase::Retaining, _) => Step::KeepWhole,
        (Phase::Retained { .. }, _) if copy_kept => Step::Finish,
        (Phase::Retained { .. }, _) => Step::KeepWhole,
    }
}

/// How recovery left a swap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Recovery {
    /// No swap of the target is unfinished.
    Settled,
    /// A swap of the target is left as found, for the reason given.
    Pending(String),
}

/// The journal of `target` and the folders it names, persisted on every phase change.
struct SwapRecord {
    path: PathBuf,
    parent: PathBuf,
    target: PathBuf,
    journal: Journal,
}

impl SwapRecord {
    fn new(target: &Path, journal: Journal) -> Result<Self, String> {
        Ok(Self {
            path: journal_path(target)?,
            parent: parent_of(target)?.to_path_buf(),
            target: target.to_path_buf(),
            journal,
        })
    }

    /// Records a new swap at its first phase.
    fn begin(target: &Path, journal: Journal) -> Result<Self, String> {
        let record = Self::new(target, journal)?;
        record.write()?;
        Ok(record)
    }

    /// Reads the journal of `target`: `None` when there is none, and an inner error when it
    /// cannot be read or decoded.
    fn load(target: &Path) -> Result<Option<Result<Self, String>>, String> {
        let path = journal_path(target)?;
        let bytes = match ganbaru_atomic_files::read_regular_file_capped(
            &path,
            MAX_JOURNAL_BYTES,
            FinalSymlink::Refuse,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Ok(Some(Err(format!("read the vault swap journal: {error}")))),
        };
        let journal = serde_json::from_slice::<StoredJournal>(&bytes)
            .map_err(|error| format!("decode the vault swap journal: {error}"))
            .and_then(Journal::try_from);
        Ok(Some(match journal {
            Ok(journal) => Ok(Self::new(target, journal)?),
            Err(error) => Err(error),
        }))
    }

    fn rollback(&self) -> PathBuf {
        self.parent.join(&self.journal.rollback)
    }

    fn write(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(&StoredJournal::from(&self.journal))
            .map_err(|error| format!("encode the vault swap journal: {error}"))?;
        ganbaru_atomic_files::write_file_atomically(&self.path, &bytes, FileAccess::OwnerOnly)
            .map_err(|error| format!("write the vault swap journal: {error}"))
    }

    /// Records `event`, the only way a swap changes phase.
    fn advance(&mut self, event: Event) -> Result<(), String> {
        let phase = transition(self.journal.replaced, &self.journal.phase, event)?;
        let previous = std::mem::replace(&mut self.journal.phase, phase);
        if let Err(error) = self.write() {
            self.journal.phase = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Removes the journal once nothing is left to do.
    fn close(self) -> Result<(), String> {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("remove the vault swap journal: {error}")),
        }
        ganbaru_atomic_files::sync_directory_entries(&self.parent)
            .map_err(|error| format!("flush the vault swap journal removal: {error}"))
    }

    fn observe(&self) -> Observed {
        let rollback = self.rollback();
        let folder = |path: &Path| match fs::symlink_metadata(path) {
            Err(_) => Folder::Missing,
            Ok(_) if holds_vault(path, &self.journal.vault) => Folder::Staged,
            Ok(_) => Folder::Other,
        };
        let copy_kept = match &self.journal.phase {
            Phase::Retained { copy: Some(copy) } => {
                database_rollback::holds_rollback(&self.target, copy)
                    || database_rollback::holds_rollback(&rollback, copy)
            }
            _ => true,
        };
        Observed {
            target: folder(&self.target),
            rollback: folder(&rollback),
            staging: fs::symlink_metadata(self.parent.join(&self.journal.staging)).is_ok(),
            copy_kept,
        }
    }

    /// Runs recovery steps until the swap is settled or left pending.
    fn resume(mut self) -> Result<Recovery, String> {
        loop {
            let step = next_step(&self.journal.phase, self.journal.replaced, self.observe());
            let rollback = self.rollback();
            match step {
                Step::Forget => break,
                Step::Pending => {
                    return Ok(Recovery::Pending(format!(
                        "the folders beside '{}' are not where its {:?} swap left them",
                        self.target.display(),
                        self.journal.phase
                    )));
                }
                Step::PutBack => {
                    fs::rename(&rollback, &self.target).map_err(|error| {
                        format!("recover interrupted vault replacement: {error}")
                    })?;
                    ganbaru_atomic_files::sync_directory_entries(&self.parent)
                        .map_err(|error| format!("flush the recovered vault: {error}"))?;
                    break;
                }
                Step::Retain => {
                    if self.journal.phase == Phase::Swapping {
                        self.advance(Event::Renamed)?;
                    }
                    self.advance(Event::RetentionStarted)?;
                    match self.retain(&rollback) {
                        Ok(copy) => self.advance(Event::RetentionKept { copy })?,
                        Err(error) => {
                            keep_whole(&self.target, &rollback, &error)?;
                            break;
                        }
                    }
                }
                Step::KeepWhole => {
                    keep_whole(
                        &self.target,
                        &rollback,
                        "its rollback copy may be incomplete",
                    )?;
                    break;
                }
                Step::Finish => {
                    finish_replacement(&self.target, &rollback)?;
                    if let Err(error) = database_rollback::prune_after_replacement(
                        &self.target,
                        crate::vault::now_utc(),
                    ) {
                        eprintln!("Vault replacement cleanup: {error}");
                    }
                    break;
                }
                Step::Discard => {
                    discard_replaced_folder(&self.target, &rollback).map_err(|error| {
                        format!(
                            "remove the replaced vault folder '{}': {error}",
                            rollback.display()
                        )
                    })?;
                    break;
                }
            }
        }
        self.close()?;
        Ok(Recovery::Settled)
    }

    /// Keeps the replaced folder as a rollback copy inside itself, returning the copy, or `None`
    /// when a replaced copy of the same vault holds no database to keep.
    fn retain(&self, rollback: &Path) -> Result<Option<String>, String> {
        let now = crate::vault::now_utc();
        match self.journal.replaced {
            Replaced::SameVault { flagged } => database_rollback::retain_replaced_vault(
                rollback,
                &self.journal.vault,
                flagged,
                now,
            ),
            Replaced::OtherVault => database_rollback::retain_other_vault(rollback, now).map(Some),
            Replaced::Preserve { .. } => Err("a preserved folder is never retained".to_string()),
        }
    }
}

/// Keeps `replaced` whole as a visible sibling of `target` instead of finishing it.
fn keep_whole(target: &Path, replaced: &Path, reason: &str) -> Result<(), String> {
    if fs::symlink_metadata(replaced).is_err() {
        return Ok(());
    }
    let kept = kept_previous_path(target)?;
    fs::rename(replaced, &kept)
        .map_err(|error| format!("{reason}; keep the replaced folder: {error}"))?;
    eprintln!(
        "Vault replacement kept the replaced folder in '{}': {reason}",
        kept.display()
    );
    Ok(())
}

fn parent_of(target: &Path) -> Result<&Path, String> {
    target
        .parent()
        .ok_or_else(|| "Ganbaru folder has no parent directory".to_string())
}

fn target_name(target: &Path) -> Result<&str, String> {
    target
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_folder_name(name))
        .ok_or_else(|| "the Ganbaru folder name is not valid text".to_string())
}

fn journal_path(target: &Path) -> Result<PathBuf, String> {
    Ok(parent_of(target)?.join(format!("{SWAP_PREFIX}{}.json", target_name(target)?)))
}

/// The folder that holds the replaced vault during a swap whose replaced folder is retained.
/// It is named after the target, so vaults sharing a parent folder never share it.
fn retained_rollback_path(target: &Path) -> Result<PathBuf, String> {
    Ok(parent_of(target)?.join(format!("{SWAP_PREFIX}{}.replaced", target_name(target)?)))
}

/// The name of `path` when it is directly inside `parent` and is not `target` itself.
fn sibling_name(path: &Path, parent: &Path, target: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let beside = path.parent() == Some(parent) && is_folder_name(name);
    (beside && target.file_name() != path.file_name()).then(|| name.to_string())
}

/// Settles an unfinished swap of `target`, if any. Undecodable journals and folders a swap would
/// not leave are reported as pending and left untouched.
pub(crate) fn recover(target: &Path) -> Result<Recovery, String> {
    match SwapRecord::load(target)? {
        None => Ok(Recovery::Settled),
        Some(Err(error)) => Ok(Recovery::Pending(error)),
        Some(Ok(record)) => record.resume(),
    }
}

/// Whether a swap of `target` left its journal, so it is still unfinished or pending.
pub(crate) fn has_unfinished_swap(target: &Path) -> bool {
    journal_path(target).is_ok_and(|path| fs::symlink_metadata(path).is_ok())
}

/// Settles an unfinished swap of `target`, refusing with [`SWAP_RECOVERY_PENDING`] when it is
/// left pending.
pub(crate) fn recover_or_refuse(target: &Path) -> Result<(), String> {
    match recover(target)? {
        Recovery::Settled => Ok(()),
        Recovery::Pending(reason) => {
            eprintln!("Vault replacement recovery: {reason}");
            Err(SWAP_RECOVERY_PENDING.to_string())
        }
    }
}

/// Moves `staging` into the place of `target`, both directly inside one parent folder, and keeps
/// the replaced folder as `replacement` asks. An earlier unfinished swap of `target` is settled
/// first. A replaced folder that is retained is settled before this returns; a preserved one is
/// settled by the next [`recover`], which the caller runs once the new vault is in use.
pub(crate) fn swap(staging: &Path, target: &Path, replacement: Replacement) -> Result<(), String> {
    let parent = parent_of(target)?;
    let (replaced, rollback) = match replacement {
        Replacement::SameVault { flagged } => (
            Replaced::SameVault { flagged },
            retained_rollback_path(target)?,
        ),
        Replacement::OtherVault => (Replaced::OtherVault, retained_rollback_path(target)?),
        Replacement::Preserve { folder, discard } => (Replaced::Preserve { discard }, folder),
    };
    let (Some(staging_name), Some(rollback_name)) = (
        sibling_name(staging, parent, target),
        sibling_name(&rollback, parent, target),
    ) else {
        return Err("a vault swap needs its folders side by side".to_string());
    };
    recover_or_refuse(target)?;
    // A swap carries the rollback directory along, so a restore into the target settles first.
    database_rollback::recover_restore_or_refuse(target)?;
    if fs::symlink_metadata(&rollback).is_ok() {
        eprintln!(
            "Vault replacement refused: '{}' is taken by a folder no swap recorded",
            rollback.display()
        );
        return Err(SWAP_RECOVERY_PENDING.to_string());
    }
    if fs::symlink_metadata(target).is_err() {
        return fs::rename(staging, target)
            .map_err(|error| format!("activate replacement data: {error}"));
    }
    let vault = vault_info_from_path(staging)?.vault_id;
    let mut record = SwapRecord::begin(
        target,
        Journal {
            phase: Phase::Swapping,
            replaced,
            vault,
            rollback: rollback_name,
            staging: staging_name,
        },
    )?;
    if let Err(error) = fs::rename(target, &rollback) {
        record.close()?;
        return Err(format!("prepare current data for replacement: {error}"));
    }
    if let Err(error) = fs::rename(staging, target) {
        fs::rename(&rollback, target).map_err(|restore| {
            format!("activate replacement data: {error}; put the current data back: {restore}")
        })?;
        record.close()?;
        return Err(format!("activate replacement data: {error}"));
    }
    let durable = ganbaru_atomic_files::sync_directory_entries(parent)
        .map_err(|error| format!("flush the vault swap: {error}"))
        .and_then(|()| record.advance(Event::Renamed));
    if let Err(error) = durable {
        eprintln!("Vault replacement finishes at the next start: {error}");
        return Ok(());
    }
    if replaced.retained() {
        match record.resume() {
            Ok(Recovery::Settled) => {}
            Ok(Recovery::Pending(reason)) => eprintln!("Vault replacement cleanup: {reason}"),
            Err(error) => eprintln!("Vault replacement cleanup: {error}"),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "swap_journal_tests.rs"]
mod tests;
