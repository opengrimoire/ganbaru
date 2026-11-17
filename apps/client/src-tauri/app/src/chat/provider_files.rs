//! Bounded access to documented coding-agent configuration and instruction files.

mod notes;
mod preferences;

use super::settings_commands::read_provider;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProviderInstanceConfig, ProviderInstanceId,
};
pub use notes::ProviderFileNote;
pub use preferences::ProviderPreferencesRead;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

const MAX_PROVIDER_FILE_BYTES: u64 = 1024 * 1024;
/// Extensions OpenCode accepts for a configuration file named by `OPENCODE_CONFIG`.
const OPENCODE_CONFIG_EXTENSIONS: &[&str] = &["json", "jsonc"];
/// Name of the Cursor CLI configuration file.
const CURSOR_CLI_CONFIGURATION_NAME: &str = "cli-config.json";
static PROVIDER_FILE_WRITE_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub struct ProviderFileState {
    mutation_lock: Mutex<()>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFileKind {
    Configuration,
    Instructions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFileFormat {
    Toml,
    Json,
    Jsonc,
    Markdown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFileRead {
    pub file_id: String,
    pub name: String,
    pub kind: ProviderFileKind,
    pub format: ProviderFileFormat,
    pub path: String,
    pub exists: bool,
    pub contents: String,
    pub revision: String,
    /// Teammate preferences of a configuration file whose provider keeps an allowlist.
    pub preferences: Option<ProviderPreferencesRead>,
}

/// The files of one provider instance and notes about the instruction sources they do not cover.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFilesRead {
    pub files: Vec<ProviderFileRead>,
    pub notes: Vec<ProviderFileNote>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderFileRequest {
    pub instance_id: ProviderInstanceId,
    pub file_id: String,
    pub contents: String,
    pub expected_revision: String,
}

/// An edit of one teammate preference in a provider configuration file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderPreferenceRequest {
    pub instance_id: ProviderInstanceId,
    pub file_id: String,
    pub preference_id: String,
    /// The new value, or null to remove the entry.
    #[serde(default)]
    pub value: serde_json::Value,
    pub expected_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProviderFileSpec {
    file_id: String,
    kind: ProviderFileKind,
    format: ProviderFileFormat,
    path: PathBuf,
    /// Whether saving may create missing parent folders. Only paths in the provider home may.
    create_parent: bool,
    /// Preferences this file holds that the page edits, when its provider has a catalog.
    preferences: Option<preferences::PreferenceCatalog>,
}

#[tauri::command]
pub fn chat_read_provider_files(
    app: tauri::AppHandle,
    instance_id: ProviderInstanceId,
) -> ChatResult<ProviderFilesRead> {
    let provider = read_provider(&app, &instance_id)?;
    crate::chat::settings::require_runtime_approved_on_device(&app, &provider.configuration)?;
    let private_home =
        crate::chat::settings::configured_private_home(&app, &provider.configuration)?;
    read_provider_files(
        &provider.configuration,
        private_home.as_deref(),
        &user_home(),
    )
}

/// Reads the files of one instance and the notes about sources those files do not cover, with
/// `user_home` as the folder that holds other providers' home folders.
fn read_provider_files(
    configuration: &ProviderInstanceConfig,
    private_home: Option<&Path>,
    user_home: &Path,
) -> ChatResult<ProviderFilesRead> {
    let files = provider_file_specs(configuration, private_home)?
        .into_iter()
        .map(read_provider_file)
        .collect::<ChatResult<Vec<_>>>()?;
    if configuration.family_id.as_str() == "acp" {
        return Ok(ProviderFilesRead {
            files,
            notes: Vec::new(),
        });
    }
    let own_home = provider_home(configuration)?;
    let folders = notes::NoteFolders {
        home: private_home.unwrap_or(&own_home),
        replaced_home: private_home.map(|_| own_home.clone()),
        user_home,
    };
    let notes = notes::provider_file_notes(configuration, &folders, &files);
    Ok(ProviderFilesRead { files, notes })
}

#[tauri::command]
pub fn chat_save_provider_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProviderFileState>,
    request: SaveProviderFileRequest,
) -> ChatResult<ProviderFileRead> {
    validate_contents(&request.contents)?;
    save_provider_file(
        &app,
        &state,
        &request.instance_id,
        &request.file_id,
        &request.expected_revision,
        |_| Ok(request.contents.clone()),
    )
}

/// Sets or removes one allowlisted teammate preference in a provider configuration file,
/// keeping every other key of the document.
#[tauri::command]
pub fn chat_save_provider_preference(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProviderFileState>,
    request: SaveProviderPreferenceRequest,
) -> ChatResult<ProviderFileRead> {
    save_provider_file(
        &app,
        &state,
        &request.instance_id,
        &request.file_id,
        &request.expected_revision,
        |spec| {
            let catalog = spec.preferences.ok_or_else(|| {
                ChatError::validation("fileId", "This file has no preferences to change here")
            })?;
            let contents = preferences::apply_preference(
                spec.contents,
                catalog,
                &request.preference_id,
                &request.value,
            )?;
            validate_contents(&contents)?;
            Ok(contents)
        },
    )
}

/// The current text and preferences of the file a save edits.
struct SaveTarget<'a> {
    contents: &'a str,
    preferences: Option<preferences::PreferenceCatalog>,
}

/// Writes the contents `edit` derives from the current file, after checking that the file still
/// has `expected_revision`.
fn save_provider_file(
    app: &tauri::AppHandle,
    state: &ProviderFileState,
    instance_id: &ProviderInstanceId,
    file_id: &str,
    expected_revision: &str,
    edit: impl FnOnce(SaveTarget<'_>) -> ChatResult<String>,
) -> ChatResult<ProviderFileRead> {
    let _guard = state
        .mutation_lock
        .lock()
        .map_err(|_| provider_file_error())?;
    let provider = read_provider(app, instance_id)?;
    crate::chat::settings::require_runtime_approved_on_device(app, &provider.configuration)?;
    let private_home =
        crate::chat::settings::configured_private_home(app, &provider.configuration)?;
    let spec = provider_file_specs(&provider.configuration, private_home.as_deref())?
        .into_iter()
        .find(|spec| spec.file_id == file_id)
        .ok_or_else(|| ChatError::validation("fileId", "Provider file is not supported"))?;
    let current = read_provider_file(spec.clone())?;
    if current.revision != expected_revision {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The provider file changed outside Ganbaru. Reload it before saving.",
            true,
        ));
    }
    let contents = edit(SaveTarget {
        contents: &current.contents,
        preferences: spec.preferences,
    })?;
    write_text_file_atomically(&spec.path, &contents, spec.create_parent)?;
    read_provider_file(spec)
}

