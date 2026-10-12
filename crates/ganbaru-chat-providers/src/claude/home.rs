//! Claude executable, configuration directory, environment, and continuation identity.

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
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_MODEL_LABEL_BYTES: usize = 160;
#[cfg(windows)]
const MAX_SHIM_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaudeProviderSettings {
    pub allow_custom_models: bool,
    pub custom_model_ids: Vec<String>,
    pub custom_model_labels: BTreeMap<String, String>,
}

impl ClaudeProviderSettings {
    pub fn parse(configuration: &ProviderInstanceConfig) -> ChatResult<Self> {
        if configuration.provider_config.schema_version != 1 {
            return Err(ChatError::validation(
                "providerConfig.schemaVersion",
                "Claude provider config schema is unsupported",
            ));
        }
        let settings: Self = serde_json::from_value(configuration.provider_config.value.clone())
            .map_err(|_| {
                ChatError::validation("providerConfig", "Claude provider config is invalid")
            })?;
        if settings.custom_model_ids.len() > MAX_CUSTOM_MODELS
            || settings.custom_model_ids.iter().any(|model| {
                model.trim().is_empty()
                    || model.len() > MAX_MODEL_ID_BYTES
                    || model.chars().any(char::is_control)
            })
        {
            return Err(ChatError::validation(
                "providerConfig.customModelIds",
                "Claude custom model configuration is invalid",
            ));
        }
        if !settings.allow_custom_models && !settings.custom_model_ids.is_empty() {
            return Err(ChatError::validation(
                "providerConfig.customModelIds",
                "Claude custom models require the advanced custom-model option",
            ));
        }
        let ids = settings.custom_model_ids.iter().collect::<BTreeSet<_>>();
        if settings.custom_model_labels.iter().any(|(id, label)| {
            !ids.contains(id)
                || label.trim().is_empty()
                || label.len() > MAX_MODEL_LABEL_BYTES
                || label.chars().any(char::is_control)
        }) {
            return Err(ChatError::validation(
                "providerConfig.customModelLabels",
                "Claude custom model labels are invalid",
            ));
        }
        Ok(settings)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeHome {
    pub config_directory: PathBuf,
}

impl ClaudeHome {
    pub fn continuation_group(&self) -> ChatResult<ContinuationGroupId> {
        let mut digest = Sha256::new();
        digest.update(b"ganbaru-chat-claude-config-v1\0");
        digest.update(path_identity_bytes(&self.config_directory));
        ContinuationGroupId::new(format!("claude-config-{:x}", digest.finalize()))
            .map_err(|_| internal_identity_error())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedClaudeExecutable {
    pub executable: PathBuf,
    pub prefix_arguments: Vec<String>,
}

pub fn resolve_claude_home(configuration: &ProviderInstanceConfig) -> ChatResult<ClaudeHome> {
    let configured = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("CLAUDE_CONFIG_DIR").ok())
        .unwrap_or_else(default_claude_home);
    let path = resolve_absolute_path(&configured, "providerHome")?;
    let config_directory = fs::canonicalize(&path).unwrap_or(path);
    Ok(ClaudeHome { config_directory })
}

pub fn claude_process_environment(
    configuration: &ProviderInstanceConfig,
    home: &ClaudeHome,
) -> ChatResult<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for name in minimal_inherited_environment_names() {
        if let Ok(value) = std::env::var(name) {
            if !value.contains('\0') {
                environment.insert((*name).to_string(), value);
            }
        }
    }
    for (name, value) in &configuration.environment {
        if name.is_empty()
            || name.contains(['=', '\0'])
            || value.contains('\0')
            || name.eq_ignore_ascii_case("CLAUDE_CONFIG_DIR")
            || name.eq_ignore_ascii_case("HOME")
            || name.eq_ignore_ascii_case("USERPROFILE")
        {
            return Err(ChatError::validation(
                "environment",
                "Claude environment contains a prohibited entry",
            ));
        }
        environment.insert(name.clone(), value.clone());
    }
    environment.insert(
        "CLAUDE_CONFIG_DIR".to_string(),
        home.config_directory.to_string_lossy().into_owned(),
    );
    environment.insert(
        "CLAUDE_CODE_ENTRYPOINT".to_string(),
        "ganbaru-ai".to_string(),
    );
    Ok(environment)
}

pub fn resolve_claude_executable(
    configured: &str,
    environment: &BTreeMap<String, String>,
) -> ChatResult<ResolvedClaudeExecutable> {
    let configured = configured.trim();
    if configured.is_empty() || configured.contains('\0') {
        return Err(ChatError::validation(
            "executable",
            "Claude executable is required",
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
        return Ok(if value.len() == 1 {
            home
        } else {
            home.join(&value[2..])
        });
    }
    if value.starts_with('~') {
        return Err(ChatError::validation(
            "path",
            "named-user home expansion is unsupported",
        ));
    }
    Ok(PathBuf::from(value))
}

fn default_claude_home() -> String {
    user_home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
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
    for directory in std::env::split_paths(OsStr::new(&path)) {
        for extension in executable_extensions(name, environment) {
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
        .map(|(_, value)| value.split(';').map(str::to_ascii_lowercase).collect())
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
) -> ChatResult<ResolvedClaudeExecutable> {
    let extension = candidate
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "cmd" && extension != "bat" && extension != "ps1" {
        return Ok(ResolvedClaudeExecutable {
            executable: candidate,
            prefix_arguments: Vec::new(),
        });
    }
    if candidate
        .metadata()
        .map_err(|_| executable_missing_error())?
        .len()
        > MAX_SHIM_BYTES
    {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Claude Windows command shim is too large to inspect safely",
            true,
        ));
    }
    let parent = candidate.parent().ok_or_else(executable_missing_error)?;
    let candidates = [
        parent.join("node_modules/@anthropic-ai/claude-code/bin/claude.exe"),
        parent.join("node_modules/@anthropic-ai/claude-code/cli.js"),
    ];
    for entry in candidates {
        if !entry.is_file() {
            continue;
        }
        let entry = fs::canonicalize(entry).map_err(|_| executable_missing_error())?;
        if entry.extension().and_then(OsStr::to_str) == Some("exe") {
            return Ok(ResolvedClaudeExecutable {
                executable: entry,
                prefix_arguments: Vec::new(),
            });
        }
        let sibling_node = parent.join("node.exe");
        let node = if sibling_node.is_file() {
            sibling_node
        } else {
            find_on_path("node", environment).ok_or_else(executable_missing_error)?
        };
        return Ok(ResolvedClaudeExecutable {
            executable: fs::canonicalize(node).map_err(|_| executable_missing_error())?,
            prefix_arguments: vec![entry.to_string_lossy().into_owned()],
        });
    }
    Err(ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        "Claude Windows shim does not resolve to an official package entry",
        true,
    ))
}

#[cfg(not(windows))]
fn resolve_windows_command_shim(
    candidate: PathBuf,
    _environment: &BTreeMap<String, String>,
) -> ChatResult<ResolvedClaudeExecutable> {
    Ok(ResolvedClaudeExecutable {
        executable: candidate,
        prefix_arguments: Vec::new(),
    })
}

fn executable_missing_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ExecutableMissing,
        "Claude executable could not be resolved",
        true,
    )
}

fn internal_identity_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Claude continuation identity could not be created",
        false,
    )
}
