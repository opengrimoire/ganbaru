//! The device side of the encrypted relay. One pass connects to the relay the space's server
//! configuration names, authenticates with the access key of the newest key epoch this
//! replica holds, keeps the relay's epochs and admin set in step with the verified
//! authorization chain, and moves sealed records and asset chunks both ways.
//!
//! The relay is untrusted. Every pulled frame is bounded before it is decoded, every record and
//! chunk authenticates under a key this replica opened from its own envelope before it is used,
//! and opened operations reach the store only through the verified operation path. Epoch
//! entries the relay serves are offered to the record store, which keeps only what verifies.

use crate::assets::{AssetStore, AssetStoreError, blocking, partition_held, write_staged};
use ganbaru_handoff::dial::{LoopbackPolicy, authorize_server_dial_target};
use ganbaru_handoff::pairing::certificate_fingerprint;
use ganbaru_handoff::protocol::relay::{
    MAX_RELAY_ADMIN_KEYS, MAX_RELAY_ASSET_QUERY, MAX_RELAY_EPOCH_ENTRY_BYTES,
    MAX_RELAY_PAGE_RECORDS, RELAY_ADMIN_DOMAIN, RELAY_AUTH_DOMAIN, RELAY_EPOCH_DOMAIN,
    RELAY_EXPORTER_BYTES, RELAY_EXPORTER_LABEL, RELAY_PAGE_BYTES, RELAY_PROTOCOL_VERSION,
    RELAY_WAIT_TIMEOUT, RelayAdminAction, RelayAdminCommand, RelayAssetOutcome, RelayBinding,
    RelayPushOutcome, RelayRefusalCode, RelayRequest, RelayResponse, RelaySetupToken,
    RelayWriterRange, decode_relay_admin_key, decode_relay_asset, decode_relay_bytes,
    encode_relay_asset, encode_relay_bytes,
};
use ganbaru_handoff::protocol::{MAX_CONTROL_BYTES, SyncSeq, read_frame, write_frame};
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::{
    AuthorizationView, Engine, RelayKeys, SpaceContext, SpaceKeyHolder, StoreOutcome, StoreRefusal,
    SyncError,
};
use ganbaru_sync_contracts::bounds::{MAX_RELAY_ASSET_BYTES, RELAY_ASSET_CHUNK_BYTES};
use ganbaru_sync_contracts::ids::parse_hex;
use ganbaru_sync_contracts::relay::{MAX_RELAY_ASSET_CHUNK_BYTES, MAX_RELAY_RECORD_BYTES};
use ganbaru_sync_contracts::{
    AdminKeyPair, AdminPublicKey, DeviceRole, Envelope, RelayAccessKeyPair, RelayAssetId,
    ServerMode, SignedCertificate, SpaceId, VersionVector, WriterId, relay_asset_chunk_count,
};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use zeroize::Zeroizing;

/// Bound on connecting to one relay address, the TLS handshake included.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Bound on one request and its answer; a wait answers within [`RELAY_WAIT_TIMEOUT`].
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
/// Name presented in the TLS handshake. The pinned fingerprint, not the name, identifies the
/// relay.
const RELAY_SERVER_NAME: &str = "ganbaru-relay.local";
/// Epoch steps one pass takes before it stops, so a relay that keeps moving cannot hold a pass.
const MAX_EPOCH_STEPS: usize = 16;
/// Operation bytes requested from the log for one push page. Sealing adds the certificate and
/// framing to each operation, and the page splits again at the relay's text bound.
const PUSH_READ_BYTES: usize = RELAY_PAGE_BYTES / 2;
/// Asset size a metered connection still moves; larger files wait for an unmetered one.
pub const METERED_MAX_ASSET_BYTES: u32 = 1024 * 1024;
/// Asset size an unmetered connection moves, the relay's own bound.
pub const UNMETERED_MAX_ASSET_BYTES: u32 = {
    assert!(MAX_RELAY_ASSET_BYTES <= u32::MAX as usize);
    MAX_RELAY_ASSET_BYTES as u32
};

/// The base64url length of `bytes` bytes, as the relay bounds record text.
const fn base64_length(bytes: usize) -> usize {
    (bytes * 4).div_ceil(3)
}

const MAX_PUSH_TEXT_BYTES: usize = base64_length(RELAY_PAGE_BYTES);

/// Where the space's relay listens and the certificate it must present.
#[derive(Clone, PartialEq, Eq)]
pub struct RelayTarget {
    /// Addresses as `host:port`, tried in order.
    pub endpoints: Vec<String>,
    /// SHA-256 of the relay's DER certificate.
    pub certificate_fingerprint: [u8; 32],
}

impl std::fmt::Debug for RelayTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayTarget")
            .field("endpoints", &self.endpoints)
            .finish_non_exhaustive()
    }
}

impl RelayTarget {
    /// The relay the selected server configuration names, `None` when the space has no server
    /// or its server runs as an anchor.
    pub fn of(view: &AuthorizationView) -> Option<Self> {
        let server = view.server.as_ref()?;
        (server.mode == ServerMode::Relay).then(|| Self {
            endpoints: server.endpoints.clone(),
            certificate_fingerprint: server.certificate_fingerprint,
        })
    }
}

/// Bounds of one relay pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayLimits {
    /// Bytes after which a direction stops, normally [`crate::client::MAX_EXCHANGE_BYTES`].
    pub max_bytes: usize,
    /// Largest asset this pass moves; larger ones wait, see [`METERED_MAX_ASSET_BYTES`].
    pub max_asset_bytes: u32,
    /// Whether loopback relay addresses are dialed; release builds refuse them.
    pub loopback: LoopbackPolicy,
}

/// What one relay pass did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelayPass {
    /// Operations the relay stored from this pass.
    pub pushed: usize,
    /// Operations this replica stored from the relay.
    pub pulled: usize,
    /// Operations this replica holds that the relay refused to take, which wait for a direct
    /// connection or a later pass.
    pub undelivered: u64,
    /// Asset files the relay stored from this pass.
    pub uploaded: usize,
    /// Asset files this replica stored from the relay.
    pub downloaded: usize,
    /// Asset files left for an unmetered connection.
    pub deferred_assets: usize,
    /// Epochs this pass registered on the relay.
    pub registered: Vec<u32>,
    /// The epoch an admin repair restored on the relay, when this pass repaired one.
    pub repaired: Option<u32>,
    /// Epoch the pass authenticated with last.
    pub epoch: u32,
    /// Encrypted bytes the relay stores for the space, and its quota.
    pub stored_bytes: u64,
    pub quota_bytes: u64,
    /// A direction stopped at its bound with more to move, or pulled records
    /// may reference assets that become downloadable only after they apply.
    pub more: bool,
    /// Why part of the pass stopped.
    pub problem: Option<RelayProblem>,
    /// The log text behind [`Self::problem`].
    pub detail: Option<String>,
}

impl RelayPass {
    fn note(&mut self, problem: RelayProblem, detail: String) {
        if self.problem.is_none() {
            self.problem = Some(problem);
            self.detail = Some(detail);
        }
    }
}

/// Why a relay pass, or part of it, stopped. The interface shows each as localized text; the
/// detail behind it stays in the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RelayProblem {
    /// No relay address answered, the pinned certificate did not match, or the relay did not
    /// answer in time.
    Unreachable,
    /// The relay speaks another protocol version; one side needs an update.
    Incompatible,
    /// The relay holds no space with this id: it was never claimed or an admin removed it.
    NotClaimed,
    /// The relay already holds this space, so a setup code cannot claim it again.
    AlreadyClaimed,
    /// The setup code is unknown, expired, or already used.
    SetupCodeRefused,
    /// The relay refused this device's access key.
    AccessRefused,
    /// The relay refused an admin command of this device.
    AdminRefused,
    /// The relay is at its connection budget.
    Busy,
    /// The space holds the most writers the relay allows.
    WriterLimit,
    /// The relay refused a request for another reason.
    Refused,
    /// This device holds no key epoch of the space yet.
    NoKeys,
    /// The relay's epoch did not settle within one pass.
    EpochUnsettled,
    /// The relay is at a key epoch this device cannot open yet.
    EpochAhead,
    /// The relay lacks the records before a key epoch, so members behind it wait for a direct
    /// connection.
    RelayBehind,
    /// The relay's admin set is full.
    AdminSetFull,
    /// The relay holds a gap before this device's operations.
    Gap,
    /// The relay trimmed operations this device lacks.
    Trimmed,
    /// This device refused a record or key epoch record the relay served.
    RecordRefused,
    /// The relay holds a fork of this device's writer.
    Fork,
    /// The relay reported more to pull but sent nothing new.
    NoProgress,
    /// The relay or this device refused an asset file.
    AssetRefused,
    /// A local fault on this device.
    Failed,
}

