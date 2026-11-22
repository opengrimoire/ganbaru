use super::super::{
    DATABASE_ROLLBACK_DIRECTORY, Retention, list_rollbacks, move_database_to_rollback,
    moved_entry_names, prune_after_replacement, restore_rollback, rollback_id, write_retention,
};
use super::*;
use crate::vault::{APP_SQLITE_FILE, VAULT_MANIFEST_FILE};
use chrono::TimeZone;

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 18, 7, 0, second).unwrap()
}

fn source_id() -> String {
    rollback_id(at(1))
}

fn replaced_id() -> String {
    rollback_id(at(2))
}

fn every_phase() -> [Phase; 4] {
    [
        Phase::Replacing,
        Phase::Restoring,
        Phase::Returning,
        Phase::Restored,
    ]
}

fn every_event() -> [Event; 3] {
    [Event::MovedOut, Event::MovedIn, Event::CopyReturned]
}

#[test]
fn a_restore_moves_only_forward_through_its_phases() {
    for phase in every_phase() {
        for event in every_event() {
            let expected = match (phase, event) {
                (Phase::Replacing, Event::MovedOut) => Some(Phase::Restoring),
                (Phase::Restoring, Event::MovedIn) => Some(Phase::Restored),
                (Phase::Restoring, Event::CopyReturned) => Some(Phase::Returning),
                _ => None,
            };
            assert_eq!(
                transition(phase, event).ok(),
                expected,
                "{phase:?} on {event:?}"
            );
        }
    }
}

#[test]
fn recovery_undoes_every_phase_before_the_copy_is_in_place() {
    for phase in every_phase() {
        let expected = match phase {
            Phase::Replacing | Phase::Returning => Step::PutBack,
            Phase::Restoring => Step::ReturnCopy,
            Phase::Restored => Step::Finish,
        };
        assert_eq!(next_step(phase), expected, "{phase:?}");
    }
}

#[test]
fn every_stored_combination_decodes_only_to_a_restore_it_can_record() {
    let ids = [
        source_id(),
        replaced_id(),
        "20261018T070001Z".to_string(),
        "../outside".to_string(),
    ];
    let phases = [
        StoredPhase::Replacing,
        StoredPhase::Restoring,
        StoredPhase::Returning,
        StoredPhase::Restored,
    ];
    for phase in phases {
        for source in &ids {
            for replaced in std::iter::once(None).chain(ids.iter().cloned().map(Some)) {
                for full in [false, true] {
                    let stored = StoredJournal {
                        phase,
                        source: source.clone(),
                        replaced: replaced.clone(),
                        full,
                    };
                    let canonical = |id: &str| id == source_id() || id == replaced_id();
                    let valid = canonical(source)
                        && replaced
                            .as_deref()
                            .is_none_or(|id| canonical(id) && id != source)
                        && !(phase == StoredPhase::Replacing && replaced.is_none());
                    let decoded = Journal::try_from(stored.clone());
                    assert_eq!(decoded.is_ok(), valid, "{stored:?}");
                    if let Ok(journal) = decoded {
                        assert_eq!(StoredJournal::from(&journal), stored);
                    }
                }
            }
        }
    }
}

#[test]
fn a_journal_with_unknown_fields_is_refused() {
    let bytes = format!(
        r#"{{"phase":"restored","source":"{}","replaced":null,"full":false,"extra":1}}"#,
        source_id()
    );
    assert!(serde_json::from_str::<StoredJournal>(&bytes).is_err());
}

const LIVE: u32 = 1;
const COPY: u32 = 2;
/// Offset of the application ID in a SQLite database header.
const APPLICATION_ID_OFFSET: usize = 68;

/// A vault whose live data and chosen copy carry distinct contents in every moved entry, so
/// each entry can be traced after a crash. Database files are header-sized stand-ins: recovery
/// never opens them.
struct Fixture {
    vault: PathBuf,
    full: bool,
}

impl Fixture {
    fn new(name: &str, full: bool) -> Self {
        let vault = crate::vault::tests::unique_path(name);
        fs::create_dir_all(&vault).unwrap();
        let fixture = Self { vault, full };
        fixture.write_data(&fixture.vault, LIVE, "live-vault");
        let source = fixture.copy(&source_id());
        fs::create_dir_all(&source).unwrap();
        fixture.write_data(&source, COPY, "copy-vault");
        if !full {
            write_manifest(&fixture.vault, "live-vault");
        }
        fixture
    }

