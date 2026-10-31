//! The authorization log: enrollment gating at store, entries the anchor sequences, chain
//! selection, readiness against the selected revision, and the derived device table.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    AdminKeyPair, AuthorizationRecord, Content, DeviceKind, DeviceRole, Digest32, Enroll, Entry,
    Envelope, Handover, Header, HlcClock, Operation, ProposalRejection, Remove, Sequencing,
    SetRole, SignedProposal, WriterKeyPair, Yield,
};

use super::block_on;
use super::exclusion::{edit, note};
use super::replica::{
    ANCHOR_DEVICE_ID, Identity, Replica, anchor_admin, device_admin, device_id, device_key,
    enroll_entry, person, proposal, recovery_admin, tls_fingerprint,
};
use crate::authorization::tour_spans;
use crate::store::MAX_UNSELECTED_WRITERS_PER_SPACE;
use crate::{
    AuthorizationView, HoldReason, LateChange, MAX_EXCLUDED_PER_WRITER, StoreOutcome, StoreRefusal,
};
use ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_DEVICE;

const STORED: StoreOutcome = StoreOutcome::Stored { held: None };

pub(super) fn admin_entry(index: usize) -> Entry {
    enroll_entry(
        &person(),
        index,
        DeviceRole::Admin(device_admin(index).public_key()),
    )
}

pub(super) fn remove_entry(device: &str) -> Entry {
    Entry::Remove(Remove {
        device_id: device.to_owned(),
    })
}

pub(super) async fn tag(replica: &mut Replica, id: &str) {
    replica
        .exec(&format!(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('{id}', '{id}', 'a0')"
        ))
        .await;
    replica.seal().await.unwrap();
}

/// The revision that removed `device_id` on `replica`, `None` for a device it does not know.
async fn removed_revision(replica: &mut Replica, device_id: &str) -> Option<Option<i64>> {
    sqlx::query_scalar("SELECT removed_revision FROM sync_devices WHERE device_id = ?")
        .bind(device_id)
        .fetch_optional(&mut replica.conn)
        .await
        .unwrap()
}

pub(super) async fn view(replica: &mut Replica) -> AuthorizationView {
    replica
        .engine
        .authorization(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
        .expect("test spaces have a charter")
}

/// Epoch, revision, and entry hash of the selected head.
pub(super) async fn head(replica: &mut Replica) -> (u32, u64, Digest32) {
    let view = view(replica).await;
    let hash: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT entry_hash FROM sync_auth_selected ORDER BY revision DESC LIMIT 1",
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
    seal_after(
        &last,
        &replica.identity.key,
        content,
        authorization_revision,
        replica.now_ms,
    )
}

/// The operation `key` seals with `content` right after `previous`, under the same writer and
/// space, outside the engine.
pub(super) fn seal_after(
    previous: &[u8],
    key: &WriterKeyPair,
    content: Content,
    authorization_revision: u64,
    now_ms: u64,
) -> Vec<u8> {
    let envelope = Envelope::decode(previous).unwrap();
    let previous = envelope.operation().unwrap();
    Operation {
        space: previous.space,
        writer: previous.writer,
        seq: previous.seq + 1,
        previous_hash: envelope.hash(),
        header: Header {
            clock: HlcClock::new(previous.header.clock).tick(now_ms),
            manifest_version: previous.header.manifest_version,
            authorization_revision,
            key_epoch: 0,
            group_continues: false,
            dependencies: Vec::new(),
        },
        content,
    }
    .seal(key)
    .unwrap()
    .into_bytes()
}

/// `genesis` sealed again as the genesis of `identity`, which the engine would refuse to seal
/// for a device key the chain did not enroll.
pub(super) fn genesis_of(genesis: &[u8], identity: &Identity) -> Vec<u8> {
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
pub(super) async fn forge_handover(
    replica: &mut Replica,
    index: usize,
) -> (AuthorizationRecord, Vec<u8>) {
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
        "SELECT s.revision, e.envelope FROM sync_auth_selected s JOIN sync_auth_envelopes e
            ON e.space_id = s.space_id AND e.role = 'record' AND e.envelope_hash = s.envelope_hash
         ORDER BY s.revision",
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
            // A handover its target's own admin key does not sign is a kit or recovery one,
            // which leaves no admin but the target.
            Entry::Handover(handover) => {
                let author = record.proposal.proposal.author.as_bytes().to_vec();
                let target = &handover.new_anchor_device_id;
                if expected[target].2.as_ref() != Some(&author) {
                    for (device_id, device) in &mut expected {
                        if device_id != target && device.4.is_none() {
                            device.1 = "member".to_owned();
                            device.2 = None;
                        }
                    }
                }
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

        // A removal excludes the device's operations past the cutoff, which are still stored.
        // A writer of the removed device the hub never stored has a cutoff of zero, so its
        // chain is stored and excluded from the genesis on.
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
            vec![StoreOutcome::Stored {
                held: Some(HoldReason::Invalid)
            }]
        );
        let impostor = Identity::new([34; 32], &device_key(&person(), 2), &member.device_id, None);
        assert_eq!(
            hub.receive(&[genesis_of(&rotated[0], &impostor)]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Untrusted)],
            "a removed device id under another device key is untrusted"
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
        let selected_unknown: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sync_auth_selected WHERE revision = ?")
                .bind(i64::try_from(unknown_revision).unwrap())
                .fetch_one(&mut observer.conn)
                .await
                .unwrap();
        assert_eq!(selected_unknown, 1);

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
fn concurrent_takes_freeze_the_old_epoch_until_the_kit_supersedes_it() {
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

        // Two takes out of one epoch instance contest it: it freezes at their shared point and
        // the old anchor's later entry is past the freeze.
        let frozen = view(&mut hub).await;
        assert_eq!(
            (frozen.epoch, frozen.revision, frozen.frozen_at),
            (0, continued, Some(continued))
        );
        assert_eq!(frozen.anchor_device_id, ANCHOR_DEVICE_ID);
        assert!(
            !frozen
                .devices
                .iter()
                .any(|device| device.device_id == device_id(22)),
            "the old epoch's entry at revision {superseded} is past the freeze"
        );
        for observer in &mut observers {
            assert_eq!(view(observer).await, frozen);
        }

        // The kit's handover from the freeze point pins it and demotes both takers.
        let kit = hub.handover_proposal(&recovery_admin(&person())).await;
        let revision = hub.seal_handover(&kit, None).await.unwrap().unwrap();
        let selected = view(&mut hub).await;
        assert_eq!(
            (selected.epoch, selected.revision, selected.frozen_at),
            (1, revision, None)
        );
        assert_eq!(selected.anchor_device_id, ANCHOR_DEVICE_ID);
        assert!(
            selected
                .devices
                .iter()
                .filter(|device| device.device_id != ANCHOR_DEVICE_ID)
                .all(|device| device.role == DeviceRole::Member)
        );
        for observer in &mut observers {
            observer.pull(&mut hub).await;
            assert_eq!(view(observer).await, selected);
            assert_devices_replay_the_selected_chain(observer).await;
        }
        assert_devices_replay_the_selected_chain(&mut hub).await;
        let unselected: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT entry_hash) FROM sync_auth_envelopes
                 WHERE role = 'record'
                    AND entry_hash NOT IN (SELECT entry_hash FROM sync_auth_selected)",
        )
        .fetch_one(&mut hub.conn)
        .await
        .unwrap();
        assert_eq!(unselected, 3, "both takes and the superseded enrollment");
    });
}

