//! The frontend's configuration commands and the vault's `config.json`.
//!
//! `config.json` holds only the `chat` branch, which native Chat commands own. The frontend
//! reads and patches one composed object; [`super::preferences`] routes each patch to the store
//! its registry path names and composes the preferences with that branch on read.

use super::{
    CONFIG_LOCK, FileAccess, MAX_VAULT_CONFIG_BYTES, active_vault_path, active_writable_vault_path,
    config_path, write_text_file_atomically,
};
use std::path::Path;
use tauri::Runtime;

const MAX_CONFIG_PATCH_DEPTH: usize = 16;
const MAX_CONFIG_PATCH_SEGMENT_BYTES: usize = 128;
const MAX_CONFIG_PATCH_PATH_BYTES: usize = 1024;
const MAX_CONFIG_PATCH_COUNT: usize = 256;
const MAX_CONFIG_PATCH_BATCH_BYTES: usize = 1024 * 1024;
/// Top-level branches that only native commands write, through their own validation and locks.
/// The webview reaches them through those commands, never through generic patches.
const NATIVE_OWNED_CONFIG_BRANCHES: &[&str] = &["chat"];
/// The refusal for a generic patch that reaches a branch of `config.json`.
pub(super) const NATIVE_OWNED_BRANCH_ERROR: &str =
    "vault config patch targets a native-owned branch";

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultConfigPatch {
    pub path: Vec<String>,
    #[serde(default)]
    pub remove: bool,
    #[serde(default)]
    pub value: serde_json::Value,
}

/// Read the active vault's effective preference view with the `chat` branch of
/// `config.json`, as one JSON object. Missing stores read as defaults.
#[tauri::command]
pub async fn vault_read_config(app: tauri::AppHandle) -> Result<String, String> {
    let view = super::preferences::effective_view(&app).await?;
    let config_app = app.clone();
    let raw = tauri::async_runtime::spawn_blocking(move || read_active_config_bounded(&config_app))
        .await
        .map_err(|error| format!("vault configuration worker failed: {error}"))??;
    let config = serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|error| format!("vault config is not valid JSON: {error}"))?;
    serde_json::to_string(&super::preferences::compose_with_vault_config(
        &view, &config,
    ))
    .map_err(|error| format!("encode vault configuration: {error}"))
}

/// Read the active vault's `config.json` within [`MAX_VAULT_CONFIG_BYTES`], or `"{}"` when it
/// is missing.
pub(crate) fn read_active_config_bounded<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<String, String> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|_| "vault config lock is unavailable".to_owned())?;
    let path = config_path(&active_vault_path(app)?);
    Ok(read_config_file(&path)?.unwrap_or_else(|| "{}".to_owned()))
}

/// Reads `config.json` at `path` within [`MAX_VAULT_CONFIG_BYTES`], or `None` when it is
/// missing. The caller holds [`CONFIG_LOCK`].
fn read_config_file(path: &Path) -> Result<Option<String>, String> {
    let bytes = match ganbaru_atomic_files::read_regular_file_capped(
        path,
        MAX_VAULT_CONFIG_BYTES,
        ganbaru_atomic_files::FinalSymlink::Refuse,
    ) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
            return Err(format!(
                "vault config exceeds {MAX_VAULT_CONFIG_BYTES} bytes"
            ));
        }
        Err(error) => return Err(format!("read vault config: {error}")),
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| format!("vault config is not UTF-8: {error}"))
}

