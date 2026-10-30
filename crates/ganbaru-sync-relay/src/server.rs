//! The relay's TLS listener and per-connection sessions.
//!
//! A session starts unauthenticated with small frames, names its space, and then either claims
//! the space, runs signed admin commands, or authenticates with an epoch credential. Every
//! signature covers the connection's TLS exporter, so a captured credential or admin command
//! cannot be replayed on another connection.

use crate::limits::{
    BudgetPermit, ConnectionBudget, IDLE_TIMEOUT, MAX_AUTHENTICATED_CONNECTIONS,
    MAX_AUTHENTICATED_CONNECTIONS_PER_SOURCE, MAX_PENDING_CONNECTIONS,
    MAX_PENDING_CONNECTIONS_PER_SOURCE, PRE_AUTH_DEADLINE, PRE_AUTH_FRAME_TIMEOUT, SourceKey,
    TLS_HANDSHAKE_TIMEOUT, WRITE_TIMEOUT,
};
use crate::store::{AdminChange, ClaimRequest, RelayStore, StoreError};
use ganbaru_handoff::protocol::relay::{
    MAX_RELAY_ADMIN_KEYS, RELAY_ADMIN_DOMAIN, RELAY_AUTH_DOMAIN, RELAY_EPOCH_DOMAIN,
    RELAY_EXPORTER_BYTES, RELAY_EXPORTER_LABEL, RELAY_PROTOCOL_VERSION, RELAY_WAIT_TIMEOUT,
    RelayAdminAction, RelayAdminCommand, RelayBinding, RelayEpochEntry, RelayRefusalCode,
    RelayRequest, RelayResponse, RelaySetupToken, RelayWriterRange, decode_relay_access_key,
    decode_relay_admin_key, decode_relay_asset, decode_relay_bytes, decode_relay_signature,
    decode_relay_space, decode_relay_writer, encode_relay_asset, encode_relay_bytes,
};
use ganbaru_handoff::protocol::{
    MAX_CONTROL_BYTES, MAX_UNAUTHENTICATED_CONTROL_BYTES, SyncSeq, read_frame, write_frame,
};
use ganbaru_sync_contracts::relay::{MAX_RELAY_ASSET_CHUNK_BYTES, MAX_RELAY_RECORD_BYTES};
use ganbaru_sync_contracts::{RelayAssetChunk, RelayRecord, SpaceId, WriterId};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, RwLock};
use tokio::time::Instant;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// How often the relay trims every space by age and drops expired setup tokens.
pub const RETENTION_SWEEP_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Pause after a failed accept, so a full descriptor table does not spin the loop.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(200);

/// Current Unix time in milliseconds.
pub fn unix_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Connection budgets of a relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionLimits {
    /// Unauthenticated connections at once.
    pub pending: usize,
    /// Unauthenticated connections per source.
    pub pending_per_source: usize,
    /// Authenticated connections at once.
    pub authenticated: usize,
    /// Authenticated connections per source.
    pub authenticated_per_source: usize,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            pending: MAX_PENDING_CONNECTIONS,
            pending_per_source: MAX_PENDING_CONNECTIONS_PER_SOURCE,
            authenticated: MAX_AUTHENTICATED_CONNECTIONS,
            authenticated_per_source: MAX_AUTHENTICATED_CONNECTIONS_PER_SOURCE,
        }
    }
}

struct Shared {
    store: RelayStore,
    waits: Mutex<HashMap<SpaceId, Arc<Notify>>>,
    /// Per space, a counter of admin changes that replace epoch keys (an epoch repair or a space
    /// removal). Authentication and every authenticated request hold it for reading, and those
    /// changes hold it for writing while they commit and then advance it, so a connection that
    /// verified against a replaced key can never act after the change.
    generations: Mutex<HashMap<SpaceId, Arc<RwLock<u64>>>>,
    pending: ConnectionBudget,
    authenticated: ConnectionBudget,
}

impl Shared {
    fn generation(&self, space: &SpaceId) -> Arc<RwLock<u64>> {
        let mut generations = self
            .generations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(generations.entry(*space).or_default())
    }

    fn notifier(&self, space: &SpaceId) -> Arc<Notify> {
        let mut waits = self
            .waits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(waits.entry(*space).or_default())
    }

    fn wake(&self, space: &SpaceId) {
        let waits = self
            .waits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(notify) = waits.get(space) {
            notify.notify_waiters();
        }
    }
}

