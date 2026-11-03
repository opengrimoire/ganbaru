//! A hub that claims a fork of this device's writer and shows no signed operation for it: the
//! device keeps its writer and reports the claim as unproven.

use super::{
    DirectTransport, Exchange, HUB_DEVICE, PHONE_DEVICE, Replica, block_on, copy_database, done,
    hub, phone_ids,
};
use crate::client::{ClientSession, ForkPoint, MAX_EXCHANGE_BYTES, SyncTransport};
use crate::tests::{TestDirectory, acquire};
use crate::wire::{bytes_to_wire, hash_to_wire};
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
    /// A genuine operation of `writer` the hub answers as its head instead of the real one.
    replayed_head: Option<String>,
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
            (ControlMessage::SyncWriterHeadOp { op }, _) => {
                if let Some(replayed) = &self.replayed_head {
                    *op = Some(replayed.clone());
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
                replayed_head: None,
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

#[test]
fn replayed_old_head_proves_nothing() {
    block_on(async {
        let hub_replica = Replica::new("hub-replayed-head", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-replayed-head", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        phone.seal_note("note-first").await;
        let head = phone.seal_note("note-second").await;
        done(
            phone
                .exchange_through(&transport)
                .await
                .expect("honest exchange"),
        );
        let writer = phone.writer.id();
        // A genuine, signed operation of the phone's writer that the phone already holds.
        let older = Engine::vault()
            .operation_at(
                &mut *acquire(&phone.pool).await,
                &phone.ctx,
                writer,
                head - 1,
            )
            .await
            .expect("read operation")
            .expect("older operation");
        let writers = phone.count("SELECT COUNT(*) FROM sync_writers").await;
        let liar = LyingHub {
            hub: &transport,
            lie: Lie::InflatedStored,
            writer,
            seq: head,
            replayed_head: Some(bytes_to_wire(&older)),
        };
        match phone.exchange_through(&liar).await {
            Ok(Exchange::UnprovenFork(fork)) => assert_eq!(fork.writer, writer),
            other => panic!("a replayed operation was taken as proof: {other:?}"),
        }
        assert_eq!(phone.writer.id(), writer);
        assert_eq!(
            phone.count("SELECT COUNT(*) FROM sync_writers").await,
            writers
        );
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn rollback_with_a_signed_head_still_recovers() {
    block_on(async {
        let hub_replica = Replica::new("hub-rollback-head", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-rollback-head", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        phone.seal_note("note-first").await;
        let kept = phone.seal_note("note-second").await;
        done(
            phone
                .exchange_through(&transport)
                .await
                .expect("honest exchange"),
        );

        // An older copy of the phone's vault, restored after the phone pushed a later operation.
        let phone_path = phone.directory.path().join("vault.sqlite");
        drop(phone.pool);
        phone.registry.close_all().await.expect("close phone");
        let backup_directory = TestDirectory::new("phone-rollback-head-backup");
        let backup_path = backup_directory.path().join("vault.sqlite");
        copy_database(&phone_path, &backup_path);
        phone.pool = phone
            .registry
            .connect_path(&phone_path)
            .await
            .expect("reopen phone");
        phone.seal_note("note-third").await;
        done(
            phone
                .exchange_through(&transport)
                .await
                .expect("push the later operation"),
        );
        drop(phone.pool);
        phone.registry.close_all().await.expect("close phone");
        for suffix in ["-wal", "-shm"] {
            let stale = format!("{}{suffix}", phone_path.display());
            if std::path::Path::new(&stale).exists() {
                std::fs::remove_file(&stale).expect("remove the newer journal");
            }
        }
        copy_database(&backup_path, &phone_path);
        phone.pool = phone
            .registry
            .connect_path(&phone_path)
            .await
            .expect("reopen the restored phone");

        let writer = phone.writer.id();
        match phone.exchange_through(&transport).await {
            Ok(Exchange::Fork(fork)) => assert_eq!(
                fork,
                ForkPoint {
                    writer,
                    keep_through: kept,
                }
            ),
            other => panic!("the hub's signed head proves the rollback: {other:?}"),
        }
        hub_replica.close().await;
        phone.close().await;
    });
}
