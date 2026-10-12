//! OpenCode executable probes and bounded catalog parsers.

use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ModelAvailability, ModelChoiceOption,
    ModelOptionDefinition, ProviderCapability, ProviderInstanceConfig, ProviderModel,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

pub const MINIMUM_OPENCODE_VERSION: &str = "1.14.19";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_COMMAND_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_MODELS: usize = 2_048;
const MAX_AGENTS: usize = 256;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_LABEL_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OpenCodeVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl std::fmt::Display for OpenCodeVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedOpenCodeExecutable {
    pub executable: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenCodeCatalogModel {
    pub provider_id: String,
    pub model_id: String,
    pub name: String,
    pub context_limit: Option<u64>,
    pub variants: Vec<String>,
    pub availability: ModelAvailability,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeAgent {
    pub name: String,
    pub mode: String,
    pub hidden: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeCommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub successful: bool,
}

pub fn process_environment(
    configuration: &ProviderInstanceConfig,
) -> ChatResult<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for name in inherited_environment_names() {
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
            || matches!(name.to_ascii_uppercase().as_str(), "HOME" | "USERPROFILE")
        {
            return Err(ChatError::validation(
                "environment",
                "OpenCode environment contains a prohibited entry",
            ));
        }
        environment.insert(name.clone(), value.clone());
    }
    if let Some(home) = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = Path::new(home);
        if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
            return Err(ChatError::validation(
                "providerHome",
                "OpenCode config directory must be an absolute path",
            ));
        }
        environment.insert("OPENCODE_CONFIG_DIR".to_string(), home.to_string());
    }
    environment.insert("OPENCODE_CLIENT".to_string(), "ganbaru-ai".to_string());
    Ok(environment)
}

pub fn resolve_executable(
    configured: &str,
    environment: &BTreeMap<String, String>,
) -> ChatResult<ResolvedOpenCodeExecutable> {
    let configured = configured.trim();
    if configured.is_empty() || configured.contains('\0') {
        return Err(ChatError::validation(
            "executable",
            "OpenCode executable is required",
        ));
    }
    let candidate = if path_has_separator(configured) {
        let path = PathBuf::from(configured);
        if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
            return Err(ChatError::validation(
                "executable",
                "OpenCode executable path must be absolute",
            ));
        }
        path
    } else {
        find_on_path(configured, environment).ok_or_else(executable_missing_error)?
    };
    let executable = fs::canonicalize(candidate).map_err(|_| executable_missing_error())?;
    if !executable.is_file() || !is_executable(&executable) {
        return Err(executable_missing_error());
    }
    if matches!(
        executable.extension().and_then(OsStr::to_str),
        Some("cmd" | "bat" | "ps1")
    ) {
        return Err(ChatError::unsupported(
            "OpenCode command shims are unsupported; configure the native OpenCode executable",
        ));
    }
    Ok(ResolvedOpenCodeExecutable { executable })
}

pub async fn run_command(
    executable: &Path,
    arguments: &[&str],
    working_directory: &Path,
    environment: &BTreeMap<String, String>,
) -> ChatResult<OpenCodeCommandOutput> {
    let output = tokio::process::Command::new(executable)
        .args(arguments)
        .current_dir(working_directory)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(COMMAND_TIMEOUT, output)
        .await
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Timeout,
                "OpenCode command probe timed out",
                true,
            )
        })?
        .map_err(|_| executable_missing_error())?;
    if output.stdout.len() > MAX_COMMAND_OUTPUT_BYTES
        || output.stderr.len() > MAX_COMMAND_OUTPUT_BYTES
    {
        return Err(protocol_error("command output is oversized"));
    }
    Ok(OpenCodeCommandOutput {
        stdout: output.stdout,
        stderr: output.stderr,
        successful: output.status.success(),
    })
}

pub fn parse_version(value: &str) -> ChatResult<OpenCodeVersion> {
    let version = value
        .split_whitespace()
        .find_map(|token| {
            parse_version_token(token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '.' && character != '-'
            }))
        })
        .ok_or_else(|| protocol_error("version output is invalid"))?;
    Ok(version)
}

