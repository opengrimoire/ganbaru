//! Protection of the private key files a mobile device keeps in its app config directory: the
//! platform key that seals them (an Android Keystore key reached over JNI), the startup removal
//! of key files found unsealed, and the recovery once sealed keys can no longer be opened.
//! Desktop keeps the same keys in the operating-system keyring instead.
//!
//! Losing protected keys is never repaired in place. A linked device first unlinks with
//! [`UnlinkReason::KeyProtectionLost`]; then the key files sealed with the lost platform key and
//! that key itself go, and the keys are created again. An unlinked device replaces a lost
//! linking key with a fresh identity on its own.

use crate::vault::handoff::pairing::{PairingManager, UnlinkReason};
use ganbaru_handoff::key_wrap::WrapError;
use ganbaru_handoff::key_wrap::{self, KeyWrapper, ProtectedIdentityKeyStore};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Directory under the app config directory that holds the sealed linking TLS key.
const LINKING_KEY_DIRECTORY: &str = "linking";

/// Directories under the app config directory that hold sealed private key files: the sync
/// keys, the person key, and the linking key.
const KEY_DIRECTORIES: [&str; 3] = [
    super::host::SYNC_DIRECTORY,
    crate::contacts::identity::KEY_DIRECTORY,
    LINKING_KEY_DIRECTORY,
];

/// Directories under the app config directory that hold the key files a vault's platform key
/// seals: the sync keys and the person key.
const VAULT_KEY_DIRECTORIES: [&str; 2] = [
    super::host::SYNC_DIRECTORY,
    crate::contacts::identity::KEY_DIRECTORY,
];

/// Platform key alias the health probe seals with and forgets again.
const PROBE_ALIAS: &str = "ganbaru-key-probe";
/// Key reference the health probe binds its envelope to.
const PROBE_REFERENCE: &str = "key-probe";

/// The platform key wrapper of this build: the Android Keystore on Android, a shared in-memory
/// stand-in in tests, and none elsewhere, where every seal fails as unavailable.
#[cfg_attr(not(any(mobile, test)), allow(dead_code))]
pub(crate) fn platform_wrapper() -> Arc<dyn KeyWrapper> {
    #[cfg(test)]
    {
        test_wrapper()
    }
    #[cfg(all(not(test), target_os = "android"))]
    {
        Arc::new(android::AndroidKeystore)
    }
    #[cfg(not(any(test, target_os = "android")))]
    {
        Arc::new(NoPlatformKey)
    }
}

/// The platform key stand-in every test process shares, so stores opened again read what an
/// earlier one sealed.
#[cfg(test)]
pub(crate) fn test_wrapper() -> Arc<key_wrap::TestKeyWrapper> {
    static WRAPPER: std::sync::LazyLock<Arc<key_wrap::TestKeyWrapper>> =
        std::sync::LazyLock::new(|| Arc::new(key_wrap::TestKeyWrapper::default()));
    WRAPPER.clone()
}

/// The wrapper of builds without a platform key store; nothing it would seal is ever written.
#[cfg(not(any(test, target_os = "android")))]
#[cfg_attr(not(mobile), allow(dead_code))]
struct NoPlatformKey;

#[cfg(not(any(test, target_os = "android")))]
impl KeyWrapper for NoPlatformKey {
    fn wrap(&self, _alias: &str, _aad: &[u8], _plaintext: &[u8]) -> Result<Vec<u8>, WrapError> {
        Err(WrapError::Unavailable(
            "this platform has no key store for protected key files".to_string(),
        ))
    }

    fn unwrap(&self, _alias: &str, _aad: &[u8], _sealed: &[u8]) -> Result<Vec<u8>, WrapError> {
        Err(WrapError::Unavailable(
            "this platform has no key store for protected key files".to_string(),
        ))
    }

    fn forget(&self, _alias: &str) -> Result<(), WrapError> {
        Ok(())
    }
}

/// Where the linking TLS key of this device is sealed, inside the app config directory
/// `config_dir`.
#[cfg_attr(not(any(mobile, test)), allow(dead_code))]
pub(crate) fn identity_key_store(
    config_dir: &Path,
    wrapper: Arc<dyn KeyWrapper>,
) -> ProtectedIdentityKeyStore {
    ProtectedIdentityKeyStore::new(config_dir.join(LINKING_KEY_DIRECTORY), wrapper)
}

