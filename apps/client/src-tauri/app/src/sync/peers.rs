//! LAN peer exchange glue for the active vault: admission of member listeners from the selected
//! log, and the device-local peer book of member endpoints that hubs report and dials use.

#[cfg(desktop)]
use super::host::SyncHost;
use super::service::blocking;
use super::transport::MemberTransport;
use crate::vault::handoff::pairing::PairingManager;
#[cfg(desktop)]
use ganbaru_handoff::protocol::MAX_MEMBER_ENDPOINTS;
use ganbaru_handoff::protocol::MemberEndpoint;
#[cfg(desktop)]
use ganbaru_sync::local;
use ganbaru_sync::{AuthorizationView, Engine, SpaceContext};
use ganbaru_sync_contracts::{DeviceKind, VersionVector, WriterId};
use ganbaru_sync_replica::client::{ClientSession, Exchange, Exchanged, PeerIds};
#[cfg(desktop)]
use ganbaru_sync_replica::peers::admission::MemberAdmission;
use ganbaru_sync_replica::peers::admission::fingerprint_hex;
use ganbaru_sync_replica::peers::anchor_pin::AnchorPin;
use ganbaru_sync_replica::peers::book::{PeerBook, PeerEndpoints};
use ganbaru_sync_replica::peers::plan::{PlanInput, PlannedPeer, plan_pass};
use ganbaru_sync_replica::quarantine::PullSkips;
#[cfg(desktop)]
use ganbaru_sync_replica::quarantine::Quarantine;
use ganbaru_sync_replica::writer::SyncFiles;
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
pub(super) async fn record_reported_peers(
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
    let received_ms = super::now_ms()?;
    blocking(move || {
        update_peer_book(&path, |book| {
            book.observe_reported(&reported, received_ms);
            if let Some(members) = &members {
                book.retain_members(members);
            }
        })
        .map(drop)
    })
    .await
}

/// The selected authorization state of the active vault, or `None` while sync does not run
/// there or the space is not known yet.
#[cfg(desktop)]
pub(super) async fn selected_authorization<H: SyncHost>(
    host: &H,
) -> Result<Option<(String, AuthorizationView)>, String> {
    if !host.sync_enabled()? {
        return Ok(None);
    }
    let Some(pool) = host.connect_for_sync().await? else {
        return Ok(None);
    };
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
pub(super) fn live_members(
    view: &AuthorizationView,
    quarantined: &BTreeSet<String>,
) -> BTreeSet<String> {
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
#[cfg(desktop)]
async fn discovered_members(
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
    crate::vault::handoff::discovery::discover_members(vault_id, &fingerprints)
        .await
        .into_iter()
        .map(|(device_id, endpoints)| {
            let endpoints = endpoints.iter().map(ToString::to_string).collect();
            (device_id, endpoints)
        })
        .collect()
}

#[cfg(mobile)]
async fn discovered_members(
    _vault_id: &str,
    _own_device_id: &str,
    _view: &AuthorizationView,
    _members: &BTreeSet<String>,
) -> BTreeMap<String, Vec<String>> {
    BTreeMap::new()
}

/// Admission of the active vault's member listener: the selected log and this installation's
/// quarantine. `None` while sync does not run there.
#[cfg(desktop)]
pub(crate) async fn member_admission<H: SyncHost>(
    host: &H,
) -> Result<Option<MemberAdmission>, String> {
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

/// Applies `change` to the peer book at `path` and writes it when it changed.
pub(super) fn update_peer_book(
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
#[cfg(desktop)]
pub(crate) fn read_peer_book<H: SyncHost>(host: &H, vault_id: &str) -> PeerEndpoints {
    host.sync_files(vault_id)
        .and_then(|files| PeerBook::load(&files.peers_path()))
        .map(|book| book.endpoints().clone())
        .unwrap_or_default()
}

/// What a hub reports as member endpoints: its own listener endpoints first, then the peer
/// book's newest entries for other devices, at most [`MAX_MEMBER_ENDPOINTS`].
#[cfg(desktop)]
pub(crate) fn reported_members(
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
pub(super) struct MemberPass<'a> {
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
}

/// What the member exchanges of a pass moved.
#[derive(Default)]
pub(super) struct MemberPassOutcome {
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
pub(super) async fn exchange_with_members(
    pass: MemberPass<'_>,
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
    let discovered =
        discovered_members(&pass.ids.vault_id, &pass.ids.device_id, &view, &members).await;
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
    let transport = |peer: &PlannedPeer, fingerprint: &str| MemberTransport {
        manager: pass.manager.clone(),
        endpoints: peer.endpoints.clone(),
        fingerprint: fingerprint.to_string(),
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
                max_bytes: super::client::MAX_EXCHANGE_BYTES,
                listening: pass.listening,
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
async fn record_direct_anchor_marks(
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
/// book after a handover. The coordinator pin is only read, so a handover never changes it or
/// its ownership generation.
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
#[cfg(desktop)]
pub(crate) fn peer_hints<H>(host: H) -> super::hub::HubPeerHints
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
            tauri::async_runtime::spawn_blocking(move || {
                let observed = super::now_ms().unwrap_or_default();
                if let Err(error) = update_peer_book(&path, |book| {
                    book.observe(&device_id, &listening, observed);
                }) {
                    eprintln!("sync: record member endpoints: {error}");
                }
            });
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{AnchorDial, anchor_dial};
    use ganbaru_handoff::pairing::CoordinatorPin;
    use ganbaru_sync_replica::peers::anchor_pin::AnchorPin;
    use ganbaru_sync_replica::peers::book::PeerEndpoints;

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
            generation: 3,
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
        // The pin stays as linked: same coordinator and generation.
        assert_eq!(
            (pin.device_id.as_str(), pin.generation),
            ("device-old-anchor", 3)
        );
        // Until a member reports the new anchor, it has no endpoints to dial.
        let unknown = anchor_dial(
            &anchor("device-new-anchor", 'b'),
            &pin,
            &PeerEndpoints::default(),
        );
        assert!(unknown.endpoints.is_empty());
    }
}