/// The files a provider instance reads: its configuration and user instructions, in the
/// private home that its launches use when it has one.
fn provider_file_specs(
    configuration: &ProviderInstanceConfig,
    private_home: Option<&Path>,
) -> ChatResult<Vec<ProviderFileSpec>> {
    let family = configuration.family_id.as_str();
    // A generic ACP agent has no documented configuration location Ganbaru could name.
    if family == "acp" {
        return Ok(Vec::new());
    }
    let home = match private_home {
        Some(home) => home.to_path_buf(),
        None => provider_home(configuration)?,
    };
    let mut specs = match family {
        "codex" => {
            let mut specs =
                standard_pair(&home, "config.toml", ProviderFileFormat::Toml, "AGENTS.md");
            // Codex reads this file instead of `AGENTS.md` while it has content.
            let override_path = home.join("AGENTS.override.md");
            if override_path.exists() {
                specs.push(ProviderFileSpec {
                    file_id: notes::CODEX_OVERRIDE_FILE_ID.to_string(),
                    ..instructions_spec(override_path)
                });
            }
            specs
        }
        "claude" => standard_pair(
            &home,
            "settings.json",
            ProviderFileFormat::Json,
            "CLAUDE.md",
        ),
        "cursor" => vec![configuration_spec(
            "configuration",
            home.join(CURSOR_CLI_CONFIGURATION_NAME),
            ProviderFileFormat::Json,
        )],
        "grok" => standard_pair(&home, "config.toml", ProviderFileFormat::Toml, "AGENTS.md"),
        "opencode" => opencode_specs(configuration, &home)?,
        _ => {
            return Err(ChatError::unsupported(
                "This provider does not expose documented editable files",
            ));
        }
    };
    for spec in &mut specs {
        if spec.kind == ProviderFileKind::Configuration {
            spec.preferences = preferences::file_preferences(family, spec.format);
        }
    }
    specs.sort_by(|left, right| left.file_id.cmp(&right.file_id));
    Ok(specs)
}

