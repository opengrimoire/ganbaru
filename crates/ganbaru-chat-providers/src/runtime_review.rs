//! Classification of portable provider runtime settings that need device confirmation.
//!
//! Portable configuration travels with backups and handoff, so values that can run code, widen
//! permissions, or redirect provider traffic apply only after the user confirms them natively on
//! this device. Per-family allowlists decide which values apply without asking; they never limit
//! what the user may configure.

use crate::models::ProviderInstanceConfig;
use ganbaru_chat_contracts::secret_names::is_secret_like_environment_name;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Longest single review label shown in the native confirmation.
pub const MAX_RUNTIME_REVIEW_LABEL_CHARS: usize = 240;

/// Variables that apply silently for every family because they only affect formatting or locale.
const SHARED_SILENT_ENVIRONMENT: &[&str] = &["LANG", "LC_ALL", "NO_COLOR", "TZ"];

/// Provider configuration keys that only change model presentation.
const COSMETIC_PROVIDER_CONFIG_KEYS: &[&str] = &[
    "allowCustomModels",
    "customModelIds",
    "customModelLabels",
    "refreshMcpBeforeTurn",
];

/// Variable names and prefixes that load code, redirect traffic, or move provider state.
const PERMISSION_WIDENING_ENVIRONMENT: &[&str] = &[
    "ALL_PROXY",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "DYLD_",
    "GROK_HOME",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "LD_",
    "NODE_EXTRA_CA_CERTS",
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_TLS_REJECT_UNAUTHORIZED",
    "OPENCODE_CONFIG",
    "PATH",
    "PYTHONPATH",
    "SSL_CERT_",
    "XDG_CONFIG_HOME",
];

/// Launch arguments that change tools, directories, plugins, prompts, sandboxing, or approvals.
const PERMISSION_WIDENING_ARGUMENTS: &[&str] = &[
    "--add-dir",
    "--agents",
    "--allowedTools",
    "--allowed-tools",
    "--append-system-prompt",
    "--disallowedTools",
    "--plugin-dir",
    "--system-prompt",
    "-c",
    "--config",
    "--enable",
    "--disable",
    "--endpoint",
    "-e",
];

/// One runtime value that needs this device's confirmation before it applies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeReviewItem {
    /// Stable digest of the family and value; device approvals store this.
    pub digest: String,
    /// Bounded text naming the value in the native confirmation.
    pub label: String,
    /// Whether the value is known to widen permissions or redirect provider traffic.
    pub widens_permissions: bool,
    /// Whether another local account could replace the program this item names; the app sets it
    /// for typed executable paths so the confirmation can warn about it.
    pub shared_program: bool,
}

struct FamilyAllowlist {
    silent_environment: &'static [&'static str],
    silent_inherited: &'static [&'static str],
    flags: &'static [&'static str],
    valued_options: &'static [&'static str],
    codex_config_keys: &'static [&'static str],
}

const EMPTY_ALLOWLIST: FamilyAllowlist = FamilyAllowlist {
    silent_environment: &[],
    silent_inherited: &[],
    flags: &[],
    valued_options: &[],
    codex_config_keys: &[],
};

fn family_allowlist(family_id: &str) -> FamilyAllowlist {
    match family_id {
        "claude" => FamilyAllowlist {
            silent_environment: &[
                "BASH_DEFAULT_TIMEOUT_MS",
                "BASH_MAX_TIMEOUT_MS",
                "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
                "DISABLE_AUTOUPDATER",
                "DISABLE_ERROR_REPORTING",
                "DISABLE_TELEMETRY",
                "MAX_THINKING_TOKENS",
                "MCP_TIMEOUT",
                "MCP_TOOL_TIMEOUT",
            ],
            silent_inherited: &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            flags: &[],
            valued_options: &["--fallback-model"],
            codex_config_keys: &[],
        },
        "codex" => FamilyAllowlist {
            silent_environment: &[],
            silent_inherited: &["CODEX_API_KEY", "OPENAI_API_KEY"],
            flags: &["--strict-config"],
            valued_options: &[],
            codex_config_keys: &[
                "hide_agent_reasoning",
                "model_reasoning_summary",
                "model_verbosity",
                "show_raw_agent_reasoning",
            ],
        },
        "cursor" => FamilyAllowlist {
            silent_inherited: &["CURSOR_API_KEY"],
            ..EMPTY_ALLOWLIST
        },
        "grok" => FamilyAllowlist {
            silent_inherited: &["GROK_API_KEY", "XAI_API_KEY"],
            ..EMPTY_ALLOWLIST
        },
        "opencode" => FamilyAllowlist {
            silent_environment: &[],
            silent_inherited: &[
                "ANTHROPIC_API_KEY",
                "DEEPSEEK_API_KEY",
                "GEMINI_API_KEY",
                "GOOGLE_GENERATIVE_AI_API_KEY",
                "GROQ_API_KEY",
                "MISTRAL_API_KEY",
                "OPENAI_API_KEY",
                "OPENROUTER_API_KEY",
                "XAI_API_KEY",
            ],
            flags: &["--print-logs"],
            valued_options: &["--log-level"],
            codex_config_keys: &[],
        },
        _ => EMPTY_ALLOWLIST,
    }
}

