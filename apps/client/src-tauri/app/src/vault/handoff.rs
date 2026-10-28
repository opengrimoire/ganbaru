//! Secure, LAN-only pairing and whole-vault bundle transport.

#[cfg(desktop)]
pub(crate) mod contact_requests;
#[cfg(desktop)]
pub(crate) mod coordinator;
#[cfg(desktop)]
pub(crate) mod discovery;
#[cfg(desktop)]
pub(crate) mod enrollment_approval;
#[cfg(desktop)]
mod key_store;
#[cfg(desktop)]
mod local_addresses;
pub(crate) mod receiver;
pub(crate) mod source;
pub(crate) mod transport;

#[cfg(target_os = "linux")]
use ganbaru_handoff::network_access;
pub(crate) use ganbaru_handoff::{pairing, protocol};

use pairing::PairingManager;
#[cfg(desktop)]
use protocol::{COORDINATOR_PORT, DEVELOPMENT_COORDINATOR_PORT, QrMatrix, invitation_qr_matrix};
use protocol::{HandoffCompatibility, decode_invitation, encode_invitation};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(desktop)]
use std::net::SocketAddr;
#[cfg(desktop)]
use std::sync::Mutex;
use tauri::{Manager, Runtime};

pub(crate) use protocol::sha256_file;

pub(crate) fn current_compatibility<R: Runtime>(app: &tauri::AppHandle<R>) -> HandoffCompatibility {
    let schema = Sha256::digest(ganbaru_db::migration_set_identity_material());
    HandoffCompatibility {
        app_version: app.package_info().version.to_string(),
        database_schema_sha256: format!("{schema:x}"),
    }
}

#[derive(Default)]
#[cfg(desktop)]
pub(crate) struct CoordinatorLifecycle {
    runtime: Mutex<Option<CoordinatorRuntime>>,
}

#[cfg(desktop)]
struct CoordinatorRuntime {
    port: u16,
    shutdown: tokio::sync::oneshot::Sender<()>,
    requests: coordinator::CoordinatorSender,
    /// Advertises this coordinator to linked devices until the runtime is dropped.
    _advertiser: discovery::Advertiser,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(desktop)]
pub(crate) struct PairingInvitationView {
    invitation: String,
    qr: QrMatrix,
    endpoint: String,
    expires_at_ms: i64,
    #[cfg(target_os = "linux")]
    network_access: network_access::NetworkAccessStatus,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LinkedDeviceView {
    device_id: String,
    label: Option<String>,
    is_owner: bool,
    is_coordinator: bool,
    kind: protocol::DeviceKind,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PairingStatus {
    device_id: String,
    linked: bool,
    devices: Vec<LinkedDeviceView>,
    peer_device_id: Option<String>,
    peer_label: Option<String>,
    coordinator_endpoint: Option<String>,
    vault_id: Option<String>,
    can_write: Option<bool>,
    recovery_required: bool,
    revoked_by_coordinator: bool,
    replica_ready: bool,
    pending_transfer: bool,
    can_invite: bool,
    untouched_starter_vault: bool,
    /// A device waiting for this coordinator to approve its enrollment.
    #[cfg(desktop)]
    pending_enrollment: Option<enrollment_approval::PendingEnrollmentView>,
    #[cfg(target_os = "linux")]
    network_access: network_access::NetworkAccessStatus,
}

#[cfg(desktop)]
impl CoordinatorLifecycle {
    /// Starts the coordinator on every local address at `port` unless it already runs, and
    /// returns the port it listens on.
    async fn start_on<R: Runtime>(
        &self,
        app: tauri::AppHandle<R>,
        manager: PairingManager,
        port: u16,
    ) -> Result<u16, String> {
        if let Some(port) = self.port()? {
            return Ok(port);
        }
        // The member listener holds the same port while this desktop was a linked member. Its
        // lock stays held until the coordinator records its port, so a pass cannot start the
        // member listener again in between.
        let member_app = app.clone();
        let member = member_app.state::<MemberListenerLifecycle>();
        let mut member_runtime = member.runtime.lock().await;
        member.stop_locked(&mut member_runtime).await;
        let listener = transport::CoordinatorListener::bind(port)
            .map_err(|error| format!("bind vault handoff coordinator: {error}"))?;
        let port = listener
            .port()
            .map_err(|error| format!("read vault handoff port: {error}"))?;
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        let (requests, request_receiver) = tokio::sync::mpsc::channel(8);
        let (contacts, contact_receiver) = contact_requests::channel();
        let services = transport::CoordinatorServices {
            coordinator: Some(requests.clone()),
            contacts: Some(contacts),
            hub: Some(
                crate::sync::sync_hub(&app)
                    .with_contact(hub_contact_source(app.clone(), manager.clone(), port))
                    .with_peer_hints(crate::sync::peer_hints(app.clone())),
            ),
            enrollment_sealer: Some(crate::sync::enrollment_sealer(&app)),
            approvals: app
                .state::<enrollment_approval::EnrollmentApprovals>()
                .inner()
                .clone(),
            admission: lan_admission(&app, port)?,
            quarantined: Some(crate::sync::quarantined_devices(&app)),
        };
        tauri::async_runtime::spawn(contact_requests::run(app.clone(), contact_receiver));
        let advertiser_app = app.clone();
        tauri::async_runtime::spawn(coordinator::run(app, manager.clone(), request_receiver));
        let advertiser_manager = manager.clone();
        tauri::async_runtime::spawn(transport::serve_with_coordinator(
            listener, manager, services, receiver,
        ));
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "coordinator lifecycle lock is unavailable".to_string())?;
        if runtime.is_some() {
            let _ = shutdown.send(());
            return Ok(runtime.as_ref().expect("checked runtime").port);
        }
        *runtime = Some(CoordinatorRuntime {
            port,
            shutdown,
            requests,
            _advertiser: discovery::Advertiser::spawn(
                advertiser_app,
                advertiser_manager,
                port,
                discovery::AdvertiserRole::Coordinator,
            ),
        });
        drop(runtime);
        drop(member_runtime);
        Ok(port)
    }

    pub(crate) fn stop(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            if let Some(runtime) = runtime.take() {
                let (response, _) = tokio::sync::oneshot::channel();
                let _ = runtime.requests.try_send(coordinator::CoordinatorRequest {
                    operation: coordinator::CoordinatorOperation::Shutdown,
                    response,
                });
                let _ = runtime.shutdown.send(());
            }
        }
    }

