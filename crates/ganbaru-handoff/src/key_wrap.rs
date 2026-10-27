//! Device-bound protection of private key files. On Android every private key a device keeps
//! in its app config directory (sync device, writer, admin, and recovery keys, the person key,
//! and the linking TLS key) is sealed with an AES-256-GCM key that the platform key store
//! holds and never exports. A file then holds only the envelope this module writes, so a copy
//! of the app data opens on no other device.
//!
//! The platform key lives behind [`KeyWrapper`]; this module owns the envelope format, its
//! bounds, the binding of each envelope to its key reference, and the rule that a file which
//! is not a valid envelope (a plaintext key from an older build, a damaged file, or one whose
//! platform key is gone) can never be opened again: the device unlinks and links anew.

use ganbaru_atomic_files::{FinalSymlink, read_regular_file_capped};
use std::path::{Path, PathBuf};

/// Prefix of the error returned when a protected key can never be opened again, because its
/// platform key is gone or invalidated, or the file is not a valid envelope. The device unlinks
/// and the next link creates fresh keys.
pub const KEY_PROTECTION_LOST_ERROR: &str =
    "this device's protected keys can no longer be opened; link this device again";
/// Prefix of the error returned while the platform key store cannot be reached. Nothing is
/// lost; a later attempt may succeed.
pub const KEY_PROTECTION_UNAVAILABLE_ERROR: &str = "this device's key protection is unavailable";

/// Whether `error` reports keys that can never be opened again.
pub fn is_key_protection_lost(error: &str) -> bool {
    error.contains(KEY_PROTECTION_LOST_ERROR)
}

/// Extension of every private key file in the protected directories.
pub const KEY_FILE_EXTENSION: &str = "key";
/// Largest private key a file protects. Matches the pairing certificate bound.
pub const MAX_KEY_BYTES: usize = 16 * 1024;
/// Nonce length the wrapper prefixes to its ciphertext.
pub const NONCE_BYTES: usize = 12;
/// Authentication tag length the wrapper appends to its ciphertext.
pub const TAG_BYTES: usize = 16;

const MAGIC: [u8; 4] = *b"GKWR";
const VERSION: u8 = 1;
const HEADER_BYTES: usize = MAGIC.len() + 1;
const MIN_ENVELOPE_BYTES: usize = HEADER_BYTES + NONCE_BYTES + 1 + TAG_BYTES;
/// Largest envelope file read.
pub const MAX_ENVELOPE_BYTES: usize = HEADER_BYTES + NONCE_BYTES + MAX_KEY_BYTES + TAG_BYTES;

/// Why the platform key could not seal or open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WrapError {
    /// The key store could not be reached or refused this attempt; a retry may succeed.
    Unavailable(String),
    /// The platform key is gone, invalidated, or the ciphertext does not authenticate, so what
    /// it sealed can never be opened.
    Invalidated(String),
}