#[test]
fn the_named_admin_desktop_seals_a_handover_from_the_head_it_continues() {
    block_on(async {
        let mut hub = Replica::anchor(0, 43).await;
        for index in [20, 21] {
            hub.sequence(admin_entry(index)).await.unwrap().unwrap();
        }
        let mut admin = Replica::new(20, 43).await;
        let mut other = Replica::new(21, 43).await;
        admin.pull(&mut hub).await;
        other.pull(&mut hub).await;
        let stale = admin.handover_proposal(&device_admin(20)).await;

        // Only the device a handover names seals it, and only from the head it continues.
        assert_eq!(
            other.seal_handover(&stale, None).await.unwrap(),
            Err(ProposalRejection::NotAnchor)
        );
        hub.sequence(enroll_entry(&person(), 22, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        admin.pull(&mut hub).await;
        assert_eq!(
            admin.seal_handover(&stale, None).await.unwrap(),
            Err(ProposalRejection::HandoverConflict)
        );
        let signed = admin.handover_proposal(&device_admin(20)).await;
        assert_eq!(
            admin.sequence_signed(&signed).await.unwrap(),
            Err(ProposalRejection::NotAnchor)
        );
        let enroll = proposal(
            admin.ctx.space,
            &device_admin(20),
            admin.now_ms,
            enroll_entry(&person(), 23, DeviceRole::Member),
        );
        assert_eq!(
            admin.seal_handover(&enroll, None).await.unwrap(),
            Err(ProposalRejection::UnsupportedEntry)
        );

        let old_vector = hub.stored_vector().await;
        let revision = admin
            .seal_handover(&signed, Some(old_vector))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            admin.seal_handover(&signed, None).await.unwrap(),
            Ok(revision)
        );
        let opened = view(&mut admin).await;
        assert_eq!(
            (
                opened.epoch,
                opened.revision,
                opened.anchor_device_id.as_str()
            ),
            (1, revision, device_id(20).as_str())
        );

        // The new anchor sequences in its epoch; the old one no longer can.
        let promoted = admin.sequence_signed(&enroll).await.unwrap().unwrap();
        assert_eq!(promoted, revision + 1);
        hub.pull(&mut admin).await;
        other.pull(&mut admin).await;
        assert_eq!(view(&mut hub).await, view(&mut admin).await);
        assert_eq!(view(&mut other).await, view(&mut admin).await);
        assert_eq!(
            hub.sequence(enroll_entry(&person(), 24, DeviceRole::Member))
                .await
                .unwrap(),
            Err(ProposalRejection::NotAnchor)
        );
        assert_devices_replay_the_selected_chain(&mut other).await;
    });
}

#[test]
fn two_takes_from_one_head_freeze_it_and_refuse_a_third_until_the_kit_recovers() {
    block_on(async {
        let mut hub = Replica::anchor(0, 44).await;
        for index in [20, 21, 22] {
            hub.sequence(admin_entry(index)).await.unwrap().unwrap();
        }
        let mut first = Replica::new(20, 44).await;
        let mut second = Replica::new(21, 44).await;
        let mut third = Replica::new(22, 44).await;
        for replica in [&mut first, &mut second, &mut third] {
            replica.pull(&mut hub).await;
        }
        let (_, point, _) = head(&mut hub).await;

        // Two admins take the anchor from the same head, unaware of each other.
        let to_first = first.handover_proposal(&device_admin(20)).await;
        first.seal_handover(&to_first, None).await.unwrap().unwrap();
        let to_second = second.handover_proposal(&device_admin(21)).await;
        second
            .seal_handover(&to_second, None)
            .await
            .unwrap()
            .unwrap();
        hub.pull(&mut first).await;
        hub.pull(&mut second).await;
        let frozen = view(&mut hub).await;
        assert_eq!((frozen.revision, frozen.frozen_at), (point, Some(point)));
        assert_eq!(frozen.anchor_device_id, ANCHOR_DEVICE_ID);
        assert_ne!(
            hub.sequence(enroll_entry(&person(), 23, DeviceRole::Member))
                .await
                .unwrap(),
            Ok(point + 1)
        );

        // A device that knows the contest refuses to add a third take to it.
        third.pull(&mut hub).await;
        assert_eq!(view(&mut third).await, frozen);
        let to_third = third.handover_proposal(&device_admin(22)).await;
        assert_eq!(
            third.seal_handover(&to_third, None).await.unwrap(),
            Err(ProposalRejection::HandoverConflict)
        );

        // The kit's handover at the point recovers the space.
        let forced = third.handover_proposal(&recovery_admin(&person())).await;
        third.seal_handover(&forced, None).await.unwrap().unwrap();
        let recovered = view(&mut third).await;
        assert_eq!(
            (recovered.anchor_device_id.as_str(), recovered.frozen_at),
            (device_id(22).as_str(), None)
        );
        hub.pull(&mut third).await;
        assert_eq!(view(&mut hub).await, recovered);
    });
}

#[test]
fn a_removal_of_the_anchor_under_one_of_two_takes_stands_once_the_kit_continues_it() {
    block_on(async {
        let mut hub = Replica::anchor(0, 45).await;
        for index in [20, 21, 22] {
            hub.sequence(admin_entry(index)).await.unwrap().unwrap();
        }
        tag(&mut hub, "tag-before").await;
        let mut first = Replica::new(20, 45).await;
        let mut second = Replica::new(21, 45).await;
        let mut third = Replica::new(22, 45).await;
        for replica in [&mut first, &mut second, &mut third] {
            replica.pull(&mut hub).await;
        }
        let (_, point, _) = head(&mut hub).await;

        // Both takers act under their takes: the first removes the old anchor, the second
        // enrolls a device.
        let to_first = first.handover_proposal(&device_admin(20)).await;
        first.seal_handover(&to_first, None).await.unwrap().unwrap();
        let to_second = second.handover_proposal(&device_admin(21)).await;
        second
            .seal_handover(&to_second, None)
            .await
            .unwrap()
            .unwrap();
        let removal = proposal(
            first.ctx.space,
            &device_admin(20),
            first.now_ms,
            remove_entry(ANCHOR_DEVICE_ID),
        );
        first.sequence_signed(&removal).await.unwrap().unwrap();
        let enroll = proposal(
            second.ctx.space,
            &device_admin(21),
            second.now_ms,
            enroll_entry(&person(), 23, DeviceRole::Member),
        );
        second.sequence_signed(&enroll).await.unwrap().unwrap();
        // The old anchor keeps writing past the cutoff the removal sealed.
        tag(&mut hub, "tag-after").await;

        for from in [&mut first, &mut second, &mut hub] {
            third.pull(from).await;
        }
        let frozen = view(&mut third).await;
        assert_eq!((frozen.revision, frozen.frozen_at), (point, Some(point)));
        assert_eq!(frozen.anchor_device_id, ANCHOR_DEVICE_ID);
        assert_eq!(
            removed_revision(&mut third, ANCHOR_DEVICE_ID).await,
            Some(None),
            "a removal on a contested branch is not effective"
        );
        assert_eq!(removed_revision(&mut third, &device_id(23)).await, None);
        // It holds the old anchor for data instead: its later write is stored, not applied.
        assert_eq!(tags(&mut third).await, vec!["tag-before".to_owned()]);

        // The kit's handover continuing the first taker's branch keeps its removal.
        let forced = first.handover_proposal(&recovery_admin(&person())).await;
        first.seal_handover(&forced, None).await.unwrap().unwrap();
        third.pull(&mut first).await;
        let recovered = view(&mut third).await;
        assert_eq!(
            (recovered.anchor_device_id.as_str(), recovered.frozen_at),
            (device_id(20).as_str(), None)
        );
        assert!(matches!(
            removed_revision(&mut third, ANCHOR_DEVICE_ID).await,
            Some(Some(_))
        ));
        assert_eq!(removed_revision(&mut third, &device_id(23)).await, None);
        assert_eq!(tags(&mut third).await, vec!["tag-before".to_owned()]);
    });
}

#[test]
fn only_the_recovery_key_forces_a_handover_to_a_member_desktop() {
    block_on(async {
        let mut hub = Replica::anchor(0, 47).await;
        hub.sequence(admin_entry(20)).await.unwrap().unwrap();
        for index in [21, 23] {
            hub.sequence(enroll_entry(&person(), index, DeviceRole::Member))
                .await
                .unwrap()
                .unwrap();
        }
        hub.sequence(Entry::Enroll(Enroll {
            device_id: device_id(22),
            device_key: device_key(&person(), 22).public_key(),
            tls_fingerprint: tls_fingerprint(22),
            label: "Phone".to_owned(),
            device_kind: DeviceKind::Mobile,
            role: DeviceRole::Member,
        }))
        .await
        .unwrap()
        .unwrap();
        hub.remove(&device_id(23)).await.unwrap();
        let recovery = recovery_admin(&person());
        let mut member = Replica::new(21, 47).await;
        let mut phone = Replica::new(22, 47).await;
        let mut removed = Replica::new(23, 47).await;
        for replica in [&mut member, &mut phone, &mut removed] {
            replica.pull(&mut hub).await;
        }

        // An admin key cannot hand the anchor to a desktop that is not an admin.
        let by_admin = member.handover_proposal(&device_admin(20)).await;
        assert_eq!(
            member.seal_handover(&by_admin, None).await.unwrap(),
            Err(ProposalRejection::NotAdmin)
        );
        // The recovery key can, but only to a live desktop or server.
        let to_phone = phone.handover_proposal(&recovery).await;
        assert_eq!(
            phone.seal_handover(&to_phone, None).await.unwrap(),
            Err(ProposalRejection::AdminNotDesktop)
        );
        let to_removed = removed.handover_proposal(&recovery).await;
        assert_eq!(
            removed.seal_handover(&to_removed, None).await.unwrap(),
            Err(ProposalRejection::DeviceRemoved)
        );
        let forced = member.handover_proposal(&recovery).await;
        let revision = member.seal_handover(&forced, None).await.unwrap().unwrap();

        // The forced anchor sequences its own promotion signed by the recovery key.
        let promote = proposal(
            member.ctx.space,
            &recovery,
            member.now_ms,
            Entry::SetRole(SetRole::Device {
                device_id: device_id(21),
                role: DeviceRole::Admin(device_admin(21).public_key()),
            }),
        );
        assert_eq!(
            member.sequence_signed(&promote).await.unwrap(),
            Ok(revision + 1)
        );
        let opened = view(&mut member).await;
        assert_eq!(
            (opened.epoch, opened.anchor_device_id.as_str()),
            (1, device_id(21).as_str())
        );
        hub.pull(&mut member).await;
        assert_eq!(view(&mut hub).await, opened);
        assert_eq!(
            hub.sequence(enroll_entry(&person(), 24, DeviceRole::Member))
                .await
                .unwrap(),
            Err(ProposalRejection::NotAnchor)
        );
    });
}

#[test]
fn a_new_anchor_keeps_what_the_old_anchor_stored_final_and_waits_for_writers_it_lacks() {
    block_on(async {
        let mut hub = Replica::anchor(0, 44).await;
        hub.sequence(admin_entry(20)).await.unwrap().unwrap();
        for index in [22, 23] {
            hub.sequence(enroll_entry(&person(), index, DeviceRole::Member))
                .await
                .unwrap()
                .unwrap();
        }
        let mut admin = Replica::new(20, 44).await;
        let mut known = Replica::new(22, 44).await;
        let mut unknown = Replica::new(23, 44).await;
        admin.pull(&mut hub).await;
        known.pull(&mut hub).await;
        unknown.pull(&mut hub).await;
        tag(&mut known, "first").await;
        admin.pull(&mut known).await;
        hub.pull(&mut known).await;
        // The old anchor stores operations the new one lacks, of a writer it knows and of one
        // it has never seen.
        tag(&mut known, "second").await;
        hub.pull(&mut known).await;
        tag(&mut unknown, "third").await;
        hub.pull(&mut unknown).await;

        let signed = admin.handover_proposal(&device_admin(20)).await;
        let old_vector = hub.stored_vector().await;
        admin
            .seal_handover(&signed, Some(old_vector))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            admin.sequence(remove_entry(&device_id(22))).await.unwrap(),
            Err(ProposalRejection::CatchingUp)
        );
        admin.pull(&mut unknown).await;
        admin
            .sequence(remove_entry(&device_id(22)))
            .await
            .unwrap()
            .unwrap();

        // What the old anchor stored is final, so arriving after the removal keeps it.
        admin.pull(&mut hub).await;
        hub.pull(&mut admin).await;
        for replica in [&mut admin, &mut hub] {
            assert_eq!(
                replica
                    .count("SELECT COUNT(*) FROM quick_note_tags WHERE id = 'second'")
                    .await,
                1
            );
            assert!(late_changes(replica).await.is_empty());
        }
    });
}