    /// The port the running coordinator listens on, or `None` when it is stopped.
    pub(crate) fn port(&self) -> Result<Option<u16>, String> {
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| "coordinator lifecycle lock is unavailable".to_string())?;
        Ok(runtime.as_ref().map(|runtime| runtime.port))
    }

    /// The endpoint other devices should dial while the coordinator runs: its port on the
    /// preferred local address. `None` when it is stopped or no reportable address exists.
    pub(crate) fn reportable_endpoint(&self) -> Result<Option<SocketAddr>, String> {
        let Some(port) = self.port()? else {
            return Ok(None);
        };
        Ok(local_addresses::preferred_invitation_address()
            .ok()
            .map(|address| SocketAddr::new(address, port)))
    }

    pub(crate) fn request_sender(&self) -> Result<coordinator::CoordinatorSender, String> {
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| "coordinator lifecycle lock is unavailable".to_string())?;
        runtime
            .as_ref()
            .map(|runtime| runtime.requests.clone())
            .ok_or_else(|| "vault handoff coordinator is not running".to_string())
    }
}

/// Loads pairing state and reconciles it with durable ownership. A failure disables linking for
/// this run instead of aborting startup: the link probe stays unset, so a replica keeps its
/// durable read-only authority and replicated writes stay off, and pairing commands report why.
pub(crate) fn initialize<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Err(error) = initialize_linking(app) else {
        return;
    };
    eprintln!("vault linking is disabled: {error}");
    let _ = app
        .state::<super::ownership::VaultOwnershipManager>()
        .set_link_probe(None);
    if let Err(disable_error) = app.state::<PairingManager>().disable(error) {
        eprintln!("disable vault linking: {disable_error}");
    }
}

fn initialize_linking<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    let device_id = super::ensure_device_id(app)?;
    let pairing = app.state::<PairingManager>();
    #[cfg(desktop)]
    pairing.set_identity_key_store(std::sync::Arc::new(key_store::KeyringIdentityKeyStore))?;
    // On mobile the managed manager is the process one, which a headless sync pass may have
    // loaded first.
    #[cfg(mobile)]
    super::process_managers::process_managers().load_pairing(&config_dir, device_id)?;
    #[cfg(desktop)]
    pairing.initialize(config_dir, device_id)?;
    if let Some(coordinator) = pairing.coordinator_pin()? {
        app.state::<super::ownership::VaultOwnershipManager>()
            .register_remote_owner_if_missing(
                &coordinator.vault_id,
                coordinator.device_id,
                coordinator.generation,
            )?;
    }
    restore_outgoing_ownership(app, &pairing)?;
    let probe_pairing = pairing.inner().clone();
    app.state::<super::ownership::VaultOwnershipManager>()
        .set_link_probe(Some(std::sync::Arc::new(move |vault_id: &str| {
            probe_pairing.is_linked_for(vault_id).unwrap_or(false)
        })))?;
    Ok(())
}