/// A platform key that seals private keys. Implementations generate the key under `alias` on
/// first use, never export it, and return `nonce || ciphertext || tag` from [`Self::wrap`].
/// Calls may block on platform services.
pub trait KeyWrapper: Send + Sync {
    /// Seals `plaintext` bound to `aad` under the key named `alias`, creating the key when it
    /// does not exist.
    fn wrap(&self, alias: &str, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, WrapError>;
    /// Opens what [`Self::wrap`] sealed with the same `alias` and `aad`. A missing key is
    /// [`WrapError::Invalidated`].
    fn unwrap(&self, alias: &str, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, WrapError>;
    /// Deletes the key named `alias`, so nothing it sealed opens again. A missing key is not an
    /// error.
    fn forget(&self, alias: &str) -> Result<(), WrapError>;
    /// Told by [`open`] when something sealed with `alias` was found that can never be opened.
    /// It is the only signal of a lost key: error text can carry remote messages, so callers
    /// never infer loss from it. The default ignores it.
    fn note_lost(&self, _alias: &str) {}
    /// Told by [`seal`] and [`open`] when `alias` sealed or opened a key, and by a key store
    /// that removed the only file `alias` sealed: an earlier [`Self::note_lost`] for `alias`
    /// no longer stands, because its key works again or nothing lost under it remains. The
    /// default ignores it.
    fn note_recovered(&self, _alias: &str) {}
}

/// Name of the platform key that seals the sync and person keys of `vault_id`.
pub fn vault_keys_alias(vault_id: &str) -> String {
    format!("ganbaru-sync-keys:{vault_id}")
}

/// Name of the platform key that seals the linking TLS key of `device_id`. It is per device
/// because the linking identity outlives any one vault.
pub fn linking_key_alias(device_id: &str) -> String {
    format!("ganbaru-linking-key:{device_id}")
}

/// Associated data of an envelope: the format header and the key reference, so an envelope
/// copied to another reference fails to open.
fn associated_data(reference: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(HEADER_BYTES + reference.len());
    aad.extend_from_slice(&MAGIC);
    aad.push(VERSION);
    aad.extend_from_slice(reference.as_bytes());
    aad
}

fn unavailable(detail: impl std::fmt::Display) -> String {
    format!("{KEY_PROTECTION_UNAVAILABLE_ERROR}: {detail}")
}

fn lost(detail: impl std::fmt::Display) -> String {
    format!("{KEY_PROTECTION_LOST_ERROR}: {detail}")
}

fn wrap_error(error: WrapError) -> String {
    match error {
        WrapError::Unavailable(detail) => unavailable(detail),
        WrapError::Invalidated(detail) => lost(detail),
    }
}

/// Whether `bytes` start like an envelope of any version. A DER key starts with `0x30`, so it
/// never matches.
pub fn is_envelope(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

/// Seals `plaintext`, the key stored under `reference`, with the platform key `alias`.
pub fn seal(
    wrapper: &dyn KeyWrapper,
    alias: &str,
    reference: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, String> {
    if plaintext.is_empty() || plaintext.len() > MAX_KEY_BYTES {
        return Err(format!(
            "a protected key must hold 1 to {MAX_KEY_BYTES} bytes"
        ));
    }
    let sealed = wrapper
        .wrap(alias, &associated_data(reference), plaintext)
        .map_err(wrap_error)?;
    if sealed.len() != NONCE_BYTES + plaintext.len() + TAG_BYTES {
        return Err(unavailable(
            "the key store returned a sealed key of the wrong length",
        ));
    }
    let mut envelope = Vec::with_capacity(HEADER_BYTES + sealed.len());
    envelope.extend_from_slice(&MAGIC);
    envelope.push(VERSION);
    envelope.extend_from_slice(&sealed);
    wrapper.note_recovered(alias);
    Ok(envelope)
}

/// Opens an envelope [`seal`] wrote for `reference` with the platform key `alias`. Anything but
/// a current, well-formed envelope that authenticates is [`KEY_PROTECTION_LOST_ERROR`].
pub fn open(
    wrapper: &dyn KeyWrapper,
    alias: &str,
    reference: &str,
    envelope: &[u8],
) -> Result<Vec<u8>, String> {
    let lost_here = |detail: &str| {
        wrapper.note_lost(alias);
        lost(detail)
    };
    if !is_envelope(envelope) {
        return Err(lost_here("the key file is not protected by this device"));
    }
    if !(MIN_ENVELOPE_BYTES..=MAX_ENVELOPE_BYTES).contains(&envelope.len()) {
        return Err(lost_here("the key file has an invalid length"));
    }
    if envelope[MAGIC.len()] != VERSION {
        return Err(lost_here("the key file has an unknown protection version"));
    }
    let sealed = &envelope[HEADER_BYTES..];
    let plaintext = wrapper
        .unwrap(alias, &associated_data(reference), sealed)
        .map_err(|error| {
            if matches!(error, WrapError::Invalidated(_)) {
                wrapper.note_lost(alias);
            }
            wrap_error(error)
        })?;
    if plaintext.len() != sealed.len() - NONCE_BYTES - TAG_BYTES {
        let mut plaintext = plaintext;
        wipe(&mut plaintext);
        return Err(unavailable(
            "the key store returned a key of the wrong length",
        ));
    }
    wrapper.note_recovered(alias);
    Ok(plaintext)
}

/// Reads and opens the protected key file at `path`, or `None` when it does not exist.
pub fn read_protected_file(
    wrapper: &dyn KeyWrapper,
    alias: &str,
    reference: &str,
    path: &Path,
) -> Result<Option<Vec<u8>>, String> {
    let envelope =
        match read_regular_file_capped(path, MAX_ENVELOPE_BYTES as u64, FinalSymlink::Refuse) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
                wrapper.note_lost(alias);
                return Err(lost("the key file has an invalid length"));
            }
            Err(error) => return Err(format!("read protected key: {error}")),
        };
    open(wrapper, alias, reference, &envelope).map(Some)
}

/// Seals `plaintext` and replaces the file at `path` with the envelope atomically, readable by
/// the owner only.
pub fn write_protected_file(
    wrapper: &dyn KeyWrapper,
    alias: &str,
    reference: &str,
    path: &Path,
    plaintext: &[u8],
) -> Result<(), String> {
    let envelope = seal(wrapper, alias, reference, plaintext)?;
    crate::pairing::write_private_file_atomically(path, &envelope)
}

/// Removes the file at `path`; a missing file is not an error.
pub fn remove_key_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove protected key: {error}")),
    }
}

