//! The authorization log: enrollment gating at store, entries the anchor sequences, chain
//! selection, readiness against the selected revision, and the derived device table.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    AdminKeyPair, AuthorizationRecord, Content, DeviceKind, DeviceRole, Digest32, Enroll, Entry,
    Envelope, Handover, Header, HlcClock, Operation, ProposalRejection, Remove, Sequencing,
    SetRole, SignedProposal,
};

use super::block_on;
use super::replica::{
    ANCHOR_DEVICE_ID, Identity, Replica, anchor_admin, device_id, device_key, enroll_entry, person,
    proposal,
};
use crate::{AuthorizationView, HoldReason, StoreOutcome, StoreRefusal};

const STORED: StoreOutcome = StoreOutcome::Stored { held: None };

/// An admin key for replica device `index`.
fn device_admin(index: usize) -> AdminKeyPair {
    let seed = [u8::try_from(index).unwrap().wrapping_add(100); 32];
    AdminKeyPair::from_seed(&seed).unwrap()
}

fn admin_entry(index: usize) -> Entry {
    enroll_entry(
        &person(),
        index,
        DeviceRole::Admin(device_admin(index).public_key()),
    )
}

fn remove_entry(device: &str) -> Entry {
    Entry::Remove(Remove {
        device_id: device.to_owned(),
    })
}

async fn tag(replica: &mut Replica, id: &str) {
    replica
        .exec(&format!(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('{id}', '{id}', 'a0')"
        ))
        .await;
    replica.seal().await.unwrap();
}

async fn view(replica: &mut Replica) -> AuthorizationView {
    replica
        .engine
        .authorization(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .expect("test spaces have a charter")
}

/// Epoch, revision, and entry hash of the selected head.
async fn head(replica: &mut Replica) -> (u32, u64, Digest32) {
    let view = view(replica).await;
    let hash: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT entry_hash FROM sync_auth_entries WHERE selected = 1
         ORDER BY revision DESC LIMIT 1",
    )
    .fetch_optional(&mut replica.conn)
    .await
    .unwrap();
    let hash = match hash {
        Some(bytes) => bytes.try_into().unwrap(),
        None => replica
            .engine
            .charter(&mut replica.conn, &replica.ctx)
            .await
            .unwrap()
            .unwrap()
            .hash(),
    };
    (view.epoch, view.revision, hash)
}

/// The next operation of the replica's own writer with `content`, sealed outside the engine so
/// a test can carry what the engine would never seal. The replica's own log does not hold it.
async fn forge(replica: &mut Replica, content: Content, authorization_revision: u64) -> Vec<u8> {
    let last = replica
        .own_ops()
        .await
        .pop()
        .expect("the replica has sealed");
    let envelope = Envelope::decode(&last).unwrap();
    let previous = envelope.operation().unwrap();
    Operation {
        space: previous.space,
        writer: previous.writer,
        seq: previous.seq + 1,
        previous_hash: envelope.hash(),
        header: Header {
            clock: HlcClock::new(previous.header.clock).tick(replica.now_ms),
            manifest_version: previous.header.manifest_version,
            authorization_revision,
            key_epoch: 0,
            dependencies: Vec::new(),
        },
        content,
    }
    .seal(&replica.identity.key)
    .unwrap()
    .into_bytes()
}

/// `genesis` sealed again as the genesis of `identity`, which the engine would refuse to seal
/// for a device key the chain did not enroll.
fn genesis_of(genesis: &[u8], identity: &Identity) -> Vec<u8> {
    let mut operation = Envelope::decode(genesis).unwrap().operation().unwrap();
    operation.writer = identity.id();
    operation.content = Content::Genesis(identity.certificate.clone());
    operation.seal(&identity.key).unwrap().into_bytes()
}

/// A record that follows the replica's selected head in its epoch.
async fn next_record(
    replica: &mut Replica,
    anchor: &str,
    signed: SignedProposal,
    sequencing: Sequencing,
) -> AuthorizationRecord {
    let (epoch, revision, previous_entry_hash) = head(replica).await;
    AuthorizationRecord {
        epoch,
        revision: revision + 1,
        previous_entry_hash,
        anchor_device_id: anchor.to_owned(),
        proposal: signed,
        sequencing,
    }
}

