//! Joining the vault an always-on anchor server runs from a device that never met another device,
//! and recovering a vault through such a server after every device was lost. Desktops and phones
//! run the same flows; only the device kind and the folder the copy opens in differ.
//!
//! A server dials nobody, so both flows run in reverse over a setup conversation this device opens
//! with the code the operator printed. No admin key or person key leaves this device.

use super::host::{AppHost, SyncHost};
use super::service::blocking;
use super::writer::ensure_device_key;
use super::{AnsweredKitKeys, CopyProof, JoinedKeys, RECOVERY_KIT_MISMATCH, RecoveryBinding};
use crate::vault::handoff::pairing::{PairingManager, PinnedCharter};
use crate::vault::handoff::receiver::{ReceiveOutcome, receive_recovered};
use ganbaru_contacts::PersonPublicKey;
use ganbaru_handoff::protocol::anchor::{AnchorSetupCode, AnchorSpace};
use ganbaru_handoff::protocol::decode_sync_admin_key;
use ganbaru_sync_contracts::{
    AdminPublicKey, DeviceKind, KitCode, RecoveryPublicKeys, SignedCharter,
};
use ganbaru_sync_replica::anchor_setup::{
    AnchorSetupError, JoinOutcome, JoinRequest, SetupConnection, open_setup, setup_target,
};
use ganbaru_sync_replica::authority::{
    Enrollment, ensure_admin_key, kit_public_keys, recovery_enrollment,
};
use ganbaru_sync_replica::service::RELAY_LOOPBACK;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime};

/// The kind this installation enrolls as.
const DEVICE_KIND: DeviceKind = if cfg!(mobile) {
    DeviceKind::Mobile
} else {
    DeviceKind::Desktop
};

/// The label the log records for this installation when it is enrolled through the recovery
/// kit, before the person renames it.
const RECOVERED_DEVICE_LABEL: &str = recovered_label(DEVICE_KIND);

/// The recovered label of an installation of `kind`.
const fn recovered_label(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Mobile => "Recovered phone",
        DeviceKind::Desktop | DeviceKind::Server => "Recovered desktop",
    }
}

/// Reads the setup code an anchor server printed as a QR code from a grayscale camera frame,
/// answering its text form so scanning and pasting share one path.
#[tauri::command]
pub fn sync_decode_server_qr(width: usize, height: usize, luma: Vec<u8>) -> Result<String, String> {
    AnchorSetupCode::from_qr_luma(width, height, &luma)?.to_text()
}

/// Opens a setup conversation with the server `code` names.
pub(super) async fn open<R: Runtime>(
    app: &AppHandle<R>,
    code: &AnchorSetupCode,
    vault_id: Option<&str>,
) -> Result<SetupConnection, AnchorSetupError> {
    let version = app.package_info().version.to_string();
    open_setup(code, &version, vault_id, RELAY_LOOPBACK).await
}

/// The vault a setup connection's server runs, with the charter this device then pins. Refused
/// when the server runs nothing yet or that vault is already open here.
fn served_space<R: Runtime>(
    app: &AppHandle<R>,
    connection: &SetupConnection,
) -> Result<(AnchorSpace, SignedCharter, PersonPublicKey), String> {
    let space = connection
        .identity()
        .space
        .clone()
        .ok_or_else(|| "this server runs no vault yet".to_string())?;
    let pinned = PinnedCharter {
        charter: space.charter.clone(),
        person_key: space.person_key.clone(),
    };
    let (charter, person_key) = super::host::charter_trust(&pinned, &space.vault_id)?;
    if AppHost::of(app).active_vault_id()? == space.vault_id {
        return Err("this vault is already open on this device".to_string());
    }
    Ok((space, charter, person_key))
}

