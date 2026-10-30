//! LAN peer exchange glue for the active vault: admission of member listeners from the selected
//! log, and the device-local peer book of member endpoints that hubs report and dials use.

use super::blocking;
use super::host::SyncHost;
use super::transport::MemberTransport;
use crate::client::{ClientSession, Exchange, Exchanged, PeerIds};
use crate::peers::admission::MemberAdmission;
use crate::peers::admission::fingerprint_hex;
use crate::peers::anchor_pin::AnchorPin;
use crate::peers::book::{PeerBook, PeerEndpoints};
use crate::peers::plan::{PlanInput, PlannedPeer, plan_pass};
use crate::quarantine::PullSkips;
use crate::quarantine::Quarantine;
use crate::records::{RecordExchange, RecordLink};
use crate::writer::SyncFiles;
use ganbaru_handoff::pairing::PairingManager;
use ganbaru_handoff::protocol::MAX_MEMBER_ENDPOINTS;
use ganbaru_handoff::protocol::MemberEndpoint;
use ganbaru_sync::local;
use ganbaru_sync::{AuthorizationView, Engine, SpaceContext};
use ganbaru_sync_contracts::{DeviceKind, VersionVector, WriterId};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::Poll;

/// Serializes the read, change, and write of a peer book within this process.
static PEER_BOOK_LOCK: Mutex<()> = Mutex::new(());

/// Records the member endpoints a hub reported and rebuilds the peer book against the selected
/// log: only devices enrolled, not removed, and not quarantined keep endpoints.
pub async fn record_reported_peers(
    path: PathBuf,
    engine: &Engine,
    pool: &SqlitePool,
    ctx: &SpaceContext,
    quarantined: &BTreeSet<String>,
    reported: Vec<MemberEndpoint>,
) -> Result<(), String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let view = engine
        .authorization(&mut conn, ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?;
    drop(conn);
    let members = view.map(|view| live_members(&view, quarantined));
    let received_ms = crate::now_ms()?;
    blocking(move || {
        update_peer_book(&path, |book| {
            // Without a selected log no device is known to be a member, so reports wait.
            if let Some(members) = &members {
                book.observe_reported(&reported, members, received_ms);
                book.retain_members(members);
            }
        })
        .map(drop)
    })
    .await
}

