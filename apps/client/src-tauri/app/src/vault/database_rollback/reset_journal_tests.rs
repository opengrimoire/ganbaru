use super::super::{
    DATABASE_ROLLBACK_DIRECTORY, RestoreRecovery, list_rollbacks, move_database_to_rollback,
    moved_entry_names, recover_restore, restore_journal::RESTORE_RECOVERY_PENDING,
    restore_rollback, rollback_id,
};
use super::*;
use crate::vault::APP_SQLITE_FILE;
use chrono::TimeZone;

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 18, 7, 0, second).unwrap()
}

fn copy_id() -> String {
    rollback_id(at(1))
}

#[test]
fn a_reset_moves_only_forward_through_its_phases() {
    for phase in [Phase::Moving, Phase::Moved] {
        let expected = match phase {
            Phase::Moving => Some(Phase::Moved),
            Phase::Moved => None,
        };
        assert_eq!(
            transition(phase, Event::MovedOut).ok(),
            expected,
            "{phase:?}"
        );
    }
}

#[test]
fn recovery_puts_the_database_back_until_it_wholly_moved() {
    assert_eq!(next_step(Phase::Moving), Step::PutBack);
    assert_eq!(next_step(Phase::Moved), Step::Finish);
}

#[test]
fn every_stored_combination_decodes_only_to_a_reset_it_can_record() {
    for phase in [StoredPhase::Moving, StoredPhase::Moved] {
        for copy in [
            copy_id(),
            "20261018T070001Z".to_string(),
            "../outside".to_string(),
        ] {
            let stored = StoredJournal {
                phase,
                copy: copy.clone(),
            };
            let decoded = Journal::try_from(stored.clone());
            assert_eq!(decoded.is_ok(), copy == copy_id(), "{stored:?}");
            if let Ok(journal) = decoded {
                assert_eq!(StoredJournal::from(&journal), stored);
            }
        }
    }
}

