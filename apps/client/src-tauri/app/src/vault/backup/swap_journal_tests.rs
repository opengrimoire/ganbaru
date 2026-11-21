use super::*;
use crate::vault::database_rollback::{RollbackOrigin, list_rollbacks, restore_rollback};
use crate::vault::{VAULT_MANIFEST_FILE, database_path};

const PREVIOUS_MARKER: i64 = 1;
const REPLACEMENT_MARKER: i64 = 2;
const STAGING: &str = ".ganbaru.restore";

fn every_replaced() -> Vec<Replaced> {
    vec![
        Replaced::SameVault { flagged: false },
        Replaced::SameVault { flagged: true },
        Replaced::OtherVault,
        Replaced::Preserve { discard: false },
        Replaced::Preserve { discard: true },
    ]
}

fn every_phase() -> Vec<Phase> {
    vec![
        Phase::Swapping,
        Phase::Activated,
        Phase::Retaining,
        Phase::Retained { copy: None },
        Phase::Retained {
            copy: Some("copy".to_string()),
        },
    ]
}

fn every_event() -> Vec<Event> {
    vec![
        Event::Renamed,
        Event::RetentionStarted,
        Event::RetentionKept { copy: None },
        Event::RetentionKept {
            copy: Some("copy".to_string()),
        },
    ]
}

/// The phase each swap reaches from `phase` on `event`, stated move by move.
fn expected(replaced: Replaced, phase: &Phase, event: &Event) -> Option<Phase> {
    match (phase, event) {
        (Phase::Swapping, Event::Renamed) => Some(Phase::Activated),
        (Phase::Activated, Event::RetentionStarted) => match replaced {
            Replaced::SameVault { .. } | Replaced::OtherVault => Some(Phase::Retaining),
            Replaced::Preserve { .. } => None,
        },
        (Phase::Retaining, Event::RetentionKept { copy }) => {
            Some(Phase::Retained { copy: copy.clone() })
        }
        _ => None,
    }
}

#[test]
fn every_phase_and_event_pair_follows_the_swap_order() {
    for replaced in every_replaced() {
        for phase in every_phase() {
            for event in every_event() {
                let next = transition(replaced, &phase, event.clone()).ok();
                assert_eq!(
                    next,
                    expected(replaced, &phase, &event),
                    "{replaced:?} {phase:?} {event:?}"
                );
            }
        }
    }
}

fn stored(
    phase: StoredPhase,
    copy: Option<&str>,
    replaced: StoredKind,
    flags: (bool, bool),
) -> StoredJournal {
    StoredJournal {
        phase,
        copy: copy.map(str::to_string),
        replaced,
        flagged: flags.0,
        discard: flags.1,
        vault: "vault-a".to_string(),
        rollback: ".ganbaru.swap-Ganbaru.replaced".to_string(),
        staging: STAGING.to_string(),
    }
}

