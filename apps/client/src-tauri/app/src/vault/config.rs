//! The frontend's configuration commands and the vault's `config.json`.
//!
//! `config.json` holds only the `distractions` and `chat` branches, which native features
//! own. The frontend reads and patches one composed object; [`super::preferences`] routes each
//! patch to the store its registry path names and composes the preferences with these
//! branches on read.

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

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultConfigPatch {
    pub path: Vec<String>,
    #[serde(default)]
    pub remove: bool,
    #[serde(default)]
    pub value: serde_json::Value,
}

/// Read the active vault's effective preference view with the `distractions` and `chat`
/// branches of `config.json`, as one JSON object. Missing stores read as defaults.
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
    #[cfg(not(target_os = "ios"))]
    let previous_distractions = root.get("distractions").cloned();
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
    let write = || write_text_file_atomically(&path, &serialized, FileAccess::OwnerOnly);
    #[cfg(desktop)]
    let write_result = if previous_distractions.as_ref() != root.get("distractions") {
        crate::distractions::runtime::commit_configuration(app, write)
    } else {
        write()
    };
    #[cfg(target_os = "android")]
    let write_result = if previous_distractions.as_ref() != root.get("distractions") {
        crate::distractions::android::runtime::commit_configuration(app, write)
    } else {
        write()
    };
    #[cfg(target_os = "ios")]
    let write_result = write();
    write_result.map_err(map_storage_error)?;
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
        return Err("vault config patch targets a native-owned branch".to_string());
    }
    Ok(())
}

pub(super) fn apply_config_patch(
    root: &mut serde_json::Value,
    patch: VaultConfigPatch,
) -> Result<(), String> {
    validate_config_patch_path(&patch.path)?;
    let mut current = root
        .as_object_mut()
        .ok_or_else(|| "config payload root must be an object".to_string())?;
    let (leaf, parents) = patch
        .path
        .split_last()
        .ok_or_else(|| "vault config patch path is invalid".to_string())?;
    for segment in parents {
        let next = current
            .entry(segment.clone())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !next.is_object() {
            *next = serde_json::Value::Object(serde_json::Map::new());
        }
        current = next
            .as_object_mut()
            .ok_or_else(|| "vault config patch target is invalid".to_string())?;
    }
    if patch.remove {
        current.remove(leaf);
    } else {
        current.insert(leaf.clone(), patch.value);
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
    fn config_patches_preserve_unrelated_branches() {
        let chat = serde_json::json!({
            "schemaVersion": 1,
            "providers": [{ "instanceId": "codex" }]
        });
        let mut root = serde_json::json!({
            "chat": chat.clone(),
            "distractions": { "enabled": false }
        });

        apply_config_patch(
            &mut root,
            VaultConfigPatch {
                path: vec!["distractions".to_string(), "enabled".to_string()],
                remove: false,
                value: serde_json::json!(true),
            },
        )
        .unwrap();

        assert_eq!(root["chat"], chat);
        assert_eq!(root["distractions"]["enabled"], true);
    }

    #[test]
    fn config_patches_reject_prototype_paths() {
        let mut root = serde_json::json!({});
        let error = apply_config_patch(
            &mut root,
            VaultConfigPatch {
                path: vec!["__proto__".to_string(), "polluted".to_string()],
                remove: false,
                value: serde_json::json!(true),
            },
        )
        .unwrap_err();

        assert_eq!(error, "vault config patch path is invalid");
        assert_eq!(root, serde_json::json!({}));
    }

    #[test]
    fn config_patches_refuse_native_owned_branches_and_keep_webview_branches() {
        let chat = serde_json::json!({ "schemaVersion": 1, "providers": [] });
        let mut root = serde_json::json!({ "chat": chat.clone() });
        for (path, remove) in [
            (vec!["chat"], true),
            (vec!["chat", "providers"], false),
            (vec!["chat", "behavior", "sendKey"], false),
        ] {
            let error = apply_config_patch(
                &mut root,
                VaultConfigPatch {
                    path: path.into_iter().map(str::to_owned).collect(),
                    remove,
                    value: serde_json::json!("replaced"),
                },
            )
            .unwrap_err();
            assert_eq!(error, "vault config patch targets a native-owned branch");
        }
        assert_eq!(root, serde_json::json!({ "chat": chat }));

        apply_config_patch(
            &mut root,
            VaultConfigPatch {
                path: vec!["distractions".to_string(), "rules".to_string()],
                remove: false,
                value: serde_json::json!([]),
            },
        )
        .unwrap();
        assert_eq!(root["distractions"]["rules"], serde_json::json!([]));
    }
}
