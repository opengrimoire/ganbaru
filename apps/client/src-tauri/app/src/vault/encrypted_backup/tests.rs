use super::*;
use crate::sync::backup_keys::LogAuthority;
use ganbaru_sync_contracts::RecoverySeed;

fn summary(vault: &str, other_devices: usize) -> LinkSummary {
    LinkSummary {
        space: SpaceId::personal(vault),
        other_devices,
    }
}

fn unique_test_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ganbaru-encrypted-backup-test-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn restore_outcome_matrix() {
    use RestorePlacement::{InPlace, IndependentCopy};
    let alone = summary("vault-a", 0);
    let enrolled = summary("vault-a", 2);
    let other = summary("vault-b", 0);
    let cases = [
        // Same space, no other device ever enrolled: the only restore in place.
        (Some(&alone), &alone, InPlace),
        // Another space always becomes an independent copy.
        (Some(&alone), &other, IndependentCopy),
        // A vault whose log enrolled other devices keeps its space and gets a copy.
        (Some(&enrolled), &alone, IndependentCopy),
        (Some(&enrolled), &other, IndependentCopy),
        // A backup taken while linked never rewinds the space it came from.
        (Some(&alone), &enrolled, IndependentCopy),
        // Nothing active: a copy.
        (None, &alone, IndependentCopy),
        (None, &enrolled, IndependentCopy),
    ];
    for (active, backup, expected) in cases {
        assert_eq!(
            plan_restore(active, backup, true),
            expected,
            "{active:?} {backup:?}"
        );
    }
}

/// The public keys of the kit derived from `seed`.
fn kit(seed: u8) -> RecoveryPublicKeys {
    RecoverySeed::from_bytes([seed; 32]).public_keys().unwrap()
}

/// The keys of an unlinked vault that stores `kit` and has no space log.
fn solo(kit: RecoveryPublicKeys) -> RestoreKeys {
    RestoreKeys {
        recovery: Some(kit.encryption),
        stored: Some(kit),
        log: None,
    }
}

/// The keys of a vault that stores `kit` and whose log, founded with it, has `charter`.
fn founded(kit: RecoveryPublicKeys, charter: [u8; 32]) -> RestoreKeys {
    RestoreKeys {
        log: Some(LogAuthority {
            charter,
            kit_admin: kit.admin,
            recovery_admin: kit.admin,
        }),
        ..solo(kit)
    }
}

/// The keys of a vault with no kit and no log, such as a fresh install.
const EMPTY: RestoreKeys = RestoreKeys {
    recovery: None,
    stored: None,
    log: None,
};

#[test]
fn a_forged_backup_with_another_recovery_key_is_never_in_place() {
    // The file is sealed to the victim's kit, which opens it, but the staged vault inside
    // names the attacker's recovery key, so later backups would be sealed to the attacker.
    let victim = kit(0x42);
    let active = solo(victim);
    let staged = solo(kit(0x66));
    let trusted = same_authority(&active, &staged, Some(&victim));
    assert!(
        !trusted,
        "a staged key other than the opener is not trusted"
    );
    let alone = summary("vault-a", 0);
    assert_eq!(
        plan_restore(Some(&alone), &alone, trusted),
        RestorePlacement::IndependentCopy
    );
}

#[test]
fn a_forged_backup_with_another_kit_admin_key_is_never_in_place() {
    // The staged vault stores the victim's public recovery encryption key, so the kit opens
    // the file and the encryption keys match, but its admin key is the attacker's, which a
    // later founding would name as the kit admin of the space.
    let victim = kit(0x42);
    let forged = RecoveryPublicKeys {
        admin: kit(0x66).admin,
        encryption: victim.encryption,
    };
    let alone = summary("vault-a", 0);
    for opener in [Some(&victim), None] {
        let trusted = same_authority(&solo(victim), &solo(forged), opener);
        assert!(!trusted, "{opener:?}");
        assert_eq!(
            plan_restore(Some(&alone), &alone, trusted),
            RestorePlacement::IndependentCopy
        );
    }
    // A fresh install compares the staged row with the kit the user entered.
    assert!(!same_authority(&EMPTY, &solo(forged), Some(&victim)));
    // A staged log whose kit admin is not the opener's, or not the active log's.
    let charter = [7; 32];
    let mut staged = founded(victim, charter);
    if let Some(log) = staged.log.as_mut() {
        log.kit_admin = forged.admin;
    }
    let same_log = RestoreKeys {
        stored: None,
        ..staged
    };
    assert!(!same_authority(&same_log, &staged, Some(&victim)));
    assert!(!same_authority(
        &founded(victim, charter),
        &staged,
        Some(&victim)
    ));
    assert!(!same_authority(&founded(victim, charter), &staged, None));
    // An epoch-opened file needs the whole active kit row: a log key alone leaves the admin
    // key the kit derives unknown.
    let published = RestoreKeys {
        stored: None,
        ..founded(victim, charter)
    };
    let forged_row = RestoreKeys {
        stored: Some(forged),
        ..founded(victim, charter)
    };
    assert!(!same_authority(&published, &forged_row, None));
    // A forged row beside an honest log fails against the active row too.
    assert!(!same_authority(
        &founded(victim, charter),
        &forged_row,
        Some(&victim)
    ));
}