#[test]
fn every_stored_combination_decodes_only_to_a_swap_it_can_record() {
    let phases = [
        StoredPhase::Swapping,
        StoredPhase::Activated,
        StoredPhase::Retaining,
        StoredPhase::Retained,
    ];
    let kinds = [
        StoredKind::SameVault,
        StoredKind::OtherVault,
        StoredKind::Preserve,
    ];
    let mut decoded = 0;
    for phase in phases {
        for copy in [None, Some("copy")] {
            for kind in kinds {
                for flags in [(false, false), (true, false), (false, true), (true, true)] {
                    let replaced = match (kind, flags) {
                        (StoredKind::SameVault, (flagged, false)) => {
                            Some(Replaced::SameVault { flagged })
                        }
                        (StoredKind::OtherVault, (false, false)) => Some(Replaced::OtherVault),
                        (StoredKind::Preserve, (false, discard)) => {
                            Some(Replaced::Preserve { discard })
                        }
                        _ => None,
                    };
                    let journal_phase = match (phase, copy) {
                        (StoredPhase::Swapping, None) => Some(Phase::Swapping),
                        (StoredPhase::Activated, None) => Some(Phase::Activated),
                        (StoredPhase::Retaining, None) => Some(Phase::Retaining),
                        (StoredPhase::Retained, copy) => Some(Phase::Retained {
                            copy: copy.map(str::to_string),
                        }),
                        _ => None,
                    };
                    let want = match (replaced, journal_phase) {
                        (Some(replaced), Some(phase))
                            if replaced.retained()
                                || matches!(phase, Phase::Swapping | Phase::Activated) =>
                        {
                            Some(Journal {
                                phase,
                                replaced,
                                vault: "vault-a".to_string(),
                                rollback: ".ganbaru.swap-Ganbaru.replaced".to_string(),
                                staging: STAGING.to_string(),
                            })
                        }
                        _ => None,
                    };
                    let got = Journal::try_from(stored(phase, copy, kind, flags)).ok();
                    assert_eq!(got, want, "{phase:?} {copy:?} {kind:?} {flags:?}");
                    if let Some(journal) = got {
                        decoded += 1;
                        let bytes = serde_json::to_vec(&StoredJournal::from(&journal)).unwrap();
                        let again = serde_json::from_slice::<StoredJournal>(&bytes).unwrap();
                        assert_eq!(Journal::try_from(again).unwrap(), journal);
                    }
                }
            }
        }
    }
    assert_eq!(decoded, 19, "every recordable swap decodes exactly once");
}

#[test]
fn journals_naming_paths_outside_the_parent_or_no_vault_are_refused() {
    let base = || {
        stored(
            StoredPhase::Activated,
            None,
            StoredKind::SameVault,
            (false, false),
        )
    };
    let broken: [fn(&mut StoredJournal); 7] = [
        |journal| journal.vault = " ".to_string(),
        |journal| journal.rollback = "../Ganbaru".to_string(),
        |journal| journal.rollback = "nested/folder".to_string(),
        |journal| journal.staging = String::new(),
        |journal| journal.staging = "..".to_string(),
        |journal| journal.staging = journal.rollback.clone(),
        |journal| {
            journal.phase = StoredPhase::Retained;
            journal.copy = Some("../copy".to_string());
        },
    ];
    for (index, breaks) in broken.into_iter().enumerate() {
        let mut journal = base();
        breaks(&mut journal);
        assert!(Journal::try_from(journal).is_err(), "case {index}");
    }
    let unknown = br#"{"phase":"activated","replaced":"sameVault","flagged":false,"discard":false,"vault":"v","rollback":"r","staging":"s","extra":1}"#;
    assert!(serde_json::from_slice::<StoredJournal>(unknown).is_err());
    assert!(Journal::try_from(base()).is_ok());
}

fn every_observation() -> Vec<Observed> {
    let folders = [Folder::Missing, Folder::Staged, Folder::Other];
    let mut observations = Vec::new();
    for target in folders {
        for rollback in folders {
            for staging in [false, true] {
                for copy_kept in [false, true] {
                    observations.push(Observed {
                        target,
                        rollback,
                        staging,
                        copy_kept,
                    });
                }
            }
        }
    }
    observations
}

#[test]
fn no_recovery_step_deletes_a_folder_the_journal_does_not_prove_replaced() {
    for phase in every_phase() {
        for replaced in every_replaced() {
            for observed in every_observation() {
                let step = next_step(&phase, replaced, observed);
                let case = format!("{phase:?} {replaced:?} {observed:?} gives {step:?}");
                if matches!(
                    step,
                    Step::Finish | Step::Discard | Step::Retain | Step::KeepWhole
                ) {
                    assert_eq!(observed.target, Folder::Staged, "{case}");
                    assert_ne!(observed.rollback, Folder::Missing, "{case}");
                }
                match step {
                    Step::Discard => {
                        assert_eq!(replaced, Replaced::Preserve { discard: true }, "{case}");
                        assert!(
                            matches!(phase, Phase::Swapping | Phase::Activated),
                            "{case}"
                        );
                    }
                    Step::Finish => {
                        assert!(matches!(phase, Phase::Retained { .. }), "{case}");
                        assert!(observed.copy_kept, "{case}");
                    }
                    Step::Retain => {
                        assert!(replaced.retained(), "{case}");
                        assert!(
                            matches!(phase, Phase::Swapping | Phase::Activated),
                            "{case}"
                        );
                    }
                    Step::PutBack => {
                        assert_eq!(phase, Phase::Swapping, "{case}");
                        assert_eq!(observed.target, Folder::Missing, "{case}");
                        assert_ne!(observed.rollback, Folder::Missing, "{case}");
                    }
                    Step::Forget | Step::KeepWhole | Step::Pending => {}
                }
                if phase != Phase::Swapping && observed.target != Folder::Staged {
                    assert_eq!(step, Step::Pending, "{case}");
                }
            }
        }
    }
}