fn restore_outgoing_ownership<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pairing: &PairingManager,
) -> Result<(), String> {
    let Some(transfer) = pairing.outgoing_transfer()? else {
        return Ok(());
    };
    let (_, archive_path) = pairing.outgoing_snapshot_paths(&transfer.metadata.transfer_id)?;
    if let Err(error) = protocol::validate_staging_file(&archive_path, &transfer.metadata) {
        if transfer.committed {
            return Err(format!(
                "committed handoff snapshot is unavailable: {error}"
            ));
        }
        pairing.clear_outgoing_transfer(&transfer.metadata.transfer_id)?;
        let _ = std::fs::remove_file(archive_path);
        return Ok(());
    }
    if transfer.purpose != protocol::BundlePurpose::Ownership {
        return Ok(());
    }
    let previous_generation = transfer
        .metadata
        .generation
        .checked_sub(1)
        .ok_or_else(|| "persisted ownership transfer generation is invalid".to_string())?;
    let ownership = app.state::<super::ownership::VaultOwnershipManager>();
    let status = ownership.status(&transfer.metadata.vault_id)?;
    if transfer.committed {
        return if status.transfer_phase
            == (super::ownership::TransferPhase::OutgoingCommitted {
                transfer_id: transfer.metadata.transfer_id,
                receiver_device_id: transfer.metadata.device_id,
                committed_generation: transfer.metadata.generation,
            }) {
            Ok(())
        } else {
            Err("committed handoff conflicts with durable ownership state".to_string())
        };
    }
    if status.transfer_phase
        == (super::ownership::TransferPhase::PreparingOutgoing {
            transfer_id: transfer.metadata.transfer_id.clone(),
            receiver_device_id: transfer.metadata.device_id.clone(),
            next_generation: transfer.metadata.generation,
        })
    {
        return Ok(());
    }
    if status.transfer_phase
        == (super::ownership::TransferPhase::OutgoingCommitted {
            transfer_id: transfer.metadata.transfer_id.clone(),
            receiver_device_id: transfer.metadata.device_id.clone(),
            committed_generation: transfer.metadata.generation,
        })
    {
        return pairing.mark_outgoing_committed(&transfer.metadata.transfer_id);
    }
    ownership.begin_outgoing(
        &transfer.metadata.vault_id,
        previous_generation,
        transfer.metadata.transfer_id,
        transfer.metadata.device_id,
    )
}

#[cfg(desktop)]
pub(crate) fn start_desktop(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<PairingManager>().coordinator_pin()?.is_some() {
        receiver::start_reconnect_refresh(app.clone());
        return Ok(());
    }
    tauri::async_runtime::block_on(start_desktop_when_reachable(app))
}

/// Whether the desktop coordinator listens before the user asks for it. Linked devices, a
/// transfer in flight, and contact cards handed out for this person are the only callers that
/// can expect it; otherwise it starts when linking or Contacts first need it.
#[cfg(desktop)]
fn coordinator_needed_at_launch(
    linked_peers: bool,
    pending_transfer: bool,
    person_identity: bool,
) -> bool {
    linked_peers || pending_transfer || person_identity
}

/// Starts the coordinator when linked devices, a transfer, or a contact card may already be
/// trying to reach it. Runs at launch and after another vault becomes active.
#[cfg(desktop)]
pub(crate) async fn start_desktop_when_reachable<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let manager = app.state::<PairingManager>().inner().clone();
    if manager.coordinator_pin()?.is_some() {
        return Ok(());
    }
    let needed = coordinator_needed_at_launch(
        !manager.linked_peers()?.is_empty(),
        manager.has_pending_transfer()?,
        crate::contacts::person_identity_exists(app).await,
    );
    if needed {
        ensure_desktop_coordinator(app).await?;
    }
    Ok(())
}

/// Starts the coordinator on every local address unless it already runs, and returns the port it
/// listens on. A device linked to another coordinator never coordinates, so it gets `None`.
#[cfg(desktop)]
pub(crate) async fn ensure_desktop_coordinator<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<u16>, String> {
    let manager = app.state::<PairingManager>().inner().clone();
    if manager.coordinator_pin()?.is_some() {
        return Ok(None);
    }
    let port = coordinator_port(&app.config().identifier);
    app.state::<CoordinatorLifecycle>()
        .start_on(app.clone(), manager, port)
        .await
        .map(Some)
}

