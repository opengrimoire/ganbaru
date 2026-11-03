//! What the hub answers a device that an anchor handover contest holds for data: its own
//! writers in full, and nothing about the writers of other devices.

use super::{
    DirectTransport, Exchange, ForkedVault, HUB_DEVICE, PHONE_DEVICE, Replica, TABLET_DEVICE,
    block_on, contact_source, done, hub, member_hub, open_hub, phone_ids, tablet_ids, test_contact,
};
use crate::client::SyncTransport;
use crate::hub::{HubStability, SyncHub};
use crate::tests::{VAULT_ID, acquire};
use crate::wire::{forked_from_wire, hash_to_wire, ops_to_wire, vector_from_wire, vector_to_wire};
use ganbaru_handoff::protocol::{ControlMessage, PROTOCOL_VERSION, SyncProbe};
use ganbaru_sync::Engine;
use ganbaru_sync_contracts::{VersionVector, WriterId};
use tokio::sync::watch;

/// Marks `device_id` held for data in the selection `replica` stores, as a frozen contest
/// over an anchor handover would.
async fn hold(replica: &Replica, device_id: &str) {
    let updated = sqlx::query("UPDATE sync_devices SET held = 1 WHERE device_id = ?")
        .bind(device_id)
        .execute(&replica.pool)
        .await
        .expect("hold device")
        .rows_affected();
    assert_eq!(updated, 1, "{device_id} is enrolled");
}

/// The answer parts of a `SyncState` a held device could read metadata from.
#[derive(Debug)]
struct State {
    stored: VersionVector,
    probe_hash: Option<String>,
    stable: VersionVector,
    bootstrap_required: bool,
    forked: Vec<(WriterId, u64)>,
}

fn state(response: ControlMessage) -> State {
    match response {
        ControlMessage::SyncState {
            stored,
            probe_hash,
            stable,
            bootstrap_required,
            forked,
            ..
        } => State {
            stored: vector_from_wire(&stored).expect("stored vector"),
            probe_hash,
            stable: vector_from_wire(&stable).expect("stable vector"),
            bootstrap_required,
            forked: forked_from_wire(&forked).expect("forked writers"),
        },
        other => panic!("expected a sync state, got {other:?}"),
    }
}

fn hello(
    device_id: &str,
    stored: &VersionVector,
    probe: Option<(WriterId, u64)>,
) -> ControlMessage {
    ControlMessage::SyncHello {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT_ID.to_string(),
        device_id: device_id.to_string(),
        manifest_version: Engine::vault().manifest().version,
        stored: vector_to_wire(stored),
        probe: probe.map(|(writer, seq)| SyncProbe {
            writer: writer.to_hex(),
            seq,
            hash: hash_to_wire(&[0; 32]),
        }),
        listening: Vec::new(),
    }
}

fn hashes(device_id: &str, writer: WriterId) -> ControlMessage {
    ControlMessage::SyncHashes {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT_ID.to_string(),
        device_id: device_id.to_string(),
        writer: writer.to_hex(),
        from_seq: 1,
        limit: 16,
    }
}

fn hash_list(response: ControlMessage) -> Vec<String> {
    match response {
        ControlMessage::SyncHashList { hashes } => hashes,
        other => panic!("expected a hash list, got {other:?}"),
    }
}

fn evidence_request(device_id: &str, writer: WriterId, evidence: &[Vec<u8>]) -> ControlMessage {
    ControlMessage::SyncForkEvidence {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT_ID.to_string(),
        device_id: device_id.to_string(),
        writer: writer.to_hex(),
        evidence: ops_to_wire(evidence),
    }
}

fn fork_pair(response: ControlMessage) -> Vec<String> {
    match response {
        ControlMessage::SyncForkPair { evidence } => evidence,
        other => panic!("expected a fork pair, got {other:?}"),
    }
}