/// A bound relay listener.
pub struct RelayServer {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    shared: Arc<Shared>,
}

impl RelayServer {
    /// Serves `store` on `listener` with the TLS identity in `tls`.
    pub fn new(
        listener: TcpListener,
        tls: Arc<rustls::ServerConfig>,
        store: RelayStore,
        limits: ConnectionLimits,
    ) -> Self {
        Self {
            listener,
            acceptor: TlsAcceptor::from(tls),
            shared: Arc::new(Shared {
                store,
                waits: Mutex::new(HashMap::new()),
                generations: Mutex::new(HashMap::new()),
                pending: ConnectionBudget::new(limits.pending, limits.pending_per_source),
                authenticated: ConnectionBudget::new(
                    limits.authenticated,
                    limits.authenticated_per_source,
                ),
            }),
        }
    }

    /// Address the listener is bound to.
    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.listener
            .local_addr()
            .map_err(|error| format!("read relay listener address: {error}"))
    }

    /// Accepts connections until the task is dropped, and trims storage periodically.
    pub async fn run(self) {
        let mut sweep = tokio::time::interval(RETENTION_SWEEP_INTERVAL);
        loop {
            tokio::select! {
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, peer)) => self.admit(stream, peer),
                    Err(error) => {
                        eprintln!("relay accept failed: {error}");
                        tokio::time::sleep(ACCEPT_BACKOFF).await;
                    }
                },
                _ = sweep.tick() => {
                    if let Err(error) = self.shared.store.sweep(unix_time_ms()).await {
                        eprintln!("relay retention sweep failed: {error}");
                    }
                }
            }
        }
    }

    fn admit(&self, stream: TcpStream, peer: SocketAddr) {
        let source = SourceKey::of(peer.ip());
        // Without a pending slot the connection closes before any TLS work.
        let Some(permit) = self.shared.pending.try_acquire(source) else {
            return;
        };
        let acceptor = self.acceptor.clone();
        let shared = Arc::clone(&self.shared);
        tokio::spawn(async move {
            let Ok(Ok(tls)) =
                tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await
            else {
                return;
            };
            let Ok(exporter) = tls.get_ref().1.export_keying_material(
                [0u8; RELAY_EXPORTER_BYTES],
                RELAY_EXPORTER_LABEL,
                None,
            ) else {
                return;
            };
            let mut session = Session {
                shared,
                source,
                exporter,
                phase: Phase::Start,
                permit,
                deadline: Instant::now() + PRE_AUTH_DEADLINE,
            };
            session.run(tls).await;
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Start,
    Space(SpaceId),
    /// `generation` is the space's epoch generation when the credential verified; see
    /// [`Shared::generations`].
    Authenticated {
        space: SpaceId,
        epoch: u32,
        generation: u64,
    },
}

/// Whether a session continues after a response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    Continue,
    Close,
}

struct Session {
    shared: Arc<Shared>,
    source: SourceKey,
    exporter: [u8; RELAY_EXPORTER_BYTES],
    phase: Phase,
    permit: BudgetPermit,
    deadline: Instant,
}

fn refused(code: RelayRefusalCode, message: &str) -> (RelayResponse, Next) {
    let next = match code {
        RelayRefusalCode::Version
        | RelayRefusalCode::Authentication
        | RelayRefusalCode::Token
        | RelayRefusalCode::Busy => Next::Close,
        _ => Next::Continue,
    };
    (RelayResponse::refused(code, message), next)
}

fn store_failure(error: StoreError) -> (RelayResponse, Next) {
    match error {
        StoreError::Refused(code, reason) => refused(code, reason),
        StoreError::StaleAdminCounter(last) => {
            (RelayResponse::stale_admin_counter(last), Next::Continue)
        }
        StoreError::Database(error) => {
            eprintln!("relay database error: {error}");
            (
                RelayResponse::refused(RelayRefusalCode::Busy, "relay storage failed"),
                Next::Close,
            )
        }
    }
}

fn bad_request(message: &str) -> (RelayResponse, Next) {
    refused(RelayRefusalCode::BadRequest, message)
}

fn parse_vector(have: &[SyncSeq]) -> Result<HashMap<WriterId, u64>, String> {
    let mut vector = HashMap::with_capacity(have.len());
    for entry in have {
        let writer = decode_relay_writer(&entry.writer)?;
        if vector.insert(writer, entry.seq).is_some() {
            return Err("relay vector repeats a writer".to_string());
        }
    }
    Ok(vector)
}

impl Session {
    async fn run(&mut self, mut tls: TlsStream<TcpStream>) {
        loop {
            let authenticated = matches!(self.phase, Phase::Authenticated { .. });
            let (limit, wait) = if authenticated {
                (MAX_CONTROL_BYTES, IDLE_TIMEOUT)
            } else {
                let left = self.deadline.saturating_duration_since(Instant::now());
                (
                    MAX_UNAUTHENTICATED_CONTROL_BYTES,
                    left.min(PRE_AUTH_FRAME_TIMEOUT),
                )
            };
            let request =
                match tokio::time::timeout(wait, read_frame::<_, RelayRequest>(&mut tls, limit))
                    .await
                {
                    Ok(Ok(request)) => request,
                    Ok(Err(error)) => {
                        let (response, _) = bad_request(&error);
                        let _ =
                            tokio::time::timeout(WRITE_TIMEOUT, write_frame(&mut tls, &response))
                                .await;
                        break;
                    }
                    Err(_) => break,
                };
            let (response, next) = self.handle(request).await;
            match tokio::time::timeout(WRITE_TIMEOUT, write_frame(&mut tls, &response)).await {
                Ok(Ok(())) => {}
                _ => break,
            }
            if next == Next::Close {
                break;
            }
        }
        let _ = tokio::time::timeout(WRITE_TIMEOUT, tls.shutdown()).await;
    }

    fn binding(&self, space: SpaceId) -> RelayBinding {
        RelayBinding {
            exporter: self.exporter,
            space,
        }
    }

    fn space(&self) -> Option<SpaceId> {
        match self.phase {
            Phase::Start => None,
            Phase::Space(space) | Phase::Authenticated { space, .. } => Some(space),
        }
    }

    async fn handle(&mut self, request: RelayRequest) -> (RelayResponse, Next) {
        match (&self.phase, request) {
            (
                Phase::Start,
                RelayRequest::Hello {
                    protocol_version,
                    space,
                },
            ) => {
                if protocol_version != RELAY_PROTOCOL_VERSION {
                    return refused(RelayRefusalCode::Version, "unsupported relay protocol");
                }
                match decode_relay_space(&space) {
                    Ok(space) => {
                        self.phase = Phase::Space(space);
                        (
                            RelayResponse::Welcome {
                                protocol_version: RELAY_PROTOCOL_VERSION,
                            },
                            Next::Continue,
                        )
                    }
                    Err(error) => bad_request(&error),
                }
            }
            (Phase::Space(space), RelayRequest::Auth { epoch, signature }) => {
                let space = *space;
                self.authenticate(space, epoch, &signature).await
            }
            (
                Phase::Space(space),
                RelayRequest::Claim {
                    token,
                    epoch,
                    access_key,
                    admins,
                    counter,
                    signer,
                    signature,
                },
            ) => {
                let space = *space;
                self.claim(
                    space,
                    &token,
                    epoch,
                    &access_key,
                    &admins,
                    counter,
                    &signer,
                    &signature,
                )
                .await
            }
            (
                Phase::Space(_) | Phase::Authenticated { .. },
                RelayRequest::Admin {
                    action,
                    counter,
                    signer,
                    signature,
                },
            ) => {
                let Some(space) = self.space() else {
                    return bad_request("admin needs a space");
                };
                self.admin(space, &action, counter, &signer, &signature)
                    .await
            }
            (
                Phase::Authenticated {
                    space,
                    epoch,
                    generation,
                },
                request,
            ) => {
                let (space, epoch, generation) = (*space, *epoch, *generation);
                self.authenticated(space, epoch, generation, request).await
            }
            (Phase::Space(_), _) => refused(
                RelayRefusalCode::Unauthenticated,
                "authenticate before this request",
            ),
            (Phase::Start, _) => bad_request("send hello first"),
        }
    }

    async fn authenticate(
        &mut self,
        space: SpaceId,
        epoch: u32,
        signature: &str,
    ) -> (RelayResponse, Next) {
        let signature = match decode_relay_signature(signature) {
            Ok(signature) => signature,
            Err(error) => return bad_request(&error),
        };
        let lock = self.shared.generation(&space);
        let generation = lock.read().await;
        let credential = match self.shared.store.credential(&space, epoch).await {
            Ok(credential) => credential,
            Err(error) => return store_failure(error),
        };
        let payload = self.binding(space).auth_payload(epoch);
        if !credential
            .access_key
            .verify(RELAY_AUTH_DOMAIN, &payload, &signature)
        {
            return refused(
                RelayRefusalCode::Authentication,
                "access signature is invalid",
            );
        }
        let Some(permit) = self.shared.authenticated.try_acquire(self.source) else {
            return refused(RelayRefusalCode::Busy, "relay is at its connection limit");
        };
        // Replacing the pending permit releases its slot.
        self.permit = permit;
        self.phase = Phase::Authenticated {
            space,
            epoch,
            generation: *generation,
        };
        (
            RelayResponse::Authenticated {
                epoch,
                current_epoch: credential.current_epoch,
                stale: epoch < credential.current_epoch,
            },
            Next::Continue,
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn claim(
        &mut self,
        space: SpaceId,
        token: &RelaySetupToken,
        epoch: u32,
        access_key: &str,
        admins: &[String],
        counter: u64,
        signer: &str,
        signature: &str,
    ) -> (RelayResponse, Next) {
        let parsed = (|| -> Result<_, String> {
            let access_key = decode_relay_access_key(access_key)?;
            let admins = admins
                .iter()
                .map(|admin| decode_relay_admin_key(admin))
                .collect::<Result<Vec<_>, _>>()?;
            let signer = decode_relay_admin_key(signer)?;
            let signature = decode_relay_signature(signature)?;
            Ok((access_key, admins, signer, signature))
        })();
        let (access_key, admins, signer, signature) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => return bad_request(&error),
        };
        if admins.len() > MAX_RELAY_ADMIN_KEYS || !admins.contains(&signer) {
            return refused(
                RelayRefusalCode::AdminSignature,
                "signer is not in the admin set",
            );
        }
        let payload = self.binding(space).admin_payload(
            counter,
            &RelayAdminCommand::Claim {
                epoch,
                access_key: &access_key,
                admins: &admins,
            },
        );
        if !signer.verify(RELAY_ADMIN_DOMAIN, &payload, &signature) {
            return refused(
                RelayRefusalCode::AdminSignature,
                "admin signature is invalid",
            );
        }
        let claim = ClaimRequest {
            space,
            token,
            epoch,
            access_key,
            admins: &admins,
            counter,
        };
        match self.shared.store.claim(&claim, unix_time_ms()).await {
            Ok(()) => (RelayResponse::Claimed { epoch }, Next::Continue),
            Err(error) => store_failure(error),
        }
    }

    async fn admin(
        &mut self,
        space: SpaceId,
        action: &RelayAdminAction,
        counter: u64,
        signer: &str,
        signature: &str,
    ) -> (RelayResponse, Next) {
        let parsed = (|| -> Result<_, String> {
            let change = match action {
                RelayAdminAction::AddKey { key } => {
                    AdminChange::AddKey(decode_relay_admin_key(key)?)
                }
                RelayAdminAction::RemoveKey { key } => {
                    AdminChange::RemoveKey(decode_relay_admin_key(key)?)
                }
                RelayAdminAction::Wipe => AdminChange::Wipe,
                RelayAdminAction::RemoveSpace => AdminChange::RemoveSpace,
                RelayAdminAction::RelayEpochRepair { epoch, access_key } => {
                    AdminChange::RelayEpochRepair {
                        epoch: *epoch,
                        access_key: decode_relay_access_key(access_key)?,
                    }
                }
            };
            Ok((
                change,
                decode_relay_admin_key(signer)?,
                decode_relay_signature(signature)?,
            ))
        })();
        let (change, signer, signature) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => return bad_request(&error),
        };
        let command = match &change {
            AdminChange::AddKey(key) => RelayAdminCommand::AddKey(key),
            AdminChange::RemoveKey(key) => RelayAdminCommand::RemoveKey(key),
            AdminChange::Wipe => RelayAdminCommand::Wipe,
            AdminChange::RemoveSpace => RelayAdminCommand::RemoveSpace,
            AdminChange::RelayEpochRepair { epoch, access_key } => {
                RelayAdminCommand::RelayEpochRepair {
                    epoch: *epoch,
                    access_key,
                }
            }
        };
        let payload = self.binding(space).admin_payload(counter, &command);
        if !signer.verify(RELAY_ADMIN_DOMAIN, &payload, &signature) {
            return refused(
                RelayRefusalCode::AdminSignature,
                "admin signature is invalid",
            );
        }
        let replaces_keys = matches!(
            change,
            AdminChange::RelayEpochRepair { .. } | AdminChange::RemoveSpace
        );
        let lock = self.shared.generation(&space);
        let mut generation = if replaces_keys {
            Some(lock.write().await)
        } else {
            None
        };
        let applied = self
            .shared
            .store
            .apply_admin(&space, &signer, counter, change, unix_time_ms())
            .await;
        match applied {
            Ok(()) => {
                if let Some(generation) = generation.as_mut() {
                    **generation = generation.wrapping_add(1);
                }
                drop(generation);
                self.shared.wake(&space);
                (RelayResponse::AdminApplied { counter }, Next::Continue)
            }
            Err(error) => store_failure(error),
        }
    }

    async fn authenticated(
        &mut self,
        space: SpaceId,
        epoch: u32,
        generation: u64,
        request: RelayRequest,
    ) -> (RelayResponse, Next) {
        let lock = self.shared.generation(&space);
        let current = lock.read().await;
        if *current != generation {
            return refused(
                RelayRefusalCode::Authentication,
                "an admin replaced the space's epoch keys; authenticate again",
            );
        }
        // A wait only reads, so it must not hold off an admin change for its whole timeout.
        let _generation = (!matches!(request, RelayRequest::Wait { .. })).then_some(current);
        let store = &self.shared.store;
        let result = match request {
            RelayRequest::State => store.state(&space).await.map(|state| RelayResponse::State {
                current_epoch: state.current_epoch,
                writers: state
                    .writers
                    .iter()
                    .map(|range| RelayWriterRange {
                        writer: range.writer.to_hex(),
                        floor: range.floor,
                        next: range.next,
                    })
                    .collect(),
                stored_bytes: state.stored_bytes,
                quota_bytes: store.retention().quota_bytes,
                admins: state
                    .admins
                    .iter()
                    .map(|admin| encode_relay_bytes(admin.as_bytes()))
                    .collect(),
                admin_counter: state.admin_counter,
            }),
            RelayRequest::Push { records } => {
                let mut decoded = Vec::with_capacity(records.len());
                for text in &records {
                    let parsed = decode_relay_bytes("relay record", text, MAX_RELAY_RECORD_BYTES)
                        .and_then(|bytes| {
                            RelayRecord::decode(&bytes)
                                .map(|record| (*record.header(), bytes))
                                .map_err(|error| format!("relay record: {error}"))
                        });
                    match parsed {
                        Ok((header, bytes)) if header.space == space && header.epoch == epoch => {
                            decoded.push((header, bytes));
                        }
                        Ok(_) => {
                            return refused(
                                RelayRefusalCode::BadRecord,
                                "record names another space or epoch",
                            );
                        }
                        Err(error) => return refused(RelayRefusalCode::BadRecord, &error),
                    }
                }
                let result = store.push(&space, epoch, &decoded, unix_time_ms()).await;
                if result.is_ok() {
                    self.shared.wake(&space);
                }
                result.map(|outcomes| RelayResponse::Pushed { outcomes })
            }
            RelayRequest::Pull { have } => match parse_vector(&have) {
                Ok(have) => store.pull(&space, epoch, &have).await.map(records_response),
                Err(error) => return bad_request(&error),
            },
            RelayRequest::Wait { have } => match parse_vector(&have) {
                Ok(have) => self.wait(space, epoch, &have).await,
                Err(error) => return bad_request(&error),
            },
            RelayRequest::Epochs { after } => {
                store
                    .epochs(&space, after)
                    .await
                    .map(|page| RelayResponse::Epochs {
                        current_epoch: page.current_epoch,
                        entries: page
                            .entries
                            .iter()
                            .map(|(epoch, entry)| RelayEpochEntry {
                                epoch: *epoch,
                                entry: encode_relay_bytes(entry),
                            })
                            .collect(),
                        more: page.more,
                    })
            }
            RelayRequest::RegisterEpoch {
                epoch: new_epoch,
                access_key,
                entry,
                signature,
            } => {
                let parsed = (|| -> Result<_, String> {
                    Ok((
                        decode_relay_access_key(&access_key)?,
                        decode_relay_bytes(
                            "relay epoch entry",
                            &entry,
                            ganbaru_handoff::protocol::relay::MAX_RELAY_EPOCH_ENTRY_BYTES,
                        )?,
                        decode_relay_signature(&signature)?,
                    ))
                })();
                let (access_key, entry, signature) = match parsed {
                    Ok(parsed) => parsed,
                    Err(error) => return bad_request(&error),
                };
                let payload = self.binding(space).epoch_payload(new_epoch);
                if !access_key.verify(RELAY_EPOCH_DOMAIN, &payload, &signature) {
                    return refused(
                        RelayRefusalCode::Authentication,
                        "epoch signature does not prove the next key",
                    );
                }
                let result = store
                    .register_epoch(
                        &space,
                        epoch,
                        new_epoch,
                        &access_key,
                        &entry,
                        unix_time_ms(),
                    )
                    .await;
                if result.is_ok() {
                    self.shared.wake(&space);
                }
                result.map(|()| RelayResponse::EpochRegistered { epoch: new_epoch })
            }
            RelayRequest::AssetQuery { assets } => {
                let parsed = assets
                    .iter()
                    .map(|asset| decode_relay_asset(asset))
                    .collect::<Result<Vec<_>, _>>();
                match parsed {
                    Ok(assets) => {
                        store
                            .complete_assets(&space, epoch, &assets)
                            .await
                            .map(|assets| RelayResponse::AssetPresence {
                                assets: assets.iter().map(encode_relay_asset).collect(),
                            })
                    }
                    Err(error) => return bad_request(&error),
                }
            }
            RelayRequest::AssetPut { chunk } => {
                let parsed =
                    decode_relay_bytes("relay asset chunk", &chunk, MAX_RELAY_ASSET_CHUNK_BYTES)
                        .and_then(|bytes| {
                            RelayAssetChunk::decode(&bytes)
                                .map(|chunk| (*chunk.header(), bytes))
                                .map_err(|error| format!("relay asset chunk: {error}"))
                        });
                match parsed {
                    Ok((header, bytes)) => store
                        .put_asset_chunk(&space, epoch, &header, &bytes, unix_time_ms())
                        .await
                        .map(|outcome| RelayResponse::AssetStored { outcome }),
                    Err(error) => return refused(RelayRefusalCode::BadRecord, &error),
                }
            }
            RelayRequest::AssetGet { asset, index } => match decode_relay_asset(&asset) {
                Ok(asset) => store
                    .asset_chunk(&space, epoch, &asset, index, unix_time_ms())
                    .await
                    .map(|chunk| RelayResponse::AssetChunk {
                        chunk: encode_relay_bytes(&chunk),
                    }),
                Err(error) => return bad_request(&error),
            },
            RelayRequest::Hello { .. } | RelayRequest::Auth { .. } | RelayRequest::Claim { .. } => {
                return bad_request("the connection is already authenticated");
            }
            RelayRequest::Admin { .. } => return bad_request("admin is handled before"),
        };
        match result {
            Ok(response) => (response, Next::Continue),
            Err(error) => store_failure(error),
        }
    }

    async fn wait(
        &self,
        space: SpaceId,
        epoch: u32,
        have: &HashMap<WriterId, u64>,
    ) -> Result<RelayResponse, StoreError> {
        let notify = self.shared.notifier(&space);
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self
            .shared
            .store
            .pull(&space, epoch, have)
            .await?
            .records
            .is_empty()
        {
            return Ok(RelayResponse::Waited { ready: true });
        }
        if tokio::time::timeout(RELAY_WAIT_TIMEOUT, notified)
            .await
            .is_err()
        {
            return Ok(RelayResponse::Waited { ready: false });
        }
        let ready = !self
            .shared
            .store
            .pull(&space, epoch, have)
            .await?
            .records
            .is_empty();
        Ok(RelayResponse::Waited { ready })
    }
}

fn records_response(page: crate::store::PullPage) -> RelayResponse {
    RelayResponse::Records {
        records: page
            .records
            .iter()
            .map(|record| encode_relay_bytes(record))
            .collect(),
        behind_floor: page
            .behind_floor
            .iter()
            .map(|(writer, floor)| SyncSeq {
                writer: writer.to_hex(),
                seq: *floor,
            })
            .collect(),
        more: page.more,
    }
}