#[test]
fn each_phase_resumes_with_the_step_its_folders_call_for() {
    let seen = |target, rollback, staging| Observed {
        target,
        rollback,
        staging,
        copy_kept: true,
    };
    let flagged = Replaced::SameVault { flagged: true };
    let cases = [
        (
            Phase::Swapping,
            flagged,
            seen(Folder::Staged, Folder::Missing, true),
            Step::Forget,
        ),
        (
            Phase::Swapping,
            flagged,
            seen(Folder::Missing, Folder::Staged, true),
            Step::PutBack,
        ),
        (
            Phase::Swapping,
            flagged,
            seen(Folder::Missing, Folder::Missing, true),
            Step::Pending,
        ),
        (
            Phase::Swapping,
            flagged,
            seen(Folder::Staged, Folder::Staged, false),
            Step::Retain,
        ),
        (
            Phase::Swapping,
            flagged,
            seen(Folder::Staged, Folder::Staged, true),
            Step::Pending,
        ),
        (
            Phase::Swapping,
            flagged,
            seen(Folder::Other, Folder::Staged, false),
            Step::Pending,
        ),
        (
            Phase::Activated,
            flagged,
            seen(Folder::Staged, Folder::Staged, true),
            Step::Retain,
        ),
        (
            Phase::Activated,
            flagged,
            seen(Folder::Staged, Folder::Other, false),
            Step::KeepWhole,
        ),
        (
            Phase::Activated,
            flagged,
            seen(Folder::Staged, Folder::Missing, false),
            Step::Forget,
        ),
        (
            Phase::Activated,
            flagged,
            seen(Folder::Missing, Folder::Staged, false),
            Step::Pending,
        ),
        (
            Phase::Activated,
            Replaced::OtherVault,
            seen(Folder::Staged, Folder::Other, false),
            Step::Retain,
        ),
        (
            Phase::Activated,
            Replaced::Preserve { discard: true },
            seen(Folder::Staged, Folder::Other, false),
            Step::Discard,
        ),
        (
            Phase::Activated,
            Replaced::Preserve { discard: false },
            seen(Folder::Staged, Folder::Other, false),
            Step::Forget,
        ),
        (
            Phase::Retaining,
            flagged,
            seen(Folder::Staged, Folder::Staged, false),
            Step::KeepWhole,
        ),
        (
            Phase::Retained { copy: None },
            flagged,
            seen(Folder::Staged, Folder::Other, false),
            Step::Finish,
        ),
        (
            Phase::Retained {
                copy: Some("copy".to_string()),
            },
            flagged,
            Observed {
                copy_kept: false,
                ..seen(Folder::Staged, Folder::Other, false)
            },
            Step::KeepWhole,
        ),
    ];
    for (phase, replaced, observed, step) in cases {
        assert_eq!(
            next_step(&phase, replaced, observed),
            step,
            "{phase:?} {replaced:?} {observed:?}"
        );
    }
}

/// Writes a migrated database marked with `marker` into `vault`.
async fn write_marker(vault: &Path, marker: i64) {
    ganbaru_db::test_support::write_migrated_database(database_path(vault))
        .await
        .unwrap();
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(vault)).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "PRAGMA application_id = {marker}"
    )))
    .execute(&pool)
    .await
    .unwrap();
    registry.close_all().await.unwrap();
}

