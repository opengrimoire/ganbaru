//! Space keys on the authorization log: device key entries, the first key epoch a recovery key
//! starts, removals that take effect only with the key epoch the anchor seals after them, the
//! chain of wrapped previous keys, and anchors that cannot open the current epoch.

use std::sync::Arc;

use ganbaru_sync_contracts::{
    AdminPublicKey, DeviceKey, DeviceKeyPair, EncryptionKeyPair, EncryptionPublicKey, Entry,
    KeyEnvelope, KeyEpoch, Proposal, ProposalRejection, RecoveryKey, SignedProposal, SpaceId,
    SpaceKey, sign_device_proposal,
};

use super::authorization::{remove_entry, view};
use super::block_on;
use super::replica::{
    ANCHOR_DEVICE_ID, Replica, anchor_device_key, connection_from, device_id, device_key, person,
    person_proposal, recovery_admin, test_charter,
};
use crate::error::failpoints;
use crate::local::prepare_snapshot_copy;
use crate::{SpaceKeyHolder, StoredSpaceKey, SyncError};

/// The keys of one test installation: its device signing key, a device encryption key, and
/// optionally the recovery key a kit derives.
struct TestKeys {
    device: DeviceKeyPair,
    encryption: EncryptionKeyPair,
    recovery: Option<EncryptionKeyPair>,
}

impl TestKeys {
    fn new(device: DeviceKeyPair) -> Arc<Self> {
        Arc::new(Self {
            device,
            encryption: EncryptionKeyPair::generate().unwrap(),
            recovery: None,
        })
    }

    fn with_recovery(device: DeviceKeyPair, recovery: EncryptionKeyPair) -> Arc<Self> {
        Arc::new(Self {
            device,
            encryption: EncryptionKeyPair::generate().unwrap(),
            recovery: Some(recovery),
        })
    }
}

impl SpaceKeyHolder for TestKeys {
    fn device_key(&self) -> &DeviceKeyPair {
        &self.device
    }

    fn encryption_key(&self) -> EncryptionPublicKey {
        self.encryption.public_key()
    }

    fn open(&self, space: &SpaceId, epoch: u32, envelope: &KeyEnvelope) -> Option<SpaceKey> {
        std::iter::once(&self.encryption)
            .chain(self.recovery.as_ref())
            .find_map(|pair| envelope.open(space, epoch, pair).ok())
    }
}

/// A device key entry for `device_id` publishing `encryption_key`, signed by `signer`.
fn device_key_proposal(
    replica: &Replica,
    signer: &DeviceKeyPair,
    device_id: &str,
    encryption_key: EncryptionPublicKey,
) -> SignedProposal {
    let proposal = Proposal {
        space: replica.ctx.space,
        charter: test_charter(&person(), replica.ctx.space).hash(),
        author: AdminPublicKey::from_bytes(*signer.public_key().as_bytes()),
        created_at_ms: i64::try_from(replica.now_ms).unwrap(),
        entry: Entry::DeviceKey(DeviceKey {
            device_id: device_id.to_owned(),
            encryption_key,
        }),
    };
    sign_device_proposal(&proposal, signer).unwrap()
}

/// An anchor holding `keys` that published its own encryption key.
async fn keyed_anchor(salt: u64, keys: Arc<TestKeys>) -> Replica {
    let mut anchor = Replica::anchor(0, salt).await;
    anchor.identity.keys = Some(keys.clone());
    let own = device_key_proposal(
        &anchor,
        &anchor_device_key(&person()),
        ANCHOR_DEVICE_ID,
        keys.encryption.public_key(),
    );
    anchor.sequence_signed(&own).await.unwrap().unwrap();
    anchor
}

/// Sequences a device key entry for member `index` holding `keys` on `anchor`.
async fn publish_member(anchor: &mut Replica, index: usize, keys: &TestKeys) -> u64 {
    let signed = device_key_proposal(
        anchor,
        &device_key(&person(), index),
        &device_id(index),
        keys.encryption.public_key(),
    );
    anchor.sequence_signed(&signed).await.unwrap().unwrap()
}

