//! The installation's space keys: the device encryption key in each key store, publication of
//! its device key entry, the epoch keys opened from the log, and quarantined devices.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::{
    DEVICE_ID, NOW_MS, TestDirectory, VAULT_ID, acquire, anchor_keys, block_on, context,
    create_vault, device, test_wrapper,
};
use crate::authority::{
    self, DeviceKeyPublication, Enrollment, device_key_proposal, propose, recovery_key_entry,
};
use crate::keys::SpaceKeys;
use crate::outbox::Outbox;
use crate::writer::{
    FileKeyStore, MemoryKeyStore, SuccessorPlan, SyncKey, SyncKeyStore, create_writer,
};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::key_wrap::is_envelope;
use ganbaru_sync::Engine;
use ganbaru_sync_contracts::{
    DeviceKeyPair, DeviceKind, EncryptionKeyPair, ProposalRejection, VersionVector,
};

const PHONE_ID: &str = "device-phone";
const PHONE_SEED: u8 = 30;

fn phone() -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&[PHONE_SEED; 32]).expect("phone seed")
}

fn phone_enrollment() -> Enrollment {
    Enrollment {
        device_id: PHONE_ID.to_string(),
        device_key: phone().public_key(),
        tls_fingerprint: [PHONE_SEED; 32],
        label: PHONE_ID.to_string(),
        device_kind: DeviceKind::Mobile,
    }
}

#[test]
fn the_device_encryption_key_is_created_once_and_kept_sealed_in_the_file_store() {
    let directory = TestDirectory::new("space-keys-file");
    let keys = FileKeyStore::new(directory.path().join("keys"), test_wrapper());
    let created = SpaceKeys::load(&keys, VAULT_ID).expect("create keys");
    let path = directory
        .path()
        .join("keys")
        .join(format!("device-encryption-key-{VAULT_ID}.key"));
    let stored = std::fs::read(&path).expect("stored encryption key");
    assert!(is_envelope(&stored), "the encryption key is stored wrapped");

    let secret = keys
        .read(VAULT_ID, SyncKey::DeviceEncryption)
        .expect("read")
        .expect("stored");
    assert!(
        !stored.windows(secret.len()).any(|window| window == secret),
        "the stored file never holds the plaintext key"
    );
    let reloaded = SpaceKeys::load(&keys, VAULT_ID).expect("reload keys");
    assert_eq!(reloaded.public_key(), created.public_key());
    assert!(!format!("{reloaded:?}").contains(&format!("{:?}", secret.as_slice())));

    let memory = MemoryKeyStore::default();
    let first = SpaceKeys::load(&memory, VAULT_ID).expect("memory keys");
    assert_eq!(
        SpaceKeys::load(&memory, VAULT_ID)
            .expect("memory keys again")
            .public_key(),
        first.public_key()
    );
    assert_ne!(first.public_key(), created.public_key());

    memory
        .write(VAULT_ID, SyncKey::DeviceEncryption, &[1; 16])
        .expect("write malformed");
    assert!(SpaceKeys::load(&memory, VAULT_ID).is_err());
}