impl RelayProblem {
    /// The problem a relay refusal reports.
    fn of_refusal(code: RelayRefusalCode) -> Self {
        match code {
            RelayRefusalCode::Version => Self::Incompatible,
            RelayRefusalCode::UnknownSpace => Self::NotClaimed,
            RelayRefusalCode::SpaceExists => Self::AlreadyClaimed,
            RelayRefusalCode::Token => Self::SetupCodeRefused,
            RelayRefusalCode::Unauthenticated
            | RelayRefusalCode::StaleEpoch
            | RelayRefusalCode::UnknownEpoch
            | RelayRefusalCode::Authentication => Self::AccessRefused,
            RelayRefusalCode::AdminSignature
            | RelayRefusalCode::AdminCounter
            | RelayRefusalCode::AdminSet => Self::AdminRefused,
            RelayRefusalCode::Busy => Self::Busy,
            RelayRefusalCode::WriterLimit => Self::WriterLimit,
            RelayRefusalCode::BadRequest
            | RelayRefusalCode::BadRecord
            | RelayRefusalCode::NotFound
            | RelayRefusalCode::AssetMismatch => Self::Refused,
        }
    }

    /// Whether a fault reporting this problem means the relay answered, so it counts as
    /// contact with the relay.
    pub fn relay_answered(self) -> bool {
        matches!(
            self,
            Self::Incompatible
                | Self::NotClaimed
                | Self::AlreadyClaimed
                | Self::SetupCodeRefused
                | Self::AccessRefused
                | Self::AdminRefused
                | Self::Busy
                | Self::WriterLimit
                | Self::Refused
        )
    }
}

/// A relay fault: the problem the interface reports and the detail the log keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayError {
    pub problem: RelayProblem,
    pub detail: String,
}

impl RelayError {
    /// A fault reporting `problem`.
    pub fn new(problem: RelayProblem, detail: impl Into<String>) -> Self {
        Self {
            problem,
            detail: detail.into(),
        }
    }
}

/// A local fault, known only by its text.
impl From<String> for RelayError {
    fn from(detail: String) -> Self {
        Self::new(RelayProblem::Failed, detail)
    }
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for RelayError {}

/// One replica's view for a relay pass.
pub struct RelaySession<'a> {
    pub engine: &'a Engine,
    pub pool: &'a SqlitePool,
    pub ctx: &'a SpaceContext,
    /// The installation's keys, which open its envelopes of each key epoch.
    pub holder: &'a dyn SpaceKeyHolder,
    /// The installation's admin key, which keeps the relay's admin set and repairs a wrong
    /// epoch registration while this device is a live admin.
    pub admin: Option<&'a AdminKeyPair>,
    /// Every writer this installation sealed with.
    pub own_writers: &'a BTreeSet<WriterId>,
    /// Devices this installation quarantined, whose writers' operations it does not store.
    pub quarantined: &'a BTreeSet<String>,
    /// The vault's managed asset files; without it only records move.
    pub assets: Option<&'a Arc<dyn AssetStore>>,
    pub limits: RelayLimits,
}

/// How an authentication attempt ended.
enum Authenticated {
    /// The relay accepted the credential; `current` is its current epoch.
    Session {
        connection: Box<Connection>,
        current: u32,
    },
    /// The relay is at an older epoch than the one offered.
    UnknownEpoch,
    /// The relay holds another access key for the epoch.
    KeyMismatch,
}