fn standard_pair(
    home: &Path,
    configuration_name: &str,
    configuration_format: ProviderFileFormat,
    instructions_name: &str,
) -> Vec<ProviderFileSpec> {
    vec![
        configuration_spec(
            "configuration",
            home.join(configuration_name),
            configuration_format,
        ),
        instructions_spec(home.join(instructions_name)),
    ]
}

fn configuration_spec(
    file_id: &str,
    path: PathBuf,
    format: ProviderFileFormat,
) -> ProviderFileSpec {
    ProviderFileSpec {
        file_id: file_id.to_string(),
        kind: ProviderFileKind::Configuration,
        format,
        path,
        create_parent: true,
        preferences: None,
    }
}

fn instructions_spec(path: PathBuf) -> ProviderFileSpec {
    ProviderFileSpec {
        file_id: "instructions".to_string(),
        kind: ProviderFileKind::Instructions,
        format: ProviderFileFormat::Markdown,
        path,
        create_parent: true,
        preferences: None,
    }
}

/// OpenCode files: the file `OPENCODE_CONFIG` names, the configuration in the instance's home,
/// and the global configuration, which OpenCode loads first even when the instance has its own
/// home.
fn opencode_specs(
    configuration: &ProviderInstanceConfig,
    home: &Path,
) -> ChatResult<Vec<ProviderFileSpec>> {
    let mut specs = Vec::new();
    if let Some(custom_path) = configured_environment(configuration, "OPENCODE_CONFIG") {
        let path = opencode_config_path(&custom_path)?;
        specs.push(ProviderFileSpec {
            create_parent: false,
            ..configuration_spec("custom_configuration", path.clone(), json_format(&path))
        });
    }
    opencode_folder_specs(home, "configuration", &mut specs);
    let global = opencode_global_folder(configuration);
    if global != home {
        opencode_folder_specs(&global, "global_configuration", &mut specs);
    }
    specs.push(instructions_spec(home.join("AGENTS.md")));
    Ok(specs)
}

/// The `opencode.json` and `opencode.jsonc` of `folder`: whichever exist, or the JSON file when
/// neither does.
fn opencode_folder_specs(folder: &Path, file_id: &str, specs: &mut Vec<ProviderFileSpec>) {
    let json = folder.join("opencode.json");
    let jsonc = folder.join("opencode.jsonc");
    let shows_json = json.exists() || !jsonc.exists();
    if shows_json {
        specs.push(configuration_spec(file_id, json, ProviderFileFormat::Json));
    }
    if jsonc.exists() {
        let id = if shows_json {
            format!("{file_id}_jsonc")
        } else {
            file_id.to_string()
        };
        specs.push(configuration_spec(&id, jsonc, ProviderFileFormat::Jsonc));
    }
}