#[test]
fn an_honest_backup_of_the_current_kit_restores_in_place() {
    let ours = kit(0x42);
    let alone = summary("vault-a", 0);
    for keys in [solo(ours), founded(ours, [7; 32])] {
        for opener in [Some(&ours), None] {
            let trusted = same_authority(&keys, &keys, opener);
            assert!(trusted, "{keys:?} {opener:?}");
            assert_eq!(
                plan_restore(Some(&alone), &alone, trusted),
                RestorePlacement::InPlace
            );
        }
    }
    // A fresh install with no kit row trusts a backup the kit itself opened.
    assert!(same_authority(&EMPTY, &solo(ours), Some(&ours)));
}

#[test]
fn an_epoch_opened_backup_needs_a_known_active_key() {
    // A removed device still holds old epoch keys, so a file only an epoch key opened needs
    // recovery keys this device already stores.
    let forged = solo(kit(0x66));
    assert!(!same_authority(&EMPTY, &forged, None));
    assert!(!same_authority(&EMPTY, &EMPTY, None));
    assert!(!same_authority(&EMPTY, &EMPTY, Some(&kit(0x42))));
}

#[test]
fn a_backup_older_than_a_kit_change_is_an_independent_copy() {
    let old = kit(0x42);
    let current = kit(0x43);
    let active = solo(current);
    let staged = solo(old);
    let alone = summary("vault-a", 0);
    for opener in [Some(&old), Some(&current), None] {
        let trusted = same_authority(&active, &staged, opener);
        assert!(!trusted, "{opener:?}");
        assert_eq!(
            plan_restore(Some(&alone), &alone, trusted),
            RestorePlacement::IndependentCopy
        );
    }
    // A backup whose charter differs from the active one, such as a kit-only backup restored
    // after the first link, is a copy too.
    assert!(!same_authority(
        &founded(current, [7; 32]),
        &solo(current),
        Some(&current)
    ));
}

#[test]
fn a_private_archive_keeps_its_authority_and_a_backup_file_needs_an_active_vault() {
    let keys = solo(kit(0x42));
    assert!(ArchiveOrigin::PrivateArchive.trusts(None, &keys));
    let file = ArchiveOrigin::BackupFile {
        space: SpaceId::personal("vault-a"),
        opened_by: Some(kit(0x42)),
    };
    assert!(file.trusts(Some(&keys), &keys));
    assert!(!file.trusts(None, &keys));
}

/// Counts the rows of `table` in the database of the vault at `root`.
async fn count_rows(root: &Path, table: &str) -> i64 {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(root)).await.unwrap();
    let rows: i64 =
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
            .fetch_one(&pool)
            .await
            .unwrap();
    registry.close_all().await.unwrap();
    rows
}

