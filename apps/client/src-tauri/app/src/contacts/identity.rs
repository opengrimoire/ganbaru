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
#[cfg(any(mobile, test))]
use ganbaru_handoff::key_wrap::{self, KeyWrapper};
use sqlx::{FromRow, SqlitePool};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};

#[cfg(desktop)]
const KEYRING_SERVICE: &str = "com.ganbaru.contacts";
const KEY_REFERENCE_PREFIX: &str = "person-key:";
/// Directory under the app config directory that holds the sealed person key copies on mobile.
pub(crate) const KEY_DIRECTORY: &str = "contacts";
#[cfg(desktop)]
const KEY_UNAVAILABLE_CODE: &str = "key_unavailable";

/// The vault id and identity public key whose key copy this device has already confirmed, so
/// reconnect polls skip the lookup while the identity row still names that key.
static KEY_CONFIRMED: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Serializes identity creation and card nonce repair between Contacts commands and the sync
/// service.
static IDENTITY_CREATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The person identity as this device knows it.
pub(crate) struct LocalIdentity {
    pub public_key: PersonPublicKey,
    /// `None` while the stored nonce does not decode: a snapshot copy skips sync admission, so
    /// the row can hold one, and the rest of the identity stays usable until it is rotated.
    pub card_nonce: Option<[u8; CARD_NONCE_BYTES]>,
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