/// Sequences the recovery key entry that starts key epoch 1.
async fn start_epochs(anchor: &mut Replica, recovery: &EncryptionKeyPair) -> u64 {
    let entry = Entry::RecoveryKey(RecoveryKey {
        encryption_key: recovery.public_key(),
    });
    anchor.sequence(entry).await.unwrap().unwrap()
}

async fn stored_keys(replica: &mut Replica) -> Vec<StoredSpaceKey> {
    replica
        .engine
        .space_keys(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
}

/// Opens the own envelope `replica` stores for `epoch` with `keys`.
async fn own_key(replica: &mut Replica, keys: &TestKeys, epoch: u32) -> Option<SpaceKey> {
    let space = replica.ctx.space;
    stored_keys(replica)
        .await
        .into_iter()
        .find(|stored| stored.epoch == epoch)?
        .own_envelope
        .and_then(|envelope| keys.open(&space, epoch, &envelope))
}

/// A member that stored and applied everything `anchor` sealed.
async fn synced_member(index: usize, salt: u64, anchor: &mut Replica) -> Replica {
    let mut member = Replica::new(index, salt).await;
    member.pull(anchor).await;
    member.apply_all().await.unwrap();
    member
}

#[test]
fn device_key_entries_are_signed_by_the_live_device_they_name() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 1).await;
        let keys = TestKeys::new(device_key(&person(), 1));

        let wrong = device_key_proposal(
            &anchor,
            &device_key(&person(), 2),
            &device_id(1),
            keys.encryption.public_key(),
        );
        assert_eq!(
            anchor.sequence_signed(&wrong).await.unwrap(),
            Err(ProposalRejection::NotAdmin)
        );

        let revision = publish_member(&mut anchor, 1, &keys).await;
        assert_eq!(view(&mut anchor).await.revision, revision);
        let published = view(&mut anchor)
            .await
            .devices
            .into_iter()
            .find(|device| device.device_id == device_id(1))
            .unwrap();
        assert_eq!(published.encryption_key, Some(keys.encryption.public_key()));

        anchor.remove(&device_id(2)).await.unwrap();
        let removed = device_key_proposal(
            &anchor,
            &device_key(&person(), 2),
            &device_id(2),
            keys.encryption.public_key(),
        );
        assert_eq!(
            anchor.sequence_signed(&removed).await.unwrap(),
            Err(ProposalRejection::DeviceRemoved)
        );
    });
}

#[test]
fn a_key_epoch_proposal_is_refused_because_only_the_anchor_seals_one() {
    block_on(async {
        let keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(2, keys.clone()).await;
        let space = anchor.ctx.space;
        let key = SpaceKey::generate(1).unwrap();
        let proposal = Proposal {
            space,
            charter: test_charter(&person(), space).hash(),
            author: AdminPublicKey::from_bytes(
                *anchor_device_key(&person()).public_key().as_bytes(),
            ),
            created_at_ms: i64::try_from(anchor.now_ms).unwrap(),
            entry: Entry::KeyEpoch(KeyEpoch {
                key_epoch: 1,
                wrapped_previous: None,
                envelopes: vec![
                    KeyEnvelope::seal(&space, &key, &keys.encryption.public_key()).unwrap(),
                ],
                relay_access_key: key.relay_access_public_key(&space).unwrap(),
            }),
        };
        let signed = sign_device_proposal(&proposal, &anchor_device_key(&person())).unwrap();
        assert_eq!(
            anchor.sequence_signed(&signed).await.unwrap(),
            Err(ProposalRejection::AnchorOnly)
        );
    });
}