/// A handover of the anchor to replica device `index`, carried by that replica's writer.
async fn forge_handover(replica: &mut Replica, index: usize) -> (AuthorizationRecord, Vec<u8>) {
    let (epoch, revision, hash) = head(replica).await;
    let device = device_id(index);
    let entry = Entry::Handover(Handover {
        new_anchor_device_id: device.clone(),
        epoch: epoch + 1,
        continues_from_revision: revision,
        continues_from_hash: hash,
    });
    let record = AuthorizationRecord {
        epoch: epoch + 1,
        revision: revision + 1,
        previous_entry_hash: hash,
        anchor_device_id: device,
        proposal: proposal(
            replica.ctx.space,
            &device_admin(index),
            replica.now_ms,
            entry,
        ),
        sequencing: Sequencing::Handover {
            old_anchor_vector: None,
        },
    };
    let bytes = forge(
        replica,
        Content::Authorization(Box::new(record.clone())),
        revision,
    )
    .await;
    (record, bytes)
}

/// A device row: key, role, admin key, enrolled revision, removed revision.
type DeviceRow = (Vec<u8>, String, Option<Vec<u8>>, i64, Option<i64>);

/// A `sync_devices` row as stored: device id, then the columns of [`DeviceRow`].
type StoredDevice = (String, Vec<u8>, String, Option<Vec<u8>>, i64, Option<i64>);