impl RelaySession<'_> {
    /// Runs one pass against `target`. Errors are transport or local faults; refusals of
    /// particular records or assets are reported in [`RelayPass::problem`].
    pub async fn pass(&self, target: &RelayTarget) -> Result<RelayPass, RelayError> {
        let mut pass = RelayPass::default();
        let mut keys = self.open_keys().await?;
        let Some(mut epoch) = keys.newest_epoch() else {
            return Err(RelayError::new(
                RelayProblem::NoKeys,
                "this device holds no key epoch of the space yet",
            ));
        };
        let mut steps = 0usize;
        let (mut connection, current) = loop {
            steps += 1;
            if steps > MAX_EPOCH_STEPS {
                return Err(RelayError::new(
                    RelayProblem::EpochUnsettled,
                    "the relay's epoch did not settle within one pass",
                ));
            }
            match self.authenticate(target, &keys, epoch).await? {
                Authenticated::Session {
                    connection,
                    current,
                } if current == epoch => break (connection, current),
                Authenticated::Session {
                    mut connection,
                    current,
                } => {
                    // The relay is at a newer epoch than any key this replica holds.
                    self.admit_epochs(&mut connection, epoch, &mut pass).await?;
                    let drained = self.pull(&mut connection, &keys, &mut pass).await?;
                    keys = self.open_keys().await?;
                    let newest = keys.newest_epoch().unwrap_or(epoch);
                    if newest > epoch {
                        epoch = newest;
                        continue;
                    }
                    if drained && self.repairs_squat(&mut connection, epoch).await? {
                        drop(connection);
                        self.repair(target, &keys, epoch, &mut pass).await?;
                        continue;
                    }
                    pass.epoch = epoch;
                    pass.note(
                        RelayProblem::EpochAhead,
                        format!(
                            "the relay is at epoch {current}, which this device cannot open yet"
                        ),
                    );
                    return Ok(pass);
                }
                Authenticated::UnknownEpoch => {
                    if let Some(stalled) =
                        self.catch_up_relay(target, &keys, epoch, &mut pass).await?
                    {
                        pass.epoch = stalled;
                        return Ok(pass);
                    }
                }
                Authenticated::KeyMismatch => {
                    if !self.is_live_admin().await? {
                        return Err(RelayError::new(
                            RelayProblem::AccessRefused,
                            format!(
                                "the relay holds another access key for epoch {epoch}; an admin \
                             device repairs it on its next pass"
                            ),
                        ));
                    }
                    self.repair(target, &keys, epoch, &mut pass).await?;
                }
            }
        };
        pass.epoch = current;
        if let Some(store) = self.assets {
            self.upload_assets(&mut connection, &keys, store, current, &mut pass)
                .await?;
        }
        let compacted = self
            .push(&mut connection, &keys, current, &mut pass)
            .await?;
        let drained = self.pull(&mut connection, &keys, &mut pass).await?;
        if pass.pulled > 0 && self.assets.is_some() {
            pass.more = true;
        }
        // The admin set follows the chain only once this replica holds everything the relay
        // holds for it, so an admin that lags behind a demotion never restores the demoted key.
        if drained
            && let Some(admin) = self.admin
            && self.is_live_admin().await?
        {
            self.keep_admin_set(&mut connection, admin, &mut pass)
                .await?;
        }
        if let Some(store) = self.assets {
            self.download_assets(&mut connection, &keys, store, &mut pass)
                .await?;
        }
        match connection.request(&RelayRequest::State).await? {
            RelayResponse::State {
                writers,
                stored_bytes,
                quota_bytes,
                ..
            } => {
                pass.stored_bytes = stored_bytes;
                pass.quota_bytes = quota_bytes;
                let reached = relay_ranges(&writers)?;
                pass.undelivered = self.undelivered(&reached, &compacted).await?;
            }
            other => return Err(unexpected("State", &other)),
        }
        Ok(pass)
    }

    /// Long poll after a pass: whether the relay holds records past this replica's log before
    /// its bounded wait ends. Authenticates at the newest epoch this replica holds.
    pub async fn wait(&self, target: &RelayTarget) -> Result<bool, RelayError> {
        let keys = self.open_keys().await?;
        let epoch = keys
            .newest_epoch()
            .ok_or_else(|| "this device holds no key epoch of the space yet".to_string())?;
        let Authenticated::Session { mut connection, .. } =
            self.authenticate(target, &keys, epoch).await?
        else {
            return Ok(true);
        };
        let have = vector_to_relay(&self.stored().await?);
        match connection.request(&RelayRequest::Wait { have }).await? {
            RelayResponse::Waited { ready } => Ok(ready),
            other => Err(unexpected("Waited", &other)),
        }
    }

    async fn open_keys(&self) -> Result<RelayKeys, RelayError> {
        let mut conn = acquire(self.pool).await?;
        let stored = self
            .engine
            .space_keys(&mut conn, self.ctx)
            .await
            .map_err(|error| format!("read sync space keys: {error}"))?;
        Ok(RelayKeys::open(self.ctx.space, &stored, self.holder))
    }

    async fn view(&self) -> Result<AuthorizationView, RelayError> {
        let mut conn = acquire(self.pool).await?;
        self.engine
            .authorization(&mut conn, self.ctx)
            .await
            .map_err(|error| format!("read authorization state: {error}"))?
            .ok_or_else(|| RelayError::from("the space has no authorization state".to_string()))
    }

    /// Whether this installation's admin key belongs to an admin device the selected chain has
    /// not removed.
    async fn is_live_admin(&self) -> Result<bool, RelayError> {
        let Some(admin) = self.admin else {
            return Ok(false);
        };
        let view = self.view().await?;
        let key = admin.public_key();
        Ok(live_admin_keys(&view).contains(&key))
    }

    /// Whether the relay's registration of `held + 1` is a squat this admin device repairs, on
    /// a connection authenticated at `held` whose pull just drained the relay. Fails closed:
    /// the caller asks only after a drained pull, this device must be a live admin, the chain
    /// it selected after that pull still names no key epoch past `held`, and the record the
    /// relay serves for `held + 1` must fail to authenticate as that epoch's key epoch record
    /// against the stored verified envelope set. Only the anchor's device and writer keys make
    /// one that authenticates, so a record that does means this device lags behind a real
    /// rotation and repairing would roll the relay back below it.
    async fn repairs_squat(
        &self,
        connection: &mut Connection,
        held: u32,
    ) -> Result<bool, RelayError> {
        if !self.is_live_admin().await? || self.view().await?.key_epoch > held {
            return Ok(false);
        }
        let next = held
            .checked_add(1)
            .ok_or_else(|| "the key epoch is at its limit".to_string())?;
        let Some(entry) = self.relay_epoch_entry(connection, next).await? else {
            return Ok(true);
        };
        let mut conn = acquire(self.pool).await?;
        let authentic = self
            .engine
            .authenticates_key_epoch(&mut conn, self.ctx, &entry, next)
            .await
            .map_err(|error| format!("check the relay's key epoch record: {error}"))?;
        Ok(!authentic)
    }

    /// The record the relay serves for `epoch`, bounded before it is decoded.
    async fn relay_epoch_entry(
        &self,
        connection: &mut Connection,
        epoch: u32,
    ) -> Result<Option<Vec<u8>>, RelayError> {
        let after = epoch.saturating_sub(1);
        let entries = match connection.request(&RelayRequest::Epochs { after }).await? {
            RelayResponse::Epochs { entries, .. } => entries,
            other => return Err(unexpected("Epochs", &other)),
        };
        entries
            .iter()
            .find(|entry| entry.epoch == epoch)
            .map(|entry| {
                decode_relay_bytes(
                    "relay epoch entry",
                    &entry.entry,
                    MAX_RELAY_EPOCH_ENTRY_BYTES,
                )
            })
            .transpose()
            .map_err(RelayError::from)
    }

    async fn stored(&self) -> Result<VersionVector, RelayError> {
        let mut conn = acquire(self.pool).await?;
        self.engine
            .stored_vector(&mut conn, self.ctx)
            .await
            .map_err(|error| format!("read sync log: {error}").into())
    }

    /// Operations of this installation's own writers the relay does not hold: past what the
    /// relay reached and past the floor the log compacted, which no relay can take any more.
    async fn undelivered(
        &self,
        reached: &VersionVector,
        compacted: &VersionVector,
    ) -> Result<u64, RelayError> {
        let stored = self.stored().await?;
        Ok(undelivered_count(
            self.own_writers,
            &stored,
            reached,
            compacted,
        ))
    }

    /// Connects and authenticates at `epoch` on a fresh connection.
    async fn authenticate(
        &self,
        target: &RelayTarget,
        keys: &RelayKeys,
        epoch: u32,
    ) -> Result<Authenticated, RelayError> {
        let access = access_key(keys, epoch)?;
        let mut connection = Connection::open(target, self.ctx.space, self.limits.loopback).await?;
        connection.hello().await?;
        let signature = access.sign(RELAY_AUTH_DOMAIN, &connection.binding().auth_payload(epoch));
        let response = connection
            .request(&RelayRequest::Auth {
                epoch,
                signature: encode_relay_bytes(&signature),
            })
            .await?;
        match response {
            RelayResponse::Authenticated {
                epoch: accepted,
                current_epoch,
                ..
            } if accepted == epoch => Ok(Authenticated::Session {
                connection: Box::new(connection),
                current: current_epoch,
            }),
            RelayResponse::Refused {
                code: RelayRefusalCode::UnknownEpoch,
                ..
            } => Ok(Authenticated::UnknownEpoch),
            RelayResponse::Refused {
                code: RelayRefusalCode::Authentication,
                ..
            } => Ok(Authenticated::KeyMismatch),
            other => Err(unexpected("Authenticated", &other)),
        }
    }

    /// Brings a relay that lags behind `newest` up to it: finds the relay's current epoch, then
    /// for each following epoch delivers the record that introduced it under the epoch before
    /// and registers it.
    ///
    /// Returns the epoch the relay stays at when the relay does not hold the chain through the
    /// record that opens the next epoch, so this pass ends there instead of retrying; `None`
    /// when the caller authenticates again.
    async fn catch_up_relay(
        &self,
        target: &RelayTarget,
        keys: &RelayKeys,
        newest: u32,
        pass: &mut RelayPass,
    ) -> Result<Option<u32>, RelayError> {
        let mut probe = newest;
        let (mut connection, current) = loop {
            probe = probe
                .checked_sub(1)
                .filter(|epoch| *epoch > 0 && keys.key(*epoch).is_some())
                .ok_or_else(|| {
                    "the relay holds none of the key epochs this device holds".to_string()
                })?;
            match self.authenticate(target, keys, probe).await? {
                Authenticated::Session {
                    connection,
                    current,
                } => break (connection, current),
                Authenticated::UnknownEpoch => continue,
                Authenticated::KeyMismatch => {
                    return Err(RelayError::new(
                        RelayProblem::AccessRefused,
                        format!("the relay holds another access key for epoch {probe}"),
                    ));
                }
            }
        };
        if current != probe {
            if current >= newest || keys.key(current).is_none() {
                // Another device moved the relay meanwhile; the next attempt starts over.
                return Ok(None);
            }
            connection = match self.authenticate(target, keys, current).await? {
                Authenticated::Session { connection, .. } => connection,
                _ => return Ok(None),
            };
        }
        let mut at = current;
        while at < newest {
            let next = at + 1;
            // Registering before the relay holds the chain through the record that opens
            // `next` would leave members at `at` unable to reach it through the relay.
            if !self
                .flush_before(&mut connection, keys, at, next, pass)
                .await?
            {
                return Ok(Some(at));
            }
            self.register(&mut connection, keys, next).await?;
            pass.registered.push(next);
            connection = match self.authenticate(target, keys, next).await? {
                Authenticated::Session { connection, .. } => connection,
                _ => return Ok(None),
            };
            at = next;
        }
        Ok(None)
    }

    /// Delivers, under the key of `epoch`, the operations of the writer whose record introduced
    /// `next`, through that record. Members still at `epoch` store the record and its parent
    /// from these and open their envelope of `next` before they can open anything sealed under
    /// it. The writer sealed these operations no later than the rotation itself, so a device the
    /// rotation removed learns only its removal and the rotation from them.
    ///
    /// Returns whether the relay holds the writer's operations through that record afterwards,
    /// read back from the relay's ranges; a stall, a gap, or a missing operation means it does
    /// not.
    async fn flush_before(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        epoch: u32,
        next: u32,
        pass: &mut RelayPass,
    ) -> Result<bool, RelayError> {
        let entry = self.epoch_entry(next).await?;
        let envelope = Envelope::decode(&entry)
            .map_err(|_| "this device holds a malformed key epoch record".to_string())?;
        let ranges = connection.ranges().await?;
        let mut cursor = self.stored().await?;
        cursor.set(envelope.writer, ranges.get(&envelope.writer));
        if cursor.get(&envelope.writer) >= envelope.seq {
            return Ok(true);
        }
        let mut certificates = BTreeMap::new();
        loop {
            let page = {
                let mut conn = acquire(self.pool).await?;
                match self
                    .engine
                    .ops_after(&mut conn, self.ctx, &cursor, PUSH_READ_BYTES)
                    .await
                {
                    Ok(page) => page,
                    Err(SyncError::Compacted { writer, seq }) => {
                        cursor.set(writer, seq);
                        continue;
                    }
                    Err(error) => return Err(format!("read sync operations: {error}").into()),
                }
            };
            let ops: Vec<Vec<u8>> = page
                .ops
                .into_iter()
                .filter(|op| {
                    Envelope::decode(op).is_ok_and(|decoded| {
                        decoded.writer == envelope.writer && decoded.seq <= envelope.seq
                    })
                })
                .collect();
            let stalled = if ops.is_empty() {
                true
            } else {
                let sealed = self.seal(keys, epoch, &ops, &mut certificates).await?;
                let outcome = self.send_records(connection, sealed, pass).await?;
                for op in &ops {
                    if let Ok(decoded) = Envelope::decode(op) {
                        cursor.advance(decoded.writer, decoded.seq);
                    }
                }
                outcome.stalled.contains(&envelope.writer)
            };
            if stalled || cursor.get(&envelope.writer) >= envelope.seq || !page.more {
                let delivered = connection.ranges().await?.get(&envelope.writer) >= envelope.seq;
                if !delivered {
                    pass.note(
                        RelayProblem::RelayBehind,
                        format!(
                            "the relay does not hold the records before epoch {next}, so this \
                         device does not register it; members still at epoch {epoch} wait for \
                         a direct connection"
                        ),
                    );
                }
                return Ok(delivered);
            }
        }
    }

    async fn epoch_entry(&self, epoch: u32) -> Result<Vec<u8>, RelayError> {
        let mut conn = acquire(self.pool).await?;
        self.engine
            .key_epoch_entry(&mut conn, self.ctx, epoch)
            .await
            .map_err(|error| format!("read key epoch record: {error}"))?
            .ok_or_else(|| {
                RelayError::from(format!("this device holds no record of key epoch {epoch}"))
            })
    }

    /// Registers `next` with its access key and the record that introduced it, on a connection
    /// authenticated at the epoch before.
    async fn register(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        next: u32,
    ) -> Result<(), RelayError> {
        let entry = self.epoch_entry(next).await?;
        if entry.len() > MAX_RELAY_EPOCH_ENTRY_BYTES {
            return Err(format!("the record of key epoch {next} is too large").into());
        }
        let access = access_key(keys, next)?;
        let signature = access.sign(
            RELAY_EPOCH_DOMAIN,
            &connection.binding().epoch_payload(next),
        );
        let request = RelayRequest::RegisterEpoch {
            epoch: next,
            access_key: encode_relay_bytes(access.public_key().as_bytes()),
            entry: encode_relay_bytes(&entry),
            signature: encode_relay_bytes(&signature),
        };
        match connection.request(&request).await? {
            RelayResponse::EpochRegistered { epoch } if epoch == next => Ok(()),
            other => Err(unexpected("EpochRegistered", &other)),
        }
    }

    /// Restores `epoch` with this device's access key as the relay's current epoch, which drops
    /// whatever the relay holds above it, then registers its record again from the epoch
    /// before so members that lag behind still find it.
    async fn repair(
        &self,
        target: &RelayTarget,
        keys: &RelayKeys,
        epoch: u32,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        let Some(admin) = self.admin else {
            return Err("this device holds no admin key".to_string().into());
        };
        if pass.repaired.is_some() {
            return Err(RelayError::new(
                RelayProblem::EpochUnsettled,
                format!("the relay refused epoch {epoch} again after a repair in this pass"),
            ));
        }
        let access_key = access_key(keys, epoch)?.public_key();
        let mut connection = Connection::open(target, self.ctx.space, self.limits.loopback).await?;
        connection.hello().await?;
        connection
            .admin(
                admin,
                1,
                &RelayAdminCommand::RelayEpochRepair {
                    epoch,
                    access_key: &access_key,
                },
            )
            .await?;
        pass.repaired = Some(epoch);
        drop(connection);
        if epoch > 1
            && let Authenticated::Session { mut connection, .. } =
                self.authenticate(target, keys, epoch - 1).await?
        {
            self.register(&mut connection, keys, epoch).await?;
        }
        Ok(())
    }

    /// Offers the key epoch records the relay holds past `epoch` to the record store, which
    /// keeps only what verifies against the chain this replica selected.
    async fn admit_epochs(
        &self,
        connection: &mut Connection,
        epoch: u32,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        let mut after = epoch;
        loop {
            let (entries, more) = match connection.request(&RelayRequest::Epochs { after }).await? {
                RelayResponse::Epochs { entries, more, .. } => (entries, more),
                other => return Err(unexpected("Epochs", &other)),
            };
            let Some(last) = entries.iter().map(|entry| entry.epoch).max() else {
                return Ok(());
            };
            if last <= after {
                return Err(RelayError::new(
                    RelayProblem::RecordRefused,
                    "the relay sent key epochs out of order",
                ));
            }
            let mut envelopes = Vec::with_capacity(entries.len());
            for entry in &entries {
                envelopes.push(decode_relay_bytes(
                    "relay epoch entry",
                    &entry.entry,
                    MAX_RELAY_EPOCH_ENTRY_BYTES,
                )?);
            }
            let admitted = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .admit_records(&mut conn, self.ctx, &envelopes, crate::now_ms()?)
                    .await
            };
            if let Err(error) = admitted {
                pass.note(
                    RelayProblem::RecordRefused,
                    format!("this device refused a relay key epoch record: {error}"),
                );
            }
            if !more {
                return Ok(());
            }
            after = last;
        }
    }

    /// Adds the keys of live admin devices and the recovery key to the relay's admin set and
    /// removes every other key, so a removed or demoted admin loses relay authority too.
    async fn keep_admin_set(
        &self,
        connection: &mut Connection,
        admin: &AdminKeyPair,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        let view = self.view().await?;
        let wanted = relay_admin_set(&view);
        let (held, mut counter) = match connection.request(&RelayRequest::State).await? {
            RelayResponse::State {
                admins,
                admin_counter,
                ..
            } => {
                let held = admins
                    .iter()
                    .map(|key| decode_relay_admin_key(key))
                    .collect::<Result<BTreeSet<_>, _>>()?;
                (held, admin_counter)
            }
            other => return Err(unexpected("State", &other)),
        };
        for key in wanted.difference(&held) {
            if held.len() + 1 > MAX_RELAY_ADMIN_KEYS {
                pass.note(
                    RelayProblem::AdminSetFull,
                    "the relay's admin set is full".to_string(),
                );
                break;
            }
            counter = connection
                .admin(admin, counter + 1, &RelayAdminCommand::AddKey(key))
                .await?;
        }
        for key in held.difference(&wanted) {
            counter = connection
                .admin(admin, counter + 1, &RelayAdminCommand::RemoveKey(key))
                .await?;
        }
        Ok(())
    }

    /// Seals operations under `epoch` with their writers' certificates.
    async fn seal(
        &self,
        keys: &RelayKeys,
        epoch: u32,
        ops: &[Vec<u8>],
        certificates: &mut BTreeMap<WriterId, SignedCertificate>,
    ) -> Result<Vec<(WriterId, String)>, RelayError> {
        let mut sealed = Vec::with_capacity(ops.len());
        for op in ops {
            let envelope = Envelope::decode(op)
                .map_err(|_| "this device holds a malformed operation".to_string())?;
            let certificate = match certificates.entry(envelope.writer) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let mut conn = acquire(self.pool).await?;
                    let certificate = self
                        .engine
                        .writer_certificate(&mut conn, self.ctx, envelope.writer)
                        .await
                        .map_err(|error| format!("read writer certificate: {error}"))?
                        .ok_or_else(|| "a stored operation has no stored writer".to_string())?;
                    entry.insert(certificate)
                }
            };
            let record = keys
                .seal_record_at(epoch, op, certificate)
                .map_err(|error| format!("seal relay record: {error}"))?;
            sealed.push((envelope.writer, encode_relay_bytes(&record.encode())));
        }
        Ok(sealed)
    }

    /// Pushes sealed records in pages within the relay's bounds. A writer whose record the
    /// relay refused as a gap stalls: none of its later records in the batch are sent.
    async fn send_records(
        &self,
        connection: &mut Connection,
        sealed: Vec<(WriterId, String)>,
        pass: &mut RelayPass,
    ) -> Result<Sent, RelayError> {
        let mut sent = Sent::default();
        let mut page: Vec<(WriterId, String)> = Vec::new();
        let mut text = 0usize;
        for (writer, record) in sealed {
            if !page.is_empty()
                && (page.len() >= MAX_RELAY_PAGE_RECORDS
                    || text.saturating_add(record.len()) > MAX_PUSH_TEXT_BYTES)
            {
                self.push_page(connection, std::mem::take(&mut page), &mut sent, pass)
                    .await?;
                text = 0;
            }
            text = text.saturating_add(record.len());
            page.push((writer, record));
        }
        if !page.is_empty() {
            self.push_page(connection, page, &mut sent, pass).await?;
        }
        Ok(sent)
    }

    async fn push_page(
        &self,
        connection: &mut Connection,
        page: Vec<(WriterId, String)>,
        sent: &mut Sent,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        let (writers, records): (Vec<WriterId>, Vec<String>) = page
            .into_iter()
            .filter(|(writer, _)| !sent.stalled.contains(writer))
            .unzip();
        if records.is_empty() {
            return Ok(());
        }
        let outcomes = match connection.request(&RelayRequest::Push { records }).await? {
            RelayResponse::Pushed { outcomes } if outcomes.len() == writers.len() => outcomes,
            other => return Err(unexpected("Pushed", &other)),
        };
        for (writer, outcome) in writers.into_iter().zip(outcomes) {
            match outcome {
                RelayPushOutcome::Stored => pass.pushed += 1,
                RelayPushOutcome::Duplicate
                | RelayPushOutcome::SlotTaken
                | RelayPushOutcome::Trimmed => {}
                RelayPushOutcome::Gap => {
                    sent.stalled.insert(writer);
                }
            }
        }
        Ok(())
    }

    /// Pushes every operation the relay lacks under the current epoch, from the writer ranges
    /// the relay reports. Returns the floors the log compacted past the relay's ranges, whose
    /// operations no relay can take any more.
    async fn push(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        epoch: u32,
        pass: &mut RelayPass,
    ) -> Result<VersionVector, RelayError> {
        let mut compacted = VersionVector::new();
        if keys.newest_epoch() != Some(epoch) {
            return Ok(compacted);
        }
        let ranges = connection.ranges().await?;
        let stored = self.stored().await?;
        let mut cursor = ranges.clone();
        let mut certificates = BTreeMap::new();
        let mut stalled = false;
        let mut moved = 0usize;
        loop {
            let page = {
                let mut conn = acquire(self.pool).await?;
                match self
                    .engine
                    .ops_after(&mut conn, self.ctx, &cursor, PUSH_READ_BYTES)
                    .await
                {
                    Ok(page) => page,
                    Err(SyncError::Compacted { writer, seq }) => {
                        cursor.set(writer, seq);
                        compacted.set(writer, seq);
                        continue;
                    }
                    Err(error) => return Err(format!("read sync operations: {error}").into()),
                }
            };
            if page.ops.is_empty() {
                break;
            }
            let sealed = self.seal(keys, epoch, &page.ops, &mut certificates).await?;
            let sent = self.send_records(connection, sealed, pass).await?;
            for op in &page.ops {
                if let Ok(envelope) = Envelope::decode(op) {
                    cursor.advance(envelope.writer, envelope.seq);
                }
            }
            for writer in sent.stalled {
                cursor.set(writer, stored.get(&writer).max(cursor.get(&writer)));
                stalled = true;
            }
            moved += page.ops.iter().map(Vec::len).sum::<usize>();
            if !page.more {
                break;
            }
            if moved >= self.limits.max_bytes {
                pass.more = true;
                break;
            }
        }
        if stalled {
            pass.note(
                RelayProblem::Gap,
                "the relay holds a gap before operations this device holds; they wait for a \
                 direct connection"
                    .to_string(),
            );
        }
        Ok(compacted)
    }

    /// Pulls the records the relay holds at or below the session's epoch that this replica
    /// lacks, and stores the operations they carry through the verified operation path.
    ///
    /// Returns whether the pull drained the relay: the relay reported nothing more to pull,
    /// never reported operations trimmed below its floor, and this replica refused no record.
    /// A stop at the byte bound, a refusal, or a trimmed range means this replica cannot know
    /// what the relay still holds for it.
    async fn pull(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        pass: &mut RelayPass,
    ) -> Result<bool, RelayError> {
        let mut skipped = VersionVector::new();
        let mut moved = 0usize;
        let mut trimmed = false;
        loop {
            let mut known = self.stored().await?;
            known.merge(&skipped);
            let request = RelayRequest::Pull {
                have: vector_to_relay(&known),
            };
            let (records, behind_floor, more) = match connection.request(&request).await? {
                RelayResponse::Records {
                    records,
                    behind_floor,
                    more,
                } => (records, behind_floor, more),
                other => return Err(unexpected("Records", &other)),
            };
            if !behind_floor.is_empty() {
                trimmed = true;
                pass.note(
                    RelayProblem::Trimmed,
                    "the relay trimmed operations this device lacks; link directly with a \
                     device that holds them"
                        .to_string(),
                );
            }
            if records.is_empty() {
                return Ok(!more && !trimmed);
            }
            let mut opened = Vec::with_capacity(records.len());
            for text in &records {
                let bytes = decode_relay_bytes("relay record", text, MAX_RELAY_RECORD_BYTES)?;
                moved += bytes.len();
                match keys.open_record(&bytes) {
                    Ok(record) => opened.push((record.operation, record.certificate)),
                    Err(error) => {
                        pass.note(
                            RelayProblem::RecordRefused,
                            format!("this device refused a relay record: {error}"),
                        );
                        return Ok(false);
                    }
                }
            }
            let outcomes = {
                let mut conn = acquire(self.pool).await?;
                self.engine
                    .store_relay_records(
                        &mut conn,
                        self.ctx,
                        &opened,
                        self.quarantined,
                        crate::now_ms()?,
                    )
                    .await
                    .map_err(|error| format!("store relay operations: {error}"))?
            };
            for ((operation, _), outcome) in opened.iter().zip(&outcomes) {
                match outcome {
                    StoreOutcome::Stored { .. } => pass.pulled += 1,
                    StoreOutcome::Duplicate => {}
                    StoreOutcome::Refused(StoreRefusal::Revoked | StoreRefusal::Quarantined) => {
                        if let Ok(envelope) = Envelope::decode(operation) {
                            skipped.advance(envelope.writer, envelope.seq);
                        }
                    }
                    StoreOutcome::Refused(refusal) => {
                        let writer = Envelope::decode(operation)
                            .map(|envelope| envelope.writer)
                            .map_err(|_| "the relay sent a malformed operation".to_string())?;
                        let own = matches!(refusal, StoreRefusal::Fork { .. })
                            && self.own_writers.contains(&writer);
                        let problem = if own {
                            RelayProblem::Fork
                        } else {
                            RelayProblem::RecordRefused
                        };
                        pass.note(
                            problem,
                            if own {
                                format!(
                                    "the relay holds a fork of this device's writer {}; link \
                                 directly so the device can re-seal it",
                                    writer.to_hex()
                                )
                            } else {
                                format!(
                                    "this device refused a relay operation of writer {}: \
                                 {refusal:?}",
                                    writer.to_hex()
                                )
                            },
                        );
                        return Ok(false);
                    }
                }
            }
            if !more {
                return Ok(!trimmed);
            }
            let mut reached = self.stored().await?;
            reached.merge(&skipped);
            if known.dominates(&reached) {
                pass.note(
                    RelayProblem::NoProgress,
                    "the relay sent no new operations".to_string(),
                );
                return Ok(false);
            }
            if moved >= self.limits.max_bytes {
                pass.more = true;
                return Ok(false);
            }
        }
    }

    async fn referenced(&self) -> Result<Vec<AssetRef>, RelayError> {
        let mut conn = acquire(self.pool).await?;
        self.engine
            .referenced_assets(&mut conn)
            .await
            .map_err(|error| format!("load asset references: {error}").into())
    }

    /// The relay ids of `assets` the relay holds completely.
    async fn present(
        connection: &mut Connection,
        keys: &RelayKeys,
        assets: &[AssetRef],
    ) -> Result<BTreeSet<RelayAssetId>, RelayError> {
        let mut present = BTreeSet::new();
        for batch in assets.chunks(MAX_RELAY_ASSET_QUERY) {
            let ids = batch
                .iter()
                .map(|asset| relay_asset_id(keys, asset))
                .collect::<Result<Vec<_>, _>>()?;
            let request = RelayRequest::AssetQuery {
                assets: ids
                    .iter()
                    .map(|id| encode_relay_asset(id.as_bytes()))
                    .collect(),
            };
            match connection.request(&request).await? {
                RelayResponse::AssetPresence { assets } => {
                    for text in assets {
                        let bytes = decode_relay_asset(&text)?;
                        present.insert(RelayAssetId::from_bytes(bytes));
                    }
                }
                other => return Err(unexpected("AssetPresence", &other)),
            }
        }
        Ok(present)
    }

    /// Uploads the referenced files this replica holds and the relay lacks, before the
    /// operations that reference them.
    async fn upload_assets(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        epoch: u32,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        if keys.newest_epoch() != Some(epoch) {
            return Ok(());
        }
        let (held, _) = partition_held(store, self.referenced().await?)
            .await
            .map_err(|error| error.describe())?;
        let (held, deferred): (Vec<_>, Vec<_>) = held
            .into_iter()
            .partition(|asset| asset.byte_size() <= self.limits.max_asset_bytes);
        pass.deferred_assets += deferred.len();
        let present = Self::present(connection, keys, &held).await?;
        let mut moved = 0usize;
        for asset in &held {
            let id = relay_asset_id(keys, asset)?;
            if present.contains(&id) {
                continue;
            }
            if moved >= self.limits.max_bytes {
                pass.more = true;
                break;
            }
            match self
                .upload_asset(connection, keys, store, asset, id, &mut moved)
                .await?
            {
                Ok(true) => pass.uploaded += 1,
                Ok(false) => {}
                Err(message) => pass.note(
                    RelayProblem::AssetRefused,
                    format!(
                        "the relay refused {} asset {}: {message}",
                        asset.class().name,
                        asset.sha256()
                    ),
                ),
            }
        }
        Ok(())
    }

    /// Seals and puts every chunk of one file. `Ok(false)` when the local file changed or
    /// vanished; the inner error is the relay's refusal.
    async fn upload_asset(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        asset: &AssetRef,
        id: RelayAssetId,
        moved: &mut usize,
    ) -> Result<Result<bool, String>, RelayError> {
        let size = asset.byte_size() as usize;
        let Some(count) = relay_asset_chunk_count(size) else {
            return Ok(Err(
                "the file is empty or too large for the relay".to_string()
            ));
        };
        for index in 0..count {
            let offset = index as usize * RELAY_ASSET_CHUNK_BYTES;
            let Ok(offset) = u32::try_from(offset) else {
                return Ok(Err("the file is too large for the relay".to_string()));
            };
            let plaintext = {
                let asset = asset.clone();
                blocking(store, move |store| {
                    store.read_chunk(&asset, offset, RELAY_ASSET_CHUNK_BYTES)
                })
                .await
            };
            let plaintext = match plaintext {
                Ok(bytes) if !bytes.is_empty() => Zeroizing::new(bytes),
                Ok(_) | Err(AssetStoreError::Unavailable) => return Ok(Ok(false)),
                Err(error) => return Err(error.describe().into()),
            };
            let chunk = keys
                .seal_asset_chunk(id, index, count, &plaintext)
                .map_err(|error| format!("seal relay asset chunk: {error}"))?;
            *moved += plaintext.len();
            let request = RelayRequest::AssetPut {
                chunk: encode_relay_bytes(&chunk.encode()),
            };
            match connection.request(&request).await? {
                RelayResponse::AssetStored {
                    outcome:
                        RelayAssetOutcome::Stored
                        | RelayAssetOutcome::Duplicate
                        | RelayAssetOutcome::SlotTaken,
                } => {}
                RelayResponse::Refused { code, message, .. } => {
                    return Ok(Err(format!("{code:?}: {message}")));
                }
                other => return Err(unexpected("AssetStored", &other)),
            }
        }
        Ok(Ok(true))
    }

    /// Downloads the referenced files this replica lacks and the relay holds.
    async fn download_assets(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        let (_, missing) = partition_held(store, self.referenced().await?)
            .await
            .map_err(|error| error.describe())?;
        let present = Self::present(connection, keys, &missing).await?;
        let mut moved = 0usize;
        for asset in &missing {
            let id = relay_asset_id(keys, asset)?;
            if !present.contains(&id) {
                continue;
            }
            if asset.byte_size() > self.limits.max_asset_bytes {
                pass.deferred_assets += 1;
                continue;
            }
            if moved >= self.limits.max_bytes {
                pass.more = true;
                break;
            }
            match self
                .download_asset(connection, keys, store, asset, id, &mut moved)
                .await?
            {
                Ok(true) => pass.downloaded += 1,
                Ok(false) => {}
                Err(message) => pass.note(
                    RelayProblem::AssetRefused,
                    format!(
                        "this device refused {} asset {} from the relay: {message}",
                        asset.class().name,
                        asset.sha256()
                    ),
                ),
            }
        }
        Ok(())
    }

    /// Fetches, opens, and stages every chunk of one file, then commits it once the opener
    /// confirms the whole digest. `Ok(false)` when the relay no longer holds it; the inner error
    /// refuses the file, whose staged bytes are dropped.
    async fn download_asset(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        asset: &AssetRef,
        id: RelayAssetId,
        moved: &mut usize,
    ) -> Result<Result<bool, String>, RelayError> {
        let Some(expected) = relay_asset_chunk_count(asset.byte_size() as usize) else {
            return Ok(Err(
                "the file is empty or too large for the relay".to_string()
            ));
        };
        let sha256 = sha256_of(asset)?;
        let mut opener = keys
            .asset_opener(sha256)
            .map_err(|error| format!("open relay asset: {error}"))?;
        let mut staged = {
            let asset = asset.clone();
            blocking(store, move |store| store.stage(&asset))
                .await
                .map_err(|error| error.describe())?
        };
        for index in 0..expected {
            let request = RelayRequest::AssetGet {
                asset: encode_relay_asset(id.as_bytes()),
                index,
            };
            let text = match connection.request(&request).await? {
                RelayResponse::AssetChunk { chunk } => chunk,
                RelayResponse::Refused {
                    code: RelayRefusalCode::NotFound,
                    ..
                } => return Ok(Ok(false)),
                other => return Err(unexpected("AssetChunk", &other)),
            };
            let bytes =
                decode_relay_bytes("relay asset chunk", &text, MAX_RELAY_ASSET_CHUNK_BYTES)?;
            *moved += bytes.len();
            let (plaintext, count) = match keys.open_asset_chunk(&mut opener, &bytes) {
                Ok(opened) => opened,
                Err(error) => return Ok(Err(error.to_string())),
            };
            if count != expected {
                return Ok(Err(
                    "the chunk count disagrees with the file size".to_string()
                ));
            }
            staged = match write_staged(staged, plaintext).await {
                Ok(staged) => staged,
                Err(error) => return Ok(Err(error.describe())),
            };
        }
        if let Err(error) = opener.finish() {
            return Ok(Err(error.to_string()));
        }
        match blocking(store, move |store| store.commit(staged)).await {
            Ok(()) => Ok(Ok(true)),
            Err(AssetStoreError::Mismatch(message)) => Ok(Err(message)),
            Err(error) => Err(error.describe().into()),
        }
    }
}

