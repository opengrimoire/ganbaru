//! The hub side of sync. The coordinator stores what linked devices push, serves what they pull,
//! and answers long polls when its log grows. Its own service applies what the hub stores.

use super::assets::{
    ASSET_UNAVAILABLE, AssetStore, AssetStoreError, INVALID_ASSET, StagedAsset, blocking,
    partition_held, write_staged,
};
use super::wire::{
    asset_from_wire, asset_key, bytes_from_wire, bytes_to_wire, hash_to_wire, ops_from_wire,
    ops_to_wire, vector_from_wire, vector_to_wire, writer_from_wire,
};
use ganbaru_handoff::protocol::{
    ControlMessage, SYNC_ASSET_CHUNK_BYTES, SyncAsset, SyncAssetKey, SyncRefusal, SyncRefusalCode,
    validate_coordinator_endpoints, validate_discovery_key,
};
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::manifest::Manifest;
use ganbaru_sync::{Engine, SpaceContext, StoreOutcome, StoreRefusal, local};
use ganbaru_sync_contracts::{Envelope, Seq, VersionVector, WriterId};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// How long a wait is held before the hub answers with its unchanged state. It stays below the
/// control timeout of the transport.
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(12);
/// How long a pending upload survives without a new chunk. A peer that disconnected mid-upload
/// restarts from the beginning, and its temporary file is removed with the next upload request
/// any peer makes.
const UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The vault database the hub serves.
pub struct HubVault {
    pub pool: SqlitePool,
    pub vault_id: String,
    /// Device id of the coordinator whose writers may revoke: the hub's own device.
    pub device_id: String,
    /// The vault's managed asset files.
    pub assets: Arc<dyn AssetStore>,
}

/// Opens the vault the hub serves, or `None` while it cannot replicate.
pub type OpenHubVault = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<Option<HubVault>, String>> + Send>>
        + Send
        + Sync,
>;

/// How linked devices reach the coordinator: the listener endpoints it reports now and the
/// vault's discovery key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubContact {
    pub endpoints: Vec<String>,
    pub discovery_key: String,
}

/// Reports the coordinator's contact for the vault with the given ID.
pub type HubContactSource = Arc<dyn Fn(&str) -> Result<HubContact, String> + Send + Sync>;

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
    /// The asset each peer is uploading; a peer uploads one at a time.
    uploads: Arc<Mutex<HashMap<String, PendingUpload>>>,
    /// The contact every sync state carries; without it the hub answers no state.
    contact: Option<HubContactSource>,
}

/// The received prefix of an upload, staged in a temporary file beside its final path.
struct PendingUpload {
    key: SyncAssetKey,
    staged: StagedAsset,
    /// When the last chunk arrived.
    touched: Instant,
}

impl SyncHub {
    pub fn new(open: OpenHubVault, changes: watch::Sender<u64>) -> Self {
        Self {
            open,
            changes,
            waits: Arc::default(),
            uploads: Arc::default(),
            contact: None,
        }
    }

    /// Sets where the hub reads the coordinator contact that each sync state reports.
    pub fn with_contact(mut self, contact: HubContactSource) -> Self {
        self.contact = Some(contact);
        self
    }