/// The enrollment of this installation as an admin of `vault_id`, creating its keys on first
/// use, with the admin key it holds.
async fn own_enrollment<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
    label: String,
) -> Result<(Enrollment, AdminPublicKey), String> {
    let store = AppHost::of(app).key_store()?;
    let owned_vault_id = vault_id.to_string();
    let (device, admin) = blocking(move || {
        Ok((
            ensure_device_key(store.as_ref(), &owned_vault_id)?,
            ensure_admin_key(store.as_ref(), &owned_vault_id)?,
        ))
    })
    .await?;
    let pairing = app.state::<PairingManager>();
    let enrollment = Enrollment {
        device_id: pairing.device_id()?,
        device_key: device.public_key(),
        tls_fingerprint: Sha256::digest(pairing.certificate()?.as_ref()).into(),
        label,
        device_kind: DEVICE_KIND,
    };
    Ok((enrollment, admin.public_key()))
}

/// Recovers the vault an anchor server runs on this new device: proves the code, derives the
/// recovery admin key from `kit` in memory, enrolls this device as an admin through the server,
/// and copies the vault here. The server must name the kit's admin key before the signed
/// enrollment discloses it, and the copy opens only when its log names the kit through a key the
/// server answered. A copy that failed after the enrollment is repeated with a fresh code and the
/// same kit.
#[tauri::command]
pub async fn sync_recover_through_server(
    app: AppHandle,
    code: String,
    kit: String,
) -> Result<ReceiveOutcome, String> {
    let kit = KitCode::new(kit);
    let kit_keys = kit_public_keys(&kit)?;
    let code = AnchorSetupCode::from_text(code.trim())?;
    let mut connection = open(&app, &code, None)
        .await
        .map_err(|error| error.to_string())?;
    let (space, charter, person_key) = served_space(&app, &connection)?;
    let answered = server_names_kit(&space, &kit_keys)?;
    let files = AppHost::of(&app).sync_files(&space.vault_id)?;
    if !files.server_sync_path().exists() {
        let label = RECOVERED_DEVICE_LABEL.to_string();
        let (enrollment, admin_key) = own_enrollment(&app, &space.vault_id, label).await?;
        let proposal = recovery_enrollment(
            charter.charter.space,
            charter.hash(),
            &kit,
            enrollment,
            admin_key,
            super::now_ms()?,
        )?;
        connection
            .recover(&proposal)
            .await
            .map_err(|error| error.to_string())?;
        blocking(move || files.mark_server_sync()).await?;
    }
    drop(connection);
    let binding = RecoveryBinding {
        charter,
        person_key,
        proof: CopyProof::Kit {
            keys: kit_keys,
            answered,
        },
    };
    receive_recovered(app, setup_target(&code), space.vault_id, binding).await
}

/// Refusal of a join the server holds until the vault's anchor collects it. The webview maps it
/// to a localized message, so the text is stable.
pub(crate) const JOIN_WAITING_FOR_ANCHOR: &str =
    "the server holds this enrollment until the vault's anchor is online";

/// Joins the vault an always-on server runs on this device, which needs no other device: proves
/// the code, asks the server to enroll this device as an admin, and copies the vault here. The
/// code is the trust: only a shell on the server prints it, and it pins the server's
/// certificate. The copy opens only when its log verifies under the charter the server answered
/// and enrolls this device with the keys it joined with. A copy that failed after the
/// enrollment, or a join the server holds for its anchor, is repeated with a fresh code.
#[tauri::command]
pub async fn sync_join_through_server(
    app: AppHandle,
    code: String,
) -> Result<ReceiveOutcome, String> {
    let code = AnchorSetupCode::from_text(code.trim())?;
    let mut connection = open(&app, &code, None)
        .await
        .map_err(|error| error.to_string())?;
    let (space, charter, person_key) = served_space(&app, &connection)?;
    let label = crate::vault::handoff::suggested_device_label();
    let (enrollment, admin_key) = own_enrollment(&app, &space.vault_id, label.clone()).await?;
    let joined = JoinedKeys {
        device_id: enrollment.device_id.clone(),
        device_key: enrollment.device_key,
        admin_key,
    };
    let files = AppHost::of(&app).sync_files(&space.vault_id)?;
    if !files.server_sync_path().exists() {
        let request = JoinRequest {
            device_id: enrollment.device_id,
            device_label: label,
            device_key: enrollment.device_key,
            admin_key,
            tls_fingerprint: enrollment.tls_fingerprint,
        };
        match connection
            .join(&request)
            .await
            .map_err(|error| error.to_string())?
        {
            JoinOutcome::Enrolled(_) => {}
            JoinOutcome::WaitingForAnchor => return Err(JOIN_WAITING_FOR_ANCHOR.to_string()),
        }
        blocking(move || files.mark_server_sync()).await?;
    }
    drop(connection);
    let binding = RecoveryBinding {
        charter,
        person_key,
        proof: CopyProof::Joined(joined),
    };
    receive_recovered(app, setup_target(&code), space.vault_id, binding).await
}