/// The sync listener of a member desktop: a linked device that does not coordinate the vault
/// serves sync requests from the other members on the coordinator port, which it never binds
/// for anything else.
#[derive(Default)]
#[cfg(desktop)]
pub(crate) struct MemberListenerLifecycle {
    runtime: tokio::sync::Mutex<Option<MemberListenerRuntime>>,
    /// The port the listener holds, or 0 while it is stopped, readable without waiting for a
    /// start or stop in progress.
    port: std::sync::atomic::AtomicU16,
}

#[cfg(desktop)]
struct MemberListenerRuntime {
    vault_id: String,
    port: u16,
    shutdown: tokio::sync::oneshot::Sender<()>,
    served: tauri::async_runtime::JoinHandle<()>,
    /// Advertises the listener over DNS-SD until the runtime is dropped.
    _advertiser: discovery::Advertiser,
}

#[cfg(desktop)]
impl MemberListenerLifecycle {
    /// Stops the listener and waits until it released its port.
    pub(crate) async fn stop(&self) {
        self.stop_locked(&mut *self.runtime.lock().await).await;
    }

    /// Stops the listener `runtime` holds and waits until it released its port.
    async fn stop_locked(&self, runtime: &mut Option<MemberListenerRuntime>) {
        self.port.store(0, std::sync::atomic::Ordering::Relaxed);
        if let Some(runtime) = runtime.take() {
            let _ = runtime.shutdown.send(());
            if let Err(error) = runtime.served.await {
                eprintln!("sync member listener: stop: {error}");
            }
        }
    }

    /// Asks the listener to stop without waiting, for app exit.
    pub(crate) fn stop_now(&self) {
        if let Ok(mut runtime) = self.runtime.try_lock()
            && let Some(runtime) = runtime.take()
        {
            let _ = runtime.shutdown.send(());
        }
        self.port.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// The port the listener holds, when it runs.
    #[cfg(target_os = "linux")]
    pub(crate) fn port(&self) -> Option<u16> {
        match self.port.load(std::sync::atomic::Ordering::Relaxed) {
            0 => None,
            port => Some(port),
        }
    }
}

/// Keeps the member listener of the pinned vault running while this desktop is a linked member,
/// and returns the endpoints it is reachable at. A desktop without a coordinator pin stops it,
/// since it either coordinates the vault itself or is not linked.
#[cfg(desktop)]
pub(crate) async fn ensure_member_listener<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<String>, String> {
    let lifecycle = app.state::<MemberListenerLifecycle>();
    let manager = app.state::<PairingManager>().inner().clone();
    // The coordinator takes the same lock while it binds the port, so this check holds until
    // the listener below is recorded.
    let mut runtime = lifecycle.runtime.lock().await;
    let Some(pin) = manager.coordinator_pin()? else {
        lifecycle.stop_locked(&mut runtime).await;
        return Ok(Vec::new());
    };
    if app.state::<CoordinatorLifecycle>().port()?.is_some() {
        lifecycle.stop_locked(&mut runtime).await;
        return Ok(Vec::new());
    }
    if let Some(running) = runtime.as_ref()
        && running.vault_id == pin.vault_id
        && !running.served.inner().is_finished()
    {
        return Ok(local_addresses::reported_endpoints(running.port));
    }
    if let Some(stale) = runtime.take() {
        let _ = stale.shutdown.send(());
        let _ = stale.served.await;
    }
    let port = coordinator_port(&app.config().identifier);
    let listener = transport::CoordinatorListener::bind(port)
        .map_err(|error| format!("bind member sync listener: {error}"))?;
    let port = listener
        .port()
        .map_err(|error| format!("read member sync listener port: {error}"))?;
    let services = transport::MemberServices {
        hub: crate::sync::member_hub(app)
            .with_contact(hub_contact_source(app.clone(), manager.clone(), port))
            .with_peer_hints(crate::sync::peer_hints(app.clone())),
        admission: crate::sync::member_admission_source(app),
        lan: lan_admission(app, port)?,
    };
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let advertiser = discovery::Advertiser::spawn(
        app.clone(),
        manager.clone(),
        port,
        discovery::AdvertiserRole::Member,
    );
    let served = tauri::async_runtime::spawn(transport::serve_member_listener(
        listener, manager, services, receiver,
    ));
    *runtime = Some(MemberListenerRuntime {
        vault_id: pin.vault_id,
        port,
        shutdown,
        served,
        _advertiser: advertiser,
    });
    lifecycle
        .port
        .store(port, std::sync::atomic::Ordering::Relaxed);
    Ok(local_addresses::reported_endpoints(port))
}

#[cfg(desktop)]
#[tauri::command]
pub(crate) async fn handoff_create_pairing_invitation<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<PairingInvitationView, String> {
    let manager = app.state::<PairingManager>().inner().clone();
    let Some(port) = ensure_desktop_coordinator(&app).await? else {
        return Err("a linked client device cannot coordinate additional devices".to_string());
    };
    let endpoint = SocketAddr::new(local_addresses::preferred_invitation_address()?, port);
    let ownership = super::ownership::active_status(&app)?;
    // A device waiting with the replaced invitation can no longer be approved.
    app.state::<enrollment_approval::EnrollmentApprovals>()
        .cancel();
    let invitation = manager.create_invitation(
        endpoint,
        ownership.vault_id,
        ownership.generation,
        current_compatibility(&app),
        protocol::unix_time_ms(),
    )?;
    let encoded = encode_invitation(&invitation)?;
    Ok(PairingInvitationView {
        qr: invitation_qr_matrix(&invitation)?,
        invitation: encoded,
        endpoint: endpoint.to_string(),
        expires_at_ms: invitation.expires_at_ms,
        #[cfg(target_os = "linux")]
        network_access: network_access_status_for_port(&app, port)?,
    })
}

/// Admission for a coordinator listening on `port`: on Linux, LAN peers are served only on
/// networks where the user confirmed the firewall rules for the address they reached.
#[cfg(target_os = "linux")]
fn lan_admission<R: Runtime>(
    app: &tauri::AppHandle<R>,
    port: u16,
) -> Result<transport::LanAdmission, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    Ok(transport::LanAdmission::linux_firewall(config_dir, port))
}

