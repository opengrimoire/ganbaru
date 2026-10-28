//! Reversible vault replacement: a database reset and a handoff replacement of the
//! same vault move the previous data into a bounded set of rollback copies that the
//! user can restore from Settings.
//!
//! A reset keeps the database. A handoff replacement keeps the replaced database,
//! and when the replacement holds drastically less data, the whole replaced vault
//! layout, flagged for review on this device.

use super::backup::holds_vault;
use super::{APP_SQLITE_FILE, VAULT_MANIFEST_FILE, active_writable_vault_path};
use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeDelta, Utc};
use ganbaru_atomic_files::FileAccess;
use ganbaru_sync_replica::backup::BUNDLE_DIRECTORIES;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

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

/// Retention record stored in [`RETENTION_FILE`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Retention {
    pub origin: RollbackOrigin,
    /// The replacement held drastically less data than this copy.
    pub flagged: bool,
    /// The user dismissed the flagged notice.
    pub dismissed: bool,
    /// The copy holds the whole vault layout, not only the database.
    pub full: bool,
}

impl Retention {
    /// Retention of a same-vault replacement, keeping the whole layout when flagged.
    pub(crate) fn replacement(flagged: bool) -> Self {
        Self {
            origin: RollbackOrigin::Replacement,
            flagged,
            dismissed: false,
            full: flagged,
        }
    }
}

/// One restorable copy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRollback {
    pub id: String,
    pub created_at: String,
    pub size_bytes: u64,
    pub origin: RollbackOrigin,
    pub flagged: bool,
    pub dismissed: bool,
    #[serde(skip)]
    full: bool,
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

fn read_retention(directory: &Path) -> Retention {
    let path = directory.join(RETENTION_FILE);
    let Ok(file) = fs::File::open(&path) else {
        return Retention::default();
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_RETENTION_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_RETENTION_FILE_BYTES
    {
        return Retention::default();
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn write_retention(directory: &Path, retention: Retention) -> Result<(), String> {
    let bytes = serde_json::to_vec(&retention)
        .map_err(|error| format!("encode database rollback retention: {error}"))?;
    ganbaru_atomic_files::write_file_atomically(
        &directory.join(RETENTION_FILE),
        &bytes,
        FileAccess::OwnerOnly,
    )
    .map_err(|error| format!("write database rollback retention: {error}"))
}

/// Lists rollback directories, newest first, ignoring entries that are not
/// rollback IDs or that hold no regular database file.
pub(crate) fn list_rollbacks(vault: &Path) -> Result<Vec<DatabaseRollback>, String> {
    let root = rollback_root(vault);
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("read database rollbacks: {error}")),
    };
    let mut rollbacks = Vec::new();
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
        let retention = read_retention(&entry.path());
        rollbacks.push(DatabaseRollback {
            id,
            created_at: created.to_rfc3339_opts(SecondsFormat::Millis, true),
            size_bytes: metadata.len(),
            origin: retention.origin,
            flagged: retention.flagged,
            dismissed: retention.dismissed,
            full: retention.full,
        });
    }
    rollbacks.sort_by(|left, right| right.id.cmp(&left.id));
    Ok(rollbacks)
}

