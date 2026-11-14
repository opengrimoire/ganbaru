//! Fail-closed Codex permission-profile support for organizational runs.

use super::protocol::{CodexActivePermissionProfile, ConfigReadResponse, ThreadOpenResponse};
use crate::models::{
    ChatError, ChatErrorCode, ChatResult, McpStatusRead, ProviderAuthoritySupport,
    ProviderFolderCapability, ProviderInternalMcpConfig, ProviderOrganizationalScope, SafetyMode,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The permission profile of organizational runs that write their working directory.
pub(super) const ORGANIZATIONAL_PERMISSION_PROFILE: &str = "ganbaru_organizational";
/// The permission profile of organizational runs that only read their working directory.
pub(super) const ORGANIZATIONAL_READ_PERMISSION_PROFILE: &str = "ganbaru_organizational_read";
pub(super) const INTERNAL_MCP_TOKEN_ENVIRONMENT: &str = "GANBARU_CHAT_MCP_TOKEN";

const MAX_MCP_SERVER_NAMES: usize = 3_200;
const MAX_PERMISSION_PROFILES: usize = 3_200;
const MAX_MCP_SERVER_NAME_BYTES: usize = 1_000;
const ORGANIZATIONAL_SHELL_ENVIRONMENT_TOML: &str =
    r#"{inherit="core",ignore_default_excludes=false,experimental_use_profile=false,set={}}"#;
/// Codex features no organizational run loads. The code-mode host stays on: current Codex
/// models reach every tool, including the internal host tools, only through its `exec` tool,
/// which evaluates JavaScript in a V8 isolate without Node, file system, network, or imports,
/// so the nested tools left below decide what a run can touch. `view_image` reads any local
/// path, so it stays off.
const DISABLED_ORGANIZATIONAL_FEATURES: &[&str] = &[
    "apps",
    "artifact",
    "auth_elicitation",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "code_mode.enabled",
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
    "view_image",
    "workspace_dependencies",
];
/// Codex features that load a shell tool, off unless the scope runs commands.
const SHELL_FEATURES: &[&str] = &["shell_tool", "unified_exec"];

/// The Codex launch shape that enforces an organizational scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CodexOrganizationalAccess {
    /// A read-only working directory without a shell: a conversation or a read grant.
    Read,
    /// A writable working directory without a shell: an edit grant.
    Edit,
    /// A writable working directory whose commands run in the Codex sandbox: execute, publish,
    /// and scratch targets.
    Commands,
}

impl CodexOrganizationalAccess {
    /// The access of a launch under `internal_mcp`: none for a direct run, and an error for an
    /// organizational run that carries no authorized scope.
    pub(super) fn for_launch(
        internal_mcp: Option<&ProviderInternalMcpConfig>,
    ) -> ChatResult<Option<Self>> {
        match internal_mcp {
            Some(server) if server.organizational_authority => server
                .organizational_scope
                .map(|scope| Some(Self::for_scope(scope)))
                .ok_or_else(organizational_permission_error),
            _ => Ok(None),
        }
    }

    pub(super) fn for_scope(scope: ProviderOrganizationalScope) -> Self {
        match scope {
            ProviderOrganizationalScope::Conversation
            | ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Read,
            } => Self::Read,
            ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Edit,
            } => Self::Edit,
            ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Execute | ProviderFolderCapability::Publish,
            }
            | ProviderOrganizationalScope::Scratch => Self::Commands,
        }
    }

    /// The permission profile the launch defines, selects by default, and names on every thread
    /// and turn, so thread-open verification proves the root access of the scope.
    pub(super) fn permission_profile(self) -> &'static str {
        match self {
            Self::Read => ORGANIZATIONAL_READ_PERMISSION_PROFILE,
            Self::Edit | Self::Commands => ORGANIZATIONAL_PERMISSION_PROFILE,
        }
    }

    fn workspace_access(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit | Self::Commands => "write",
        }
    }

    fn runs_commands(self) -> bool {
        self == Self::Commands
    }

    fn profile_toml(self) -> String {
        format!(
            r#"{{filesystem={{":minimal"="read",":workspace_roots"={{"."="{}"}}}},network={{enabled=false}}}}"#,
            self.workspace_access()
        )
    }

    /// Every feature the launch turns off and verification requires to read back as off.
    fn disabled_features(self) -> impl Iterator<Item = &'static str> {
        let shell: &[&str] = if self.runs_commands() {
            &[]
        } else {
            SHELL_FEATURES
        };
        DISABLED_ORGANIZATIONAL_FEATURES
            .iter()
            .chain(shell)
            .copied()
    }
}

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

/// The boundaries an organizational Codex launch proves before dispatch. Read and edit launches
/// drop the shell tools, and every launch names a permission profile whose root access matches
/// the scope and whose network is off. Commands run in the Codex sandbox, and no organizational
/// approval reaches a person or the automatic reviewer: Ganbaru declines every native approval
/// request, and approve-for-me runs use the `never` policy, so a write or command the sandbox
/// refuses fails instead of retrying outside it. Commands therefore never count as person
/// approved.
pub(super) fn organizational_authority_support() -> ProviderAuthoritySupport {
    ProviderAuthoritySupport {
        isolated_conversation: true,
        internal_host_tools: true,
        deny_shell: true,
        read_only_root: true,
        writable_root: true,
        confined_commands: true,
        network_boundary: true,
        classified_publish: false,
        person_approved_commands: false,
    }
}