/// What the sync hub of a desktop listening on `port` reports to linked devices: its current
/// listener endpoints under its own device id, refreshed at most every few seconds, then the
/// peer book's other members.
#[cfg(desktop)]
fn hub_contact_source<R: Runtime>(
    app: tauri::AppHandle<R>,
    manager: PairingManager,
    port: u16,
) -> crate::sync::hub::HubContactSource {
    let endpoints = std::sync::Arc::new(local_addresses::ReportedEndpointCache::default());
    std::sync::Arc::new(move |vault_id: &str| {
        let device_id = manager.device_id()?;
        let observed_ms = u64::try_from(protocol::unix_time_ms()).unwrap_or_default();
        let own = endpoints
            .get_or_refresh(std::time::Instant::now(), || {
                local_addresses::reported_endpoints(port)
            })
            .into_iter()
            .map(|endpoint| protocol::MemberEndpoint {
                device_id: device_id.clone(),
                endpoint,
                observed_ms,
            })
            .collect();
        let book = crate::sync::read_peer_book(&app, vault_id);
        Ok(crate::sync::hub::HubContact {
            members: crate::sync::reported_members(&device_id, own, &book),
        })
    })
}

/// Admission for a coordinator listening on `port`. Only Linux keeps per-network firewall rules,
/// so other desktops serve every LAN peer.
#[cfg(all(desktop, not(target_os = "linux")))]
fn lan_admission<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    _port: u16,
) -> Result<transport::LanAdmission, String> {
    Ok(transport::LanAdmission::default())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub(crate) async fn handoff_grant_network_access<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<network_access::NetworkAccessStatus, String> {
    let port = listening_port(&app)?
        .ok_or_else(|| "neither the coordinator nor the member listener is running".to_string())?;
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    let status = tauri::async_runtime::spawn_blocking(move || {
        network_access::grant(
            &config_dir,
            &local_addresses::reported_interface_addresses(),
            port,
        )
    })
    .await
    .map_err(|error| format!("authorize Linux network access: {error}"))?;
    crate::sync::firewall_changed(&app);
    status
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub(crate) async fn handoff_revoke_network_access<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<network_access::NetworkAccessStatus, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    let status = tauri::async_runtime::spawn_blocking(move || network_access::revoke(&config_dir))
        .await
        .map_err(|error| format!("revoke Linux network access: {error}"))?;
    crate::sync::firewall_changed(&app);
    status
}

#[tauri::command]
pub(crate) fn handoff_decode_pairing_qr(
    width: usize,
    height: usize,
    luma: Vec<u8>,
) -> Result<String, String> {
    let invitation =
        protocol::decode_pairing_qr_luma(width, height, &luma, protocol::unix_time_ms())?;
    encode_invitation(&invitation)
}

#[tauri::command]
pub(crate) async fn handoff_enroll(
    app: tauri::AppHandle,
    invitation: String,
    device_label: String,
) -> Result<(), String> {
    let invitation = decode_invitation(&invitation, protocol::unix_time_ms())?;
    protocol::ensure_compatible(&current_compatibility(&app), &invitation.compatibility)?;
    let manager = app.state::<PairingManager>().inner().clone();
    manager.ensure_enrollment_target(&invitation)?;
    let device_kind = if cfg!(mobile) {
        protocol::DeviceKind::Phone
    } else {
        protocol::DeviceKind::Computer
    };
    let sync_device_key = crate::sync::ensure_device_key(&app, &invitation.vault_id).await?;
    #[cfg(desktop)]
    let sync_admin_key = Some(crate::sync::ensure_admin_key(&app, &invitation.vault_id).await?);
    #[cfg(mobile)]
    let sync_admin_key: Option<ganbaru_sync_contracts::AdminPublicKey> = None;
    transport::enroll(
        &manager,
        &invitation,
        device_label,
        device_kind,
        &sync_device_key,
        sync_admin_key.as_ref(),
    )
    .await?;
    app.state::<super::ownership::VaultOwnershipManager>()
        .register_remote_owner(
            &invitation.vault_id,
            invitation.coordinator_device_id,
            invitation.generation,
        )?;
    #[cfg(desktop)]
    {
        app.state::<CoordinatorLifecycle>().stop();
        receiver::start_reconnect_refresh(app.clone());
    }
    Ok(())
}

/// Code this device shows while it enrolls with the invitation. The coordinator shows the same
/// code only for a request signed with this device's certificate.
#[tauri::command]
pub(crate) fn handoff_enrollment_code(
    app: tauri::AppHandle,
    invitation: String,
) -> Result<String, String> {
    let invitation = decode_invitation(&invitation, protocol::unix_time_ms())?;
    let certificate = app.state::<PairingManager>().certificate()?;
    Ok(pairing::enrollment_code(&invitation, certificate.as_ref()))
}

/// Answers the enrollment waiting on this coordinator.
#[cfg(desktop)]
#[tauri::command]
pub(crate) fn handoff_respond_enrollment(
    app: tauri::AppHandle,
    request_id: String,
    approve: bool,
) -> Result<(), String> {
    let decision = if approve {
        enrollment_approval::EnrollmentDecision::Approved
    } else {
        enrollment_approval::EnrollmentDecision::Declined
    };
    app.state::<enrollment_approval::EnrollmentApprovals>()
        .decide(&request_id, decision)
}

#[tauri::command]
pub(crate) fn handoff_suggested_device_label() -> String {
    suggested_device_label()
}

#[cfg(target_os = "android")]
fn suggested_device_label() -> String {
    std::process::Command::new("getprop")
        .arg("ro.product.model")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|label| normalized_device_label(&label))
        .unwrap_or_else(|| "Phone".to_string())
}