/// Replays the selected entries from the charter and checks `sync_devices` holds exactly what
/// they leave.
async fn assert_devices_replay_the_selected_chain(replica: &mut Replica) {
    let charter = replica
        .engine
        .charter(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .unwrap()
        .charter;
    let mut expected: BTreeMap<String, DeviceRow> = BTreeMap::from([(
        charter.anchor_device_id.clone(),
        (
            charter.anchor_device_key.as_bytes().to_vec(),
            "admin".to_owned(),
            Some(charter.anchor_admin_key.as_bytes().to_vec()),
            0,
            None,
        ),
    )]);
    let envelopes: Vec<(i64, Vec<u8>)> = sqlx::query_as(
        "SELECT e.revision, o.envelope FROM sync_auth_entries e JOIN sync_ops o
            ON o.space_id = e.space_id AND o.writer_id = e.writer_id AND o.seq = e.seq
         WHERE e.selected = 1 ORDER BY e.revision",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap();
    let role_row = |role: DeviceRole| match role {
        DeviceRole::Member => ("member".to_owned(), None),
        DeviceRole::Admin(key) => ("admin".to_owned(), Some(key.as_bytes().to_vec())),
    };
    for (position, (revision, bytes)) in envelopes.iter().enumerate() {
        assert_eq!(*revision, i64::try_from(position + 1).unwrap());
        let Content::Authorization(record) = Envelope::decode(bytes)
            .unwrap()
            .operation()
            .unwrap()
            .content
        else {
            panic!("a selected entry is not a kind 4 operation");
        };
        match record.proposal.proposal.entry {
            Entry::Enroll(enroll) => {
                let (role, key) = role_row(enroll.role);
                expected.insert(
                    enroll.device_id,
                    (
                        enroll.device_key.as_bytes().to_vec(),
                        role,
                        key,
                        *revision,
                        None,
                    ),
                );
            }
            Entry::Remove(remove) => {
                expected.get_mut(&remove.device_id).unwrap().4 = Some(*revision);
            }
            Entry::SetRole(SetRole::Device { device_id, role }) => {
                let (role, key) = role_row(role);
                let device = expected.get_mut(&device_id).unwrap();
                device.1 = role;
                device.2 = key;
            }
            _ => {}
        }
    }
    let rows: Vec<StoredDevice> = sqlx::query_as(
        "SELECT device_id, device_key, role, admin_key, enrolled_revision, removed_revision
         FROM sync_devices ORDER BY device_id",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap();
    let derived: BTreeMap<String, DeviceRow> = rows
        .into_iter()
        .map(|(device, key, role, admin, enrolled, removed)| {
            (device, (key, role, admin, enrolled, removed))
        })
        .collect();
    assert_eq!(derived, expected);
}

#[test]
fn store_admits_only_writers_of_enrolled_devices_with_their_enrolled_key() {
    block_on(async {
        let mut hub = Replica::anchor(0, 31).await;

        // An enrolled device id under another device key is untrusted.
        let mut member = Replica::new(1, 31).await;
        tag(&mut member, "tag-member").await;
        let member_ops = member.own_ops().await;
        let impostor = Identity::new([33; 32], &device_key(&person(), 2), &device_id(1), None);
        assert_eq!(
            hub.receive(&[genesis_of(&member_ops[0], &impostor)]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Untrusted)]
        );

        // A device the chain has not enrolled is refused until its enrollment arrives: the
        // anchor stores the newcomer, and a replica that receives the forwarded genesis before
        // applying the enrollment stores it on the next delivery.
        let enrolled = hub
            .sequence(enroll_entry(&person(), 20, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        let mut newcomer = Replica::new(20, 31).await;
        newcomer.pull(&mut hub).await;
        tag(&mut newcomer, "tag-new").await;
        let newcomer_ops = newcomer.own_ops().await;
        let mut observer = Replica::new(2, 31).await;
        assert_eq!(
            observer.receive(&newcomer_ops).await[0],
            StoreOutcome::Refused(StoreRefusal::Unenrolled)
        );
        assert_eq!(hub.receive(&newcomer_ops).await, vec![STORED; 2]);
        hub.apply_all().await.unwrap();
        observer.pull(&mut hub).await;
        assert_eq!(view(&mut observer).await.revision, enrolled);
        observer.pull(&mut hub).await;
        assert_eq!(observer.stored_vector().await.get(&newcomer.writer()), 2);

        // A removal excludes the device's operations past the cutoff, which are still stored,
        // and refuses its new writers.
        hub.pull(&mut member).await;
        let removed = hub.remove(&member.device_id).await.unwrap();
        tag(&mut member, "tag-late").await;
        let late = member.own_ops().await;
        assert_eq!(
            hub.receive(&late[2..]).await,
            vec![StoreOutcome::Stored {
                held: Some(HoldReason::Invalid)
            }]
        );
        member.rotate();
        tag(&mut member, "tag-rotated").await;
        let rotated = member.own_ops().await;
        assert_eq!(
            hub.receive(&rotated[..1]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Revoked)]
        );
        observer.pull(&mut hub).await;
        let removal = view(&mut observer).await;
        assert_eq!(removal.revision, removed);
        assert!(removal.devices.iter().any(|device| {
            device.device_id == member.device_id && device.removed_revision == Some(removed)
        }));
        assert_devices_replay_the_selected_chain(&mut observer).await;
        // The first delivery enrolls the newcomer, whose genesis the second one stores.
        member.pull(&mut hub).await;
        member.pull(&mut hub).await;
        assert!(member.device_removed().await);
    });
}

#[test]
fn entries_are_taken_only_from_the_anchor_and_unknown_kinds_are_kept_but_ignored() {
    block_on(async {
        let mut hub = Replica::anchor(0, 33).await;
        let mut observer = Replica::new(2, 33).await;
        let before = view(&mut hub).await;

        // A member carrying a valid anchor-signed entry is held, not taken.
        let mut member = Replica::new(1, 33).await;
        tag(&mut member, "tag-member").await;
        let space = member.ctx.space;
        let signed = proposal(
            space,
            &anchor_admin(&person()),
            member.now_ms,
            enroll_entry(&person(), 21, DeviceRole::Member),
        );
        let record = next_record(&mut member, ANCHOR_DEVICE_ID, signed, Sequencing::Empty).await;
        let carried = forge(
            &mut member,
            Content::Authorization(Box::new(record)),
            before.revision,
        )
        .await;
        let mut member_ops = member.own_ops().await;
        member_ops.push(carried);
        assert_eq!(hub.receive(&member_ops).await, vec![STORED; 3]);
        hub.apply_all().await.unwrap();
        assert_eq!(view(&mut hub).await, before);
        let held = hub
            .engine
            .held_ops(&mut hub.conn, &hub.ctx, 10)
            .await
            .unwrap();
        assert_eq!((held[0].writer, held[0].seq), (member.writer(), 3));

        // An entry kind this build does not know is stored, forwarded, and chained, and changes
        // nothing.
        let unknown = proposal(
            space,
            &anchor_admin(&person()),
            hub.now_ms,
            Entry::Unknown {
                kind: 200,
                bytes: vec![1, 2, 3],
            },
        );
        let record = next_record(
            &mut hub,
            ANCHOR_DEVICE_ID,
            unknown,
            Sequencing::Opaque(vec![4, 5]),
        )
        .await;
        let unknown_revision = record.revision;
        let forged = forge(
            &mut hub,
            Content::Authorization(Box::new(record)),
            before.revision,
        )
        .await;
        assert_eq!(hub.receive(&[forged]).await, vec![STORED]);
        hub.apply_all().await.unwrap();
        observer.pull(&mut hub).await;
        let ignored = view(&mut observer).await;
        assert_eq!(ignored.revision, unknown_revision);
        assert_eq!(ignored.devices, before.devices);
        let kinds: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT revision, selected FROM sync_auth_entries WHERE entry_kind = 200",
        )
        .fetch_all(&mut observer.conn)
        .await
        .unwrap();
        assert_eq!(kinds, [(i64::try_from(unknown_revision).unwrap(), 1)]);

        let next = hub
            .sequence(enroll_entry(&person(), 20, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next, unknown_revision + 1);
        observer.pull(&mut hub).await;
        let chained = view(&mut observer).await;
        assert_eq!(chained.revision, next);
        assert!(
            chained
                .devices
                .iter()
                .any(|device| device.device_id == device_id(20))
        );
        assert!(
            !chained
                .devices
                .iter()
                .any(|device| device.device_id == device_id(21))
        );
        assert_devices_replay_the_selected_chain(&mut observer).await;
    });
}

#[test]
fn a_handover_supersedes_the_old_epoch_and_equal_epoch_handovers_take_the_lower_hash() {
    block_on(async {
        let mut hub = Replica::anchor(0, 41).await;
        for index in [20, 21] {
            hub.sequence(admin_entry(index)).await.unwrap().unwrap();
        }
        let mut contenders = Vec::new();
        for index in [20, 21] {
            let mut contender = Replica::new(index, 41).await;
            contender.pull(&mut hub).await;
            tag(&mut contender, &format!("tag-{index}")).await;
            contenders.push(contender);
        }
        let continued = view(&mut hub).await.revision;

        // The old anchor keeps sequencing in epoch 0 while both admins take over.
        let superseded = hub
            .sequence(enroll_entry(&person(), 22, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        let mut handovers = Vec::new();
        for (contender, index) in contenders.iter_mut().zip([20, 21]) {
            let (record, bytes) = forge_handover(contender, index).await;
            let mut ops = contender.own_ops().await;
            ops.push(bytes);
            handovers.push((record, ops));
        }
        let winner = handovers
            .iter()
            .min_by_key(|(record, _)| record.hash())
            .map(|(record, _)| record.anchor_device_id.clone())
            .unwrap();

        let mut observers = vec![Replica::new(2, 41).await, Replica::new(3, 41).await];
        for (position, observer) in observers.iter_mut().enumerate() {
            observer.pull(&mut hub).await;
            for offset in 0..handovers.len() {
                let (_, ops) = &handovers[(position + offset) % handovers.len()];
                assert_eq!(observer.receive(ops).await, vec![STORED; ops.len()]);
                observer.apply_all().await.unwrap();
            }
        }
        for (_, ops) in &handovers {
            hub.receive(ops).await;
        }
        hub.apply_all().await.unwrap();

        let selected = view(&mut hub).await;
        assert_eq!(selected.epoch, 1);
        assert_eq!(selected.revision, continued + 1);
        assert_eq!(selected.anchor_device_id, winner);
        assert!(
            !selected
                .devices
                .iter()
                .any(|device| device.device_id == device_id(22)),
            "the old epoch's entry at revision {superseded} is superseded"
        );
        for observer in &mut observers {
            assert_eq!(view(observer).await, selected);
            assert_devices_replay_the_selected_chain(observer).await;
        }
        assert_devices_replay_the_selected_chain(&mut hub).await;
        let unselected: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sync_auth_entries WHERE selected = 0")
                .fetch_one(&mut hub.conn)
                .await
                .unwrap();
        assert_eq!(
            unselected, 2,
            "the losing handover and the superseded enrollment"
        );
        assert_eq!(
            hub.sequence(enroll_entry(&person(), 23, DeviceRole::Member))
                .await
                .unwrap(),
            Err(ProposalRejection::NotAnchor)
        );
    });
}

#[test]
fn operations_wait_for_the_revision_they_were_sealed_against_but_entries_never_wait_on_themselves()
{
    block_on(async {
        let mut hub = Replica::anchor(0, 51).await;
        let mut member = Replica::new(1, 51).await;
        tag(&mut member, "tag-1").await;
        let template = view(&mut member).await.revision;
        let revision = hub
            .sequence(enroll_entry(&person(), 20, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(revision, template + 1);
        let entry = hub.own_ops().await.pop().unwrap();
        let operation = Envelope::decode(&entry).unwrap().operation().unwrap();
        assert_eq!(operation.header.authorization_revision, template);

        // A member operation sealed against the new revision, with no dependency on the entry.
        let mut ops = member.own_ops().await;
        ops.push(forge(&mut member, Content::Acknowledgement, revision).await);
        let mut observer = Replica::new(2, 51).await;
        assert_eq!(observer.receive(&ops).await, vec![STORED; 3]);
        observer.apply_all().await.unwrap();
        assert_eq!(observer.applied_vector().await.get(&member.writer()), 2);

        observer.pull(&mut hub).await;
        assert_eq!(view(&mut observer).await.revision, revision);
        assert_eq!(observer.applied_vector().await.get(&member.writer()), 3);
    });
}

#[test]
fn the_anchor_rejects_entries_its_rules_forbid() {
    block_on(async {
        let mut hub = Replica::anchor(0, 61).await;
        for index in [20, 21] {
            hub.sequence(admin_entry(index)).await.unwrap().unwrap();
        }

        // Two admins removing each other: the first sequenced wins and the second author is no
        // longer an admin.
        let space = hub.ctx.space;
        let first = proposal(
            space,
            &device_admin(20),
            hub.now_ms,
            remove_entry(&device_id(21)),
        );
        let second = proposal(
            space,
            &device_admin(21),
            hub.now_ms,
            remove_entry(&device_id(20)),
        );
        let removed = hub.sequence_signed(&first).await.unwrap().unwrap();
        assert_eq!(
            hub.sequence_signed(&second).await.unwrap(),
            Err(ProposalRejection::NotAdmin)
        );
        assert_eq!(hub.sequence_signed(&first).await.unwrap(), Ok(removed));
        let devices = view(&mut hub).await.devices;
        let removed_of = |index: usize| {
            devices
                .iter()
                .find(|device| device.device_id == device_id(index))
                .unwrap()
                .removed_revision
        };
        assert_eq!((removed_of(20), removed_of(21)), (None, Some(removed)));

        // The anchor cannot be removed or demoted.
        assert_eq!(
            hub.sequence(remove_entry(ANCHOR_DEVICE_ID)).await.unwrap(),
            Err(ProposalRejection::NamesAnchor)
        );
        let demote = Entry::SetRole(SetRole::Device {
            device_id: ANCHOR_DEVICE_ID.to_owned(),
            role: DeviceRole::Member,
        });
        assert_eq!(
            hub.sequence(demote).await.unwrap(),
            Err(ProposalRejection::NamesAnchor)
        );

        // Only desktops can be admins.
        let mobile = |role: DeviceRole| {
            Entry::Enroll(Enroll {
                device_id: device_id(22),
                device_key: device_key(&person(), 22).public_key(),
                tls_fingerprint: [3; 32],
                label: "Phone".to_owned(),
                device_kind: DeviceKind::Mobile,
                role,
            })
        };
        let admin = DeviceRole::Admin(device_admin(22).public_key());
        assert_eq!(
            hub.sequence(mobile(admin)).await.unwrap(),
            Err(ProposalRejection::AdminNotDesktop)
        );
        hub.sequence(mobile(DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        let promote = Entry::SetRole(SetRole::Device {
            device_id: device_id(22),
            role: admin,
        });
        assert_eq!(
            hub.sequence(promote).await.unwrap(),
            Err(ProposalRejection::AdminNotDesktop)
        );

        // Removed devices, strangers, and non-anchor devices cannot sequence.
        assert_eq!(
            hub.sequence(remove_entry(&device_id(21))).await.unwrap(),
            Err(ProposalRejection::DeviceRemoved)
        );
        let stranger = AdminKeyPair::from_seed(&[77; 32]).unwrap();
        let unsigned = proposal(
            space,
            &stranger,
            hub.now_ms,
            enroll_entry(&person(), 23, DeviceRole::Member),
        );
        assert_eq!(
            hub.sequence_signed(&unsigned).await.unwrap(),
            Err(ProposalRejection::NotAdmin)
        );
        let mut member = Replica::new(1, 61).await;
        assert_eq!(
            member
                .sequence(enroll_entry(&person(), 23, DeviceRole::Member))
                .await
                .unwrap(),
            Err(ProposalRejection::NotAnchor)
        );
        assert_devices_replay_the_selected_chain(&mut hub).await;
    });
}