async fn read_marker(vault: &Path) -> i64 {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry
        .connect_path_read_only(database_path(vault))
        .await
        .unwrap();
    let marker = sqlx::query_scalar("PRAGMA application_id")
        .fetch_one(&pool)
        .await
        .unwrap();
    registry.close_all().await.unwrap();
    marker
}

/// A vault folder with a large asset and a device file beside a staged replacement.
struct Fixture {
    parent: PathBuf,
    target: PathBuf,
    staging: PathBuf,
    asset: Vec<u8>,
}

const ASSET: &str = "assets/kept.bin";

impl Fixture {
    async fn new(name: &str, same_vault: bool) -> Self {
        let parent = crate::vault::tests::unique_path(name);
        let target = parent.join("Ganbaru");
        let staging = parent.join(STAGING);
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&staging).unwrap();
        crate::vault::initialize_vault(&target).unwrap();
        write_marker(&target, PREVIOUS_MARKER).await;
        let asset = vec![7_u8; 2 * 1024 * 1024];
        fs::create_dir_all(target.join("assets")).unwrap();
        fs::write(target.join(ASSET), &asset).unwrap();
        fs::write(target.join("README.md"), b"device file").unwrap();
        if same_vault {
            fs::copy(
                target.join(VAULT_MANIFEST_FILE),
                staging.join(VAULT_MANIFEST_FILE),
            )
            .unwrap();
        } else {
            crate::vault::initialize_vault(&staging).unwrap();
        }
        write_marker(&staging, REPLACEMENT_MARKER).await;
        Self {
            parent,
            target,
            staging,
            asset,
        }
    }

    fn staged_vault(&self) -> String {
        vault_info_from_path(&self.staging).unwrap().vault_id
    }

    fn retained_rollback(&self) -> PathBuf {
        retained_rollback_path(&self.target).unwrap()
    }

    /// Leaves the folders and the journal as a crash at `phase` would, after `renames` of the
    /// two swap renames.
    fn crash(&self, phase: Phase, replaced: Replaced, rollback: &Path, renames: usize) {
        let journal = Journal {
            phase,
            replaced,
            vault: self.staged_vault(),
            rollback: rollback.file_name().unwrap().to_str().unwrap().to_string(),
            staging: STAGING.to_string(),
        };
        SwapRecord::begin(&self.target, journal).unwrap();
        if renames > 0 {
            fs::rename(&self.target, rollback).unwrap();
        }
        if renames > 1 {
            fs::rename(&self.staging, &self.target).unwrap();
        }
    }

    fn kept_whole(&self) -> Option<PathBuf> {
        fs::read_dir(&self.parent)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("Ganbaru before replacement")
            })
            .map(|entry| entry.path())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}

#[tokio::test]
async fn a_crash_before_any_rename_leaves_both_folders_as_they_were() {
    let fixture = Fixture::new("swap-crash-begin", true).await;
    let rollback = fixture.retained_rollback();
    fixture.crash(
        Phase::Swapping,
        Replaced::SameVault { flagged: true },
        &rollback,
        0,
    );

    assert_eq!(recover(&fixture.target), Ok(Recovery::Settled));

    assert_eq!(read_marker(&fixture.target).await, PREVIOUS_MARKER);
    assert_eq!(read_marker(&fixture.staging).await, REPLACEMENT_MARKER);
    assert!(!rollback.exists());
    assert!(!has_unfinished_swap(&fixture.target));
}

#[tokio::test]
async fn a_crash_between_the_renames_puts_the_replaced_vault_back() {
    let fixture = Fixture::new("swap-crash-between", true).await;
    let rollback = fixture.retained_rollback();
    fixture.crash(
        Phase::Swapping,
        Replaced::SameVault { flagged: true },
        &rollback,
        1,
    );

    assert_eq!(recover(&fixture.target), Ok(Recovery::Settled));

    assert_eq!(read_marker(&fixture.target).await, PREVIOUS_MARKER);
    assert_eq!(fs::read(fixture.target.join(ASSET)).unwrap(), fixture.asset);
    assert!(!rollback.exists());
    assert!(!has_unfinished_swap(&fixture.target));
}