/// The selected authorization state of the active vault, or `None` while sync does not run
/// there or the space is not known yet.
pub async fn selected_authorization<H: SyncHost>(
    host: &H,
) -> Result<Option<(String, AuthorizationView)>, String> {
    if !host.sync_enabled()? {
        return Ok(None);
    }
    let pool = host.connect_for_sync().await?;
    let vault_id = host.active_vault_id()?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let Some(ctx) = local::space_context(&mut conn, &vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(None);
    };
    let view = Engine::vault()
        .authorization(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?;
    Ok(view.map(|view| (vault_id, view)))
}

/// The devices of `view` that are enrolled and not removed, minus quarantined ones.
pub fn live_members(view: &AuthorizationView, quarantined: &BTreeSet<String>) -> BTreeSet<String> {
    view.devices
        .iter()
        .filter(|device| {
            device.removed_revision.is_none() && !quarantined.contains(&device.device_id)
        })
        .map(|device| device.device_id.clone())
        .collect()
}

/// Endpoints of `members` that answered LAN discovery, found by the certificate fingerprints
/// their enrollments recorded. Only desktops browse; a phone plans from the anchor and the peer
/// book alone.
async fn discovered_members<H: SyncHost>(
    host: &H,
    vault_id: &str,
    own_device_id: &str,
    view: &AuthorizationView,
    members: &BTreeSet<String>,
) -> BTreeMap<String, Vec<String>> {
    let fingerprints: Vec<(String, [u8; 32])> = view
        .devices
        .iter()
        .filter(|device| device.device_id != own_device_id && members.contains(&device.device_id))
        .map(|device| (device.device_id.clone(), device.tls_fingerprint))
        .collect();
    if fingerprints.is_empty() {
        return BTreeMap::new();
    }
    host.discover_members(vault_id, &fingerprints).await
}

/// Admission of the active vault's member listener: the selected log and this installation's
/// quarantine. `None` while sync does not run there.
pub async fn member_admission<H: SyncHost>(host: &H) -> Result<Option<MemberAdmission>, String> {
    let Some((vault_id, view)) = selected_authorization(host).await? else {
        return Ok(None);
    };
    let path = host.sync_files(&vault_id)?.quarantine_path();
    let quarantined = blocking(move || Quarantine::load(&path)).await?;
    let own_device_id = host.pairing().device_id()?;
    Ok(Some(
        MemberAdmission::new(&vault_id, &view, quarantined.devices()).served_by(&own_device_id),
    ))
}

#[cfg(desktop)]
/// Admission from the selected log alone, for the coordinator listener, which applies the
/// quarantine through its own list with a last-known fallback. `None` while sync does not run
/// there.
pub async fn log_admission<H: SyncHost>(host: &H) -> Result<Option<MemberAdmission>, String> {
    let Some((vault_id, view)) = selected_authorization(host).await? else {
        return Ok(None);
    };
    let own_device_id = host.pairing().device_id()?;
    Ok(Some(
        MemberAdmission::new(&vault_id, &view, &std::collections::BTreeSet::new())
            .served_by(&own_device_id),
    ))
}

/// Applies `change` to the peer book at `path` and writes it when it changed.
pub fn update_peer_book(
    path: &Path,
    change: impl FnOnce(&mut PeerEndpoints),
) -> Result<PeerEndpoints, String> {
    let _guard = PEER_BOOK_LOCK
        .lock()
        .map_err(|_| "sync peer book lock is unavailable".to_string())?;
    let mut book = match PeerBook::load(path) {
        Ok(book) => book,
        Err(error) => {
            // The book only holds address hints, so a damaged one starts over.
            eprintln!("sync: {error}; starting a new peer book");
            if let Err(error) = std::fs::remove_file(path) {
                eprintln!("sync: remove the damaged peer book: {error}");
            }
            PeerBook::load(path)?
        }
    };
    let mut endpoints = book.endpoints().clone();
    change(&mut endpoints);
    book.store(endpoints.clone())?;
    Ok(endpoints)
}

/// The peer book of `vault_id`, empty when it cannot be read.
pub fn read_peer_book<H: SyncHost>(host: &H, vault_id: &str) -> PeerEndpoints {
    host.sync_files(vault_id)
        .and_then(|files| PeerBook::load(&files.peers_path()))
        .map(|book| book.endpoints().clone())
        .unwrap_or_default()
}

/// What a hub reports as member endpoints: its own listener endpoints first, then the peer
/// book's newest entries for other devices, at most [`MAX_MEMBER_ENDPOINTS`].
pub fn reported_members(
    own_device_id: &str,
    own: Vec<MemberEndpoint>,
    book: &PeerEndpoints,
) -> Vec<MemberEndpoint> {
    let mut members = own;
    members.truncate(MAX_MEMBER_ENDPOINTS);
    for member in book.report() {
        if members.len() >= MAX_MEMBER_ENDPOINTS {
            break;
        }
        if member.device_id != own_device_id {
            members.push(member);
        }
    }
    members
}

/// What one pass needs to exchange with other members besides its coordinator exchange.
pub struct MemberPass<'a, H> {
    pub host: &'a H,
    pub manager: &'a PairingManager,
    pub ids: &'a PeerIds,
    pub engine: &'a Engine,
    pub pool: &'a SqlitePool,
    pub ctx: &'a SpaceContext,
    pub writer: Option<WriterId>,
    pub own_writers: &'a BTreeSet<WriterId>,
    pub quarantined: &'a BTreeSet<String>,
    pub pull_skips: &'a PullSkips,
    pub listening: &'a [String],
    pub files: &'a SyncFiles,
    /// Record exchange state shared with this device's hubs.
    pub records: &'a RecordExchange,
}

/// What the member exchanges of a pass moved.
#[derive(Default)]
pub struct MemberPassOutcome {
    /// Operations pulled from members.
    pub pulled: usize,
    /// A link to the anchor when it is not the coordinator and answered this pass, pinned to
    /// the certificate its enrollment recorded, so waiting proposals go to it directly instead
    /// of waiting at the coordinator for its forward request.
    pub anchor: Option<MemberTransport>,
    /// What this device knows after a complete exchange with that anchor, which a long poll to
    /// it starts from while the coordinator is away.
    pub anchor_known: Option<VersionVector>,
}