/// Selected head, entries with their selected flag, and operation states with exclusions.
type Settled = (
    (u32, u64, Digest32),
    Vec<(Vec<u8>, i64, i64)>,
    Vec<(Vec<u8>, i64, String, Option<i64>)>,
);

async fn settled(replica: &mut Replica) -> Settled {
    let head = head(replica).await;
    let entries = sqlx::query_as(
        "SELECT r.writer_id, r.seq,
            EXISTS (SELECT 1 FROM sync_auth_selected s WHERE s.entry_hash = r.entry_hash)
         FROM sync_auth_envelopes r WHERE r.role = 'record' ORDER BY r.writer_id, r.seq",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap();
    let ops = sqlx::query_as(
        "SELECT writer_id, seq, state, excluded_revision FROM sync_ops ORDER BY writer_id, seq",
    )
    .fetch_all(&mut replica.conn)
    .await
    .unwrap();
    (head, entries, ops)
}

async fn late_changes(replica: &mut Replica) -> Vec<LateChange> {
    replica
        .engine
        .late_changes(&mut replica.conn, &replica.ctx)
        .await
        .unwrap()
}

async fn tags(replica: &mut Replica) -> Vec<String> {
    sqlx::query_scalar("SELECT id FROM quick_note_tags ORDER BY id")
        .fetch_all(&mut replica.conn)
        .await
        .unwrap()
}

#[test]
fn two_admins_removing_each_other_freeze_until_one_yields_whatever_order_their_chains_arrive_in() {
    block_on(async {
        let mut hub = Replica::anchor(0, 71).await;
        for index in [20, 21] {
            hub.sequence(admin_entry(index)).await.unwrap().unwrap();
        }
        let mut contenders = Vec::new();
        for index in [20, 21] {
            let mut contender = Replica::new(index, 71).await;
            contender.pull(&mut hub).await;
            tag(&mut contender, &format!("tag-{index}")).await;
            contenders.push(contender);
        }
        let (left, right) = contenders.split_at_mut(1);
        left[0].pull(&mut right[0]).await;
        right[0].pull(&mut left[0]).await;

        // Each admin takes over the anchor and removes the other, then keeps writing. Observers
        // already hold what both wrote before.
        let mut shared = Vec::new();
        let mut chains = Vec::new();
        for (contender, (index, other)) in contenders.iter_mut().zip([(20, 21), (21, 20)]) {
            let before = contender.own_ops().await;
            let (record, bytes) = forge_handover(contender, index).await;
            assert_eq!(contender.receive(&[bytes]).await, vec![STORED]);
            contender.apply_all().await.unwrap();
            assert_eq!(view(contender).await.anchor_device_id, device_id(index));
            let removal = proposal(
                contender.ctx.space,
                &device_admin(index),
                contender.now_ms,
                remove_entry(&device_id(other)),
            );
            contender.sequence_signed(&removal).await.unwrap().unwrap();
            tag(contender, &format!("late-{index}")).await;
            let after = contender.own_ops().await;
            chains.push((record, after[before.len()..].to_vec()));
            shared.extend(before);
        }

        // Admin 21 gives way to admin 20's take on its own branch.
        let (winner, loser) = (20, 21);
        let loser_replica = &mut contenders[1];
        let before = loser_replica.own_ops().await;
        let concession = proposal(
            loser_replica.ctx.space,
            &device_admin(loser),
            loser_replica.now_ms,
            Entry::Yield(Yield {
                handover_hash: chains[0].0.hash(),
            }),
        );
        loser_replica
            .sequence_signed(&concession)
            .await
            .unwrap()
            .unwrap();
        let yielded = loser_replica.own_ops().await[before.len()..].to_vec();

        let mut observers = vec![Replica::new(2, 71).await, Replica::new(3, 71).await];
        for (position, observer) in observers.iter_mut().enumerate() {
            observer.pull(&mut hub).await;
            observer.receive(&shared).await;
            observer.apply_all().await.unwrap();
            for offset in 0..chains.len() {
                let (_, ops) = &chains[(position + offset) % chains.len()];
                observer.receive(ops).await;
                observer.apply_all().await.unwrap();
            }
            // Both takes continue the same head, so the old epoch freezes there and holds both
            // admins, each of which a take removes.
            let frozen = view(observer).await;
            assert_eq!(frozen.anchor_device_id, ANCHOR_DEVICE_ID);
            assert!(frozen.frozen_at.is_some());
            for index in [winner, loser] {
                assert!(held(observer, index).await, "device {index} is held");
            }
        }

        for observer in &mut observers {
            assert_eq!(
                observer.receive(&yielded).await,
                vec![STORED; yielded.len()]
            );
            observer.apply_all().await.unwrap();
        }
        let expected = settled(&mut observers[0]).await;
        assert_eq!(settled(&mut observers[1]).await, expected);
        let selected = view(&mut observers[0]).await;
        assert_eq!(selected.anchor_device_id, device_id(winner));
        assert_eq!(selected.frozen_at, None);
        let removed = |index: usize| {
            selected
                .devices
                .iter()
                .find(|device| device.device_id == device_id(index))
                .unwrap()
                .removed_revision
        };
        assert!(removed(loser).is_some());
        assert_eq!(removed(winner), None);
        let unselected = expected.1.iter().filter(|entry| entry.2 == 0).count();
        assert_eq!(
            unselected, 3,
            "the yielded handover, its removal, and the yield"
        );
        for observer in &mut observers {
            assert_eq!(view(observer).await, selected);
            assert_devices_replay_the_selected_chain(observer).await;
            for index in [winner, loser] {
                assert!(!held(observer, index).await, "device {index} is released");
            }
            assert_eq!(
                tags(observer).await,
                vec![
                    format!("late-{winner}"),
                    "tag-20".to_owned(),
                    "tag-21".to_owned()
                ]
            );
            assert_eq!(
                late_changes(observer).await.len(),
                1,
                "the loser's tag after its cutoff"
            );
        }
    });
}

async fn held(replica: &mut Replica, index: usize) -> bool {
    replica
        .engine
        .device_held(&mut replica.conn, &replica.ctx, &device_id(index))
        .await
        .unwrap()
}

/// Devices of a space whose epoch 0 goes on after admin 20 took the anchor over in epoch 1.
pub(super) struct Superseded {
    /// The epoch 0 anchor, which enrolled devices 22 and 23 after the handover point.
    pub(super) hub: Replica,
    /// Admin 20, anchor of epoch 1.
    pub(super) contender: Replica,
    /// Member 1. Its last operation is sealed against epoch 0's last revision, which epoch 1
    /// never reaches.
    pub(super) member: Replica,
    /// Device 22, enrolled only in epoch 0.
    pub(super) dropout: Replica,
}

pub(super) async fn superseded(salt: u64) -> Superseded {
    let mut hub = Replica::anchor(0, salt).await;
    hub.sequence(admin_entry(20)).await.unwrap().unwrap();
    let mut member = Replica::new(1, salt).await;
    member.pull(&mut hub).await;
    tag(&mut member, "tag-1a").await;
    let mut contender = Replica::new(20, salt).await;
    contender.pull(&mut hub).await;
    contender.pull(&mut member).await;
    tag(&mut contender, "tag-20").await;

    for index in [22, 23] {
        hub.sequence(enroll_entry(&person(), index, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
    }
    let mut dropout = Replica::new(22, salt).await;
    dropout.pull(&mut hub).await;
    tag(&mut dropout, "tag-22a").await;
    member.pull(&mut hub).await;
    tag(&mut member, "tag-1b").await;

    let (_, bytes) = forge_handover(&mut contender, 20).await;
    assert_eq!(contender.receive(&[bytes]).await, vec![STORED]);
    contender.apply_all().await.unwrap();
    Superseded {
        hub,
        contender,
        member,
        dropout,
    }
}

/// Authorization revision `bytes` was sealed against.
fn stamp(bytes: &[u8]) -> u64 {
    Envelope::decode(bytes)
        .unwrap()
        .operation()
        .unwrap()
        .header
        .authorization_revision
}

#[test]
fn an_operation_sealed_against_a_superseded_entry_still_applies() {
    block_on(async {
        let mut space = superseded(81).await;
        let mut observer = Replica::new(2, 81).await;
        observer.pull(&mut space.hub).await;
        let ops = space.member.own_ops().await;
        let (seen, last) = ops.split_at(ops.len() - 1);
        observer.receive(seen).await;
        observer.receive(&space.contender.own_ops().await).await;
        observer.apply_all().await.unwrap();
        let selected = view(&mut observer).await;
        assert_eq!(selected.anchor_device_id, device_id(20));

        assert!(stamp(&last[0]) > selected.revision);
        observer.receive(last).await;
        observer.apply_all().await.unwrap();
        assert_eq!(
            observer.applied_vector().await.get(&space.member.writer()),
            u64::try_from(ops.len()).unwrap()
        );
        assert!(tags(&mut observer).await.contains(&"tag-1b".to_owned()));
    });
}

#[test]
fn excluding_an_operation_admitted_against_a_superseded_entry_equals_never_applying_it() {
    block_on(async {
        let mut space = superseded(82).await;
        let removal = proposal(
            space.contender.ctx.space,
            &device_admin(20),
            space.contender.now_ms,
            remove_entry(&device_id(1)),
        );
        space
            .contender
            .sequence_signed(&removal)
            .await
            .unwrap()
            .unwrap();
        let chain = space.contender.own_ops().await;
        let member = space.member.own_ops().await;

        // The first observer applies the member's last operation before it learns of the
        // removal; the second learns of the removal first and never applies it.
        let mut applied = Replica::new(2, 82).await;
        applied.pull(&mut space.hub).await;
        applied.receive(&chain[..chain.len() - 1]).await;
        applied.receive(&member).await;
        applied.apply_all().await.unwrap();
        assert!(tags(&mut applied).await.contains(&"tag-1b".to_owned()));
        applied.receive(&chain).await;
        applied.apply_all().await.unwrap();

        let mut never = Replica::new(3, 82).await;
        never.pull(&mut space.hub).await;
        never.receive(&chain).await;
        never.apply_all().await.unwrap();
        never.receive(&member).await;
        never.apply_all().await.unwrap();

        let expected = settled(&mut never).await;
        assert!(
            expected
                .2
                .iter()
                .any(|op| op.0 == space.member.writer().as_bytes().to_vec()
                    && op.2 == "held"
                    && op.3.is_some()),
            "the member's operation past its cutoff is excluded"
        );
        assert_eq!(settled(&mut applied).await, expected);
        assert!(!tags(&mut never).await.contains(&"tag-1b".to_owned()));
        assert_eq!(tags(&mut applied).await, tags(&mut never).await);
        let listed = late_changes(&mut never).await;
        assert_eq!(listed.len(), 1);
        assert_eq!(late_changes(&mut applied).await, listed);
        for replica in [&mut applied, &mut never] {
            replica.check_invariants().await;
        }
    });
}

#[test]
fn reselecting_a_chain_without_the_removal_reverts_its_exclusions() {
    block_on(async {
        let mut space = superseded(84).await;
        // Epoch 0 goes on to remove the member; the epoch 1 chain forked before the removal.
        space
            .hub
            .sequence(remove_entry(&device_id(1)))
            .await
            .unwrap()
            .unwrap();
        let member = space.member.own_ops().await;
        let chain = space.contender.own_ops().await;
        let member_tags = ["tag-1a".to_owned(), "tag-1b".to_owned()];

        let mut reverted = Replica::new(2, 84).await;
        reverted.pull(&mut space.hub).await;
        reverted.receive(&member).await;
        reverted.apply_all().await.unwrap();
        assert_eq!(
            view(&mut reverted).await.anchor_device_id,
            space.hub.device_id
        );
        let excluded = reverted
            .count("SELECT count(*) FROM sync_ops WHERE excluded_revision IS NOT NULL")
            .await;
        assert_eq!(excluded, i64::try_from(member.len()).unwrap());
        let before = tags(&mut reverted).await;
        assert!(member_tags.iter().all(|id| !before.contains(id)));

        reverted.receive(&chain).await;
        reverted.apply_all().await.unwrap();
        assert_eq!(view(&mut reverted).await.anchor_device_id, device_id(20));
        let after = tags(&mut reverted).await;
        assert!(member_tags.iter().all(|id| after.contains(id)));
        assert!(late_changes(&mut reverted).await.is_empty());

        let mut direct = Replica::new(3, 84).await;
        direct.pull(&mut space.hub).await;
        direct.receive(&chain).await;
        direct.receive(&member).await;
        direct.apply_all().await.unwrap();
        assert_eq!(settled(&mut reverted).await, settled(&mut direct).await);
        assert_eq!(tags(&mut direct).await, after);
        for replica in [&mut reverted, &mut direct] {
            replica.check_invariants().await;
        }
    });
}

#[test]
fn a_device_left_unenrolled_by_reselection_keeps_what_applied_and_its_new_operations_wait() {
    block_on(async {
        let mut space = superseded(83).await;
        let mut observer = Replica::new(2, 83).await;
        observer.pull(&mut space.hub).await;
        observer.pull(&mut space.dropout).await;
        assert!(tags(&mut observer).await.contains(&"tag-22a".to_owned()));

        observer.receive(&space.member.own_ops().await).await;
        observer.receive(&space.contender.own_ops().await).await;
        observer.apply_all().await.unwrap();
        let selected = view(&mut observer).await;
        assert!(
            !selected
                .devices
                .iter()
                .any(|device| device.device_id == device_id(22))
        );
        assert!(tags(&mut observer).await.contains(&"tag-22a".to_owned()));
        let applied = observer.applied_vector().await.get(&space.dropout.writer());

        tag(&mut space.dropout, "tag-22b").await;
        observer.pull(&mut space.dropout).await;
        assert!(!tags(&mut observer).await.contains(&"tag-22b".to_owned()));
        assert_eq!(
            observer.applied_vector().await.get(&space.dropout.writer()),
            applied
        );
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sync_ops WHERE writer_id = ? AND state = 'waiting'",
        )
        .bind(space.dropout.writer().as_bytes().as_slice())
        .fetch_one(&mut observer.conn)
        .await
        .unwrap();
        assert_eq!(waiting, 1);
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
fn an_entry_applied_before_the_local_writer_seals_its_genesis_does_not_freeze_it() {
    block_on(async {
        let mut hub = Replica::anchor(0, 58).await;
        let mut member = Replica::new(1, 58).await;
        tag(&mut hub, "tag-1").await;
        member.pull(&mut hub).await;
        assert_eq!(member.stored_vector().await.get(&member.writer()), 0);

        // The member renames the tag without sealing, then receives an entry and a rename of
        // the same tag, which waits for the member's capture to seal first.
        member
            .exec("UPDATE quick_note_tags SET name = 'Member' WHERE id = 'tag-1'")
            .await;
        hub.sequence(enroll_entry(&person(), 20, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        hub.exec("UPDATE quick_note_tags SET name = 'Hub' WHERE id = 'tag-1'")
            .await;
        hub.seal().await.unwrap();
        let known = member.stored_vector().await;
        let ops = hub.ops_since(&known).await;
        member.receive(&ops).await;
        let report = member.apply_once().await.unwrap();
        assert!(!report.local_writer_frozen);
        assert!(!report.pending_captures);
        assert_eq!(
            member.applied_vector().await,
            member.stored_vector().await,
            "the member seals its genesis and capture, then applies both operations"
        );
        assert_eq!(member.stored_vector().await.get(&member.writer()), 2);
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

        // Only desktops and servers can be admins.
        hub.sequence(Entry::Enroll(Enroll {
            device_id: device_id(23),
            device_key: device_key(&person(), 23).public_key(),
            tls_fingerprint: tls_fingerprint(23),
            label: "Server".to_owned(),
            device_kind: DeviceKind::Server,
            role: DeviceRole::Admin(device_admin(23).public_key()),
        }))
        .await
        .unwrap()
        .unwrap();
        let mobile = |role: DeviceRole| {
            Entry::Enroll(Enroll {
                device_id: device_id(22),
                device_key: device_key(&person(), 22).public_key(),
                tls_fingerprint: tls_fingerprint(22),
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

#[test]
fn a_chain_never_orders_one_proposal_twice() {
    block_on(async {
        let mut hub = Replica::anchor(0, 64).await;
        let mut observer = Replica::new(2, 64).await;
        let space = hub.ctx.space;
        let set_recovery = |seed: u8| {
            Entry::SetRole(SetRole::Recovery {
                recovery_admin_key: AdminKeyPair::from_seed(&[seed; 32]).unwrap().public_key(),
            })
        };
        let leaked = proposal(
            space,
            &anchor_admin(&person()),
            hub.now_ms,
            set_recovery(90),
        );
        hub.sequence_signed(&leaked).await.unwrap().unwrap();
        hub.sequence(set_recovery(91)).await.unwrap().unwrap();
        let before = view(&mut hub).await;

        // An anchor that carries the old proposal again cannot reinstate the replaced key.
        let record = next_record(&mut hub, ANCHOR_DEVICE_ID, leaked, Sequencing::Empty).await;
        let replayed = forge(
            &mut hub,
            Content::Authorization(Box::new(record)),
            before.revision,
        )
        .await;
        assert_eq!(hub.receive(&[replayed]).await, vec![STORED]);
        hub.apply_all().await.unwrap();
        observer.pull(&mut hub).await;
        for replica in [&mut hub, &mut observer] {
            let after = view(replica).await;
            assert_eq!(after.revision, before.revision);
            assert_eq!(
                after.recovery_admin_key,
                AdminKeyPair::from_seed(&[91; 32]).unwrap().public_key()
            );
        }
    });
}

#[test]
fn a_certificate_enrolls_one_device_for_the_life_of_the_log() {
    block_on(async {
        let mut hub = Replica::anchor(0, 65).await;
        hub.sequence(enroll_entry(&person(), 20, DeviceRole::Member))
            .await
            .unwrap()
            .unwrap();
        hub.remove(&device_id(20)).await.unwrap();
        let decoy = |index: usize, fingerprint: [u8; 32]| {
            Entry::Enroll(Enroll {
                device_id: device_id(index),
                device_key: device_key(&person(), index).public_key(),
                tls_fingerprint: fingerprint,
                label: "Decoy".to_owned(),
                device_kind: DeviceKind::Desktop,
                role: DeviceRole::Member,
            })
        };
        let anchor_fingerprint = hub
            .engine
            .charter(&mut hub.conn, &hub.ctx)
            .await
            .unwrap()
            .unwrap()
            .charter
            .anchor_tls_fingerprint;
        for fingerprint in [tls_fingerprint(20), anchor_fingerprint] {
            assert_eq!(
                hub.sequence(decoy(21, fingerprint)).await.unwrap(),
                Err(ProposalRejection::AlreadyEnrolled)
            );
        }
        hub.sequence(decoy(21, tls_fingerprint(21)))
            .await
            .unwrap()
            .unwrap();
    });
}

#[test]
fn a_proposal_has_a_revision_once_the_selected_chain_holds_it() {
    block_on(async {
        let mut hub = Replica::anchor(0, 62).await;
        let mut member = Replica::new(1, 62).await;
        let signed = proposal(
            hub.ctx.space,
            &anchor_admin(&person()),
            hub.now_ms,
            enroll_entry(&person(), 24, DeviceRole::Member),
        );
        let hash = signed.hash();
        let revision_of = async |replica: &mut Replica| {
            replica
                .engine
                .proposal_revision(&mut replica.conn, &replica.ctx, &hash)
                .await
                .unwrap()
        };
        assert_eq!(revision_of(&mut hub).await, None);
        let revision = hub.sequence_signed(&signed).await.unwrap().unwrap();
        assert_eq!(revision_of(&mut hub).await, Some(revision));
        assert_eq!(revision_of(&mut member).await, None);
        member.pull(&mut hub).await;
        member.apply_all().await.unwrap();
        assert_eq!(revision_of(&mut member).await, Some(revision));
    });
}

#[test]
fn a_removal_cutoff_covers_what_the_coordinator_reported_beyond_the_anchor() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 61).await;
        let mut member = Replica::new(1, 61).await;
        tag(&mut member, "tag-first").await;
        tag(&mut member, "tag-second").await;
        let ops = member.own_ops().await;
        assert!(
            ops.len() > 2,
            "the member wrote past what the anchor stores"
        );
        // The anchor stores only part of the member's chain, while the coordinator reported
        // storing all of it.
        anchor.receive(&ops[..2]).await;
        let reported = u64::try_from(ops.len()).unwrap();
        let mut covered = ganbaru_sync_contracts::VersionVector::new();
        covered.set(member.writer(), reported);
        let signed = super::replica::person_proposal(
            &anchor.person,
            anchor.ctx.space,
            &anchor_admin(&anchor.person),
            anchor.now_ms,
            remove_entry(&member.device_id),
        );
        anchor
            .engine
            .sequence_proposal_covering(
                &mut anchor.conn,
                &anchor.ctx,
                &signed,
                [6; 32],
                &covered,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap()
            .unwrap();
        let cutoff: Option<i64> =
            sqlx::query_scalar("SELECT cutoff_seq FROM sync_writers WHERE device_id = ?")
                .bind(&member.device_id)
                .fetch_one(&mut anchor.conn)
                .await
                .unwrap();
        assert_eq!(cutoff, Some(i64::try_from(reported).unwrap()));
    });
}

#[test]
fn a_reported_covered_vector_raises_a_cutoff_only_a_bounded_distance_past_the_anchor() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 63).await;
        let mut member = Replica::new(1, 63).await;
        tag(&mut member, "tag-first").await;
        let ops = member.own_ops().await;
        anchor.receive(&ops).await;
        let stored = u64::try_from(ops.len()).unwrap();
        let mut covered = ganbaru_sync_contracts::VersionVector::new();
        covered.set(member.writer(), u64::MAX);
        let signed = super::replica::person_proposal(
            &anchor.person,
            anchor.ctx.space,
            &anchor_admin(&anchor.person),
            anchor.now_ms,
            remove_entry(&member.device_id),
        );
        anchor
            .engine
            .sequence_proposal_covering(
                &mut anchor.conn,
                &anchor.ctx,
                &signed,
                [6; 32],
                &covered,
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap()
            .unwrap();
        let cutoff: Option<i64> =
            sqlx::query_scalar("SELECT cutoff_seq FROM sync_writers WHERE device_id = ?")
                .bind(&member.device_id)
                .fetch_one(&mut anchor.conn)
                .await
                .unwrap();
        assert_eq!(
            cutoff,
            Some(i64::try_from(stored + crate::authorization::MAX_COVERED_AHEAD).unwrap())
        );
    });
}

#[test]
fn a_removal_rotates_the_discovery_key_and_keeps_the_one_it_replaced() {
    block_on(async {
        let mut anchor = Replica::anchor(0, 62).await;
        let member = Replica::new(1, 62).await;
        let founded = view(&mut anchor).await;
        assert_eq!(founded.previous_discovery_key, None);
        let signed = super::replica::person_proposal(
            &anchor.person,
            anchor.ctx.space,
            &anchor_admin(&anchor.person),
            anchor.now_ms,
            remove_entry(&member.device_id),
        );
        let rotated = [6; 32];
        assert_ne!(founded.discovery_key, rotated);
        anchor
            .engine
            .sequence_proposal_covering(
                &mut anchor.conn,
                &anchor.ctx,
                &signed,
                rotated,
                &ganbaru_sync_contracts::VersionVector::new(),
                &mut anchor.identity.local(),
                anchor.now_ms,
            )
            .await
            .unwrap()
            .unwrap();
        let removed = view(&mut anchor).await;
        assert_eq!(removed.discovery_key, rotated);
        assert_eq!(removed.previous_discovery_key, Some(founded.discovery_key));
    });
}

/// Identity of writer `writer` of replica device `index`, sealed by whoever holds it.
fn phantom_identity(holder: &Replica, writer: usize, index: usize) -> Identity {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&u64::try_from(writer).unwrap().to_le_bytes());
    seed[31] = 3;
    Identity::new(
        seed,
        &device_key(&holder.person, index),
        &device_id(index),
        None,
    )
}

#[test]
fn a_vetoed_branch_enrolls_writers_only_within_the_unselected_bounds() {
    block_on(async {
        let salt = 331;
        let mut hub = Replica::anchor(0, salt).await;
        hub.sequence(admin_entry(20)).await.unwrap().unwrap();
        let mut stale = Replica::new(20, salt).await;
        stale.pull(&mut hub).await;
        tag(&mut stale, "tag-20").await;
        let mut member = Replica::new(1, salt).await;
        member.pull(&mut hub).await;

        // The anchor removes the admin, which takes over from before its removal and enrolls
        // phantom devices on its vetoed branch.
        hub.remove(&device_id(20)).await.unwrap();
        let (_, handover) = forge_handover(&mut stale, 20).await;
        assert_eq!(stale.receive(&[handover]).await, vec![STORED]);
        stale.apply_all().await.unwrap();
        assert_eq!(view(&mut stale).await.anchor_device_id, device_id(20));
        let phantoms = MAX_UNSELECTED_WRITERS_PER_SPACE / MAX_WRITERS_PER_DEVICE + 1;
        let device = |writer: usize| 30 + writer / MAX_WRITERS_PER_DEVICE;
        for index in 30..30 + phantoms {
            let signed = proposal(
                stale.ctx.space,
                &device_admin(20),
                stale.now_ms,
                enroll_entry(&person(), index, DeviceRole::Member),
            );
            stale.sequence_signed(&signed).await.unwrap().unwrap();
        }
        let branch = stale.own_ops().await;
        let enrolled = stale.stored_vector().await;
        stale.identity = phantom_identity(&stale, 0, device(0));
        edit(&mut stale, &note("note-phantom", "Phantom", 0)).await;
        for index in 1..MAX_EXCLUDED_PER_WRITER {
            edit(
                &mut stale,
                &format!(
                    "UPDATE quick_notes SET title = 'Phantom {index}' WHERE id = 'note-phantom'"
                ),
            )
            .await;
        }
        let first = stale.ops_since(&enrolled).await;
        assert_eq!(first.len() as u64, MAX_EXCLUDED_PER_WRITER + 1);
        let sealed = stale.stored_vector().await;
        for writer in 1..=MAX_UNSELECTED_WRITERS_PER_SPACE {
            stale.identity = phantom_identity(&stale, writer, device(writer));
            stale.seal().await.unwrap();
        }
        let geneses = stale.ops_since(&sealed).await;
        assert_eq!(geneses.len(), MAX_UNSELECTED_WRITERS_PER_SPACE);

        let mut observer = Replica::new(2, salt).await;
        observer.pull(&mut hub).await;
        observer.receive(&branch).await;
        observer.apply_all().await.unwrap();
        assert_eq!(view(&mut observer).await.anchor_device_id, ANCHOR_DEVICE_ID);

        // The phantom writer keeps the excluded bound, and phantom writers fill only the
        // unselected writers of the space.
        let outcomes = observer.receive(&first).await;
        assert!(
            outcomes[..first.len() - 1]
                .iter()
                .all(|outcome| matches!(outcome, StoreOutcome::Stored { .. }))
        );
        assert_eq!(
            outcomes.last(),
            Some(&StoreOutcome::Refused(StoreRefusal::Revoked))
        );
        let outcomes = observer.receive(&geneses).await;
        assert!(
            outcomes[..geneses.len() - 1]
                .iter()
                .all(|outcome| matches!(outcome, StoreOutcome::Stored { .. }))
        );
        assert_eq!(
            outcomes.last(),
            Some(&StoreOutcome::Refused(StoreRefusal::WriterLimit))
        );
        let unselected: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sync_writers w WHERE NOT EXISTS (
                SELECT 1 FROM sync_devices d WHERE d.device_id = w.device_id
             )",
        )
        .fetch_one(&mut observer.conn)
        .await
        .unwrap();
        assert_eq!(
            usize::try_from(unselected).unwrap(),
            MAX_UNSELECTED_WRITERS_PER_SPACE
        );

        // A writer of a device the selected chain enrolls is still admitted.
        tag(&mut member, "tag-1").await;
        let honest = member.own_ops().await;
        assert!(matches!(
            observer.receive(&honest).await.first(),
            Some(StoreOutcome::Stored { .. })
        ));
        observer.apply_all().await.unwrap();
        assert!(tags(&mut observer).await.contains(&"tag-1".to_owned()));
        observer.check_invariants().await;
    });
}

#[test]
fn a_writer_keeps_the_unselected_bound_when_the_selection_enrolls_its_device_under_another_key() {
    block_on(async {
        let salt = 332;
        let mut hub = Replica::anchor(0, salt).await;
        hub.sequence(admin_entry(20)).await.unwrap().unwrap();
        let mut stale = Replica::new(20, salt).await;
        stale.pull(&mut hub).await;
        tag(&mut stale, "tag-20").await;

        // The removed admin enrolls device 30 on its vetoed branch, whose writer seals one
        // operation past the bound.
        hub.remove(&device_id(20)).await.unwrap();
        let (_, handover) = forge_handover(&mut stale, 20).await;
        assert_eq!(stale.receive(&[handover]).await, vec![STORED]);
        stale.apply_all().await.unwrap();
        let signed = proposal(
            stale.ctx.space,
            &device_admin(20),
            stale.now_ms,
            enroll_entry(&person(), 30, DeviceRole::Member),
        );
        stale.sequence_signed(&signed).await.unwrap().unwrap();
        let branch = stale.own_ops().await;
        let enrolled = stale.stored_vector().await;
        stale.identity = phantom_identity(&stale, 0, 30);
        edit(&mut stale, &note("note-phantom", "Phantom", 0)).await;
        for index in 1..MAX_EXCLUDED_PER_WRITER {
            edit(
                &mut stale,
                &format!(
                    "UPDATE quick_notes SET title = 'Phantom {index}' WHERE id = 'note-phantom'"
                ),
            )
            .await;
        }
        let first = stale.ops_since(&enrolled).await;
        assert_eq!(first.len() as u64, MAX_EXCLUDED_PER_WRITER + 1);

        let mut observer = Replica::new(2, salt).await;
        observer.pull(&mut hub).await;
        observer.receive(&branch).await;
        observer.apply_all().await.unwrap();
        assert!(matches!(
            observer.receive(&first[..1]).await.first(),
            Some(StoreOutcome::Stored { .. })
        ));

        // The anchor then enrolls the same device id with another key, which does not lift the
        // bound of the writer the vetoed branch enrolled.
        hub.sequence(Entry::Enroll(Enroll {
            device_id: device_id(30),
            device_key: device_key(&person(), 99).public_key(),
            tls_fingerprint: tls_fingerprint(99),
            label: "Device 30".to_owned(),
            device_kind: DeviceKind::Desktop,
            role: DeviceRole::Member,
        }))
        .await
        .unwrap()
        .unwrap();
        observer.pull(&mut hub).await;
        observer.apply_all().await.unwrap();
        let outcomes = observer.receive(&first[1..]).await;
        assert!(
            outcomes[..outcomes.len() - 1]
                .iter()
                .all(|outcome| matches!(outcome, StoreOutcome::Stored { .. }))
        );
        assert_eq!(
            outcomes.last(),
            Some(&StoreOutcome::Refused(StoreRefusal::Revoked))
        );
        observer.check_invariants().await;
    });
}

#[test]
fn tour_spans_nest_exactly_along_ancestry() {
    // Siblings take consecutive indexes when discovered, so index order is not tour order.
    let children = vec![
        vec![1, 2],
        vec![3],
        vec![4, 5],
        vec![6],
        vec![],
        vec![],
        vec![],
    ];
    let parents = [None, Some(0), Some(0), Some(1), Some(2), Some(2), Some(3)];
    let spans = tour_spans(&children);
    for (ancestor, outer) in spans.iter().enumerate() {
        for (descendant, inner) in spans.iter().enumerate() {
            let mut walked = Some(descendant);
            while walked.is_some_and(|at| at != ancestor) {
                walked = walked.and_then(|at| parents[at]);
            }
            let nested = outer.0 <= inner.0 && inner.1 <= outer.1;
            assert_eq!(nested, walked.is_some(), "{ancestor} over {descendant}");
        }
    }
}

#[test]
fn tour_spans_cover_a_chain_too_deep_to_recurse() {
    let depth = 200_000;
    let mut children: Vec<Vec<usize>> = (1..depth).map(|child| vec![child]).collect();
    children.push(Vec::new());
    let spans = tour_spans(&children);
    assert!(
        spans
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0 && pair[1].1 < pair[0].1)
    );
}