#[test]
fn held_device_sees_only_its_own_writers() {
    block_on(async {
        let vault = ForkedVault::new("held-metadata").await;
        assert_eq!(
            vault.offer(&vault.hub).await,
            ganbaru_sync::EvidenceOutcome::Stored
        );
        let real = vault.hub.stored().await;
        let authority_writer = real
            .iter()
            .map(|(writer, _)| writer)
            .find(|writer| *writer != vault.writer)
            .expect("authority writer");
        assert!(real.get(&vault.writer) > 1);
        let stability = HubStability::default();
        stability.publish(VAULT_ID, real.clone());
        let transport = DirectTransport(
            SyncHub::new(open_hub(&vault.hub), watch::Sender::new(0))
                .with_contact(contact_source(test_contact()))
                .with_stability(stability),
        );
        hold(&vault.hub, TABLET_DEVICE).await;

        // A wait from nothing returns at once, since the authorization chain is servable.
        let wait = ControlMessage::SyncWait {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: TABLET_DEVICE.to_string(),
            known: Vec::new(),
        };
        let waited = state(transport.request(wait).await.expect("wait"));
        assert_eq!(
            waited.stored,
            VersionVector::new(),
            "a wait echoes the claim"
        );
        assert!(waited.stable.is_empty());
        assert!(waited.forked.is_empty());

        assert!(vault.hub.compact().await > 0);
        let claim: VersionVector = [(vault.writer, 1)].into_iter().collect();
        let answer = state(
            transport
                .request(hello(TABLET_DEVICE, &claim, Some((vault.writer, 1))))
                .await
                .expect("hello"),
        );
        assert_eq!(
            answer.stored, claim,
            "foreign writers read back the claim, not the hub's vector"
        );
        assert_eq!(answer.stored.get(&authority_writer), 0);
        assert!(answer.stable.is_empty(), "no stability of foreign writers");
        assert!(
            !answer.bootstrap_required,
            "no copy is offered to a held device"
        );
        assert!(answer.forked.is_empty(), "no foreign fork is reported");
        assert_eq!(answer.probe_hash, None, "no hash of a foreign operation");

        // The same hub still tells a device that is not held the truth.
        let free = state(
            transport
                .request(hello(PHONE_DEVICE, &claim, None))
                .await
                .expect("hello"),
        );
        assert_eq!(free.stored, real);
        assert!(free.bootstrap_required);
        assert_eq!(free.forked, vec![(vault.writer, vault.fork_seq)]);
        vault.close().await;
    });
}

#[test]
fn held_device_cannot_read_foreign_hashes_or_evidence() {
    block_on(async {
        let vault = ForkedVault::new("held-evidence").await;
        hold(&vault.hub, TABLET_DEVICE).await;

        // The held tablet offers the pair: the hub keeps it but answers nothing back.
        let offered = fork_pair(
            vault
                .transport
                .request(evidence_request(TABLET_DEVICE, vault.writer, &vault.pair))
                .await
                .expect("offer evidence"),
        );
        assert!(offered.is_empty(), "the answer to an offer holds no pair");
        assert_eq!(
            vault.forked(&vault.hub).await,
            vec![(vault.writer, vault.fork_seq)],
            "offered evidence is still stored"
        );
        hold(&vault.hub, TABLET_DEVICE).await;

        let asked = fork_pair(
            vault
                .transport
                .request(evidence_request(TABLET_DEVICE, vault.writer, &[]))
                .await
                .expect("ask evidence"),
        );
        assert!(asked.is_empty(), "no evidence body of a foreign writer");
        let listed = hash_list(
            vault
                .transport
                .request(hashes(TABLET_DEVICE, vault.writer))
                .await
                .expect("hashes"),
        );
        assert!(listed.is_empty(), "no hash of a foreign writer");

        // The phone that owns the writer is not held and reads both.
        assert_eq!(
            fork_pair(
                vault
                    .transport
                    .request(evidence_request(PHONE_DEVICE, vault.writer, &[]))
                    .await
                    .expect("ask evidence"),
            )
            .len(),
            2
        );
        vault.close().await;
    });
}

