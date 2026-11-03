//! What the hub answers a device that an anchor handover contest holds for data: its own
//! writers in full, and nothing about the writers of other devices.

use super::{
    DirectTransport, Exchange, ForkedVault, HUB_DEVICE, PHONE_DEVICE, Replica, TABLET_DEVICE,
    block_on, contact_source, done, hub, member_hub, open_hub, phone_ids, tablet_ids, test_contact,
};
use crate::client::SyncTransport;
use crate::hub::{BOOTSTRAP_REQUIRED, HubStability, SyncHub};
use crate::tests::{VAULT_ID, acquire};
use crate::wire::{forked_from_wire, hash_to_wire, ops_to_wire, vector_from_wire, vector_to_wire};
use ganbaru_handoff::protocol::{
    ControlMessage, PROTOCOL_VERSION, SYNC_PAGE_BYTES, SyncProbe, SyncRefusal, SyncRefusalCode,
};
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

#[test]
fn held_device_records_no_anchor_marks() {
    block_on(async {
        let hub_replica = Replica::new("hub-held-marks", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-held-marks", PHONE_DEVICE).await;
        let tablet = Replica::new("tablet-held-marks", TABLET_DEVICE).await;
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

        // The tablet takes later phone operations from the phone directly, past what the
        // anchor stores.
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
        hold(&hub_replica, TABLET_DEVICE).await;
        hold(&tablet, TABLET_DEVICE).await;

        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        let echoed = done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(hub_replica.stored().await.get(&phone_writer), reached);
        assert_eq!(
            echoed.hub.get(&phone_writer),
            phone_head,
            "the anchor echoes the held tablet's own claim"
        );

        let mut conn = acquire(&tablet.pool).await;
        let view = tablet
            .engine
            .authorization(&mut conn, &tablet.ctx)
            .await
            .expect("read authorization")
            .expect("authorization view");
        assert_eq!(view.anchor_device_id, HUB_DEVICE);
        assert!(
            !crate::client::record_anchor_marks(
                &tablet.engine,
                &mut conn,
                &tablet.ctx,
                TABLET_DEVICE,
                HUB_DEVICE,
                &echoed.hub,
            )
            .await
            .expect("record anchor marks"),
            "a held device records no anchor marks from an echo"
        );
        let marked: Option<i64> =
            sqlx::query_scalar("SELECT seq FROM sync_anchor_marks WHERE writer_id = ?")
                .bind(phone_writer.as_bytes().as_slice())
                .fetch_optional(&mut *conn)
                .await
                .expect("read anchor marks");
        assert_eq!(marked, None);
        drop(conn);
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

fn push(device_id: &str, ops: &[Vec<u8>]) -> ControlMessage {
    ControlMessage::SyncPush {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT_ID.to_string(),
        device_id: device_id.to_string(),
        ops: ops_to_wire(ops),
    }
}

fn push_result(response: ControlMessage) -> (VersionVector, Option<SyncRefusal>) {
    match response {
        ControlMessage::SyncPushResult { stored, refusal } => {
            (vector_from_wire(&stored).expect("stored vector"), refusal)
        }
        other => panic!("expected a push result, got {other:?}"),
    }
}

fn pull(device_id: &str, known: &VersionVector) -> ControlMessage {
    ControlMessage::SyncPull {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT_ID.to_string(),
        device_id: device_id.to_string(),
        known: vector_to_wire(known),
        max_bytes: u32::try_from(SYNC_PAGE_BYTES).expect("page size fits"),
    }
}

#[test]
fn held_device_push_refusal_hides_foreign_progress() {
    block_on(async {
        let hub_replica = Replica::new("hub-held-gap", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-held-gap", PHONE_DEVICE).await;
        let tablet = Replica::new("tablet-held-gap", TABLET_DEVICE).await;
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
        phone.seal_note("note-2").await;
        let phone_head = phone.seal_note("note-3").await;
        done(
            phone
                .exchange(&member_hub(&tablet), &phone_ids())
                .await
                .expect("exchange with the tablet"),
        );
        let phone_writer = phone.writer.id();
        let past_gap = Engine::vault()
            .operation_at(
                &mut *acquire(&tablet.pool).await,
                &tablet.ctx,
                phone_writer,
                phone_head,
            )
            .await
            .expect("read operation")
            .expect("phone operation");
        assert!(phone_head > reached + 1);
        hold(&hub_replica, TABLET_DEVICE).await;

        // A crafted push past the hub's stored sequence reads back the page alone.
        let (stored, refusal) = push_result(
            transport
                .request(push(TABLET_DEVICE, std::slice::from_ref(&past_gap)))
                .await
                .expect("push"),
        );
        assert_eq!(refusal, None, "no refusal names the hub's expected sequence");
        assert_eq!(stored.get(&phone_writer), phone_head);
        assert_eq!(hub_replica.stored().await.get(&phone_writer), reached);

        // The phone that owns the writer is not held and is told where the gap starts.
        let (_, refusal) = push_result(
            transport
                .request(push(PHONE_DEVICE, &[past_gap]))
                .await
                .expect("push"),
        );
        assert_eq!(
            refusal,
            Some(SyncRefusal {
                code: SyncRefusalCode::Gap,
                writer: Some(phone_writer.to_hex()),
                seq: Some(reached + 1),
            })
        );
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn held_device_push_of_a_foreign_fork_reads_back_the_echo() {
    block_on(async {
        let vault = ForkedVault::new("held-fork-push").await;
        hold(&vault.hub, TABLET_DEVICE).await;
        let (stored, refusal) = push_result(
            vault
                .transport
                .request(push(TABLET_DEVICE, &vault.pair[1..]))
                .await
                .expect("push"),
        );
        assert_eq!(refusal, None, "no fork refusal names the hub's sequence");
        assert_eq!(stored.get(&vault.writer), vault.fork_seq);
        vault.close().await;
    });
}

#[test]
fn held_device_pull_hides_the_compacted_vector() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-held-compacted", HUB_DEVICE).await;
        let phone = Replica::new("phone-held-compacted", PHONE_DEVICE).await;
        let tablet = Replica::new("tablet-held-compacted", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        hub_replica.seal_note("note-1").await;
        hub_replica.seal_note("note-2").await;
        hub_replica.seal_note("note-3").await;
        assert!(hub_replica.compact().await > 0);
        let compacted = Engine::vault()
            .compacted_vector(&mut *acquire(&hub_replica.pool).await, &hub_replica.ctx)
            .await
            .expect("compacted vector");
        let (hub_writer, through) = compacted.iter().next().expect("a compacted writer");
        assert!(through > 1);
        hold(&hub_replica, TABLET_DEVICE).await;

        // Every claim at or below the compacted sequence gets the same answer, so varying it
        // reveals nothing about where compaction stands.
        let mut answers = Vec::new();
        for seq in 0..=through {
            let known: VersionVector = [(hub_writer, seq)].into_iter().collect();
            match transport
                .request(pull(TABLET_DEVICE, &known))
                .await
                .expect("pull")
            {
                ControlMessage::SyncOps { ops, more } => answers.push((ops, more)),
                other => panic!("a held pull from {seq} answered {other:?}"),
            }
        }
        assert!(answers.windows(2).all(|pair| pair[0] == pair[1]));

        // The phone is not held and still learns that it needs a fresh copy.
        match transport
            .request(pull(PHONE_DEVICE, &VersionVector::new()))
            .await
            .expect("pull")
        {
            ControlMessage::Error { code, .. } => assert_eq!(code, BOOTSTRAP_REQUIRED),
            other => panic!("expected a bootstrap refusal, got {other:?}"),
        }
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}
