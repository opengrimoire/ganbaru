//! Adding an always-on anchor server to the active vault from an admin desktop, and recovering a
//! vault through such a server on a new desktop after every device was lost.
//!
//! A server dials nobody, so both flows run in reverse over a setup conversation this device opens
//! with the code the operator printed. No admin key or person key leaves this device.

use super::authority;
use super::commands::run_authority;
use super::host::{AppHost, SyncHost};
use super::service::{AuthorityAction, SyncRuntime, blocking};
use super::writer::ensure_device_key;
use crate::vault::handoff::pairing::{PairingManager, PinnedCharter, random_token};
use crate::vault::handoff::protocol::encode_sync_charter;
use crate::vault::handoff::receiver::{ReceiveOutcome, receive_recovered};
use crate::vault::handoff::snapshot::{archive_active_vault, bundle_metadata};
use crate::vault::quiescence::begin_snapshot_quiescence;
use ganbaru_handoff::protocol::anchor::AnchorSetupCode;
use ganbaru_sync::{AuthorizationView, Engine, EnrolledDevice, local};
use ganbaru_sync_contracts::authorization::Server;
use ganbaru_sync_contracts::{DeviceKind, DeviceRole};
use ganbaru_sync_replica::anchor_setup::{
    AnchorSetupError, SetupConnection, open_setup, setup_target,
};
use ganbaru_sync_replica::authority::{Enrollment, ensure_admin_key, recovery_enrollment};
use ganbaru_sync_replica::service::{RELAY_LOOPBACK, ServerEnrollment};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, State};

/// How long a call waits for the anchor to sequence the enrollment before it hands back to the
/// caller, which repeats the call with the same code.
const ENROLLMENT_WAIT: Duration = Duration::from_secs(20);
const ENROLLMENT_POLL: Duration = Duration::from_millis(500);
/// The label the log records for a desktop enrolled through the recovery kit.
const RECOVERED_DESKTOP_LABEL: &str = "Recovered desktop";

/// How far adding a server came.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AddServerStep {
    /// The enrollment waits for the anchor; repeat the call with the same code.
    WaitingForAnchor,
    /// The server runs the vault.
    Active,
    /// The server speaks an older protocol and must be updated first.
    ServerNeedsUpdate,
}

/// What opening a setup conversation found.
enum Opened {
    Proven(Box<SetupConnection>),
    ServerNeedsUpdate,
}

async fn open<R: Runtime>(
    app: &AppHandle<R>,
    code: &AnchorSetupCode,
    vault_id: Option<&str>,
) -> Result<Opened, String> {
    let version = app.package_info().version.to_string();
    match open_setup(code, &version, vault_id, RELAY_LOOPBACK).await {
        Ok(connection) => Ok(Opened::Proven(Box::new(connection))),
        Err(AnchorSetupError::ServerNeedsUpdate) => Ok(Opened::ServerNeedsUpdate),
        Err(error) => Err(error.to_string()),
    }
}

/// Whether the selected log enrolls the server of `enrollment` and names it in the configuration.
fn is_enrolled(view: &AuthorizationView, enrollment: &ServerEnrollment) -> bool {
    names_server(&view.devices, view.server.as_ref(), enrollment)
}

fn names_server(
    devices: &[EnrolledDevice],
    server: Option<&Server>,
    enrollment: &ServerEnrollment,
) -> bool {
    let live = devices.iter().any(|device| {
        device.device_id == enrollment.device_id
            && device.removed_revision.is_none()
            && device.device_key == enrollment.device_key
            && device.kind == DeviceKind::Server
            && device.role == DeviceRole::Admin(enrollment.admin_key)
    });
    live && server == Some(&enrollment.server)
}

async fn wait_for_enrollment<R: Runtime>(
    app: &AppHandle<R>,
    runtime: &SyncRuntime,
    enrollment: &ServerEnrollment,
) -> Result<bool, String> {
    let deadline = tokio::time::Instant::now() + ENROLLMENT_WAIT;
    loop {
        runtime.sync_now();
        tokio::time::sleep(ENROLLMENT_POLL).await;
        let view = authority::authorization(app).await?;
        if view.is_some_and(|view| is_enrolled(&view, enrollment)) {
            return Ok(true);
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(false);
        }
    }
}