/// Deletes every key file directly in `directory`, sealed or not, and returns how many it
/// removed. A missing directory holds nothing.
pub fn remove_key_files(directory: &Path) -> Result<usize, String> {
    remove_key_files_where(directory, |_| true)
}

/// Deletes the key files directly in `directory` whose file stem `matches` accepts, and returns
/// how many it removed. A missing directory holds nothing.
pub fn remove_key_files_where(
    directory: &Path,
    matches: impl Fn(&str) -> bool,
) -> Result<usize, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("list protected keys: {error}")),
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|error| format!("list protected keys: {error}"))?;
        let path = entry.path();
        let is_directory = entry
            .file_type()
            .map_err(|error| format!("list protected keys: {error}"))?
            .is_dir();
        if is_directory
            || path.extension().and_then(|extension| extension.to_str()) != Some(KEY_FILE_EXTENSION)
            || !path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(&matches)
        {
            continue;
        }
        remove_key_file(&path)?;
        removed += 1;
    }
    Ok(removed)
}

/// Deletes every key file directly in `directory` that is not an envelope and returns the
/// paths it removed; see [`unprotected_key_files`].
pub fn remove_unprotected_key_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let removed = unprotected_key_files(directory)?;
    for path in &removed {
        remove_key_file(path)?;
    }
    Ok(removed)
}

/// The key files directly in `directory` that are not envelopes, such as a plaintext key an
/// older build wrote, without changing them. Envelopes are left out, since telling a current
/// one from a lost one needs the platform key. A missing directory holds nothing.
pub fn unprotected_key_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("list protected keys: {error}")),
    };
    let mut removed = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("list protected keys: {error}"))?
            .path();
        if path.extension().and_then(|extension| extension.to_str()) != Some(KEY_FILE_EXTENSION) {
            continue;
        }
        let protected = match read_regular_file_capped(
            &path,
            MAX_ENVELOPE_BYTES as u64,
            FinalSymlink::Refuse,
        ) {
            Ok(mut bytes) => {
                let protected = is_envelope(&bytes);
                wipe(&mut bytes);
                protected
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            // A symlink, a directory, or an oversized file named like a key is not an envelope.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::InvalidInput | std::io::ErrorKind::FileTooLarge
                ) =>
            {
                false
            }
            Err(error) => return Err(format!("read protected key: {error}")),
        };
        if !protected && !path.is_dir() {
            removed.push(path);
        }
    }
    Ok(removed)
}