#[tokio::test]
async fn staging_a_backup_file_clears_forged_local_rows() {
    use super::super::replaced_vaults::{replaced_vault_backup_path, write_replaced_vault_backup};
    let root = unique_test_path("forged-local");
    let vault = root.join("Ganbaru");
    fs::create_dir_all(&vault).unwrap();
    let original = crate::vault::initialize_vault(&vault).unwrap();
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(&vault)).await.unwrap();
    {
        let mut conn = pool.acquire().await.unwrap();
        ganbaru_sync::Engine::vault()
            .init_space(&mut conn, &original.vault_id, 1_000)
            .await
            .unwrap();
        let recovery = RecoverySeed::from_bytes([0x66; 32]).public_keys().unwrap();
        ganbaru_sync::local::store_recovery_kit_keys(&mut conn, &recovery, 1_000)
            .await
            .unwrap();
    }
    // A forged space key row, which a device would otherwise read as its own.
    sqlx::query(
        "INSERT INTO sync_space_keys (space_id, epoch, wrapped_previous, own_envelope, sequenced_revision)
         SELECT space_id, 1, NULL, NULL, 1 FROM sync_spaces",
    )
    .execute(&pool)
    .await
    .unwrap();
    registry.close_all().await.unwrap();
    assert_eq!(count_rows(&vault, "sync_space_keys").await, 1);
    let kept = root.join("replaced-vaults");
    let backup = write_replaced_vault_backup(&kept, &vault, 1_000)
        .await
        .unwrap();
    let archive = replaced_vault_backup_path(&kept, &backup.id).unwrap();

    let from_file = root.join("from-file");
    let origin = ArchiveOrigin::BackupFile {
        space: SpaceId::personal(&original.vault_id),
        opened_by: Some(kit(0x42)),
    };
    stage_restored_vault(&archive, &from_file, origin)
        .await
        .unwrap();
    assert_eq!(count_rows(&from_file, "sync_space_keys").await, 0);
    assert_eq!(
        count_rows(&from_file, "sync_recovery_kit").await,
        1,
        "the recovery key row stays for the authority comparison"
    );
    let (_, staged) = read_staged_state(&from_file, &original.vault_id, "device-a")
        .await
        .unwrap();
    assert_eq!(staged.recovery, Some(kit(0x66).encryption));
    assert_eq!(staged.stored, Some(kit(0x66)));
    assert!(!same_authority(
        &RestoreKeys {
            log: staged.log,
            ..solo(kit(0x42))
        },
        &staged,
        Some(&kit(0x42)),
    ));

    let private = root.join("private");
    stage_restored_vault(&archive, &private, ArchiveOrigin::PrivateArchive)
        .await
        .unwrap();
    assert_eq!(
        count_rows(&private, "sync_space_keys").await,
        1,
        "this device's own archive keeps its device rows"
    );

    let elsewhere = ArchiveOrigin::BackupFile {
        space: SpaceId::personal("another-vault"),
        opened_by: None,
    };
    assert!(matches!(
        stage_restored_vault(&archive, &root.join("elsewhere"), elsewhere).await,
        Err(RestoreError::Refused(BackupRefusal::Damaged))
    ));
    fs::remove_dir_all(root).unwrap();
}

/// Creates a vault at `vault` with a founded space that stores the kit of `seed`, and returns
/// its id.
async fn founded_vault(vault: &Path, seed: u8) -> String {
    fs::create_dir_all(vault).unwrap();
    let info = crate::vault::initialize_vault(vault).unwrap();
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(vault)).await.unwrap();
    {
        let mut conn = pool.acquire().await.unwrap();
        ganbaru_sync::local::store_recovery_kit_keys(&mut conn, &kit(seed), 1_000)
            .await
            .unwrap();
        ganbaru_sync::Engine::vault()
            .init_space(&mut conn, &info.vault_id, 1_000)
            .await
            .unwrap();
    }
    registry.close_all().await.unwrap();
    info.vault_id
}

