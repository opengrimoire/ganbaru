//! The device side of sync: one exchange pushes what the hub lacks, pulls what this replica
//! lacks, and reports a fork of this installation's writers so the service can re-seal.

use super::wire::{
    hash_from_wire, hash_to_wire, ops_from_wire, ops_to_wire, vector_from_wire, vector_to_wire,
    writer_from_wire,
};
use ganbaru_handoff::protocol::{
    ControlMessage, MAX_SYNC_HASHES, PROTOCOL_VERSION, SYNC_PAGE_BYTES, SyncProbe, SyncRefusal,
    SyncRefusalCode,
};
use ganbaru_sync::{Engine, SpaceContext, StoreOutcome, StoreRefusal};
use ganbaru_sync_contracts::{Envelope, Seq, VersionVector, WriterId};
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::future::Future;

/// Bound on operation bytes moved in one direction of one exchange. Pulled operations wait in
/// the log until the pass applies them, so the bound also caps that backlog; the service runs
/// the next pass right away to continue.
pub const MAX_EXCHANGE_BYTES: usize = 32 * 1024 * 1024;

fn page_bytes(ops: &[Vec<u8>]) -> usize {
    ops.iter().map(Vec::len).sum()
}

/// Sends one sync request to the hub and returns its answer.
pub trait SyncTransport {
    fn request(
        &self,
        message: ControlMessage,
    ) -> impl Future<Output = Result<ControlMessage, String>> + Send;
}

/// The ids every request presents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerIds {
    pub vault_id: String,
    pub device_id: String,
}

/// A chain of this installation that another copy continued differently. Operations after
/// `keep_through` are re-sealed by a successor writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForkPoint {
    pub writer: WriterId,
    pub keep_through: Seq,
}

/// Result of an exchange that found no fork.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exchanged {
    pub pushed: usize,
    pub pulled: usize,
    /// Local stored vector after the exchange, what a following wait reports as known.
    pub known: VersionVector,
    /// Why part of the exchange stopped, when the hub or this replica refused operations.
    pub problem: Option<String>,
    /// A direction stopped at its byte bound with operations left to move.
    pub more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exchange {
    Done(Exchanged),
    Fork(ForkPoint),
}

/// One replica's view for an exchange.
pub struct ClientSession<'a, T: SyncTransport> {
    pub transport: &'a T,
    pub ids: &'a PeerIds,
    pub engine: &'a Engine,
    pub pool: &'a SqlitePool,
    pub ctx: &'a SpaceContext,
    /// The active writer of this installation.
    pub writer: Option<WriterId>,
    /// Every writer this installation sealed with, the active one included.
    pub own_writers: &'a BTreeSet<WriterId>,
    /// Operation bytes after which a direction stops, normally [`MAX_EXCHANGE_BYTES`].
    pub max_bytes: usize,
}