#[test]
fn the_first_recovery_key_opens_epoch_one_for_every_published_device() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(3, anchor_keys.clone()).await;
        let member_keys = TestKeys::new(device_key(&person(), 1));
        publish_member(&mut anchor, 1, &member_keys).await;
        let recovery = EncryptionKeyPair::generate().unwrap();
        let revision = start_epochs(&mut anchor, &recovery).await;

        let head = view(&mut anchor).await;
        assert_eq!(head.revision, revision + 1, "the key epoch follows at once");
        assert_eq!(head.key_epoch, 1);
        assert_eq!(head.recovery_encryption_key, Some(recovery.public_key()));
        assert_eq!(head.pending_removal, None);

        let anchor_key = own_key(&mut anchor, &anchor_keys, 1).await.unwrap();
        let mut member = synced_member(1, 3, &mut anchor).await;
        let member_key = own_key(&mut member, &member_keys, 1).await.unwrap();
        assert_eq!(anchor_key.bytes(), member_key.bytes());
        assert_eq!(stored_keys(&mut member).await[0].wrapped_previous, None);

        let unpublished = TestKeys::new(device_key(&person(), 2));
        let mut other = synced_member(2, 3, &mut anchor).await;
        assert!(own_key(&mut other, &unpublished, 1).await.is_none());

        // A device that publishes after epoch 1 gets the current key with its entry.
        publish_member(&mut anchor, 2, &unpublished).await;
        other.pull(&mut anchor).await;
        other.apply_all().await.unwrap();
        let late = own_key(&mut other, &unpublished, 1).await.unwrap();
        assert_eq!(late.bytes(), anchor_key.bytes());
    });
}

#[test]
fn a_removal_seals_with_a_key_epoch_the_removed_device_cannot_open() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(4, anchor_keys.clone()).await;
        let kept_keys = TestKeys::new(device_key(&person(), 1));
        let removed_keys = TestKeys::new(device_key(&person(), 2));
        publish_member(&mut anchor, 1, &kept_keys).await;
        publish_member(&mut anchor, 2, &removed_keys).await;
        start_epochs(&mut anchor, &EncryptionKeyPair::generate().unwrap()).await;

        let removal = anchor.remove(&device_id(2)).await.unwrap();
        let head = view(&mut anchor).await;
        assert_eq!(head.revision, removal + 1);
        assert_eq!(head.key_epoch, 2);
        assert_eq!(head.pending_removal, None);
        let removed = head
            .devices
            .iter()
            .find(|device| device.device_id == device_id(2))
            .unwrap();
        assert_eq!(removed.removed_revision, Some(removal));

        let mut kept = synced_member(1, 4, &mut anchor).await;
        let mut gone = synced_member(2, 4, &mut anchor).await;
        let new_key = own_key(&mut kept, &kept_keys, 2).await.unwrap();
        assert_eq!(
            own_key(&mut anchor, &anchor_keys, 2).await.unwrap().bytes(),
            new_key.bytes()
        );
        assert!(own_key(&mut gone, &removed_keys, 1).await.is_some());
        assert!(own_key(&mut gone, &removed_keys, 2).await.is_none());
        let space = anchor.ctx.space;
        let link = stored_keys(&mut kept).await[1]
            .wrapped_previous
            .clone()
            .unwrap();
        assert_eq!(
            link.open(&space, &new_key).unwrap().bytes(),
            own_key(&mut kept, &kept_keys, 1).await.unwrap().bytes()
        );
    });
}

#[test]
fn a_removal_before_the_first_key_epoch_applies_alone() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 5).await;
        let removal = anchor.remove(&device_id(1)).await.unwrap();
        let head = view(&mut anchor).await;
        assert_eq!(head.revision, removal);
        assert_eq!(head.key_epoch, 0);
        assert_eq!(head.pending_removal, None);
        assert!(anchor.count("SELECT count(*) FROM sync_space_keys").await == 0);
        let removed = head
            .devices
            .iter()
            .find(|device| device.device_id == device_id(1))
            .unwrap();
        assert_eq!(removed.removed_revision, Some(removal));
    });
}

