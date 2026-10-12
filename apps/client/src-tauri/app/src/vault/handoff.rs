//! Secure, LAN-only pairing and whole-vault bundle transport.

#[cfg(desktop)]
pub(crate) mod coordinator;
pub(crate) mod receiver;
pub(crate) mod source;
pub(crate) mod transport;

#[cfg(target_os = "linux")]
use ganbaru_handoff::network_access;
#[cfg(target_os = "linux")]
pub(crate) use ganbaru_handoff::network_access::run_privileged_helper_if_requested;
pub(crate) use ganbaru_handoff::{pairing, protocol};

use pairing::PairingManager;
#[cfg(desktop)]
use protocol::{COORDINATOR_PORT, DEVELOPMENT_COORDINATOR_PORT, QrMatrix, invitation_qr_matrix};
use protocol::{HandoffCompatibility, decode_invitation, encode_invitation};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(desktop)]
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
#[cfg(desktop)]
use std::sync::Mutex;
use tauri::{Manager, Runtime};
#[cfg(desktop)]
use tokio::net::TcpListener;

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
    endpoint: SocketAddr,
    shutdown: tokio::sync::oneshot::Sender<()>,
    requests: coordinator::CoordinatorSender,
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
    #[cfg(target_os = "linux")]
    network_access: network_access::NetworkAccessStatus,
}

#[cfg(desktop)]
impl CoordinatorLifecycle {
    async fn start_on<R: Runtime>(
        &self,
        app: tauri::AppHandle<R>,
        manager: PairingManager,
        address: IpAddr,
        port: u16,
    ) -> Result<SocketAddr, String> {
        {
            let runtime = self
                .runtime
                .lock()
                .map_err(|_| "coordinator lifecycle lock is unavailable".to_string())?;
            if let Some(runtime) = runtime.as_ref() {
                return Ok(runtime.endpoint);
            }
        }
        if !is_lan_address(address) {
            return Err("vault handoff coordinator requires a private LAN address".to_string());
        }
        let listener = TcpListener::bind(SocketAddr::new(address, port))
            .await
            .map_err(|error| format!("bind vault handoff coordinator: {error}"))?;
        let endpoint = listener
            .local_addr()
            .map_err(|error| format!("read vault handoff endpoint: {error}"))?;
        let (shutdown, receiver) = tokio::sync::oneshot::channel();
        let (requests, request_receiver) = tokio::sync::mpsc::channel(8);
        let hub = crate::sync::sync_hub(&app);
        tauri::async_runtime::spawn(coordinator::run(app, manager.clone(), request_receiver));
        tauri::async_runtime::spawn(transport::serve_with_coordinator(
            listener,
            manager,
            Some(requests.clone()),
            Some(hub),
            receiver,
        ));
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "coordinator lifecycle lock is unavailable".to_string())?;
        if runtime.is_some() {
            let _ = shutdown.send(());
            return Ok(runtime.as_ref().expect("checked runtime").endpoint);
        }
        *runtime = Some(CoordinatorRuntime {
            endpoint,
            shutdown,
            requests,
        });
        Ok(endpoint)
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