#[test]
fn held_device_recovers_its_own_fork() {
    block_on(async {
        let vault = ForkedVault::new("held-own-fork").await;
        assert_eq!(
            vault.offer(&vault.hub).await,
            ganbaru_sync::EvidenceOutcome::Stored
        );
        hold(&vault.hub, PHONE_DEVICE).await;

        let stored = vault.phone.stored().await;
        let answer = state(
            vault
                .transport
                .request(hello(PHONE_DEVICE, &stored, None))
                .await
                .expect("hello"),
        );
        assert_eq!(
            answer.stored.get(&vault.writer),
            vault.hub.stored().await.get(&vault.writer),
            "an own writer reads the hub's value"
        );
        assert_eq!(answer.forked, vec![(vault.writer, vault.fork_seq)]);
        assert!(
            !hash_list(
                vault
                    .transport
                    .request(hashes(PHONE_DEVICE, vault.writer))
                    .await
                    .expect("hashes"),
            )
            .is_empty()
        );
        assert_eq!(
            fork_pair(
                vault
                    .transport
                    .request(evidence_request(PHONE_DEVICE, vault.writer, &[]))
                    .await
                    .expect("ask evidence"),
            )
            .len(),
            2
        );

        let fork = match vault
            .phone
            .exchange(&vault.transport, &phone_ids())
            .await
            .expect("exchange")
        {
            Exchange::Fork(fork) => fork,
            Exchange::Done(done) => panic!("the held device missed its own fork: {done:?}"),
        };
        assert_eq!(fork.writer, vault.writer);
        assert_eq!(fork.keep_through, vault.fork_seq - 1);
        vault.close().await;
    });
}

#[test]
fn held_device_does_not_resend_foreign_operations() {
    block_on(async {
        let hub_replica = Replica::new("hub-held-push", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-held-push", PHONE_DEVICE).await;
        let mut tablet = Replica::new("tablet-held-push", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let reached = phone.seal_note("note-1").await;
        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );

        // The tablet takes later phone operations from the phone directly.
        phone.seal_note("note-2").await;
        let phone_head = phone.seal_note("note-3").await;
        done(
            phone
                .exchange(&member_hub(&tablet), &phone_ids())
                .await
                .expect("exchange with the tablet"),
        );
        let phone_writer = phone.writer.id();
        assert_eq!(tablet.stored().await.get(&phone_writer), phone_head);
        let own = tablet.seal_note("note-tablet").await;
        hold(&hub_replica, TABLET_DEVICE).await;

        let first = done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(first.problem, None);
        assert_eq!(
            first.pushed,
            usize::try_from(own).expect("count fits"),
            "a held device pushes its own operations only"
        );
        assert_eq!(hub_replica.stored().await.get(&phone_writer), reached);
        assert_eq!(hub_replica.stored().await.get(&tablet.writer.id()), own);
        let again = done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(again.pushed, 0, "the next pass sends nothing");

        // A page with a foreign operation reads back the page, not the hub's vector.
        let next = Engine::vault()
            .operation_at(
                &mut *acquire(&tablet.pool).await,
                &tablet.ctx,
                phone_writer,
                reached + 1,
            )
            .await
            .expect("read operation")
            .expect("phone operation");
        let push = ControlMessage::SyncPush {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: TABLET_DEVICE.to_string(),
            ops: ops_to_wire(&[next]),
        };
        let stored = match transport.request(push).await.expect("push") {
            ControlMessage::SyncPushResult { stored, refusal } => {
                assert_eq!(refusal, None);
                vector_from_wire(&stored).expect("stored vector")
            }
            other => panic!("expected a push result, got {other:?}"),
        };
        let expected: VersionVector = [(phone_writer, reached + 1), (tablet.writer.id(), own)]
            .into_iter()
            .collect();
        assert_eq!(stored, expected);
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}