#[cfg(desktop)]
fn suggested_device_label() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .and_then(|label| normalized_device_label(&label))
        .or_else(|| {
            std::env::var("COMPUTERNAME")
                .ok()
                .and_then(|label| normalized_device_label(&label))
        })
        .unwrap_or_else(|| "Computer".to_string())
}

#[cfg(target_os = "ios")]
fn suggested_device_label() -> String {
    "Phone".to_string()
}

fn normalized_device_label(label: &str) -> Option<String> {
    let label = label.trim();
    protocol::validate_device_label("device label", label)
        .is_ok()
        .then(|| label.to_string())
}

#[tauri::command]
pub(crate) fn handoff_pairing_status<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<PairingStatus, String> {
    let manager = app.state::<PairingManager>();
    let device_id = manager.device_id()?;
    let coordinator = manager.coordinator_pin()?;
    let vault_id = status_vault_id(coordinator.as_ref(), || super::active_vault_id(&app).ok());
    let peers = match vault_id.as_deref() {
        Some(vault_id) => manager.linked_peers_for(vault_id)?,
        None => Vec::new(),
    };
    let ownership = vault_id
        .as_deref()
        .map(|vault_id| {
            app.state::<super::ownership::VaultOwnershipManager>()
                .status(vault_id)
        })
        .transpose()?;
    let peer = ownership
        .as_ref()
        .and_then(|ownership| {
            peers
                .iter()
                .find(|peer| peer.device_id == ownership.owner_device_id)
        })
        .or_else(|| peers.first());
    let mut devices = peers
        .iter()
        .map(|peer| LinkedDeviceView {
            device_id: peer.device_id.clone(),
            label: Some(peer.device_label.clone()),
            is_owner: ownership
                .as_ref()
                .is_some_and(|ownership| ownership.owner_device_id == peer.device_id),
            is_coordinator: false,
            kind: peer.device_kind,
        })
        .collect::<Vec<_>>();
    if let Some(coordinator) = coordinator.as_ref() {
        if !devices
            .iter()
            .any(|device| device.device_id == coordinator.device_id)
        {
            devices.push(LinkedDeviceView {
                device_id: coordinator.device_id.clone(),
                label: coordinator.device_label.clone(),
                is_owner: ownership
                    .as_ref()
                    .is_some_and(|ownership| ownership.owner_device_id == coordinator.device_id),
                is_coordinator: true,
                kind: protocol::DeviceKind::Computer,
            });
        }
    }
    #[cfg(target_os = "linux")]
    let network_access = desktop_network_access_status(&app)?;
    Ok(PairingStatus {
        device_id,
        linked: !peers.is_empty() || coordinator.is_some(),
        devices,
        peer_device_id: peer.map(|peer| peer.device_id.clone()).or_else(|| {
            coordinator
                .as_ref()
                .map(|coordinator| coordinator.device_id.clone())
        }),
        peer_label: peer.map(|peer| peer.device_label.clone()).or_else(|| {
            coordinator
                .as_ref()
                .and_then(|coordinator| coordinator.device_label.clone())
        }),
        coordinator_endpoint: coordinator
            .as_ref()
            .map(|coordinator| coordinator.current_endpoint().to_string()),
        vault_id,
        can_write: ownership.as_ref().map(|status| status.can_write),
        recovery_required: ownership
            .as_ref()
            .is_some_and(|status| status.role == "recovery"),
        revoked_by_coordinator: manager.revoked_by_coordinator()?,
        replica_ready: manager.replica_ready()? || !peers.is_empty(),
        pending_transfer: manager.has_pending_transfer()?,
        can_invite: cfg!(desktop) && coordinator.is_none(),
        untouched_starter_vault: super::untouched_starter_vault(&app)?,
        #[cfg(desktop)]
        pending_enrollment: app
            .state::<enrollment_approval::EnrollmentApprovals>()
            .pending()?,
        #[cfg(target_os = "linux")]
        network_access,
    })
}

