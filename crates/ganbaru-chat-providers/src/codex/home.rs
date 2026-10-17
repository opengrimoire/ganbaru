//! Codex executable, environment, home, and continuation identity handling.

use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ContinuationGroupId, ProviderInstanceConfig,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_CUSTOM_MODELS: usize = 128;
const MAX_CUSTOM_MODEL_BYTES: usize = 256;
const MAX_CUSTOM_MODEL_LABEL_BYTES: usize = 160;
#[cfg(windows)]
const MAX_SHIM_BYTES: u64 = 64 * 1024;
const SHARED_DIRECTORY_NAMES: &[&str] = &[
    "sessions",
    "archived_sessions",
    "sqlite",
    "shell_snapshots",
    "worktrees",
    "skills",
    "plugins",
    "cache",
    "logs",
    "mcp-oauth-locks",
];
const PRIVATE_ENTRY_NAMES: &[&str] = &["auth.json", "models_cache.json"];
const SHADOW_LOCAL_ENTRY_NAMES: &[&str] = &["log", "memories", "tmp"];

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CodexProviderSettings {
    pub shadow_home_path: Option<String>,
    pub refresh_mcp_before_turn: bool,
    pub allow_custom_models: bool,
    pub custom_model_ids: Vec<String>,
    pub custom_model_labels: BTreeMap<String, String>,
}