/// Exchanges with the members the pass plan picks, at most the device kind's limit at once.
///
/// The coordinator exchange already ran with the pinned coordinator, so it is skipped here
/// while the coordinator is the anchor. After a handover the anchor pin follows the log to the
/// new anchor, which is dialed at the endpoints the peer book holds for it and pinned to the
/// certificate its enrollment recorded; the waiting proposals go to it directly, so its answer
/// arrives over a link only the anchor can complete. Every other member is pinned the same way
/// and never receives proposals. What a member refuses, such as an operation of a writer it
/// does not know yet, is logged and retried on the next pass instead of becoming this device's
/// status.
pub async fn exchange_with_members<H: SyncHost>(
    pass: MemberPass<'_, H>,
    coordinator_reachable: bool,
) -> Result<MemberPassOutcome, String> {
    let Some(pin) = pass.manager.coordinator_pin()? else {
        return Ok(MemberPassOutcome::default());
    };
    let view = {
        let mut conn = pass
            .pool
            .acquire()
            .await
            .map_err(|error| format!("acquire sync connection: {error}"))?;
        pass.engine
            .authorization(&mut conn, pass.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
    };
    let anchor_pin = {
        let path = pass.files.anchor_path();
        let vault_id = pass.ids.vault_id.clone();
        let view = view.clone();
        blocking(move || AnchorPin::follow(&path, &vault_id, view.as_ref())).await?
    };
    let Some(view) = view else {
        return Ok(MemberPassOutcome::default());
    };
    let mut members = live_members(&view, pass.quarantined);
    let path = pass.files.peers_path();
    let known = blocking(move || {
        Ok(PeerBook::load(&path)
            .map(|book| book.endpoints().clone())
            .unwrap_or_default())
    })
    .await?;
    let kind = view
        .devices
        .iter()
        .find(|device| device.device_id == pass.ids.device_id)
        .map_or(DeviceKind::Desktop, |device| device.kind);
    let dial = anchor_pin
        .as_ref()
        .map(|anchor| anchor_dial(anchor, &pin, &known));
    // The coordinator exchange already covers an anchor that is the coordinator, so it takes no
    // slot of the fan-out.
    let direct_anchor = dial.as_ref().filter(|dial| !dial.anchor_is_coordinator);
    if direct_anchor.is_none() {
        members.remove(&pin.device_id);
    }
    let discovered = discovered_members(
        pass.host,
        &pass.ids.vault_id,
        &pass.ids.device_id,
        &view,
        &members,
    )
    .await;
    let plan = plan_pass(&PlanInput {
        kind,
        own_device_id: &pass.ids.device_id,
        members: &members,
        anchor: direct_anchor.map(|dial| (dial.device_id.as_str(), dial.endpoints.as_slice())),
        anchor_reachable: direct_anchor.is_some() || coordinator_reachable,
        discovered: &discovered,
        known: &known,
    });
    let targets: Vec<(PlannedPeer, String)> = plan
        .peers
        .into_iter()
        .filter(|peer| !peer.endpoints.is_empty())
        .filter_map(|peer| {
            let fingerprint = match direct_anchor {
                Some(anchor) if anchor.device_id == peer.device_id => anchor.fingerprint.clone(),
                _ => view
                    .devices
                    .iter()
                    .find(|device| device.device_id == peer.device_id)
                    .map(|device| fingerprint_hex(&device.tls_fingerprint))?,
            };
            Some((peer, fingerprint))
        })
        .collect();
    let dialer = pass.host.dialer();
    let transport = |peer: &PlannedPeer, fingerprint: &str| MemberTransport {
        manager: pass.manager.clone(),
        device_id: peer.device_id.clone(),
        endpoints: peer.endpoints.clone(),
        fingerprint: fingerprint.to_string(),
        dialer: dialer.clone(),
    };
    let exchanges = targets.iter().map(|(peer, fingerprint)| {
        let pass = &pass;
        let transport = transport(peer, fingerprint);
        async move {
            let session = ClientSession {
                transport: &transport,
                ids: pass.ids,
                engine: pass.engine,
                pool: pass.pool,
                ctx: pass.ctx,
                writer: pass.writer,
                own_writers: pass.own_writers,
                quarantined: pass.quarantined,
                pull_skips: Some(pass.pull_skips),
                max_bytes: crate::client::MAX_EXCHANGE_BYTES,
                listening: pass.listening,
                records: Some(RecordLink {
                    state: pass.records,
                    peer: &peer.device_id,
                }),
            };
            (peer.device_id.clone(), session.exchange().await)
        }
    });
    let mut outcome = MemberPassOutcome::default();
    let mut reported = Vec::new();
    let mut anchor_answered = false;
    for (device_id, result) in join_all(exchanges.collect()).await {
        match result {
            Ok(Exchange::Done(done)) => {
                outcome.pulled += done.pulled;
                if device_id == view.anchor_device_id {
                    anchor_answered = true;
                    record_direct_anchor_marks(pass.engine, pass.pool, pass.ctx, &view, &done)
                        .await?;
                    if done.problem.is_none() && !done.more && !done.bootstrap_required {
                        outcome.anchor_known = Some(done.known.clone());
                    }
                }
                if let Some(problem) = done.problem {
                    eprintln!("sync: member {device_id} refused part of an exchange: {problem}");
                }
                reported.extend(done.members);
            }
            Ok(Exchange::Fork(fork)) => eprintln!(
                "sync: member {device_id} reported a fork of writer {} at {}; the coordinator \
                 exchange recovers it",
                fork.writer.to_hex(),
                fork.keep_through
            ),
            Err(error) => eprintln!("sync: exchange with member {device_id}: {error}"),
        }
    }
    if anchor_answered {
        outcome.anchor = targets
            .iter()
            .find(|(peer, _)| peer.device_id == view.anchor_device_id)
            .map(|(peer, fingerprint)| transport(peer, fingerprint));
    }
    // Also prunes the book against the log when no member reported anything; the file is
    // written only when its content changed.
    record_reported_peers(
        pass.files.peers_path(),
        pass.engine,
        pass.pool,
        pass.ctx,
        pass.quarantined,
        reported,
    )
    .await?;
    Ok(outcome)
}

/// Records what the log's anchor reported storing in a direct exchange as anchor marks, so
/// displaced versions it covers are released without waiting for the coordinator to relay it.
pub(super) async fn record_direct_anchor_marks(
    engine: &Engine,
    pool: &SqlitePool,
    ctx: &SpaceContext,
    view: &AuthorizationView,
    done: &Exchanged,
) -> Result<(), String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    engine
        .record_anchor_marks(&mut conn, ctx, &view.anchor_device_id, &done.hub)
        .await
        .map_err(|error| format!("record anchor marks: {error}"))?;
    Ok(())
}