/// The boundaries an organizational Codex launch proves when the Codex command sandbox cannot
/// start on this device: a conversation from a neutral directory with the internal host tools,
/// and no folder or scratch access, because every folder scope depends on that sandbox.
pub(super) fn organizational_conversation_support() -> ProviderAuthoritySupport {
    ProviderAuthoritySupport {
        isolated_conversation: true,
        internal_host_tools: true,
        ..ProviderAuthoritySupport::default()
    }
}

/// Whether an organizational launch under `internal_mcp` reaches a folder or scratch scope, and
/// so needs the Codex command sandbox to start on this device.
pub(super) fn organizational_scope_needs_sandbox(
    internal_mcp: Option<&ProviderInternalMcpConfig>,
) -> bool {
    internal_mcp.is_some_and(|server| {
        server.organizational_authority
            && server.organizational_scope != Some(ProviderOrganizationalScope::Conversation)
    })
}

/// The refusal of a folder or scratch launch on a device where the Codex sandbox cannot start.
pub(super) fn sandbox_unavailable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Codex cannot start its command sandbox on this device, so it cannot take folder or scratch access",
        false,
    )
}

pub(super) fn append_organizational_base_arguments(
    arguments: &mut Vec<String>,
    access: CodexOrganizationalAccess,
) {
    let profile = access.permission_profile();
    push_config(
        arguments,
        format!("permissions.{profile}={}", access.profile_toml()),
    );
    push_config(arguments, format!("default_permissions=\"{profile}\""));
    push_config(arguments, "allow_login_shell=false".to_string());
    push_config(arguments, "web_search=\"disabled\"".to_string());
    push_config(
        arguments,
        format!("shell_environment_policy={ORGANIZATIONAL_SHELL_ENVIRONMENT_TOML}"),
    );
    for feature in access.disabled_features() {
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
    access: CodexOrganizationalAccess,
) -> ChatResult<()> {
    let config = response
        .config
        .as_object()
        .ok_or_else(|| organizational_configuration_error("config response"))?;
    if config.get("default_permissions").and_then(Value::as_str)
        != Some(access.permission_profile())
        || config.get("allow_login_shell").and_then(Value::as_bool) != Some(false)
        || config.get("web_search").and_then(Value::as_str) != Some("disabled")
    {
        return Err(organizational_configuration_error("hosted tool boundary"));
    }
    verify_organizational_profile(config, access)?;
    verify_organizational_shell_environment(config)?;
    verify_disabled_features(config, access)?;
    verify_effective_mcp_servers(config, internal_mcp)
}

pub(super) fn verify_permission_profile_list(
    response: &PermissionProfileListResponse,
    access: CodexOrganizationalAccess,
) -> ChatResult<()> {
    if response.next_cursor.is_some()
        || response.data.len() > MAX_PERMISSION_PROFILES
        || !response
            .data
            .iter()
            .any(|profile| profile.id == access.permission_profile() && profile.allowed)
    {
        return Err(organizational_permission_error());
    }
    Ok(())
}

pub(super) fn verify_organizational_thread_open(
    mode: SafetyMode,
    workspace: &Path,
    response: &ThreadOpenResponse,
    access: CodexOrganizationalAccess,
) -> ChatResult<()> {
    let expected = organizational_safety(mode)?;
    let expected_profile = CodexActivePermissionProfile {
        id: access.permission_profile().to_string(),
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

#[derive(Debug)]
pub(super) struct OrganizationalSafety {
    pub approval_policy: &'static str,
    pub approvals_reviewer: &'static str,
}

/// The Codex approval settings of an organizational run. Ask keeps `on-request` with the person
/// as reviewer, so every request to retry outside the sandbox reaches the router, which declines
/// it. Approve-for-me and full access use `never`, because the automatic reviewer approves writes
/// outside the working directory and retries outside the sandbox; the sandbox then decides alone.
pub(super) fn organizational_safety(mode: SafetyMode) -> ChatResult<OrganizationalSafety> {
    match mode {
        SafetyMode::AskForApproval => Ok(OrganizationalSafety {
            approval_policy: "on-request",
            approvals_reviewer: "user",
        }),
        SafetyMode::ApproveForMe | SafetyMode::FullAccess => Ok(OrganizationalSafety {
            approval_policy: "never",
            approvals_reviewer: "user",
        }),
        SafetyMode::Custom => Err(organizational_permission_error()),
    }
}

fn verify_organizational_profile(
    config: &serde_json::Map<String, Value>,
    access: CodexOrganizationalAccess,
) -> ChatResult<()> {
    let profile = config
        .get("permissions")
        .and_then(Value::as_object)
        .and_then(|profiles| profiles.get(access.permission_profile()))
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
        || workspace_rules.get(".").and_then(Value::as_str) != Some(access.workspace_access())
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

fn verify_disabled_features(
    config: &serde_json::Map<String, Value>,
    access: CodexOrganizationalAccess,
) -> ChatResult<()> {
    let features = config
        .get("features")
        .and_then(Value::as_object)
        .ok_or_else(organizational_permission_error)?;
    for feature in access.disabled_features() {
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