/// The platform key wrapper every key file of the process goes through. It records each key
/// reference [`key_wrap::open`] reports as lost, by alias, which is the only signal that starts
/// a recovery: error text can carry messages from a remote hub, so it never decides that keys
/// are lost. A reference leaves the record once its key seals or opens again or its file is
/// removed, so a loss already repaired (a replaced linking identity, a platform key the store
/// generated again) never unlinks a device later. An alias counts as lost while any of its
/// references does, so a healthy key under one alias never clears a damaged key under the same
/// alias. The health probe's alias is never recorded.
pub(crate) struct KeyProtection {
    inner: Arc<dyn KeyWrapper>,
    lost: Mutex<BTreeMap<String, BTreeSet<String>>>,
}

#[cfg_attr(not(any(mobile, test)), allow(dead_code))]
impl KeyProtection {
    pub(crate) fn new(inner: Arc<dyn KeyWrapper>) -> Self {
        Self {
            inner,
            lost: Mutex::default(),
        }
    }

    /// The aliases with a key reported lost since the last call, in alias order.
    pub(crate) fn take_lost(&self) -> Vec<String> {
        match self.lost.lock() {
            Ok(mut lost) => std::mem::take(&mut *lost).into_keys().collect(),
            Err(_) => Vec::new(),
        }
    }
}