/// Portable runtime values of `configuration` that need device confirmation, in stable order.
///
/// Credential references are not listed: they are bound to their instance and variable, so a
/// portable configuration cannot use them to reach another secret.
pub fn portable_runtime_review_items(
    configuration: &ProviderInstanceConfig,
) -> Vec<RuntimeReviewItem> {
    let family = configuration.family_id.as_str();
    let allowlist = family_allowlist(family);
    let mut items = Vec::new();
    if !launch_arguments_are_silent(&configuration.launch_arguments, &allowlist) {
        let label = format!(
            "Launch arguments: {}",
            configuration.launch_arguments.join(" ")
        );
        items.push(review_item(
            family,
            &["arguments", &configuration.launch_arguments.join("\0")],
            label,
            configuration
                .launch_arguments
                .iter()
                .any(|argument| argument_widens_permissions(argument)),
        ));
    }
    for (name, value) in &configuration.environment {
        if environment_is_silent(name, value, &allowlist) {
            continue;
        }
        let label = if value.strip_prefix("inherit:") == Some(name.as_str()) {
            format!("Environment: {name} (inherited from this device)")
        } else if is_secret_like_environment_name(name) {
            format!("Environment: {name} (plaintext value hidden)")
        } else {
            format!("Environment: {name}={value}")
        };
        items.push(review_item(
            family,
            &["environment", name, value],
            label,
            environment_widens_permissions(name),
        ));
    }
    if let Value::Object(entries) = &configuration.provider_config.value {
        for (key, value) in entries {
            if COSMETIC_PROVIDER_CONFIG_KEYS.contains(&key.as_str())
                || provider_config_value_is_default(key, value)
            {
                continue;
            }
            let rendered = value.to_string();
            items.push(review_item(
                family,
                &["providerConfig", key, &rendered],
                format!("Provider setting: {key} = {rendered}"),
                true,
            ));
        }
    }
    items
}

/// Review item for a device-local executable or provider home path that no picker supplied.
pub fn device_path_review_item(
    family_id: &str,
    kind: DevicePathKind,
    path: &str,
) -> RuntimeReviewItem {
    let (key, label) = match kind {
        DevicePathKind::Executable => ("executable", format!("Program: {path}")),
        DevicePathKind::ProviderHome => ("providerHome", format!("Provider home: {path}")),
    };
    review_item(family_id, &[key, path], label, true)
}

/// Whether `executable` is a bare default command name the family resolves through `PATH`.
pub fn is_default_executable(family_id: &str, executable: &str) -> bool {
    crate::registry::PROVIDER_METADATA
        .iter()
        .find(|metadata| metadata.family_id == family_id)
        .is_some_and(|metadata| {
            metadata
                .default_executable_candidates
                .contains(&executable.trim())
        })
}

/// Device-local provider paths that a picker or confirmation must vouch for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DevicePathKind {
    Executable,
    ProviderHome,
}

fn review_item(
    family_id: &str,
    parts: &[&str],
    label: String,
    widens_permissions: bool,
) -> RuntimeReviewItem {
    let mut hasher = Sha256::new();
    hasher.update(family_id.as_bytes());
    for part in parts {
        hasher.update([0]);
        hasher.update(part.as_bytes());
    }
    RuntimeReviewItem {
        digest: format!("{:x}", hasher.finalize()),
        label: bounded_label(&label),
        widens_permissions,
        shared_program: false,
    }
}

fn bounded_label(label: &str) -> String {
    let cleaned = label
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if cleaned.chars().count() <= MAX_RUNTIME_REVIEW_LABEL_CHARS {
        return cleaned;
    }
    let mut truncated = cleaned
        .chars()
        .take(MAX_RUNTIME_REVIEW_LABEL_CHARS - 1)
        .collect::<String>();
    truncated.push('\u{2026}');
    truncated
}

fn launch_arguments_are_silent(arguments: &[String], allowlist: &FamilyAllowlist) -> bool {
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if allowlist.flags.contains(&argument) {
            index += 1;
            continue;
        }
        if let Some((name, value)) = argument.split_once('=') {
            if !silent_option_value(name, value, allowlist) {
                return false;
            }
            index += 1;
            continue;
        }
        let Some(value) = arguments.get(index + 1) else {
            return false;
        };
        if value.starts_with('-') || !silent_option_value(argument, value, allowlist) {
            return false;
        }
        index += 2;
    }
    true
}

fn silent_option_value(name: &str, value: &str, allowlist: &FamilyAllowlist) -> bool {
    if allowlist.valued_options.contains(&name) {
        return !value.is_empty();
    }
    if matches!(name, "-c" | "--config") {
        return value
            .split_once('=')
            .is_some_and(|(key, _)| allowlist.codex_config_keys.contains(&key.trim()));
    }
    false
}

fn environment_is_silent(name: &str, value: &str, allowlist: &FamilyAllowlist) -> bool {
    if let Some(inherited) = value.strip_prefix("inherit:") {
        return inherited == name
            && (allowlist.silent_inherited.contains(&name)
                || allowlist.silent_environment.contains(&name)
                || SHARED_SILENT_ENVIRONMENT.contains(&name));
    }
    allowlist.silent_environment.contains(&name) || SHARED_SILENT_ENVIRONMENT.contains(&name)
}

fn environment_widens_permissions(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.ends_with("_BASE_URL")
        || upper.ends_with("_API_URL")
        || upper.ends_with("_ENDPOINT")
        || PERMISSION_WIDENING_ENVIRONMENT.iter().any(|known| {
            if known.ends_with('_') {
                upper.starts_with(known)
            } else {
                upper == *known || upper.starts_with(&format!("{known}_"))
            }
        })
}

fn argument_widens_permissions(argument: &str) -> bool {
    let name = argument.split_once('=').map_or(argument, |(name, _)| name);
    PERMISSION_WIDENING_ARGUMENTS.contains(&name)
}

fn provider_config_value_is_default(key: &str, value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::String(text) => text.trim().is_empty() || (key == "mode" && text == "local"),
        Value::Array(entries) => entries.is_empty(),
        Value::Object(entries) => entries.is_empty(),
        Value::Bool(true) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests;