#[test]
fn the_anchor_publishes_its_key_and_opens_each_epoch_it_seals() {
    block_on(async {
        let directory = TestDirectory::new("space-keys-anchor");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let store = MemoryKeyStore::default();
        let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
        let mut conn = acquire(&pool).await;
        let ctx = context(&mut conn).await;
        let mut writer = create_writer(
            &files,
            &store,
            SuccessorPlan::default(),
            &device(),
            DEVICE_ID,
            NOW_MS,
        )
        .expect("create writer");
        let keys = Arc::new(SpaceKeys::new(
            device(),
            EncryptionKeyPair::generate().expect("encryption key"),
        ));
        writer.attach_keys(keys.clone());
        let mut outbox = Outbox::load(&files.outbox_path()).expect("load outbox");

        let published = authority::publish_device_key(
            &engine,
            &mut conn,
            &ctx,
            &mut writer.local(),
            &keys,
            &mut outbox,
            NOW_MS,
        )
        .await
        .expect("publish");
        assert!(matches!(published, DeviceKeyPublication::Sequenced(_)));
        assert_eq!(
            authority::publish_device_key(
                &engine,
                &mut conn,
                &ctx,
                &mut writer.local(),
                &keys,
                &mut outbox,
                NOW_MS,
            )
            .await
            .expect("publish again"),
            DeviceKeyPublication::Current
        );
        assert_eq!(
            keys.open_epochs(&engine, &mut conn, &ctx)
                .await
                .expect("open"),
            None,
            "no epoch before the first recovery key"
        );

        let charter = authority::charter_hash(&engine, &mut conn, &ctx)
            .await
            .expect("charter");
        let recovery = EncryptionKeyPair::generate().expect("recovery key");
        let entry = recovery_key_entry(recovery.public_key());
        let signed = propose(ctx.space, charter, &anchor_keys().admin, entry, NOW_MS + 1)
            .expect("sign recovery key");
        authority::sequence(
            &engine,
            &mut conn,
            &ctx,
            &mut writer.local(),
            &signed,
            NOW_MS + 1,
        )
        .await
        .expect("sequence")
        .expect("recovery key accepted");
        assert_eq!(
            keys.open_epochs(&engine, &mut conn, &ctx)
                .await
                .expect("open"),
            Some(1)
        );
        let first = keys.epoch_key(1).expect("epoch 1 key");

        authority::enroll(
            &engine,
            &mut conn,
            &ctx,
            &anchor_keys().admin,
            &mut writer.local(),
            phone_enrollment(),
            NOW_MS + 2,
        )
        .await
        .expect("enroll phone");
        authority::remove(
            &engine,
            &mut conn,
            &ctx,
            &anchor_keys().admin,
            &mut writer.local(),
            PHONE_ID,
            NOW_MS + 3,
        )
        .await
        .expect("remove phone");
        assert_eq!(
            keys.open_epochs(&engine, &mut conn, &ctx)
                .await
                .expect("open"),
            Some(2)
        );
        assert_eq!(keys.newest_epoch(), Some(2));
        assert_eq!(keys.epoch_key(1).expect("chained").bytes(), first.bytes());
        assert_ne!(keys.epoch_key(2).expect("epoch 2").bytes(), first.bytes());
        drop(conn);
        registry.close_all().await.expect("close pools");
    });
}

#[test]
fn the_anchor_refuses_the_device_key_of_a_quarantined_device() {
    block_on(async {
        let directory = TestDirectory::new("space-keys-quarantine");
        let registry = DatabasePoolRegistry::default();
        let engine = Engine::vault();
        let files = directory.files();
        let store = MemoryKeyStore::default();
        let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
        let mut conn = acquire(&pool).await;
        let ctx = context(&mut conn).await;
        let mut writer = create_writer(
            &files,
            &store,
            SuccessorPlan::default(),
            &device(),
            DEVICE_ID,
            NOW_MS,
        )
        .expect("create writer");
        authority::enroll(
            &engine,
            &mut conn,
            &ctx,
            &anchor_keys().admin,
            &mut writer.local(),
            phone_enrollment(),
            NOW_MS,
        )
        .await
        .expect("enroll phone");
        let charter = authority::charter_hash(&engine, &mut conn, &ctx)
            .await
            .expect("charter");
        let phone_keys = SpaceKeys::new(phone(), EncryptionKeyPair::generate().expect("key"));
        let proposal = device_key_proposal(ctx.space, charter, PHONE_ID, &phone_keys, NOW_MS)
            .expect("sign device key");

        let quarantined = BTreeSet::from([PHONE_ID.to_string()]);
        let refused = authority::sequence_covering(
            &engine,
            &mut conn,
            &ctx,
            &mut writer.local(),
            &proposal,
            &VersionVector::new(),
            &quarantined,
            NOW_MS + 1,
        )
        .await
        .expect("sequence");
        assert_eq!(refused, Err(ProposalRejection::DeviceQuarantined));

        authority::sequence_covering(
            &engine,
            &mut conn,
            &ctx,
            &mut writer.local(),
            &proposal,
            &VersionVector::new(),
            &BTreeSet::new(),
            NOW_MS + 1,
        )
        .await
        .expect("sequence")
        .expect("an unquarantined device publishes its key");
        drop(conn);
        registry.close_all().await.expect("close pools");
    });
}
