//! Fail-closed Codex permission-profile support for organizational runs.

use super::protocol::{CodexActivePermissionProfile, ConfigReadResponse, ThreadOpenResponse};
use crate::models::{
    ChatError, ChatErrorCode, ChatResult, McpStatusRead, ProviderAuthoritySupport, SafetyMode,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) const ORGANIZATIONAL_PERMISSION_PROFILE: &str = "ganbaru_organizational";
pub(super) const INTERNAL_MCP_TOKEN_ENVIRONMENT: &str = "GANBARU_CHAT_MCP_TOKEN";

const MAX_MCP_SERVER_NAMES: usize = 3_200;
const MAX_PERMISSION_PROFILES: usize = 3_200;
const MAX_MCP_SERVER_NAME_BYTES: usize = 1_000;
const ORGANIZATIONAL_PROFILE_TOML: &str =
    r#"{filesystem={":minimal"="read",":workspace_roots"={"."="write"}},network={enabled=false}}"#;
const ORGANIZATIONAL_SHELL_ENVIRONMENT_TOML: &str =
    r#"{inherit="core",ignore_default_excludes=false,experimental_use_profile=false,set={}}"#;
const DISABLED_ORGANIZATIONAL_FEATURES: &[&str] = &[
    "apps",
    "artifact",
    "auth_elicitation",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "code_mode.enabled",
    "code_mode_host",
    "computer_use",
    "enable_mcp_apps",
    "external_agent_memory_import",
    "hooks",
    "image_generation",
    "in_app_browser",
    "memories",
    "multi_agent",
    "multi_agent_v2",
    "network_proxy",
    "plugin_sharing",
    "plugins",
    "recommended_plugins",
    "remote_plugin",
    "request_permissions_tool",
    "respect_system_proxy",
    "shell_snapshot",
    "skill_mcp_dependency_install",
    "skill_search",
    "standalone_web_search",
    "use_agent_identity",
    "workspace_dependencies",
];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PermissionProfileListResponse {
    pub data: Vec<PermissionProfileSummary>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct PermissionProfileSummary {
    pub id: String,
    pub allowed: bool,
}

pub(super) fn organizational_authority_support() -> ProviderAuthoritySupport {
    ProviderAuthoritySupport {
        isolated_conversation: true,
        internal_host_tools: true,
        deny_shell: false,
        read_only_root: false,
        writable_root: true,
        confined_commands: true,
        network_boundary: true,
        classified_publish: false,
    }
}

pub(super) fn append_organizational_base_arguments(arguments: &mut Vec<String>) {
    push_config(
        arguments,
        format!("permissions.{ORGANIZATIONAL_PERMISSION_PROFILE}={ORGANIZATIONAL_PROFILE_TOML}"),
    );
    push_config(
        arguments,
        format!("default_permissions=\"{ORGANIZATIONAL_PERMISSION_PROFILE}\""),
    );
    push_config(arguments, "allow_login_shell=false".to_string());
    push_config(arguments, "web_search=\"disabled\"".to_string());
    push_config(
        arguments,
        format!("shell_environment_policy={ORGANIZATIONAL_SHELL_ENVIRONMENT_TOML}"),
    );
    for feature in DISABLED_ORGANIZATIONAL_FEATURES {
        push_config(arguments, format!("features.{feature}=false"));
    }
}

pub(super) fn append_disabled_mcp_arguments(
    arguments: &mut Vec<String>,
    names: &[String],
    internal_name: &str,
) -> ChatResult<()> {
    for name in names {
        if name == internal_name {
            continue;
        }
        let name = mcp_server_config_key(name)?;
        push_config(arguments, format!("mcp_servers.{name}.enabled=false"));
    }
    Ok(())
}

pub(super) fn append_internal_mcp_arguments(
    arguments: &mut Vec<String>,
    name: &str,
    url: &str,
) -> ChatResult<()> {
    let name = mcp_server_config_key(name)?;
    validate_internal_mcp_url(url)?;
    let url = serde_json::to_string(url)
        .map_err(|_| organizational_configuration_error("internal MCP URL"))?;
    let token_environment = serde_json::to_string(INTERNAL_MCP_TOKEN_ENVIRONMENT)
        .map_err(|_| organizational_configuration_error("internal MCP token reference"))?;
    push_config(arguments, format!("mcp_servers.{name}.url={url}"));
    push_config(
        arguments,
        format!("mcp_servers.{name}.bearer_token_env_var={token_environment}"),
    );
    push_config(arguments, format!("mcp_servers.{name}.enabled=true"));
    push_config(arguments, format!("mcp_servers.{name}.required=true"));
    Ok(())
}

pub(super) fn mcp_server_names(response: ConfigReadResponse) -> ChatResult<Vec<String>> {
    let config = response
        .config
        .as_object()
        .ok_or_else(|| organizational_configuration_error("config response"))?;
    let servers = match config.get("mcp_servers") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Object(servers)) => servers,
        Some(_) => return Err(organizational_configuration_error("MCP server table")),
    };
    if servers.len() > MAX_MCP_SERVER_NAMES {
        return Err(organizational_configuration_error("MCP server count"));
    }
    servers
        .keys()
        .map(|name| {
            validate_mcp_server_name(name)?;
            Ok(name.clone())
        })
        .collect()
}