impl<T: SyncTransport> ClientSession<'_, T> {
    /// Page size for the next page of a direction that already moved `moved` bytes: the rest
    /// of the bound, at least one byte so the page still carries one operation.
    fn page_limit(&self, moved: usize) -> usize {
        self.max_bytes
            .saturating_sub(moved)
            .clamp(1, SYNC_PAGE_BYTES)
    }

    /// Hello, push, and pull until both sides hold the same operations or a refusal stops one
    /// direction.
    pub async fn exchange(&self) -> Result<Exchange, String> {
        let local = self.stored().await?;
        let probe = self.probe(&local).await?;
        let hello = ControlMessage::SyncHello {
            protocol_version: PROTOCOL_VERSION,
            vault_id: self.ids.vault_id.clone(),
            device_id: self.ids.device_id.clone(),
            manifest_version: self.engine.manifest().version,
            stored: vector_to_wire(&local),
            probe: probe.as_ref().map(|(writer, seq, hash)| SyncProbe {
                writer: writer.to_hex(),
                seq: *seq,
                hash: hash_to_wire(hash),
            }),
        };
        let (stored, probe_hash) = answer(
            self.transport.request(hello).await?,
            "SyncState",
            |response| match response {
                ControlMessage::SyncState {
                    stored, probe_hash, ..
                } => Some((stored, probe_hash)),
                _ => None,
            },
        )?;
        let mut hub = vector_from_wire(&stored)?;
        if let (Some((writer, seq, hash)), Some(theirs)) = (probe, probe_hash)
            && hash_from_wire(&theirs)? != hash
        {
            return self.fork_of(writer, seq).await;
        }
        if let Some(writer) = self.writer
            && hub.get(&writer) > local.get(&writer)
        {
            return Ok(Exchange::Fork(ForkPoint {
                writer,
                keep_through: local.get(&writer),
            }));
        }
        let mut exchanged = Exchanged::default();
        if let Some(fork) = self.push(&mut hub, &mut exchanged).await? {
            return Ok(Exchange::Fork(fork));
        }
        if let Some(fork) = self.pull(&mut exchanged).await? {
            return Ok(Exchange::Fork(fork));
        }
        exchanged.known = self.stored().await?;
        Ok(Exchange::Done(exchanged))
    }

    async fn push(
        &self,
        hub: &mut VersionVector,
        exchanged: &mut Exchanged,
    ) -> Result<Option<ForkPoint>, String> {
        let mut moved = 0usize;
        loop {
            let page = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .ops_after(&mut conn, self.ctx, hub, self.page_limit(moved))
                    .await
                    .map_err(|error| format!("read sync operations: {error}"))?
            };
            if page.ops.is_empty() {
                return Ok(None);
            }
            let request = ControlMessage::SyncPush {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                ops: ops_to_wire(&page.ops),
            };
            let (stored, refusal) = answer(
                self.transport.request(request).await?,
                "SyncPushResult",
                |response| match response {
                    ControlMessage::SyncPushResult { stored, refusal } => Some((stored, refusal)),
                    _ => None,
                },
            )?;
            let before = hub.clone();
            *hub = vector_from_wire(&stored)?;
            exchanged.pushed += page.ops.len();
            if let Some(refusal) = refusal {
                if let Some(fork) = self.own_fork(&refusal).await? {
                    return Ok(Some(fork));
                }
                exchanged.problem = Some(describe_refusal("the hub", &refusal));
                return Ok(None);
            }
            if !page.more {
                return Ok(None);
            }
            if before.dominates(hub) {
                exchanged.problem = Some("the hub stored nothing of a pushed page".to_string());
                return Ok(None);
            }
            moved += page_bytes(&page.ops);
            if moved >= self.max_bytes {
                exchanged.more = true;
                return Ok(None);
            }
        }
    }

    async fn pull(&self, exchanged: &mut Exchanged) -> Result<Option<ForkPoint>, String> {
        let mut moved = 0usize;
        loop {
            let known = self.stored().await?;
            let request = ControlMessage::SyncPull {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                known: vector_to_wire(&known),
                max_bytes: u32::try_from(self.page_limit(moved)).unwrap_or(u32::MAX),
            };
            let (ops, more) = answer(
                self.transport.request(request).await?,
                "SyncOps",
                |response| match response {
                    ControlMessage::SyncOps { ops, more } => Some((ops, more)),
                    _ => None,
                },
            )?;
            if ops.is_empty() {
                return Ok(None);
            }
            let ops = ops_from_wire(&ops)?;
            let outcomes = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .store_many(&mut conn, self.ctx, &ops, super::now_ms()?)
                    .await
                    .map_err(|error| format!("store sync operations: {error}"))?
            };
            for (op, outcome) in ops.iter().zip(&outcomes) {
                match outcome {
                    StoreOutcome::Stored { .. } => exchanged.pulled += 1,
                    StoreOutcome::Duplicate => {}
                    StoreOutcome::Refused(refusal) => {
                        let envelope = Envelope::decode(op)
                            .map_err(|_| "the hub sent a malformed operation".to_string())?;
                        if let StoreRefusal::Fork { seq } = refusal
                            && self.own_writers.contains(&envelope.writer)
                        {
                            return self.fork_of(envelope.writer, *seq).await.map(fork_only);
                        }
                        exchanged.problem = Some(format!(
                            "this device refused an operation of writer {}: {refusal:?}",
                            envelope.writer.to_hex()
                        ));
                        return Ok(None);
                    }
                }
            }
            if !more {
                return Ok(None);
            }
            if known.dominates(&self.stored().await?) {
                exchanged.problem = Some("the hub sent no new operations".to_string());
                return Ok(None);
            }
            moved += page_bytes(&ops);
            if moved >= self.max_bytes {
                exchanged.more = true;
                return Ok(None);
            }
        }
    }

    /// A fork refusal of one of this installation's writers, located.
    async fn own_fork(&self, refusal: &SyncRefusal) -> Result<Option<ForkPoint>, String> {
        if refusal.code != SyncRefusalCode::Fork {
            return Ok(None);
        }
        let (Some(writer), Some(seq)) = (&refusal.writer, refusal.seq) else {
            return Ok(None);
        };
        let writer = writer_from_wire(writer)?;
        if !self.own_writers.contains(&writer) {
            return Ok(None);
        }
        self.fork_of(writer, seq).await.map(fork_only)
    }

    /// Finds the first sequence where the hub's copy of `writer` differs from the local one,
    /// comparing pages of hashes from the start of the chain up to `hint`, a sequence at or
    /// after the divergence.
    async fn fork_of(&self, writer: WriterId, hint: Seq) -> Result<Exchange, String> {
        let mut from: Seq = 1;
        while from <= hint {
            let request = ControlMessage::SyncHashes {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                writer: writer.to_hex(),
                from_seq: from,
                limit: MAX_SYNC_HASHES,
            };
            let hashes = answer(
                self.transport.request(request).await?,
                "SyncHashList",
                |response| match response {
                    ControlMessage::SyncHashList { hashes } => Some(hashes),
                    _ => None,
                },
            )?;
            let local = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .op_hashes(&mut conn, self.ctx, writer, from, MAX_SYNC_HASHES)
                    .await
                    .map_err(|error| format!("read sync log: {error}"))?
            };
            let mut compared: Seq = 0;
            for (theirs, (seq, ours)) in hashes.iter().zip(&local) {
                if *seq != from + compared {
                    break;
                }
                if hash_from_wire(theirs)? != *ours {
                    return fork_at(writer, *seq);
                }
                compared += 1;
            }
            if compared < Seq::from(MAX_SYNC_HASHES) {
                break;
            }
            from += compared;
        }
        Err(format!(
            "the copies of writer {} agree, so the fork cannot be located",
            writer.to_hex()
        ))
    }

    async fn stored(&self) -> Result<VersionVector, String> {
        let mut conn = acquire(self.pool).await?;
        self.engine
            .stored_vector(&mut conn, self.ctx)
            .await
            .map_err(|error| format!("read sync log: {error}"))
    }

    /// The local head of the active writer, which the hub compares with its copy.
    async fn probe(
        &self,
        local: &VersionVector,
    ) -> Result<Option<(WriterId, Seq, ganbaru_sync_contracts::Digest32)>, String> {
        let Some(writer) = self.writer else {
            return Ok(None);
        };
        let seq = local.get(&writer);
        if seq == 0 {
            return Ok(None);
        }
        let mut conn = acquire(self.pool).await?;
        let hashes = self
            .engine
            .op_hashes(&mut conn, self.ctx, writer, seq, 1)
            .await
            .map_err(|error| format!("read sync log: {error}"))?;
        Ok(hashes
            .first()
            .filter(|(found, _)| *found == seq)
            .map(|(_, hash)| (writer, seq, *hash)))
    }
}