/// The charter of the active vault as a receiver pins it.
async fn pinned_charter<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<PinnedCharter, String> {
    let pool = crate::db::connect_active_vault_for_sync(app).await?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let ctx = local::space_context(&mut conn, vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
        .ok_or_else(|| "this vault has no sync space".to_string())?;
    let charter = Engine::vault()
        .charter(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read sync charter: {error}"))?
        .ok_or_else(|| "this vault has no sync charter".to_string())?;
    Ok(PinnedCharter {
        charter: encode_sync_charter(&charter),
        person_key: ctx.person_key.to_text(),
    })
}

/// Prepares a snapshot of the active vault as a linking device would receive it and uploads it
/// over a fresh setup conversation.
async fn upload_snapshot<R: Runtime>(
    app: &AppHandle<R>,
    code: &AnchorSetupCode,
    vault_id: &str,
    take_anchor: bool,
) -> Result<AddServerStep, String> {
    let pairing = app.state::<PairingManager>().inner().clone();
    let charter = pinned_charter(app, vault_id).await?;
    let transfer_id = random_token("transfer")?;
    let quiescence = begin_snapshot_quiescence(app).await?;
    let archived = archive_active_vault(app, &pairing, &transfer_id).await;
    drop(quiescence);
    let archive = archived?;
    let uploaded = async {
        let metadata = bundle_metadata(
            app,
            vault_id.to_string(),
            pairing.device_id()?,
            transfer_id,
            &archive,
        )?;
        match open(app, code, Some(vault_id)).await? {
            Opened::ServerNeedsUpdate => Ok(AddServerStep::ServerNeedsUpdate),
            Opened::Proven(connection) => connection
                .upload(metadata, &archive, charter, take_anchor)
                .await
                .map(|()| AddServerStep::Active)
                .map_err(|error| error.to_string()),
        }
    }
    .await;
    if let Err(error) = std::fs::remove_file(&archive) {
        eprintln!("sync: remove the uploaded snapshot: {error}");
    }
    uploaded
}

/// Adds the server behind `code` to the active vault: proves the code, proposes the server's
/// enrollment as an admin and its configuration, and uploads a snapshot it activates. With
/// `take_anchor`, the server takes the anchor once it can. The code stays valid until the server
/// activates, so a caller answered `waitingForAnchor` repeats the call with it.
#[tauri::command]
pub async fn sync_add_anchor_server<R: Runtime>(
    app: AppHandle<R>,
    runtime: State<'_, SyncRuntime>,
    code: String,
    take_anchor: bool,
) -> Result<AddServerStep, String> {
    let code = AnchorSetupCode::from_text(code.trim())?;
    let vault_id = AppHost::of(&app).active_vault_id()?;
    let enrollment = match open(&app, &code, Some(&vault_id)).await? {
        Opened::ServerNeedsUpdate => return Ok(AddServerStep::ServerNeedsUpdate),
        Opened::Proven(connection) => connection.identity().enrollment.clone(),
    };
    let view = authority::authorization(&app)
        .await?
        .ok_or_else(|| "this vault has no sync space".to_string())?;
    if !is_enrolled(&view, &enrollment) {
        run_authority(&runtime, AuthorityAction::EnrollServer(enrollment.clone())).await?;
        if !wait_for_enrollment(&app, &runtime, &enrollment).await? {
            return Ok(AddServerStep::WaitingForAnchor);
        }
    }
    upload_snapshot(&app, &code, &vault_id, take_anchor).await
}

/// Recovers the vault an anchor server runs on this new desktop: proves the code, derives the
/// recovery admin key from `kit` in memory, enrolls this desktop as an admin through the server,
/// and copies the vault into a new folder. A copy that failed after the enrollment is repeated
/// with a fresh code and the same kit.
#[tauri::command]
pub async fn sync_recover_through_server(
    app: AppHandle,
    code: String,
    kit: String,
) -> Result<ReceiveOutcome, String> {
    let code = AnchorSetupCode::from_text(code.trim())?;
    let Opened::Proven(mut connection) = open(&app, &code, None).await? else {
        return Err(AnchorSetupError::ServerNeedsUpdate.to_string());
    };
    let space = connection
        .identity()
        .space
        .clone()
        .ok_or_else(|| "this server runs no vault yet".to_string())?;
    let charter = PinnedCharter {
        charter: space.charter,
        person_key: space.person_key,
    }
    .verify(&space.vault_id)?;
    if AppHost::of(&app).active_vault_id()? == space.vault_id {
        return Err("this vault is already open on this device".to_string());
    }
    let host = AppHost::of(&app);
    let marker = host.sync_files(&space.vault_id)?.server_recovery_path();
    if !marker.exists() {
        let store = host.key_store()?;
        let vault_id = space.vault_id.clone();
        let (device, admin) = blocking(move || {
            Ok((
                ensure_device_key(store.as_ref(), &vault_id)?,
                ensure_admin_key(store.as_ref(), &vault_id)?,
            ))
        })
        .await?;
        let pairing = app.state::<PairingManager>();
        let proposal = recovery_enrollment(
            charter.charter.space,
            charter.hash(),
            &kit,
            Enrollment {
                device_id: pairing.device_id()?,
                device_key: device.public_key(),
                tls_fingerprint: Sha256::digest(pairing.certificate()?.as_ref()).into(),
                label: RECOVERED_DESKTOP_LABEL.to_string(),
                device_kind: DeviceKind::Desktop,
            },
            admin.public_key(),
            super::now_ms()?,
        )?;
        connection
            .recover(&proposal)
            .await
            .map_err(|error| error.to_string())?;
        let written = marker.clone();
        blocking(move || {
            ganbaru_atomic_files::write_file_atomically(
                &written,
                b"",
                ganbaru_atomic_files::FileAccess::OwnerOnly,
            )
            .map_err(|error| format!("record the recovery: {error}"))
        })
        .await?;
    }
    drop(connection);
    receive_recovered(app, setup_target(&code), space.vault_id).await
}
