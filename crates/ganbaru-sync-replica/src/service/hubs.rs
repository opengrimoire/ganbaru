//! The hubs this device serves: the coordinator hub, the member hub, and the always-on
//! server's hub, with the proposal and forward handoffs to the pass and the admission sources
//! their listeners read from the selected log.

#[cfg(desktop)]
use super::DELEGATED_PASS_TIMEOUT;
use super::host::SyncHost;
use super::{SyncRuntime, blocking, peers};
#[cfg(desktop)]
use crate::hub::{HubContact, HubContactSource, HubForwards, HubProposals};
use crate::hub::{HubVault, OpenHubVault, SyncHub};
use crate::quarantine::Quarantine;
use crate::serve::MemberAdmissionSource;
#[cfg(desktop)]
use crate::serve::coordinator::QuarantinedDevices;
#[cfg(desktop)]
use std::collections::BTreeSet;
use std::sync::Arc;
#[cfg(desktop)]
use std::sync::Mutex;

/// The hub the coordinator listener routes sync requests to, serving the active vault.
#[cfg(desktop)]
pub fn sync_hub<H: SyncHost>(host: &H, runtime: &SyncRuntime) -> SyncHub {
    let opener = host.clone();
    let open: OpenHubVault = Arc::new(move || {
        let host = opener.clone();
        Box::pin(async move { open_hub_vault(&host).await })
    });
    SyncHub::new(open, runtime.changes.clone())
        .with_records(runtime.records.clone())
        .with_stability(runtime.stability.clone())
        .with_proposals(hub_proposals(runtime))
        .with_forwards(hub_forwards(runtime))
}

/// Hands an anchor's forward request to the pass, which answers it from this device's outbox.
#[cfg(desktop)]
fn hub_forwards(runtime: &SyncRuntime) -> HubForwards {
    let queue = runtime.clone();
    Arc::new(move |device_id, settled| {
        let runtime = queue.clone();
        Box::pin(async move {
            let receiver = runtime.request_forward(device_id, settled)?;
            tokio::time::timeout(DELEGATED_PASS_TIMEOUT, receiver)
                .await
                .map_err(|_| "the coordinator did not collect proposals in time".to_string())?
                .map_err(|_| "the coordinator stopped before it collected proposals".to_string())?
        })
    })
}

/// Hands a proposal a listener received to the pass, which sequences it when this desktop
/// anchors and answers that it is not the anchor otherwise.
#[cfg(desktop)]
fn hub_proposals(runtime: &SyncRuntime) -> HubProposals {
    let queue = runtime.clone();
    Arc::new(move |proposal| {
        let runtime = queue.clone();
        Box::pin(async move {
            let receiver = runtime.request_proposal(proposal)?;
            tokio::time::timeout(DELEGATED_PASS_TIMEOUT, receiver)
                .await
                .map_err(|_| "the anchor did not sequence the proposal in time".to_string())?
                .map_err(|_| "the anchor stopped before it sequenced the proposal".to_string())?
        })
    })
}

/// The hub a member listener routes admitted sync requests to, serving the active vault. Its
/// admission lets proposals through only while the selected log names this desktop as the
/// anchor, so a member reaches the anchor directly after a handover; it forwards none, since
/// the coordinator holds proposals for an anchor members cannot reach. It relays geneses of
/// other members, which the engine admits only for enrolled devices. A phone never anchors, so
/// its hub takes no proposals and reports no stability.
pub fn member_hub<H: SyncHost>(host: &H, runtime: &SyncRuntime) -> SyncHub {
    let opener = host.clone();
    let open: OpenHubVault = Arc::new(move || {
        let host = opener.clone();
        Box::pin(async move { open_hub_vault(&host).await })
    });
    with_anchor_services(
        SyncHub::new(open, runtime.changes.clone())
            .with_relayed_geneses()
            .with_records(runtime.records.clone()),
        runtime,
    )
}