/// The folder of OpenCode's global configuration: `opencode` under the XDG configuration home.
fn opencode_global_folder(configuration: &ProviderInstanceConfig) -> PathBuf {
    configured_environment(configuration, "XDG_CONFIG_HOME")
        .or_else(|| process_environment("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| user_home().join(".config"))
        .join("opencode")
}

/// The folder of the Cursor CLI configuration the CLI reads: `CURSOR_CONFIG_DIR`, then `cursor`
/// under the XDG configuration home when the file exists there, then `~/.cursor`.
fn cursor_configuration_folder(configuration: &ProviderInstanceConfig) -> PathBuf {
    let variable = |name| {
        configured_environment(configuration, name)
            .or_else(|| process_environment(name))
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if let Some(folder) = variable("CURSOR_CONFIG_DIR") {
        return folder;
    }
    if let Some(folder) = variable("XDG_CONFIG_HOME").map(|home| home.join("cursor"))
        && folder.join(CURSOR_CLI_CONFIGURATION_NAME).is_file()
    {
        return folder;
    }
    user_home().join(".cursor")
}

/// Resolves an `OPENCODE_CONFIG` value, which must name a JSON or JSONC file.
///
/// The value is user configuration that this device confirmed before the portal opens, so it may
/// point outside the provider home as OpenCode allows; the extension check keeps it from naming
/// keys, shell profiles, or other non-configuration files.
fn opencode_config_path(value: &str) -> ChatResult<PathBuf> {
    let path = resolve_absolute_path(value, "OPENCODE_CONFIG")?;
    let supported = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            OPENCODE_CONFIG_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        });
    if !supported {
        return Err(ChatError::validation(
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG must name a JSON or JSONC file",
        ));
    }
    Ok(path)
}

/// The folder that holds a provider's user configuration and instruction files: the instance's
/// provider home, then the provider's own home variable, then its default under the user home.
///
/// # Errors
///
/// Returns an unsupported error for a family without documented files, or a validation error
/// when the resolved path is not absolute.
pub(crate) fn provider_home(configuration: &ProviderInstanceConfig) -> ChatResult<PathBuf> {
    let family = configuration.family_id.as_str();
    let configured = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let value = match family {
        "codex" => configured
            .map(str::to_string)
            .or_else(|| process_environment("CODEX_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".codex")),
        "claude" => configured
            .map(str::to_string)
            .or_else(|| process_environment("CLAUDE_CONFIG_DIR"))
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".claude")),
        "cursor" => cursor_configuration_folder(configuration),
        "grok" => configured
            .map(str::to_string)
            .or_else(|| configured_environment(configuration, "GROK_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".grok")),
        "opencode" => configured
            .map(PathBuf::from)
            .or_else(|| {
                configured_environment(configuration, "OPENCODE_CONFIG_DIR").map(PathBuf::from)
            })
            .unwrap_or_else(|| opencode_global_folder(configuration)),
        _ => return Err(ChatError::unsupported("Provider files are unavailable")),
    };
    resolve_absolute_path(value.to_string_lossy().as_ref(), "providerHome")
}

fn configured_environment(configuration: &ProviderInstanceConfig, name: &str) -> Option<String> {
    let value = configuration.environment.get(name)?.trim();
    if value == format!("inherit:{name}") {
        process_environment(name)
    } else if value.is_empty() || value.starts_with("inherit:") {
        None
    } else {
        Some(value.to_string())
    }
}

fn process_environment(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The user's home folder, from `HOME` or `USERPROFILE`.
pub(crate) fn user_home() -> PathBuf {
    #[cfg(windows)]
    let names = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let names = ["HOME", "USERPROFILE"];
    names
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_absolute_path(value: &str, field: &str) -> ChatResult<PathBuf> {
    let path = if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        if value.len() == 1 {
            user_home()
        } else {
            user_home().join(&value[2..])
        }
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return Err(ChatError::validation(
            field,
            "Provider file path must be absolute",
        ));
    }
    Ok(path)
}

fn read_provider_file(spec: ProviderFileSpec) -> ChatResult<ProviderFileRead> {
    reject_symbolic_path(&spec.path)?;
    let exists = spec.path.exists();
    let contents = if exists {
        read_bounded_utf8(&spec.path)?
    } else {
        String::new()
    };
    let preferences = spec
        .preferences
        .map(|catalog| preferences::read_preferences(&contents, catalog));
    Ok(ProviderFileRead {
        file_id: spec.file_id,
        name: spec
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(provider_file_error)?,
        kind: spec.kind,
        format: spec.format,
        path: display_path(&spec.path),
        exists,
        revision: revision(&contents, exists),
        contents,
        preferences,
    })
}

/// Home-relative display form of a provider file path, so the webview never sees the home folder.
fn display_path(path: &Path) -> String {
    match path.strip_prefix(user_home()) {
        Ok(relative) if user_home().is_absolute() => {
            let relative = relative.to_string_lossy();
            if relative.is_empty() {
                "~".to_string()
            } else {
                format!("~{}{relative}", std::path::MAIN_SEPARATOR)
            }
        }
        _ => path.to_string_lossy().into_owned(),
    }
}

fn read_bounded_utf8(path: &Path) -> ChatResult<String> {
    let metadata = fs::metadata(path).map_err(|_| provider_file_error())?;
    if !metadata.is_file() || metadata.len() > MAX_PROVIDER_FILE_BYTES {
        return Err(ChatError::validation(
            "providerFile",
            "Provider file must be a UTF-8 text file no larger than 1 MiB",
        ));
    }
    let mut file = File::open(path).map_err(|_| provider_file_error())?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_PROVIDER_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| provider_file_error())?;
    if bytes.len() as u64 > MAX_PROVIDER_FILE_BYTES {
        return Err(ChatError::validation(
            "providerFile",
            "Provider file exceeds the 1 MiB limit",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        ChatError::validation("providerFile", "Provider file must contain valid UTF-8")
    })
}

fn validate_contents(contents: &str) -> ChatResult<()> {
    if contents.len() as u64 > MAX_PROVIDER_FILE_BYTES || contents.contains('\0') {
        return Err(ChatError::validation(
            "contents",
            "Provider file must be UTF-8 text no larger than 1 MiB",
        ));
    }
    Ok(())
}

fn revision(contents: &str, exists: bool) -> String {
    let mut digest = Sha256::new();
    digest.update(if exists {
        b"provider-file-v1\0".as_slice()
    } else {
        b"provider-file-missing-v1\0".as_slice()
    });
    digest.update(contents.as_bytes());
    format!("{:x}", digest.finalize())
}

fn reject_symbolic_path(path: &Path) -> ChatResult<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ChatError::validation(
                    "providerFile",
                    "Symbolic links are not editable from Chat settings",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(provider_file_error()),
        }
    }
    Ok(())
}

fn write_text_file_atomically(path: &Path, contents: &str, create_parent: bool) -> ChatResult<()> {
    let parent = path.parent().ok_or_else(provider_file_error)?;
    reject_symbolic_path(path)?;
    if !parent.is_dir() {
        if !create_parent {
            return Err(ChatError::validation(
                "providerFile",
                "The folder for this provider file does not exist",
            ));
        }
        ganbaru_atomic_files::create_private_directories(parent)
            .map_err(|_| provider_file_error())?;
    }
    reject_symbolic_path(path)?;
    let file_name = path
        .file_name()
        .ok_or_else(provider_file_error)?
        .to_string_lossy();
    let generation = PROVIDER_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.ganbaru.{}.{}.tmp",
        std::process::id(),
        generation
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| provider_file_error())?;
        file.write_all(contents.as_bytes())
            .map_err(|_| provider_file_error())?;
        file.sync_all().map_err(|_| provider_file_error())?;
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, target: &Path) -> ChatResult<()> {
    fs::rename(temporary, target).map_err(|_| provider_file_error())
}

#[cfg(windows)]
fn replace_file(temporary: &Path, target: &Path) -> ChatResult<()> {
    if !target.exists() {
        return fs::rename(temporary, target).map_err(|_| provider_file_error());
    }
    let generation = PROVIDER_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let backup = target.with_extension(format!(
        "ganbaru.{}.{}.backup",
        std::process::id(),
        generation
    ));
    fs::rename(target, &backup).map_err(|_| provider_file_error())?;
    if fs::rename(temporary, target).is_err() {
        let _ = fs::rename(&backup, target);
        return Err(provider_file_error());
    }
    fs::remove_file(backup).map_err(|_| provider_file_error())
}

fn json_format(path: &Path) -> ProviderFileFormat {
    if path.extension().and_then(|value| value.to_str()) == Some("jsonc") {
        ProviderFileFormat::Jsonc
    } else {
        ProviderFileFormat::Json
    }
}

fn provider_file_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Provider file could not be read or saved",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::notes::ProviderFileNoteKind;
    use super::*;
    use ganbaru_chat_contracts::models::{ProviderFamilyId, VersionedJson};
    use std::collections::BTreeMap;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let generation = PROVIDER_FILE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ganbaru-provider-files-{label}-{}-{generation}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn configuration(family: &str, home: &Path) -> ProviderInstanceConfig {
        ProviderInstanceConfig {
            schema_version: 1,
            instance_id: ProviderInstanceId::new(format!("{family}-test")).unwrap(),
            family_id: ProviderFamilyId::new(family).unwrap(),
            label: family.to_string(),
            enabled: true,
            executable: family.to_string(),
            provider_home: Some(home.to_string_lossy().into_owned()),
            launch_arguments: Vec::new(),
            environment: BTreeMap::new(),
            credential_references: BTreeMap::new(),
            visible_model_ids: Vec::new(),
            favorite_model_ids: Vec::new(),
            provider_config: VersionedJson {
                schema_version: 1,
                value: serde_json::json!({}),
            },
            internal_mcp: None,
            private_home: None,
            unknown_fields: BTreeMap::new(),
        }
    }

    #[test]
    fn provider_specs_keep_each_family_in_its_documented_home() {
        let root = PathBuf::from("/provider-home");
        let cases = [
            ("codex", "config.toml", Some("AGENTS.md")),
            ("claude", "settings.json", Some("CLAUDE.md")),
            ("grok", "config.toml", Some("AGENTS.md")),
        ];
        for (family, config_name, instruction_name) in cases {
            let specs = provider_file_specs(&configuration(family, &root), None).unwrap();
            assert!(specs.iter().any(|spec| spec.path == root.join(config_name)));
            assert!(
                specs
                    .iter()
                    .any(|spec| spec.path == root.join(instruction_name.unwrap()))
            );
        }
    }

    #[test]
    fn generic_acp_agents_expose_no_provider_files() {
        let specs =
            provider_file_specs(&configuration("acp", Path::new("/provider-home")), None).unwrap();
        assert!(specs.is_empty());
    }

    #[test]
    fn cursor_exposes_the_cli_configuration_the_cli_reads() {
        let root = TestDirectory::new("cursor");
        let mut cursor = configuration("cursor", Path::new("/ignored"));
        let xdg = root.path().join("config");
        cursor.environment.insert(
            "XDG_CONFIG_HOME".to_string(),
            xdg.to_string_lossy().into_owned(),
        );
        cursor.environment.insert(
            "CURSOR_CONFIG_DIR".to_string(),
            root.path().join("chosen").to_string_lossy().into_owned(),
        );
        let specs = provider_file_specs(&cursor, None).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].file_id, "configuration");
        assert_eq!(
            specs[0].path,
            root.path().join("chosen").join("cli-config.json")
        );
        assert!(specs[0].preferences.is_some());

        cursor.environment.remove("CURSOR_CONFIG_DIR");
        fs::create_dir_all(xdg.join("cursor")).unwrap();
        fs::write(xdg.join("cursor").join("cli-config.json"), "{}").unwrap();
        let specs = provider_file_specs(&cursor, None).unwrap();
        assert_eq!(specs[0].path, xdg.join("cursor").join("cli-config.json"));
    }

    #[test]
    fn grok_files_follow_the_private_home_of_an_api_key_instance() {
        let private = Path::new("/config/ganbaru/chat-provider-homes/vault/instance");
        let specs = provider_file_specs(
            &configuration("grok", Path::new("/provider-home")),
            Some(private),
        )
        .unwrap();
        assert!(specs.iter().all(|spec| spec.path.starts_with(private)));
    }

    #[test]
    fn opencode_with_its_own_home_also_shows_the_global_configuration() {
        let root = TestDirectory::new("opencode-global");
        let mut opencode = configuration("opencode", &root.path().join("instance"));
        opencode.environment.insert(
            "XDG_CONFIG_HOME".to_string(),
            root.path().join("config").to_string_lossy().into_owned(),
        );
        let specs = provider_file_specs(&opencode, None).unwrap();
        let global = specs
            .iter()
            .find(|spec| spec.file_id == "global_configuration")
            .unwrap();
        assert_eq!(
            global.path,
            root.path()
                .join("config")
                .join("opencode")
                .join("opencode.json")
        );
        opencode.provider_home = None;
        let specs = provider_file_specs(&opencode, None).unwrap();
        assert!(
            specs
                .iter()
                .all(|spec| !spec.file_id.starts_with("global_"))
        );
    }

    #[test]
    fn configuration_files_carry_preferences_and_instructions_do_not() {
        let root = PathBuf::from("/provider-home");
        for family in ["claude", "cursor", "codex", "grok", "opencode"] {
            let specs = provider_file_specs(&configuration(family, &root), None).unwrap();
            for spec in &specs {
                assert_eq!(
                    spec.preferences.is_some(),
                    spec.kind == ProviderFileKind::Configuration,
                    "{family} {}",
                    spec.file_id
                );
            }
        }
    }

    #[test]
    fn opencode_honors_its_config_directory_and_explicit_config_file() {
        let root = TestDirectory::new("opencode");
        let mut configured_home = configuration("opencode", root.path());
        let specs = provider_file_specs(&configured_home, None).unwrap();
        assert!(
            specs
                .iter()
                .any(|spec| spec.path == root.path().join("opencode.json"))
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.path == root.path().join("AGENTS.md"))
        );

        let custom = root.path().join("custom.jsonc");
        configured_home.environment.insert(
            "OPENCODE_CONFIG".to_string(),
            custom.to_string_lossy().into_owned(),
        );
        let specs = provider_file_specs(&configured_home, None).unwrap();
        let config = specs
            .iter()
            .find(|spec| spec.file_id == "custom_configuration")
            .unwrap();
        assert_eq!(config.path, custom);
        assert_eq!(config.format, ProviderFileFormat::Jsonc);
    }

    #[test]
    fn read_and_write_preserve_revisions_and_missing_state() {
        let root = TestDirectory::new("round-trip");
        let spec = configuration_spec(
            "configuration",
            root.path().join("config.toml"),
            ProviderFileFormat::Toml,
        );
        let missing = read_provider_file(spec.clone()).unwrap();
        assert!(!missing.exists);
        write_text_file_atomically(&spec.path, "approval_policy = \"on-request\"\n", true).unwrap();
        let saved = read_provider_file(spec).unwrap();
        assert!(saved.exists);
        assert_eq!(saved.contents, "approval_policy = \"on-request\"\n");
        assert_ne!(saved.revision, missing.revision);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_parent_directories_are_rejected() {
        use std::os::unix::fs::symlink;

        let root = TestDirectory::new("symlink");
        let real = root.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let linked = root.path().join("linked");
        symlink(&real, &linked).unwrap();
        let error = reject_symbolic_path(&linked.join("config.toml")).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
    }

    #[test]
    fn opencode_config_must_name_a_json_file() {
        let root = TestDirectory::new("opencode-target");
        let mut configuration = configuration("opencode", root.path());
        for target in [
            "~/.ssh/id_ed25519",
            "~/.bashrc",
            "/etc/passwd",
            "relative.json",
        ] {
            configuration
                .environment
                .insert("OPENCODE_CONFIG".to_string(), target.to_string());
            let error = provider_file_specs(&configuration, None).unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Validation, "{target}");
        }
        configuration.environment.insert(
            "OPENCODE_CONFIG".to_string(),
            "~/work/opencode.JSON".to_string(),
        );
        assert!(provider_file_specs(&configuration, None).is_ok());
    }

    #[test]
    fn custom_config_saves_never_create_missing_folders() {
        let root = TestDirectory::new("missing-parent");
        let mut opencode = configuration("opencode", root.path());
        let target = root.path().join("absent").join("opencode.json");
        opencode.environment.insert(
            "OPENCODE_CONFIG".to_string(),
            target.to_string_lossy().into_owned(),
        );
        let spec = provider_file_specs(&opencode, None)
            .unwrap()
            .into_iter()
            .find(|spec| spec.file_id == "custom_configuration")
            .unwrap();
        assert!(!spec.create_parent);
        let error = write_text_file_atomically(&spec.path, "{}", spec.create_parent).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert!(!root.path().join("absent").exists());

        let home_spec =
            provider_file_specs(&configuration("codex", &root.path().join("new-home")), None)
                .unwrap()
                .remove(0);
        write_text_file_atomically(&home_spec.path, "", home_spec.create_parent).unwrap();
        assert!(home_spec.path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(root.path().join("new-home"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_parents_are_rejected_before_any_folder_is_created() {
        use std::os::unix::fs::symlink;

        let root = TestDirectory::new("symlink-write");
        let real = root.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let linked = root.path().join("linked");
        symlink(&real, &linked).unwrap();
        let target = linked.join("nested").join("config.toml");
        let error = write_text_file_atomically(&target, "x", true).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert!(!real.join("nested").exists());
    }

    #[test]
    fn display_paths_hide_the_home_folder() {
        let inside = user_home().join(".codex").join("config.toml");
        let shown = display_path(&inside);
        assert!(shown.starts_with('~'));
        assert!(!shown.contains(user_home().to_string_lossy().as_ref()));
        assert!(shown.ends_with("config.toml"));
    }

    fn note_kinds(read: &ProviderFilesRead) -> Vec<ProviderFileNoteKind> {
        read.notes.iter().map(|note| note.kind).collect()
    }

    #[test]
    fn codex_shows_its_override_file_and_notes_it_only_while_it_has_content() {
        let root = TestDirectory::new("codex-override");
        let codex = configuration("codex", root.path());
        let read = read_provider_files(&codex, None, root.path()).unwrap();
        assert!(
            read.files
                .iter()
                .all(|file| file.file_id != "instructions_override")
        );
        assert!(read.notes.is_empty());

        fs::write(root.path().join("AGENTS.override.md"), "  \n").unwrap();
        let read = read_provider_files(&codex, None, root.path()).unwrap();
        assert!(
            read.files
                .iter()
                .any(|file| file.file_id == "instructions_override")
        );
        assert!(read.notes.is_empty());

        fs::write(root.path().join("AGENTS.override.md"), "Use pnpm.").unwrap();
        let read = read_provider_files(&codex, None, root.path()).unwrap();
        assert_eq!(note_kinds(&read), [ProviderFileNoteKind::CodexOverride]);
        assert!(read.notes[0].paths[0].ends_with("AGENTS.override.md"));
    }

    #[test]
    fn grok_notes_name_the_replaced_home_its_rules_and_skipped_vendor_homes() {
        let root = TestDirectory::new("grok-notes");
        let own_home = root.path().join("grok");
        let private = root.path().join("private");
        let user_home = root.path().join("user");
        fs::create_dir_all(private.join("rules")).unwrap();
        fs::create_dir_all(user_home.join(".claude")).unwrap();
        fs::create_dir_all(user_home.join(".cursor").join("rules")).unwrap();
        fs::write(user_home.join(".claude").join("CLAUDE.md"), "rules").unwrap();
        let grok = configuration("grok", &own_home);

        let read = read_provider_files(&grok, Some(&private), &user_home).unwrap();
        assert_eq!(
            note_kinds(&read),
            [
                ProviderFileNoteKind::GrokPrivateHome,
                ProviderFileNoteKind::GrokRules,
                ProviderFileNoteKind::GrokOtherHomeInstructions,
            ]
        );
        assert_eq!(read.notes[0].paths, [display_path(&own_home)]);
        assert_eq!(read.notes[2].paths.len(), 2);

        let empty_user_home = root.path().join("empty");
        let read = read_provider_files(&grok, None, &empty_user_home).unwrap();
        assert!(read.notes.is_empty());
    }

    #[test]
    fn opencode_notes_follow_the_claude_fallback_and_listed_instructions() {
        let root = TestDirectory::new("opencode-notes");
        let user_home = root.path().join("user");
        fs::create_dir_all(user_home.join(".claude")).unwrap();
        fs::write(user_home.join(".claude").join("CLAUDE.md"), "rules").unwrap();
        let home = root.path().join("opencode");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join("opencode.json"),
            r#"{"instructions": ["docs/rules.md"]}"#,
        )
        .unwrap();
        let mut opencode = configuration("opencode", &home);
        opencode.environment.insert(
            "XDG_CONFIG_HOME".to_string(),
            root.path().to_string_lossy().into_owned(),
        );

        let read = read_provider_files(&opencode, None, &user_home).unwrap();
        assert_eq!(
            note_kinds(&read),
            [
                ProviderFileNoteKind::OpencodeClaudeFallback,
                ProviderFileNoteKind::OpencodeInstructions,
            ]
        );

        opencode.environment.insert(
            "OPENCODE_DISABLE_CLAUDE_CODE_PROMPT".to_string(),
            "true".to_string(),
        );
        fs::write(home.join("opencode.json"), r#"{"instructions": []}"#).unwrap();
        let read = read_provider_files(&opencode, None, &user_home).unwrap();
        assert!(read.notes.is_empty());

        opencode
            .environment
            .remove("OPENCODE_DISABLE_CLAUDE_CODE_PROMPT");
        fs::write(home.join("AGENTS.md"), "rules").unwrap();
        let read = read_provider_files(&opencode, None, &user_home).unwrap();
        assert!(read.notes.is_empty());
    }

    #[test]
    fn cursor_always_notes_account_rules_and_claude_notes_an_existing_rules_folder() {
        let root = TestDirectory::new("vendor-notes");
        let mut cursor = configuration("cursor", root.path());
        cursor.environment.insert(
            "CURSOR_CONFIG_DIR".to_string(),
            root.path().to_string_lossy().into_owned(),
        );
        let read = read_provider_files(&cursor, None, root.path()).unwrap();
        assert_eq!(note_kinds(&read), [ProviderFileNoteKind::CursorUserRules]);

        let claude = configuration("claude", root.path());
        assert!(
            read_provider_files(&claude, None, root.path())
                .unwrap()
                .notes
                .is_empty()
        );
        fs::create_dir_all(root.path().join("rules")).unwrap();
        let read = read_provider_files(&claude, None, root.path()).unwrap();
        assert_eq!(note_kinds(&read), [ProviderFileNoteKind::ClaudeRules]);
    }

    #[test]
    fn revisions_distinguish_missing_and_empty_files() {
        assert_ne!(revision("", false), revision("", true));
        assert_ne!(revision("first", true), revision("second", true));
    }
}
