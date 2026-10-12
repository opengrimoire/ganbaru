//! The hub side of sync. The coordinator stores what linked devices push, serves what they pull,
//! and answers long polls when its log grows. Its own service applies what the hub stores.

use super::wire::{
    hash_to_wire, ops_from_wire, ops_to_wire, vector_from_wire, vector_to_wire, writer_from_wire,
};
use ganbaru_handoff::protocol::{ControlMessage, SyncRefusal, SyncRefusalCode};
use ganbaru_sync::{Engine, SpaceContext, StoreOutcome, StoreRefusal, local};
use ganbaru_sync_contracts::{Envelope, Seq, VersionVector, WriterId};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

/// How long a wait is held before the hub answers with its unchanged state. It stays below the
/// control timeout of the transport.
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(12);

/// The vault database the hub serves.
pub struct HubVault {
    pub pool: SqlitePool,
    pub vault_id: String,
}

/// Opens the vault the hub serves, or `None` while it cannot replicate.
pub type OpenHubVault = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<Option<HubVault>, String>> + Send>>
        + Send
        + Sync,
>;

/// A failed hub request, answered as a protocol error.
#[derive(Debug, PartialEq, Eq)]
pub struct HubError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl HubError {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "sync_unavailable",
            message: message.into(),
            retryable: true,
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        Self {
            code: "sync_failed",
            message: message.into(),
            retryable: true,
        }
    }

    fn rejected(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
        }
    }
}

/// The hub handle the coordinator listener routes sync requests to.
#[derive(Clone)]
pub struct SyncHub {
    open: OpenHubVault,
    /// Bumped whenever the hub's log may have grown, by stores here and by its own service.
    changes: watch::Sender<u64>,
    /// The current wait of each peer; a newer wait ends the older one.
    waits: Arc<Mutex<HashMap<String, u64>>>,
}

impl SyncHub {
    pub fn new(open: OpenHubVault, changes: watch::Sender<u64>) -> Self {
        Self {
            open,
            changes,
            waits: Arc::default(),
        }
    }

    /// Answers one sync request of a peer whose identity the listener verified.
    pub async fn respond(&self, request: ControlMessage) -> ControlMessage {
        match self.respond_inner(request).await {
            Ok(response) => response,
            Err(error) => ControlMessage::Error {
                code: error.code.to_string(),
                message: error.message,
                retryable: error.retryable,
            },
        }
    }

    async fn respond_inner(&self, request: ControlMessage) -> Result<ControlMessage, HubError> {
        let Some((vault_id, device_id)) = request.sync_peer() else {
            return Err(HubError::rejected(
                "unsupported_message",
                "the hub only answers sync requests",
            ));
        };
        let (vault_id, device_id) = (vault_id.to_string(), device_id.to_string());
        let Some(vault) = (self.open)().await.map_err(HubError::unavailable)? else {
            return Err(HubError::unavailable("the hub vault cannot replicate now"));
        };
        if vault.vault_id != vault_id {
            return Err(HubError::rejected(
                "vault_mismatch",
                "the hub serves a different vault",
            ));
        }
        let ctx = {
            let mut conn = acquire(&vault.pool).await?;
            local::space_context(&mut conn, &vault_id)
                .await
                .map_err(|error| HubError::failed(format!("read sync space: {error}")))?
        };
        let Some(ctx) = ctx else {
            return Err(HubError::unavailable(
                "the hub vault has no person identity",
            ));
        };
        let hub = HubSession {
            engine: Engine::vault(),
            pool: vault.pool,
            ctx,
        };
        match request {
            ControlMessage::SyncHello { probe, .. } => {
                let probe = match probe {
                    Some(probe) => {
                        Some((writer_from_wire(&probe.writer).map_err(invalid)?, probe.seq))
                    }
                    None => None,
                };
                hub.state(probe).await
            }
            ControlMessage::SyncPush { ops, .. } => {
                let ops = ops_from_wire(&ops).map_err(invalid)?;
                let (response, stored_any) = hub.push(&device_id, &ops).await?;
                if stored_any {
                    self.changes.send_modify(|generation| *generation += 1);
                }
                Ok(response)
            }
            ControlMessage::SyncPull {
                known, max_bytes, ..
            } => {
                let known = vector_from_wire(&known).map_err(invalid)?;
                hub.pull(&known, max_bytes as usize).await
            }
            ControlMessage::SyncWait { known, .. } => {
                let known = vector_from_wire(&known).map_err(invalid)?;
                self.wait(&hub, &device_id, &known).await
            }
            ControlMessage::SyncHashes {
                writer,
                from_seq,
                limit,
                ..
            } => {
                let writer = writer_from_wire(&writer).map_err(invalid)?;
                hub.hashes(writer, from_seq, limit).await
            }
            _ => Err(HubError::rejected(
                "unsupported_message",
                "the hub only answers sync requests",
            )),
        }
    }