    /// The coordinator contact for `vault_id`, checked against the wire bounds so a bad source
    /// fails this request instead of the frame.
    fn contact(&self, vault_id: &str) -> Result<HubContact, HubError> {
        let source = self
            .contact
            .as_ref()
            .ok_or_else(|| HubError::unavailable("the coordinator cannot report its contact"))?;
        let contact = source(vault_id).map_err(HubError::unavailable)?;
        validate_coordinator_endpoints(&contact.endpoints)
            .and_then(|()| validate_discovery_key(&contact.discovery_key))
            .map_err(|error| HubError::failed(format!("coordinator contact: {error}")))?;
        Ok(contact)
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
            local::space_context(&mut conn, &vault_id, &vault.device_id)
                .await
                .map_err(|error| HubError::failed(format!("read sync space: {error}")))?
        };
        let Some(ctx) = ctx else {
            return Err(HubError::unavailable(
                "the hub vault has no person identity",
            ));
        };
        let assets = vault.assets;
        let hub = HubSession {
            engine: Engine::vault(),
            pool: vault.pool,
            ctx,
        };
        let manifest = hub.engine.manifest();
        match request {
            ControlMessage::SyncAssetQuery {
                assets: queried, ..
            } => asset_presence(&assets, manifest, &queried).await,
            ControlMessage::SyncAssetGet { asset, offset, .. } => {
                asset_chunk(&assets, manifest, &asset, offset).await
            }
            ControlMessage::SyncAssetPut {
                asset,
                offset,
                data,
                ..
            } => {
                let asset = upload_from_wire(&asset, manifest)?;
                let data = bytes_from_wire(&data).map_err(invalid)?;
                self.receive(&assets, &device_id, &asset, offset, data)
                    .await
            }
            ControlMessage::SyncHello { probe, .. } => {
                let probe = match probe {
                    Some(probe) => {
                        Some((writer_from_wire(&probe.writer).map_err(invalid)?, probe.seq))
                    }
                    None => None,
                };
                let contact = self.contact(&vault_id)?;
                hub.state(probe, contact).await
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
                let contact = self.contact(&vault_id)?;
                self.wait(&hub, &device_id, &known, contact).await
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
        contact: HubContact,
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
                break Ok(hub.state_message(&stored, None, contact));
            }
            match tokio::time::timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => break Ok(hub.state_message(&stored, None, contact)),
            }
        };
        if let Ok(mut waits) = self.waits.lock()
            && waits.get(device_id) == Some(&token)
        {
            waits.remove(device_id);
        }
        result
    }

    /// Adds an upload chunk of a peer. A chunk at offset 0 starts the upload over; a chunk
    /// that does not continue the pending upload is answered with where it does continue. The
    /// complete asset is verified against its reference before it is stored.
    async fn receive(
        &self,
        store: &Arc<dyn AssetStore>,
        device_id: &str,
        asset: &AssetRef,
        offset: u32,
        data: Vec<u8>,
    ) -> Result<ControlMessage, HubError> {
        let size = asset.byte_size();
        let key = asset_key(asset);
        let pending = self.take_upload(device_id)?;
        let pending = if offset == 0 {
            drop(pending);
            let held = {
                let asset = asset.clone();
                blocking(store, move |store| store.holds(&asset)).await
            }
            .map_err(store_failure)?;
            if held {
                return Ok(ControlMessage::SyncAssetReceived { received: size });
            }
            let staged = {
                let asset = asset.clone();
                blocking(store, move |store| store.stage(&asset)).await
            }
            .map_err(store_failure)?;
            PendingUpload {
                key,
                staged,
                touched: Instant::now(),
            }
        } else {
            match pending {
                Some(pending) if pending.key == key && pending.staged.received() == offset => {
                    pending
                }
                Some(pending) => {
                    let received = if pending.key == key {
                        pending.staged.received()
                    } else {
                        0
                    };
                    self.keep_upload(device_id, pending)?;
                    return Ok(ControlMessage::SyncAssetReceived { received });
                }
                None => return Ok(ControlMessage::SyncAssetReceived { received: 0 }),
            }
        };
        let staged = match write_staged(pending.staged, data).await {
            Ok(staged) => staged,
            Err(AssetStoreError::Mismatch(message)) => return Err(invalid(message)),
            Err(error) => return Err(store_failure(error)),
        };
        if !staged.is_complete() {
            let received = staged.received();
            self.keep_upload(
                device_id,
                PendingUpload {
                    key: pending.key,
                    staged,
                    touched: Instant::now(),
                },
            )?;
            return Ok(ControlMessage::SyncAssetReceived { received });
        }
        match blocking(store, move |store| store.commit(staged)).await {
            Ok(()) => Ok(ControlMessage::SyncAssetReceived { received: size }),
            Err(AssetStoreError::Mismatch(message)) => {
                Err(HubError::rejected(INVALID_ASSET, message))
            }
            Err(error) => Err(store_failure(error)),
        }
    }

    /// Removes and returns the pending upload of a peer, and removes uploads that went idle.
    /// Their temporary files are removed outside the lock.
    fn take_upload(&self, device_id: &str) -> Result<Option<PendingUpload>, HubError> {
        let now = Instant::now();
        let (pending, idle) = {
            let mut uploads = self
                .uploads
                .lock()
                .map_err(|_| HubError::failed("sync upload lock is unavailable"))?;
            let idle_peers: Vec<String> = uploads
                .iter()
                .filter(|(_, pending)| now.duration_since(pending.touched) >= UPLOAD_IDLE_TIMEOUT)
                .map(|(peer, _)| peer.clone())
                .collect();
            let idle: Vec<PendingUpload> = idle_peers
                .iter()
                .filter_map(|peer| uploads.remove(peer))
                .collect();
            (uploads.remove(device_id), idle)
        };
        drop(idle);
        Ok(pending)
    }

    /// Keeps the pending upload of a peer for its next chunk. A newer upload the peer started
    /// meanwhile wins, and this one is removed.
    fn keep_upload(&self, device_id: &str, pending: PendingUpload) -> Result<(), HubError> {
        let replaced = {
            let mut uploads = self
                .uploads
                .lock()
                .map_err(|_| HubError::failed("sync upload lock is unavailable"))?;
            if uploads.contains_key(device_id) {
                Some(pending)
            } else {
                uploads.insert(device_id.to_string(), pending);
                None
            }
        };
        drop(replaced);
        Ok(())
    }
}