/// Writers whose records the relay refused as a gap in one push.
#[derive(Default)]
struct Sent {
    stalled: BTreeSet<WriterId>,
}

/// The admin set the relay keeps for the space: every live admin device's key and the recovery
/// kit's admin key.
pub fn relay_admin_set(view: &AuthorizationView) -> BTreeSet<AdminPublicKey> {
    let mut keys = live_admin_keys(view);
    keys.insert(view.recovery_admin_key);
    keys
}

/// The admin keys of enrolled admin devices the selected chain has not removed.
pub fn live_admin_keys(view: &AuthorizationView) -> BTreeSet<AdminPublicKey> {
    view.devices
        .iter()
        .filter(|device| device.removed_revision.is_none())
        .filter_map(|device| match device.role {
            DeviceRole::Admin(key) => Some(key),
            DeviceRole::Member => None,
        })
        .collect()
}

fn access_key(keys: &RelayKeys, epoch: u32) -> Result<RelayAccessKeyPair, RelayError> {
    keys.key(epoch)
        .ok_or_else(|| format!("this device holds no key of epoch {epoch}"))?
        .relay_access_key(&keys_space(keys))
        .map_err(|error| format!("derive relay access key: {error}").into())
}

fn keys_space(keys: &RelayKeys) -> SpaceId {
    keys.space()
}

