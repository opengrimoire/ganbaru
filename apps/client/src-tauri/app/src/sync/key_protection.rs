//! Protection of the private key files a mobile device keeps in its app config directory: the
//! platform key that seals them (an Android Keystore key reached over JNI), the startup removal
//! of key files found unsealed, and the unlink once sealed keys can no longer be opened.
//! Desktop keeps the same keys in the operating-system keyring instead.
//!
//! Losing protected keys is never repaired in place: the device unlinks with
//! [`UnlinkReason::KeyProtectionLost`], every key file and platform key goes, and linking again
//! creates fresh keys.

use crate::vault::handoff::pairing::{PairingManager, UnlinkReason};
#[cfg(not(any(test, target_os = "android")))]
use ganbaru_handoff::key_wrap::WrapError;
use ganbaru_handoff::key_wrap::{self, KeyWrapper, ProtectedIdentityKeyStore};
use std::path::Path;
use std::sync::Arc;

/// Directory under the app config directory that holds the sealed linking TLS key.
const LINKING_KEY_DIRECTORY: &str = "linking";

/// Directories under the app config directory that hold sealed private key files: the sync
/// keys, the person key, and the linking key.
const KEY_DIRECTORIES: [&str; 3] = [
    super::host::SYNC_DIRECTORY,
    crate::contacts::identity::KEY_DIRECTORY,
    LINKING_KEY_DIRECTORY,
];

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

/// Deletes every private key file in the app config directory `config_dir` that is not sealed,
/// such as one an older build wrote in plaintext, and unlinks a linked device when it found
/// one, because a key that sat in plaintext may have been copied. Failures are logged: the
/// next start scans again, and a pass that reads an unsealed key unlinks too.
#[cfg_attr(not(any(mobile, test)), allow(dead_code))]
pub(crate) fn remove_unprotected_keys(
    config_dir: &Path,
    pairing: &PairingManager,
    wrapper: &dyn KeyWrapper,
) {
    let mut removed = 0_usize;
    for directory in KEY_DIRECTORIES {
        match key_wrap::remove_unprotected_key_files(&config_dir.join(directory)) {
            Ok(paths) => removed += paths.len(),
            Err(error) => eprintln!("key protection: {error}"),
        }
    }
    if removed == 0 {
        return;
    }
    eprintln!("key protection: removed {removed} private key files that were not sealed");
    match pairing.has_links() {
        Ok(false) => {}
        Ok(true) => {
            if let Err(error) = unlink_after_key_loss(config_dir, pairing, wrapper) {
                eprintln!("key protection: {error}");
            }
        }
        Err(error) => eprintln!("key protection: {error}"),
    }
}

/// Unlinks this device because its protected keys can no longer be opened, then removes every
/// private key file and the platform keys that sealed them, so linking again starts from fresh
/// keys and a fresh linking identity. An active handoff keeps the device linked and its keys in
/// place until it finishes; the next pass that fails to open a key tries again.
pub(crate) fn unlink_after_key_loss(
    config_dir: &Path,
    pairing: &PairingManager,
    wrapper: &dyn KeyWrapper,
) -> Result<(), String> {
    pairing
        .unlink_for(UnlinkReason::KeyProtectionLost)
        .map_err(|error| format!("unlink after losing protected keys: {error}"))?;
    eprintln!("key protection: unlinked because protected keys can no longer be opened");
    for directory in KEY_DIRECTORIES {
        key_wrap::remove_key_files(&config_dir.join(directory))?;
    }
    pairing.forget_cached_identity_key()?;
    let mut aliases = vec![key_wrap::linking_key_alias(&pairing.device_id()?)];
    if let Ok(vault_id) = crate::vault::active_vault_id_in(config_dir) {
        aliases.push(key_wrap::vault_keys_alias(&vault_id));
    }
    for alias in aliases {
        // The sealed files are gone, so a platform key left behind protects nothing.
        if let Err(error) = wrapper.forget(&alias) {
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
    fn losing_protected_keys_removes_every_key_file_and_platform_key() {
        let config = ConfigDirectory::new("lost");
        let wrapper = Arc::new(TestKeyWrapper::default());
        let managers = ProcessManagers::with_key_wrapper(wrapper.clone());
        managers
            .load_pairing(&config.0, "device-phone".to_string())
            .expect("load pairing");
        let (_, original) = managers.pairing().identity().expect("identity");
        let person = config.0.join("contacts").join("person-key-vault-1.key");
        key_wrap::write_protected_file(
            wrapper.as_ref(),
            &key_wrap::vault_keys_alias("vault-1"),
            "person-key:vault-1",
            &person,
            b"person key",
        )
        .expect("seal person key");

        unlink_after_key_loss(&config.0, managers.pairing(), wrapper.as_ref()).expect("unlink");
        assert_eq!(
            managers.pairing().unlink_reason().expect("reason"),
            Some(UnlinkReason::KeyProtectionLost)
        );
        assert!(!person.exists());
        assert!(
            !identity_key_store(&config.0, wrapper.clone())
                .path("device-phone")
                .exists()
        );
        assert!(!wrapper.has_key(&key_wrap::linking_key_alias("device-phone")));
        let (_, replaced) = managers.pairing().identity().expect("fresh identity");
        assert_ne!(replaced.certificate, original.certificate);
    }
}