    fn write_data(&self, folder: &Path, marker: u32, vault_id: &str) {
        let mut header = vec![0_u8; 100];
        header[APPLICATION_ID_OFFSET..APPLICATION_ID_OFFSET + 4]
            .copy_from_slice(&marker.to_be_bytes());
        fs::write(folder.join(APP_SQLITE_FILE), header).unwrap();
        for suffix in ["-wal", "-shm"] {
            fs::write(
                folder.join(format!("{APP_SQLITE_FILE}{suffix}")),
                format!("{marker}{suffix}"),
            )
            .unwrap();
        }
        if self.full {
            write_manifest(folder, vault_id);
            for directory in ganbaru_sync_replica::backup::BUNDLE_DIRECTORIES {
                fs::create_dir_all(folder.join(directory)).unwrap();
            }
            fs::write(folder.join("assets").join("owner"), vault_id).unwrap();
        }
    }

    fn root(&self) -> PathBuf {
        self.vault.join(DATABASE_ROLLBACK_DIRECTORY)
    }

    fn copy(&self, id: &str) -> PathBuf {
        self.root().join(id)
    }

    fn names(&self) -> Vec<String> {
        moved_entry_names(self.full)
    }

    /// Leaves the on-disk state of a restore that stopped in `phase`, with the first
    /// `out` names of the vault data in the replaced copy and the first `inward` names of the
    /// chosen copy in the vault.
    fn crash(&self, phase: Phase, out: usize, inward: usize) {
        let replaced = self.copy(&replaced_id());
        fs::create_dir_all(&replaced).unwrap();
        write_retention(
            &replaced,
            &Retention::Reset {
                full: self.full,
                owner: None,
            },
        )
        .unwrap();
        RestoreRecord::begin(
            &self.vault,
            Journal {
                phase,
                source: source_id(),
                replaced: Some(replaced_id()),
                full: self.full,
            },
        )
        .unwrap();
        let names = self.names();
        for name in names.iter().take(out) {
            fs::rename(self.vault.join(name), replaced.join(name)).unwrap();
        }
        for name in names.iter().take(inward) {
            fs::rename(self.copy(&source_id()).join(name), self.vault.join(name)).unwrap();
        }
    }

    fn journal(&self) -> PathBuf {
        self.root().join(JOURNAL_FILE)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.vault);
    }
}

fn write_manifest(folder: &Path, vault_id: &str) {
    let manifest = serde_json::json!({
        "app": "ganbaru",
        "schemaVersion": crate::vault::VAULT_SCHEMA_VERSION,
        "vaultId": vault_id,
        "displayName": "Test",
        "createdAt": "2026-10-18T07:00:00.000Z",
        "hasData": true,
    });
    fs::write(folder.join(VAULT_MANIFEST_FILE), manifest.to_string()).unwrap();
}

fn marker(folder: &Path) -> u32 {
    let bytes = fs::read(folder.join(APP_SQLITE_FILE)).unwrap();
    u32::from_be_bytes(
        bytes[APPLICATION_ID_OFFSET..APPLICATION_ID_OFFSET + 4]
            .try_into()
            .unwrap(),
    )
}

/// Asserts that `folder` holds the whole data marked `marker` and nothing of the other copy.
fn assert_holds(fixture: &Fixture, folder: &Path, marker_value: u32, vault_id: &str) {
    assert_eq!(marker(folder), marker_value, "{}", folder.display());
    for suffix in ["-wal", "-shm"] {
        assert_eq!(
            fs::read_to_string(folder.join(format!("{APP_SQLITE_FILE}{suffix}"))).unwrap(),
            format!("{marker_value}{suffix}"),
            "{suffix} must stay with its database in {}",
            folder.display()
        );
    }
    if fixture.full {
        let manifest = fs::read_to_string(folder.join(VAULT_MANIFEST_FILE)).unwrap();
        assert!(manifest.contains(vault_id), "{}", folder.display());
        assert_eq!(
            fs::read_to_string(folder.join("assets").join("owner")).unwrap(),
            vault_id
        );
    }
}

/// Asserts the vault holds its data from before the restore and the chosen copy is whole again.
fn assert_undone(fixture: &Fixture) {
    assert_eq!(recover(&fixture.vault).unwrap(), Recovery::Settled);
    assert_holds(fixture, &fixture.vault, LIVE, "live-vault");
    assert_holds(fixture, &fixture.copy(&source_id()), COPY, "copy-vault");
    assert!(!fixture.copy(&replaced_id()).exists());
    assert!(!fixture.journal().exists());
}

#[test]
fn a_crash_while_replacing_puts_the_vault_data_back() {
    for full in [false, true] {
        let names = moved_entry_names(full).len();
        // Before the first move, with the database split from its log, and with every entry out.
        for out in [0, 1, names] {
            let fixture = Fixture::new("restore-replacing", full);
            fixture.crash(Phase::Replacing, out, 0);
            assert_undone(&fixture);
        }
    }
}