/// Selects the rollbacks retention no longer keeps, given every rollback newest
/// first: resets and unflagged replacements beyond their counts, dismissed flagged
/// copies past their age, and flagged copies beyond their count, keeping the
/// earliest undismissed ones first.
fn stale_rollbacks(rollbacks: &[DatabaseRollback], now: DateTime<Utc>) -> Vec<String> {
    let mut stale = Vec::new();
    let mut resets = 0;
    let mut replacements = 0;
    let mut flagged: Vec<&DatabaseRollback> = Vec::new();
    for rollback in rollbacks {
        let count = match (rollback.origin, rollback.flagged) {
            (_, true) => {
                let expired = rollback.dismissed
                    && parse_rollback_id(&rollback.id)
                        .is_ok_and(|created| created + DISMISSED_FLAGGED_RETENTION < now);
                if expired {
                    stale.push(rollback.id.clone());
                } else {
                    flagged.push(rollback);
                }
                continue;
            }
            (RollbackOrigin::Reset, false) => &mut resets,
            (RollbackOrigin::Replacement, false) => &mut replacements,
        };
        *count += 1;
        let limit = match rollback.origin {
            RollbackOrigin::Reset => MAX_DATABASE_ROLLBACKS,
            RollbackOrigin::Replacement => MAX_REPLACEMENT_ROLLBACKS,
        };
        if *count > limit {
            stale.push(rollback.id.clone());
        }
    }
    flagged.sort_by(|left, right| {
        left.dismissed
            .cmp(&right.dismissed)
            .then_with(|| match left.dismissed {
                false => left.id.cmp(&right.id),
                true => right.id.cmp(&left.id),
            })
    });
    stale.extend(
        flagged
            .into_iter()
            .skip(MAX_FLAGGED_REPLACEMENTS)
            .map(|rollback| rollback.id.clone()),
    );
    stale
}

/// Deletes the rollbacks retention no longer keeps.
fn prune_rollbacks(vault: &Path, now: DateTime<Utc>) -> Result<(), String> {
    let root = rollback_root(vault);
    for stale in stale_rollbacks(&list_rollbacks(vault)?, now) {
        fs::remove_dir_all(root.join(&stale))
            .map_err(|error| format!("remove old database rollback: {error}"))?;
    }
    Ok(())
}

/// Moves the vault database into a new reset copy and prunes old copies.
/// Returns `None` when the vault has no database to move.
#[cfg(any(test, desktop))]
pub(crate) fn move_database_to_rollback(
    vault: &Path,
    at: DateTime<Utc>,
) -> Result<Option<String>, String> {
    let created = create_rollback(vault, at, Retention::default())?;
    prune_rollbacks(vault, at)?;
    Ok(created)
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

/// Applies retention limits to the vault at `vault` after a replacement.
pub(crate) fn prune_after_replacement(vault: &Path, at: DateTime<Utc>) -> Result<(), String> {
    prune_rollbacks(vault, at)
}

/// Moves the vault data a copy keeps into a new rollback directory whose ID sorts
/// after every existing copy, so a clock step backwards or a pruned ID never makes
/// the new copy the oldest. Returns `None` when there is no database to move.
fn create_rollback(
    vault: &Path,
    at: DateTime<Utc>,
    retention: Retention,
) -> Result<Option<String>, String> {
    if fs::symlink_metadata(database_file(vault, "")).is_err() {
        return Ok(None);
    }
    let root = rollback_root(vault);
    ganbaru_atomic_files::create_private_directories(&root)
        .map_err(|error| format!("create database rollbacks: {error}"))?;
    let mut created = at;
    if let Some(newest) = list_rollbacks(vault)?.first() {
        let after_newest = parse_rollback_id(&newest.id)? + TimeDelta::milliseconds(1);
        created = created.max(after_newest);
    }
    let directory = loop {
        let candidate = root.join(rollback_id(created));
        match ganbaru_atomic_files::create_private_directory(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                created += TimeDelta::milliseconds(1);
            }
            Err(error) => return Err(format!("create database rollback: {error}")),
        }
    };
    let names = moved_entry_names(retention.full);
    let kept = if retention == Retention::default() {
        Ok(())
    } else {
        write_retention(&directory, retention)
    }
    .and_then(|()| move_entries(vault, &directory, &names));
    if let Err(error) = kept {
        return Err(match fs::remove_dir_all(&directory) {
            Ok(()) => error,
            Err(cleanup) => format!("{error}; remove empty database rollback: {cleanup}"),
        });
    }
    Ok(Some(rollback_id(created)))
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
/// database. A full copy must also hold the same vault as `vault`.
async fn validate_rollback(vault: &Path, id: &str) -> Result<(PathBuf, Retention), String> {
    parse_rollback_id(id)?;
    let directory = rollback_root(vault).join(id);
    let is_directory = fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir());
    let is_file =
        fs::symlink_metadata(database_file(&directory, "")).is_ok_and(|meta| meta.is_file());
    if !is_directory || !is_file {
        return Err("unknown database rollback".to_string());
    }
    let retention = read_retention(&directory);
    if retention.full && !holds_vault(&directory, &vault_id_at(vault)?) {
        return Err("database rollback belongs to a different vault".to_string());
    }
    validate_rollback_database(&database_file(&directory, "")).await?;
    Ok((directory, retention))
}