#[test]
fn a_removal_without_its_key_epoch_stays_pending_on_every_replica() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(6, anchor_keys).await;
        start_epochs(&mut anchor, &EncryptionKeyPair::generate().unwrap()).await;
        let removal = anchor.remove(&device_id(3)).await.unwrap();
        let ops = anchor.own_ops().await;
        let (last, prefix) = ops.split_last().unwrap();

        let mut first = Replica::new(1, 6).await;
        let mut second = Replica::new(2, 6).await;
        for member in [&mut first, &mut second] {
            member.receive(prefix).await;
            member.apply_all().await.unwrap();
        }
        let pending = view(&mut first).await;
        assert_eq!(pending, view(&mut second).await);
        assert_eq!(first.dump().await, second.dump().await);
        assert_eq!(
            pending.revision, removal,
            "the pending removal is the selected head"
        );
        assert_eq!(pending.pending_removal, Some(device_id(3)));
        assert_eq!(pending.key_epoch, 1);
        assert_eq!(pending.frozen_at, None);
        assert!(first.evidence().await.is_empty());
        let still_live = pending
            .devices
            .iter()
            .find(|device| device.device_id == device_id(3))
            .unwrap();
        assert_eq!(still_live.removed_revision, None);

        first.receive(std::slice::from_ref(last)).await;
        first.apply_all().await.unwrap();
        let applied = view(&mut first).await;
        assert_eq!(applied.revision, removal + 1);
        assert_eq!(applied.key_epoch, 2);
        assert_eq!(applied.pending_removal, None);
        assert_eq!(applied, view(&mut anchor).await);
    });
}

#[test]
fn a_failed_seal_leaves_neither_the_removal_nor_its_key_epoch() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(7, anchor_keys).await;
        start_epochs(&mut anchor, &EncryptionKeyPair::generate().unwrap()).await;
        let before = anchor.snapshot().await;
        let head = view(&mut anchor).await;

        failpoints::arm("seal_records.between");
        let crashed = anchor.sequence(remove_entry(&device_id(1))).await;
        failpoints::clear();
        assert!(
            matches!(crashed, Err(SyncError::Failpoint("seal_records.between"))),
            "{crashed:?}"
        );
        assert!(anchor.snapshot().await == before, "the crash left records");
        anchor.conn = connection_from(&before).await;
        assert_eq!(view(&mut anchor).await, head);

        let removal = anchor.remove(&device_id(1)).await.unwrap();
        assert_eq!(view(&mut anchor).await.revision, removal + 1);
    });
}

#[test]
fn chain_links_walk_back_through_every_epoch() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(8, anchor_keys.clone()).await;
        start_epochs(&mut anchor, &EncryptionKeyPair::generate().unwrap()).await;
        for index in 1..=4 {
            anchor.remove(&device_id(index)).await.unwrap();
        }
        let stored = stored_keys(&mut anchor).await;
        assert_eq!(
            stored.iter().map(|keys| keys.epoch).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
        let space = anchor.ctx.space;
        let mut key = anchor_keys
            .open(&space, 5, stored[4].own_envelope.as_ref().unwrap())
            .unwrap();
        for index in (1..5).rev() {
            key = stored[index]
                .wrapped_previous
                .as_ref()
                .unwrap()
                .open(&space, &key)
                .unwrap();
            let direct = anchor_keys
                .open(
                    &space,
                    key.epoch(),
                    stored[index - 1].own_envelope.as_ref().unwrap(),
                )
                .unwrap();
            assert_eq!(key.epoch(), stored[index - 1].epoch);
            assert_eq!(key.bytes(), direct.bytes());
        }
    });
}