pub(super) fn verify_organizational_effective_config(
    response: ConfigReadResponse,
    internal_mcp: Option<(&str, &str)>,
) -> ChatResult<()> {
    let config = response
        .config
        .as_object()
        .ok_or_else(|| organizational_configuration_error("config response"))?;
    if config.get("default_permissions").and_then(Value::as_str)
        != Some(ORGANIZATIONAL_PERMISSION_PROFILE)
        || config.get("allow_login_shell").and_then(Value::as_bool) != Some(false)
        || config.get("web_search").and_then(Value::as_str) != Some("disabled")
    {
        return Err(organizational_configuration_error("hosted tool boundary"));
    }
    verify_organizational_profile(config)?;
    verify_organizational_shell_environment(config)?;
    verify_disabled_features(config)?;
    verify_effective_mcp_servers(config, internal_mcp)
}

pub(super) fn verify_permission_profile_list(
    response: &PermissionProfileListResponse,
) -> ChatResult<()> {
    if response.next_cursor.is_some()
        || response.data.len() > MAX_PERMISSION_PROFILES
        || !response
            .data
            .iter()
            .any(|profile| profile.id == ORGANIZATIONAL_PERMISSION_PROFILE && profile.allowed)
    {
        return Err(organizational_permission_error());
    }
    Ok(())
}

pub(super) fn verify_organizational_thread_open(
    mode: SafetyMode,
    workspace: &Path,
    response: &ThreadOpenResponse,
) -> ChatResult<()> {
    let expected = organizational_safety(mode)?;
    let expected_profile = CodexActivePermissionProfile {
        id: ORGANIZATIONAL_PERMISSION_PROFILE.to_string(),
        extends: None,
    };
    if response.active_permission_profile.as_ref() != Some(&expected_profile)
        || response.approval_policy.as_str() != Some(expected.approval_policy)
        || response.approvals_reviewer != expected.approvals_reviewer
        || response.runtime_workspace_roots.as_slice() != [PathBuf::from(workspace)]
    {
        return Err(organizational_permission_error());
    }
    Ok(())
}

pub(super) fn verify_organizational_mcp_status(
    status: &McpStatusRead,
    internal_name: &str,
) -> ChatResult<()> {
    let enabled = status
        .servers
        .iter()
        .filter(|server| server.enabled)
        .collect::<Vec<_>>();
    let [server] = enabled.as_slice() else {
        return Err(organizational_permission_error());
    };
    let runtime_ready = server.runtime_status.as_deref().is_none_or(|status| {
        matches!(
            status.to_ascii_lowercase().as_str(),
            "ready" | "connected" | "initialized"
        )
    });
    if server.name != internal_name
        || server.auth_status.as_deref() != Some("bearerToken")
        || !runtime_ready
    {
        return Err(organizational_permission_error());
    }
    Ok(())
}

pub(super) struct OrganizationalSafety {
    pub approval_policy: &'static str,
    pub approvals_reviewer: &'static str,
}

pub(super) fn organizational_safety(mode: SafetyMode) -> ChatResult<OrganizationalSafety> {
    match mode {
        SafetyMode::AskForApproval => Ok(OrganizationalSafety {
            approval_policy: "on-request",
            approvals_reviewer: "user",
        }),
        SafetyMode::ApproveForMe => Ok(OrganizationalSafety {
            approval_policy: "on-request",
            approvals_reviewer: "auto_review",
        }),
        SafetyMode::FullAccess => Ok(OrganizationalSafety {
            approval_policy: "never",
            approvals_reviewer: "user",
        }),
        SafetyMode::Custom => Err(organizational_permission_error()),
    }
}

fn verify_organizational_profile(config: &serde_json::Map<String, Value>) -> ChatResult<()> {
    let profile = config
        .get("permissions")
        .and_then(Value::as_object)
        .and_then(|profiles| profiles.get(ORGANIZATIONAL_PERMISSION_PROFILE))
        .and_then(Value::as_object)
        .ok_or_else(organizational_permission_error)?;
    if profile.get("extends").is_some_and(|value| !value.is_null()) {
        return Err(organizational_permission_error());
    }
    let has_profile_roots = profile
        .get("workspace_roots")
        .and_then(Value::as_object)
        .is_some_and(|roots| roots.values().any(|value| value == &Value::Bool(true)));
    if has_profile_roots {
        return Err(organizational_permission_error());
    }
    let filesystem = profile
        .get("filesystem")
        .and_then(Value::as_object)
        .ok_or_else(organizational_permission_error)?;
    if filesystem.get(":minimal").and_then(Value::as_str) != Some("read") {
        return Err(organizational_permission_error());
    }
    let workspace_rules = filesystem
        .get(":workspace_roots")
        .and_then(Value::as_object)
        .ok_or_else(organizational_permission_error)?;
    if workspace_rules.len() != 1
        || workspace_rules.get(".").and_then(Value::as_str) != Some("write")
    {
        return Err(organizational_permission_error());
    }
    if filesystem.iter().any(|(key, value)| {
        !matches!(
            key.as_str(),
            ":minimal" | ":workspace_roots" | "glob_scan_max_depth"
        ) && !value.is_null()
    }) {
        return Err(organizational_permission_error());
    }
    if profile
        .get("network")
        .and_then(Value::as_object)
        .and_then(|network| network.get("enabled"))
        .and_then(Value::as_bool)
        != Some(false)
    {
        return Err(organizational_permission_error());
    }
    Ok(())
}

