//! The local person identity row and the device-local copy of the signing key.

use super::{ContactsError, IdentityView};
#[cfg(desktop)]
use crate::vault::handoff::coordinator::CoordinatorResponse;
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::protocol::ControlMessage;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_contacts::{
    CARD_NONCE_BYTES, PersonKeyPair, PersonPublicKey, decode_nonce, encode_nonce, random_bytes,
};
use sqlx::{FromRow, SqlitePool};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};

#[cfg(desktop)]
const KEYRING_SERVICE: &str = "com.ganbaru-ai.contacts";
const KEY_REFERENCE_PREFIX: &str = "person-key:";
#[cfg(mobile)]
const KEY_DIRECTORY: &str = "contacts";
#[cfg(desktop)]
const KEY_UNAVAILABLE_CODE: &str = "key_unavailable";

/// Vault whose key copy this device has already confirmed, so reconnect polls skip the lookup.
static KEY_CONFIRMED_FOR_VAULT: Mutex<Option<String>> = Mutex::new(None);

/// Serializes identity creation between Contacts commands and the sync service.
static IDENTITY_CREATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The person identity as this device knows it.
pub(crate) struct LocalIdentity {
    pub public_key: PersonPublicKey,
    pub card_nonce: [u8; CARD_NONCE_BYTES],
    pub card_revision: i64,
    /// Present only when this device holds a copy of the private key.
    pub key: Option<PersonKeyPair>,
}

impl LocalIdentity {
    pub(crate) fn view(&self) -> IdentityView {
        IdentityView {
            public_key: self.public_key.to_text(),
            contact_id: self.public_key.contact_id(),
            card_revision: self.card_revision,
            private_key_available: self.key.is_some(),
        }
    }