/// Linking TLS key store of one protected file per device ID in `directory`, sealed with the
/// device's [`linking_key_alias`].
pub struct ProtectedIdentityKeyStore {
    directory: PathBuf,
    wrapper: std::sync::Arc<dyn KeyWrapper>,
}

impl ProtectedIdentityKeyStore {
    pub fn new(directory: PathBuf, wrapper: std::sync::Arc<dyn KeyWrapper>) -> Self {
        Self { directory, wrapper }
    }

    /// The protected file of `device_id`.
    pub fn path(&self, device_id: &str) -> PathBuf {
        self.directory.join(format!(
            "{}.{KEY_FILE_EXTENSION}",
            identity_reference(device_id).replace(':', "-")
        ))
    }
}

fn identity_reference(device_id: &str) -> String {
    format!("linking-key:{device_id}")
}

impl crate::pairing::IdentityKeyStore for ProtectedIdentityKeyStore {
    fn read(&self, device_id: &str) -> Result<Option<Vec<u8>>, String> {
        read_protected_file(
            self.wrapper.as_ref(),
            &linking_key_alias(device_id),
            &identity_reference(device_id),
            &self.path(device_id),
        )
    }

    fn write(&self, device_id: &str, pkcs8: &[u8]) -> Result<(), String> {
        write_protected_file(
            self.wrapper.as_ref(),
            &linking_key_alias(device_id),
            &identity_reference(device_id),
            &self.path(device_id),
            pkcs8,
        )
    }

    /// Removes the protected file of `device_id`, the only file its linking alias seals, so a
    /// loss noted for that alias no longer stands.
    fn remove(&self, device_id: &str) -> Result<(), String> {
        remove_key_file(&self.path(device_id))?;
        self.wrapper.note_recovered(&linking_key_alias(device_id));
        Ok(())
    }
}

/// Overwrites `bytes` with zeros before they are dropped. Best effort: no crate here offers a
/// guaranteed wipe, and copies the allocator or the platform made earlier are out of reach.
pub fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    std::hint::black_box(bytes);
}

/// In-memory platform keys for tests: real AES-256-GCM keys that `forget` deletes, with an
/// injectable outage.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct TestKeyWrapper {
    keys: std::sync::Mutex<std::collections::HashMap<String, [u8; 32]>>,
    unavailable: std::sync::atomic::AtomicBool,
    noted_lost: std::sync::Mutex<Vec<String>>,
    noted_recovered: std::sync::Mutex<Vec<String>>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestKeyWrapper {
    /// Makes every later call fail with [`WrapError::Unavailable`] while `unavailable` holds.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable
            .store(unavailable, std::sync::atomic::Ordering::SeqCst);
    }

    /// The aliases [`KeyWrapper::note_lost`] reported, oldest first.
    pub fn noted_lost(&self) -> Vec<String> {
        self.noted_lost
            .lock()
            .map(|noted| noted.clone())
            .unwrap_or_default()
    }

    /// The aliases [`KeyWrapper::note_recovered`] reported, oldest first.
    pub fn noted_recovered(&self) -> Vec<String> {
        self.noted_recovered
            .lock()
            .map(|noted| noted.clone())
            .unwrap_or_default()
    }

    /// Whether a key named `alias` exists.
    pub fn has_key(&self, alias: &str) -> bool {
        self.keys
            .lock()
            .map(|keys| keys.contains_key(alias))
            .unwrap_or(false)
    }

    fn check_available(&self) -> Result<(), WrapError> {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(WrapError::Unavailable(
                "test key store is offline".to_string(),
            ));
        }
        Ok(())
    }

    fn key(&self, alias: &str, create: bool) -> Result<Option<ring::aead::LessSafeKey>, WrapError> {
        use ring::rand::SecureRandom;
        let mut keys = self
            .keys
            .lock()
            .map_err(|_| WrapError::Unavailable("test key store is poisoned".to_string()))?;
        if !keys.contains_key(alias) {
            if !create {
                return Ok(None);
            }
            let mut bytes = [0_u8; 32];
            ring::rand::SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| WrapError::Unavailable("test key generation failed".to_string()))?;
            keys.insert(alias.to_string(), bytes);
        }
        let bytes = keys[alias];
        let key = ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &bytes)
            .map_err(|_| WrapError::Unavailable("test key is invalid".to_string()))?;
        Ok(Some(ring::aead::LessSafeKey::new(key)))
    }
}