pub fn ensure_supported_version(version: OpenCodeVersion) -> ChatResult<()> {
    let minimum = parse_version(MINIMUM_OPENCODE_VERSION)?;
    if version < minimum {
        return Err(ChatError::new(
            ChatErrorCode::UnsupportedVersion,
            format!(
                "OpenCode {version} is unsupported; version {MINIMUM_OPENCODE_VERSION} or newer is required"
            ),
            false,
        ));
    }
    Ok(())
}

pub fn parse_models_cli_output(output: &[u8]) -> ChatResult<Vec<OpenCodeCatalogModel>> {
    if output.len() > MAX_COMMAND_OUTPUT_BYTES {
        return Err(protocol_error("model catalog is oversized"));
    }
    let output = String::from_utf8_lossy(output);
    let mut models = Vec::new();
    let mut slug: Option<&str> = None;
    let mut json_lines = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim_end_matches('\r');
        if is_valid_model_slug(trimmed) {
            flush_model(slug.take(), &mut json_lines, &mut models)?;
            slug = Some(trimmed);
        } else if slug.is_some() {
            json_lines.push(trimmed);
        }
    }
    flush_model(slug, &mut json_lines, &mut models)?;
    if models.len() > MAX_MODELS {
        return Err(protocol_error("model catalog has too many entries"));
    }
    models.sort_by(|left, right| {
        left.provider_id
            .cmp(&right.provider_id)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    models.dedup_by(|left, right| {
        left.provider_id == right.provider_id && left.model_id == right.model_id
    });
    Ok(models)
}

pub fn parse_agents_cli_output(output: &[u8]) -> ChatResult<Vec<OpenCodeAgent>> {
    if output.len() > MAX_COMMAND_OUTPUT_BYTES {
        return Err(protocol_error("agent catalog is oversized"));
    }
    let output = String::from_utf8_lossy(output);
    let mut agents = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim_end_matches('\r').trim();
        let Some((name, mode)) = parse_agent_header(trimmed) else {
            continue;
        };
        if agents.len() == MAX_AGENTS {
            return Err(protocol_error("agent catalog has too many entries"));
        }
        agents.push(OpenCodeAgent {
            hidden: matches!(name, "compaction" | "summary" | "title"),
            name: name.to_string(),
            mode: mode.to_string(),
        });
    }
    agents.sort_by(|left, right| left.name.cmp(&right.name));
    agents.dedup_by(|left, right| left.name == right.name);
    Ok(agents)
}

pub fn provider_models(
    models: &[OpenCodeCatalogModel],
    agents: &[OpenCodeAgent],
) -> ChatResult<Vec<ProviderModel>> {
    let agent_choices = agents
        .iter()
        .filter(|agent| !agent.hidden && matches!(agent.mode.as_str(), "primary" | "all"))
        .map(|agent| ModelChoiceOption {
            value: agent.name.clone(),
            label: title_case(&agent.name),
            description: None,
        })
        .collect::<Vec<_>>();
    models
        .iter()
        .map(|model| {
            let id =
                crate::models::ModelId::new(format!("{}/{}", model.provider_id, model.model_id))
                    .map_err(|_| protocol_error("model ID is invalid"))?;
            let mut options = Vec::new();
            if !model.variants.is_empty() {
                options.push(ModelOptionDefinition::Choice {
                    key: "variant".to_string(),
                    label: "Variant".to_string(),
                    description: Some("Provider-specific model reasoning variant".to_string()),
                    options: model
                        .variants
                        .iter()
                        .map(|variant| ModelChoiceOption {
                            value: variant.clone(),
                            label: title_case(variant),
                            description: None,
                        })
                        .collect(),
                    default_value: default_variant(&model.provider_id, &model.variants),
                });
            }
            if !agent_choices.is_empty() {
                options.push(ModelOptionDefinition::Choice {
                    key: "agent".to_string(),
                    label: "Agent".to_string(),
                    description: Some("OpenCode primary agent".to_string()),
                    options: agent_choices.clone(),
                    default_value: agent_choices
                        .iter()
                        .find(|choice| choice.value == "build")
                        .or_else(|| agent_choices.first())
                        .map(|choice| choice.value.clone()),
                });
            }
            Ok(ProviderModel {
                id,
                display_name: model.name.clone(),
                description: Some(model.provider_id.clone()),
                context_limit: model.context_limit,
                availability: model.availability,
                capabilities: vec![
                    ProviderCapability::FileReferences,
                    ProviderCapability::Approvals,
                    ProviderCapability::StructuredQuestions,
                    ProviderCapability::ReasoningSummaries,
                    ProviderCapability::ContextUsage,
                    ProviderCapability::CostReporting,
                ],
                options,
                custom: false,
            })
        })
        .collect()
}