impl CodexProviderSettings {
    pub fn parse(configuration: &ProviderInstanceConfig) -> ChatResult<Self> {
        if configuration.provider_config.schema_version != 1 {
            return Err(ChatError::validation(
                "providerConfig.schemaVersion",
                "Codex provider config schema is unsupported",
            ));
        }
        let settings: Self = serde_json::from_value(configuration.provider_config.value.clone())
            .map_err(|_| {
                ChatError::validation("providerConfig", "Codex provider config is invalid")
            })?;
        if settings.custom_model_ids.len() > MAX_CUSTOM_MODELS
            || settings.custom_model_ids.iter().any(|model| {
                model.trim().is_empty()
                    || model.len() > MAX_CUSTOM_MODEL_BYTES
                    || model.chars().any(char::is_control)
            })
        {
            return Err(ChatError::validation(
                "providerConfig.customModelIds",
                "Codex custom model configuration is invalid",
            ));
        }
        if !settings.allow_custom_models && !settings.custom_model_ids.is_empty() {
            return Err(ChatError::validation(
                "providerConfig.customModelIds",
                "Codex custom models require the advanced custom-model option",
            ));
        }
        let custom_ids = settings.custom_model_ids.iter().collect::<BTreeSet<_>>();
        if settings
            .custom_model_labels
            .iter()
            .any(|(model_id, label)| {
                !custom_ids.contains(model_id)
                    || label.trim().is_empty()
                    || label.len() > MAX_CUSTOM_MODEL_LABEL_BYTES
                    || label.chars().any(char::is_control)
            })
        {
            return Err(ChatError::validation(
                "providerConfig.customModelLabels",
                "Codex custom model labels are invalid",
            ));
        }
        if settings
            .shadow_home_path
            .as_deref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err(ChatError::validation(
                "providerConfig.shadowHomePath",
                "Codex shadow home path cannot be empty",
            ));
        }
        Ok(settings)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexHomeLayout {
    pub shared_home: PathBuf,
    pub effective_home: PathBuf,
    pub shadowed: bool,
}

impl CodexHomeLayout {
    pub fn continuation_group_with_authority(
        &self,
        organizational_authority: bool,
    ) -> ChatResult<ContinuationGroupId> {
        let mut digest = Sha256::new();
        digest.update(b"ganbaru-chat-codex-home-v2\0");
        digest.update(path_identity_bytes(&self.shared_home));
        digest.update([u8::from(organizational_authority)]);
        ContinuationGroupId::new(format!("codex-home-{:x}", digest.finalize()))
            .map_err(identifier_error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCodexExecutable {
    pub executable: PathBuf,
    pub prefix_arguments: Vec<String>,
}

pub fn resolve_codex_home_layout(
    configuration: &ProviderInstanceConfig,
    settings: &CodexProviderSettings,
) -> ChatResult<CodexHomeLayout> {
    let configured_shared = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("CODEX_HOME").ok())
        .unwrap_or_else(default_codex_home);
    let shared_home = resolve_absolute_path(&configured_shared, "providerHome")?;
    if !shared_home.is_dir() {
        return Err(ChatError::validation(
            "providerHome",
            "Codex home must be an existing directory",
        ));
    }
    let shared_home = fs::canonicalize(&shared_home).map_err(|_| {
        ChatError::validation("providerHome", "Codex home could not be canonicalized")
    })?;
    let Some(shadow_home) = settings.shadow_home_path.as_deref() else {
        return Ok(CodexHomeLayout {
            effective_home: shared_home.clone(),
            shared_home,
            shadowed: false,
        });
    };
    let effective_home = resolve_absolute_path(shadow_home, "providerConfig.shadowHomePath")?;
    if paths_resolve_equal(&shared_home, &effective_home) {
        return Err(ChatError::validation(
            "providerConfig.shadowHomePath",
            "Codex shadow home must differ from the shared home",
        ));
    }
    Ok(CodexHomeLayout {
        shared_home,
        effective_home,
        shadowed: true,
    })
}

pub fn materialize_codex_shadow_home(layout: &mut CodexHomeLayout) -> ChatResult<()> {
    if !layout.shadowed {
        return Ok(());
    }
    fs::create_dir_all(&layout.effective_home).map_err(|_| shadow_home_error())?;
    layout.effective_home = fs::canonicalize(&layout.effective_home).map_err(|_| {
        ChatError::validation(
            "providerConfig.shadowHomePath",
            "Codex shadow home could not be canonicalized",
        )
    })?;
    if layout.shared_home == layout.effective_home {
        return Err(ChatError::validation(
            "providerConfig.shadowHomePath",
            "Codex shadow home resolves to the shared home",
        ));
    }
    for directory in SHARED_DIRECTORY_NAMES {
        fs::create_dir_all(layout.shared_home.join(directory)).map_err(|_| shadow_home_error())?;
    }
    for private in PRIVATE_ENTRY_NAMES {
        reject_symlink(&layout.effective_home.join(private), private)?;
    }

    let mut entries = SHARED_DIRECTORY_NAMES
        .iter()
        .map(|entry| (*entry).to_string())
        .collect::<BTreeSet<_>>();
    for entry in fs::read_dir(&layout.shared_home).map_err(|_| shadow_home_error())? {
        let entry = entry.map_err(|_| shadow_home_error())?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !PRIVATE_ENTRY_NAMES.contains(&name.as_str())
            && !SHADOW_LOCAL_ENTRY_NAMES.contains(&name.as_str())
        {
            entries.insert(name);
        }
    }
    for entry in entries {
        ensure_shared_link(layout, &entry)?;
    }
    verify_codex_shadow_home(layout)
}

pub fn verify_codex_shadow_home(layout: &CodexHomeLayout) -> ChatResult<()> {
    if !layout.shadowed {
        return Ok(());
    }
    for private in PRIVATE_ENTRY_NAMES {
        reject_symlink(&layout.effective_home.join(private), private)?;
    }
    for directory in SHARED_DIRECTORY_NAMES {
        verify_shared_link(layout, directory)?;
    }
    Ok(())
}

pub fn codex_process_environment(
    configuration: &ProviderInstanceConfig,
    layout: &CodexHomeLayout,
) -> ChatResult<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for name in minimal_inherited_environment_names() {
        if let Ok(value) = std::env::var(name) {
            if !value.contains('\0') {
                environment.insert((*name).to_string(), value);
            }
        }
    }
    if let Some(home) = platform_user_home() {
        append_fallback_executable_directories(&mut environment, &home);
    }
    for (name, value) in &configuration.environment {
        if name.is_empty()
            || name.contains(['=', '\0'])
            || value.contains('\0')
            || name.eq_ignore_ascii_case("CODEX_HOME")
        {
            return Err(ChatError::validation(
                "environment",
                "Codex environment contains a prohibited entry",
            ));
        }
        environment.insert(name.clone(), value.clone());
    }
    environment.insert(
        "CODEX_HOME".to_string(),
        layout.effective_home.to_string_lossy().into_owned(),
    );
    Ok(environment)
}

fn platform_user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(super) fn append_fallback_executable_directories(
    environment: &mut BTreeMap<String, String>,
    home: &Path,
) {
    let path_key = environment
        .keys()
        .find(|key| key.eq_ignore_ascii_case("PATH"))
        .cloned()
        .unwrap_or_else(|| "PATH".to_string());
    let mut directories = environment
        .get(&path_key)
        .map(|value| std::env::split_paths(OsStr::new(value)).collect::<Vec<_>>())
        .unwrap_or_default();
    for candidate in fallback_executable_directories(home) {
        if candidate.is_dir() && !directories.iter().any(|entry| entry == &candidate) {
            directories.push(candidate);
        }
    }
    let Ok(joined) = std::env::join_paths(directories) else {
        return;
    };
    if let Some(joined) = joined.to_str() {
        environment.insert(path_key, joined.to_string());
    }
}

fn fallback_executable_directories(home: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            home.join("AppData").join("Roaming").join("npm"),
            home.join("AppData").join("Local").join("pnpm"),
            home.join(".bun").join("bin"),
            home.join(".cargo").join("bin"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![
            home.join(".local").join("bin"),
            home.join(".local").join("share").join("pnpm"),
            home.join(".npm-global").join("bin"),
            home.join(".bun").join("bin"),
            home.join(".cargo").join("bin"),
            home.join(".volta").join("bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
        ]
    }
}

pub fn resolve_codex_executable(
    configured: &str,
    environment: &BTreeMap<String, String>,
) -> ChatResult<ResolvedCodexExecutable> {
    let configured = configured.trim();
    if configured.is_empty() || configured.contains('\0') {
        return Err(ChatError::validation(
            "executable",
            "Codex executable is required",
        ));
    }
    let candidate = if path_has_separator(configured) {
        resolve_absolute_path(configured, "executable")?
    } else {
        find_on_path(configured, environment).ok_or_else(executable_missing_error)?
    };
    let candidate = fs::canonicalize(candidate).map_err(|_| executable_missing_error())?;
    if !candidate.is_file() || !is_executable(&candidate) {
        return Err(executable_missing_error());
    }
    resolve_windows_command_shim(candidate, environment)
}

fn ensure_shared_link(layout: &CodexHomeLayout, entry: &str) -> ChatResult<()> {
    let target = layout.shared_home.join(entry);
    let link = layout.effective_home.join(entry);
    match fs::symlink_metadata(&link) {
        Ok(metadata) => {
            if !metadata.file_type().is_symlink() {
                return Err(ChatError::validation(
                    "providerConfig.shadowHomePath",
                    format!("Codex shadow entry '{entry}' already contains local data"),
                ));
            }
            verify_link_target(&link, &target, entry)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_symlink(&target, &link).map_err(|_| shadow_home_error())
        }
        Err(_) => Err(shadow_home_error()),
    }
}

fn verify_shared_link(layout: &CodexHomeLayout, entry: &str) -> ChatResult<()> {
    verify_link_target(
        &layout.effective_home.join(entry),
        &layout.shared_home.join(entry),
        entry,
    )
}

fn verify_link_target(link: &Path, target: &Path, entry: &str) -> ChatResult<()> {
    let metadata = fs::symlink_metadata(link).map_err(|_| shadow_home_error())?;
    if !metadata.file_type().is_symlink() {
        return Err(ChatError::validation(
            "providerConfig.shadowHomePath",
            format!("Codex shadow entry '{entry}' is not a shared link"),
        ));
    }
    let existing_target = fs::read_link(link).map_err(|_| shadow_home_error())?;
    let resolved = if existing_target.is_absolute() {
        existing_target
    } else {
        link.parent()
            .unwrap_or_else(|| Path::new(""))
            .join(existing_target)
    };
    let resolved = fs::canonicalize(resolved).map_err(|_| shadow_home_error())?;
    let target = fs::canonicalize(target).map_err(|_| shadow_home_error())?;
    if resolved != target {
        return Err(ChatError::validation(
            "providerConfig.shadowHomePath",
            format!("Codex shadow entry '{entry}' points to a different location"),
        ));
    }
    Ok(())
}

fn reject_symlink(path: &Path, entry: &str) -> ChatResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ChatError::validation(
            "providerConfig.shadowHomePath",
            format!("Codex private shadow entry '{entry}' cannot be a symlink"),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(shadow_home_error()),
    }
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(not(any(unix, windows)))]
fn create_symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlinks are unsupported",
    ))
}

