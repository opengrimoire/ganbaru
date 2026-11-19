//! Recovery kit handovers: a regenerated recovery key beside the printed kit, kits that promote
//! or force the anchor to a member desktop, kit takeovers of the recovery key, and kit rotation.

use super::{
    LAPTOP_DEVICE, commit_sequenced, enroll_mobile, forwarding_hub, laptop_ids, sequence_here,
    view_of,
};
use crate::authority::{self, DeviceKeyPublication, Enrollment};
use crate::client;
use crate::keys::SpaceKeys;
use crate::outbox::{MAX_OUTBOX_PROPOSALS, Outbox};
use crate::tests::exchange::{HUB_DEVICE, Replica, device_key, hub_keys, hub_recovery_seed};
use crate::tests::{NOW_MS, TestDirectory, VAULT_ID, acquire, block_on};
use crate::writer::{SyncKey, SyncKeyStore};
use ganbaru_sync::SpaceKeyHolder;
use ganbaru_sync_contracts::{
    AdminKeyPair, Content, DeviceKind, EncryptionKeyPair, EncryptionPublicKey, Entry, Envelope,
    KeyEpoch, ProposalRejection, SignedProposal, VersionVector,
};
use ganbaru_sync_contracts::{KitCode, RecoverySeed};
use std::sync::Arc;

/// The code of the recovery kit waiting in `keys`, which tests compare because seeds have no
/// equality.
fn pending_code(keys: &dyn crate::writer::SyncKeyStore) -> Option<KitCode> {
    authority::pending_recovery_seed(keys, VAULT_ID)
        .expect("pending seed")
        .map(|seed| seed.kit_code())
}

#[test]
fn a_regenerated_recovery_key_recovers_beside_the_printed_kit() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-recovery-kit", HUB_DEVICE).await;
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let keys = hub_replica.directory.keys();
        let outbox_path = hub_replica.directory.files().outbox_path();
        let admin = hub_keys().admin;
        let regenerate = |outbox: &mut Outbox, view: &ganbaru_sync::AuthorizationView| {
            authority::regenerate_recovery_kit(&keys, VAULT_ID, outbox, view, space, &admin, NOW_MS)
        };

        // The hub publishes its device key and starts the key epochs with the founding kit.
        let space_keys = Arc::new(SpaceKeys::new(
            device_key(HUB_DEVICE),
            EncryptionKeyPair::generate().expect("encryption key"),
        ));
        hub_replica.writer.attach_keys(space_keys.clone());
        {
            let mut conn = acquire(&hub_replica.pool).await;
            let mut outbox = Outbox::load(&outbox_path).expect("outbox");
            let published = authority::publish_device_key(
                &hub_replica.engine,
                &mut conn,
                &hub_replica.ctx,
                &mut hub_replica.writer.local(),
                space_keys.as_ref(),
                &mut outbox,
                NOW_MS,
            )
            .await
            .expect("publish the device key");
            assert!(matches!(published, DeviceKeyPublication::Sequenced(_)));
            authority::start_key_epochs(
                &hub_replica.engine,
                &mut conn,
                &hub_replica.ctx,
                &admin,
                &mut hub_replica.writer.local(),
                &hub_keys().recovery,
                NOW_MS,
            )
            .await
            .expect("start the key epochs")
            .expect("the founding kit starts them")
            .expect("a key epoch was sealed");
        }
        commit_sequenced(&mut hub_replica).await;
        assert_eq!(view_of(&hub_replica).await.key_epoch, 1);

        // The founding kit waits unsaved: it may not be replaced, and saving it forgets it.
        keys.write(
            VAULT_ID,
            SyncKey::RecoverySeed,
            hub_recovery_seed().as_bytes(),
        )
        .expect("store the founding seed");
        let view = view_of(&hub_replica).await;
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");
        assert!(regenerate(&mut outbox, &view).is_err());
        authority::forget_recovery_seed(&keys, VAULT_ID, &view).expect("save the founding kit");

        // Only the anchor's admin key installs a recovery key, so no other key replaces the kit.
        let other_admin = AdminKeyPair::from_seed(&[36; 32]).expect("admin seed");
        assert!(
            authority::regenerate_recovery_kit(
                &keys,
                VAULT_ID,
                &mut outbox,
                &view,
                space,
                &other_admin,
                NOW_MS
            )
            .is_err()
        );
        assert!(
            outbox.entries().is_empty(),
            "a refused replacement proposes nothing"
        );

        // An outbox with room for only one of the two proposals keeps neither, and no seed
        // replaces anything.
        for index in 0..MAX_OUTBOX_PROPOSALS - 1 {
            let device_id = format!("device-filler-{index}");
            outbox
                .add(enroll_mobile(space, charter, &device_id, 60))
                .expect("fill the outbox");
        }
        assert!(regenerate(&mut outbox, &view).is_err());
        assert_eq!(outbox.entries().len(), MAX_OUTBOX_PROPOSALS - 1);
        assert_eq!(pending_code(&keys), None);
        Outbox::remove(&outbox_path).expect("empty the outbox");
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");

        let seed = regenerate(&mut outbox, &view).expect("regenerate");
        assert_eq!(
            pending_code(&keys),
            Some(seed.kit_code()),
            "the key store keeps the kit the user was shown"
        );
        assert!(
            RecoverySeed::parse_kit_code(seed.kit_code().as_str())
                .expect("parse kit code")
                .ct_eq(&seed)
        );
        assert!(
            regenerate(&mut outbox, &view).is_err(),
            "a kit whose proposal still waits is not replaced"
        );
        assert!(
            authority::forget_recovery_seed(&keys, VAULT_ID, &view).is_err(),
            "a kit the log does not name yet is kept"
        );
        {
            let mut conn = acquire(&hub_replica.pool).await;
            authority::sequence_outbox(
                &hub_replica.engine,
                &mut conn,
                &hub_replica.ctx,
                &mut hub_replica.writer.local(),
                &mut outbox,
                &Default::default(),
                &Default::default(),
                NOW_MS,
            )
            .await
            .expect("sequence the rotation");
        }
        commit_sequenced(&mut hub_replica).await;
        let view = view_of(&hub_replica).await;
        assert_eq!(
            view.recovery_encryption_key,
            Some(seed.public_keys().expect("kit keys").encryption),
            "the new kit receives every later envelope"
        );
        assert_eq!(
            view.key_epoch, 3,
            "the recovery key entry rotates first, so no epoch after the regeneration seals to the old kit"
        );

        // The installed recovery key signs for the space, and the printed kit still does.
        let enroll = |admin: &AdminKeyPair, device_id: &str, fingerprint: u8| {
            authority::propose(
                space,
                charter,
                admin,
                authority::enroll_entry(Enrollment {
                    device_id: device_id.to_string(),
                    device_key: device_key(device_id).public_key(),
                    tls_fingerprint: [fingerprint; 32],
                    label: device_id.to_string(),
                    device_kind: DeviceKind::Mobile,
                }),
                NOW_MS + 1,
            )
            .expect("sign enrollment")
        };
        let old_kit = hub_recovery_seed()
            .recovery_admin_key()
            .expect("old recovery key");
        let new_kit = seed.recovery_admin_key().expect("new recovery key");
        sequence_here(&mut hub_replica, &enroll(&new_kit, "device-new-kit", 42)).await;
        let view = view_of(&hub_replica).await;
        assert_eq!(
            view.recovery_admin_key,
            new_kit.public_key(),
            "the installed key recovers"
        );
        assert_eq!(
            view.kit_admin_key,
            old_kit.public_key(),
            "a recovery key never replaces the printed kit"
        );
        sequence_here(&mut hub_replica, &enroll(&old_kit, "device-old-kit", 43)).await;
        let view = view_of(&hub_replica).await;
        authority::forget_recovery_seed(&keys, VAULT_ID, &view).expect("save the active kit");
        assert_eq!(pending_code(&keys), None);
        hub_replica.close().await;
    });
}