    /// Holds the request until the hub stores operations past `known`, a newer wait of the same
    /// peer arrives, or the wait times out.
    async fn wait(
        &self,
        hub: &HubSession,
        device_id: &str,
        known: &VersionVector,
    ) -> Result<ControlMessage, HubError> {
        let token = {
            let mut waits = self
                .waits
                .lock()
                .map_err(|_| HubError::failed("sync wait lock is unavailable"))?;
            let token = waits.get(device_id).map_or(0, |token| token + 1);
            waits.insert(device_id.to_string(), token);
            token
        };
        let mut changes = self.changes.subscribe();
        self.changes.send_modify(|generation| *generation += 1);
        changes.borrow_and_update();
        let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
        let result = loop {
            let stored = hub.stored().await?;
            let superseded = self
                .waits
                .lock()
                .map_err(|_| HubError::failed("sync wait lock is unavailable"))?
                .get(device_id)
                != Some(&token);
            if superseded || !known.dominates(&stored) {
                break Ok(hub.state_message(&stored, None));
            }
            match tokio::time::timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => break Ok(hub.state_message(&stored, None)),
            }
        };
        if let Ok(mut waits) = self.waits.lock()
            && waits.get(device_id) == Some(&token)
        {
            waits.remove(device_id);
        }
        result
    }
}

/// One request against the hub vault.
struct HubSession {
    engine: Engine,
    pool: SqlitePool,
    ctx: SpaceContext,
}

impl HubSession {
    async fn stored(&self) -> Result<VersionVector, HubError> {
        let mut conn = acquire(&self.pool).await?;
        self.engine
            .stored_vector(&mut conn, &self.ctx)
            .await
            .map_err(|error| HubError::failed(format!("read sync log: {error}")))
    }

    fn state_message(&self, stored: &VersionVector, probe_hash: Option<String>) -> ControlMessage {
        ControlMessage::SyncState {
            manifest_version: self.engine.manifest().version,
            stored: vector_to_wire(stored),
            probe_hash,
        }
    }

    /// The hub's stored vector and its hash at the probed operation, when it holds it.
    async fn state(&self, probe: Option<(WriterId, u64)>) -> Result<ControlMessage, HubError> {
        let stored = self.stored().await?;
        let probe_hash = match probe {
            Some((writer, seq)) => {
                let mut conn = acquire(&self.pool).await?;
                self.engine
                    .op_hashes(&mut conn, &self.ctx, writer, seq, 1)
                    .await
                    .map_err(|error| HubError::failed(format!("read sync log: {error}")))?
                    .first()
                    .filter(|(found, _)| *found == seq)
                    .map(|(_, hash)| hash_to_wire(hash))
            }
            None => None,
        };
        Ok(self.state_message(&stored, probe_hash))
    }

    /// Stores pushed operations up to the first refusal. A genesis of a writer the hub does not
    /// hold is accepted only when its certificate names the pushing device.
    async fn push(
        &self,
        device_id: &str,
        ops: &[Vec<u8>],
    ) -> Result<(ControlMessage, bool), HubError> {
        let known = self.stored().await?;
        let (accepted, mut refusal) = admit_geneses(ops, &known, device_id);
        let outcomes = if accepted == 0 {
            Vec::new()
        } else {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .store_many(&mut conn, &self.ctx, &ops[..accepted], now_ms()?)
                .await
                .map_err(|error| HubError::failed(format!("store sync operations: {error}")))?
        };
        let mut stored_any = false;
        for (op, outcome) in ops.iter().zip(&outcomes) {
            match outcome {
                StoreOutcome::Stored { .. } => stored_any = true,
                StoreOutcome::Duplicate => {}
                StoreOutcome::Refused(store_refusal) => {
                    refusal = Some(match Envelope::decode(op) {
                        Ok(envelope) => {
                            refusal_to_wire(*store_refusal, envelope.writer, envelope.seq)
                        }
                        Err(_) => invalid_refusal(),
                    });
                    break;
                }
            }
        }
        let stored = self.stored().await?;
        Ok((
            ControlMessage::SyncPushResult {
                stored: vector_to_wire(&stored),
                refusal,
            },
            stored_any,
        ))
    }

