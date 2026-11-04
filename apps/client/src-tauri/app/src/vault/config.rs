//! The frontend's configuration commands.
//!
//! The frontend reads and patches one composed object; [`super::preferences`] routes each
//! patch to the store its registry path names and composes the stores on read. Native Chat
//! device settings live in the device preference file under a name the registry never routes,
//! so generic patches cannot reach them.

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

/// Read the active vault's effective preference view as one JSON object. Missing stores read
/// as defaults.
#[tauri::command]
pub async fn vault_read_config(app: tauri::AppHandle) -> Result<String, String> {
    let view = super::preferences::effective_view(&app).await?;
    serde_json::to_string(&*view).map_err(|error| format!("encode vault configuration: {error}"))
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
    fn config_patch_paths_refuse_prototype_empty_and_oversized_segments() {
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
        let oversized = "a".repeat(MAX_CONFIG_PATCH_SEGMENT_BYTES + 1);
        for refused in [&[][..], &[""], &["profile", oversized.as_str()]] {
            assert!(validate_config_patch_path(&path(refused)).is_err());
        }
        for accepted in [
            &["profile", "displayName"][..],
            &["chat", "behavior", "sendKey"],
        ] {
            assert_eq!(validate_config_patch_path(&path(accepted)), Ok(()));
        }
    }
}
