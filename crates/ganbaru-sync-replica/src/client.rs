//! The device side of sync: one exchange pushes what the hub lacks, pulls what this replica
//! lacks, and reports a fork of this installation's writers so the service can re-seal.

use super::outbox::Outbox;
use super::quarantine::PullSkips;
use super::wire::BOOTSTRAP_REQUIRED;
use super::wire::{
    forked_from_wire, hash_from_wire, hash_to_wire, ops_from_wire, ops_to_wire, proposed_from_wire,
    vector_from_wire, vector_to_wire, writer_from_wire,
};
use ganbaru_handoff::protocol::{
    ControlMessage, MAX_SYNC_HASHES, PROTOCOL_VERSION, SYNC_PAGE_BYTES, SyncProbe, SyncRefusal,
    SyncRefusalCode, SyncSettledProposal, decode_sync_proposal, encode_sync_proposal,
};
use ganbaru_sync::{
    Engine, EvidenceOutcome, EvidenceRefusal, SpaceContext, StoreOutcome, StoreRefusal,
};
use ganbaru_sync_contracts::{
    Digest32, Envelope, ProposalRejection, Seq, SignedProposal, VersionVector, WriterId,
};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet};
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
    /// Code of the hub's refusal of a pushed page, when the hub refused one.
    pub refusal: Option<SyncRefusalCode>,
    /// A direction stopped at its byte bound with operations left to move.
    pub more: bool,
    /// Operations the hub reported every counted device holds.
    pub stable: VersionVector,
    /// The hub reported that it no longer holds operations this device lacks. Pulling stops
    /// until the device bootstraps from a fresh copy of the vault; pushing continues.
    pub bootstrap_required: bool,
    /// The head of the active writer, when the hub confirmed it holds the same operation there.
    /// A device compacts its own writer no further than this head, so a fork found later can
    /// still be re-sealed.
    pub confirmed: Option<(WriterId, Seq)>,
    /// The hub's stored vector as of its last answer to a hello or push. When the hub is the
    /// anchor, it marks what the anchor stores, see `Engine::record_anchor_marks`.
    pub hub: VersionVector,
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
    /// Devices this installation quarantined, whose writers' operations it does not store.
    pub quarantined: &'a BTreeSet<String>,
    /// Operations of quarantined devices earlier pulls turned away, which later pulls move past.
    pub pull_skips: Option<&'a PullSkips>,
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
        let (stored, probe_hash, stable, bootstrap_required, forked) = answer(
            self.transport.request(hello).await?,
            "SyncState",
            |response| match response {
                ControlMessage::SyncState {
                    stored,
                    probe_hash,
                    stable,
                    bootstrap_required,
                    forked,
                    ..
                } => Some((stored, probe_hash, stable, bootstrap_required, forked)),
                _ => None,
            },
        )?;
        let mut hub = vector_from_wire(&stored)?;
        let hub_forked = forked_from_wire(&forked)?;
        if let Some(fork) = self.reported_own_fork(&hub_forked, &local).await? {
            return Ok(Exchange::Fork(fork));
        }
        let mut confirmed = None;
        if let (Some((writer, seq, hash)), Some(theirs)) = (probe, probe_hash) {
            if hash_from_wire(&theirs)? != hash {
                return self.fork_of(writer, seq).await;
            }
            confirmed = Some((writer, seq));
        }
        if let Some(writer) = self.writer
            && hub.get(&writer) > local.get(&writer)
        {
            return Ok(Exchange::Fork(ForkPoint {
                writer,
                keep_through: local.get(&writer),
            }));
        }
        let mut exchanged = Exchanged {
            stable: vector_from_wire(&stable)?,
            bootstrap_required,
            confirmed,
            ..Exchanged::default()
        };
        let pushed = self.push(&mut hub, &mut exchanged).await?;
        exchanged.hub = hub;
        if let Some(fork) = pushed {
            return Ok(Exchange::Fork(fork));
        }
        if !exchanged.bootstrap_required
            && let Some(fork) = self.pull(&mut exchanged).await?
        {
            return Ok(Exchange::Fork(fork));
        }
        self.share_evidence(&hub_forked, &mut exchanged).await?;
        exchanged.known = self.stored().await?;
        Ok(Exchange::Done(exchanged))
    }

    async fn push(
        &self,
        hub: &mut VersionVector,
        exchanged: &mut Exchanged,
    ) -> Result<Option<ForkPoint>, String> {
        let mut moved = 0usize;
        // What the hub stored plus the operations it took without a refusal, which include
        // those of devices it quarantined, so the next page moves past them.
        let mut cursor = hub.clone();
        loop {
            let page = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .ops_after(&mut conn, self.ctx, &cursor, self.page_limit(moved))
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
            *hub = vector_from_wire(&stored)?;
            exchanged.pushed += page.ops.len();
            if let Some(refusal) = refusal {
                if let Some(fork) = self.own_fork(&refusal).await? {
                    return Ok(Some(fork));
                }
                exchanged.problem = Some(describe_refusal("the hub", &refusal));
                exchanged.refusal = Some(refusal.code);
                if refusal.code == SyncRefusalCode::BootstrapRequired {
                    exchanged.bootstrap_required = true;
                }
                return Ok(None);
            }
            if !page.more {
                return Ok(None);
            }
            let passed = cursor.clone();
            cursor.merge(hub);
            for op in &page.ops {
                let envelope = Envelope::decode(op)
                    .map_err(|_| "this device holds a malformed operation".to_string())?;
                cursor.advance(envelope.writer, envelope.seq);
            }
            if passed.dominates(&cursor) {
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
        // Operations this replica refuses without a fork, counted as known so the next page
        // moves past them.
        let mut skipped = self
            .pull_skips
            .map(|skips| skips.for_quarantine(self.quarantined))
            .unwrap_or_default();
        loop {
            let mut known = self.stored().await?;
            known.merge(&skipped);
            let request = ControlMessage::SyncPull {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                known: vector_to_wire(&known),
                max_bytes: u32::try_from(self.page_limit(moved)).unwrap_or(u32::MAX),
            };
            let response = self.transport.request(request).await?;
            if let ControlMessage::Error { code, message, .. } = &response
                && code == BOOTSTRAP_REQUIRED
            {
                exchanged.bootstrap_required = true;
                exchanged.problem = Some(format!("{code}: {message}"));
                return Ok(None);
            }
            let (ops, more) = answer(response, "SyncOps", |response| match response {
                ControlMessage::SyncOps { ops, more } => Some((ops, more)),
                _ => None,
            })?;
            if ops.is_empty() {
                return Ok(None);
            }
            let ops = ops_from_wire(&ops)?;
            let outcomes = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .store_many_quarantined(
                        &mut conn,
                        self.ctx,
                        &ops,
                        self.quarantined,
                        super::now_ms()?,
                    )
                    .await
                    .map_err(|error| format!("store sync operations: {error}"))?
            };
            for (op, outcome) in ops.iter().zip(&outcomes) {
                match outcome {
                    StoreOutcome::Stored { .. } => exchanged.pulled += 1,
                    // An operation past a cutoff this replica already knows, which a hub still
                    // holds until its next apply excludes it, or of a quarantined device.
                    StoreOutcome::Duplicate => {}
                    StoreOutcome::Refused(
                        refusal @ (StoreRefusal::Revoked | StoreRefusal::Quarantined),
                    ) => {
                        if let Ok(envelope) = Envelope::decode(op) {
                            skipped.advance(envelope.writer, envelope.seq);
                            if *refusal == StoreRefusal::Quarantined
                                && let Some(skips) = self.pull_skips
                            {
                                skips.advance(envelope.writer, envelope.seq);
                            }
                        }
                    }
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
            let mut reached = self.stored().await?;
            reached.merge(&skipped);
            if known.dominates(&reached) {
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
                    let fork = fork_at(writer, *seq)?;
                    self.offer_own(writer, *seq).await?;
                    return Ok(fork);
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

    /// A writer of this installation that the hub reports forked while this replica still holds
    /// its copy past the shared prefix. The hub's evidence is stored here first, so the fork
    /// reaches the re-seal however it was found. Once re-sealed, this replica holds the prefix
    /// only and the report no longer applies.
    async fn reported_own_fork(
        &self,
        hub_forked: &[(WriterId, Seq)],
        local: &VersionVector,
    ) -> Result<Option<ForkPoint>, String> {
        for (writer, seq) in hub_forked {
            if !self.own_writers.contains(writer) || local.get(writer) < *seq {
                continue;
            }
            let pair = self.request_evidence(*writer, Vec::new()).await?;
            if let Some(EvidenceOutcome::Refused(refusal)) = self.store_evidence(&pair).await? {
                return Err(format!(
                    "this device refused fork evidence of its writer {}: {refusal:?}",
                    writer.to_hex()
                ));
            }
            return fork_at(*writer, *seq).map(fork_only);
        }
        Ok(None)
    }

    /// Offers this installation's operation of `writer` at the first differing sequence, so the
    /// hub holds fork evidence that it forwards to every member, and stores the pair the hub
    /// answers. A hub that no longer holds its own copy answers no pair.
    async fn offer_own(&self, writer: WriterId, seq: Seq) -> Result<(), String> {
        let operation = {
            let mut conn = acquire(self.pool).await?;
            self.engine
                .operation_at(&mut conn, self.ctx, writer, seq)
                .await
                .map_err(|error| format!("read sync log: {error}"))?
        };
        let Some(operation) = operation else {
            return Ok(());
        };
        let pair = self.request_evidence(writer, vec![operation]).await?;
        match self.store_evidence(&pair).await? {
            None | Some(EvidenceOutcome::Stored | EvidenceOutcome::Known) => Ok(()),
            Some(EvidenceOutcome::Refused(refusal)) => Err(format!(
                "the hub answered fork evidence of writer {} this device refused: {refusal:?}",
                writer.to_hex()
            )),
        }
    }

    /// Exchanges fork evidence with the hub: fetches the pair of every writer the hub reports
    /// forked that this replica does not know forked at that sequence, and offers the pair of
    /// every writer this replica knows forked that the hub does not. A pair whose writer this
    /// replica has not stored yet waits for a later exchange.
    async fn share_evidence(
        &self,
        hub_forked: &[(WriterId, Seq)],
        exchanged: &mut Exchanged,
    ) -> Result<(), String> {
        let local: BTreeMap<WriterId, Seq> = {
            let mut conn = acquire(self.pool).await?;
            self.engine
                .forked_writers(&mut conn, self.ctx)
                .await
                .map_err(|error| format!("read fork evidence: {error}"))?
                .into_iter()
                .collect()
        };
        for (writer, seq) in hub_forked {
            if local.get(writer).is_some_and(|known| known <= seq) {
                continue;
            }
            let pair = self.request_evidence(*writer, Vec::new()).await?;
            if let Some(EvidenceOutcome::Refused(refusal)) = self.store_evidence(&pair).await?
                && refusal != EvidenceRefusal::UnknownWriter
            {
                exchanged.problem = Some(format!(
                    "this device refused fork evidence of writer {}: {refusal:?}",
                    writer.to_hex()
                ));
            }
        }
        let hub: BTreeMap<WriterId, Seq> = hub_forked.iter().copied().collect();
        for (writer, seq) in &local {
            if hub.get(writer).is_some_and(|known| known <= seq) {
                continue;
            }
            let evidence = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .fork_evidence(&mut conn, self.ctx, *writer)
                    .await
                    .map_err(|error| format!("read fork evidence: {error}"))?
            };
            if let Some(evidence) = evidence {
                self.request_evidence(*writer, vec![evidence.first, evidence.second])
                    .await?;
            }
        }
        Ok(())
    }

    /// Sends fork evidence of `writer`, possibly none, and returns the pair the hub holds.
    async fn request_evidence(
        &self,
        writer: WriterId,
        evidence: Vec<Vec<u8>>,
    ) -> Result<Vec<Vec<u8>>, String> {
        let request = ControlMessage::SyncForkEvidence {
            protocol_version: PROTOCOL_VERSION,
            vault_id: self.ids.vault_id.clone(),
            device_id: self.ids.device_id.clone(),
            writer: writer.to_hex(),
            evidence: ops_to_wire(&evidence),
        };
        let pair = answer(
            self.transport.request(request).await?,
            "SyncForkPair",
            |response| match response {
                ControlMessage::SyncForkPair { evidence } => Some(evidence),
                _ => None,
            },
        )?;
        ops_from_wire(&pair)
    }

    /// Stores a pair the hub answered, or nothing when it answered none.
    async fn store_evidence(&self, pair: &[Vec<u8>]) -> Result<Option<EvidenceOutcome>, String> {
        let [first, second] = pair else {
            return Ok(None);
        };
        let mut conn = acquire(self.pool).await?;
        self.engine
            .store_fork_evidence(&mut conn, self.ctx, first, second, super::now_ms()?)
            .await
            .map(Some)
            .map_err(|error| format!("store fork evidence: {error}"))
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

/// Sends an admin-signed proposal to the hub and returns the revision that holds it, or why the
/// anchor rejected it. A hub that is not the anchor answers [`ProposalRejection::NotAnchor`].
pub async fn propose<T: SyncTransport>(
    transport: &T,
    ids: &PeerIds,
    proposal: &SignedProposal,
) -> Result<Result<u64, ProposalRejection>, String> {
    let request = ControlMessage::SyncPropose {
        protocol_version: PROTOCOL_VERSION,
        vault_id: ids.vault_id.clone(),
        device_id: ids.device_id.clone(),
        proposal: encode_sync_proposal(proposal),
    };
    let (revision, rejection) = answer(
        transport.request(request).await?,
        "SyncProposed",
        |response| match response {
            ControlMessage::SyncProposed {
                revision,
                rejection,
            } => Some((revision, rejection)),
            _ => None,
        },
    )?;
    proposed_from_wire(revision, rejection)
}

/// Sends every waiting proposal of the outbox to the hub and records each answer. A revision
/// leaves the proposal waiting until [`Outbox::prune`] finds it in the selected chain, after the
/// exchange brings the entry and the pass applies it. Returns how many proposals the anchor
/// sequenced.
pub async fn replay_outbox<T: SyncTransport>(
    transport: &T,
    ids: &PeerIds,
    outbox: &mut Outbox,
) -> Result<usize, String> {
    let mut sequenced = 0;
    for proposal in outbox.pending() {
        let outcome = propose(transport, ids, &proposal).await?;
        sequenced += usize::from(outcome.is_ok());
        outbox.settle(&proposal.hash(), outcome)?;
    }
    Ok(sequenced)
}

/// Follows the local replica's selected chain with [`Outbox::prune`], which also puts a held
/// proposal a handover superseded back to waiting, then sends what waits with
/// [`replay_outbox`]. A member runs this after each exchange, even when nothing waited before.
pub async fn prune_and_replay_outbox<T: SyncTransport>(
    engine: &Engine,
    pool: &SqlitePool,
    ctx: &SpaceContext,
    transport: &T,
    ids: &PeerIds,
    outbox: &mut Outbox,
) -> Result<usize, String> {
    if outbox.entries().is_empty() {
        return Ok(0);
    }
    {
        let mut conn = acquire(pool).await?;
        outbox.prune(engine, &mut conn, ctx).await?;
    }
    if outbox.pending().is_empty() {
        return Ok(0);
    }
    replay_outbox(transport, ids, outbox).await
}

/// On an anchor that is not the hub, reports the final rejections it gave proposals of an
/// earlier forward and collects the proposals the hub holds for it. The hub answers only the
/// device the selected log names as anchor; anyone else receives none.
pub async fn forward<T: SyncTransport>(
    transport: &T,
    ids: &PeerIds,
    settled: &[(Digest32, ProposalRejection)],
) -> Result<Vec<SignedProposal>, String> {
    let request = ControlMessage::SyncForward {
        protocol_version: PROTOCOL_VERSION,
        vault_id: ids.vault_id.clone(),
        device_id: ids.device_id.clone(),
        settled: settled
            .iter()
            .map(|(hash, rejection)| SyncSettledProposal {
                proposal: hash_to_wire(hash),
                rejection: rejection.as_u8(),
            })
            .collect(),
    };
    let proposals = answer(
        transport.request(request).await?,
        "SyncForwarded",
        |response| match response {
            ControlMessage::SyncForwarded { proposals } => Some(proposals),
            _ => None,
        },
    )?;
    proposals
        .iter()
        .map(|proposal| decode_sync_proposal(proposal))
        .collect()
}

/// Extracts the expected response, or reports the hub's error.
pub(crate) fn answer<V>(
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