#[test]
fn the_key_store_forgets_a_saved_recovery_kit_until_the_next_charter() {
    block_on(async {
        let replica = Replica::new("recovery-kit-store", HUB_DEVICE).await;
        let keys = replica.directory.keys();
        keys.write(
            VAULT_ID,
            SyncKey::RecoverySeed,
            hub_recovery_seed().as_bytes(),
        )
        .expect("store the founding seed");
        let founding = authority::ensure_anchor_keys(&keys, VAULT_ID, None).expect("anchor keys");
        assert_eq!(
            pending_code(&keys),
            Some(hub_recovery_seed().kit_code()),
            "the founding kit waits until the user saves it"
        );
        assert_eq!(
            founding.recovery,
            hub_recovery_seed().public_keys().expect("kit keys")
        );
        authority::forget_recovery_seed(&keys, VAULT_ID, &view_of(&replica).await)
            .expect("forget seed");
        assert_eq!(pending_code(&keys), None);
        authority::ensure_admin_key(&keys, VAULT_ID).expect("admin key");
        assert_eq!(
            pending_code(&keys),
            None,
            "signing as an admin never recreates the seed"
        );
        let kept = authority::ensure_anchor_keys(&keys, VAULT_ID, Some(founding.recovery))
            .expect("anchor keys");
        assert_eq!(
            kept.recovery, founding.recovery,
            "the kit the vault database keeps founds the next charter as it is"
        );
        assert_eq!(pending_code(&keys), None, "a saved kit is never recreated");
        let next = authority::ensure_anchor_keys(&keys, VAULT_ID, None).expect("anchor keys");
        assert_ne!(
            next.recovery, founding.recovery,
            "without a kept kit, a later charter founds with a fresh kit"
        );
        assert_eq!(
            authority::pending_recovery_seed(&keys, VAULT_ID)
                .expect("pending seed")
                .map(|seed| seed.public_keys().expect("kit keys")),
            Some(next.recovery),
            "the fresh kit waits for the user to save it"
        );
        assert_eq!(
            next.admin.public_key(),
            founding.admin.public_key(),
            "the admin key survives"
        );
        replica.close().await;
    });
}

/// A vault without a sync space forgets its saved kit only when the vault database keeps the
/// kit's keys for the charter its first link founds.
#[test]
fn an_unlinked_vault_forgets_only_the_recovery_kit_it_keeps() {
    let directory = TestDirectory::new("recovery-kit-unlinked");
    let keys = directory.keys();
    let created = authority::create_recovery_seed(&keys, VAULT_ID).expect("create kit");
    let public = created.public_keys().expect("kit keys");
    let other = hub_recovery_seed().public_keys().expect("other kit keys");
    assert!(
        authority::forget_unlinked_recovery_seed(&keys, VAULT_ID, None).is_err(),
        "a kit the vault does not keep stays"
    );
    assert!(
        authority::forget_unlinked_recovery_seed(&keys, VAULT_ID, Some(other)).is_err(),
        "a replaced kit stays until the vault keeps it"
    );
    assert_eq!(pending_code(&keys), Some(created.kit_code()));
    authority::forget_unlinked_recovery_seed(&keys, VAULT_ID, Some(public)).expect("forget");
    assert_eq!(pending_code(&keys), None);
    let founding =
        authority::ensure_anchor_keys(&keys, VAULT_ID, Some(public)).expect("anchor keys");
    assert_eq!(founding.recovery, public, "the kept kit founds the charter");
    assert_eq!(
        pending_code(&keys),
        None,
        "founding with a kept kit creates no seed"
    );
}