/// Vault whose memberships the pairing status reports: the pinned coordinator's vault on a linked
/// client, otherwise the active vault. Devices linked to other vaults stay hidden and inactive.
fn status_vault_id(
    coordinator: Option<&pairing::CoordinatorPin>,
    active_vault_id: impl FnOnce() -> Option<String>,
) -> Option<String> {
    coordinator
        .map(|coordinator| coordinator.vault_id.clone())
        .or_else(active_vault_id)
}

#[cfg(target_os = "linux")]
pub(crate) fn desktop_network_access_status<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<network_access::NetworkAccessStatus, String> {
    match listening_port(app)? {
        Some(port) => network_access_status_for_port(app, port),
        None => Ok(network_access::not_required()),
    }
}

/// The port this desktop's sync listener holds: the coordinator's, otherwise the member
/// listener's. Both get the same scoped rules on the same port.
#[cfg(target_os = "linux")]
fn listening_port<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Option<u16>, String> {
    Ok(app
        .state::<CoordinatorLifecycle>()
        .port()?
        .or_else(|| app.state::<MemberListenerLifecycle>().port()))
}

/// Firewall state of the member listener, `None` while this desktop coordinates or the member
/// listener is not running.
#[cfg(target_os = "linux")]
pub(crate) fn member_network_access_state<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<network_access::NetworkAccessState>, String> {
    if app.state::<CoordinatorLifecycle>().port()?.is_some() {
        return Ok(None);
    }
    app.state::<MemberListenerLifecycle>()
        .port()
        .map(|port| network_access_status_for_port(app, port).map(|status| status.state()))
        .transpose()
}

/// Firewall status for peers dialing any endpoint this coordinator reports on `port`, over IPv4
/// and IPv6.
#[cfg(target_os = "linux")]
fn network_access_status_for_port<R: Runtime>(
    app: &tauri::AppHandle<R>,
    port: u16,
) -> Result<network_access::NetworkAccessStatus, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    Ok(network_access::status(
        &config_dir,
        &local_addresses::reported_interface_addresses(),
        port,
    ))
}