/// The fork at the first differing sequence. A genesis that differs cannot be re-sealed.
fn fork_at(writer: WriterId, first_difference: Seq) -> Result<Exchange, String> {
    if first_difference <= 1 {
        return Err(format!(
            "writer {} has two different geneses",
            writer.to_hex()
        ));
    }
    Ok(Exchange::Fork(ForkPoint {
        writer,
        keep_through: first_difference - 1,
    }))
}

fn fork_only(exchange: Exchange) -> Option<ForkPoint> {
    match exchange {
        Exchange::Fork(fork) => Some(fork),
        Exchange::Done(_) => None,
    }
}

fn describe_refusal(side: &str, refusal: &SyncRefusal) -> String {
    let writer = refusal.writer.as_deref().unwrap_or("unknown");
    format!(
        "{side} refused an operation of writer {writer}: {:?}",
        refusal.code
    )
}

/// Long poll: the hub's stored vector once it holds operations past `known`, or after its
/// bounded wait.
pub async fn wait<T: SyncTransport>(
    transport: &T,
    ids: &PeerIds,
    known: &VersionVector,
) -> Result<VersionVector, String> {
    let request = ControlMessage::SyncWait {
        protocol_version: PROTOCOL_VERSION,
        vault_id: ids.vault_id.clone(),
        device_id: ids.device_id.clone(),
        known: vector_to_wire(known),
    };
    let stored = answer(
        transport.request(request).await?,
        "SyncState",
        |response| match response {
            ControlMessage::SyncState { stored, .. } => Some(stored),
            _ => None,
        },
    )?;
    vector_from_wire(&stored)
}

/// Extracts the expected response, or reports the hub's error.
fn answer<V>(
    response: ControlMessage,
    expected: &str,
    extract: impl FnOnce(ControlMessage) -> Option<V>,
) -> Result<V, String> {
    if let ControlMessage::Error { code, message, .. } = &response {
        return Err(format!("{code}: {message}"));
    }
    extract(response).ok_or_else(|| format!("the hub did not answer with {expected}"))
}

async fn acquire(pool: &SqlitePool) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, String> {
    pool.acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))
}