#[tokio::test]
async fn a_flagged_swap_that_crashed_after_both_renames_keeps_the_whole_layout() {
    for phase in [Phase::Swapping, Phase::Activated] {
        let fixture = Fixture::new("swap-crash-flagged", true).await;
        let rollback = fixture.retained_rollback();
        fixture.crash(
            phase.clone(),
            Replaced::SameVault { flagged: true },
            &rollback,
            2,
        );

        assert_eq!(recover(&fixture.target), Ok(Recovery::Settled));

        assert!(!rollback.exists(), "{phase:?}");
        assert_eq!(read_marker(&fixture.target).await, REPLACEMENT_MARKER);
        assert_eq!(
            fs::read(fixture.target.join("README.md")).unwrap(),
            b"device file"
        );
        let rollbacks = list_rollbacks(&fixture.target).unwrap();
        assert_eq!(rollbacks.len(), 1, "{phase:?}");
        assert!(rollbacks[0].retention.flagged(), "{phase:?}");
        restore_rollback(&fixture.target, &rollbacks[0].id, chrono::Utc::now())
            .await
            .unwrap();
        assert_eq!(read_marker(&fixture.target).await, PREVIOUS_MARKER);
        assert_eq!(fs::read(fixture.target.join(ASSET)).unwrap(), fixture.asset);
    }
}

#[tokio::test]
async fn an_activated_restore_of_another_vault_keeps_it_whole_inside_the_new_one() {
    let fixture = Fixture::new("swap-crash-other", false).await;
    let rollback = fixture.retained_rollback();
    fixture.crash(Phase::Activated, Replaced::OtherVault, &rollback, 2);

    assert_eq!(recover(&fixture.target), Ok(Recovery::Settled));

    assert!(!rollback.exists());
    assert_eq!(read_marker(&fixture.target).await, REPLACEMENT_MARKER);
    let rollbacks = list_rollbacks(&fixture.target).unwrap();
    assert_eq!(rollbacks.len(), 1);
    assert_eq!(rollbacks[0].retention.origin(), RollbackOrigin::Replacement);
}

#[tokio::test]
async fn an_activated_preserve_discards_only_when_asked() {
    for discard in [false, true] {
        let fixture = Fixture::new("swap-crash-preserve", false).await;
        let folder = fixture.parent.join("Ganbaru before linking t1");
        fixture.crash(Phase::Activated, Replaced::Preserve { discard }, &folder, 2);

        assert_eq!(recover(&fixture.target), Ok(Recovery::Settled));

        assert_eq!(folder.exists(), !discard);
        assert_eq!(read_marker(&fixture.target).await, REPLACEMENT_MARKER);
        assert!(list_rollbacks(&fixture.target).unwrap().is_empty());
        assert!(!has_unfinished_swap(&fixture.target));
    }
}

#[tokio::test]
async fn a_crash_while_retaining_keeps_the_replaced_folder_whole() {
    let fixture = Fixture::new("swap-crash-retaining", true).await;
    let rollback = fixture.retained_rollback();
    fixture.crash(
        Phase::Retaining,
        Replaced::SameVault { flagged: true },
        &rollback,
        2,
    );

    assert_eq!(recover(&fixture.target), Ok(Recovery::Settled));

    assert!(!rollback.exists());
    let kept = fixture
        .kept_whole()
        .expect("the replaced folder is kept beside the vault");
    assert_eq!(read_marker(&kept).await, PREVIOUS_MARKER);
    assert_eq!(fs::read(kept.join(ASSET)).unwrap(), fixture.asset);
    assert_eq!(read_marker(&fixture.target).await, REPLACEMENT_MARKER);
}