#[test]
fn a_journal_with_unknown_fields_is_refused() {
    let bytes = format!(r#"{{"phase":"moved","copy":"{}","extra":1}}"#, copy_id());
    assert!(serde_json::from_str::<StoredJournal>(&bytes).is_err());
}

/// A vault whose database files carry distinct contents, so each can be traced after a crash.
struct Fixture {
    vault: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let vault = crate::vault::tests::unique_path(name);
        fs::create_dir_all(&vault).unwrap();
        for name in moved_entry_names(false) {
            fs::write(vault.join(&name), name.as_bytes()).unwrap();
        }
        Self { vault }
    }

    fn copy(&self) -> PathBuf {
        self.vault.join(DATABASE_ROLLBACK_DIRECTORY).join(copy_id())
    }

    fn journal(&self) -> PathBuf {
        self.vault
            .join(DATABASE_ROLLBACK_DIRECTORY)
            .join(JOURNAL_FILE)
    }

    /// Leaves the on-disk state of a reset that stopped in `phase` with the first `out` database
    /// files in the reset copy.
    fn crash(&self, phase: Phase, out: usize) {
        fs::create_dir_all(self.copy()).unwrap();
        ResetRecord::begin_at(
            &self.vault,
            Journal {
                phase,
                copy: copy_id(),
            },
        )
        .unwrap();
        for name in moved_entry_names(false).iter().take(out) {
            fs::rename(self.vault.join(name), self.copy().join(name)).unwrap();
        }
    }

    /// Asserts that `folder` holds every database file, each with its own contents.
    fn assert_holds(&self, folder: &Path) {
        for name in moved_entry_names(false) {
            assert_eq!(
                fs::read_to_string(folder.join(&name)).unwrap(),
                name,
                "{}",
                folder.display()
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.vault);
    }
}

#[test]
fn a_crash_while_moving_puts_the_database_back_with_its_log() {
    let names = moved_entry_names(false).len();
    // Before the first move, with the database split from its log, and with every file out.
    for out in [0, 1, names - 1, names] {
        let fixture = Fixture::new("reset-moving");
        fixture.crash(Phase::Moving, out);
        assert_eq!(
            recover_restore(&fixture.vault).unwrap(),
            RestoreRecovery::Settled
        );
        fixture.assert_holds(&fixture.vault);
        assert!(!fixture.copy().exists(), "{out}");
        assert!(!fixture.journal().exists(), "{out}");
    }
}

#[test]
fn a_crash_while_moving_before_the_copy_exists_settles() {
    let fixture = Fixture::new("reset-moving-no-copy");
    fs::create_dir_all(fixture.vault.join(DATABASE_ROLLBACK_DIRECTORY)).unwrap();
    ResetRecord::begin_at(
        &fixture.vault,
        Journal {
            phase: Phase::Moving,
            copy: copy_id(),
        },
    )
    .unwrap();
    assert_eq!(
        recover_restore(&fixture.vault).unwrap(),
        RestoreRecovery::Settled
    );
    fixture.assert_holds(&fixture.vault);
    assert!(!fixture.journal().exists());
}

#[test]
fn a_crash_once_moved_keeps_the_reset_copy() {
    let fixture = Fixture::new("reset-moved");
    fixture.crash(Phase::Moved, moved_entry_names(false).len());
    assert_eq!(
        recover_restore(&fixture.vault).unwrap(),
        RestoreRecovery::Settled
    );
    fixture.assert_holds(&fixture.copy());
    for name in moved_entry_names(false) {
        assert!(!fixture.vault.join(name).exists());
    }
    assert_eq!(list_rollbacks(&fixture.vault).unwrap()[0].id, copy_id());
    assert!(!fixture.journal().exists());
}

#[test]
fn a_file_in_both_places_leaves_the_reset_pending_and_untouched() {
    let fixture = Fixture::new("reset-both");
    fixture.crash(Phase::Moving, 1);
    let first = &moved_entry_names(false)[0];
    fs::write(fixture.vault.join(first), b"other").unwrap();
    assert!(matches!(
        recover_restore(&fixture.vault).unwrap(),
        RestoreRecovery::Pending(_)
    ));
    assert_eq!(fs::read(fixture.vault.join(first)).unwrap(), b"other");
    assert_eq!(
        fs::read_to_string(fixture.copy().join(first)).unwrap(),
        *first
    );
    assert!(fixture.journal().exists());
}

#[test]
fn an_undecodable_journal_leaves_the_vault_untouched() {
    let fixture = Fixture::new("reset-undecodable");
    fs::create_dir_all(fixture.vault.join(DATABASE_ROLLBACK_DIRECTORY)).unwrap();
    fs::write(fixture.journal(), b"not a journal").unwrap();
    assert!(matches!(
        recover_restore(&fixture.vault).unwrap(),
        RestoreRecovery::Pending(_)
    ));
    fixture.assert_holds(&fixture.vault);
    assert!(fixture.journal().exists());
}

#[test]
fn a_reset_copy_holding_other_entries_stays_after_the_database_returns() {
    let fixture = Fixture::new("reset-stray");
    fixture.crash(Phase::Moving, 2);
    fs::write(fixture.copy().join("stray"), b"stray").unwrap();
    assert!(matches!(
        recover_restore(&fixture.vault).unwrap(),
        RestoreRecovery::Pending(_)
    ));
    fixture.assert_holds(&fixture.vault);
    assert!(fixture.copy().join("stray").exists());
}

#[tokio::test]
async fn resets_and_restores_wait_while_a_reset_is_pending() {
    let fixture = Fixture::new("reset-pending");
    fs::create_dir_all(fixture.vault.join(DATABASE_ROLLBACK_DIRECTORY)).unwrap();
    fs::write(fixture.journal(), b"not a journal").unwrap();
    assert_eq!(
        move_database_to_rollback(&fixture.vault, at(2)),
        Err(RESTORE_RECOVERY_PENDING.to_string())
    );
    assert_eq!(
        restore_rollback(&fixture.vault, &copy_id(), at(2)).await,
        Err(RESTORE_RECOVERY_PENDING.to_string())
    );
    fixture.assert_holds(&fixture.vault);
}

#[test]
fn a_finished_reset_leaves_no_journal() {
    let fixture = Fixture::new("reset-finished");
    let id = move_database_to_rollback(&fixture.vault, at(1))
        .unwrap()
        .unwrap();
    fixture.assert_holds(&fixture.vault.join(DATABASE_ROLLBACK_DIRECTORY).join(id));
    assert!(!fixture.journal().exists());
}

/// Writes a migrated database marked by `marker_pragma` at `path` and closes it, so no log remains.
async fn write_marked_database(path: &Path, marker_pragma: &'static str) {
    ganbaru_db::test_support::write_migrated_database(path)
        .await
        .unwrap();
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(path).await.unwrap();
    sqlx::query(marker_pragma).execute(&pool).await.unwrap();
    registry.close_all().await.unwrap();
}

#[tokio::test]
async fn opening_the_vault_after_a_crash_mid_reset_reopens_its_data() {
    let config = crate::vault::tests::unique_path("reset-open-config");
    let vault = crate::vault::tests::unique_path("reset-open-vault");
    let database =
        crate::vault::activate_test_vault(&config, &vault, "live-vault", "device-a").unwrap();
    let vault = database.parent().unwrap().to_path_buf();
    write_marked_database(&database, "PRAGMA application_id = 7").await;
    let copy = vault.join(DATABASE_ROLLBACK_DIRECTORY).join(copy_id());
    fs::create_dir_all(&copy).unwrap();
    ResetRecord::begin_at(
        &vault,
        Journal {
            phase: Phase::Moving,
            copy: copy_id(),
        },
    )
    .unwrap();
    // The database reached the copy and the process stopped before the reset was recorded.
    fs::rename(&database, copy.join(APP_SQLITE_FILE)).unwrap();

    let registry = crate::db::DatabaseState::default();
    let pool = crate::db::connect_vault_for_sync_in(&registry, &config)
        .await
        .unwrap();
    let opened: i64 = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&pool)
        .await
        .unwrap();
    registry.close_all().await.unwrap();
    assert_eq!(opened, 7);
    assert!(!copy.exists());
    assert!(
        !vault
            .join(DATABASE_ROLLBACK_DIRECTORY)
            .join(JOURNAL_FILE)
            .exists()
    );
    let _ = fs::remove_dir_all(vault);
    let _ = fs::remove_dir_all(config);
}

#[test]
fn a_crash_once_moved_stops_the_marker_recording_data() {
    let fixture = Fixture::new("reset-moved-marker");
    let marker = serde_json::json!({
        "app": "ganbaru",
        "schemaVersion": crate::vault::VAULT_SCHEMA_VERSION,
        "vaultId": "live-vault",
        "displayName": "Test",
        "createdAt": "2026-10-18T07:00:00.000Z",
        "hasData": true,
    });
    fs::write(fixture.vault.join(VAULT_MANIFEST_FILE), marker.to_string()).unwrap();
    fixture.crash(Phase::Moved, moved_entry_names(false).len());
    assert_eq!(
        recover_restore(&fixture.vault).unwrap(),
        RestoreRecovery::Settled
    );
    assert!(
        !crate::vault::read_vault_manifest(&fixture.vault)
            .unwrap()
            .has_data
    );
    assert!(crate::vault::refuse_lost_database(&fixture.vault).is_ok());
    assert!(!fixture.journal().exists());
}