/// How a pass reaches the anchor its pin names.
#[derive(Debug, PartialEq, Eq)]
struct AnchorDial {
    /// Handoff device id of the anchor.
    device_id: String,
    /// The certificate fingerprint every dial to the anchor pins.
    fingerprint: String,
    /// Whether the pinned coordinator is the anchor, so the coordinator exchange covers it.
    anchor_is_coordinator: bool,
    /// The endpoints to dial the anchor at.
    endpoints: Vec<String>,
}

/// Reaches the anchor through the coordinator pin while it is the anchor, and through the peer
/// book after a handover. The coordinator pin is only read, so a handover never changes it.
fn anchor_dial(
    anchor: &AnchorPin,
    pin: &ganbaru_handoff::pairing::CoordinatorPin,
    known: &PeerEndpoints,
) -> AnchorDial {
    let anchor_is_coordinator = anchor.device_id == pin.device_id;
    AnchorDial {
        device_id: anchor.device_id.clone(),
        fingerprint: anchor.certificate_fingerprint.clone(),
        anchor_is_coordinator,
        endpoints: if anchor_is_coordinator {
            pin.reported_endpoints.clone()
        } else {
            known.endpoints(&anchor.device_id)
        },
    }
}

/// Drives every future in `futures` concurrently and returns their outputs in order.
async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut pending: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut outputs: Vec<Option<F::Output>> = pending.iter().map(|_| None).collect();
    std::future::poll_fn(|context| {
        let mut ready = true;
        for (future, output) in pending.iter_mut().zip(outputs.iter_mut()) {
            if output.is_none() {
                match future.as_mut().poll(context) {
                    Poll::Ready(value) => *output = Some(value),
                    Poll::Pending => ready = false,
                }
            }
        }
        if ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    outputs.into_iter().flatten().collect()
}

/// Records in the peer book the listener endpoints an authenticated member reports in its
/// hello, off the async runtime.
pub fn peer_hints<H>(host: H) -> crate::hub::HubPeerHints
where
    H: SyncHost + Clone + Send + Sync + 'static,
{
    std::sync::Arc::new(
        move |vault_id: &str, device_id: &str, listening: &[String]| {
            if listening.is_empty() {
                return;
            }
            let path = match host.sync_files(vault_id) {
                Ok(files) => files.peers_path(),
                Err(error) => {
                    eprintln!("sync: record member endpoints: {error}");
                    return;
                }
            };
            let device_id = device_id.to_string();
            let listening = listening.to_vec();
            tokio::task::spawn_blocking(move || {
                let observed = crate::now_ms().unwrap_or_default();
                if let Err(error) = update_peer_book(&path, |book| {
                    book.observe(&device_id, &listening, observed);
                }) {
                    eprintln!("sync: record member endpoints: {error}");
                }
            });
        },
    )
}

/// The member desktops a device may take a vault copy from after the coordinator it enrolled
/// with, from the selected log of the active vault `vault_id`. Empty while the active vault is
/// another one or sync does not run there, as on a first link: without the log no other
/// device's certificate is known, so the coordinator is the only source.
pub async fn bootstrap_members<H: SyncHost>(
    host: &H,
    vault_id: &str,
    coordinator_device_id: &str,
) -> Result<Vec<BundleSource>, String> {
    let Some((active_vault_id, view)) = selected_authorization(host).await? else {
        return Ok(Vec::new());
    };
    if active_vault_id != vault_id {
        return Ok(Vec::new());
    }
    let path = host.sync_files(vault_id)?.quarantine_path();
    let quarantined = blocking(move || Quarantine::load(&path)).await?;
    let own_device_id = host.pairing().device_id()?;
    let members = serving_members(
        &view,
        &own_device_id,
        coordinator_device_id,
        quarantined.devices(),
    );
    let known = read_peer_book(host, vault_id);
    let discovered = discovered_members(host, vault_id, &own_device_id, &view, &members).await;
    let mut sources = bootstrap_sources(&view, &members, &discovered, &known);
    sources.extend(server_source(&view, &own_device_id, quarantined.devices()));
    Ok(sources)
}

/// The anchor server of `view` as a copy source, unless it is this device or quarantined here.
fn server_source(
    view: &AuthorizationView,
    own_device_id: &str,
    quarantined: &BTreeSet<String>,
) -> Option<BundleSource> {
    let target = crate::anchor_link::ServerTarget::of(view)?;
    let device_id = target.device_id(view)?;
    (device_id != own_device_id && !quarantined.contains(device_id))
        .then_some(BundleSource::Server(target))
}

/// The live members of `view` that serve vault copies besides the coordinator: computers other
/// than this device. Phones serve no copies.
fn serving_members(
    view: &AuthorizationView,
    own_device_id: &str,
    coordinator_device_id: &str,
    quarantined: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut members = live_members(view, quarantined);
    members.retain(|device_id| {
        device_id != own_device_id
            && device_id != coordinator_device_id
            && view
                .devices
                .iter()
                .any(|device| &device.device_id == device_id && device.kind.may_be_admin())
    });
    members
}

/// One copy source per member with a known endpoint, pinned to the certificate its enrollment
/// recorded in `view`. Discovered endpoints come before those the peer book holds.
fn bootstrap_sources(
    view: &AuthorizationView,
    members: &BTreeSet<String>,
    discovered: &BTreeMap<String, Vec<String>>,
    known: &PeerEndpoints,
) -> Vec<BundleSource> {
    members
        .iter()
        .filter_map(|device_id| {
            let fingerprint = view
                .devices
                .iter()
                .find(|device| &device.device_id == device_id)
                .map(|device| fingerprint_hex(&device.tls_fingerprint))?;
            let mut endpoints: Vec<String> = Vec::new();
            let found = discovered.get(device_id).cloned().unwrap_or_default();
            for endpoint in found.into_iter().chain(known.endpoints(device_id)) {
                if !endpoints.contains(&endpoint) {
                    endpoints.push(endpoint);
                }
            }
            endpoints.truncate(MAX_MEMBER_ENDPOINTS);
            (!endpoints.is_empty()).then(|| BundleSource::Member {
                device_id: device_id.clone(),
                endpoints,
                fingerprint,
            })
        })
        .collect()
}

/// The device a vault copy comes from: the coordinator this device is pinned to, or a member
/// listener pinned to the certificate fingerprint its enrollment recorded in the selected log, or
/// the anchor server the selected log configures, dialed under the server rule, or the server a
/// desktop recovered through, which it knows from the setup code and has no pin or log for yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleSource {
    Coordinator,
    Member {
        device_id: String,
        endpoints: Vec<String>,
        fingerprint: String,
    },
    Server(crate::anchor_link::ServerTarget),
    Recovery {
        target: crate::anchor_link::ServerTarget,
        vault_id: String,
    },
}