#[test]
fn a_crash_while_replacing_before_the_replaced_copy_exists_settles() {
    let fixture = Fixture::new("restore-replacing-no-copy", false);
    RestoreRecord::begin(
        &fixture.vault,
        Journal {
            phase: Phase::Replacing,
            source: source_id(),
            replaced: Some(replaced_id()),
            full: false,
        },
    )
    .unwrap();
    assert_undone(&fixture);
}

#[test]
fn a_crash_while_restoring_returns_the_copy_and_the_vault_data() {
    for full in [false, true] {
        let names = moved_entry_names(full).len();
        // Before the copy moves, with its database split from its log, and wholly moved in.
        for inward in [0, 1, names] {
            let fixture = Fixture::new("restore-restoring", full);
            fixture.crash(Phase::Restoring, names, inward);
            assert_undone(&fixture);
        }
    }
}

#[test]
fn a_crash_while_returning_finishes_putting_the_vault_data_back() {
    for full in [false, true] {
        let names = moved_entry_names(full).len();
        for back in [0, 1, names] {
            let fixture = Fixture::new("restore-returning", full);
            fixture.crash(Phase::Returning, names, 0);
            let replaced = fixture.copy(&replaced_id());
            for name in fixture.names().iter().take(back) {
                fs::rename(replaced.join(name), fixture.vault.join(name)).unwrap();
            }
            assert_undone(&fixture);
        }
    }
}

#[test]
fn a_crash_once_restored_keeps_the_copy_and_removes_its_directory() {
    for full in [false, true] {
        for source_removed in [false, true] {
            let fixture = Fixture::new("restore-restored", full);
            let names = moved_entry_names(full).len();
            fixture.crash(Phase::Restored, names, names);
            if source_removed {
                fs::remove_dir_all(fixture.copy(&source_id())).unwrap();
            }
            assert_eq!(recover(&fixture.vault).unwrap(), Recovery::Settled);
            assert_holds(&fixture, &fixture.vault, COPY, "copy-vault");
            assert_holds(&fixture, &fixture.copy(&replaced_id()), LIVE, "live-vault");
            assert!(!fixture.copy(&source_id()).exists());
            assert!(!fixture.journal().exists());
        }
    }
}

#[test]
fn a_crash_while_restoring_into_a_vault_without_a_database_returns_the_copy() {
    let fixture = Fixture::new("restore-no-database", false);
    for suffix in ["", "-wal", "-shm"] {
        fs::remove_file(fixture.vault.join(format!("{APP_SQLITE_FILE}{suffix}"))).unwrap();
    }
    RestoreRecord::begin(
        &fixture.vault,
        Journal {
            phase: Phase::Restoring,
            source: source_id(),
            replaced: None,
            full: false,
        },
    )
    .unwrap();
    fs::rename(
        fixture.copy(&source_id()).join(APP_SQLITE_FILE),
        fixture.vault.join(APP_SQLITE_FILE),
    )
    .unwrap();
    assert_eq!(recover(&fixture.vault).unwrap(), Recovery::Settled);
    assert!(!fixture.vault.join(APP_SQLITE_FILE).exists());
    assert_holds(&fixture, &fixture.copy(&source_id()), COPY, "copy-vault");
    assert!(!fixture.journal().exists());
}

#[test]
fn an_entry_in_both_places_leaves_the_restore_pending_and_untouched() {
    let fixture = Fixture::new("restore-conflict", false);
    fixture.crash(Phase::Replacing, 1, 0);
    let replaced = fixture.copy(&replaced_id());
    fs::write(fixture.vault.join(APP_SQLITE_FILE), b"unexplained").unwrap();
    assert!(matches!(
        recover(&fixture.vault).unwrap(),
        Recovery::Pending(_)
    ));
    assert_eq!(marker(&replaced), LIVE);
    assert_eq!(
        fs::read(fixture.vault.join(APP_SQLITE_FILE)).unwrap(),
        b"unexplained"
    );
    assert!(fixture.journal().exists());
    assert_eq!(
        recover_or_refuse(&fixture.vault),
        Err(RESTORE_RECOVERY_PENDING.to_string())
    );
}

#[test]
fn an_undecodable_journal_leaves_the_vault_untouched() {
    let fixture = Fixture::new("restore-undecodable", false);
    fs::write(fixture.journal(), b"{\"phase\":\"sideways\"}").unwrap();
    assert!(matches!(
        recover(&fixture.vault).unwrap(),
        Recovery::Pending(_)
    ));
    assert_holds(&fixture, &fixture.vault, LIVE, "live-vault");
    assert_holds(&fixture, &fixture.copy(&source_id()), COPY, "copy-vault");
    assert!(fixture.journal().exists());
}