fn resolve_absolute_path(value: &str, field: &str) -> ChatResult<PathBuf> {
    let expanded = expand_user_path(value)?;
    if !expanded.is_absolute()
        || expanded
            .components()
            .any(|part| part == Component::ParentDir)
    {
        return Err(ChatError::validation(field, "path must be absolute"));
    }
    Ok(expanded)
}

fn expand_user_path(value: &str) -> ChatResult<PathBuf> {
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = user_home().ok_or_else(|| {
            ChatError::validation("path", "user home directory could not be resolved")
        })?;
        if value.len() == 1 {
            return Ok(home);
        }
        return Ok(home.join(&value[2..]));
    }
    if value.starts_with('~') {
        return Err(ChatError::validation(
            "path",
            "named-user home expansion is unsupported",
        ));
    }
    Ok(PathBuf::from(value))
}

fn default_codex_home() -> String {
    user_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .to_string_lossy()
        .into_owned()
}

fn user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    let names = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let names = ["HOME", "USERPROFILE"];
    names
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

fn paths_resolve_equal(left: &Path, right: &Path) -> bool {
    fs::canonicalize(right)
        .map(|right| right == left)
        .unwrap_or_else(|_| right == left)
}

fn path_identity_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().to_lowercase().into_bytes()
    }
}