/// The kit keys `space` answered, refused unless one of them is the admin key of `kit`. A server
/// that never held the vault cannot name that key, because no proposal has disclosed it yet.
fn server_names_kit(
    space: &AnchorSpace,
    kit: &RecoveryPublicKeys,
) -> Result<AnsweredKitKeys, String> {
    let answered = AnsweredKitKeys {
        kit_admin_key: decode_sync_admin_key(&space.kit_admin_key)?,
        recovery_admin_key: decode_sync_admin_key(&space.recovery_admin_key)?,
    };
    if answered.kit_admin_key != kit.admin && answered.recovery_admin_key != kit.admin {
        return Err(RECOVERY_KIT_MISMATCH.to_string());
    }
    Ok(answered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_handoff::protocol::encode_sync_admin_key;
    use ganbaru_sync_contracts::{AdminKeyPair, AdminPublicKey, RecoverySeed};

    fn kit() -> RecoveryPublicKeys {
        RecoverySeed::from_bytes([31; 32]).public_keys().unwrap()
    }

    fn admin(seed: u8) -> AdminPublicKey {
        AdminKeyPair::from_seed(&[seed; 32]).unwrap().public_key()
    }

    fn answer(kit_admin_key: &AdminPublicKey, recovery_admin_key: &AdminPublicKey) -> AnchorSpace {
        AnchorSpace {
            vault_id: "vault-recovered".to_string(),
            charter: "charter".to_string(),
            person_key: "person".to_string(),
            kit_admin_key: encode_sync_admin_key(kit_admin_key),
            recovery_admin_key: encode_sync_admin_key(recovery_admin_key),
        }
    }

    #[test]
    fn recovery_refuses_a_server_that_does_not_name_the_kit() {
        let forged = answer(&admin(32), &admin(33));
        assert_eq!(
            server_names_kit(&forged, &kit()),
            Err(RECOVERY_KIT_MISMATCH.to_string()),
            "a server that answers keys of its own gets no proposal"
        );
    }

    #[test]
    fn recovery_proceeds_when_the_server_names_the_kit_by_either_key() {
        let printed = answer(&kit().admin, &admin(33));
        assert_eq!(
            server_names_kit(&printed, &kit()),
            Ok(AnsweredKitKeys {
                kit_admin_key: kit().admin,
                recovery_admin_key: admin(33),
            })
        );
        let installed = answer(&admin(32), &kit().admin);
        assert_eq!(
            server_names_kit(&installed, &kit()),
            Ok(AnsweredKitKeys {
                kit_admin_key: admin(32),
                recovery_admin_key: kit().admin,
            })
        );
    }

    #[test]
    fn recovery_refuses_an_answer_with_a_malformed_kit_key() {
        let mut malformed = answer(&kit().admin, &kit().admin);
        malformed.kit_admin_key = "not base64!".to_string();
        assert!(server_names_kit(&malformed, &kit()).is_err());
    }

    #[test]
    fn a_recovered_installation_is_labeled_by_its_kind() {
        assert_eq!(recovered_label(DeviceKind::Mobile), "Recovered phone");
        assert_eq!(recovered_label(DeviceKind::Desktop), "Recovered desktop");
    }

    #[cfg(desktop)]
    #[test]
    fn a_desktop_build_enrolls_as_a_desktop() {
        assert_eq!(DEVICE_KIND, DeviceKind::Desktop);
        assert_eq!(RECOVERED_DEVICE_LABEL, "Recovered desktop");
    }
}