#[test]
fn a_recovery_kit_promotes_a_member_desktop_that_then_takes_the_anchor() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-kit-use", HUB_DEVICE).await;
        let mut laptop = Replica::new("laptop-kit-use", LAPTOP_DEVICE).await;
        let laptop_admin = AdminKeyPair::from_seed(&[31; 32]).expect("admin seed");
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let enroll = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: LAPTOP_DEVICE.to_string(),
                device_key: device_key(LAPTOP_DEVICE).public_key(),
                tls_fingerprint: [33; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            NOW_MS,
        )
        .expect("sign enrollment");
        sequence_here(&mut hub_replica, &enroll).await;
        let transport = forwarding_hub(&hub_replica);
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("pull the log");
        laptop.apply().await;
        let view = {
            let mut conn = acquire(&laptop.pool).await;
            laptop
                .engine
                .authorization(&mut conn, &laptop.ctx)
                .await
                .expect("read authorization")
                .expect("charter")
        };
        assert!(!authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));

        // A kit the log does not name, or a mistyped code, signs nothing.
        let foreign = RecoverySeed::from_bytes([77; 32]).kit_code();
        assert!(
            authority::recovery_promotion(
                &view,
                space,
                &foreign,
                LAPTOP_DEVICE,
                laptop_admin.public_key(),
                NOW_MS,
            )
            .is_err()
        );
        let code = hub_recovery_seed().kit_code();
        assert!(
            authority::recovery_promotion(
                &view,
                space,
                &KitCode::new(format!("{}0", code.as_str())),
                LAPTOP_DEVICE,
                laptop_admin.public_key(),
                NOW_MS,
            )
            .is_err()
        );

        // The member desktop proposes its own promotion with the kit and the anchor sequences it.
        let promotion = authority::recovery_promotion(
            &view,
            space,
            &code,
            LAPTOP_DEVICE,
            laptop_admin.public_key(),
            NOW_MS,
        )
        .expect("sign with the kit");
        let outcome = client::propose(&transport, &laptop_ids(), &promotion)
            .await
            .expect("propose through the hub");
        assert_eq!(
            outcome,
            Err(ProposalRejection::NotAnchor),
            "the test hub has no anchor service, so the anchor sequences it directly"
        );
        sequence_here(&mut hub_replica, &promotion).await;
        laptop
            .exchange(&transport, &laptop_ids())
            .await
            .expect("pull the promotion");
        laptop.apply().await;
        let stored = laptop.stored().await;
        let mut conn = acquire(&laptop.pool).await;
        let view = laptop
            .engine
            .authorization(&mut conn, &laptop.ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        assert!(authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));
        authority::seal_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &laptop_admin,
            &mut laptop.writer.local(),
            Some(stored),
            NOW_MS,
        )
        .await
        .expect("seal handover")
        .expect("the desktop the kit promoted takes the anchor");
        drop(conn);
        hub_replica.close().await;
        laptop.close().await;
    });
}

#[test]
fn a_recovery_kit_forces_the_anchor_to_a_member_desktop_without_the_lost_anchor() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-kit-force", HUB_DEVICE).await;
        let mut laptop = Replica::new("laptop-kit-force", LAPTOP_DEVICE).await;
        let laptop_admin = AdminKeyPair::from_seed(&[32; 32]).expect("admin seed");
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;
        let enroll = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: LAPTOP_DEVICE.to_string(),
                device_key: device_key(LAPTOP_DEVICE).public_key(),
                tls_fingerprint: [33; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            NOW_MS,
        )
        .expect("sign enrollment");
        sequence_here(&mut hub_replica, &enroll).await;
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, "device-watch", 50),
        )
        .await;
        laptop
            .exchange(&forwarding_hub(&hub_replica), &laptop_ids())
            .await
            .expect("pull the log");
        laptop.apply().await;
        // The anchor is lost from here on.
        hub_replica.close().await;

        let mut conn = acquire(&laptop.pool).await;
        let view = laptop
            .engine
            .authorization(&mut conn, &laptop.ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        let recovery =
            authority::recovery_admin(&view, &hub_recovery_seed().kit_code()).expect("kit");
        // The device review marks the watch and, redundantly, the lost anchor and this desktop.
        let forced = authority::ForcedHandover {
            signer: &recovery,
            promote: Some(laptop_admin.public_key()),
            point: None,
            removals: vec![
                "device-watch".to_string(),
                HUB_DEVICE.to_string(),
                LAPTOP_DEVICE.to_string(),
            ],
            recovery: None,
        };
        let mut outbox = Outbox::load(&laptop.directory.files().outbox_path()).expect("outbox");
        let revision = authority::force_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &forced,
            &mut laptop.writer.local(),
            &mut outbox,
            &Default::default(),
            NOW_MS,
        )
        .await
        .expect("force handover")
        .expect("the recovery key hands the anchor to a member desktop")
        .revision;
        let view = laptop
            .engine
            .authorization(&mut conn, &laptop.ctx)
            .await
            .expect("read authorization")
            .expect("charter");
        assert_eq!(
            (view.revision, view.anchor_device_id.as_str()),
            (revision, LAPTOP_DEVICE)
        );
        assert!(authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));
        // A kit forced handover also removes the lost anchor, so a stolen one cannot return.
        assert!(
            view.devices
                .iter()
                .any(|device| device.device_id == HUB_DEVICE && device.removed_revision.is_some()),
            "the lost anchor is removed"
        );
        let removed: Vec<&str> = view
            .devices
            .iter()
            .filter(|device| device.removed_revision.is_some())
            .map(|device| device.device_id.as_str())
            .collect();
        assert_eq!(
            removed.len(),
            2,
            "the reviewed device is removed under the kit, each device once and never this desktop"
        );
        assert!(removed.contains(&"device-watch"));
        assert!(
            authority::recovery_admin(&view, &RecoverySeed::from_bytes([78; 32]).kit_code())
                .is_err()
        );
        drop(conn);
        laptop.close().await;
    });
}

/// A member laptop holding the log of a lost hub that enrolled it and `others`, each
/// `(device id, removed)`, removing the ones marked removed before it was lost. Returns the
/// laptop with the hub already closed.
async fn lost_hub_space(label: &str, others: &[(&str, bool)]) -> Replica {
    let mut hub_replica = Replica::new(&format!("hub-{label}"), HUB_DEVICE).await;
    let mut laptop = Replica::new(&format!("laptop-{label}"), LAPTOP_DEVICE).await;
    let charter = hub_replica.charter().await;
    let space = hub_replica.ctx.space;
    let enroll = authority::propose(
        space,
        charter,
        &hub_keys().admin,
        authority::enroll_entry(Enrollment {
            device_id: LAPTOP_DEVICE.to_string(),
            device_key: device_key(LAPTOP_DEVICE).public_key(),
            tls_fingerprint: [33; 32],
            label: "Laptop".to_string(),
            device_kind: DeviceKind::Desktop,
        }),
        NOW_MS,
    )
    .expect("sign enrollment");
    sequence_here(&mut hub_replica, &enroll).await;
    for (index, (device_id, removed)) in others.iter().enumerate() {
        let seed = 50 + u8::try_from(index).expect("few devices");
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, device_id, seed),
        )
        .await;
        if *removed {
            let removal = authority::propose(
                space,
                charter,
                &hub_keys().admin,
                authority::remove_entry(device_id),
                NOW_MS,
            )
            .expect("sign removal");
            sequence_here(&mut hub_replica, &removal).await;
        }
    }
    laptop
        .exchange(&forwarding_hub(&hub_replica), &laptop_ids())
        .await
        .expect("pull the log");
    laptop.apply().await;
    hub_replica.close().await;
    laptop
}

