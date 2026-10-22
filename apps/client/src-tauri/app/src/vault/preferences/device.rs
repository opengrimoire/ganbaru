//! The device preference file: one flat JSON object per vault in the app config directory,
//! keyed by registry path. It lives outside the vault, so vault replacement, rollback restores,
//! backups, and sync never carry it.

use super::super::{CONFIG_LOCK, FileAccess, MAX_VAULT_CONFIG_BYTES, write_text_file_atomically};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Directory under the app config directory that holds one file per vault.
const DEVICE_PREFERENCES_DIRECTORY: &str = "vault-preferences";
const DEVICE_PREFERENCES_EXTENSION: &str = "json";

/// The device preference file of `vault_id` under `app_config_dir`.
pub(crate) fn device_file_path(app_config_dir: &Path, vault_id: &str) -> Result<PathBuf, String> {
    ganbaru_handoff::protocol::validate_identifier("Ganbaru folder id", vault_id)?;
    Ok(app_config_dir
        .join(DEVICE_PREFERENCES_DIRECTORY)
        .join(format!("{vault_id}.{DEVICE_PREFERENCES_EXTENSION}")))
}

/// Reads the device preference file, or an empty object when it does not exist yet.
pub(crate) fn read_device_file(path: &Path) -> Result<Map<String, Value>, String> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|_| "vault config lock is unavailable".to_owned())?;
    read_unlocked(path)
}

/// Applies `mutate` to the device preference file and writes it back when it reports a
/// change. Returns whether the file changed.
pub(crate) fn update_device_file(
    path: &Path,
    mutate: impl FnOnce(&mut Map<String, Value>) -> bool,
) -> Result<bool, String> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|_| "vault config lock is unavailable".to_owned())?;
    let mut entries = read_unlocked(path)?;
    if !mutate(&mut entries) {
        return Ok(false);
    }
    let serialized = serde_json::to_string_pretty(&Value::Object(entries))
        .map_err(|error| format!("encode device preferences: {error}"))?;
    if serialized.len() as u64 > MAX_VAULT_CONFIG_BYTES {
        return Err(format!(
            "device preferences would exceed {MAX_VAULT_CONFIG_BYTES} bytes"
        ));
    }
    if let Some(directory) = path.parent() {
        ganbaru_atomic_files::create_private_directories(directory)
            .map_err(|error| format!("create device preferences directory: {error}"))?;
    }
    write_text_file_atomically(path, &serialized, FileAccess::OwnerOnly)?;
    Ok(true)
}

/// The caller holds [`CONFIG_LOCK`].
fn read_unlocked(path: &Path) -> Result<Map<String, Value>, String> {
    let bytes = match ganbaru_atomic_files::read_regular_file_capped(
        path,
        MAX_VAULT_CONFIG_BYTES,
        ganbaru_atomic_files::FinalSymlink::Refuse,
    ) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
            return Err(format!(
                "device preferences exceed {MAX_VAULT_CONFIG_BYTES} bytes"
            ));
        }
        Err(error) => return Err(format!("read device preferences: {error}")),
    };
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(entries)) => Ok(entries),
        Ok(_) => Err("device preferences are not a JSON object".to_owned()),
        Err(error) => Err(format!("device preferences are not valid JSON: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preferences_device_file_round_trips_and_skips_unchanged_writes() {
        let directory = super::super::super::tests::unique_path("device-preferences");
        let path = device_file_path(&directory, "vault-1").unwrap();
        assert!(path.starts_with(directory.join(DEVICE_PREFERENCES_DIRECTORY)));
        assert_eq!(read_device_file(&path).unwrap(), Map::new());

        assert!(!update_device_file(&path, |_| false).unwrap());
        assert!(!path.exists());
        assert!(
            update_device_file(&path, |entries| {
                entries.insert("theme.activeId".to_owned(), json!("dark"));
                true
            })
            .unwrap()
        );
        assert_eq!(read_device_file(&path).unwrap()["theme.activeId"], "dark");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }

        std::fs::write(&path, "[]").unwrap();
        assert!(read_device_file(&path).unwrap_err().contains("object"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn preferences_device_file_refuses_unsafe_vault_ids() {
        let directory = super::super::super::tests::unique_path("device-preferences-id");
        for id in ["", "../escape", "a/b"] {
            assert!(device_file_path(&directory, id).is_err(), "{id}");
        }
    }
}
