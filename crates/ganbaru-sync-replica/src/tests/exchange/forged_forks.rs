//! A hub that claims a fork of this device's writer and shows no signed operation for it: the
//! device keeps its writer and reports the claim as unproven.

use super::{
    DirectTransport, Exchange, HUB_DEVICE, PHONE_DEVICE, Replica, block_on, done, hub, phone_ids,
};
use crate::client::{ClientSession, MAX_EXCHANGE_BYTES, SyncTransport};
use crate::tests::acquire;
use crate::wire::hash_to_wire;
use ganbaru_handoff::protocol::{ControlMessage, SyncRefusal, SyncRefusalCode, SyncSeq};
use ganbaru_sync::Engine;
use ganbaru_sync_contracts::{Seq, WriterId};
use std::collections::BTreeSet;

/// What a lying hub forges about the phone's writer.
#[derive(Clone, Copy, Debug)]
enum Lie {
    /// A probe hash that differs from the phone's head, with hashes that differ after genesis.
    ProbeHash,
    /// A fork entry for the phone's writer in the hub's state.
    ForkedEntry,
    /// A stored sequence of the phone's writer past what the phone sealed.
    InflatedStored,
    /// A fork refusal of a pushed page, with hashes that differ after genesis.
    PushRefusal,
}

/// A hub that answers honestly apart from `lie` about `writer`.
struct LyingHub<'a> {
    hub: &'a DirectTransport,
    lie: Lie,
    writer: WriterId,
    /// The fork sequence a forged entry or refusal names.
    seq: Seq,
}

/// A hash no operation of the test has.
fn forged_hash() -> String {
    hash_to_wire(&[0x5a; 32])
}

impl SyncTransport for LyingHub<'_> {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        let hashes_from = match &message {
            ControlMessage::SyncHashes { from_seq, .. } => Some(*from_seq),
            _ => None,
        };
        let mut response = self.hub.request(message).await?;
        let writer = self.writer.to_hex();
        match (&mut response, self.lie) {
            (ControlMessage::SyncState { probe_hash, .. }, Lie::ProbeHash) => {
                if probe_hash.is_some() {
                    *probe_hash = Some(forged_hash());
                }
            }
            (ControlMessage::SyncState { forked, .. }, Lie::ForkedEntry) => {
                forked.push(SyncSeq {
                    writer,
                    seq: self.seq,
                });
            }
            (ControlMessage::SyncState { stored, .. }, Lie::InflatedStored) => {
                match stored.iter_mut().find(|entry| entry.writer == writer) {
                    Some(entry) => entry.seq += 1,
                    None => stored.push(SyncSeq { writer, seq: 1 }),
                }
            }
            (ControlMessage::SyncPushResult { refusal, .. }, Lie::PushRefusal) => {
                *refusal = Some(SyncRefusal {
                    code: SyncRefusalCode::Fork,
                    writer: Some(writer),
                    seq: Some(self.seq),
                });
            }
            (ControlMessage::SyncHashList { hashes }, Lie::ProbeHash | Lie::PushRefusal) => {
                let from = hashes_from.unwrap_or(1);
                for (seq, hash) in (from..).zip(hashes.iter_mut()) {
                    if seq >= 2 {
                        *hash = forged_hash();
                    }
                }
            }
            _ => {}
        }
        response.validate()?;
        Ok(response)
    }
}

impl Replica {
    async fn exchange_through<T: SyncTransport>(&self, transport: &T) -> Result<Exchange, String> {
        let engine = Engine::vault();
        let own_writers = self.writer.own_writers();
        ClientSession {
            transport,
            ids: &phone_ids(),
            engine: &engine,
            pool: &self.pool,
            ctx: &self.ctx,
            writer: Some(self.writer.id()),
            own_writers: &own_writers,
            quarantined: &BTreeSet::new(),
            pull_skips: None,
            max_bytes: MAX_EXCHANGE_BYTES,
            listening: &[],
            records: None,
        }
        .exchange()
        .await
    }

    async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&mut *acquire(&self.pool).await)
            .await
            .expect("count rows")
    }
}

#[test]
fn unsigned_fork_claim_leaves_the_writer_in_place() {
    block_on(async {
        for lie in [
            Lie::ProbeHash,
            Lie::ForkedEntry,
            Lie::InflatedStored,
            Lie::PushRefusal,
        ] {
            let hub_replica = Replica::new("hub-forged-fork", HUB_DEVICE).await;
            let mut phone = Replica::new("phone-forged-fork", PHONE_DEVICE).await;
            let (transport, _changes) = hub(&hub_replica);
            phone.seal_note("note-first").await;
            let head = phone.seal_note("note-second").await;
            assert!(
                head >= 2,
                "the phone's writer holds operations past genesis"
            );
            done(
                phone
                    .exchange_through(&transport)
                    .await
                    .expect("honest exchange"),
            );
            if matches!(lie, Lie::PushRefusal) {
                phone.seal_note("note-third").await;
            }

            let writer = phone.writer.id();
            let writers = phone.count("SELECT COUNT(*) FROM sync_writers").await;
            let ops = phone.count("SELECT COUNT(*) FROM sync_ops").await;
            let liar = LyingHub {
                hub: &transport,
                lie,
                writer,
                seq: 2,
            };
            match phone.exchange_through(&liar).await {
                Ok(Exchange::UnprovenFork(fork)) => assert_eq!(fork.writer, writer, "{lie:?}"),
                other => panic!("{lie:?}: a forged fork claim was taken as proof: {other:?}"),
            }

            assert_eq!(phone.writer.id(), writer, "{lie:?}");
            assert_eq!(
                phone.count("SELECT COUNT(*) FROM sync_writers").await,
                writers,
                "{lie:?}"
            );
            assert_eq!(
                phone.count("SELECT COUNT(*) FROM sync_ops").await,
                ops,
                "{lie:?}"
            );
            phone.seal_note("note-after").await;
            hub_replica.close().await;
            phone.close().await;
        }
    });
}
