//! Desktop operating-system keyring storage for the device linking TLS key.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_handoff::pairing::IdentityKeyStore;

const KEYRING_SERVICE: &str = "com.ganbaru.handoff";
const KEY_REFERENCE_PREFIX: &str = "device-key:";

/// Keeps each device's PKCS#8 linking key under its device ID in the system keyring.
pub(crate) struct KeyringIdentityKeyStore;

fn keyring_entry(device_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        KEYRING_SERVICE,
        &format!("{KEY_REFERENCE_PREFIX}{device_id}"),
    )
    .map_err(|error| format!("open device key store: {error}"))
}

impl IdentityKeyStore for KeyringIdentityKeyStore {
    fn read(&self, device_id: &str) -> Result<Option<Vec<u8>>, String> {
        match keyring_entry(device_id)?.get_password() {
            Ok(encoded) => URL_SAFE_NO_PAD
                .decode(encoded)
                .map(Some)
                .map_err(|error| format!("decode stored device key: {error}")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("read device key: {error}")),
        }
    }

    fn write(&self, device_id: &str, pkcs8: &[u8]) -> Result<(), String> {
        keyring_entry(device_id)?
            .set_password(&URL_SAFE_NO_PAD.encode(pkcs8))
            .map_err(|error| format!("store device key: {error}"))
    }

    fn remove(&self, device_id: &str) -> Result<(), String> {
        match keyring_entry(device_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("remove device key: {error}")),
        }
    }
}