impl KeyWrapper for KeyProtection {
    fn wrap(&self, alias: &str, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, WrapError> {
        self.inner.wrap(alias, aad, plaintext)
    }

    fn unwrap(&self, alias: &str, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, WrapError> {
        self.inner.unwrap(alias, aad, sealed)
    }

    fn forget(&self, alias: &str) -> Result<(), WrapError> {
        self.inner.forget(alias)
    }

    fn note_lost(&self, alias: &str, reference: &str) {
        self.inner.note_lost(alias, reference);
        if alias == PROBE_ALIAS {
            return;
        }
        if let Ok(mut lost) = self.lost.lock() {
            lost.entry(alias.to_string())
                .or_default()
                .insert(reference.to_string());
        }
    }

    fn note_recovered(&self, alias: &str, reference: &str) {
        self.inner.note_recovered(alias, reference);
        if let Ok(mut lost) = self.lost.lock()
            && let Some(references) = lost.get_mut(alias)
        {
            references.remove(reference);
            if references.is_empty() {
                lost.remove(alias);
            }
        }
    }
}

/// Deletes every private key file in the app config directory `config_dir` that is not sealed,
/// such as one an older build wrote in plaintext. A linked device unlinks durably first, because
/// a key that sat in plaintext may have been copied; if the unlink fails the files stay, so the
/// next start unlinks and deletes again. Failures are logged.
#[cfg_attr(not(any(mobile, test)), allow(dead_code))]
pub(crate) fn remove_unprotected_keys(config_dir: &Path, pairing: &PairingManager) {
    let mut found = Vec::new();
    for directory in KEY_DIRECTORIES {
        match key_wrap::unprotected_key_files(&config_dir.join(directory)) {
            Ok(paths) => found.extend(paths),
            Err(error) => eprintln!("key protection: {error}"),
        }
    }
    if found.is_empty() {
        return;
    }
    match pairing.has_links() {
        Ok(false) => {}
        Ok(true) => {
            if let Err(error) = pairing.unlink_for(UnlinkReason::KeyProtectionLost) {
                eprintln!("key protection: unlink before removing unsealed keys: {error}");
                return;
            }
            eprintln!("key protection: unlinked because private key files were not sealed");
        }
        Err(error) => {
            eprintln!("key protection: {error}");
            return;
        }
    }
    let linking = config_dir.join(LINKING_KEY_DIRECTORY);
    let mut removed = 0_usize;
    let mut linking_key_removed = false;
    for path in found {
        match key_wrap::remove_key_file(&path) {
            Ok(()) => {
                removed += 1;
                linking_key_removed |= path.parent() == Some(linking.as_path());
            }
            Err(error) => eprintln!("key protection: {error}"),
        }
    }
    eprintln!("key protection: removed {removed} private key files that were not sealed");
    if linking_key_removed && let Err(error) = pairing.forget_cached_identity_key() {
        eprintln!("key protection: {error}");
    }
}

/// Whether the platform key store seals and opens again right now, checked with a throwaway
/// key. A store that fails the probe is unavailable, which is no proof of a lost key.
fn key_store_is_healthy(wrapper: &dyn KeyWrapper) -> bool {
    let probe = b"key protection probe";
    let healthy = key_wrap::seal(wrapper, PROBE_ALIAS, PROBE_REFERENCE, probe)
        .and_then(|envelope| key_wrap::open(wrapper, PROBE_ALIAS, PROBE_REFERENCE, &envelope))
        .is_ok_and(|opened| opened == probe);
    if let Err(error) = wrapper.forget(PROBE_ALIAS) {
        eprintln!("key protection: forget the probe key: {error:?}");
    }
    healthy
}

/// Whether the key file stem `stem` holds a key of `vault_id`, sealed with its vault alias. A
/// writer key stem is `writer-key-{vault}-{writer}` with a hexadecimal writer ID, so the part
/// after the vault ID must be hexadecimal alone: the key of a vault whose ID extends `vault_id`
/// with `-...` never matches. A vault ID the identifier rules refuse matches nothing.
fn is_vault_key_file(stem: &str, vault_id: &str) -> bool {
    if ganbaru_handoff::protocol::validate_identifier("vault id", vault_id).is_err() {
        return false;
    }
    let is_writer_key = stem
        .strip_prefix(&format!("writer-key-{vault_id}-"))
        .is_some_and(|writer| {
            !writer.is_empty() && writer.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
    is_writer_key
        || ["device-key", "admin-key", "recovery-seed", "person-key"]
            .iter()
            .any(|kind| stem == format!("{kind}-{vault_id}"))
}

/// Whether a lost alias in `lost` held keys the device's links depend on: its own linking key,
/// or the keys of a vault it is linked for. The keys of a vault no link uses go without
/// unlinking.
fn loses_link(
    pairing: &PairingManager,
    lost: &[String],
    linking_alias: &str,
    vault_prefix: &str,
) -> Result<bool, String> {
    for alias in lost {
        if alias == linking_alias {
            return Ok(true);
        }
        if let Some(vault_id) = alias.strip_prefix(vault_prefix)
            && pairing.is_linked_for(vault_id)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Recovers from the platform key aliases in `lost`, whose sealed key files can no longer be
/// opened. Nothing happens unless a probe shows the key store working, so an unavailable store
/// is retried later instead of read as a loss. A linked device unlinks durably before any file
/// goes, but only when it lost its linking key or the keys of a vault it is linked for. Then
/// each lost vault alias loses that vault's key files and its platform key, and a
/// lost linking key of a device that was linked loses its file, cached copy, and platform key;
/// an unlinked device replaces its linking identity on the next read instead. Key files of other
/// vaults stay. An active handoff keeps the device linked and its keys in place until it
/// finishes; the next pass that fails to open a key tries again.
pub(crate) fn recover_after_key_loss(
    config_dir: &Path,
    pairing: &PairingManager,
    wrapper: &dyn KeyWrapper,
    lost: &[String],
) -> Result<(), String> {
    if lost.is_empty() {
        return Ok(());
    }
    if !key_store_is_healthy(wrapper) {
        return Err("the key store failed a health check; keys are kept for a later pass".into());
    }
    let linked = pairing.has_links()?;
    let device_id = pairing.device_id()?;
    let linking_alias = key_wrap::linking_key_alias(&device_id);
    let vault_prefix = key_wrap::vault_keys_alias("");
    if linked && loses_link(pairing, lost, &linking_alias, &vault_prefix)? {
        pairing
            .unlink_for(UnlinkReason::KeyProtectionLost)
            .map_err(|error| format!("unlink after losing protected keys: {error}"))?;
        eprintln!("key protection: unlinked because protected keys can no longer be opened");
    }
    for alias in lost {
        if *alias == linking_alias {
            if !linked {
                continue;
            }
            let stem = format!("linking-key-{device_id}");
            key_wrap::remove_key_files_where(&config_dir.join(LINKING_KEY_DIRECTORY), |name| {
                name == stem
            })?;
            pairing.forget_cached_identity_key()?;
        } else if let Some(vault_id) = alias.strip_prefix(&vault_prefix) {
            for directory in VAULT_KEY_DIRECTORIES {
                key_wrap::remove_key_files_where(&config_dir.join(directory), |name| {
                    is_vault_key_file(name, vault_id)
                })?;
            }
        } else {
            continue;
        }
        // The files it sealed are gone, so the platform key protects nothing.
        if let Err(error) = wrapper.forget(alias) {
            eprintln!("key protection: forget a platform key: {error:?}");
        }
    }
    Ok(())
}

/// The Android Keystore reached through the `SyncKeystore` Kotlin object the background sync
/// plugin registers. Registration happens once per process, from the plugin or the worker,
/// before Rust needs a key; until then every call is unavailable.
#[cfg(target_os = "android")]
pub(crate) mod android {
    use ganbaru_handoff::key_wrap::{self, KeyWrapper, WrapError};
    use std::sync::OnceLock;
    use tauri::wry::prelude::jni;

    /// Status byte the Kotlin bridge prefixes to each result.
    const STATUS_OK: u8 = 0;
    const STATUS_UNAVAILABLE: u8 = 1;
    const STATUS_INVALIDATED: u8 = 2;
    /// Longest diagnostic text the bridge returns with a failure status.
    const MAX_DETAIL_BYTES: usize = 256;
    /// Local references one call creates: the alias, two byte arrays, and the result.
    const LOCAL_FRAME_CAPACITY: i32 = 8;
    const WRAP_SIGNATURE: &str = "(Ljava/lang/String;[B[B)[B";
    const FORGET_SIGNATURE: &str = "(Ljava/lang/String;)[B";

    struct Bridge {
        vm: jni::JavaVM,
        keystore: jni::objects::GlobalRef,
    }

    static BRIDGE: OnceLock<Bridge> = OnceLock::new();

    /// Keeps the Java VM and a global reference to the Kotlin `SyncKeystore` object. A second
    /// registration keeps the first, since the object is a process singleton.
    pub(crate) fn register(
        env: &mut jni::JNIEnv<'_>,
        keystore: &jni::objects::JObject<'_>,
    ) -> Result<(), String> {
        if BRIDGE.get().is_some() {
            return Ok(());
        }
        if keystore.is_null() {
            return Err("the key store object is null".to_string());
        }
        let vm = env
            .get_java_vm()
            .map_err(|error| format!("read the Java VM: {error}"))?;
        let keystore = env
            .new_global_ref(keystore)
            .map_err(|error| format!("keep the key store object: {error}"))?;
        let _ = BRIDGE.set(Bridge { vm, keystore });
        Ok(())
    }

    /// A JNI failure or a status the bridge reported.
    enum Failure {
        Jni(jni::errors::Error),
        Wrap(WrapError),
    }

    impl From<jni::errors::Error> for Failure {
        fn from(error: jni::errors::Error) -> Self {
            Self::Jni(error)
        }
    }

    /// The `SyncKeystore` bridge as a [`KeyWrapper`].
    pub(crate) struct AndroidKeystore;

    impl AndroidKeystore {
        /// Calls `method` on the Kotlin object with the alias and `arguments` as byte arrays,
        /// and returns the payload after the status byte, at most `max_payload` bytes. Byte
        /// arrays this call created or received are zeroed before it returns when `secret`.
        fn call(
            method: &str,
            signature: &str,
            alias: &str,
            arguments: &[&[u8]],
            max_payload: usize,
            secret: bool,
        ) -> Result<Vec<u8>, WrapError> {
            let bridge = BRIDGE.get().ok_or_else(|| {
                WrapError::Unavailable("the Android key store is not registered yet".to_string())
            })?;
            let mut env = bridge.vm.attach_current_thread().map_err(|error| {
                WrapError::Unavailable(format!("attach to the Java VM: {error}"))
            })?;
            let result = env.with_local_frame(LOCAL_FRAME_CAPACITY, |env| {
                call_in_frame(
                    env,
                    &bridge.keystore,
                    method,
                    signature,
                    alias,
                    arguments,
                    max_payload,
                    secret,
                )
            });
            match result {
                Ok(payload) => Ok(payload),
                Err(Failure::Wrap(error)) => Err(error),
                Err(Failure::Jni(error)) => {
                    if env.exception_check().unwrap_or(false) {
                        let _ = env.exception_clear();
                    }
                    Err(WrapError::Unavailable(format!(
                        "the Android key store call failed: {error}"
                    )))
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call_in_frame(
        env: &mut jni::JNIEnv<'_>,
        keystore: &jni::objects::GlobalRef,
        method: &str,
        signature: &str,
        alias: &str,
        arguments: &[&[u8]],
        max_payload: usize,
        secret: bool,
    ) -> Result<Vec<u8>, Failure> {
        use jni::objects::{JByteArray, JObject, JValue};
        let alias = JObject::from(env.new_string(alias)?);
        let mut arrays = Vec::with_capacity(arguments.len());
        for argument in arguments {
            arrays.push(env.byte_array_from_slice(argument)?);
        }
        let mut values = vec![JValue::Object(&alias)];
        values.extend(arrays.iter().map(|array| JValue::Object(array)));
        let returned = env.call_method(keystore, method, signature, &values);
        // JNI allows no other call while an exception is pending, so a thrown exception is
        // cleared before the arguments are zeroed; `returned` still reports the failure.
        if env.exception_check().unwrap_or(true) {
            let _ = env.exception_clear();
        }
        if secret {
            for (array, argument) in arrays.iter().zip(arguments) {
                let _ = env.set_byte_array_region(array, 0, &vec![0; argument.len()]);
            }
        }
        let result = JByteArray::from(returned?.l()?);
        if result.is_null() {
            return Err(Failure::Wrap(WrapError::Unavailable(
                "the Android key store returned nothing".to_string(),
            )));
        }
        let length = usize::try_from(env.get_array_length(&result)?).unwrap_or(0);
        if length == 0 || length > 1 + max_payload.max(MAX_DETAIL_BYTES) {
            return Err(Failure::Wrap(WrapError::Unavailable(
                "the Android key store returned a result of an invalid length".to_string(),
            )));
        }
        let mut bytes = env.convert_byte_array(&result)?;
        if secret {
            let _ = env.set_byte_array_region(&result, 0, &vec![0; length]);
        }
        let status = bytes.remove(0);
        match status {
            STATUS_OK if bytes.len() <= max_payload => Ok(bytes),
            STATUS_OK => {
                key_wrap::wipe(&mut bytes);
                Err(Failure::Wrap(WrapError::Unavailable(
                    "the Android key store returned too many bytes".to_string(),
                )))
            }
            STATUS_INVALIDATED => Err(Failure::Wrap(WrapError::Invalidated(detail(&bytes)))),
            STATUS_UNAVAILABLE => Err(Failure::Wrap(WrapError::Unavailable(detail(&bytes)))),
            _ => Err(Failure::Wrap(WrapError::Unavailable(
                "the Android key store returned an unknown status".to_string(),
            ))),
        }
    }

    /// The diagnostic text of a failure result, which names an exception class and never key
    /// material.
    fn detail(bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_DETAIL_BYTES)]);
        format!("Android key store: {text}")
    }

    impl KeyWrapper for AndroidKeystore {
        fn wrap(&self, alias: &str, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, WrapError> {
            Self::call(
                "wrap",
                WRAP_SIGNATURE,
                alias,
                &[aad, plaintext],
                key_wrap::NONCE_BYTES + key_wrap::MAX_KEY_BYTES + key_wrap::TAG_BYTES,
                true,
            )
        }

        fn unwrap(&self, alias: &str, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, WrapError> {
            Self::call(
                "unwrap",
                WRAP_SIGNATURE,
                alias,
                &[aad, sealed],
                key_wrap::MAX_KEY_BYTES,
                true,
            )
        }

        fn forget(&self, alias: &str) -> Result<(), WrapError> {
            Self::call("forget", FORGET_SIGNATURE, alias, &[], 0, false).map(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::process_managers::ProcessManagers;
    use ganbaru_handoff::key_wrap::TestKeyWrapper;

    struct ConfigDirectory(std::path::PathBuf);

    impl ConfigDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ganbaru-key-protection-{label}-{}",
                ganbaru_handoff::pairing::random_token("test").expect("token")
            ));
            std::fs::create_dir_all(&path).expect("create config directory");
            Self(path)
        }
    }

    impl Drop for ConfigDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
        std::fs::write(path, bytes).expect("write file");
    }

    #[test]
    fn an_unlinked_device_drops_unsealed_keys_and_stays_unlinked_without_a_reason() {
        let config = ConfigDirectory::new("unlinked");
        let plaintext = config.0.join("sync").join("device-key-vault-1.key");
        write(&plaintext, &[0x30, 0x51]);
        let managers = ProcessManagers::with_key_wrapper(Arc::new(TestKeyWrapper::default()));
        managers
            .load_pairing(&config.0, "device-phone".to_string())
            .expect("load pairing");
        assert!(!plaintext.exists());
        assert_eq!(managers.pairing().unlink_reason().expect("reason"), None);
    }

    #[test]
    fn the_linking_key_is_sealed_on_its_first_use() {
        let config = ConfigDirectory::new("linking");
        let wrapper = Arc::new(TestKeyWrapper::default());
        let managers = ProcessManagers::with_key_wrapper(wrapper.clone());
        managers
            .load_pairing(&config.0, "device-phone".to_string())
            .expect("load pairing");
        managers.pairing().identity().expect("identity");
        let state = std::fs::read_to_string(config.0.join("vault-handoff.json")).expect("state");
        assert!(!state.contains("privateKey"));
        let sealed = identity_key_store(&config.0, wrapper.clone()).path("device-phone");
        assert!(key_wrap::is_envelope(
            &std::fs::read(sealed).expect("sealed key")
        ));
        assert!(wrapper.has_key(&key_wrap::linking_key_alias("device-phone")));
    }

    #[test]
    fn an_unlinked_device_loses_only_the_keys_of_the_vault_whose_platform_key_is_gone() {
        let config = ConfigDirectory::new("lost");
        let wrapper = Arc::new(TestKeyWrapper::default());
        let managers = ProcessManagers::with_key_wrapper(wrapper.clone());
        managers
            .load_pairing(&config.0, "device-phone".to_string())
            .expect("load pairing");
        let (_, original) = managers.pairing().identity().expect("identity");
        let seal = |directory: &str, vault_id: &str, reference: &str| {
            let path = config
                .0
                .join(directory)
                .join(format!("{}.key", reference.replace(':', "-")));
            key_wrap::write_protected_file(
                wrapper.as_ref(),
                &key_wrap::vault_keys_alias(vault_id),
                reference,
                &path,
                b"private key",
            )
            .expect("seal key file");
            path
        };
        let person = seal("contacts", "vault-1", "person-key:vault-1");
        let writer = seal("sync", "vault-1", "writer-key:vault-1:0a1b");
        let other = seal("sync", "vault-12", "device-key:vault-12");
        let extended = seal("sync", "vault-1-2", "writer-key:vault-1-2:0a1b");
        let lost = vec![
            key_wrap::vault_keys_alias("vault-1"),
            key_wrap::linking_key_alias("device-phone"),
        ];

        recover_after_key_loss(&config.0, managers.pairing(), wrapper.as_ref(), &lost)
            .expect("recover");
        assert_eq!(managers.pairing().unlink_reason().expect("reason"), None);
        assert!(!person.exists() && !writer.exists());
        assert!(other.exists());
        assert!(
            extended.exists(),
            "a vault ID that extends another matches none of its keys"
        );
        assert!(!wrapper.has_key(&key_wrap::vault_keys_alias("vault-1")));
        assert!(wrapper.has_key(&key_wrap::vault_keys_alias("vault-12")));
        assert!(
            identity_key_store(&config.0, wrapper.clone())
                .path("device-phone")
                .exists(),
            "an unlinked device replaces its own linking identity"
        );
        assert!(wrapper.has_key(&key_wrap::linking_key_alias("device-phone")));
        let (_, kept) = managers.pairing().identity().expect("identity");
        assert_eq!(kept.certificate, original.certificate);
    }

    #[test]
    fn an_unavailable_key_store_keeps_every_key_for_a_later_pass() {
        let config = ConfigDirectory::new("unavailable");
        let wrapper = Arc::new(TestKeyWrapper::default());
        let managers = ProcessManagers::with_key_wrapper(wrapper.clone());
        managers
            .load_pairing(&config.0, "device-phone".to_string())
            .expect("load pairing");
        let person = config.0.join("contacts").join("person-key-vault-1.key");
        key_wrap::write_protected_file(
            wrapper.as_ref(),
            &key_wrap::vault_keys_alias("vault-1"),
            "person-key:vault-1",
            &person,
            b"person key",
        )
        .expect("seal person key");
        wrapper.set_unavailable(true);
        let lost = vec![key_wrap::vault_keys_alias("vault-1")];
        recover_after_key_loss(&config.0, managers.pairing(), wrapper.as_ref(), &lost)
            .expect_err("unhealthy");
        wrapper.set_unavailable(false);
        assert!(person.exists());
        assert!(wrapper.has_key(&key_wrap::vault_keys_alias("vault-1")));
    }

    #[test]
    fn the_key_protection_wrapper_records_each_lost_alias_once() {
        let protection = KeyProtection::new(Arc::new(TestKeyWrapper::default()));
        let envelope =
            key_wrap::seal(&protection, "alias-a", "reference", b"private key").expect("seal");
        key_wrap::open(&protection, "alias-a", "reference", &envelope).expect("open");
        assert!(protection.take_lost().is_empty());
        protection.forget("alias-a").expect("forget");
        for _ in 0..2 {
            key_wrap::open(&protection, "alias-a", "reference", &envelope).expect_err("lost");
        }
        key_wrap::open(&protection, "alias-b", "reference", b"not an envelope").expect_err("raw");
        assert_eq!(protection.take_lost(), vec!["alias-a", "alias-b"]);
        assert!(protection.take_lost().is_empty());
    }

    #[test]
    fn a_key_that_works_again_leaves_the_record_and_the_probe_is_never_recorded() {
        let protection = KeyProtection::new(Arc::new(TestKeyWrapper::default()));
        key_wrap::open(&protection, "alias-a", "reference", b"not an envelope").expect_err("raw");
        key_wrap::open(&protection, PROBE_ALIAS, PROBE_REFERENCE, b"raw").expect_err("raw");
        // The platform store generated the lost key again and sealed with it.
        key_wrap::seal(&protection, "alias-a", "reference", b"private key").expect("seal");
        assert!(protection.take_lost().is_empty());
    }

    #[test]
    fn a_healthy_key_does_not_clear_a_damaged_key_under_the_same_alias() {
        let protection = KeyProtection::new(Arc::new(TestKeyWrapper::default()));
        let device = key_wrap::seal(&protection, "alias-a", "device-key", b"device key")
            .expect("seal device key");
        key_wrap::open(&protection, "alias-a", "person-key", b"damaged").expect_err("damaged");
        key_wrap::open(&protection, "alias-a", "device-key", &device).expect("open device key");
        assert_eq!(protection.take_lost(), vec!["alias-a"]);

        key_wrap::open(&protection, "alias-a", "person-key", b"damaged").expect_err("damaged");
        key_wrap::seal(&protection, "alias-a", "person-key", b"person key").expect("replace");
        assert!(protection.take_lost().is_empty());
    }

    #[test]
    fn a_replaced_linking_identity_leaves_no_recorded_loss() {
        let config = ConfigDirectory::new("replaced");
        let wrapper = Arc::new(TestKeyWrapper::default());
        let managers = ProcessManagers::with_key_wrapper(wrapper.clone());
        managers
            .load_pairing(&config.0, "device-phone".to_string())
            .expect("load pairing");
        let (_, original) = managers.pairing().identity().expect("identity");
        wrapper
            .forget(&key_wrap::linking_key_alias("device-phone"))
            .expect("lose the linking key");
        managers
            .pairing()
            .forget_cached_identity_key()
            .expect("forget cached key");
        let (_, replaced) = managers.pairing().identity().expect("replaced identity");
        assert_ne!(replaced.certificate, original.certificate);
        assert!(
            managers.key_protection().take_lost().is_empty(),
            "a relinked device must not read the replaced key as lost"
        );
    }

    #[test]
    fn vault_key_files_match_only_their_own_vault() {
        assert!(is_vault_key_file("writer-key-vault-1-0a1b", "vault-1"));
        assert!(!is_vault_key_file("writer-key-vault-1-2-0a1b", "vault-1"));
        assert!(!is_vault_key_file("writer-key-vault-1-", "vault-1"));
        assert!(!is_vault_key_file("device-key-vault/1", "vault/1"));
    }
}