    pub(crate) fn signing_key(&self) -> Result<&PersonKeyPair, ContactsError> {
        self.key.as_ref().ok_or_else(|| {
            ContactsError::KeyUnavailable(
                "this device holds no copy of the person key yet".to_string(),
            )
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct IdentityRow {
    pub public_key: String,
    pub card_nonce: String,
    pub card_revision: i64,
}

pub(crate) async fn load_identity_row(pool: &SqlitePool) -> Result<Option<IdentityRow>, String> {
    sqlx::query_as(
        "SELECT public_key, card_nonce, card_revision FROM contacts_local_identity WHERE singleton = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("read person identity: {error}"))
}

/// Combines the identity row with a key copy, which is kept only when it matches the row.
pub(crate) fn identity_from_row(
    row: IdentityRow,
    key_pkcs8: Option<&[u8]>,
) -> Result<LocalIdentity, String> {
    let public_key = PersonPublicKey::from_text(&row.public_key)
        .map_err(|_| "stored person public key is invalid".to_string())?;
    let card_nonce =
        decode_nonce(&row.card_nonce).ok_or_else(|| "stored card nonce is invalid".to_string())?;
    let key = key_pkcs8
        .and_then(|bytes| PersonKeyPair::from_pkcs8(bytes).ok())
        .filter(|key| key.public_key() == public_key);
    Ok(LocalIdentity {
        public_key,
        card_nonce,
        card_revision: row.card_revision,
        key,
    })
}

/// The device-local copy of the person key when it matches `public_key`. It reads no vault
/// database, so it is usable while the vault is quiesced.
pub(crate) async fn person_key_matching<R: Runtime>(
    app: &AppHandle<R>,
    public_key: &PersonPublicKey,
) -> Result<Option<PersonKeyPair>, String> {
    let key = load_key(app).await?;
    Ok(key
        .and_then(|bytes| PersonKeyPair::from_pkcs8(&bytes).ok())
        .filter(|key| key.public_key() == *public_key))
}

/// Loads the identity with whichever key copy this device holds, without creating one.
pub(crate) async fn load_identity<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> Result<Option<LocalIdentity>, String> {
    let Some(row) = load_identity_row(pool).await? else {
        return Ok(None);
    };
    let key = load_key(app).await?;
    identity_from_row(row, key.as_deref()).map(Some)
}

/// Loads the identity, creating it on the first writable use of a vault that has none.
pub(crate) async fn ensure_identity<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> Result<LocalIdentity, ContactsError> {
    if let Some(identity) = load_identity(app, pool).await? {
        return Ok(identity);
    }
    // Callers race on a fresh vault, and a losing insert removes the stored key, so creation
    // runs one at a time and rechecks the row.
    let _creating = IDENTITY_CREATION.lock().await;
    if let Some(identity) = load_identity(app, pool).await? {
        return Ok(identity);
    }
    let status = crate::vault::ownership::active_status(app)?;
    if !status.can_write {
        return Err(ContactsError::IdentityUnavailable(
            "the vault owner creates the person identity".to_string(),
        ));
    }
    if app.state::<PairingManager>().coordinator_pin()?.is_some() {
        return Err(ContactsError::IdentityUnavailable(
            "the coordinator creates the person identity".to_string(),
        ));
    }
    let (pkcs8, key) = PersonKeyPair::generate()
        .map_err(|error| ContactsError::Failed(format!("generate person key: {error}")))?;
    let mut card_nonce = [0u8; CARD_NONCE_BYTES];
    random_bytes(&mut card_nonce)
        .map_err(|error| ContactsError::Failed(format!("generate card nonce: {error}")))?;
    let public_key = key.public_key();
    store_key(app, pkcs8).await?;
    let now = super::store::format_time(super::store::now());
    let inserted = sqlx::query(
        "INSERT INTO contacts_local_identity (singleton, public_key, card_nonce, created_at, updated_at)
         VALUES (1, ?, ?, ?, ?)",
    )
    .bind(public_key.to_text())
    .bind(encode_nonce(&card_nonce))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await;
    if let Err(error) = inserted {
        let _ = remove_key(app).await;
        return Err(ContactsError::Failed(format!(
            "store person identity: {error}"
        )));
    }
    Ok(LocalIdentity {
        public_key,
        card_nonce,
        card_revision: 1,
        key: Some(key),
    })
}

fn key_reference(vault_id: &str) -> String {
    format!("{KEY_REFERENCE_PREFIX}{vault_id}")
}

#[cfg(desktop)]
fn keyring_entry(vault_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, &key_reference(vault_id))
        .map_err(|error| format!("open person key store: {error}"))
}

#[cfg(desktop)]
fn read_key_sync(vault_id: &str) -> Result<Option<Vec<u8>>, String> {
    match keyring_entry(vault_id)?.get_password() {
        Ok(encoded) => URL_SAFE_NO_PAD
            .decode(encoded)
            .map(Some)
            .map_err(|error| format!("decode stored person key: {error}")),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("read person key: {error}")),
    }
}

#[cfg(desktop)]
fn write_key_sync(vault_id: &str, pkcs8: &[u8]) -> Result<(), String> {
    keyring_entry(vault_id)?
        .set_password(&URL_SAFE_NO_PAD.encode(pkcs8))
        .map_err(|error| format!("store person key: {error}"))
}

#[cfg(desktop)]
fn remove_key_sync(vault_id: &str) -> Result<(), String> {
    match keyring_entry(vault_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("remove person key: {error}")),
    }
}

#[cfg(mobile)]
fn key_path<R: Runtime>(app: &AppHandle<R>, vault_id: &str) -> Result<std::path::PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    Ok(directory
        .join(KEY_DIRECTORY)
        .join(format!("{}.key", key_reference(vault_id).replace(':', "-"))))
}

#[cfg(mobile)]
fn read_key_sync(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read person key: {error}")),
    }
}

#[cfg(mobile)]
fn write_key_sync(path: &std::path::Path, pkcs8: &[u8]) -> Result<(), String> {
    crate::vault::handoff::pairing::write_private_file_atomically(path, pkcs8)
}

#[cfg(mobile)]
fn remove_key_sync(path: &std::path::Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove person key: {error}")),
    }
}

async fn load_key<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Vec<u8>>, String> {
    let vault_id = crate::vault::active_vault_id(app)?;
    #[cfg(desktop)]
    let task = tauri::async_runtime::spawn_blocking(move || read_key_sync(&vault_id));
    #[cfg(mobile)]
    let task = {
        let path = key_path(app, &vault_id)?;
        tauri::async_runtime::spawn_blocking(move || read_key_sync(&path))
    };
    task.await
        .map_err(|error| format!("person key task: {error}"))?
}