fn sha256_of(asset: &AssetRef) -> Result<[u8; 32], RelayError> {
    parse_hex::<32>(asset.sha256())
        .ok_or_else(|| RelayError::from("an asset reference is malformed".to_string()))
}

fn relay_asset_id(keys: &RelayKeys, asset: &AssetRef) -> Result<RelayAssetId, RelayError> {
    keys.asset_id(&sha256_of(asset)?)
        .map_err(|error| format!("derive relay asset id: {error}").into())
}

fn vector_to_relay(vector: &VersionVector) -> Vec<SyncSeq> {
    vector
        .iter()
        .map(|(writer, seq)| SyncSeq {
            writer: writer.to_hex(),
            seq,
        })
        .collect()
}

fn unexpected(expected: &str, response: &RelayResponse) -> RelayError {
    match response {
        RelayResponse::Refused { code, message, .. } => RelayError::new(
            RelayProblem::of_refusal(*code),
            format!("the relay refused the request ({code:?}): {message}"),
        ),
        _ => RelayError::new(
            RelayProblem::Refused,
            format!("the relay did not answer with {expected}"),
        ),
    }
}

/// Operations of `own_writers` this installation stores past both what the relay `reached` and
/// the floor its log `compacted`, which no relay can take any more. Other writers' operations
/// never count: they are not this device's to deliver.
fn undelivered_count(
    own_writers: &BTreeSet<WriterId>,
    stored: &VersionVector,
    reached: &VersionVector,
    compacted: &VersionVector,
) -> u64 {
    own_writers
        .iter()
        .map(|writer| {
            stored
                .get(writer)
                .saturating_sub(reached.get(writer).max(compacted.get(writer)))
        })
        .sum()
}