    /// The nonce a card can carry, refused until an unreadable stored nonce is rotated.
    pub(crate) fn readable_nonce(&self) -> Result<[u8; CARD_NONCE_BYTES], ContactsError> {
        self.card_nonce
            .ok_or_else(|| ContactsError::Failed("the stored card nonce is unreadable".to_string()))
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct IdentityRow {
    pub public_key: String,
    pub card_nonce: String,
    pub card_revision: i64,
}

/// The person identity with the current card state. An identity whose card row is missing reads
/// as no identity; creation always writes both rows together.
pub(crate) async fn load_identity_row(pool: &SqlitePool) -> Result<Option<IdentityRow>, String> {
    sqlx::query_as(
        "SELECT i.public_key, c.card_nonce, c.card_revision
         FROM contacts_local_identity i
         JOIN contact_card c ON c.key = 'card'
         WHERE i.singleton = 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("read person identity: {error}"))
}

/// Combines the identity row with a key copy, which is kept only when it matches the row. Only
/// the public key can fail: the card row replicates, so its nonce never decides whether the
/// person identity loads.
pub(crate) fn identity_from_row(
    row: IdentityRow,
    key_pkcs8: Option<&[u8]>,
) -> Result<LocalIdentity, String> {
    let public_key = PersonPublicKey::from_text(&row.public_key)
        .map_err(|_| "stored person public key is invalid".to_string())?;
    let card_nonce = decode_nonce(&row.card_nonce);
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
    let identity = read_identity(pool, &|| load_key(app)).await?;
    Ok(identity.map(|(identity, _)| identity))
}

/// Reads the identity row, then the key copy `load_key` returns, and pairs the identity with the
/// card nonce text exactly as stored.
async fn read_identity<L, K>(
    pool: &SqlitePool,
    load_key: &L,
) -> Result<Option<(LocalIdentity, String)>, String>
where
    L: Fn() -> K,
    K: Future<Output = Result<Option<Vec<u8>>, String>>,
{
    let Some(row) = load_identity_row(pool).await? else {
        return Ok(None);
    };
    let stored_nonce = row.card_nonce.clone();
    let key = load_key().await?;
    identity_from_row(row, key.as_deref()).map(|identity| Some((identity, stored_nonce)))
}

/// The identity as found before any creation step.
pub(crate) enum ExistingIdentity {
    /// An identity whose card nonce is readable, after a repair when the stored one was not.
    Ready(LocalIdentity),
    /// No identity exists. The guard keeps creation to one caller until it is dropped.
    Missing(tokio::sync::MutexGuard<'static, ()>),
}

/// Loads the identity and heals an unreadable card nonce. A readable identity returns without
/// waiting on the creation lock. Otherwise the lock is taken and the row is read again, so a racing
/// caller never repairs or creates over what another caller just wrote.
pub(crate) async fn existing_identity<L, K>(
    pool: &SqlitePool,
    load_key: L,
) -> Result<ExistingIdentity, ContactsError>
where
    L: Fn() -> K,
    K: Future<Output = Result<Option<Vec<u8>>, String>>,
{
    if let Some((identity, _)) = read_identity(pool, &load_key).await?
        && identity.card_nonce.is_some()
    {
        return Ok(ExistingIdentity::Ready(identity));
    }
    let creating = IDENTITY_CREATION.lock().await;
    if let Some((identity, stored_nonce)) = read_identity(pool, &load_key).await? {
        return super::card::heal_unreadable_nonce(pool, identity, &stored_nonce)
            .await
            .map(ExistingIdentity::Ready);
    }
    Ok(ExistingIdentity::Missing(creating))
}

/// Writes the identity and its first card in one transaction.
async fn insert_identity(
    pool: &SqlitePool,
    public_key: &str,
    card_nonce: &str,
    now: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO contacts_local_identity (singleton, public_key, created_at) VALUES (1, ?, ?)",
    )
    .bind(public_key)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO contact_card (key, card_nonce, card_revision, updated_at)
         VALUES ('card', ?, 1, ?)",
    )
    .bind(card_nonce)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Loads the identity, creating it on the first writable use of a vault that has none and
/// rotating a card nonce that does not decode.
pub(crate) async fn ensure_identity<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> Result<LocalIdentity, ContactsError> {
    // Every key step names the vault read here, so a vault switch while this runs never moves
    // the key into another vault's entry.
    let vault_id = crate::vault::active_vault_id(app)?;
    // Callers race on a fresh vault, and a losing insert removes the stored key, so creation
    // runs under the guard `existing_identity` returns.
    let _creating = match existing_identity(pool, || load_key_for_vault(app, &vault_id)).await? {
        ExistingIdentity::Ready(identity) => return Ok(identity),
        ExistingIdentity::Missing(creating) => creating,
    };
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
    store_key(app, &vault_id, pkcs8).await?;
    let now = super::store::format_time(super::store::now());
    let inserted = insert_identity(
        pool,
        &public_key.to_text(),
        &encode_nonce(&card_nonce),
        &now,
    )
    .await;
    if let Err(error) = inserted {
        let _ = remove_key(app, &vault_id).await;
        return Err(ContactsError::Failed(format!(
            "store person identity: {error}"
        )));
    }
    Ok(LocalIdentity {
        public_key,
        card_nonce: Some(card_nonce),
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
    Ok(person_key_path_in(&directory, vault_id))
}

/// The file that holds the device-local person key copy of `vault_id` on Android, inside the
/// app config directory `config_dir`.
#[cfg(any(mobile, test))]
pub(crate) fn person_key_path_in(
    config_dir: &std::path::Path,
    vault_id: &str,
) -> std::path::PathBuf {
    config_dir
        .join(KEY_DIRECTORY)
        .join(format!("{}.key", key_reference(vault_id).replace(':', "-")))
}

/// The person key of `vault_id` sealed in the file at `path` when it matches `public_key`. It
/// reads no vault database, so it is usable while the vault is quiesced.
#[cfg(any(mobile, test))]
pub(crate) fn person_key_file_matching(
    wrapper: &dyn KeyWrapper,
    path: &std::path::Path,
    vault_id: &str,
    public_key: &PersonPublicKey,
) -> Result<Option<PersonKeyPair>, String> {
    Ok(read_key_file(wrapper, path, vault_id)?
        .and_then(|bytes| PersonKeyPair::from_pkcs8(&bytes).ok())
        .filter(|key| key.public_key() == *public_key))
}

/// The person key of `vault_id` sealed in the file at `path`, sealed with the vault's platform
/// key.
#[cfg(any(mobile, test))]
fn read_key_file(
    wrapper: &dyn KeyWrapper,
    path: &std::path::Path,
    vault_id: &str,
) -> Result<Option<Vec<u8>>, String> {
    key_wrap::read_protected_file(
        wrapper,
        &key_wrap::vault_keys_alias(vault_id),
        &key_reference(vault_id),
        path,
    )
    .map_err(|error| format!("read person key: {error}"))
}

/// Seals the person key `pkcs8` of `vault_id` into the file at `path`.
#[cfg(any(mobile, test))]
pub(crate) fn write_key_file(
    wrapper: &dyn KeyWrapper,
    path: &std::path::Path,
    vault_id: &str,
    pkcs8: &[u8],
) -> Result<(), String> {
    key_wrap::write_protected_file(
        wrapper,
        &key_wrap::vault_keys_alias(vault_id),
        &key_reference(vault_id),
        path,
        pkcs8,
    )
    .map_err(|error| format!("store person key: {error}"))
}

#[cfg(mobile)]
fn remove_key_sync(path: &std::path::Path) -> Result<(), String> {
    key_wrap::remove_key_file(path).map_err(|error| format!("remove person key: {error}"))
}

/// The platform key that seals the person key on mobile.
#[cfg(mobile)]
fn platform_wrapper() -> std::sync::Arc<dyn KeyWrapper> {
    crate::vault::process_managers::process_managers().key_wrapper()
}

async fn load_key<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Vec<u8>>, String> {
    load_key_for_vault(app, &crate::vault::active_vault_id(app)?).await
}

async fn load_key_for_vault<R: Runtime>(
    #[cfg_attr(desktop, allow(unused_variables))] app: &AppHandle<R>,
    vault_id: &str,
) -> Result<Option<Vec<u8>>, String> {
    let vault_id = vault_id.to_string();
    #[cfg(desktop)]
    let task = tauri::async_runtime::spawn_blocking(move || read_key_sync(&vault_id));
    #[cfg(mobile)]
    let task = {
        let path = key_path(app, &vault_id)?;
        let wrapper = platform_wrapper();
        tauri::async_runtime::spawn_blocking(move || {
            read_key_file(wrapper.as_ref(), &path, &vault_id)
        })
    };
    task.await
        .map_err(|error| format!("person key task: {error}"))?
}

async fn store_key<R: Runtime>(
    #[cfg_attr(desktop, allow(unused_variables))] app: &AppHandle<R>,
    vault_id: &str,
    pkcs8: Vec<u8>,
) -> Result<(), String> {
    let vault_id = vault_id.to_string();
    #[cfg(desktop)]
    let task = tauri::async_runtime::spawn_blocking(move || write_key_sync(&vault_id, &pkcs8));
    #[cfg(mobile)]
    let task = {
        let path = key_path(app, &vault_id)?;
        let wrapper = platform_wrapper();
        tauri::async_runtime::spawn_blocking(move || {
            write_key_file(wrapper.as_ref(), &path, &vault_id, &pkcs8)
        })
    };
    task.await
        .map_err(|error| format!("person key task: {error}"))?
}

async fn remove_key<R: Runtime>(
    #[cfg_attr(desktop, allow(unused_variables))] app: &AppHandle<R>,
    vault_id: &str,
) -> Result<(), String> {
    forget_confirmation(vault_id);
    let vault_id = vault_id.to_string();
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

/// Coordinator side: hands the key of `vault_id` to a device that already proved its
/// membership of that vault. The caller checks that `vault_id` is the active vault; the key is
/// still read for `vault_id` itself and must match the identity row, so a vault switch in
/// between yields a refusal instead of another vault's key.
#[cfg(desktop)]
pub(crate) async fn release_key_for_coordinator<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<CoordinatorResponse, String> {
    let pool = super::contacts_pool(app).await?;
    let row = load_identity_row(&pool).await?;
    let pkcs8 = match row {
        Some(_) => load_key_for_vault(app, vault_id).await?,
        None => None,
    };
    key_release_response(row, pkcs8)
}

/// The coordinator's answer to a key request, given the active identity row and the key copy
/// stored for the requested vault.
#[cfg(desktop)]
fn key_release_response(
    row: Option<IdentityRow>,
    pkcs8: Option<Vec<u8>>,
) -> Result<CoordinatorResponse, String> {
    let unavailable = |message: &str| CoordinatorResponse::Rejected {
        code: KEY_UNAVAILABLE_CODE.to_string(),
        message: message.to_string(),
        retryable: true,
    };
    let Some(row) = row else {
        return Ok(unavailable("the vault has no person identity yet"));
    };
    let Some(pkcs8) = pkcs8 else {
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
/// identity row but this device has no key copy that matches it. Returns true when a matching
/// copy is present.
pub(crate) async fn try_release<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(vault_id) = crate::vault::active_vault_id(app) else {
        return false;
    };
    let Ok(pool) = super::contacts_pool(app).await else {
        return false;
    };
    let Ok(Some(row)) = load_identity_row(&pool).await else {
        return false;
    };
    // The pool opens after the vault id is read, so a switch in between reads another vault's
    // row.
    if crate::vault::active_vault_id(app).ok().as_deref() != Some(vault_id.as_str()) {
        return false;
    }
    if is_confirmed(&vault_id, &row.public_key) {
        return true;
    }
    let manager = app.state::<PairingManager>().inner().clone();
    let fetch = || async move {
        match crate::vault::handoff::transport::request_person_key(&manager).await {
            Ok(ControlMessage::PersonKeyRelease {
                public_key,
                private_key_pkcs8,
            }) => Some(ReleasedKey {
                public_key,
                private_key_pkcs8,
            }),
            _ => None,
        }
    };
    release_person_key(&AppKeyCopies(app), &vault_id, &row, fetch).await
}

/// A person key as the coordinator released it.
struct ReleasedKey {
    public_key: String,
    private_key_pkcs8: String,
}

/// The device-local key copies a release reads and writes, each step bound to the vault it
/// names.
trait PersonKeyCopies {
    /// The vault that is active now.
    fn active_vault_id(&self) -> Result<String, String>;
    /// Admits the final key write; the admission is held until the write finished.
    fn admit_write(&self) -> Result<impl Send, String>;
    async fn load(&self, vault_id: &str) -> Result<Option<Vec<u8>>, String>;
    async fn store(&self, vault_id: &str, pkcs8: Vec<u8>) -> Result<(), String>;
}

struct AppKeyCopies<'a, R: Runtime>(&'a AppHandle<R>);

impl<R: Runtime> PersonKeyCopies for AppKeyCopies<'_, R> {
    fn active_vault_id(&self) -> Result<String, String> {
        crate::vault::active_vault_id(self.0)
    }

    fn admit_write(&self) -> Result<impl Send, String> {
        self.0
            .state::<crate::vault::write_fence::VaultWriteFence>()
            .acquire_managed_write()
    }

    async fn load(&self, vault_id: &str) -> Result<Option<Vec<u8>>, String> {
        load_key_for_vault(self.0, vault_id).await
    }

    async fn store(&self, vault_id: &str, pkcs8: Vec<u8>) -> Result<(), String> {
        store_key(self.0, vault_id, pkcs8).await
    }
}

/// Makes sure `copies` holds a key of `vault_id` that matches the identity `row` read from that
/// vault, fetching one with `fetch` when it does not. The fetched key is written into the entry
/// of `vault_id` only while that vault is still the active one, so a vault switch during the
/// fetch writes nothing, and the copy is confirmed only once it matches the row.
async fn release_person_key<C, F, Fut>(
    copies: &C,
    vault_id: &str,
    row: &IdentityRow,
    fetch: F,
) -> bool
where
    C: PersonKeyCopies,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Option<ReleasedKey>>,
{
    if let Ok(Some(stored)) = copies.load(vault_id).await
        && key_matches_row(row, &stored)
    {
        confirm_key(vault_id, &row.public_key);
        return true;
    }
    let Some(released) = fetch().await else {
        return false;
    };
    if released.public_key != row.public_key {
        return false;
    }
    let Ok(pkcs8) = URL_SAFE_NO_PAD.decode(released.private_key_pkcs8) else {
        return false;
    };
    if !key_matches_row(row, &pkcs8) {
        return false;
    }
    let Ok(_admitted) = copies.admit_write() else {
        return false;
    };
    if copies.active_vault_id().ok().as_deref() != Some(vault_id) {
        return false;
    }
    if copies.store(vault_id, pkcs8).await.is_err() {
        return false;
    }
    confirm_key(vault_id, &row.public_key);
    true
}

/// Whether the key `pkcs8` decodes and belongs to the identity `row` names.
fn key_matches_row(row: &IdentityRow, pkcs8: &[u8]) -> bool {
    let Ok(public_key) = PersonPublicKey::from_text(&row.public_key) else {
        return false;
    };
    PersonKeyPair::from_pkcs8(pkcs8).is_ok_and(|key| key.public_key() == public_key)
}

fn is_confirmed(vault_id: &str, public_key: &str) -> bool {
    KEY_CONFIRMED.lock().ok().is_some_and(|confirmed| {
        confirmed
            .as_ref()
            .is_some_and(|(vault, key)| vault == vault_id && key == public_key)
    })
}

fn confirm_key(vault_id: &str, public_key: &str) {
    if let Ok(mut confirmed) = KEY_CONFIRMED.lock() {
        *confirmed = Some((vault_id.to_string(), public_key.to_string()));
    }
}

/// Drops the confirmation of `vault_id`, whose key copy is being removed.
fn forget_confirmation(vault_id: &str) {
    if let Ok(mut confirmed) = KEY_CONFIRMED.lock()
        && confirmed
            .as_ref()
            .is_some_and(|(vault, _)| vault == vault_id)
    {
        *confirmed = None;
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    fn row_for(public_key: &PersonPublicKey) -> IdentityRow {
        IdentityRow {
            public_key: public_key.to_text(),
            card_nonce: encode_nonce(&[7; CARD_NONCE_BYTES]),
            card_revision: 1,
        }
    }

    #[test]
    fn key_release_survives_an_unreadable_card_nonce() {
        let (pkcs8, key) = PersonKeyPair::generate().unwrap();
        let mut row = row_for(&key.public_key());
        row.card_nonce = "!".repeat(22);

        let released = key_release_response(Some(row), Some(pkcs8));

        assert!(
            matches!(
                released,
                Ok(CoordinatorResponse::PersonKeyReleased { ref public_key, .. })
                    if *public_key == key.public_key().to_text()
            ),
            "the key is released whatever the card nonce holds"
        );
    }

    #[test]
    fn key_release_refuses_a_key_copy_that_does_not_match_the_open_vault() {
        let (pkcs8, key) = PersonKeyPair::generate().unwrap();
        let (_, other) = PersonKeyPair::generate().unwrap();

        let released =
            key_release_response(Some(row_for(&key.public_key())), Some(pkcs8.clone())).unwrap();
        assert!(matches!(
            released,
            CoordinatorResponse::PersonKeyReleased { ref public_key, .. }
                if *public_key == key.public_key().to_text()
        ));

        // The key stored for the requested vault belongs to another identity than the open one.
        let mismatched =
            key_release_response(Some(row_for(&other.public_key())), Some(pkcs8)).unwrap();
        assert!(matches!(
            mismatched,
            CoordinatorResponse::Rejected { ref code, .. } if code == KEY_UNAVAILABLE_CODE
        ));

        for refused in [
            key_release_response(None, None).unwrap(),
            key_release_response(Some(row_for(&key.public_key())), None).unwrap(),
        ] {
            assert!(matches!(
                refused,
                CoordinatorResponse::Rejected { ref code, .. } if code == KEY_UNAVAILABLE_CODE
            ));
        }
    }

    /// Key copies by vault id with a settable active vault.
    struct FakeCopies {
        active: std::cell::RefCell<String>,
        stored: std::cell::RefCell<std::collections::HashMap<String, Vec<u8>>>,
    }

    impl FakeCopies {
        fn new(active: &str) -> Self {
            Self {
                active: std::cell::RefCell::new(active.to_string()),
                stored: std::cell::RefCell::default(),
            }
        }

        fn stored(&self, vault_id: &str) -> Option<Vec<u8>> {
            self.stored.borrow().get(vault_id).cloned()
        }
    }

    impl PersonKeyCopies for FakeCopies {
        fn active_vault_id(&self) -> Result<String, String> {
            Ok(self.active.borrow().clone())
        }

        fn admit_write(&self) -> Result<impl Send, String> {
            Ok(())
        }

        async fn load(&self, vault_id: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.stored(vault_id))
        }

        async fn store(&self, vault_id: &str, pkcs8: Vec<u8>) -> Result<(), String> {
            self.stored.borrow_mut().insert(vault_id.to_string(), pkcs8);
            Ok(())
        }
    }

    fn released(public_key: &PersonPublicKey, pkcs8: &[u8]) -> ReleasedKey {
        ReleasedKey {
            public_key: public_key.to_text(),
            private_key_pkcs8: URL_SAFE_NO_PAD.encode(pkcs8),
        }
    }

    #[test]
    fn a_vault_switch_during_the_key_fetch_stores_the_key_nowhere() {
        let (pkcs8, key) = PersonKeyPair::generate().unwrap();
        let row = row_for(&key.public_key());
        let copies = FakeCopies::new("vault-fetch-a");

        let present = tauri::async_runtime::block_on(release_person_key(
            &copies,
            "vault-fetch-a",
            &row,
            || async {
                *copies.active.borrow_mut() = "vault-fetch-b".to_string();
                Some(released(&key.public_key(), &pkcs8))
            },
        ));

        assert!(!present);
        assert_eq!(copies.stored("vault-fetch-a"), None);
        assert_eq!(copies.stored("vault-fetch-b"), None);
        assert!(!is_confirmed("vault-fetch-a", &row.public_key));
        assert!(!is_confirmed("vault-fetch-b", &row.public_key));
    }

    #[test]
    fn a_stored_key_of_another_identity_is_replaced_before_it_is_confirmed() {
        let (pkcs8, key) = PersonKeyPair::generate().unwrap();
        let (other_pkcs8, _) = PersonKeyPair::generate().unwrap();
        let row = row_for(&key.public_key());
        let copies = FakeCopies::new("vault-replace");
        copies
            .stored
            .borrow_mut()
            .insert("vault-replace".to_string(), other_pkcs8);

        let present = tauri::async_runtime::block_on(release_person_key(
            &copies,
            "vault-replace",
            &row,
            || async { Some(released(&key.public_key(), &pkcs8)) },
        ));

        assert!(present);
        assert_eq!(copies.stored("vault-replace"), Some(pkcs8));
        assert!(is_confirmed("vault-replace", &row.public_key));
        assert!(!is_confirmed("vault-replace", "another public key"));
    }

    #[test]
    fn a_released_key_that_does_not_match_the_row_is_refused() {
        let (_, key) = PersonKeyPair::generate().unwrap();
        let (other_pkcs8, _) = PersonKeyPair::generate().unwrap();
        let row = row_for(&key.public_key());
        let copies = FakeCopies::new("vault-mismatch");

        let present = tauri::async_runtime::block_on(release_person_key(
            &copies,
            "vault-mismatch",
            &row,
            || async { Some(released(&key.public_key(), &other_pkcs8)) },
        ));

        assert!(!present);
        assert_eq!(copies.stored("vault-mismatch"), None);
        assert!(!is_confirmed("vault-mismatch", &row.public_key));
    }
}