    pub(crate) fn endpoint(&self) -> Result<Option<SocketAddr>, String> {
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| "coordinator lifecycle lock is unavailable".to_string())?;
        Ok(runtime.as_ref().map(|runtime| runtime.endpoint))
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

pub(crate) fn initialize<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    let device_id = super::ensure_device_id(app)?;
    let pairing = app.state::<PairingManager>();
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
    let address = discover_private_lan_address()?;
    let port = coordinator_port(&app.config().identifier);
    let manager = app.state::<PairingManager>().inner().clone();
    let lifecycle = app.state::<CoordinatorLifecycle>();
    tauri::async_runtime::block_on(lifecycle.start_on(app.clone(), manager, address, port))?;
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
pub(crate) async fn handoff_create_pairing_invitation<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<PairingInvitationView, String> {
    let manager = app.state::<PairingManager>().inner().clone();
    if manager.coordinator_pin()?.is_some() {
        return Err("a linked client device cannot coordinate additional devices".to_string());
    }
    let lifecycle = app.state::<CoordinatorLifecycle>();
    let endpoint = match lifecycle.endpoint()? {
        Some(endpoint) => endpoint,
        None => {
            let address = discover_private_lan_address()?;
            let port = coordinator_port(&app.config().identifier);
            lifecycle
                .start_on(app.clone(), manager.clone(), address, port)
                .await?
        }
    };
    let ownership = super::ownership::active_status(&app)?;
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
        network_access: network_access::status(
            &app.path()
                .app_config_dir()
                .map_err(|error| format!("find app config directory: {error}"))?,
            match endpoint.ip() {
                IpAddr::V4(address) => address,
                IpAddr::V6(_) => {
                    return Err("Linux firewall access requires an IPv4 LAN address".to_string());
                }
            },
            endpoint.port(),
        ),
    })
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub(crate) async fn handoff_grant_network_access<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<network_access::NetworkAccessStatus, String> {
    let endpoint = app
        .state::<CoordinatorLifecycle>()
        .endpoint()?
        .ok_or_else(|| "vault handoff coordinator is not running".to_string())?;
    let IpAddr::V4(address) = endpoint.ip() else {
        return Err("Linux firewall access requires an IPv4 LAN address".to_string());
    };
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    let port = endpoint.port();
    tauri::async_runtime::spawn_blocking(move || network_access::grant(&config_dir, address, port))
        .await
        .map_err(|error| format!("authorize Linux network access: {error}"))?
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
    tauri::async_runtime::spawn_blocking(move || network_access::revoke(&config_dir))
        .await
        .map_err(|error| format!("revoke Linux network access: {error}"))?
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
    transport::enroll(&manager, &invitation, device_label, device_kind).await?;
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
    (!label.is_empty()
        && label.len() <= protocol::MAX_DEVICE_LABEL_BYTES
        && !label.chars().any(char::is_control))
    .then(|| label.to_string())
}

#[tauri::command]
pub(crate) fn handoff_pairing_status<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<PairingStatus, String> {
    let manager = app.state::<PairingManager>();
    let (device_id, _) = manager.identity()?;
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
            .map(|coordinator| coordinator.endpoint.clone()),
        vault_id,
        can_write: ownership.as_ref().map(|status| status.can_write),
        recovery_required: ownership
            .as_ref()
            .is_some_and(|status| status.role == "recovery"),
        revoked_by_coordinator: manager.revoked_by_coordinator()?,
        replica_ready: manager.replica_ready()? || !peers.is_empty(),
        pending_transfer: manager.has_pending_transfer()?,
        can_invite: cfg!(desktop) && coordinator.is_none(),
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
fn desktop_network_access_status<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<network_access::NetworkAccessStatus, String> {
    let Some(endpoint) = app.state::<CoordinatorLifecycle>().endpoint()? else {
        return Ok(network_access::not_required());
    };
    let IpAddr::V4(address) = endpoint.ip() else {
        return Err("Linux firewall access requires an IPv4 LAN address".to_string());
    };
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    Ok(network_access::status(
        &config_dir,
        address,
        endpoint.port(),
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
    manager.unlink_device(&device_id)
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

#[cfg(desktop)]
fn discover_private_lan_address() -> Result<IpAddr, String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .map_err(|error| format!("inspect LAN address: {error}"))?;
    socket
        .connect((Ipv4Addr::new(192, 0, 2, 1), 9))
        .map_err(|error| format!("select LAN route: {error}"))?;
    let address = socket
        .local_addr()
        .map_err(|error| format!("read LAN address: {error}"))?
        .ip();
    if !is_lan_address(address) || address.is_loopback() {
        return Err("no private LAN address is currently available".to_string());
    }
    Ok(address)
}

#[cfg(desktop)]
fn is_lan_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            address.is_private() || address.is_link_local() || address.is_loopback()
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unicast_link_local()
                || (address.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn coordinator_rejects_public_bind_addresses() {
        assert!(!is_lan_address("8.8.8.8".parse().expect("address")));
        assert!(is_lan_address("192.168.10.4".parse().expect("address")));
        assert!(is_lan_address("fd00::1".parse().expect("address")));
    }

    #[test]
    fn pairing_status_follows_the_pinned_coordinator_before_the_active_vault() {
        let coordinator = pairing::CoordinatorPin {
            device_id: "device-desktop".to_string(),
            device_label: None,
            endpoint: "192.168.10.4:43821".to_string(),
            vault_id: "vault-coordinated".to_string(),
            generation: 3,
            certificate_fingerprint: "fingerprint".to_string(),
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
    fn development_and_production_coordinators_use_distinct_ports() {
        assert_eq!(coordinator_port("org.opengrimoire.ganbaruai"), 43_821);
        assert_eq!(coordinator_port("org.opengrimoire.ganbaruai.dev"), 43_822);
    }
}