#[cfg(any(test, feature = "test-support"))]
impl KeyWrapper for TestKeyWrapper {
    fn wrap(&self, alias: &str, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, WrapError> {
        use ring::rand::SecureRandom;
        self.check_available()?;
        let key = self
            .key(alias, true)?
            .ok_or_else(|| WrapError::Unavailable("test key is missing".to_string()))?;
        let mut nonce = [0_u8; NONCE_BYTES];
        ring::rand::SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| WrapError::Unavailable("test nonce generation failed".to_string()))?;
        let mut sealed = plaintext.to_vec();
        key.seal_in_place_append_tag(
            ring::aead::Nonce::assume_unique_for_key(nonce),
            ring::aead::Aad::from(aad),
            &mut sealed,
        )
        .map_err(|_| WrapError::Unavailable("test seal failed".to_string()))?;
        let mut output = nonce.to_vec();
        output.extend_from_slice(&sealed);
        Ok(output)
    }

    fn unwrap(&self, alias: &str, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, WrapError> {
        self.check_available()?;
        let key = self
            .key(alias, false)?
            .ok_or_else(|| WrapError::Invalidated("the platform key is gone".to_string()))?;
        if sealed.len() < NONCE_BYTES + TAG_BYTES {
            return Err(WrapError::Invalidated(
                "the sealed key is truncated".to_string(),
            ));
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_BYTES);
        let nonce = ring::aead::Nonce::try_assume_unique_for_key(nonce)
            .map_err(|_| WrapError::Invalidated("the nonce is invalid".to_string()))?;
        let mut buffer = ciphertext.to_vec();
        let length = key
            .open_in_place(nonce, ring::aead::Aad::from(aad), &mut buffer)
            .map_err(|_| {
                WrapError::Invalidated("the sealed key does not authenticate".to_string())
            })?
            .len();
        buffer.truncate(length);
        Ok(buffer)
    }

    fn forget(&self, alias: &str) -> Result<(), WrapError> {
        self.check_available()?;
        self.keys
            .lock()
            .map_err(|_| WrapError::Unavailable("test key store is poisoned".to_string()))?
            .remove(alias);
        Ok(())
    }

    fn note_lost(&self, alias: &str) {
        if let Ok(mut noted) = self.noted_lost.lock() {
            noted.push(alias.to_string());
        }
    }

    fn note_recovered(&self, alias: &str) {
        if let Ok(mut noted) = self.noted_recovered.lock() {
            noted.push(alias.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALIAS: &str = "ganbaru-sync-keys:vault-1";
    const REFERENCE: &str = "device-key:vault-1";

    fn directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-key-wrap-{name}-{}",
            crate::pairing::random_token("test").expect("token")
        ));
        std::fs::create_dir_all(&path).expect("create directory");
        path
    }

    #[test]
    fn a_sealed_key_opens_only_with_its_alias_and_reference() {
        let wrapper = TestKeyWrapper::default();
        let envelope = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect("seal");
        assert!(is_envelope(&envelope));
        assert!(!envelope.windows(10).any(|window| window == b"secret key"));
        assert_eq!(
            open(&wrapper, ALIAS, REFERENCE, &envelope).expect("open"),
            b"secret key"
        );
        let moved = open(&wrapper, ALIAS, "admin-key:vault-1", &envelope).expect_err("moved");
        assert!(is_key_protection_lost(&moved));
        let other_alias =
            open(&wrapper, "ganbaru-sync-keys:vault-2", REFERENCE, &envelope).expect_err("alias");
        assert!(is_key_protection_lost(&other_alias));
    }

    #[test]
    fn plaintext_damaged_and_unknown_envelopes_are_lost_keys() {
        let wrapper = TestKeyWrapper::default();
        let mut envelope = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect("seal");
        let plaintext =
            open(&wrapper, ALIAS, REFERENCE, &[0x30, 0x51, 0x02, 0x01]).expect_err("der");
        assert!(is_key_protection_lost(&plaintext));
        let truncated = open(
            &wrapper,
            ALIAS,
            REFERENCE,
            &envelope[..MIN_ENVELOPE_BYTES - 1],
        )
        .expect_err("short");
        assert!(is_key_protection_lost(&truncated));
        let last = envelope.len() - 1;
        envelope[last] ^= 1;
        let tampered = open(&wrapper, ALIAS, REFERENCE, &envelope).expect_err("tampered");
        assert!(is_key_protection_lost(&tampered));
        envelope[last] ^= 1;
        envelope[MAGIC.len()] = VERSION + 1;
        let future = open(&wrapper, ALIAS, REFERENCE, &envelope).expect_err("version");
        assert!(is_key_protection_lost(&future));
    }

    #[test]
    fn every_truncated_envelope_is_a_lost_key_without_panicking() {
        let wrapper = TestKeyWrapper::default();
        let envelope = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect("seal");
        for length in 0..MIN_ENVELOPE_BYTES {
            let error = open(&wrapper, ALIAS, REFERENCE, &envelope[..length])
                .expect_err("truncated envelope");
            assert!(is_key_protection_lost(&error), "length {length}");
        }
        let header_only = open(&wrapper, ALIAS, REFERENCE, &MAGIC).expect_err("magic only");
        assert!(is_key_protection_lost(&header_only));
        assert_eq!(wrapper.noted_lost().len(), MIN_ENVELOPE_BYTES + 1);
    }

    #[test]
    fn an_unreachable_key_store_reports_no_loss() {
        let wrapper = TestKeyWrapper::default();
        let envelope = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect("seal");
        wrapper.set_unavailable(true);
        open(&wrapper, ALIAS, REFERENCE, &envelope).expect_err("unavailable");
        assert!(wrapper.noted_lost().is_empty());
        wrapper.set_unavailable(false);
        wrapper.forget(ALIAS).expect("forget");
        open(&wrapper, ALIAS, REFERENCE, &envelope).expect_err("forgotten");
        assert_eq!(wrapper.noted_lost(), vec![ALIAS.to_string()]);
    }

    #[test]
    fn a_working_key_or_a_removed_linking_file_reports_recovery_and_failures_do_not() {
        let wrapper = TestKeyWrapper::default();
        let envelope = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect("seal");
        open(&wrapper, ALIAS, REFERENCE, &envelope).expect("open");
        assert_eq!(wrapper.noted_recovered(), vec![ALIAS, ALIAS]);
        wrapper.set_unavailable(true);
        open(&wrapper, ALIAS, REFERENCE, &envelope).expect_err("offline");
        seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect_err("offline");
        wrapper.set_unavailable(false);
        open(&wrapper, ALIAS, REFERENCE, b"not an envelope").expect_err("raw");
        assert_eq!(wrapper.noted_recovered().len(), 2);

        let shared = std::sync::Arc::new(TestKeyWrapper::default());
        let store = ProtectedIdentityKeyStore::new(directory("recovered"), shared.clone());
        crate::pairing::IdentityKeyStore::remove(&store, "device-phone").expect("remove");
        assert_eq!(
            shared.noted_recovered(),
            vec![linking_key_alias("device-phone")]
        );
        let _ = std::fs::remove_dir_all(store.directory.clone());
    }

    #[test]
    fn a_forgotten_platform_key_loses_what_it_sealed() {
        let wrapper = TestKeyWrapper::default();
        let envelope = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect("seal");
        wrapper.forget(ALIAS).expect("forget");
        assert!(!wrapper.has_key(ALIAS));
        let error = open(&wrapper, ALIAS, REFERENCE, &envelope).expect_err("forgotten");
        assert!(is_key_protection_lost(&error));
        wrapper.forget(ALIAS).expect("forgetting twice");
    }

    #[test]
    fn an_unreachable_key_store_is_transient() {
        let wrapper = TestKeyWrapper::default();
        let envelope = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect("seal");
        wrapper.set_unavailable(true);
        let read = open(&wrapper, ALIAS, REFERENCE, &envelope).expect_err("offline");
        assert!(read.starts_with(KEY_PROTECTION_UNAVAILABLE_ERROR));
        assert!(!is_key_protection_lost(&read));
        let write = seal(&wrapper, ALIAS, REFERENCE, b"secret key").expect_err("offline");
        assert!(!is_key_protection_lost(&write));
        wrapper.set_unavailable(false);
        assert_eq!(
            open(&wrapper, ALIAS, REFERENCE, &envelope).expect("back online"),
            b"secret key"
        );
    }

    #[test]
    fn seal_refuses_empty_and_oversized_keys() {
        let wrapper = TestKeyWrapper::default();
        assert!(seal(&wrapper, ALIAS, REFERENCE, b"").is_err());
        assert!(seal(&wrapper, ALIAS, REFERENCE, &vec![1; MAX_KEY_BYTES + 1]).is_err());
        let largest = seal(&wrapper, ALIAS, REFERENCE, &vec![1; MAX_KEY_BYTES]).expect("largest");
        assert_eq!(largest.len(), MAX_ENVELOPE_BYTES);
    }

    #[test]
    fn a_wrapper_returning_the_wrong_length_is_refused() {
        struct Short;
        impl KeyWrapper for Short {
            fn wrap(&self, _: &str, _: &[u8], _: &[u8]) -> Result<Vec<u8>, WrapError> {
                Ok(vec![0; NONCE_BYTES + TAG_BYTES])
            }
            fn unwrap(&self, _: &str, _: &[u8], _: &[u8]) -> Result<Vec<u8>, WrapError> {
                Ok(Vec::new())
            }
            fn forget(&self, _: &str) -> Result<(), WrapError> {
                Ok(())
            }
        }
        let wrapped = seal(&Short, ALIAS, REFERENCE, b"secret").expect_err("short seal");
        assert!(!is_key_protection_lost(&wrapped));
        let envelope = seal(&TestKeyWrapper::default(), ALIAS, REFERENCE, b"secret").expect("seal");
        let opened = open(&Short, ALIAS, REFERENCE, &envelope).expect_err("short open");
        assert!(!is_key_protection_lost(&opened));
    }

    #[test]
    fn protected_files_round_trip_and_never_hold_the_plaintext() {
        let directory = directory("files");
        let path = directory.join("device-key-vault-1.key");
        let wrapper = TestKeyWrapper::default();
        assert_eq!(
            read_protected_file(&wrapper, ALIAS, REFERENCE, &path).expect("missing"),
            None
        );
        write_protected_file(&wrapper, ALIAS, REFERENCE, &path, b"secret key").expect("write");
        let stored = std::fs::read(&path).expect("stored");
        assert!(is_envelope(&stored));
        assert!(!stored.windows(10).any(|window| window == b"secret key"));
        assert_eq!(
            read_protected_file(&wrapper, ALIAS, REFERENCE, &path).expect("read"),
            Some(b"secret key".to_vec())
        );
        remove_key_file(&path).expect("remove");
        remove_key_file(&path).expect("remove twice");
        std::fs::write(&path, vec![0x47; MAX_ENVELOPE_BYTES + 1]).expect("oversized");
        let oversized = read_protected_file(&wrapper, ALIAS, REFERENCE, &path).expect_err("big");
        assert!(is_key_protection_lost(&oversized));
        std::fs::remove_dir_all(directory).expect("clean up");
    }

    #[test]
    fn a_filtered_removal_keeps_key_files_it_does_not_match() {
        let directory = directory("filtered");
        std::fs::create_dir_all(&directory).expect("directory");
        let ours = directory.join("device-key-vault-1.key");
        let theirs = directory.join("device-key-vault-2.key");
        let record = directory.join("writer-vault-1.json");
        for path in [&ours, &theirs, &record] {
            std::fs::write(path, b"x").expect("write");
        }
        let removed =
            remove_key_files_where(&directory, |stem| stem.ends_with("-vault-1")).expect("remove");
        assert_eq!(removed, 1);
        assert!(!ours.exists());
        assert!(theirs.exists() && record.exists());
        std::fs::remove_dir_all(directory).expect("clean up");
    }

    #[test]
    fn the_startup_scan_removes_only_unprotected_key_files() {
        let directory = directory("scan");
        let wrapper = TestKeyWrapper::default();
        let protected = directory.join("device-key-vault-1.key");
        write_protected_file(&wrapper, ALIAS, REFERENCE, &protected, b"secret").expect("write");
        let plaintext = directory.join("writer-key-vault-1-abc.key");
        std::fs::write(&plaintext, [0x30, 0x51, 0x02]).expect("plaintext");
        let empty = directory.join("admin-key-vault-1.key");
        std::fs::write(&empty, []).expect("empty");
        let record = directory.join("writer-vault-1.json");
        std::fs::write(&record, b"{}").expect("record");
        let mut found = unprotected_key_files(&directory).expect("look");
        found.sort();
        assert!(plaintext.exists() && empty.exists());
        let mut removed = remove_unprotected_key_files(&directory).expect("scan");
        assert_eq!(found, removed);
        removed.sort();
        let mut expected = vec![empty.clone(), plaintext.clone()];
        expected.sort();
        assert_eq!(removed, expected);
        assert!(protected.exists());
        assert!(record.exists());
        assert!(!plaintext.exists());
        assert!(
            remove_unprotected_key_files(&directory.join("missing"))
                .expect("missing directory")
                .is_empty()
        );
        assert_eq!(remove_key_files(&directory).expect("remove all"), 1);
        assert!(!protected.exists());
        assert!(record.exists());
        assert_eq!(
            remove_key_files(&directory.join("missing")).expect("missing"),
            0
        );
        std::fs::remove_dir_all(directory).expect("clean up");
    }

    #[test]
    fn the_identity_key_store_seals_the_linking_key_per_device() {
        use crate::pairing::IdentityKeyStore;
        let directory = directory("identity");
        let wrapper = std::sync::Arc::new(TestKeyWrapper::default());
        let store = ProtectedIdentityKeyStore::new(directory.clone(), wrapper.clone());
        assert_eq!(store.read("device-1").expect("missing"), None);
        store.write("device-1", b"tls key").expect("write");
        assert!(is_envelope(
            &std::fs::read(store.path("device-1")).expect("file")
        ));
        assert_eq!(
            store.read("device-1").expect("read"),
            Some(b"tls key".to_vec())
        );
        assert!(wrapper.has_key(&linking_key_alias("device-1")));
        std::fs::copy(store.path("device-1"), store.path("device-2")).expect("copy");
        let copied = store.read("device-2").expect_err("copied envelope");
        assert!(is_key_protection_lost(&copied));
        store.remove("device-1").expect("remove");
        assert_eq!(store.read("device-1").expect("removed"), None);
        std::fs::remove_dir_all(directory).expect("clean up");
    }

    #[test]
    fn wipe_zeroes_the_buffer() {
        let mut bytes = vec![7_u8; 32];
        wipe(&mut bytes);
        assert!(bytes.iter().all(|byte| *byte == 0));
    }
}