pub(crate) fn mutate_active_vault_config<R: Runtime, T, E>(
    app: &tauri::AppHandle<R>,
    mutate: impl FnOnce(&mut serde_json::Value) -> Result<T, E>,
    map_storage_error: impl Fn(String) -> E,
) -> Result<T, E> {
    let writable_root = active_writable_vault_path(app).map_err(&map_storage_error)?;
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|_| map_storage_error("vault config lock is unavailable".to_string()))?;
    let path = config_path(&writable_root);
    let raw = read_config_file(&path)
        .map_err(&map_storage_error)?
        .unwrap_or_else(|| "{}".to_owned());
    let mut root = serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|error| map_storage_error(format!("config payload is not valid JSON: {error}")))?;
    if !root.is_object() {
        return Err(map_storage_error(
            "config payload root must be an object".to_string(),
        ));
    }
    let result = mutate(&mut root)?;
    ganbaru_chat_contracts::config::parse_chat_config_branch(&root)
        .map_err(|error| map_storage_error(format!("config Chat branch is invalid: {error}")))?;
    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|error| map_storage_error(error.to_string()))?;
    if serialized.len() as u64 > MAX_VAULT_CONFIG_BYTES {
        return Err(map_storage_error(format!(
            "vault config would exceed {MAX_VAULT_CONFIG_BYTES} bytes"
        )));
    }
    write_text_file_atomically(&path, &serialized, FileAccess::OwnerOnly)
        .map_err(map_storage_error)?;
    Ok(result)
}

pub(super) fn validate_config_patch_path(path: &[String]) -> Result<(), String> {
    let total_bytes = path.iter().map(String::len).sum::<usize>();
    if path.is_empty()
        || path.len() > MAX_CONFIG_PATCH_DEPTH
        || total_bytes > MAX_CONFIG_PATCH_PATH_BYTES
        || path.iter().any(|segment| {
            segment.is_empty()
                || segment.len() > MAX_CONFIG_PATCH_SEGMENT_BYTES
                || matches!(segment.as_str(), "__proto__" | "prototype" | "constructor")
        })
    {
        return Err("vault config patch path is invalid".to_string());
    }
    if NATIVE_OWNED_CONFIG_BRANCHES.contains(&path[0].as_str()) {
        return Err(NATIVE_OWNED_BRANCH_ERROR.to_string());
    }
    Ok(())
}

/// Applies bounded key-level mutations to the stores their registered paths name, without
/// replacing unrelated values that another feature or device may have updated since the
/// frontend loaded.
#[tauri::command]
pub async fn vault_patch_config(
    app: tauri::AppHandle,
    patches: Vec<VaultConfigPatch>,
) -> Result<(), String> {
    if patches.is_empty() {
        return Ok(());
    }
    if patches.len() > MAX_CONFIG_PATCH_COUNT
        || serde_json::to_vec(&patches)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_CONFIG_PATCH_BATCH_BYTES
    {
        return Err("vault config patch batch is too large".to_string());
    }
    super::preferences::apply_patches(&app, patches).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_reads_accept_the_limit_and_refuse_one_byte_more() {
        let directory = super::super::tests::unique_path("bounded-config");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.json");
        assert_eq!(read_config_file(&path).unwrap(), None);

        let limit = usize::try_from(MAX_VAULT_CONFIG_BYTES).unwrap();
        let mut contents = vec![b' '; limit - 2];
        contents.splice(0..0, *b"{");
        contents.push(b'}');
        std::fs::write(&path, &contents).unwrap();
        assert_eq!(read_config_file(&path).unwrap().unwrap().len(), limit);

        contents.push(b' ');
        std::fs::write(&path, &contents).unwrap();
        let error = read_config_file(&path).unwrap_err();
        assert!(error.contains("exceeds"), "{error}");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn config_patch_paths_refuse_prototype_segments_and_native_owned_branches() {
        let path = |segments: &[&str]| {
            segments
                .iter()
                .map(|segment| (*segment).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            validate_config_patch_path(&path(&["__proto__", "polluted"])).unwrap_err(),
            "vault config patch path is invalid"
        );
        for refused in [
            &["chat"][..],
            &["chat", "providers"],
            &["chat", "behavior", "sendKey"],
        ] {
            assert_eq!(
                validate_config_patch_path(&path(refused)).unwrap_err(),
                NATIVE_OWNED_BRANCH_ERROR
            );
        }
        assert_eq!(
            validate_config_patch_path(&path(&["profile", "displayName"])),
            Ok(())
        );
    }
}