/// Archives the vault at `vault` to `archive` as a decrypted backup holds it. `forge` runs on
/// its database first, as a forger is free to do, and `undo` runs after, so the vault stays
/// one this app opens.
async fn archive_vault_with(vault: &Path, archive: &Path, forge: &str, undo: &str) {
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(vault)).await.unwrap();
    let run = async |statements: &str| {
        if !statements.is_empty() {
            sqlx::raw_sql(sqlx::AssertSqlSafe(statements.to_string()))
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    run(forge).await;
    let snapshot = archive.with_extension("sqlite");
    ganbaru_sync_replica::backup::create_backup_archive(
        &ArchiveGate::process(),
        &pool,
        vault,
        &snapshot,
        archive,
    )
    .await
    .unwrap();
    fs::remove_file(snapshot).unwrap();
    run(undo).await;
    registry.close_all().await.unwrap();
}

/// The vault at `vault` as a restore on the device `device_id` sees it when it is active.
async fn active_vault(vault: &Path, vault_id: &str, device_id: &str) -> ActiveVault {
    let (summary, keys) = read_staged_state(vault, vault_id, device_id).await.unwrap();
    ActiveVault {
        root: vault.to_path_buf(),
        summary,
        keys,
        linked: false,
        epoch_key: None,
    }
}

#[tokio::test]
async fn placement_restores_an_honest_backup_in_place_and_a_forged_one_as_a_copy() {
    let root = unique_test_path("placement");
    let vault = root.join("Ganbaru");
    let device = "device-a";
    let vault_id = founded_vault(&vault, 0x42).await;
    let active = active_vault(&vault, &vault_id, device).await;
    let honest = root.join("honest.zip");
    archive_vault_with(&vault, &honest, "", "").await;
    // The forger knows only the victim's recovery public key: the file opens with the
    // victim's kit, but the vault inside stores the attacker's kit.
    let forged = root.join("forged.zip");
    {
        let registry = ganbaru_db::DatabasePoolRegistry::default();
        let pool = registry.connect_path(database_path(&vault)).await.unwrap();
        let mut conn = pool.acquire().await.unwrap();
        ganbaru_sync::local::store_recovery_kit_keys(&mut conn, &kit(0x66), 2_000)
            .await
            .unwrap();
        drop(conn);
        registry.close_all().await.unwrap();
    }
    archive_vault_with(&vault, &forged, "", "").await;
    let space = SpaceId::personal(&vault_id);
    let opened_by_kit = ArchiveOrigin::BackupFile {
        space,
        opened_by: Some(kit(0x42)),
    };
    let opened_by_epoch = ArchiveOrigin::BackupFile {
        space,
        opened_by: None,
    };
    let cases = [
        (
            "honest, kit",
            &honest,
            opened_by_kit,
            Some(&active),
            RestorePlacement::InPlace,
        ),
        (
            "honest, epoch",
            &honest,
            opened_by_epoch,
            Some(&active),
            RestorePlacement::InPlace,
        ),
        (
            "honest, nothing active",
            &honest,
            opened_by_kit,
            None,
            RestorePlacement::IndependentCopy,
        ),
        (
            "forged, kit",
            &forged,
            opened_by_kit,
            Some(&active),
            RestorePlacement::IndependentCopy,
        ),
        (
            "forged, epoch",
            &forged,
            opened_by_epoch,
            Some(&active),
            RestorePlacement::IndependentCopy,
        ),
    ];
    for (index, (name, archive, origin, active, expected)) in cases.into_iter().enumerate() {
        let staging = root.join(format!("staging-{index}"));
        let (info, placement) = stage_and_plan(device, archive, &staging, origin, active)
            .await
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(info.vault_id, vault_id, "{name}");
        assert_eq!(placement, expected, "{name}");
    }
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_backup_whose_database_carries_extra_schema_objects_is_refused() {
    let root = unique_test_path("forged-schema");
    let vault = root.join("Ganbaru");
    let device = "device-a";
    let vault_id = founded_vault(&vault, 0x42).await;
    let active = active_vault(&vault, &vault_id, device).await;
    let origin = ArchiveOrigin::BackupFile {
        space: SpaceId::personal(&vault_id),
        opened_by: Some(kit(0x42)),
    };
    // Each forged object survives row checks: the trigger plants the attacker's admin key
    // when the user later saves a kit, and the others reshape what restore reads.
    let forged_objects = [
        (
            "CREATE TRIGGER plant_admin AFTER UPDATE ON sync_recovery_kit BEGIN \
             UPDATE sync_recovery_kit SET recovery_admin_key = zeroblob(32); END",
            "DROP TRIGGER plant_admin",
        ),
        (
            "CREATE TRIGGER refill_keys AFTER DELETE ON sync_space_keys BEGIN \
             INSERT INTO sync_space_keys VALUES (old.space_id, old.epoch, \
             old.wrapped_previous, old.own_envelope, old.sequenced_revision); END",
            "DROP TRIGGER refill_keys",
        ),
        (
            "CREATE VIEW forged_view AS SELECT * FROM sync_recovery_kit",
            "DROP VIEW forged_view",
        ),
        (
            "CREATE INDEX forged_index ON sync_recovery_kit (created_at_ms)",
            "DROP INDEX forged_index",
        ),
        (
            "CREATE TABLE forged_table (value TEXT)",
            "DROP TABLE forged_table",
        ),
    ];
    for (index, (forge, undo)) in forged_objects.into_iter().enumerate() {
        let archive = root.join(format!("forged-{index}.zip"));
        archive_vault_with(&vault, &archive, forge, undo).await;
        let staging = root.join(format!("staging-{index}"));
        let staged = stage_and_plan(device, &archive, &staging, origin, Some(&active)).await;
        match staged {
            Err(RestoreError::Failed(error)) => assert!(
                error.contains("schema differs"),
                "{forge}: refused for another reason: {error}"
            ),
            Err(error) => panic!("{forge}: refused for another reason: {error:?}"),
            Ok((_, placement)) => panic!("{forge}: staged as {placement:?}"),
        }
    }
    // The same vault without forged objects still restores in place.
    let honest = root.join("honest.zip");
    archive_vault_with(&vault, &honest, "", "").await;
    let (_, placement) = stage_and_plan(
        device,
        &honest,
        &root.join("staging-honest"),
        origin,
        Some(&active),
    )
    .await
    .unwrap();
    assert_eq!(placement, RestorePlacement::InPlace);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn format_errors_map_to_refusals_the_user_can_act_on() {
    assert_eq!(
        format_refusal(BackupError::NotBackup, false),
        Some(BackupRefusal::NotBackup)
    );
    assert_eq!(
        format_refusal(BackupError::Version(9), true),
        Some(BackupRefusal::UnsupportedVersion)
    );
    for damaged in [
        BackupError::Truncated,
        BackupError::Malformed,
        BackupError::TooLarge,
        BackupError::Authentication,
        BackupError::Finished,
    ] {
        assert_eq!(format_refusal(damaged, true), Some(BackupRefusal::Damaged));
        assert_eq!(format_refusal(damaged, false), Some(BackupRefusal::Damaged));
    }
    for key in [BackupError::Key, BackupError::Epoch] {
        assert_eq!(format_refusal(key, true), Some(BackupRefusal::WrongKit));
        assert_eq!(format_refusal(key, false), Some(BackupRefusal::KitRequired));
    }
    assert_eq!(format_refusal(BackupError::Random, false), None);
    assert_eq!(format_refusal(BackupError::Seal, true), None);
}

#[test]
fn only_kit_refusals_keep_the_picked_file() {
    assert!(BackupRefusal::KitRequired.keeps_file());
    assert!(BackupRefusal::WrongKit.keeps_file());
    assert!(BackupRefusal::InvalidKit.keeps_file());
    assert!(!BackupRefusal::Damaged.keeps_file());
    assert!(!BackupRefusal::LinkedAndroid.keeps_file());
    assert!(!BackupRefusal::NotBackup.keeps_file());
}

#[test]
fn the_kit_wins_over_the_epoch_key_and_a_mistyped_kit_is_refused() {
    let code = RecoverySeed::from_bytes([0x42; 32]).kit_code();
    let epoch = || Some(SpaceKey::generate(1).unwrap());
    assert!(matches!(
        Unlock::choose(Some(code), epoch()),
        Ok(Unlock::Kit(_, public)) if public == kit(0x42)
    ));
    assert!(matches!(
        Unlock::choose(None, epoch()),
        Ok(Unlock::Epoch(_))
    ));
    assert!(matches!(
        Unlock::choose(None, None),
        Err(BackupRefusal::KitRequired)
    ));
    assert!(matches!(
        Unlock::choose(Some("not a kit".to_string()), epoch()),
        Err(BackupRefusal::InvalidKit)
    ));
}

#[test]
fn outcomes_serialize_with_the_webview_field_names() {
    let saved = serde_json::to_value(BackupSaveOutcome::Saved {
        file_name: "backup.ganbaru-backup".to_string(),
    })
    .unwrap();
    assert_eq!(
        saved,
        serde_json::json!({"kind": "saved", "fileName": "backup.ganbaru-backup"})
    );
    let ready = serde_json::to_value(BackupInspection::Ready {
        token: "abc".to_string(),
        needs_kit: true,
    })
    .unwrap();
    assert_eq!(
        ready,
        serde_json::json!({"kind": "ready", "token": "abc", "needsKit": true})
    );
    let refused = serde_json::to_value(BackupRestoreOutcome::Refused {
        reason: BackupRefusal::LinkedAndroid,
    })
    .unwrap();
    assert_eq!(
        refused,
        serde_json::json!({"kind": "refused", "reason": "linkedAndroid"})
    );
    assert_eq!(
        serde_json::to_value(RestorePlacement::IndependentCopy).unwrap(),
        serde_json::json!("independentCopy")
    );
}

#[test]
fn backup_file_names_carry_the_extension_and_no_colons() {
    let name = backup_file_name();
    assert!(name.starts_with("ganbaru-backup-"));
    assert!(name.ends_with(".ganbaru-backup"));
    assert!(!name.contains(':'));
}

#[test]
fn stale_transfers_are_removed_by_prefix_only() {
    let root = unique_test_path("transfers");
    fs::create_dir_all(root.join("restore-old")).unwrap();
    fs::create_dir_all(root.join("save-kept")).unwrap();
    fs::write(root.join("restore-file"), b"kept").unwrap();
    remove_stale_transfers(&root, RESTORE_TRANSFER_PREFIX);
    assert!(!root.join("restore-old").exists());
    assert!(root.join("save-kept").exists());
    assert!(root.join("restore-file").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_sweep_clears_transfers_and_restore_staging() {
    let root = unique_test_path("startup-sweep");
    let transfers = root.join("vault-backups");
    let documents = root.join("documents");
    let vault = documents.join("Ganbaru");
    let sibling = documents.join("Other");
    for folder in [
        transfers.join("restore-picked"),
        transfers.join("save-written"),
        transfers.join(".ganbaru-backup-killed"),
        transfers.join("unrelated"),
        documents.join(RESTORE_STAGING_NAME),
        documents.join(".ganbaru.restore-copy/vault"),
        vault.clone(),
        sibling.clone(),
    ] {
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("data"), b"plaintext").unwrap();
    }

    sweep_interrupted_backups_at(&transfers, Some(&vault)).unwrap();

    for removed in [
        transfers.join("restore-picked"),
        transfers.join("save-written"),
        transfers.join(".ganbaru-backup-killed"),
        documents.join(RESTORE_STAGING_NAME),
        documents.join(".ganbaru.restore-copy"),
    ] {
        assert!(!removed.exists(), "{} is swept", removed.display());
    }
    for kept in [transfers.join("unrelated"), vault, sibling] {
        assert!(kept.join("data").exists(), "{} stays", kept.display());
    }
    sweep_interrupted_backups_at(&root.join("missing"), None).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_staged_folder_is_removed_unless_kept() {
    let root = unique_test_path("staged");
    let dropped = root.join("dropped");
    let kept = root.join("kept");
    fs::create_dir_all(&dropped).unwrap();
    fs::create_dir_all(&kept).unwrap();
    drop(StagedFolder::new(&dropped));
    StagedFolder::new(&kept).keep();
    assert!(!dropped.exists());
    assert!(kept.exists());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn an_independent_copy_drops_identity_kit_and_sync_state_but_keeps_domain_rows() {
    let root = unique_test_path("detach");
    fs::create_dir_all(&root).unwrap();
    let original = crate::vault::initialize_vault(&root).unwrap();
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(&root)).await.unwrap();
    let stamp = "2026-10-30T12:00:00.000Z";
    sqlx::query(
        "INSERT INTO contacts_local_identity (singleton, public_key, created_at) VALUES (1, ?, ?)",
    )
    .bind("k".repeat(43))
    .bind(stamp)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contact_card (key, card_nonce, card_revision, updated_at) VALUES ('card', ?, 1, ?)",
    )
    .bind("n".repeat(22))
    .bind(stamp)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sync_recovery_kit (id, recovery_admin_key, recovery_encryption_key, created_at_ms) VALUES (1, ?, ?, 0)",
    )
    .bind(vec![1u8; 32])
    .bind(vec![2u8; 32])
    .execute(&pool)
    .await
    .unwrap();
    let triggers_before: Vec<String> =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'trigger' ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
    let themes_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM themes")
        .fetch_one(&pool)
        .await
        .unwrap();
    registry.close_all().await.unwrap();

    detach_restored_copy(&root).await.unwrap();

    let detached = read_vault_manifest(&root).unwrap();
    assert_ne!(detached.vault_id, original.vault_id);
    assert_eq!(detached.display_name, original.display_name);
    let pool = registry.connect_path(database_path(&root)).await.unwrap();
    for table in [
        "contacts_local_identity",
        "contact_card",
        "sync_recovery_kit",
    ] {
        let rows: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rows, 0, "{table}");
    }
    let triggers_after: Vec<String> =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'trigger' ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(triggers_after, triggers_before);
    let themes_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM themes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(themes_after, themes_before);
    let delete_card = sqlx::query("DELETE FROM contact_card").execute(&pool).await;
    assert!(delete_card.is_ok(), "an empty table deletes nothing");
    registry.close_all().await.unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_replaced_vault_copy_stages_as_the_vault_it_was_and_detaches_into_a_new_one() {
    use super::super::replaced_vaults::{replaced_vault_backup_path, write_replaced_vault_backup};
    let root = unique_test_path("replaced-restore");
    let vault = root.join("Ganbaru");
    fs::create_dir_all(&vault).unwrap();
    let original = crate::vault::initialize_vault(&vault).unwrap();
    let registry = ganbaru_db::DatabasePoolRegistry::default();
    let pool = registry.connect_path(database_path(&vault)).await.unwrap();
    let themes_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM themes")
        .fetch_one(&pool)
        .await
        .unwrap();
    registry.close_all().await.unwrap();
    let kept = root.join("replaced-vaults");
    let backup = write_replaced_vault_backup(&kept, &vault, 1_000)
        .await
        .unwrap();

    let archive = replaced_vault_backup_path(&kept, &backup.id).unwrap();
    let staging = root.join(RESTORE_STAGING_NAME);
    let info = stage_restored_vault(&archive, &staging, ArchiveOrigin::PrivateArchive)
        .await
        .unwrap();
    assert_eq!(info.vault_id, original.vault_id);
    let pool = registry
        .connect_path(database_path(&staging))
        .await
        .unwrap();
    let themes_staged: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM themes")
        .fetch_one(&pool)
        .await
        .unwrap();
    registry.close_all().await.unwrap();
    assert_eq!(themes_staged, themes_before);

    detach_restored_copy(&staging).await.unwrap();
    assert_ne!(
        read_vault_manifest(&staging).unwrap().vault_id,
        original.vault_id
    );
    assert!(archive.is_file(), "the kept copy stays restorable");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_replaced_vault_restore_reads_only_known_copies_in_the_private_folder() {
    use super::super::replaced_vaults::replaced_vault_backup_path;
    let root = unique_test_path("replaced-unknown");
    let kept = root.join("replaced-vaults");
    fs::create_dir_all(&kept).unwrap();
    fs::write(root.join("outside.ganbaru-backup"), b"zip").unwrap();
    for id in [
        "replaced-1000.ganbaru-backup",
        "../outside.ganbaru-backup",
        "replaced-1000.ganbaru-backup/../../outside.ganbaru-backup",
        "",
    ] {
        assert_eq!(
            replaced_vault_backup_path(&kept, id),
            Err("replaced vault backup is unknown".to_string()),
            "{id}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_linked_android_phone_refuses_to_restore_another_vault_until_it_unlinks() {
    use crate::vault::handoff::receiver::{is_linked, test_linked_phone};
    use RestorePlatform::{Android, Desktop};
    let root = unique_test_path("linked-phone");
    let phone = test_linked_phone(&root);
    let linked = is_linked(&phone).unwrap();
    assert!(linked);
    assert_eq!(
        refuse_linked(Android, linked),
        Err(BackupRefusal::LinkedAndroid)
    );
    assert_eq!(refuse_linked(Desktop, linked), Ok(()));
    assert_eq!(
        refuse_other_vault_while_linked(Android, linked),
        Err(LINKED_ANDROID_RESTORE_ERROR.to_string())
    );

    // Unlinking clears the coordinator pin. The log still lists the devices it once
    // enrolled, which only decides that the restored copy detaches into a new space.
    phone.unlink().unwrap();
    let linked = is_linked(&phone).unwrap();
    assert!(!linked);
    assert_eq!(refuse_linked(Android, linked), Ok(()));
    assert_eq!(refuse_other_vault_while_linked(Android, linked), Ok(()));
    let once_linked = summary("vault-a", 2);
    assert_eq!(
        plan_restore(Some(&once_linked), &summary("vault-b", 0), true),
        RestorePlacement::IndependentCopy
    );
    assert_eq!(
        plan_restore(Some(&once_linked), &summary("vault-a", 0), true),
        RestorePlacement::IndependentCopy
    );
    fs::remove_dir_all(root).unwrap();
}