/// The last sequence of each writer in the relay's ranges.
fn relay_ranges(writers: &[RelayWriterRange]) -> Result<VersionVector, RelayError> {
    let mut ranges = VersionVector::new();
    for range in writers {
        let writer = WriterId::from_hex(&range.writer)
            .ok_or_else(|| "the relay named an invalid writer".to_string())?;
        ranges.set(writer, range.next.saturating_sub(1));
    }
    Ok(ranges)
}

async fn acquire(
    pool: &SqlitePool,
) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, RelayError> {
    pool.acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}").into())
}

/// One TLS connection to the relay, pinned to the configured certificate.
pub struct Connection {
    stream: TlsStream<TcpStream>,
    exporter: Zeroizing<[u8; RELAY_EXPORTER_BYTES]>,
    space: SpaceId,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Connection")
            .field("space", &self.space)
            .finish_non_exhaustive()
    }
}

impl Connection {
    /// Dials the target's endpoints in order. Each resolved address passes the configured
    /// server dial rule first, see [`authorize_server_dial_target`].
    pub async fn open(
        target: &RelayTarget,
        space: SpaceId,
        loopback: LoopbackPolicy,
    ) -> Result<Self, RelayError> {
        let config = client_config(&target.certificate_fingerprint)?;
        let mut last = "the relay has no reachable address".to_string();
        for endpoint in &target.endpoints {
            let addresses: Vec<SocketAddr> = match tokio::time::timeout(
                CONNECT_TIMEOUT,
                tokio::net::lookup_host(endpoint),
            )
            .await
            {
                Ok(Ok(addresses)) => addresses.collect(),
                Ok(Err(error)) => {
                    last = format!("resolve relay address {endpoint}: {error}");
                    continue;
                }
                Err(_) => {
                    last = format!("resolving relay address {endpoint} timed out");
                    continue;
                }
            };
            for address in addresses {
                if let Err(error) = authorize_server_dial_target(address, loopback) {
                    last = format!("relay address {address} is not allowed: {error:?}");
                    continue;
                }
                match tokio::time::timeout(CONNECT_TIMEOUT, connect(&config, address)).await {
                    Ok(Ok((stream, exporter))) => {
                        return Ok(Self {
                            stream,
                            exporter,
                            space,
                        });
                    }
                    Ok(Err(error)) => last = error,
                    Err(_) => last = format!("connecting to relay address {address} timed out"),
                }
            }
        }
        Err(RelayError::new(RelayProblem::Unreachable, last))
    }

