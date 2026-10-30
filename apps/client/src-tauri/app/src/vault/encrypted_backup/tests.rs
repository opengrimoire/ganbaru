use super::*;
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
            plan_restore(active, backup),
            expected,
            "{active:?} {backup:?}"
        );
    }
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
        Ok(Unlock::Kit(_))
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
    let info = stage_restored_vault(&archive, &staging, None)
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
        plan_restore(Some(&once_linked), &summary("vault-b", 0)),
        RestorePlacement::IndependentCopy
    );
    assert_eq!(
        plan_restore(Some(&once_linked), &summary("vault-a", 0)),
        RestorePlacement::IndependentCopy
    );
    fs::remove_dir_all(root).unwrap();
}