#[test]
fn an_anchor_without_the_current_key_refuses_to_seal() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(9, anchor_keys).await;
        start_epochs(&mut anchor, &EncryptionKeyPair::generate().unwrap()).await;
        anchor.identity.keys = None;
        let head = view(&mut anchor).await;

        assert_eq!(
            anchor.sequence(remove_entry(&device_id(1))).await.unwrap(),
            Err(ProposalRejection::KeyUnavailable)
        );
        let member_keys = TestKeys::new(device_key(&person(), 1));
        let signed = device_key_proposal(
            &anchor,
            &device_key(&person(), 1),
            &device_id(1),
            member_keys.encryption.public_key(),
        );
        assert_eq!(
            anchor.sequence_signed(&signed).await.unwrap(),
            Err(ProposalRejection::KeyUnavailable)
        );

        // Keys whose encryption key the anchor never published open nothing either.
        anchor.identity.keys = Some(TestKeys::new(anchor_device_key(&person())));
        assert_eq!(
            anchor.sequence(remove_entry(&device_id(1))).await.unwrap(),
            Err(ProposalRejection::KeyUnavailable)
        );
        assert_eq!(view(&mut anchor).await, head);
    });
}

#[test]
fn a_kit_recovered_anchor_opens_the_current_epoch_with_the_recovery_key() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(10, anchor_keys).await;
        // A generated pair stands in for the recovery key the kit derives.
        let recovery = EncryptionKeyPair::generate().unwrap();
        start_epochs(&mut anchor, &recovery).await;

        let mut recovered = synced_member(3, 10, &mut anchor).await;
        let forced = recovered
            .handover_proposal(&recovery_admin(&person()))
            .await;
        recovered
            .seal_handover(&forced, None)
            .await
            .unwrap()
            .unwrap();

        let without_kit = TestKeys::new(device_key(&person(), 3));
        recovered.identity.keys = Some(without_kit.clone());
        let own = device_key_proposal(
            &recovered,
            &device_key(&person(), 3),
            &device_id(3),
            without_kit.encryption.public_key(),
        );
        assert_eq!(
            recovered.sequence_signed(&own).await.unwrap(),
            Err(ProposalRejection::KeyUnavailable)
        );

        let with_kit = TestKeys::with_recovery(device_key(&person(), 3), recovery);
        recovered.identity.keys = Some(with_kit.clone());
        let own = device_key_proposal(
            &recovered,
            &device_key(&person(), 3),
            &device_id(3),
            with_kit.encryption.public_key(),
        );
        recovered.sequence_signed(&own).await.unwrap().unwrap();
        let kit_removal = person_proposal(
            &person(),
            recovered.ctx.space,
            &recovery_admin(&person()),
            recovered.now_ms,
            remove_entry(&device_id(4)),
        );
        let removal = recovered
            .sequence_signed(&kit_removal)
            .await
            .unwrap()
            .unwrap();
        let head = view(&mut recovered).await;
        assert_eq!(head.revision, removal + 1);
        assert_eq!(head.key_epoch, 2);
        let own_only = TestKeys::new(device_key(&person(), 3));
        assert!(own_key(&mut recovered, &with_kit, 2).await.is_some());
        assert!(own_key(&mut recovered, &own_only, 2).await.is_none());
    });
}

#[test]
fn a_snapshot_copy_rebuilds_its_space_keys_from_the_log() {
    block_on(async {
        let anchor_keys = TestKeys::new(anchor_device_key(&person()));
        let mut anchor = keyed_anchor(11, anchor_keys.clone()).await;
        start_epochs(&mut anchor, &EncryptionKeyPair::generate().unwrap()).await;
        let stored = stored_keys(&mut anchor).await;
        assert_eq!(stored.len(), 1);
        assert!(stored[0].own_envelope.is_some());

        let mut copy = connection_from(&anchor.snapshot().await).await;
        prepare_snapshot_copy(&mut copy, anchor.engine.manifest())
            .await
            .unwrap();
        let emptied: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_space_keys")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(
            emptied, 0,
            "a snapshot never carries another device's envelope"
        );

        anchor.conn = copy;
        // Applying notes the local device again, whose envelopes the rebuild keeps.
        anchor.apply_once().await.unwrap();
        assert_eq!(stored_keys(&mut anchor).await, stored);
    });
}