#[test]
fn a_crash_after_the_forced_seal_finishes_the_follow_ups() {
    block_on(async {
        let mut laptop = lost_hub_space("forced-crash", &[("device-watch", false)]).await;
        let laptop_admin = AdminKeyPair::from_seed(&[32; 32]).expect("admin seed");
        let view = view_of(&laptop).await;
        let recovery =
            authority::recovery_admin(&view, &hub_recovery_seed().kit_code()).expect("kit");
        let forced = authority::ForcedHandover {
            signer: &recovery,
            promote: Some(laptop_admin.public_key()),
            point: None,
            removals: vec!["device-watch".to_string()],
            recovery: None,
        };
        let outbox_path = laptop.directory.files().outbox_path();
        {
            let mut outbox = Outbox::load(&outbox_path).expect("outbox");
            let mut conn = acquire(&laptop.pool).await;
            authority::CRASH_AFTER_FORCED_SEAL.with(|crash| crash.set(true));
            let crashed = authority::force_handover_here(
                &laptop.engine,
                &mut conn,
                &laptop.ctx,
                &forced,
                &mut laptop.writer.local(),
                &mut outbox,
                &Default::default(),
                NOW_MS,
            )
            .await;
            authority::CRASH_AFTER_FORCED_SEAL.with(|crash| crash.set(false));
            assert!(crashed.is_err(), "the forced handover stops after its seal");
        }
        let sealed = view_of(&laptop).await;
        assert_eq!(
            sealed.anchor_device_id, LAPTOP_DEVICE,
            "the handover is durable"
        );

        // The restarted service loop sequences what the outbox on disk holds.
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");
        let mut conn = acquire(&laptop.pool).await;
        authority::sequence_outbox(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &mut laptop.writer.local(),
            &mut outbox,
            &VersionVector::new(),
            &Default::default(),
            NOW_MS,
        )
        .await
        .expect("sequence the outbox");
        drop(conn);
        let view = view_of(&laptop).await;
        for device_id in [HUB_DEVICE, "device-watch"] {
            assert!(
                view.devices.iter().any(
                    |device| device.device_id == device_id && device.removed_revision.is_some()
                ),
                "{device_id} is removed after the restart"
            );
        }
        assert!(authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));
        laptop.close().await;
    });
}

#[test]
fn a_rejected_review_entry_does_not_stop_the_rest() {
    block_on(async {
        let mut laptop = lost_hub_space(
            "forced-review",
            &[("device-gone", true), ("device-watch", false)],
        )
        .await;
        let laptop_admin = AdminKeyPair::from_seed(&[32; 32]).expect("admin seed");
        let view = view_of(&laptop).await;
        let recovery =
            authority::recovery_admin(&view, &hub_recovery_seed().kit_code()).expect("kit");
        // The review names a device the lost hub removed already, ahead of a live one.
        let forced = authority::ForcedHandover {
            signer: &recovery,
            promote: Some(laptop_admin.public_key()),
            point: None,
            removals: vec!["device-gone".to_string(), "device-watch".to_string()],
            recovery: None,
        };
        let mut outbox = Outbox::load(&laptop.directory.files().outbox_path()).expect("outbox");
        let mut conn = acquire(&laptop.pool).await;
        let forced = authority::force_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &forced,
            &mut laptop.writer.local(),
            &mut outbox,
            &Default::default(),
            NOW_MS,
        )
        .await
        .expect("force handover")
        .expect("the kit hands the anchor to the laptop");
        drop(conn);
        assert_eq!(forced.incomplete, 0, "a device gone already is no failure");
        let view = view_of(&laptop).await;
        assert_eq!(view.revision, forced.revision);
        for device_id in [HUB_DEVICE, "device-watch"] {
            assert!(
                view.devices.iter().any(
                    |device| device.device_id == device_id && device.removed_revision.is_some()
                ),
                "{device_id} is removed"
            );
        }
        assert!(authority::is_admin(
            &view,
            LAPTOP_DEVICE,
            &laptop_admin.public_key()
        ));
        laptop.close().await;
    });
}

#[test]
fn a_lost_forced_handover_withdraws_its_gated_entries() {
    block_on(async {
        let mut laptop = lost_hub_space("forced-lost", &[("device-watch", false)]).await;
        let before = view_of(&laptop).await;
        // A key the log does not name signs the handover, so the engine refuses it.
        let stranger = AdminKeyPair::from_seed(&[77; 32]).expect("admin seed");
        let forced = authority::ForcedHandover {
            signer: &stranger,
            promote: Some(stranger.public_key()),
            point: None,
            removals: Vec::new(),
            recovery: None,
        };
        let outbox_path = laptop.directory.files().outbox_path();
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");
        let mut conn = acquire(&laptop.pool).await;
        let refused = authority::force_handover_here(
            &laptop.engine,
            &mut conn,
            &laptop.ctx,
            &forced,
            &mut laptop.writer.local(),
            &mut outbox,
            &Default::default(),
            NOW_MS,
        )
        .await
        .expect("force handover");
        drop(conn);
        assert!(refused.is_err(), "the engine refuses the handover");
        assert_eq!(view_of(&laptop).await, before);
        assert!(outbox.entries().is_empty(), "nothing waits in memory");
        let reloaded = Outbox::load(&outbox_path).expect("outbox");
        assert!(reloaded.entries().is_empty(), "nothing waits on disk");
        laptop.close().await;
    });
}