/// Restores rollback `id` as the vault data. The current data becomes a new
/// rollback first, so a restore is itself reversible.
pub(crate) async fn restore_rollback(
    vault: &Path,
    id: &str,
    at: DateTime<Utc>,
) -> Result<(), String> {
    let (directory, retention) = validate_rollback(vault, id).await?;
    let replaced = create_rollback(
        vault,
        at,
        Retention {
            full: retention.full,
            ..Retention::default()
        },
    )?;
    if let Err(error) = move_entries(&directory, vault, &moved_entry_names(retention.full)) {
        if let Some(replaced) = &replaced {
            let _ = move_entries(
                &rollback_root(vault).join(replaced),
                vault,
                &moved_entry_names(retention.full),
            );
        }
        return Err(error);
    }
    fs::remove_dir_all(&directory).map_err(|error| format!("remove restored rollback: {error}"))?;
    prune_rollbacks(vault, at)
}

/// Marks a flagged rollback as reviewed, so its notice no longer shows. The copy
/// stays restorable until retention removes it.
pub(crate) fn dismiss_rollback(vault: &Path, id: &str) -> Result<(), String> {
    parse_rollback_id(id)?;
    let directory = rollback_root(vault).join(id);
    if !fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir()) {
        return Err("unknown database rollback".to_string());
    }
    let retention = read_retention(&directory);
    if !retention.flagged || retention.dismissed {
        return Ok(());
    }
    write_retention(
        &directory,
        Retention {
            dismissed: true,
            ..retention
        },
    )
}

/// Lists the active vault's database rollbacks, newest first.
#[tauri::command]
pub async fn vault_database_rollbacks(
    app: tauri::AppHandle,
) -> Result<Vec<DatabaseRollback>, String> {
    let (vault, _) = super::active_vault(&app)?;
    list_rollbacks(&vault)
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
    validate_rollback(writable_vault.as_ref(), &id).await?;
    let quiescence = super::quiescence::begin_reserved_quiescence(
        &app,
        transition,
        super::quiescence::RunningTerminals::Block,
    )
    .await?;
    let restored = restore_rollback(writable_vault.as_ref(), &id, super::now_utc()).await;
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
            origin: retention.origin,
            flagged: retention.flagged,
            dismissed: retention.dismissed,
            full: retention.full,
        }
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
        assert_eq!(listed.origin, RollbackOrigin::Reset);
        assert!(!listed.flagged);
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
                if second < 2 { reset } else { replacement },
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
        let dismissed = Retention {
            dismissed: true,
            ..flagged
        };
        let rollbacks = vec![
            rollback(9, flagged),
            rollback(8, dismissed),
            rollback(7, flagged),
            rollback(6, flagged),
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
        assert!(!list_rollbacks(&vault).unwrap()[0].dismissed);

        fs::write(database_file(&vault, ""), b"db").unwrap();
        let flagged = create_rollback(
            &vault,
            at(2),
            Retention {
                full: false,
                ..Retention::replacement(true)
            },
        )
        .unwrap()
        .unwrap();
        dismiss_rollback(&vault, &flagged).unwrap();
        let listed = list_rollbacks(&vault).unwrap();
        assert!(listed[0].flagged && listed[0].dismissed);
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
}
