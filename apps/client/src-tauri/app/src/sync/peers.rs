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
use ganbaru_sync_contracts::{DeviceKind, WriterId};
use ganbaru_sync_replica::client::{ClientSession, Exchange, PeerIds};
#[cfg(desktop)]
use ganbaru_sync_replica::peers::admission::MemberAdmission;
use ganbaru_sync_replica::peers::admission::fingerprint_hex;
use ganbaru_sync_replica::peers::book::{PeerBook, PeerEndpoints};
use ganbaru_sync_replica::peers::plan::{PlanInput, PlannedPeer, plan_pass};
use ganbaru_sync_replica::quarantine::PullSkips;
#[cfg(desktop)]
use ganbaru_sync_replica::quarantine::Quarantine;
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
    blocking(move || {
        update_peer_book(&path, |book| {
            book.observe_reported(&reported);
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
    Ok(Some(MemberAdmission::new(
        &vault_id,
        &view,
        quarantined.devices(),
    )))
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
    pub peers_path: PathBuf,
}

/// What the member exchanges of a pass moved.
#[derive(Debug, Default)]
pub(super) struct MemberPassOutcome {
    /// Operations pulled from members.
    pub pulled: usize,
}

/// Exchanges with the members the pass plan picks, at most the device kind's limit at once.
///
/// The coordinator exchange already ran with the pinned coordinator, so it is skipped here
/// while the coordinator is the anchor; after a handover the log's anchor is dialed through the
/// peer book, pinned to the certificate its enrollment recorded. Every member is pinned that
/// way, and only the coordinator path sends proposals, so a member's answer never settles one.
/// What a member refuses, such as an operation of a writer it does not know yet, is logged and
/// retried on the next pass instead of becoming this device's status.
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
    let Some(view) = view else {
        return Ok(MemberPassOutcome::default());
    };
    let members = live_members(&view, pass.quarantined);
    let path = pass.peers_path.clone();
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
    let AnchorDial {
        anchor_is_coordinator,
        endpoints: anchor_endpoints,
    } = anchor_dial(&view, &pin, &known);
    let plan = plan_pass(&PlanInput {
        kind,
        own_device_id: &pass.ids.device_id,
        members: &members,
        anchor: Some((&view.anchor_device_id, &anchor_endpoints)),
        anchor_reachable: !anchor_is_coordinator || coordinator_reachable,
        discovered: &BTreeMap::new(),
        known: &known,
    });
    let targets: Vec<(PlannedPeer, String)> = plan
        .peers
        .into_iter()
        .filter(|peer| !(anchor_is_coordinator && peer.device_id == pin.device_id))
        .filter(|peer| !peer.endpoints.is_empty())
        .filter_map(|peer| {
            let fingerprint = view
                .devices
                .iter()
                .find(|device| device.device_id == peer.device_id)
                .map(|device| fingerprint_hex(&device.tls_fingerprint))?;
            Some((peer, fingerprint))
        })
        .collect();
    let exchanges = targets.iter().map(|(peer, fingerprint)| {
        let pass = &pass;
        async move {
            let transport = MemberTransport {
                manager: pass.manager.clone(),
                endpoints: peer.endpoints.clone(),
                fingerprint: fingerprint.clone(),
            };
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
    for (device_id, result) in join_all(exchanges.collect()).await {
        match result {
            Ok(Exchange::Done(done)) => {
                outcome.pulled += done.pulled;
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
    if !reported.is_empty() {
        record_reported_peers(
            pass.peers_path,
            pass.engine,
            pass.pool,
            pass.ctx,
            pass.quarantined,
            reported,
        )
        .await?;
    }
    Ok(outcome)
}

/// How a pass reaches the anchor the selected log names.
#[derive(Debug, PartialEq, Eq)]
struct AnchorDial {
    /// Whether the pinned coordinator is the anchor, so the coordinator exchange covers it.
    anchor_is_coordinator: bool,
    /// The endpoints to dial the anchor at.
    endpoints: Vec<String>,
}

/// Reaches the log's anchor through the coordinator pin while it is the anchor, and through
/// the peer book after a handover. The pin is only read, so a handover never changes it.
fn anchor_dial(
    view: &AuthorizationView,
    pin: &ganbaru_handoff::pairing::CoordinatorPin,
    known: &PeerEndpoints,
) -> AnchorDial {
    let anchor_is_coordinator = view.anchor_device_id == pin.device_id;
    AnchorDial {
        anchor_is_coordinator,
        endpoints: if anchor_is_coordinator {
            pin.reported_endpoints.clone()
        } else {
            known.endpoints(&view.anchor_device_id)
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
    use ganbaru_sync::AuthorizationView;
    use ganbaru_sync_contracts::AdminPublicKey;
    use ganbaru_sync_replica::peers::book::PeerEndpoints;

    fn view(anchor: &str) -> AuthorizationView {
        AuthorizationView {
            epoch: 2,
            revision: 9,
            entry_hash: [3; 32],
            anchor_device_id: anchor.to_string(),
            frozen_at: None,
            catch_up: None,
            charter: [4; 32],
            recovery_admin_key: AdminPublicKey::from_bytes([5; 32]),
            devices: Vec::new(),
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
            discovery_key: None,
            charter: None,
        }
    }

    #[test]
    fn the_pinned_coordinator_is_dialed_while_it_is_the_anchor() {
        assert_eq!(
            anchor_dial(
                &view("device-old-anchor"),
                &pin(),
                &PeerEndpoints::default()
            ),
            AnchorDial {
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
            anchor_dial(&view("device-new-anchor"), &pin, &known),
            AnchorDial {
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
        let unknown = anchor_dial(&view("device-new-anchor"), &pin, &PeerEndpoints::default());
        assert!(unknown.endpoints.is_empty());
    }
}