    /// The signature context of this connection.
    pub fn binding(&self) -> RelayBinding {
        RelayBinding {
            exporter: *self.exporter,
            space: self.space,
        }
    }

    /// Sends one request and reads its answer.
    pub async fn request(&mut self, request: &RelayRequest) -> Result<RelayResponse, RelayError> {
        let exchange = async {
            write_frame(&mut self.stream, request).await?;
            read_frame(&mut self.stream, MAX_CONTROL_BYTES).await
        };
        let timeout = REQUEST_TIMEOUT.max(RELAY_WAIT_TIMEOUT * 2);
        tokio::time::timeout(timeout, exchange)
            .await
            .map_err(|_| {
                RelayError::new(
                    RelayProblem::Unreachable,
                    "the relay did not answer in time",
                )
            })?
            .map_err(|error| RelayError::new(RelayProblem::Unreachable, error))
    }

    /// Opens the exchange for the connection's space.
    pub async fn hello(&mut self) -> Result<(), RelayError> {
        let request = RelayRequest::Hello {
            protocol_version: RELAY_PROTOCOL_VERSION,
            space: self.space.to_hex(),
        };
        match self.request(&request).await? {
            RelayResponse::Welcome { protocol_version }
                if protocol_version == RELAY_PROTOCOL_VERSION =>
            {
                Ok(())
            }
            other => Err(unexpected("Welcome", &other)),
        }
    }

    /// Runs an admin command with `counter`. A stale counter is retried once with the counter
    /// the relay names. Returns the counter the relay applied.
    pub async fn admin(
        &mut self,
        admin: &AdminKeyPair,
        counter: u64,
        command: &RelayAdminCommand<'_>,
    ) -> Result<u64, RelayError> {
        let mut counter = counter.max(1);
        for _ in 0..2 {
            let request = self.admin_request(admin, counter, command)?;
            match self.request(&request).await? {
                RelayResponse::AdminApplied { counter: applied } => return Ok(applied),
                RelayResponse::Refused {
                    code: RelayRefusalCode::AdminCounter,
                    next_counter: Some(next),
                    ..
                } if next > counter => counter = next,
                other => return Err(unexpected("AdminApplied", &other)),
            }
        }
        Err(RelayError::new(
            RelayProblem::AdminRefused,
            "the relay kept refusing the admin counter",
        ))
    }