async fn store_key<R: Runtime>(app: &AppHandle<R>, pkcs8: Vec<u8>) -> Result<(), String> {
    let vault_id = crate::vault::active_vault_id(app)?;
    #[cfg(desktop)]
    let task = tauri::async_runtime::spawn_blocking(move || write_key_sync(&vault_id, &pkcs8));
    #[cfg(mobile)]
    let task = {
        let path = key_path(app, &vault_id)?;
        tauri::async_runtime::spawn_blocking(move || write_key_sync(&path, &pkcs8))
    };
    task.await
        .map_err(|error| format!("person key task: {error}"))?
}

async fn remove_key<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let vault_id = crate::vault::active_vault_id(app)?;
    #[cfg(desktop)]
    let task = tauri::async_runtime::spawn_blocking(move || remove_key_sync(&vault_id));
    #[cfg(mobile)]
    let task = {
        let path = key_path(app, &vault_id)?;
        tauri::async_runtime::spawn_blocking(move || remove_key_sync(&path))
    };
    task.await
        .map_err(|error| format!("person key task: {error}"))?
}

/// Coordinator side: hands the key to a linked device that already proved its membership.
#[cfg(desktop)]
pub(crate) async fn release_key_for_coordinator<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<CoordinatorResponse, String> {
    let unavailable = |message: &str| CoordinatorResponse::Rejected {
        code: KEY_UNAVAILABLE_CODE.to_string(),
        message: message.to_string(),
        retryable: true,
    };
    let pool = super::contacts_pool(app).await?;
    let Some(row) = load_identity_row(&pool).await? else {
        return Ok(unavailable("the vault has no person identity yet"));
    };
    let Some(pkcs8) = load_key(app).await? else {
        return Ok(unavailable("the coordinator holds no person key copy"));
    };
    let identity = identity_from_row(row, Some(&pkcs8))?;
    if identity.key.is_none() {
        return Ok(unavailable(
            "the coordinator person key does not match the identity",
        ));
    }
    Ok(CoordinatorResponse::PersonKeyReleased {
        public_key: identity.public_key.to_text(),
        private_key_pkcs8: URL_SAFE_NO_PAD.encode(pkcs8),
    })
}

/// Linked device side: fetches the key from the coordinator when the replica carries an
/// identity row but this device has no key copy. Returns true when a copy is present.
pub(crate) async fn try_release<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(vault_id) = crate::vault::active_vault_id(app) else {
        return false;
    };
    if KEY_CONFIRMED_FOR_VAULT
        .lock()
        .ok()
        .is_some_and(|confirmed| confirmed.as_deref() == Some(vault_id.as_str()))
    {
        return true;
    }
    let Ok(pool) = super::contacts_pool(app).await else {
        return false;
    };
    let Ok(Some(row)) = load_identity_row(&pool).await else {
        return false;
    };
    if matches!(load_key(app).await, Ok(Some(_))) {
        confirm_key(vault_id);
        return true;
    }
    let manager = app.state::<PairingManager>().inner().clone();
    let response = match crate::vault::handoff::transport::request_person_key(&manager).await {
        Ok(response) => response,
        Err(_) => return false,
    };
    let ControlMessage::PersonKeyRelease {
        public_key,
        private_key_pkcs8,
    } = response
    else {
        return false;
    };
    if public_key != row.public_key {
        return false;
    }
    let Ok(pkcs8) = URL_SAFE_NO_PAD.decode(private_key_pkcs8) else {
        return false;
    };
    let Ok(identity) = identity_from_row(row, Some(&pkcs8)) else {
        return false;
    };
    if identity.key.is_none() {
        return false;
    }
    match store_key(app, pkcs8).await {
        Ok(()) => {
            confirm_key(vault_id);
            true
        }
        Err(_) => false,
    }
}

fn confirm_key(vault_id: String) {
    if let Ok(mut confirmed) = KEY_CONFIRMED_FOR_VAULT.lock() {
        *confirmed = Some(vault_id);
    }
}