    async fn pull(
        &self,
        known: &VersionVector,
        max_bytes: usize,
    ) -> Result<ControlMessage, HubError> {
        let mut conn = acquire(&self.pool).await?;
        let page = self
            .engine
            .ops_after(&mut conn, &self.ctx, known, max_bytes)
            .await
            .map_err(|error| HubError::failed(format!("read sync operations: {error}")))?;
        Ok(ControlMessage::SyncOps {
            ops: ops_to_wire(&page.ops),
            more: page.more,
        })
    }

    async fn hashes(
        &self,
        writer: WriterId,
        from_seq: u64,
        limit: u32,
    ) -> Result<ControlMessage, HubError> {
        let mut conn = acquire(&self.pool).await?;
        let hashes = self
            .engine
            .op_hashes(&mut conn, &self.ctx, writer, from_seq, limit)
            .await
            .map_err(|error| HubError::failed(format!("read sync log: {error}")))?;
        let consecutive = hashes
            .iter()
            .zip(from_seq..)
            .take_while(|((seq, _), expected)| seq == expected)
            .map(|((_, hash), _)| hash_to_wire(hash))
            .collect();
        Ok(ControlMessage::SyncHashList {
            hashes: consecutive,
        })
    }
}

/// How many leading operations may be stored, and the refusal of the first that may not: an
/// envelope that does not decode, or the genesis of an unknown writer certified for another
/// device. Later checks are the engine's.
fn admit_geneses(
    ops: &[Vec<u8>],
    known: &VersionVector,
    device_id: &str,
) -> (usize, Option<SyncRefusal>) {
    for (index, op) in ops.iter().enumerate() {
        let Ok(envelope) = Envelope::decode(op) else {
            return (index, Some(invalid_refusal()));
        };
        if envelope.seq != 1 || known.get(&envelope.writer) > 0 {
            continue;
        }
        let foreign = match envelope.genesis() {
            Ok((_, certificate)) => certificate.certificate.device_id != device_id,
            Err(_) => false,
        };
        if foreign {
            return (
                index,
                Some(SyncRefusal {
                    code: SyncRefusalCode::UnknownWriter,
                    writer: Some(envelope.writer.to_hex()),
                    seq: Some(envelope.seq),
                }),
            );
        }
    }
    (ops.len(), None)
}

fn invalid_refusal() -> SyncRefusal {
    SyncRefusal {
        code: SyncRefusalCode::Invalid,
        writer: None,
        seq: None,
    }
}

/// The wire refusal of an engine store refusal of `(writer, seq)`.
fn refusal_to_wire(refusal: StoreRefusal, writer: WriterId, seq: Seq) -> SyncRefusal {
    let (code, seq) = match refusal {
        StoreRefusal::Fork { seq } => (SyncRefusalCode::Fork, seq),
        StoreRefusal::Gap { expected } => (SyncRefusalCode::Gap, expected),
        StoreRefusal::Revoked => (SyncRefusalCode::Revoked, seq),
        StoreRefusal::UnknownWriter => (SyncRefusalCode::UnknownWriter, seq),
        StoreRefusal::NewerFormatGenesis => (SyncRefusalCode::NewerFormat, seq),
        StoreRefusal::Malformed
        | StoreRefusal::WrongSpace
        | StoreRefusal::BadSignature
        | StoreRefusal::Untrusted => (SyncRefusalCode::Invalid, seq),
    };
    SyncRefusal {
        code,
        writer: Some(writer.to_hex()),
        seq: Some(seq),
    }
}

fn invalid(message: String) -> HubError {
    HubError::rejected("invalid_sync_request", message)
}

async fn acquire(pool: &SqlitePool) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, HubError> {
    pool.acquire()
        .await
        .map_err(|error| HubError::failed(format!("acquire sync connection: {error}")))
}

fn now_ms() -> Result<u64, HubError> {
    super::now_ms().map_err(HubError::failed)
}