#[tokio::test]
async fn a_crash_after_retaining_finishes_only_while_the_copy_is_there() {
    for copy_kept in [true, false] {
        let fixture = Fixture::new("swap-crash-retained", true).await;
        let rollback = fixture.retained_rollback();
        let vault = fixture.staged_vault();
        fixture.crash(
            Phase::Retaining,
            Replaced::SameVault { flagged: true },
            &rollback,
            2,
        );
        let copy = database_rollback::retain_replaced_vault(
            &rollback,
            &vault,
            true,
            crate::vault::now_utc(),
        )
        .unwrap()
        .unwrap();
        let mut record = SwapRecord::load(&fixture.target).unwrap().unwrap().unwrap();
        record
            .advance(Event::RetentionKept {
                copy: Some(if copy_kept {
                    copy
                } else {
                    "20260101T000000000Z".to_string()
                }),
            })
            .unwrap();

        assert_eq!(recover(&fixture.target), Ok(Recovery::Settled));

        assert!(!rollback.exists());
        assert_eq!(fixture.kept_whole().is_some(), !copy_kept);
        assert_eq!(
            list_rollbacks(&fixture.target).unwrap().len(),
            usize::from(copy_kept)
        );
        assert_eq!(
            fs::read(fixture.target.join("README.md")).is_ok(),
            copy_kept,
            "a finished swap carries the device file into the new vault"
        );
    }
}

#[tokio::test]
async fn folders_no_swap_leaves_stay_pending_and_refuse_the_next_swap() {
    let fixture = Fixture::new("swap-pending", true).await;
    let rollback = fixture.retained_rollback();
    fixture.crash(
        Phase::Activated,
        Replaced::SameVault { flagged: false },
        &rollback,
        2,
    );
    fs::write(fixture.target.join(VAULT_MANIFEST_FILE), "not a marker").unwrap();

    assert!(matches!(recover(&fixture.target), Ok(Recovery::Pending(_))));
    assert_eq!(read_marker(&rollback).await, PREVIOUS_MARKER);
    assert!(has_unfinished_swap(&fixture.target));

    let next = fixture.parent.join(".ganbaru.next");
    fs::create_dir_all(&next).unwrap();
    assert_eq!(
        swap(
            &next,
            &fixture.target,
            Replacement::SameVault { flagged: false }
        ),
        Err(SWAP_RECOVERY_PENDING.to_string())
    );
    assert!(next.exists());
    assert_eq!(read_marker(&rollback).await, PREVIOUS_MARKER);
}

#[tokio::test]
async fn an_undecodable_journal_touches_nothing() {
    let fixture = Fixture::new("swap-undecodable", true).await;
    let rollback = fixture.retained_rollback();
    fixture.crash(
        Phase::Swapping,
        Replaced::SameVault { flagged: false },
        &rollback,
        1,
    );
    fs::write(journal_path(&fixture.target).unwrap(), b"{}").unwrap();

    assert!(matches!(recover(&fixture.target), Ok(Recovery::Pending(_))));
    assert!(!fixture.target.exists());
    assert_eq!(read_marker(&rollback).await, PREVIOUS_MARKER);
}

#[tokio::test]
async fn a_swap_refuses_folders_outside_the_target_parent() {
    let fixture = Fixture::new("swap-side-by-side", true).await;
    let elsewhere = fixture.parent.join("nested").join(STAGING);
    fs::create_dir_all(&elsewhere).unwrap();

    for (staging, replacement) in [
        (&elsewhere, Replacement::SameVault { flagged: false }),
        (
            &fixture.staging,
            Replacement::Preserve {
                folder: fixture.parent.join("nested").join("kept"),
                discard: false,
            },
        ),
        (&fixture.target, Replacement::OtherVault),
    ] {
        assert!(swap(staging, &fixture.target, replacement).is_err());
    }
    assert_eq!(read_marker(&fixture.target).await, PREVIOUS_MARKER);
    assert!(!has_unfinished_swap(&fixture.target));
}
