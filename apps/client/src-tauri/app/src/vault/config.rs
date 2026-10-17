//! Serialized, bounded mutations of portable vault configuration.

use super::{
    CONFIG_LOCK, active_vault_path, active_writable_vault_path, config_path,
    write_text_file_atomically,
};
use std::fs;
use std::io::Read;
use tauri::Runtime;

const MAX_CONFIG_PATCH_DEPTH: usize = 16;
const MAX_CONFIG_PATCH_SEGMENT_BYTES: usize = 128;
const MAX_CONFIG_PATCH_PATH_BYTES: usize = 1024;
const MAX_CONFIG_PATCH_COUNT: usize = 256;
const MAX_CONFIG_PATCH_BATCH_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultConfigPatch {
    pub path: Vec<String>,
    #[serde(default)]
    pub remove: bool,
    #[serde(default)]
    pub value: serde_json::Value,
}

/// Read the active vault's `config.json` as a string. Returns `"{}"` if
/// the file is missing so the frontend can treat a reset config as defaults.
#[tauri::command]
pub fn vault_read_config(app: tauri::AppHandle) -> Result<String, String> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|_| "vault config lock is unavailable".to_string())?;
    let path = config_path(&active_vault_path(&app)?);
    if !path.exists() {
        return Ok("{}".to_string());
    }
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Read native runtime preferences with a limit that also covers concurrent growth.
pub(crate) fn read_active_config_bounded<R: Runtime>(
    app: &tauri::AppHandle<R>,
    max_bytes: usize,
) -> Result<String, String> {
    let _guard = CONFIG_LOCK
        .lock()
        .map_err(|_| "vault config lock is unavailable".to_owned())?;
    let path = config_path(&active_vault_path(app)?);
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("{}".into()),
        Err(error) => return Err(format!("Open runtime preferences: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Read runtime preferences: {error}"))?;
    if bytes.len() > max_bytes {
        return Err("Runtime preferences exceed the native read limit".into());
    }
    String::from_utf8(bytes).map_err(|error| format!("Runtime preferences are not UTF-8: {error}"))
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
    let raw = if path.exists() {
        fs::read_to_string(&path).map_err(|error| map_storage_error(error.to_string()))?
    } else {
        "{}".to_string()
    };
    let mut root = serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|error| map_storage_error(format!("config payload is not valid JSON: {error}")))?;
    if !root.is_object() {
        return Err(map_storage_error(
            "config payload root must be an object".to_string(),
        ));
    }
    #[cfg(not(target_os = "ios"))]
    let previous_distractions = root.get("distractions").cloned();
    let previous_preferences = root.get("preferences").cloned();
    let result = mutate(&mut root)?;
    ganbaru_chat_contracts::config::parse_chat_config_branch(&root)
        .map_err(|error| map_storage_error(format!("config Chat branch is invalid: {error}")))?;
    let serialized = serde_json::to_string_pretty(&root)
        .map_err(|error| map_storage_error(error.to_string()))?;
    let write = || write_text_file_atomically(&path, &serialized);
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
    // Native Focus reads only the preferences branch, so other patches leave it current.
    if previous_preferences.as_ref() != root.get("preferences") {
        crate::pomodoro::invalidate_calendar(app);
    }
    Ok(result)
}

fn validate_config_patch_path(path: &[String]) -> Result<(), String> {
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
    Ok(())
}

fn apply_config_patch(root: &mut serde_json::Value, patch: VaultConfigPatch) -> Result<(), String> {
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

/// Applies bounded key-level mutations without replacing unrelated config
/// branches that another feature may have updated since the frontend loaded.
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
    tauri::async_runtime::spawn_blocking(move || {
        mutate_active_vault_config(
            &app,
            |root| {
                for patch in patches {
                    apply_config_patch(root, patch)?;
                }
                Ok(())
            },
            |error| error,
        )
    })
    .await
    .map_err(|error| format!("vault configuration worker failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_patches_preserve_unrelated_branches() {
        let chat = serde_json::json!({
            "schemaVersion": 1,
            "providers": [{ "instanceId": "codex" }]
        });
        let mut root = serde_json::json!({
            "chat": chat.clone(),
            "theme": { "activeId": "light" }
        });

        apply_config_patch(
            &mut root,
            VaultConfigPatch {
                path: vec!["theme".to_string(), "activeId".to_string()],
                remove: false,
                value: serde_json::json!("dark"),
            },
        )
        .unwrap();

        assert_eq!(root["chat"], chat);
        assert_eq!(root["theme"]["activeId"], "dark");
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
}