/// A laptop desktop holding the log of a lost hub that started the key epochs with the founding
/// kit and then regenerated the recovery key, with the laptop's encryption keys attached. With
/// `publish_laptop_key`, the hub sequenced the laptop's device key before it was lost. Returns
/// the laptop and the regenerated seed.
async fn regenerated_space(label: &str, publish_laptop_key: bool) -> (Replica, RecoverySeed) {
    let mut hub_replica = Replica::new(&format!("hub-{label}"), HUB_DEVICE).await;
    let mut laptop = Replica::new(&format!("laptop-{label}"), LAPTOP_DEVICE).await;
    let charter = hub_replica.charter().await;
    let space = hub_replica.ctx.space;
    let admin = hub_keys().admin;
    let hub_space_keys = Arc::new(SpaceKeys::new(
        device_key(HUB_DEVICE),
        EncryptionKeyPair::generate().expect("encryption key"),
    ));
    hub_replica.writer.attach_keys(hub_space_keys.clone());
    let outbox_path = hub_replica.directory.files().outbox_path();
    {
        let mut conn = acquire(&hub_replica.pool).await;
        let mut outbox = Outbox::load(&outbox_path).expect("outbox");
        authority::publish_device_key(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &mut hub_replica.writer.local(),
            hub_space_keys.as_ref(),
            &mut outbox,
            NOW_MS,
        )
        .await
        .expect("publish the device key");
        authority::start_key_epochs(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &admin,
            &mut hub_replica.writer.local(),
            &hub_keys().recovery,
            NOW_MS,
        )
        .await
        .expect("start the key epochs")
        .expect("the founding kit starts them")
        .expect("a key epoch was sealed");
    }
    commit_sequenced(&mut hub_replica).await;

    let keys = hub_replica.directory.keys();
    let mut outbox = Outbox::load(&outbox_path).expect("outbox");
    let view = view_of(&hub_replica).await;
    let seed = authority::regenerate_recovery_kit(
        &keys,
        VAULT_ID,
        &mut outbox,
        &view,
        space,
        &admin,
        NOW_MS,
    )
    .expect("regenerate");
    {
        let mut conn = acquire(&hub_replica.pool).await;
        authority::sequence_outbox(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &mut hub_replica.writer.local(),
            &mut outbox,
            &Default::default(),
            &Default::default(),
            NOW_MS,
        )
        .await
        .expect("sequence the regeneration");
    }
    commit_sequenced(&mut hub_replica).await;
    let enroll = authority::propose(
        space,
        charter,
        &admin,
        authority::enroll_entry(Enrollment {
            device_id: LAPTOP_DEVICE.to_string(),
            device_key: device_key(LAPTOP_DEVICE).public_key(),
            tls_fingerprint: [33; 32],
            label: "Laptop".to_string(),
            device_kind: DeviceKind::Desktop,
        }),
        NOW_MS,
    )
    .expect("sign enrollment");
    sequence_here(&mut hub_replica, &enroll).await;
    let laptop_keys = Arc::new(SpaceKeys::new(
        device_key(LAPTOP_DEVICE),
        EncryptionKeyPair::generate().expect("encryption key"),
    ));
    if publish_laptop_key {
        let published = authority::device_key_proposal(
            space,
            charter,
            LAPTOP_DEVICE,
            laptop_keys.as_ref(),
            NOW_MS,
        )
        .expect("sign the laptop's device key");
        sequence_here(&mut hub_replica, &published).await;
    }
    let view = view_of(&hub_replica).await;
    assert_eq!(
        view.recovery_encryption_key,
        Some(seed.public_keys().expect("kit keys").encryption),
        "the regenerated kit receives every later envelope"
    );
    assert!(view.key_epoch > 1);
    laptop
        .exchange(&forwarding_hub(&hub_replica), &laptop_ids())
        .await
        .expect("pull the log");
    laptop.apply().await;
    laptop.writer.attach_keys(laptop_keys);
    // The anchor is lost from here on.
    hub_replica.close().await;
    (laptop, seed)
}

/// Forces the anchor to `laptop` with the recovery kit `code`, promoting it to admin.
async fn force_with_kit(
    laptop: &mut Replica,
    code: &KitCode,
) -> Result<Result<u64, ProposalRejection>, String> {
    let signer = authority::kit_admin(code).expect("kit admin key");
    let recovery = authority::kit_encryption(code).expect("kit encryption key");
    let laptop_admin = AdminKeyPair::from_seed(&[32; 32]).expect("admin seed");
    let forced = authority::ForcedHandover {
        signer: &signer,
        promote: Some(laptop_admin.public_key()),
        point: None,
        removals: Vec::new(),
        recovery: Some(&recovery),
    };
    let mut outbox = Outbox::load(&laptop.directory.files().outbox_path()).expect("outbox");
    let mut conn = acquire(&laptop.pool).await;
    authority::force_handover_here(
        &laptop.engine,
        &mut conn,
        &laptop.ctx,
        &forced,
        &mut laptop.writer.local(),
        &mut outbox,
        &Default::default(),
        NOW_MS,
    )
    .await
    .map(|forced| forced.map(|forced| forced.revision))
}

#[test]
fn a_regenerated_recovery_key_keeps_the_printed_kit_from_forcing_a_keyless_anchor() {
    block_on(async {
        let (mut laptop, seed) = regenerated_space("stale-kit", false).await;
        let before = view_of(&laptop).await;
        let stored = laptop.stored().await;

        // The printed kit still signs a handover, but it opens no epoch after the regeneration
        // and the log names no key of this desktop, so nothing is sealed.
        assert_eq!(
            force_with_kit(&mut laptop, &hub_recovery_seed().kit_code()).await,
            Err(authority::KeylessAnchor::StaleKit.to_string())
        );
        assert_eq!(view_of(&laptop).await, before);
        assert_eq!(laptop.stored().await, stored);
        let outbox = Outbox::load(&laptop.directory.files().outbox_path()).expect("outbox");
        assert!(outbox.entries().is_empty(), "nothing waits for an anchor");

        // The regenerated kit opens the current recovery envelope, so it takes the anchor.
        let revision = force_with_kit(&mut laptop, &seed.kit_code())
            .await
            .expect("force handover")
            .expect("the regenerated kit hands the anchor to the laptop");
        let view = view_of(&laptop).await;
        assert_eq!(
            (view.revision, view.anchor_device_id.as_str()),
            (revision, LAPTOP_DEVICE)
        );
        assert!(
            view.devices
                .iter()
                .any(|device| device.device_id == LAPTOP_DEVICE && device.encryption_key.is_some()),
            "the new anchor publishes its own device key"
        );
        laptop.close().await;
    });
}