#[test]
fn a_replaced_copy_holding_other_entries_stays_after_its_data_returns() {
    let fixture = Fixture::new("restore-extra-entry", false);
    fixture.crash(Phase::Replacing, 3, 0);
    let replaced = fixture.copy(&replaced_id());
    fs::write(replaced.join("unexpected"), b"kept").unwrap();
    assert!(matches!(
        recover(&fixture.vault).unwrap(),
        Recovery::Pending(_)
    ));
    assert_holds(&fixture, &fixture.vault, LIVE, "live-vault");
    assert_eq!(fs::read(replaced.join("unexpected")).unwrap(), b"kept");
    assert!(fixture.journal().exists());
}

#[tokio::test]
async fn copies_are_neither_created_nor_pruned_nor_restored_while_a_restore_is_pending() {
    let fixture = Fixture::new("restore-refusals", false);
    fs::write(fixture.journal(), b"not a journal").unwrap();
    let pending = RESTORE_RECOVERY_PENDING.to_string();
    assert_eq!(
        move_database_to_rollback(&fixture.vault, at(9)),
        Err(pending.clone())
    );
    assert_eq!(
        prune_after_replacement(&fixture.vault, at(9)),
        Err(pending.clone())
    );
    assert_eq!(
        restore_rollback(&fixture.vault, &source_id(), at(9)).await,
        Err(pending)
    );
    assert_holds(&fixture, &fixture.vault, LIVE, "live-vault");
    assert_eq!(list_rollbacks(&fixture.vault).unwrap().len(), 1);
}

#[test]
fn a_vault_swap_waits_for_a_pending_restore() {
    let fixture = Fixture::new("restore-swap", false);
    fs::write(fixture.journal(), b"not a journal").unwrap();
    let staging = fixture.vault.with_extension("staging");
    fs::create_dir_all(&staging).unwrap();
    write_manifest(&staging, "live-vault");
    let swapped = crate::vault::backup::swap_journal::swap(
        &staging,
        &fixture.vault,
        crate::vault::backup::swap_journal::Replacement::SameVault { flagged: false },
    );
    assert_eq!(swapped, Err(RESTORE_RECOVERY_PENDING.to_string()));
    assert!(staging.join(VAULT_MANIFEST_FILE).exists());
    assert_holds(&fixture, &fixture.vault, LIVE, "live-vault");
    fs::remove_dir_all(staging).unwrap();
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
async fn opening_the_vault_after_a_crash_settles_the_restore_instead_of_creating_a_database() {
    let config = crate::vault::tests::unique_path("restore-open-config");
    let vault = crate::vault::tests::unique_path("restore-open-vault");
    let database =
        crate::vault::activate_test_vault(&config, &vault, "live-vault", "device-a").unwrap();
    let vault = database.parent().unwrap().to_path_buf();
    write_marked_database(&database, "PRAGMA application_id = 1").await;
    let source = vault.join(DATABASE_ROLLBACK_DIRECTORY).join(source_id());
    fs::create_dir_all(&source).unwrap();
    write_marked_database(&source.join(APP_SQLITE_FILE), "PRAGMA application_id = 2").await;
    let replaced = vault.join(DATABASE_ROLLBACK_DIRECTORY).join(replaced_id());
    fs::create_dir_all(&replaced).unwrap();
    RestoreRecord::begin(
        &vault,
        Journal {
            phase: Phase::Restoring,
            source: source_id(),
            replaced: Some(replaced_id()),
            full: false,
        },
    )
    .unwrap();
    // The vault data reached its copy and the process stopped before the chosen copy moved in.
    fs::rename(&database, replaced.join(APP_SQLITE_FILE)).unwrap();

    let registry = crate::db::DatabaseState::default();
    let pool = crate::db::connect_vault_for_sync_in(&registry, &config)
        .await
        .unwrap();
    let opened: i64 = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&pool)
        .await
        .unwrap();
    registry.close_all().await.unwrap();
    assert_eq!(opened, i64::from(LIVE));
    assert_eq!(marker(&source), COPY);
    assert!(!replaced.exists());
    assert!(
        !vault
            .join(DATABASE_ROLLBACK_DIRECTORY)
            .join(JOURNAL_FILE)
            .exists()
    );
    let _ = fs::remove_dir_all(vault);
    let _ = fs::remove_dir_all(config);
}
