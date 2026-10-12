#[cfg(target_os = "linux")]
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use tauri::Manager;

#[cfg(target_os = "linux")]
use crate::system_command::{fixed_command_output, fixed_command_status};

#[cfg(target_os = "linux")]
fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    fixed_command_output("gsettings", &["get", schema, key], 4096)
        .ok()
        .map(|stdout| stdout.trim().to_string())
}

#[cfg(target_os = "linux")]
fn gsettings_set(schema: &str, key: &str, value: &str) -> Result<(), String> {
    if !is_allowed_gsettings_restore_target(schema, key) {
        return Err(format!(
            "refusing to write unknown gsettings key {schema} {key}"
        ));
    }
    if !is_valid_shortcut_restore_value(value) {
        return Err(format!(
            "refusing to write invalid gsettings value for {schema} {key}"
        ));
    }
    if fixed_command_status("gsettings", &["set", schema, key, value]) {
        Ok(())
    } else {
        Err(format!("gsettings set {schema} {key} failed"))
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SavedShortcuts {
    pub(super) overlay_key: String,
    pub(super) dock_hotkeys: String,
    /// `(schema, key, original_value)` for every disabled keybinding.
    pub(super) disabled: Vec<(String, String, String)>,
}

#[cfg(target_os = "linux")]
pub(super) struct ShortcutRestoreCleanup {
    saved: SavedShortcuts,
    path: PathBuf,
}

#[cfg(target_os = "linux")]
impl ShortcutRestoreCleanup {
    pub(super) fn restore(&self) {
        restore_shortcuts(&self.saved);
        clear_shortcut_restore(&self.path);
    }
}

#[cfg(target_os = "linux")]
const SHORTCUT_RESTORE_FILE: &str = "gnome-shortcuts-restore.json";
#[cfg(target_os = "linux")]
const SHORTCUT_RESTORE_MAX_BYTES: u64 = 256 * 1024;
#[cfg(target_os = "linux")]
pub(super) const SHORTCUT_RESTORE_VALUE_MAX_BYTES: usize = 2048;
#[cfg(target_os = "linux")]
const SHORTCUT_RESTORE_DISABLED_MAX_ITEMS: usize = 512;
#[cfg(target_os = "linux")]
const GNOME_KEYBINDING_SCHEMAS: [&str; 4] = [
    "org.gnome.desktop.wm.keybindings",
    "org.gnome.shell.keybindings",
    "org.gnome.mutter.keybindings",
    "org.gnome.settings-daemon.plugins.media-keys",
];
#[cfg(target_os = "linux")]
const GNOME_SPECIAL_RESTORE_KEYS: [(&str, &str); 2] = [
    ("org.gnome.mutter", "overlay-key"),
    ("org.gnome.shell.extensions.dash-to-dock", "hot-keys"),
];

#[cfg(target_os = "linux")]
fn gsettings_list_recursively(schema: &str) -> Result<Vec<(String, String)>, String> {
    if !is_known_keybinding_schema(schema) {
        return Err(format!("unknown gsettings schema {schema}"));
    }
    let stdout = fixed_command_output("gsettings", &["list-recursively", schema], 64 * 1024)?;
    let mut result = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(3, ' ');
        let _schema = parts.next();
        let key = match parts.next() {
            Some(k) => k.to_string(),
            None => continue,
        };
        let value = match parts.next() {
            Some(v) => v.to_string(),
            None => continue,
        };
        result.push((key, value));
    }
    Ok(result)
}

#[cfg(target_os = "linux")]
fn is_known_keybinding_schema(schema: &str) -> bool {
    GNOME_KEYBINDING_SCHEMAS.contains(&schema)
}

#[cfg(target_os = "linux")]
pub(super) fn is_allowed_gsettings_restore_target(schema: &str, key: &str) -> bool {
    GNOME_SPECIAL_RESTORE_KEYS.contains(&(schema, key))
        || (is_known_keybinding_schema(schema) && is_valid_gsettings_key_name(key))
}

#[cfg(target_os = "linux")]
fn is_valid_gsettings_key_name(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(target_os = "linux")]
fn is_valid_shortcut_restore_value(value: &str) -> bool {
    !value.as_bytes().contains(&0) && value.len() <= SHORTCUT_RESTORE_VALUE_MAX_BYTES
}

#[cfg(target_os = "linux")]
fn current_shortcut_schema_keys() -> Result<HashMap<String, HashSet<String>>, String> {
    let mut keys = HashMap::new();
    for schema in GNOME_KEYBINDING_SCHEMAS {
        let schema_keys = gsettings_list_recursively(schema)?
            .into_iter()
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();
        keys.insert(schema.to_string(), schema_keys);
    }
    Ok(keys)
}

#[cfg(target_os = "linux")]
pub(super) fn validate_saved_shortcuts(
    saved: &SavedShortcuts,
    known_keys: &HashMap<String, HashSet<String>>,
) -> Result<(), String> {
    if !is_valid_shortcut_restore_value(&saved.overlay_key) {
        return Err("invalid GNOME overlay key restore value".to_string());
    }
    if !is_valid_shortcut_restore_value(&saved.dock_hotkeys) {
        return Err("invalid GNOME dock hotkeys restore value".to_string());
    }
    if saved.disabled.len() > SHORTCUT_RESTORE_DISABLED_MAX_ITEMS {
        return Err("GNOME shortcut restore file has too many disabled keys".to_string());
    }
    for (schema, key, value) in &saved.disabled {
        if !is_known_keybinding_schema(schema) {
            return Err(format!("unknown GNOME shortcut schema {schema}"));
        }
        if !is_valid_gsettings_key_name(key) {
            return Err(format!("invalid GNOME shortcut key name {key}"));
        }
        if !known_keys
            .get(schema)
            .is_some_and(|schema_keys| schema_keys.contains(key))
        {
            return Err(format!("unknown GNOME shortcut key {schema} {key}"));
        }
        if !is_valid_shortcut_restore_value(value) {
            return Err(format!(
                "invalid GNOME shortcut restore value for {schema} {key}"
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn write_owner_only_file(path: &Path, contents: &str) -> Result<(), String> {
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(path).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut file, contents.as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;

    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn read_saved_shortcuts_file(path: &Path) -> Result<SavedShortcuts, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > SHORTCUT_RESTORE_MAX_BYTES {
        return Err("GNOME shortcut restore file is too large".to_string());
    }
    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
pub(super) fn read_saved_shortcuts_file_or_clear(path: &Path) -> Result<SavedShortcuts, String> {
    match read_saved_shortcuts_file(path) {
        Ok(saved) => Ok(saved),
        Err(err) => {
            clear_shortcut_restore(path);
            Err(err)
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn validate_saved_shortcuts_or_clear(
    path: &Path,
    saved: SavedShortcuts,
    known_keys: &HashMap<String, HashSet<String>>,
) -> Result<SavedShortcuts, String> {
    match validate_saved_shortcuts(&saved, known_keys) {
        Ok(()) => Ok(saved),
        Err(err) => {
            clear_shortcut_restore(path);
            Err(err)
        }
    }
}

#[cfg(target_os = "linux")]
impl SavedShortcuts {
    fn capture() -> Self {
        let mut disabled = Vec::new();

        for schema in GNOME_KEYBINDING_SCHEMAS {
            let Ok(entries) = gsettings_list_recursively(schema) else {
                continue;
            };
            for (key, value) in entries {
                if value.contains("Super") || value.contains("<Alt>") {
                    disabled.push((schema.to_string(), key.clone(), value));
                }
            }
        }

        let overlay_key =
            gsettings_get("org.gnome.mutter", "overlay-key").unwrap_or_else(|| "'Super_L'".into());

        let dock_hotkeys = gsettings_get("org.gnome.shell.extensions.dash-to-dock", "hot-keys")
            .unwrap_or_else(|| "true".into());

        Self {
            overlay_key,
            dock_hotkeys,
            disabled,
        }
    }

    fn disable(&self) {
        for (schema, key, _) in &self.disabled {
            if let Err(err) = gsettings_set(schema, key, "['']") {
                eprintln!("failed to disable GNOME shortcut {schema} {key}: {err}");
            }
        }
        if let Err(err) = gsettings_set("org.gnome.mutter", "overlay-key", "") {
            eprintln!("failed to disable GNOME overlay key: {err}");
        }
        if let Err(err) = gsettings_set(
            "org.gnome.shell.extensions.dash-to-dock",
            "hot-keys",
            "false",
        ) {
            eprintln!("failed to disable GNOME dock hotkeys: {err}");
        }
    }

    fn save_and_disable(app: &tauri::AppHandle) -> Result<Self, String> {
        let saved = Self::capture();
        let known_keys = current_shortcut_schema_keys()?;
        validate_saved_shortcuts(&saved, &known_keys)?;
        let path = shortcut_restore_path(app)?;
        persist_shortcut_restore(&path, &saved)?;
        saved.disable();
        Ok(saved)
    }
}

#[cfg(target_os = "linux")]
fn shortcut_restore_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    path.push(SHORTCUT_RESTORE_FILE);
    Ok(path)
}

#[cfg(target_os = "linux")]
fn persist_shortcut_restore(path: &Path, saved: &SavedShortcuts) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temporary_path = path.with_extension("json.tmp");
    let json = serde_json::to_string(saved).map_err(|e| e.to_string())?;
    write_owner_only_file(&temporary_path, &json)?;
    std::fs::rename(&temporary_path, path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn restore_shortcuts(saved: &SavedShortcuts) {
    if let Err(err) = gsettings_set("org.gnome.mutter", "overlay-key", &saved.overlay_key) {
        eprintln!("failed to restore GNOME overlay key: {err}");
    }
    if let Err(err) = gsettings_set(
        "org.gnome.shell.extensions.dash-to-dock",
        "hot-keys",
        &saved.dock_hotkeys,
    ) {
        eprintln!("failed to restore GNOME dock hotkeys: {err}");
    }
    for (schema, key, value) in &saved.disabled {
        if let Err(err) = gsettings_set(schema, key, value) {
            eprintln!("failed to restore GNOME shortcut {schema} {key}: {err}");
        }
    }
}

#[cfg(target_os = "linux")]
fn clear_shortcut_restore(path: &Path) {
    if path.exists() {
        if let Err(err) = std::fs::remove_file(path) {
            eprintln!("failed to clear GNOME shortcut restore file: {err}");
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn disable_overlay_shortcuts(
    app: &tauri::AppHandle,
) -> Result<ShortcutRestoreCleanup, String> {
    let path = shortcut_restore_path(app)?;
    let saved = SavedShortcuts::save_and_disable(app)?;
    Ok(ShortcutRestoreCleanup { saved, path })
}

#[cfg(target_os = "linux")]
pub fn restore_stale_shortcuts(app: &tauri::AppHandle) -> Result<(), String> {
    let path = shortcut_restore_path(app)?;
    if !path.exists() {
        return Ok(());
    }
    let saved = read_saved_shortcuts_file_or_clear(&path)?;
    let known_keys = current_shortcut_schema_keys()?;
    let saved = validate_saved_shortcuts_or_clear(&path, saved, &known_keys)?;
    restore_shortcuts(&saved);
    clear_shortcut_restore(&path);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn restore_stale_shortcuts(_app: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}