    fn admin_request(
        &self,
        admin: &AdminKeyPair,
        counter: u64,
        command: &RelayAdminCommand<'_>,
    ) -> Result<RelayRequest, RelayError> {
        let action = match command {
            RelayAdminCommand::AddKey(key) => RelayAdminAction::AddKey {
                key: encode_relay_bytes(key.as_bytes()),
            },
            RelayAdminCommand::RemoveKey(key) => RelayAdminAction::RemoveKey {
                key: encode_relay_bytes(key.as_bytes()),
            },
            RelayAdminCommand::Wipe => RelayAdminAction::Wipe,
            RelayAdminCommand::RemoveSpace => RelayAdminAction::RemoveSpace,
            RelayAdminCommand::RelayEpochRepair { epoch, access_key } => {
                RelayAdminAction::RelayEpochRepair {
                    epoch: *epoch,
                    access_key: encode_relay_bytes(access_key.as_bytes()),
                }
            }
            RelayAdminCommand::Claim { .. } => {
                return Err("a claim is not an admin request".to_string().into());
            }
        };
        let payload = self.binding().admin_payload(counter, command);
        Ok(RelayRequest::Admin {
            action,
            counter,
            signer: encode_relay_bytes(admin.public_key().as_bytes()),
            signature: encode_relay_bytes(&admin.sign(RELAY_ADMIN_DOMAIN, &payload)),
        })
    }

    /// Claims the space with a one-time setup token: its first epoch takes the access key of
    /// `epoch` and the relay's admin set is `admins`, `admin` among them.
    pub async fn claim(
        &mut self,
        token: &RelaySetupToken,
        epoch: u32,
        access: &RelayAccessKeyPair,
        admin: &AdminKeyPair,
        admins: &[AdminPublicKey],
    ) -> Result<(), RelayError> {
        let access_key = access.public_key();
        let payload = self.binding().admin_payload(
            1,
            &RelayAdminCommand::Claim {
                epoch,
                access_key: &access_key,
                admins,
            },
        );
        let request = RelayRequest::Claim {
            token: token.clone(),
            epoch,
            access_key: encode_relay_bytes(access_key.as_bytes()),
            admins: admins
                .iter()
                .map(|key| encode_relay_bytes(key.as_bytes()))
                .collect(),
            counter: 1,
            signer: encode_relay_bytes(admin.public_key().as_bytes()),
            signature: encode_relay_bytes(&admin.sign(RELAY_ADMIN_DOMAIN, &payload)),
        };
        match self.request(&request).await? {
            RelayResponse::Claimed { epoch: claimed } if claimed == epoch => Ok(()),
            other => Err(unexpected("Claimed", &other)),
        }
    }

    /// The last sequence of each writer the relay holds.
    async fn ranges(&mut self) -> Result<VersionVector, RelayError> {
        match self.request(&RelayRequest::State).await? {
            RelayResponse::State { writers, .. } => relay_ranges(&writers),
            other => Err(unexpected("State", &other)),
        }
    }
}

/// Claims `space` on the relay `target` names, see [`Connection::claim`].
pub async fn claim_space(
    target: &RelayTarget,
    space: SpaceId,
    loopback: LoopbackPolicy,
    token: &RelaySetupToken,
    first: &ganbaru_sync_contracts::SpaceKey,
    admin: &AdminKeyPair,
    admins: &[AdminPublicKey],
) -> Result<(), RelayError> {
    let access = first
        .relay_access_key(&space)
        .map_err(|error| format!("derive relay access key: {error}"))?;
    let mut connection = Connection::open(target, space, loopback).await?;
    connection.hello().await?;
    connection
        .claim(token, first.epoch(), &access, admin, admins)
        .await
}

/// Opens the exchange with the relay `target` names and returns the protocol version it
/// answers with. Needs no key, so it tells whether the relay answers at all.
pub async fn probe(
    target: &RelayTarget,
    space: SpaceId,
    loopback: LoopbackPolicy,
) -> Result<u16, RelayError> {
    let mut connection = Connection::open(target, space, loopback).await?;
    let request = RelayRequest::Hello {
        protocol_version: RELAY_PROTOCOL_VERSION,
        space: space.to_hex(),
    };
    match connection.request(&request).await? {
        RelayResponse::Welcome { protocol_version } => Ok(protocol_version),
        other => Err(unexpected("Welcome", &other)),
    }
}

/// Asks the relay `target` names to drop `space` and everything it stores for it, signed by
/// `admin`, which must be in the relay's admin set.
pub async fn remove_space(
    target: &RelayTarget,
    space: SpaceId,
    loopback: LoopbackPolicy,
    admin: &AdminKeyPair,
) -> Result<(), RelayError> {
    let mut connection = Connection::open(target, space, loopback).await?;
    connection.hello().await?;
    connection
        .admin(admin, 1, &RelayAdminCommand::RemoveSpace)
        .await?;
    Ok(())
}

async fn connect(
    config: &Arc<rustls::ClientConfig>,
    address: SocketAddr,
) -> Result<(TlsStream<TcpStream>, Zeroizing<[u8; RELAY_EXPORTER_BYTES]>), String> {
    let socket = TcpStream::connect(address)
        .await
        .map_err(|error| format!("connect to relay address {address}: {error}"))?;
    let name = ServerName::try_from(RELAY_SERVER_NAME)
        .map_err(|_| "relay server name is invalid".to_string())?;
    let stream = TlsConnector::from(Arc::clone(config))
        .connect(name, socket)
        .await
        .map_err(|error| format!("relay TLS handshake with {address}: {error}"))?;
    let exporter = stream
        .get_ref()
        .1
        .export_keying_material(
            Zeroizing::new([0u8; RELAY_EXPORTER_BYTES]),
            RELAY_EXPORTER_LABEL,
            None,
        )
        .map_err(|error| format!("relay TLS exporter: {error}"))?;
    Ok((stream, exporter))
}

fn client_config(fingerprint: &[u8; 32]) -> Result<Arc<rustls::ClientConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinnedVerifier {
        fingerprint: fingerprint
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        algorithms: provider.signature_verification_algorithms,
    });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|error| format!("relay TLS configuration: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Accepts exactly the certificate whose SHA-256 the server configuration pins, with no chain.
#[derive(Debug)]
struct PinnedVerifier {
    fingerprint: String,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if !intermediates.is_empty()
            || certificate_fingerprint(end_entity.as_ref()) != self.fingerprint
        {
            return Err(rustls::Error::General(
                "relay certificate is not the pinned one".to_string(),
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, certificate, signature, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, certificate, signature, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn writer(byte: u8) -> WriterId {
        WriterId::from_bytes([byte; ganbaru_sync_contracts::ids::ID_BYTES])
    }

    fn vector(entries: &[(u8, u64)]) -> VersionVector {
        let mut vector = VersionVector::new();
        for &(byte, seq) in entries {
            vector.set(writer(byte), seq);
        }
        vector
    }

    #[test]
    fn undelivered_counts_only_own_operations_the_relay_lacks() {
        let own = BTreeSet::from([writer(1), writer(2)]);
        let stored = vector(&[(1, 10), (2, 4), (3, 50)]);
        let reached = vector(&[(1, 7), (3, 1)]);
        assert_eq!(
            undelivered_count(&own, &stored, &reached, &VersionVector::new()),
            3 + 4
        );
        let compacted = vector(&[(2, 4)]);
        assert_eq!(undelivered_count(&own, &stored, &reached, &compacted), 3);
        let ahead = vector(&[(1, 12), (2, 9)]);
        assert_eq!(
            undelivered_count(&own, &stored, &ahead, &VersionVector::new()),
            0
        );
        assert_eq!(
            undelivered_count(&BTreeSet::new(), &stored, &reached, &compacted),
            0
        );
    }

    #[test]
    fn relay_refusals_map_to_problems_and_count_as_contact() {
        assert_eq!(
            RelayProblem::of_refusal(RelayRefusalCode::Version),
            RelayProblem::Incompatible
        );
        assert_eq!(
            RelayProblem::of_refusal(RelayRefusalCode::Token),
            RelayProblem::SetupCodeRefused
        );
        assert_eq!(
            RelayProblem::of_refusal(RelayRefusalCode::StaleEpoch),
            RelayProblem::AccessRefused
        );
        assert_eq!(
            RelayProblem::of_refusal(RelayRefusalCode::AdminCounter),
            RelayProblem::AdminRefused
        );
        assert!(RelayProblem::of_refusal(RelayRefusalCode::Busy).relay_answered());
        assert!(!RelayProblem::Unreachable.relay_answered());
        assert!(!RelayProblem::Failed.relay_answered());
        assert_eq!(
            RelayError::from("disk full".to_string()).problem,
            RelayProblem::Failed
        );
    }
}