pub fn parse_provider_inventory(value: &Value) -> ChatResult<Vec<OpenCodeCatalogModel>> {
    let providers = value
        .as_object()
        .and_then(|object| object.get("all"))
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_error("provider inventory is invalid"))?;
    if providers.len() > 256 {
        return Err(protocol_error("provider inventory has too many providers"));
    }
    let mut models = Vec::new();
    for provider in providers {
        let Some(provider) = provider.as_object() else {
            continue;
        };
        let Some(provider_id) = text(provider, "id").filter(|value| is_valid_identifier(value))
        else {
            continue;
        };
        let Some(provider_models) = provider.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (key, value) in provider_models {
            if models.len() == MAX_MODELS {
                return Err(protocol_error("provider inventory has too many models"));
            }
            let Some(model) = value.as_object() else {
                continue;
            };
            let model_id = text(model, "id").unwrap_or(key);
            if !is_valid_identifier(model_id) {
                continue;
            }
            let Some(name) = text(model, "name").filter(|value| is_valid_label(value)) else {
                continue;
            };
            models.push(OpenCodeCatalogModel {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
                name: name.to_string(),
                context_limit: model
                    .get("limit")
                    .and_then(Value::as_object)
                    .and_then(|limit| limit.get("context"))
                    .and_then(Value::as_u64),
                variants: parse_variants(model.get("variants")),
                availability: match text(model, "status") {
                    Some("deprecated") => ModelAvailability::Deprecated,
                    Some("unavailable") => ModelAvailability::Unavailable,
                    Some("active") | None => ModelAvailability::Available,
                    Some(_) => ModelAvailability::Unknown,
                },
            });
        }
    }
    models.sort_by(|left, right| {
        left.provider_id
            .cmp(&right.provider_id)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    models.dedup_by(|left, right| {
        left.provider_id == right.provider_id && left.model_id == right.model_id
    });
    Ok(models)
}

pub fn parse_agent_inventory(value: &Value) -> ChatResult<Vec<OpenCodeAgent>> {
    let entries = value
        .as_array()
        .ok_or_else(|| protocol_error("agent inventory is invalid"))?;
    if entries.len() > MAX_AGENTS {
        return Err(protocol_error("agent inventory has too many entries"));
    }
    let mut agents = entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|agent| {
            let name = text(agent, "name").filter(|value| is_valid_label(value))?;
            let mode = text(agent, "mode")?;
            if !matches!(mode, "primary" | "subagent" | "all") {
                return None;
            }
            Some(OpenCodeAgent {
                name: name.to_string(),
                mode: mode.to_string(),
                hidden: agent
                    .get("hidden")
                    .and_then(Value::as_bool)
                    .unwrap_or(matches!(name, "compaction" | "summary" | "title")),
            })
        })
        .collect::<Vec<_>>();
    agents.sort_by(|left, right| left.name.cmp(&right.name));
    agents.dedup_by(|left, right| left.name == right.name);
    Ok(agents)
}

fn flush_model(
    slug: Option<&str>,
    json_lines: &mut Vec<&str>,
    models: &mut Vec<OpenCodeCatalogModel>,
) -> ChatResult<()> {
    let Some(slug) = slug else {
        json_lines.clear();
        return Ok(());
    };
    let serialized = json_lines.join("\n");
    json_lines.clear();
    let Ok(value) = serde_json::from_str::<Value>(serialized.trim()) else {
        return Ok(());
    };
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    let Some((provider_id, model_id)) = slug.split_once('/') else {
        return Ok(());
    };
    if text(object, "providerID") != Some(provider_id) || text(object, "id") != Some(model_id) {
        return Ok(());
    }
    let Some(name) = text(object, "name").filter(|name| is_valid_label(name)) else {
        return Ok(());
    };
    let variants = parse_variants(object.get("variants"));
    let context_limit = object
        .get("limit")
        .and_then(Value::as_object)
        .and_then(|limit| limit.get("context"))
        .and_then(Value::as_u64);
    let availability = match text(object, "status") {
        Some("deprecated") => ModelAvailability::Deprecated,
        Some("unavailable") => ModelAvailability::Unavailable,
        Some("active") | None => ModelAvailability::Available,
        Some(_) => ModelAvailability::Unknown,
    };
    models.push(OpenCodeCatalogModel {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        name: name.to_string(),
        context_limit,
        variants,
        availability,
    });
    Ok(())
}