/// Reads an uploaded asset reference. Only classes of domains a linked replica may write are
/// accepted, since no other device can reference the rest.
fn upload_from_wire(asset: &SyncAsset, manifest: &Manifest) -> Result<AssetRef, HubError> {
    match manifest.asset_class(&asset.class) {
        Some((domain, _)) if !domain.replica_writable => Err(invalid(format!(
            "sync asset class {} is not replica writable",
            asset.class
        ))),
        _ => asset_from_wire(asset, manifest).map_err(invalid),
    }
}

/// The queried assets the hub holds.
async fn asset_presence(
    store: &Arc<dyn AssetStore>,
    manifest: &Manifest,
    assets: &[SyncAsset],
) -> Result<ControlMessage, HubError> {
    let references = assets
        .iter()
        .map(|asset| asset_from_wire(asset, manifest))
        .collect::<Result<Vec<_>, _>>()
        .map_err(invalid)?;
    let (held, _) = partition_held(store, references)
        .await
        .map_err(store_failure)?;
    Ok(ControlMessage::SyncAssetPresence {
        held: held.iter().map(asset_key).collect(),
    })
}

/// A chunk of an asset the hub holds. The receiver verifies the assembled bytes.
async fn asset_chunk(
    store: &Arc<dyn AssetStore>,
    manifest: &Manifest,
    asset: &SyncAsset,
    offset: u32,
) -> Result<ControlMessage, HubError> {
    let asset = asset_from_wire(asset, manifest).map_err(invalid)?;
    let chunk = blocking(store, move |store| {
        store.read_chunk(&asset, offset, SYNC_ASSET_CHUNK_BYTES)
    })
    .await;
    match chunk {
        Ok(chunk) => Ok(ControlMessage::SyncAssetChunk {
            data: bytes_to_wire(&chunk),
        }),
        Err(AssetStoreError::Unavailable) => Err(HubError::rejected(
            ASSET_UNAVAILABLE,
            "the hub does not hold this asset",
        )),
        Err(error) => Err(store_failure(error)),
    }
}

fn store_failure(error: AssetStoreError) -> HubError {
    HubError::failed(format!("managed assets: {}", error.describe()))
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

    fn state_message(
        &self,
        stored: &VersionVector,
        probe_hash: Option<String>,
        contact: HubContact,
    ) -> ControlMessage {
        ControlMessage::SyncState {
            manifest_version: self.engine.manifest().version,
            stored: vector_to_wire(stored),
            probe_hash,
            coordinator_endpoints: contact.endpoints,
            discovery_key: contact.discovery_key,
        }
    }

    /// The hub's stored vector and its hash at the probed operation, when it holds it.
    async fn state(
        &self,
        probe: Option<(WriterId, u64)>,
        contact: HubContact,
    ) -> Result<ControlMessage, HubError> {
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
        Ok(self.state_message(&stored, probe_hash, contact))
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
        | StoreRefusal::Untrusted
        | StoreRefusal::UnauthorizedRevoke => (SyncRefusalCode::Invalid, seq),
        StoreRefusal::WriterLimit => (SyncRefusalCode::WriterLimit, seq),
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