impl BundleSource {
    /// A short name for logs and error messages.
    pub fn describe(&self) -> String {
        match self {
            Self::Coordinator => "the coordinator".to_string(),
            Self::Member { device_id, .. } => format!("member {device_id}"),
            Self::Server(_) | Self::Recovery { .. } => "the anchor server".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnchorDial, AuthorizationView, BTreeMap, BTreeSet, BundleSource, DeviceKind, anchor_dial,
        bootstrap_sources, fingerprint_hex, server_source, serving_members,
    };
    use crate::peers::anchor_pin::AnchorPin;
    use crate::peers::book::PeerEndpoints;
    use ganbaru_handoff::pairing::CoordinatorPin;
    use ganbaru_sync::EnrolledDevice;
    use ganbaru_sync_contracts::authorization::{Server, ServerMode};
    use ganbaru_sync_contracts::{AdminPublicKey, DevicePublicKey, DeviceRole};

    fn anchor(device_id: &str, fingerprint: char) -> AnchorPin {
        AnchorPin {
            vault_id: "vault-1".to_string(),
            device_id: device_id.to_string(),
            certificate_fingerprint: fingerprint.to_string().repeat(64),
            epoch: 2,
        }
    }

    fn pin() -> CoordinatorPin {
        CoordinatorPin {
            device_id: "device-old-anchor".to_string(),
            device_label: None,
            endpoint: "192.168.1.20:43821".to_string(),
            vault_id: "vault-1".to_string(),
            certificate_fingerprint: "a".repeat(64),
            last_endpoint: None,
            reported_endpoints: vec!["192.168.1.20:43821".to_string()],
            charter: None,
        }
    }

    #[test]
    fn the_pinned_coordinator_is_dialed_while_it_is_the_anchor() {
        assert_eq!(
            anchor_dial(
                &anchor("device-old-anchor", 'a'),
                &pin(),
                &PeerEndpoints::default()
            ),
            AnchorDial {
                device_id: "device-old-anchor".to_string(),
                fingerprint: "a".repeat(64),
                anchor_is_coordinator: true,
                endpoints: vec!["192.168.1.20:43821".to_string()],
            }
        );
    }

    #[test]
    fn a_handover_reaches_the_new_anchor_through_the_peer_book_without_relinking() {
        let pin = pin();
        let mut known = PeerEndpoints::default();
        known.observe(
            "device-new-anchor",
            &["192.168.1.30:43821".to_string()],
            1_700_000_000_000,
        );
        assert_eq!(
            anchor_dial(&anchor("device-new-anchor", 'b'), &pin, &known),
            AnchorDial {
                device_id: "device-new-anchor".to_string(),
                // Dials to the new anchor pin its enrollment, never the coordinator certificate.
                fingerprint: "b".repeat(64),
                anchor_is_coordinator: false,
                endpoints: vec!["192.168.1.30:43821".to_string()],
            }
        );
        // The pin stays as linked: same coordinator.
        assert_eq!(pin.device_id, "device-old-anchor");
        // Until a member reports the new anchor, it has no endpoints to dial.
        let unknown = anchor_dial(
            &anchor("device-new-anchor", 'b'),
            &pin,
            &PeerEndpoints::default(),
        );
        assert!(unknown.endpoints.is_empty());
    }

    fn enrolled(
        device_id: &str,
        fingerprint: u8,
        kind: DeviceKind,
        removed: bool,
    ) -> EnrolledDevice {
        EnrolledDevice {
            device_id: device_id.to_string(),
            device_key: DevicePublicKey::from_bytes([3; 32]),
            tls_fingerprint: [fingerprint; 32],
            label: None,
            kind,
            role: DeviceRole::Member,
            enrolled_revision: 2,
            removed_revision: removed.then_some(3),
            encryption_key: None,
        }
    }

    /// A log with the coordinator, this phone, two live desktops, a removed desktop, a
    /// quarantined desktop, and another phone.
    fn view() -> AuthorizationView {
        AuthorizationView {
            epoch: 1,
            revision: 4,
            entry_hash: [7; 32],
            anchor_device_id: "device-coordinator".to_string(),
            frozen_at: None,
            catch_up: None,
            charter: [8; 32],
            recovery_admin_key: AdminPublicKey::from_bytes([9; 32]),
            kit_admin_key: AdminPublicKey::from_bytes([9; 32]),
            devices: vec![
                enrolled("device-coordinator", 1, DeviceKind::Desktop, false),
                enrolled("device-self", 2, DeviceKind::Mobile, false),
                enrolled("device-laptop", 3, DeviceKind::Desktop, false),
                enrolled("device-office", 4, DeviceKind::Desktop, false),
                enrolled("device-removed", 5, DeviceKind::Desktop, true),
                enrolled("device-quarantined", 6, DeviceKind::Desktop, false),
                enrolled("device-tablet", 7, DeviceKind::Mobile, false),
            ],
            discovery_key: [5; 32],
            previous_discovery_key: None,
            key_epoch: 0,
            recovery_encryption_key: None,
            pending_removal: None,
            server: None,
        }
    }

    #[test]
    fn the_anchor_server_serves_copies_unless_it_is_quarantined_or_this_device() {
        let mut view = view();
        let none = BTreeSet::new();
        assert_eq!(server_source(&view, "device-self", &none), None);
        let server = Server {
            mode: ServerMode::Anchor,
            endpoints: vec!["server.example:43823".to_string()],
            certificate_fingerprint: [3; 32],
        };
        view.server = Some(server.clone());
        assert!(matches!(
            server_source(&view, "device-self", &none),
            Some(BundleSource::Server(target)) if target.certificate_fingerprint == [3; 32]
        ));
        assert_eq!(server_source(&view, "device-laptop", &none), None);
        let quarantined = BTreeSet::from(["device-laptop".to_string()]);
        assert_eq!(server_source(&view, "device-self", &quarantined), None);
        view.server = Some(Server {
            mode: ServerMode::Relay,
            ..server
        });
        assert_eq!(
            server_source(&view, "device-self", &none),
            None,
            "a relay holds no vault to copy"
        );
    }

    #[test]
    fn only_live_member_desktops_besides_the_coordinator_serve_copies() {
        let quarantined = BTreeSet::from(["device-quarantined".to_string()]);
        assert_eq!(
            serving_members(&view(), "device-self", "device-coordinator", &quarantined),
            BTreeSet::from(["device-laptop".to_string(), "device-office".to_string()])
        );
    }

    #[test]
    fn copy_sources_pin_the_enrolled_certificate_and_prefer_discovered_endpoints() {
        let members = BTreeSet::from(["device-laptop".to_string(), "device-office".to_string()]);
        let discovered = BTreeMap::from([(
            "device-laptop".to_string(),
            vec!["192.168.1.40:43821".to_string()],
        )]);
        let mut known = PeerEndpoints::default();
        known.observe(
            "device-laptop",
            &[
                "192.168.1.40:43821".to_string(),
                "192.168.1.41:43821".to_string(),
            ],
            1_700_000_000_000,
        );
        assert_eq!(
            bootstrap_sources(&view(), &members, &discovered, &known),
            vec![BundleSource::Member {
                device_id: "device-laptop".to_string(),
                endpoints: vec![
                    "192.168.1.40:43821".to_string(),
                    "192.168.1.41:43821".to_string(),
                ],
                fingerprint: fingerprint_hex(&[3; 32]),
            }],
            "a member without a known endpoint is no source"
        );
    }
}
