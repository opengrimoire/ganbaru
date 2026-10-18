//! Reversible database reset: the reset moves the vault database into a bounded
//! set of rollback copies that the user can restore from Settings.

use super::{APP_SQLITE_FILE, active_writable_vault_path};
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use serde::Serialize;
use sqlx::Row;
use std::fs;
use std::path::{Path, PathBuf};

/// Vault-root directory that holds database rollback copies. It is outside the
/// bundle layout, so backups and handoff never carry it.
pub(crate) const DATABASE_ROLLBACK_DIRECTORY: &str = ".database-rollbacks";
/// Most rollback copies kept per vault; older copies are deleted on reset.
pub(crate) const MAX_DATABASE_ROLLBACKS: usize = 3;
/// UTC timestamp format of rollback IDs, sortable by name.
const ROLLBACK_ID_FORMAT: &str = "%Y%m%dT%H%M%S%3fZ";
/// Length of an ID produced by [`ROLLBACK_ID_FORMAT`].
const ROLLBACK_ID_LENGTH: usize = 19;
/// Database file suffixes that move together: the main file, WAL, and shared memory.
const DATABASE_FILE_SUFFIXES: [&str; 3] = ["", "-wal", "-shm"];

/// One restorable database copy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRollback {
    pub id: String,
    pub created_at: String,
    pub size_bytes: u64,
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

/// Moves the database and its sidecars from `from` to `to`. A failure moves the
/// files already moved back, so the database never ends up split.
fn move_database_files(from: &Path, to: &Path) -> Result<(), String> {
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in DATABASE_FILE_SUFFIXES {
        let source = database_file(from, suffix);
        if fs::symlink_metadata(&source).is_err() {
            continue;
        }
        let destination = database_file(to, suffix);
        if let Err(error) = fs::rename(&source, &destination) {
            for (moved_source, moved_destination) in moved.iter().rev() {
                let _ = fs::rename(moved_destination, moved_source);
            }
            return Err(format!("move database file: {error}"));
        }
        moved.push((source, destination));
    }
    Ok(())
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
        rollbacks.push(DatabaseRollback {
            id,
            created_at: created.to_rfc3339_opts(SecondsFormat::Millis, true),
            size_bytes: metadata.len(),
        });
    }
    rollbacks.sort_by(|left, right| right.id.cmp(&left.id));
    Ok(rollbacks)
}

/// Deletes the oldest rollbacks beyond [`MAX_DATABASE_ROLLBACKS`].
fn prune_rollbacks(vault: &Path) -> Result<(), String> {
    let root = rollback_root(vault);
    for stale in list_rollbacks(vault)?
        .into_iter()
        .skip(MAX_DATABASE_ROLLBACKS)
    {
        fs::remove_dir_all(root.join(&stale.id))
            .map_err(|error| format!("remove old database rollback: {error}"))?;
    }
    Ok(())
}

/// Moves the vault database into a new rollback copy and prunes old copies.
/// Returns `None` when the vault has no database to move.
pub(crate) fn move_database_to_rollback(
    vault: &Path,
    at: DateTime<Utc>,
) -> Result<Option<String>, String> {
    let created = create_rollback(vault, at)?;
    prune_rollbacks(vault)?;
    Ok(created)
}

/// Moves the vault database into a new rollback directory whose ID sorts after
/// every existing copy, so a clock step backwards or a pruned ID never makes the
/// new copy the oldest. Returns `None` when there is no database to move.
fn create_rollback(vault: &Path, at: DateTime<Utc>) -> Result<Option<String>, String> {
    if fs::symlink_metadata(database_file(vault, "")).is_err() {
        return Ok(None);
    }
    let root = rollback_root(vault);
    fs::create_dir_all(&root).map_err(|error| format!("create database rollbacks: {error}"))?;
    let mut created = at;
    if let Some(newest) = list_rollbacks(vault)?.first() {
        let after_newest = parse_rollback_id(&newest.id)? + chrono::Duration::milliseconds(1);
        created = created.max(after_newest);
    }
    let directory = loop {
        let candidate = root.join(rollback_id(created));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                created += chrono::Duration::milliseconds(1);
            }
            Err(error) => return Err(format!("create database rollback: {error}")),
        }
    };
    if let Err(error) = move_database_files(vault, &directory) {
        return Err(match fs::remove_dir(&directory) {
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

/// Resolves rollback `id` to its directory and validates its database.
async fn validate_rollback(vault: &Path, id: &str) -> Result<PathBuf, String> {
    parse_rollback_id(id)?;
    let directory = rollback_root(vault).join(id);
    let is_directory = fs::symlink_metadata(&directory).is_ok_and(|meta| meta.is_dir());
    let is_file =
        fs::symlink_metadata(database_file(&directory, "")).is_ok_and(|meta| meta.is_file());
    if !is_directory || !is_file {
        return Err("unknown database rollback".to_string());
    }
    validate_rollback_database(&database_file(&directory, "")).await?;
    Ok(directory)
}

/// Restores rollback `id` as the vault database. The current database becomes a
/// new rollback first, so a restore is itself reversible.
pub(crate) async fn restore_rollback(
    vault: &Path,
    id: &str,
    at: DateTime<Utc>,
) -> Result<(), String> {
    let directory = validate_rollback(vault, id).await?;
    let replaced = create_rollback(vault, at)?;
    if let Err(error) = move_database_files(&directory, vault) {
        if let Some(replaced) = &replaced {
            let _ = move_database_files(&rollback_root(vault).join(replaced), vault);
        }
        return Err(error);
    }
    fs::remove_dir(&directory).map_err(|error| format!("remove restored rollback: {error}"))?;
    prune_rollbacks(vault)
}

/// Lists the active vault's database rollbacks, newest first.
#[tauri::command]
pub async fn vault_database_rollbacks(
    app: tauri::AppHandle,
) -> Result<Vec<DatabaseRollback>, String> {
    let (vault, _) = super::active_vault(&app)?;
    list_rollbacks(&vault)
}

/// Restores a database rollback of the active vault under the shared vault
/// transition guard. The caller restarts the app afterwards so every runtime
/// reopens the restored database from a clean state.
#[tauri::command]
pub async fn vault_restore_database_rollback(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    let transition = super::quiescence::reserve_vault_transition()?;
    let writable_vault = active_writable_vault_path(&app)?;
    validate_rollback(writable_vault.as_ref(), &id).await?;
    let quiescence = super::quiescence::begin_reserved_quiescence(&app, transition).await?;
    let restored = restore_rollback(writable_vault.as_ref(), &id, Utc::now()).await;
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
        for suffix in DATABASE_FILE_SUFFIXES {
            assert!(!database_file(&vault, suffix).exists(), "{suffix}");
            assert_eq!(
                fs::read(database_file(&rollback, suffix)).unwrap(),
                suffix.as_bytes()
            );
        }
        assert_eq!(list_rollbacks(&vault).unwrap()[0].id, id);
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