/// The hub an always-on server runs: a member hub that also answers an anchor's forward request,
/// because a server dials nobody and its own proposals leave only that way. `device_id` is the
/// server's own device, which its contact leaves out.
#[cfg(desktop)]
pub fn server_hub<H: SyncHost>(host: &H, runtime: &SyncRuntime, device_id: String) -> SyncHub {
    member_hub(host, runtime)
        .with_forwards(hub_forwards(runtime))
        .with_contact(server_contact(host, device_id))
}

/// The contact a server reports: the members its peer book knows and no endpoint of its own,
/// since devices reach a server at the address the selected log names.
#[cfg(desktop)]
fn server_contact<H: SyncHost>(host: &H, device_id: String) -> HubContactSource {
    let host = host.clone();
    Arc::new(move |vault_id: &str| {
        let book = peers::read_peer_book(&host, vault_id);
        Ok(HubContact {
            members: peers::reported_members(&device_id, Vec::new(), &book),
        })
    })
}

/// Adds the stability report and proposal sequencing a desktop member serves once it anchors.
#[cfg(desktop)]
fn with_anchor_services(hub: SyncHub, runtime: &SyncRuntime) -> SyncHub {
    hub.with_stability(runtime.stability.clone())
        .with_proposals(hub_proposals(runtime))
}

/// A phone never anchors, so its member hub serves no anchor services.
#[cfg(mobile)]
fn with_anchor_services(hub: SyncHub, _runtime: &SyncRuntime) -> SyncHub {
    hub
}

/// Admission of the active vault's member listener, read from the selected log and the
/// quarantine for every connection.
pub fn member_admission_source<H: SyncHost>(host: &H) -> MemberAdmissionSource {
    let host = host.clone();
    Arc::new(move || {
        let host = host.clone();
        Box::pin(async move { peers::member_admission(&host).await })
    })
}

/// Admission of the coordinator listener, read from the selected log for every connection. The
/// coordinator applies the quarantine on its own, see [`quarantined_devices`].
#[cfg(desktop)]
pub fn log_admission_source<H: SyncHost>(host: &H) -> MemberAdmissionSource {
    let host = host.clone();
    Arc::new(move || {
        let host = host.clone();
        Box::pin(async move { peers::log_admission(&host).await })
    })
}

/// The devices this installation quarantined in the active vault, which the coordinator
/// listener refuses.
#[cfg(desktop)]
pub fn quarantined_devices<H: SyncHost>(host: &H) -> QuarantinedDevices {
    let host = host.clone();
    // The last list read for each vault, which stands in when the file cannot be read.
    let last_known: Arc<Mutex<std::collections::BTreeMap<String, BTreeSet<String>>>> =
        Arc::default();
    Arc::new(move || {
        let host = host.clone();
        let last_known = last_known.clone();
        Box::pin(async move {
            let vault_id = host.active_vault_id()?;
            let path = host.sync_files(&vault_id)?.quarantine_path();
            let loaded = blocking(move || Quarantine::load(&path)).await;
            let mut last_known = last_known
                .lock()
                .map_err(|_| "sync quarantine cache is unavailable".to_string())?;
            match loaded {
                Ok(quarantine) => {
                    last_known.insert(vault_id, quarantine.devices().clone());
                    Ok(quarantine.devices().clone())
                }
                Err(error) => {
                    eprintln!("sync: read the quarantine list, keeping the last one read: {error}");
                    Ok(last_known.get(&vault_id).cloned().unwrap_or_default())
                }
            }
        })
    })
}

async fn open_hub_vault<H: SyncHost>(host: &H) -> Result<Option<HubVault>, String> {
    if !host.sync_enabled()? {
        return Ok(None);
    }
    let pool = host.connect_for_sync().await?;
    let vault_id = host.active_vault_id()?;
    let assets = host.asset_store(&vault_id)?;
    let path = host.sync_files(&vault_id)?.quarantine_path();
    let quarantined = blocking(move || Quarantine::load(&path))
        .await?
        .devices()
        .clone();
    Ok(Some(HubVault {
        pool,
        vault_id,
        assets,
        quarantined,
        device_id: host.device_id()?,
    }))
}