fn minimal_inherited_environment_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "WINDIR",
            "USERPROFILE",
            "TEMP",
            "TMP",
        ]
    }
    #[cfg(not(windows))]
    {
        &["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"]
    }
}

fn path_has_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || Path::new(value).is_absolute()
}

fn find_on_path(name: &str, environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = environment
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))?
        .1
        .clone();
    let extensions = executable_extensions(name, environment);
    for directory in std::env::split_paths(OsStr::new(&path)) {
        for extension in &extensions {
            let candidate = directory.join(format!("{name}{extension}"));
            if candidate.is_file() && is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(windows)]
fn executable_extensions(name: &str, environment: &BTreeMap<String, String>) -> Vec<String> {
    if Path::new(name).extension().is_some() {
        return vec![String::new()];
    }
    environment
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATHEXT"))
        .map(|(_, value)| {
            value
                .split(';')
                .map(|value| value.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_else(|| vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()])
}

#[cfg(not(windows))]
fn executable_extensions(_name: &str, _environment: &BTreeMap<String, String>) -> Vec<String> {
    vec![String::new()]
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(windows)]
fn resolve_windows_command_shim(
    candidate: PathBuf,
    environment: &BTreeMap<String, String>,
) -> ChatResult<ResolvedCodexExecutable> {
    let extension = candidate
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "cmd" && extension != "bat" {
        return Ok(ResolvedCodexExecutable {
            executable: candidate,
            prefix_arguments: Vec::new(),
        });
    }
    let metadata = candidate
        .metadata()
        .map_err(|_| executable_missing_error())?;
    if metadata.len() > MAX_SHIM_BYTES {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Codex Windows command shim is too large to inspect safely",
            true,
        ));
    }
    let source = fs::read_to_string(&candidate).map_err(|_| executable_missing_error())?;
    let marker = "node_modules\\@openai\\codex\\bin\\codex.js";
    let lower = source.to_ascii_lowercase();
    let end = lower.find(marker).ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Codex Windows shim does not reference the official Codex entry point",
            true,
        )
    })? + marker.len();
    let start = source[..end]
        .rfind(['\"', ' '])
        .map(|index| index + 1)
        .unwrap_or(0);
    let raw_entry = source[start..end].trim_matches(['\"', '\'', ' ']);
    let parent = candidate.parent().ok_or_else(executable_missing_error)?;
    let entry = raw_entry
        .replace("%~dp0", &format!("{}\\", parent.to_string_lossy()))
        .replace('/', "\\");
    let entry = fs::canonicalize(entry).map_err(|_| executable_missing_error())?;
    let sibling_node = parent.join("node.exe");
    let node = if sibling_node.is_file() {
        sibling_node
    } else {
        find_on_path("node", environment).ok_or_else(executable_missing_error)?
    };
    Ok(ResolvedCodexExecutable {
        executable: fs::canonicalize(node).map_err(|_| executable_missing_error())?,
        prefix_arguments: vec![entry.to_string_lossy().into_owned()],
    })
}

#[cfg(not(windows))]
fn resolve_windows_command_shim(
    candidate: PathBuf,
    _environment: &BTreeMap<String, String>,
) -> ChatResult<ResolvedCodexExecutable> {
    Ok(ResolvedCodexExecutable {
        executable: candidate,
        prefix_arguments: Vec::new(),
    })
}

fn executable_missing_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ExecutableMissing,
        "Codex executable could not be resolved",
        true,
    )
}

fn shadow_home_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "Codex shadow home could not be prepared safely",
        true,
    )
}

fn identifier_error(_error: String) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Codex continuation identity could not be created",
        false,
    )
}