fn parse_variants(value: Option<&Value>) -> Vec<String> {
    let mut variants = value
        .and_then(Value::as_object)
        .map(|variants| {
            variants
                .keys()
                .filter(|key| is_valid_identifier(key))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    variants.sort();
    variants.dedup();
    variants
}

fn is_valid_model_slug(value: &str) -> bool {
    value.split_once('/').is_some_and(|(provider, model)| {
        is_valid_identifier(provider) && is_valid_identifier(model)
    })
}

fn parse_agent_header(value: &str) -> Option<(&str, &str)> {
    let prefix = value.strip_suffix(')')?;
    let (name, mode) = prefix.rsplit_once(" (")?;
    let name = name.trim();
    if !is_valid_label(name) || !matches!(mode, "primary" | "subagent" | "all") {
        return None;
    }
    Some((name, mode))
}

fn is_valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && !value.chars().any(|character| {
            character.is_control() || character.is_whitespace() || matches!(character, '/' | '\\')
        })
}

fn is_valid_label(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_LABEL_BYTES
        && !value.chars().any(|character| character.is_control())
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn title_case(value: &str) -> String {
    value
        .split(['-', '_', '/'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_variant(provider_id: &str, variants: &[String]) -> Option<String> {
    let preferred = if provider_id == "anthropic" || provider_id.starts_with("google") {
        "high"
    } else {
        "medium"
    };
    variants
        .iter()
        .find(|variant| variant.as_str() == preferred)
        .or_else(|| (variants.len() == 1).then(|| &variants[0]))
        .cloned()
}

fn parse_version_token(value: &str) -> Option<OpenCodeVersion> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let prefix = value.split('-').next()?;
    let mut parts = prefix.split('.');
    let version = OpenCodeVersion {
        major: parts.next()?.parse().ok()?,
        minor: parts.next()?.parse().ok()?,
        patch: parts.next()?.parse().ok()?,
    };
    parts.next().is_none().then_some(version)
}

fn path_has_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || Path::new(value).is_absolute()
}

fn find_on_path(name: &str, environment: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = environment
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))?
        .1
        .as_str();
    let mut extensions = executable_extensions(name, environment);
    let mut seen = BTreeSet::new();
    extensions.retain(|extension| seen.insert(extension.clone()));
    std::env::split_paths(path).find_map(|directory| {
        extensions.iter().find_map(|extension| {
            let candidate = directory.join(format!("{name}{extension}"));
            (candidate.is_file() && is_executable(&candidate)).then_some(candidate)
        })
    })
}

#[cfg(windows)]
fn executable_extensions(name: &str, environment: &BTreeMap<String, String>) -> Vec<String> {
    if Path::new(name).extension().is_some() {
        return vec![String::new()];
    }
    let mut extensions = vec![".exe".to_string()];
    if let Some(value) = environment
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PATHEXT"))
        .map(|(_, value)| value)
    {
        extensions.extend(value.split(';').map(str::to_ascii_lowercase));
    }
    extensions.push(String::new());
    extensions
}

#[cfg(not(windows))]
fn executable_extensions(_name: &str, _environment: &BTreeMap<String, String>) -> Vec<String> {
    vec![String::new()]
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn inherited_environment_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[
            "PATH",
            "PATHEXT",
            "SYSTEMROOT",
            "WINDIR",
            "TEMP",
            "TMP",
            "USERPROFILE",
        ]
    }
    #[cfg(not(windows))]
    {
        &["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR"]
    }
}

fn executable_missing_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::ExecutableMissing,
        "OpenCode executable is unavailable",
        true,
    )
}

fn protocol_error(message: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("OpenCode {message}"),
        false,
    )
}