#[cfg(desktop)]
fn coordinator_port(identifier: &str) -> u16 {
    if identifier.ends_with(".dev") {
        DEVELOPMENT_COORDINATOR_PORT
    } else {
        COORDINATOR_PORT
    }
}

#[tauri::command]
pub(crate) fn handoff_unlink<R: Runtime>(
    app: tauri::AppHandle<R>,
    device_id: String,
) -> Result<(), String> {
    let manager = app.state::<PairingManager>();
    manager.ensure_can_unlink()?;
    let coordinator = manager.coordinator_pin()?;
    let vault_id = if let Some(peer) = manager.linked_peer(&device_id)? {
        peer.vault_id
    } else if let Some(coordinator) = coordinator
        .as_ref()
        .filter(|coordinator| coordinator.device_id == device_id)
    {
        coordinator.vault_id.clone()
    } else {
        return Err("linked device was not found".to_string());
    };
    let ownership = app
        .state::<super::ownership::VaultOwnershipManager>()
        .status(&vault_id)?;
    if !matches!(
        ownership.transfer_phase,
        super::ownership::TransferPhase::Stable
    ) {
        return Err("finish or retry the active handoff before unlinking".to_string());
    }
    if ownership.owner_device_id == device_id
        && ownership.device_id != device_id
        && manager.replica_ready()?
    {
        return Err("switch ownership away from this device before unlinking it".to_string());
    }
    if ownership.can_write
        && coordinator.is_some_and(|coordinator| coordinator.device_id == device_id)
    {
        return Err(
            "switch ownership to the coordinating computer before unlinking it".to_string(),
        );
    }
    manager.unlink_device(&device_id)?;
    // The next pass removes the device from the vault's sync space.
    app.state::<crate::sync::SyncRuntime>().sync_now();
    Ok(())
}

#[tauri::command]
pub(crate) fn handoff_recover_local_copy<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<u64, String> {
    let manager = app.state::<PairingManager>();
    manager.ensure_can_unlink()?;
    let vault_id = super::active_vault_id(&app)?;
    let ownership = app.state::<super::ownership::VaultOwnershipManager>();
    ownership.ensure_can_recover_local_copy(&vault_id)?;
    manager.unlink()?;
    ownership.recover_local_copy(&vault_id)
}

#[cfg(desktop)]
#[tauri::command]
pub(crate) async fn handoff_request_owner_bundle<R: Runtime>(
    app: tauri::AppHandle<R>,
    purpose: protocol::BundlePurpose,
) -> Result<(), String> {
    let response = coordinator::request(
        &app.state::<CoordinatorLifecycle>().request_sender()?,
        coordinator::CoordinatorOperation::RequestUpload { purpose },
    )
    .await?;
    match response {
        coordinator::CoordinatorResponse::UploadRequested { purpose: requested }
            if requested == purpose =>
        {
            Ok(())
        }
        _ => Err("coordinator returned an invalid upload request result".to_string()),
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn pairing_status_follows_the_pinned_coordinator_before_the_active_vault() {
        let coordinator = pairing::CoordinatorPin {
            device_id: "device-desktop".to_string(),
            device_label: None,
            endpoint: "192.168.10.4:43821".to_string(),
            vault_id: "vault-coordinated".to_string(),
            generation: 3,
            certificate_fingerprint: "fingerprint".to_string(),
            last_endpoint: None,
            reported_endpoints: Vec::new(),
            charter: None,
        };
        assert_eq!(
            status_vault_id(Some(&coordinator), || Some("vault-local".to_string())).as_deref(),
            Some("vault-coordinated")
        );
        assert_eq!(
            status_vault_id(None, || Some("vault-local".to_string())).as_deref(),
            Some("vault-local")
        );
        assert_eq!(status_vault_id(None, || None), None);
    }

    #[test]
    fn an_unlinked_desktop_without_a_person_identity_does_not_listen_at_launch() {
        assert!(!coordinator_needed_at_launch(false, false, false));
        assert!(coordinator_needed_at_launch(true, false, false));
        assert!(coordinator_needed_at_launch(false, true, false));
        assert!(coordinator_needed_at_launch(false, false, true));
    }

    #[test]
    fn development_and_production_coordinators_use_distinct_ports() {
        assert_eq!(coordinator_port("org.opengrimoire.ganbaru"), 43_821);
        assert_eq!(coordinator_port("org.opengrimoire.ganbaru.dev"), 43_822);
    }
}