fn verify_organizational_shell_environment(
    config: &serde_json::Map<String, Value>,
) -> ChatResult<()> {
    let policy = config
        .get("shell_environment_policy")
        .and_then(Value::as_object)
        .ok_or_else(organizational_permission_error)?;
    if policy.get("inherit").and_then(Value::as_str) != Some("core")
        || policy
            .get("ignore_default_excludes")
            .and_then(Value::as_bool)
            != Some(false)
        || policy
            .get("experimental_use_profile")
            .and_then(Value::as_bool)
            != Some(false)
    {
        return Err(organizational_permission_error());
    }
    let set = policy
        .get("set")
        .and_then(Value::as_object)
        .ok_or_else(organizational_permission_error)?;
    if set.iter().any(|(name, value)| {
        let uppercase = name.to_ascii_uppercase();
        uppercase.contains("KEY")
            || uppercase.contains("SECRET")
            || uppercase.contains("TOKEN")
            || value.as_str().is_none_or(|value| value.contains('\0'))
    }) {
        return Err(organizational_permission_error());
    }
    Ok(())
}

fn verify_disabled_features(config: &serde_json::Map<String, Value>) -> ChatResult<()> {
    let features = config
        .get("features")
        .and_then(Value::as_object)
        .ok_or_else(organizational_permission_error)?;
    for feature in DISABLED_ORGANIZATIONAL_FEATURES {
        let mut value = Value::Object(features.clone());
        for component in feature.split('.') {
            value = value
                .as_object()
                .and_then(|object| object.get(component))
                .cloned()
                .ok_or_else(organizational_permission_error)?;
        }
        if value.as_bool() != Some(false) {
            return Err(organizational_permission_error());
        }
    }
    Ok(())
}

fn verify_effective_mcp_servers(
    config: &serde_json::Map<String, Value>,
    internal_mcp: Option<(&str, &str)>,
) -> ChatResult<()> {
    let servers = config
        .get("mcp_servers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if servers.len() > MAX_MCP_SERVER_NAMES {
        return Err(organizational_configuration_error("MCP server count"));
    }
    let mut enabled = BTreeSet::new();
    for (name, value) in servers {
        validate_mcp_server_name(&name)?;
        let server = value
            .as_object()
            .ok_or_else(|| organizational_configuration_error("MCP server entry"))?;
        if server.get("enabled").and_then(Value::as_bool) != Some(false) {
            enabled.insert(name);
        }
    }
    match internal_mcp {
        Some((expected_name, expected_url))
            if enabled == BTreeSet::from([expected_name.to_string()]) =>
        {
            let server = config
                .get("mcp_servers")
                .and_then(Value::as_object)
                .and_then(|servers| servers.get(expected_name))
                .and_then(Value::as_object)
                .ok_or_else(organizational_permission_error)?;
            if server.get("url").and_then(Value::as_str) != Some(expected_url)
                || server.get("bearer_token_env_var").and_then(Value::as_str)
                    != Some(INTERNAL_MCP_TOKEN_ENVIRONMENT)
                || server.get("required").and_then(Value::as_bool) != Some(true)
            {
                return Err(organizational_permission_error());
            }
            Ok(())
        }
        None if enabled.is_empty() => Ok(()),
        _ => Err(organizational_permission_error()),
    }
}

fn push_config(arguments: &mut Vec<String>, value: String) {
    arguments.push("-c".to_string());
    arguments.push(value);
}

pub(super) fn mcp_server_config_key(name: &str) -> ChatResult<String> {
    validate_mcp_server_name(name)?;
    Ok(name.to_string())
}

fn validate_mcp_server_name(name: &str) -> ChatResult<()> {
    if name.trim().is_empty()
        || name.len() > MAX_MCP_SERVER_NAME_BYTES
        || name.chars().any(char::is_control)
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(organizational_configuration_error("MCP server name"));
    }
    Ok(())
}

fn validate_internal_mcp_url(value: &str) -> ChatResult<()> {
    let parsed = reqwest::Url::parse(value).ok();
    let loopback = parsed.as_ref().is_some_and(|url| {
        url.scheme() == "http"
            && url.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|address| address.is_loopback())
            })
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    });
    if value.len() > 2_048 || value.chars().any(char::is_control) || !loopback {
        return Err(organizational_configuration_error("internal MCP URL"));
    }
    Ok(())
}

fn organizational_configuration_error(boundary: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::ConfigurationInvalid,
        format!("Codex organizational {boundary} is unavailable"),
        true,
    )
}

fn organizational_permission_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Codex could not prove the required organizational authority boundary",
        false,
    )
}