#[test]
fn a_printed_kit_forces_the_anchor_to_a_desktop_whose_key_the_log_names() {
    block_on(async {
        let (mut laptop, _seed) = regenerated_space("published-kit", true).await;
        let revision = force_with_kit(&mut laptop, &hub_recovery_seed().kit_code())
            .await
            .expect("force handover")
            .expect("the printed kit hands the anchor to a desktop that opens the epoch");
        let view = view_of(&laptop).await;
        assert_eq!(
            (view.revision, view.anchor_device_id.as_str()),
            (revision, LAPTOP_DEVICE)
        );
        assert!(
            view.devices
                .iter()
                .any(|device| device.device_id == HUB_DEVICE && device.removed_revision.is_some()),
            "the kit removes the lost anchor"
        );
        laptop.close().await;
    });
}

/// A hub, holding its space keys, that started the key epochs with the founding kit, enrolled
/// the laptop desktop, and sequenced its device key. Returns the hub, the laptop, which has not
/// pulled the log yet, the hub's encryption public key, and the laptop's space keys.
async fn founded_space(label: &str) -> (Replica, Replica, EncryptionPublicKey, Arc<SpaceKeys>) {
    let mut hub_replica = Replica::new(&format!("hub-{label}"), HUB_DEVICE).await;
    let laptop = Replica::new(&format!("laptop-{label}"), LAPTOP_DEVICE).await;
    let charter = hub_replica.charter().await;
    let space = hub_replica.ctx.space;
    let admin = hub_keys().admin;
    let hub_space_keys = Arc::new(SpaceKeys::new(
        device_key(HUB_DEVICE),
        EncryptionKeyPair::generate().expect("encryption key"),
    ));
    let hub_encryption = hub_space_keys.encryption_key();
    hub_replica.writer.attach_keys(hub_space_keys.clone());
    {
        let mut conn = acquire(&hub_replica.pool).await;
        let mut outbox =
            Outbox::load(&hub_replica.directory.files().outbox_path()).expect("outbox");
        authority::publish_device_key(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &mut hub_replica.writer.local(),
            hub_space_keys.as_ref(),
            &mut outbox,
            NOW_MS,
        )
        .await
        .expect("publish the device key");
        authority::start_key_epochs(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &admin,
            &mut hub_replica.writer.local(),
            &hub_keys().recovery,
            NOW_MS,
        )
        .await
        .expect("start the key epochs")
        .expect("the founding kit starts them")
        .expect("a key epoch was sealed");
    }
    commit_sequenced(&mut hub_replica).await;
    let enroll = authority::propose(
        space,
        charter,
        &admin,
        authority::enroll_entry(Enrollment {
            device_id: LAPTOP_DEVICE.to_string(),
            device_key: device_key(LAPTOP_DEVICE).public_key(),
            tls_fingerprint: [33; 32],
            label: "Laptop".to_string(),
            device_kind: DeviceKind::Desktop,
        }),
        NOW_MS,
    )
    .expect("sign enrollment");
    sequence_here(&mut hub_replica, &enroll).await;
    let laptop_keys = Arc::new(SpaceKeys::new(
        device_key(LAPTOP_DEVICE),
        EncryptionKeyPair::generate().expect("encryption key"),
    ));
    let published =
        authority::device_key_proposal(space, charter, LAPTOP_DEVICE, laptop_keys.as_ref(), NOW_MS)
            .expect("sign the laptop's device key");
    sequence_here(&mut hub_replica, &published).await;
    (hub_replica, laptop, hub_encryption, laptop_keys)
}

/// A laptop desktop holding the log of a lost hub built by [`founded_space`]. With `planted`,
/// the hub, stolen, then named that recovery encryption key and sealed a key epoch to it.
/// Returns the laptop and the hub's encryption public key.
async fn kit_space(
    label: &str,
    planted: Option<EncryptionPublicKey>,
) -> (Replica, EncryptionPublicKey) {
    let (mut hub_replica, mut laptop, hub_encryption, laptop_keys) = founded_space(label).await;
    let charter = hub_replica.charter().await;
    let space = hub_replica.ctx.space;
    let admin = hub_keys().admin;
    if let Some(thief) = planted {
        let plant = authority::propose(
            space,
            charter,
            &admin,
            authority::recovery_key_entry(thief),
            NOW_MS,
        )
        .expect("sign the planted recovery key");
        sequence_here(&mut hub_replica, &plant).await;
        // The next entry seals the rotation the planted key waits for.
        sequence_here(
            &mut hub_replica,
            &enroll_mobile(space, charter, "device-watch", 50),
        )
        .await;
        let view = view_of(&hub_replica).await;
        assert_eq!(view.recovery_encryption_key, Some(thief));
        let sealed = key_epoch_of(&hub_replica, view.key_epoch).await;
        assert!(
            sealed.envelope_for(&thief).is_some(),
            "the stolen anchor seals a key epoch to its own recovery key"
        );
    }
    laptop
        .exchange(&forwarding_hub(&hub_replica), &laptop_ids())
        .await
        .expect("pull the log");
    laptop.apply().await;
    laptop.writer.attach_keys(laptop_keys);
    // The anchor is lost from here on.
    hub_replica.close().await;
    (laptop, hub_encryption)
}

/// The key epoch entry the selected chain of `replica` holds for `epoch`.
async fn key_epoch_of(replica: &Replica, epoch: u32) -> KeyEpoch {
    let mut conn = acquire(&replica.pool).await;
    let bytes = replica
        .engine
        .key_epoch_entry(&mut conn, &replica.ctx, epoch)
        .await
        .expect("read the key epoch record")
        .expect("the key epoch record");
    let operation = Envelope::decode(&bytes)
        .expect("decode")
        .operation()
        .expect("operation");
    let Content::Authorization(record) = operation.content else {
        panic!("an authorization record");
    };
    let Entry::KeyEpoch(entry) = record.proposal.proposal.entry else {
        panic!("a key epoch entry");
    };
    entry
}

/// How many recovery key entries the selected chain of `replica` holds.
async fn recovery_key_entries(replica: &Replica) -> usize {
    let envelopes: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT e.envelope FROM sync_auth_selected s
         JOIN sync_auth_envelopes e
           ON e.space_id = s.space_id AND e.envelope_hash = s.envelope_hash AND e.role = 'record'
         WHERE s.space_id = ?",
    )
    .bind(replica.ctx.space.as_bytes().as_slice())
    .fetch_all(&replica.pool)
    .await
    .expect("read the selected records");
    envelopes
        .iter()
        .filter(|bytes| {
            let operation = Envelope::decode(bytes)
                .expect("decode")
                .operation()
                .expect("operation");
            matches!(
                operation.content,
                Content::Authorization(record)
                    if matches!(record.proposal.proposal.entry, Entry::RecoveryKey(_))
            )
        })
        .count()
}

#[test]
fn a_kit_takeover_replaces_a_planted_recovery_key() {
    block_on(async {
        let thief = EncryptionKeyPair::generate()
            .expect("thief key")
            .public_key();
        let (mut laptop, hub_encryption) = kit_space("planted-kit", Some(thief)).await;
        let kit = hub_recovery_seed()
            .public_keys()
            .expect("kit keys")
            .encryption;

        force_with_kit(&mut laptop, &hub_recovery_seed().kit_code())
            .await
            .expect("force handover")
            .expect("the printed kit hands the anchor to the laptop");
        let view = view_of(&laptop).await;
        assert_eq!(view.anchor_device_id, LAPTOP_DEVICE);
        assert_eq!(
            view.recovery_encryption_key,
            Some(kit),
            "the kit names its own recovery key again"
        );
        let removal = view
            .devices
            .iter()
            .find(|device| device.device_id == HUB_DEVICE)
            .and_then(|device| device.removed_revision)
            .expect("the kit removes the stolen anchor");
        let latest = key_epoch_of(&laptop, view.key_epoch).await;
        let mut conn = acquire(&laptop.pool).await;
        let sealed_at = laptop
            .engine
            .space_keys(&mut conn, &laptop.ctx)
            .await
            .expect("read the space keys")
            .iter()
            .find(|key| key.epoch == view.key_epoch)
            .map(|key| key.sequenced_revision)
            .expect("the latest key epoch");
        drop(conn);
        assert!(
            sealed_at > removal,
            "the latest key epoch follows the removal"
        );
        assert!(
            latest.envelope_for(&thief).is_none(),
            "no later epoch key seals to the planted recovery key"
        );
        assert!(
            latest.envelope_for(&hub_encryption).is_none(),
            "the removed anchor opens no later epoch key"
        );
        assert!(latest.envelope_for(&kit).is_some());
        laptop.close().await;
    });
}

#[test]
fn a_kit_takeover_keeps_a_recovery_key_it_already_names() {
    block_on(async {
        let (mut laptop, _hub_encryption) = kit_space("named-kit", None).await;
        let before = recovery_key_entries(&laptop).await;
        force_with_kit(&mut laptop, &hub_recovery_seed().kit_code())
            .await
            .expect("force handover")
            .expect("the printed kit hands the anchor to the laptop");
        assert_eq!(
            recovery_key_entries(&laptop).await,
            before,
            "a kit the log already names seals no recovery key entry"
        );
        assert_eq!(
            view_of(&laptop).await.recovery_encryption_key,
            Some(
                hub_recovery_seed()
                    .public_keys()
                    .expect("kit keys")
                    .encryption
            )
        );
        laptop.close().await;
    });
}

/// Sequences `proposal` through the writer of `replica`, which must anchor, and records the
/// commit when the anchor accepts it.
async fn try_sequence(
    replica: &mut Replica,
    proposal: &SignedProposal,
) -> Result<u64, ProposalRejection> {
    let mut conn = acquire(&replica.pool).await;
    let sequenced = authority::sequence(
        &replica.engine,
        &mut conn,
        &replica.ctx,
        &mut replica.writer.local(),
        proposal,
        NOW_MS,
    )
    .await
    .expect("sequence");
    drop(conn);
    if sequenced.is_ok() {
        commit_sequenced(replica).await;
    }
    sequenced
}

/// Rotates the recovery kit with the kit `code` through the key store and outbox of `replica`,
/// as its own view sees the log. Returns the new seed and the proposals the outbox then holds.
fn rotate_with(
    replica: &Replica,
    view: &ganbaru_sync::AuthorizationView,
    code: &KitCode,
) -> Result<(RecoverySeed, Vec<SignedProposal>), String> {
    let outbox_path = replica.directory.files().outbox_path();
    let mut outbox = Outbox::load(&outbox_path).expect("outbox");
    let old_kit = authority::kit_admin(code).expect("kit admin key");
    let seed = authority::rotate_recovery_kit(
        &replica.directory.keys(),
        VAULT_ID,
        &mut outbox,
        view,
        replica.ctx.space,
        &old_kit,
        NOW_MS,
    )?;
    Ok((seed, outbox.pending()))
}

/// A hub and a laptop desktop that pulled its log, as [`founded_space`] builds them.
async fn linked_space(label: &str) -> (Replica, Replica) {
    let (hub_replica, mut laptop, _hub_encryption, laptop_keys) = founded_space(label).await;
    laptop
        .exchange(&forwarding_hub(&hub_replica), &laptop_ids())
        .await
        .expect("pull the log");
    laptop.apply().await;
    laptop.writer.attach_keys(laptop_keys);
    (hub_replica, laptop)
}

#[test]
fn rotate_recovery_kit_retires_the_old_code() {
    block_on(async {
        let (mut hub_replica, mut laptop) = linked_space("rotate-kit").await;
        let old_code = hub_recovery_seed().kit_code();
        let charter = hub_replica.charter().await;
        let space = hub_replica.ctx.space;

        // A desktop that is no admin rotates the kit with the printed code.
        let rotated = rotate_with(&laptop, &view_of(&laptop).await, &old_code);
        let Ok((seed, proposals)) = rotated else {
            panic!("the current kit rotates to a new kit: {:?}", rotated.err());
        };
        assert_eq!(
            pending_code(&laptop.directory.keys()),
            Some(seed.kit_code()),
            "the key store keeps the new kit until the user saves it"
        );
        for proposal in &proposals {
            sequence_here(&mut hub_replica, proposal).await;
        }
        let fresh = seed.public_keys().expect("new kit keys");
        let view = view_of(&hub_replica).await;
        assert_eq!(
            view.kit_admin_key, fresh.admin,
            "the new kit holds the lineage"
        );
        assert_eq!(view.recovery_admin_key, fresh.admin);
        assert_eq!(view.recovery_encryption_key, Some(fresh.encryption));
        let latest = key_epoch_of(&hub_replica, view.key_epoch).await;
        let old_encryption = hub_recovery_seed()
            .public_keys()
            .expect("old kit keys")
            .encryption;
        assert!(latest.envelope_for(&fresh.encryption).is_some());
        assert!(
            latest.envelope_for(&old_encryption).is_none(),
            "the old kit opens no later key epoch"
        );

        // The old code enrolls nothing at the anchor.
        let enrollment = authority::recovery_enrollment(
            space,
            charter,
            &old_code,
            Enrollment {
                device_id: "device-intruder".to_string(),
                device_key: device_key("device-intruder").public_key(),
                tls_fingerprint: [70; 32],
                label: "Intruder".to_string(),
                device_kind: DeviceKind::Desktop,
            },
            AdminKeyPair::from_seed(&[71; 32])
                .expect("admin seed")
                .public_key(),
            NOW_MS,
        )
        .expect("sign the enrollment");
        assert!(
            try_sequence(&mut hub_replica, &enrollment).await.is_err(),
            "the anchor refuses an enrollment the old kit signs"
        );

        // Once the anchor is lost, the old code forces no handover and the new one does.
        laptop
            .exchange(&forwarding_hub(&hub_replica), &laptop_ids())
            .await
            .expect("pull the log");
        laptop.apply().await;
        hub_replica.close().await;
        let before = view_of(&laptop).await;
        assert!(!matches!(
            force_with_kit(&mut laptop, &old_code).await,
            Ok(Ok(_))
        ));
        assert_eq!(view_of(&laptop).await, before);
        force_with_kit(&mut laptop, &seed.kit_code())
            .await
            .expect("force handover")
            .expect("the new kit hands the anchor to the laptop");
        assert_eq!(view_of(&laptop).await.anchor_device_id, LAPTOP_DEVICE);
        laptop.close().await;
    });
}

#[test]
fn rotate_recovery_kit_refuses_a_code_that_is_not_the_lineage_key() {
    block_on(async {
        let (mut hub_replica, laptop) = linked_space("rotate-installed").await;
        let space = hub_replica.ctx.space;

        // The anchor installs a recovery key, which never takes the kit lineage.
        let installed = {
            let keys = hub_replica.directory.keys();
            let mut outbox =
                Outbox::load(&hub_replica.directory.files().outbox_path()).expect("outbox");
            let seed = authority::regenerate_recovery_kit(
                &keys,
                VAULT_ID,
                &mut outbox,
                &view_of(&hub_replica).await,
                space,
                &hub_keys().admin,
                NOW_MS,
            )
            .expect("regenerate");
            for proposal in outbox.pending() {
                sequence_here(&mut hub_replica, &proposal).await;
            }
            seed
        };
        let view = view_of(&hub_replica).await;
        assert_eq!(
            view.recovery_admin_key,
            installed.public_keys().expect("installed keys").admin
        );
        assert_eq!(
            view.kit_admin_key,
            hub_recovery_seed().public_keys().expect("kit keys").admin
        );

        assert!(rotate_with(&laptop, &view, &installed.kit_code()).is_err());
        let outbox = Outbox::load(&laptop.directory.files().outbox_path()).expect("outbox");
        assert!(
            outbox.entries().is_empty(),
            "a refused rotation proposes nothing"
        );
        assert_eq!(pending_code(&laptop.directory.keys()), None);
        hub_replica.close().await;
        laptop.close().await;
    });
}

#[test]
fn log_names_kit_follows_a_rotation() {
    block_on(async {
        let (mut hub_replica, laptop) = linked_space("names-kit-rotation").await;
        let old_code = hub_recovery_seed().kit_code();
        let old_kit = authority::kit_public_keys(&old_code).expect("old kit keys");
        assert!(authority::log_names_kit(
            &view_of(&hub_replica).await,
            &old_kit
        ));

        let (seed, proposals) =
            rotate_with(&laptop, &view_of(&laptop).await, &old_code).expect("rotate");
        for proposal in &proposals {
            sequence_here(&mut hub_replica, proposal).await;
        }
        let view = view_of(&hub_replica).await;
        let new_kit = authority::kit_public_keys(&seed.kit_code()).expect("new kit keys");
        assert!(
            authority::log_names_kit(&view, &new_kit),
            "the rotated log names the new kit"
        );
        assert!(
            !authority::log_names_kit(&view, &old_kit),
            "a rotation retires the old kit"
        );
        let stranger = RecoverySeed::from_bytes([72; 32])
            .public_keys()
            .expect("stranger keys");
        assert!(!authority::log_names_kit(&view, &stranger));
        hub_replica.close().await;
        laptop.close().await;
    });
}

#[test]
fn rotate_recovery_kit_racing_rotations_never_mix_two_kits() {
    block_on(async {
        let (mut hub_replica, laptop) = linked_space("rotate-race").await;
        let old_code = hub_recovery_seed().kit_code();
        let view = view_of(&hub_replica).await;

        // The owner and a thief holding a copy of the printed kit rotate at the same time, and
        // the anchor receives their proposals interleaved.
        let (owner, owner_proposals) = rotate_with(&laptop, &view, &old_code).expect("rotate");
        let (thief, thief_proposals) = rotate_with(&hub_replica, &view, &old_code).expect("rotate");
        let mut interleaved = Vec::new();
        for (ours, theirs) in owner_proposals.iter().zip(&thief_proposals) {
            interleaved.extend([ours, theirs]);
        }
        let mut refused = 0;
        for proposal in interleaved {
            if try_sequence(&mut hub_replica, proposal).await.is_err() {
                refused += 1;
            }
        }
        assert_eq!(
            refused,
            thief_proposals.len(),
            "the losing rotation lands no entry"
        );

        let view = view_of(&hub_replica).await;
        let owner = owner.public_keys().expect("owner keys");
        let thief = thief.public_keys().expect("thief keys");
        let winner = [owner, thief]
            .into_iter()
            .find(|keys| keys.admin == view.kit_admin_key)
            .expect("one rotation wins the lineage");
        assert_eq!(view.recovery_admin_key, winner.admin);
        assert_eq!(
            view.recovery_encryption_key,
            Some(winner.encryption),
            "the winning kit's encryption key is the one the log names"
        );
        hub_replica.close().await;
        laptop.close().await;
    });
}
