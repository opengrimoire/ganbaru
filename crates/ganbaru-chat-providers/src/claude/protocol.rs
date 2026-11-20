//! Typed builders and bounded decoders for Claude Code's SDK JSONL protocol.

use super::instructions::session_instructions;
use crate::models::*;
use crate::registry::{ProviderCompatibilityRange, VersionOrder, admit_version, version_refusal};
use base64::{Engine as _, engine::general_purpose};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub const MINIMUM_CLAUDE_VERSION: ClaudeVersion = ClaudeVersion::new(2, 1, 170);
const MAX_PROMPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_TEXT_ATTACHMENT_BYTES: usize = 128 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_MODEL_OPTIONS: usize = 32;
const MAX_LAUNCH_ARGUMENTS: usize = 128;
const MAX_LAUNCH_ARGUMENT_BYTES: usize = 16 * 1024;
/// Largest instructions passed inline as one launch argument; larger ones use the file form.
pub const MAX_INLINE_SYSTEM_PROMPT_BYTES: usize = MAX_LAUNCH_ARGUMENT_BYTES;

/// How session instructions reach the Claude system prompt at launch.
#[derive(Clone, Copy, Debug)]
pub enum ClaudeSystemPromptArgument<'a> {
    /// The text itself, passed with `--append-system-prompt`.
    Inline(&'a str),
    /// An owner-only file, passed with `--append-system-prompt-file`.
    File(&'a Path),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ClaudeVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl ClaudeVersion {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl std::fmt::Display for ClaudeVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ClaudeInitializeResponse {
    #[serde(default)]
    pub commands: Vec<ClaudeCommand>,
    #[serde(default)]
    pub models: Vec<ClaudeModel>,
    pub account: Option<ClaudeAccount>,
    // Deserialized to validate the provider's advertised wire shape.
    #[allow(dead_code)]
    #[serde(default)]
    pub output_style: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
}

pub fn prompt_catalog(commands: &[ClaudeCommand]) -> Vec<ChatPromptCatalogEntry> {
    let mut seen = BTreeSet::new();
    let mut entries = commands
        .iter()
        .take(512)
        .filter_map(|command| {
            let name = command.name.trim().trim_start_matches('/');
            if name.is_empty()
                || name.len() > 200
                || name.chars().any(char::is_control)
                || name.chars().any(char::is_whitespace)
                || !seen.insert(name.to_ascii_lowercase())
            {
                return None;
            }
            let description = command
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.len() <= 1_000)
                .map(str::to_string);
            let argument_hint = command
                .argument_hint
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty() && value.len() <= 500)
                .map(str::to_string);
            Some(ChatPromptCatalogEntry {
                value: format!("/{name}"),
                label: name.to_string(),
                description,
                argument_hint,
                kind: "command".to_string(),
                source: "provider".to_string(),
                stale: false,
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.label.cmp(&right.label));
    entries
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModel {
    pub value: String,
    pub resolved_model: Option<String>,
    pub display_name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub supports_effort: bool,
    #[serde(default)]
    pub supported_effort_levels: Vec<String>,
    #[serde(default)]
    pub supports_fast_mode: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccount {
    pub email: Option<String>,
    pub organization: Option<String>,
    pub subscription_type: Option<String>,
}

impl ClaudeAccount {
    /// The account's plan from its Claude subscription. An account without one bills through
    /// the API console or a cloud provider, which the CLI does not name, so it stays unknown.
    pub fn plan(&self) -> Option<ProviderAccountPlan> {
        match self.subscription_type.as_deref().map(str::trim)? {
            "" => None,
            plan if plan.eq_ignore_ascii_case("free") => Some(ProviderAccountPlan::Free),
            _ => Some(ProviderAccountPlan::Subscription),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeResumeCursor {
    pub session_uuid: String,
    pub last_assistant_uuid: Option<String>,
    pub turn_count: u64,
}

pub fn parse_version(value: &str) -> ChatResult<ClaudeVersion> {
    let token = value
        .split_whitespace()
        .find(|token| {
            token
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_digit())
        })
        .ok_or_else(unsupported_version_error)?;
    let core = token.split_once('-').map_or(token, |(core, _)| core);
    let mut parts = core.split('.');
    let major = parse_version_part(parts.next())?;
    let minor = parse_version_part(parts.next())?;
    let patch = parse_version_part(parts.next())?;
    if parts.next().is_some() {
        return Err(unsupported_version_error());
    }
    Ok(ClaudeVersion::new(major, minor, patch))
}

/// Claude Code versions with the complete interactive SDK protocol, up to the version the
/// Claude replay scenarios record, below the next major line.
pub const CLAUDE_COMPATIBILITY: ProviderCompatibilityRange = ProviderCompatibilityRange {
    minimum: "2.1.170",
    maximum_tested: "2.1.322",
    ceiling_exclusive: Some("3.0.0"),
};

/// Agent launches need `--restricted` (2.1.248), its messaging socket fix (2.1.290), the
/// late-registered tool fix (2.1.295), and `permissions.blockWritesOutsideWorkingDirectories`
/// together with a `--setting-sources` value that ignores left-out files (2.1.314).
pub const MINIMUM_ORGANIZATIONAL_CLAUDE_VERSION: ClaudeVersion = ClaudeVersion::new(2, 1, 314);

/// First CLI version whose shell prefix Ganbaru verified: it runs every shell command as
/// `PREFIX COMMAND`, with the command line as one argument, and writes its working directory
/// file under `CLAUDE_CODE_TMPDIR`. Older versions keep their commands unconfined and asked.
pub const MINIMUM_CONFINED_SHELL_CLAUDE_VERSION: ClaudeVersion = ClaudeVersion::new(2, 1, 325);

/// The oldest Claude Code version every agent launch on this device accepts, when `value` is
/// older: the agent minimum, or the confined shell minimum where the sandbox helper confines
/// agent commands, since older versions refuse every safety mode but asking.
pub fn agent_minimum_version(
    value: ClaudeVersion,
    confined_commands: bool,
) -> Option<ClaudeVersion> {
    let minimum = if confined_commands {
        MINIMUM_CONFINED_SHELL_CLAUDE_VERSION
    } else {
        MINIMUM_ORGANIZATIONAL_CLAUDE_VERSION
    };
    (value < minimum).then_some(minimum)
}

/// Variable naming the program the CLI places in front of every shell command.
pub const SHELL_PREFIX_ENVIRONMENT: &str = "CLAUDE_CODE_SHELL_PREFIX";
/// Variable naming the directory the CLI keeps shell command state and output in.
pub const TEMPORARY_DIRECTORY_ENVIRONMENT: &str = "CLAUDE_CODE_TMPDIR";

impl VersionOrder for ClaudeVersion {
    fn parse_version(value: &str) -> ChatResult<Self> {
        parse_version(value)
    }
}

/// Where `value` falls in `CLAUDE_COMPATIBILITY`, refusing a version below the minimum or at
/// or above the ceiling.
pub fn ensure_supported_version(value: ClaudeVersion) -> ChatResult<ProviderVersionAdmission> {
    let text = value.to_string();
    let admission = admit_version::<ClaudeVersion>(&CLAUDE_COMPATIBILITY, &text)?;
    match admission {
        ProviderVersionAdmission::BelowMinimum => Err(version_refusal(
            admission,
            &text,
            format!(
                "Claude Code {value} lacks the complete interactive SDK protocol; version {MINIMUM_CLAUDE_VERSION} or newer is required"
            ),
            true,
        )),
        ProviderVersionAdmission::AtOrAboveCeiling => Err(version_refusal(
            admission,
            &text,
            format!(
                "Claude Code {value} is unsupported; Ganbaru supports versions below {}",
                CLAUDE_COMPATIBILITY.ceiling_exclusive.unwrap_or_default()
            ),
            true,
        )),
        _ => Ok(admission),
    }
}

/// Refuses an agent launch on a Claude Code version below
/// `MINIMUM_ORGANIZATIONAL_CLAUDE_VERSION`; direct threads keep the family minimum.
pub fn ensure_organizational_version(value: ClaudeVersion) -> ChatResult<()> {
    if value < MINIMUM_ORGANIZATIONAL_CLAUDE_VERSION {
        return Err(version_refusal(
            ProviderVersionAdmission::BelowMinimum,
            &value.to_string(),
            format!(
                "Claude Code {value} cannot run agents; version {MINIMUM_ORGANIZATIONAL_CLAUDE_VERSION} or newer is required"
            ),
            true,
        ));
    }
    Ok(())
}

/// Refuses an agent launch whose safety mode needs confined commands on a Claude Code
/// version below `MINIMUM_CONFINED_SHELL_CLAUDE_VERSION`.
pub fn ensure_confined_shell_version(value: ClaudeVersion) -> ChatResult<()> {
    if value < MINIMUM_CONFINED_SHELL_CLAUDE_VERSION {
        return Err(version_refusal(
            ProviderVersionAdmission::BelowMinimum,
            &value.to_string(),
            format!(
                "Claude Code {value} cannot run agent commands in the Ganbaru sandbox; version {MINIMUM_CONFINED_SHELL_CLAUDE_VERSION} or newer is required"
            ),
            true,
        ));
    }
    Ok(())
}

fn parse_version_part(value: Option<&str>) -> ChatResult<u64> {
    value
        .and_then(|part| part.parse::<u64>().ok())
        .ok_or_else(unsupported_version_error)
}

#[derive(Clone, Copy, Debug)]
pub struct ClaudeLaunchOptions<'a> {
    pub fresh_session_uuid: Option<&'a str>,
    pub resume_session_uuid: Option<&'a str>,
    pub last_assistant_uuid: Option<&'a str>,
    pub model: Option<&'a ModelId>,
    pub effort: Option<&'a str>,
    pub fast_mode: Option<bool>,
    pub modes: TurnModeSnapshot,
    /// Untrusted folders start with user settings only, which leaves project
    /// settings, hooks, skills, commands, agents, MCP servers, and `CLAUDE.md`
    /// unloaded.
    pub project_configuration: ProjectConfigurationAccess,
    /// The internal `ganbaru-chat` server, whose bearer token stays in the environment.
    pub internal_mcp: Option<ClaudeInternalMcpLaunch<'a>>,
    /// Set only when the internal server carries organizational authority.
    pub organizational: Option<ClaudeOrganizationalLaunch<'a>>,
    /// Session instructions appended to the system prompt.
    pub system_prompt: Option<ClaudeSystemPromptArgument<'a>>,
    /// The probed CLI version, which decides how session instructions are passed.
    pub cli_version: Option<ClaudeVersion>,
}

/// The internal MCP server a Claude launch connects to.
#[derive(Clone, Copy, Debug)]
pub struct ClaudeInternalMcpLaunch<'a> {
    pub name: &'a str,
    pub url: &'a str,
}

/// The fail-closed launch surface of an organizational Claude run.
///
/// It sees only the internal server, loads no settings files, loads only the built-in tools
/// its scope allows, and cannot reach the denied ones. The file tools are confined to the
/// working directory, and every shell command, when the scope loads the shell, reaches
/// Ganbaru as a permission request.
#[derive(Clone, Copy, Debug)]
pub struct ClaudeOrganizationalLaunch<'a> {
    pub internal_server_name: &'a str,
    /// Internal server tools the run calls without an approval prompt, passed as
    /// `--allowedTools`. Each one names a tool of the internal server.
    pub pre_approved_tools: &'a [String],
    /// The authorized target the launch enforces.
    pub scope: ProviderOrganizationalScope,
    /// Whether the session's shell commands run under the Ganbaru sandbox.
    pub commands_confined: bool,
    /// Allowlisted keys of the user settings file, applied below the keys Ganbaru sets.
    pub user_settings: &'a Map<String, Value>,
}

/// The comma-joined `--allowedTools` value of an organizational launch, or `None` when it
/// pre-approves nothing.
///
/// The list is built by the host, but it may only name tools of the internal server, so it can
/// never pre-approve a built-in tool or another server's tool.
fn pre_approved_tools_argument(
    organizational: ClaudeOrganizationalLaunch<'_>,
) -> ChatResult<Option<String>> {
    if organizational.pre_approved_tools.is_empty() {
        return Ok(None);
    }
    let prefix = format!("mcp__{}__", organizational.internal_server_name);
    let names_internal_tool = |tool: &String| {
        tool.strip_prefix(&prefix).is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
    };
    if !organizational
        .pre_approved_tools
        .iter()
        .all(names_internal_tool)
    {
        return Err(organizational_permission_error());
    }
    Ok(Some(organizational.pre_approved_tools.join(",")))
}

/// The built-in tools one organizational scope loads, passed as `--tools`, and the tools it
/// denies on top of that allowlist as defense in depth, passed as `--disallowed-tools`.
///
/// Every scope loads the reading tools, the planning and question tools the driver maps, the
/// todo list, and web search and fetch, which always ask the person. Edit scopes add the file
/// writing tools, and command scopes add the shell. Anything else the CLI ships (subagents,
/// schedules, other sessions, skills, worktrees) stays unloaded, and the startup gate fails the session when the init report
/// carries a built-in tool outside the allowlist. Internal MCP tools are not built-in and
/// survive the allowlist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClaudeOrganizationalTools {
    pub allowed: &'static [&'static str],
    pub disallowed: &'static [&'static str],
}

const READ_ONLY_TOOLS: ClaudeOrganizationalTools = ClaudeOrganizationalTools {
    allowed: &[
        "Read",
        "Glob",
        "Grep",
        "ExitPlanMode",
        "AskUserQuestion",
        "TodoWrite",
        "WebSearch",
        "WebFetch",
    ],
    disallowed: &["Task", "Agent", "Bash", "Edit", "Write", "NotebookEdit"],
};

const EDIT_TOOLS: ClaudeOrganizationalTools = ClaudeOrganizationalTools {
    allowed: &[
        "Read",
        "Edit",
        "Write",
        "Glob",
        "Grep",
        "NotebookEdit",
        "ExitPlanMode",
        "AskUserQuestion",
        "TodoWrite",
        "WebSearch",
        "WebFetch",
    ],
    disallowed: &["Task", "Agent", "Bash"],
};

const COMMAND_TOOLS: ClaudeOrganizationalTools = ClaudeOrganizationalTools {
    allowed: &[
        "Read",
        "Edit",
        "Write",
        "Glob",
        "Grep",
        "NotebookEdit",
        "ExitPlanMode",
        "AskUserQuestion",
        "TodoWrite",
        "WebSearch",
        "WebFetch",
        "Bash",
    ],
    disallowed: &["Task", "Agent"],
};

/// The tool surface of one organizational scope.
pub fn organizational_tools(scope: ProviderOrganizationalScope) -> ClaudeOrganizationalTools {
    match scope {
        ProviderOrganizationalScope::Conversation
        | ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Read,
        } => READ_ONLY_TOOLS,
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Edit,
        } => EDIT_TOOLS,
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Execute | ProviderFolderCapability::Publish,
        }
        | ProviderOrganizationalScope::Scratch => COMMAND_TOOLS,
    }
}

/// Whether the scope loads the shell.
pub fn organizational_runs_commands(scope: ProviderOrganizationalScope) -> bool {
    organizational_tools(scope).allowed.contains(&"Bash")
}

/// Setting sources an organizational launch loads: none, so no user, project, or local file
/// adds MCP servers, hooks, agents, plugins, or environment.
pub const ORGANIZATIONAL_SETTING_SOURCES: &str = "";

/// Permission mode of AskForApproval in an organizational Build run, which asks before edits.
pub const ORGANIZATIONAL_ASK_PERMISSION_MODE: &str = "default";

/// Environment variable that carries the internal server bearer token to the CLI.
pub const INTERNAL_MCP_TOKEN_ENVIRONMENT: &str = "GANBARU_CHAT_MCP_TOKEN";

pub fn launch_arguments(
    prefix: Vec<String>,
    configured: &[String],
    options: ClaudeLaunchOptions<'_>,
) -> ChatResult<Vec<String>> {
    validate_launch_arguments(configured)?;
    let organizational_server = match options.organizational {
        Some(organizational) => {
            validate_organizational_launch_arguments(configured)?;
            match options.internal_mcp {
                Some(server) if server.name == organizational.internal_server_name => Some((
                    server,
                    organizational,
                    pre_approved_tools_argument(organizational)?,
                )),
                _ => return Err(organizational_permission_error()),
            }
        }
        None => None,
    };
    let mut arguments = prefix;
    arguments.extend([
        "-p".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--permission-prompt-tool".to_string(),
        "stdio".to_string(),
        "--include-partial-messages".to_string(),
        "--replay-user-messages".to_string(),
    ]);
    let permission_mode = match organizational_server {
        Some((_, organizational, _)) => Some(organizational_permission_mode(
            options.modes,
            organizational.scope,
            organizational.commands_confined,
        )?),
        None => permission_mode(options.modes),
    };
    if let Some(permission_mode) = permission_mode {
        arguments.push("--permission-mode".to_string());
        arguments.push(permission_mode.to_string());
        if permission_mode == "bypassPermissions" {
            arguments.push("--allow-dangerously-skip-permissions".to_string());
        }
    }
    if options.fresh_session_uuid.is_some() && options.resume_session_uuid.is_some() {
        return Err(ChatError::validation(
            "sessionUuid",
            "Claude session cannot be both fresh and resumed",
        ));
    }
    if let Some(session_uuid) = options.fresh_session_uuid {
        validate_uuid(session_uuid, "sessionUuid")?;
        arguments.push("--session-id".to_string());
        arguments.push(session_uuid.to_string());
    }
    if let Some(session_uuid) = options.resume_session_uuid {
        validate_uuid(session_uuid, "sessionUuid")?;
        arguments.push("--resume".to_string());
        arguments.push(session_uuid.to_string());
    }
    if let Some(last_assistant_uuid) = options.last_assistant_uuid {
        validate_uuid(last_assistant_uuid, "lastAssistantUuid")?;
        arguments.push("--resume-session-at".to_string());
        arguments.push(last_assistant_uuid.to_string());
    }
    if let Some(model) = options.model {
        validate_model_id(model.as_str())?;
        arguments.push("--model".to_string());
        arguments.push(model.as_str().to_string());
    }
    if let Some(effort) = options.effort {
        if !is_claude_effort(effort) {
            return Err(ChatError::validation(
                "modelOptions",
                "Claude effort value is unsupported",
            ));
        }
        arguments.push("--effort".to_string());
        arguments.push(effort.to_string());
    }
    if let Some(settings) = launch_settings(options.fast_mode, options.organizational) {
        arguments.push("--settings".to_string());
        arguments.push(settings);
    }
    if let Some((server, organizational, pre_approved_tools)) = organizational_server {
        let tools = organizational_tools(organizational.scope);
        // Configured arguments never reach an organizational launch, so nothing follows the
        // variadic deny list.
        arguments.extend([
            "--restricted".to_string(),
            "--setting-sources".to_string(),
            ORGANIZATIONAL_SETTING_SOURCES.to_string(),
            "--strict-mcp-config".to_string(),
            "--mcp-config".to_string(),
            internal_mcp_config_argument(server.name, server.url)?,
        ]);
        if let Some(system_prompt) = options.system_prompt {
            arguments.extend(system_prompt_arguments(system_prompt, options.cli_version)?);
        }
        if let Some(pre_approved_tools) = pre_approved_tools {
            arguments.extend(["--allowedTools".to_string(), pre_approved_tools]);
        }
        arguments.extend([
            "--tools".to_string(),
            tools.allowed.join(","),
            "--disallowed-tools".to_string(),
            tools.disallowed.join(","),
        ]);
        return Ok(arguments);
    }
    if options.project_configuration == ProjectConfigurationAccess::Restricted {
        arguments.push("--setting-sources".to_string());
        arguments.push("user".to_string());
    }
    if let Some(system_prompt) = options.system_prompt {
        if configured
            .iter()
            .any(|argument| names_flag(argument, SYSTEM_PROMPT_ARGUMENTS))
        {
            return Err(ChatError::validation(
                "launchArguments",
                "Claude launch arguments conflict with the session instructions",
            ));
        }
        arguments.extend(system_prompt_arguments(system_prompt, options.cli_version)?);
    }
    arguments.extend(configured.iter().cloned());
    if let Some(server) = options.internal_mcp {
        arguments.push("--mcp-config".to_string());
        arguments.push(internal_mcp_config_argument(server.name, server.url)?);
    }
    Ok(arguments)
}

/// Configured arguments that would replace or add to the system prompt of a session that
/// carries instructions, or keep a resumed session on its recorded prompt.
const SYSTEM_PROMPT_ARGUMENTS: &[&str] = &[
    "--append-system-prompt",
    "--append-system-prompt-file",
    "--system-prompt",
    "--system-prompt-file",
    "--system-prompt-snapshot",
];

/// First CLI version that records the system prompt on a conversation's first request and
/// sends that record on every later request and resume, together with the
/// `--system-prompt-snapshot` flag that turns the record off. Earlier versions render the
/// prompt fresh on every request.
pub const SYSTEM_PROMPT_SNAPSHOT_VERSION: ClaudeVersion = ClaudeVersion::new(2, 1, 267);

/// The arguments that append session instructions to the system prompt. Text that starts
/// with `--` is still read as the value of the spaced form. On CLI versions that record the
/// prompt, the record is turned off, so a resumed session uses the instructions of this
/// launch and not the ones it first opened with.
fn system_prompt_arguments(
    argument: ClaudeSystemPromptArgument<'_>,
    cli_version: Option<ClaudeVersion>,
) -> ChatResult<Vec<String>> {
    let mut arguments = match argument {
        ClaudeSystemPromptArgument::Inline(text) => {
            if text.len() > MAX_INLINE_SYSTEM_PROMPT_BYTES || text.contains('\0') {
                return Err(ChatError::validation(
                    "developerInstructions",
                    "Claude developer instructions exceed the supported limit",
                ));
            }
            vec!["--append-system-prompt".to_string(), text.to_string()]
        }
        ClaudeSystemPromptArgument::File(path) => {
            let path = path.to_str().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::ConfigurationInvalid,
                    "Claude instructions file is unavailable",
                    true,
                )
            })?;
            vec!["--append-system-prompt-file".to_string(), path.to_string()]
        }
    };
    if cli_version.is_some_and(|version| version >= SYSTEM_PROMPT_SNAPSHOT_VERSION) {
        arguments.extend(["--system-prompt-snapshot".to_string(), "off".to_string()]);
    }
    Ok(arguments)
}

/// Builds the `--mcp-config` JSON for the internal server. The header names the token
/// variable, which the CLI expands, so the bearer token never appears in argv.
pub fn internal_mcp_config_argument(name: &str, url: &str) -> ChatResult<String> {
    if name.is_empty() || url.is_empty() || name.contains('\0') || url.contains('\0') {
        return Err(ChatError::validation(
            "internalMcp",
            "Claude internal MCP server is invalid",
        ));
    }
    Ok(json!({
        "mcpServers": {
            name: {
                "type": "http",
                "url": url,
                "headers": {
                    "Authorization": format!("Bearer ${{{INTERNAL_MCP_TOKEN_ENVIRONMENT}}}")
                }
            }
        }
    })
    .to_string())
}

/// Maps the modes of an organizational run to its permission mode.
///
/// A scope that loads the shell admits AskForApproval, where the person approving each command
/// confines it, and, when the sandbox confines its commands, ApproveForMe and FullAccess. The
/// mode asks before edits in Build and stays read-only in Plan for every safety mode; Ganbaru
/// answers command and edit requests it can settle itself (see `organizational_runs_unattended`
/// and `organizational_edits_unattended`), so the CLI never bypasses permissions. A scope
/// without the shell maps every safety mode to the same asking modes, since its tool surface,
/// not the safety mode, bounds what the run can reach.
pub fn organizational_permission_mode(
    modes: TurnModeSnapshot,
    scope: ProviderOrganizationalScope,
    commands_confined: bool,
) -> ChatResult<&'static str> {
    if organizational_runs_commands(scope) {
        match (modes.safety_mode, commands_confined) {
            (SafetyMode::AskForApproval, _)
            | (SafetyMode::ApproveForMe | SafetyMode::FullAccess, true) => {}
            _ => return Err(organizational_permission_error()),
        }
    }
    Ok(match modes.interaction_mode {
        InteractionMode::Plan => "plan",
        InteractionMode::Build => ORGANIZATIONAL_ASK_PERMISSION_MODE,
    })
}

/// Whether Ganbaru allows a shell command of an organizational run without asking: only when
/// the sandbox confines the session's commands and the turn runs in Build with a safety mode
/// that approves on its own (ApproveForMe, which the Work on their own access level runs, or
/// FullAccess). The decision follows the current turn, so a later asking turn of the same
/// session asks again.
pub fn organizational_runs_unattended(
    tool_name: &str,
    modes: TurnModeSnapshot,
    commands_confined: bool,
) -> bool {
    tool_name == "Bash"
        && commands_confined
        && matches!(
            modes.safety_mode,
            SafetyMode::ApproveForMe | SafetyMode::FullAccess
        )
        && modes.interaction_mode == InteractionMode::Build
}

/// Whether a file edit tool of an organizational run may run without asking once its request
/// passed the scope and path checks: in Build under every proven safety mode, since every
/// access level that loads an edit tool lets agents edit the folder. The path rule that keeps
/// hidden files and folders with the person lives with the policy that knows the root.
pub fn organizational_edits_unattended(tool_name: &str, modes: TurnModeSnapshot) -> bool {
    matches!(tool_name, "Edit" | "Write" | "NotebookEdit")
        && modes.interaction_mode == InteractionMode::Build
        && modes.safety_mode != SafetyMode::Custom
}

/// The error of an organizational Claude run whose boundary cannot be proven.
pub fn organizational_permission_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Claude could not prove the required organizational authority boundary",
        false,
    )
}

/// Serializes the one `--settings` object a launch passes, or none when it would be empty.
///
/// Every launch setting joins this object, because the CLI reads a single `--settings` value.
/// A configured `--settings` argument stays refused for every run. An organizational launch
/// confines the file tools to the working directory and sends every web search and fetch to
/// Ganbaru as a permission request, which auto mode would otherwise answer itself; a scope that
/// loads the shell sends every command the same way: the ask rule outranks the read-only
/// commands manual mode runs unasked, and plan mode keeps its classifier from approving them.
/// Because an organizational launch loads no settings files, it also carries the allowlisted
/// user preferences, which the keys Ganbaru sets override.
pub fn launch_settings(
    fast_mode: Option<bool>,
    organizational: Option<ClaudeOrganizationalLaunch<'_>>,
) -> Option<String> {
    let mut settings = organizational
        .map(|organizational| organizational.user_settings.clone())
        .unwrap_or_default();
    if let Some(fast_mode) = fast_mode {
        settings.insert("fastMode".to_string(), Value::Bool(fast_mode));
    }
    if let Some(scope) = organizational.map(|organizational| organizational.scope) {
        let mut permissions = Map::from_iter([
            (
                "blockReadsOutsideWorkingDirectories".to_string(),
                Value::Bool(true),
            ),
            (
                "blockWritesOutsideWorkingDirectories".to_string(),
                Value::Bool(true),
            ),
        ]);
        let mut ask = vec!["WebSearch", "WebFetch"];
        if organizational_runs_commands(scope) {
            ask.push("Bash");
            settings.insert("useAutoModeDuringPlan".to_string(), Value::Bool(false));
        }
        permissions.insert("ask".to_string(), json!(ask));
        settings.insert("permissions".to_string(), Value::Object(permissions));
    }
    (!settings.is_empty()).then(|| Value::Object(settings).to_string())
}

pub fn permission_mode(modes: TurnModeSnapshot) -> Option<&'static str> {
    if modes.interaction_mode == InteractionMode::Plan {
        return Some("plan");
    }
    match modes.safety_mode {
        SafetyMode::AskForApproval => Some("acceptEdits"),
        SafetyMode::ApproveForMe => Some("auto"),
        SafetyMode::FullAccess => Some("bypassPermissions"),
        SafetyMode::Custom => None,
    }
}

pub fn control_success(request_id: &str, response: Value) -> ChatResult<Value> {
    validate_protocol_id(request_id, "requestId")?;
    if !response.is_object() {
        return Err(protocol_error("control response body"));
    }
    Ok(json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": response,
        },
    }))
}

/// Answers a control request with the CLI's error shape, used for subtypes Ganbaru does not
/// serve so the CLI sees a refusal rather than a successful empty result.
pub fn control_error(request_id: &str, message: &str) -> ChatResult<Value> {
    validate_protocol_id(request_id, "requestId")?;
    Ok(json!({
        "type": "control_response",
        "response": {
            "subtype": "error",
            "request_id": request_id,
            "error": message,
        },
    }))
}

pub fn decode_initialize(value: Value) -> ChatResult<ClaudeInitializeResponse> {
    serde_json::from_value(value).map_err(|_| protocol_error("initialize response"))
}

pub fn build_user_message(request: &SendTurnRequest) -> ChatResult<Value> {
    if request.prompt.len() > MAX_PROMPT_BYTES || request.prompt.contains('\0') {
        return Err(ChatError::validation(
            "prompt",
            "Claude prompt exceeds the supported limit",
        ));
    }
    session_instructions(request.developer_instructions.as_deref())?;
    validate_model_options(&request.model_options)?;
    let mut content = Vec::new();
    let mut prompt = request.prompt.clone();
    if !request.mentions.is_empty() {
        let paths = request
            .mentions
            .iter()
            .map(|mention| format!("@{}", mention.relative_path))
            .collect::<Vec<_>>()
            .join("\n");
        prompt.push_str("\n\nReferenced workspace paths:\n");
        prompt.push_str(&paths);
    }
    if !prompt.is_empty() {
        content.push(json!({ "type": "text", "text": prompt }));
    }
    for attachment in &request.attachments {
        match attachment.kind.as_str() {
            "image" => content.push(image_content(attachment)?),
            "text_snippet" => {
                let text = attachment.text_content.as_deref().ok_or_else(|| {
                    ChatError::validation("attachments", "Claude text context is unavailable")
                })?;
                if text.len() > MAX_TEXT_ATTACHMENT_BYTES || text.contains('\0') {
                    return Err(ChatError::validation(
                        "attachments",
                        "Claude text context exceeds the supported limit",
                    ));
                }
                content.push(json!({ "type": "text", "text": text }));
            }
            _ => {
                return Err(ChatError::unsupported(
                    "Claude prompt attachment kind is unsupported",
                ));
            }
        }
    }
    if content.is_empty() {
        return Err(ChatError::validation(
            "prompt",
            "Claude turn requires text or an image",
        ));
    }
    Ok(json!({
        "type": "user",
        "message": { "role": "user", "content": content },
        "parent_tool_use_id": Value::Null,
        "session_id": "",
    }))
}

pub fn build_slash_command_message(command: &str) -> ChatResult<Value> {
    if !command.starts_with('/')
        || command.len() > MAX_PROMPT_BYTES
        || command.contains('\0')
        || command.contains('\n')
    {
        return Err(ChatError::validation(
            "command",
            "Claude slash command is invalid",
        ));
    }
    Ok(json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{ "type": "text", "text": command }],
        },
        "parent_tool_use_id": Value::Null,
        "session_id": "",
    }))
}

fn image_content(attachment: &PromptAttachmentReference) -> ChatResult<Value> {
    let path = attachment.local_path.as_deref().ok_or_else(|| {
        ChatError::validation("attachments", "Claude image attachment is unavailable")
    })?;
    let path = Path::new(path);
    let metadata = fs::metadata(path).map_err(|_| {
        ChatError::validation("attachments", "Claude image attachment is unavailable")
    })?;
    if !path.is_absolute() || !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES {
        return Err(ChatError::validation(
            "attachments",
            "Claude image attachment is invalid or oversized",
        ));
    }
    let mime = attachment
        .mime_type
        .as_deref()
        .filter(|value| {
            matches!(
                *value,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
        })
        .ok_or_else(|| ChatError::validation("attachments", "Claude image type is unsupported"))?;
    let bytes = fs::read(path).map_err(|_| {
        ChatError::validation("attachments", "Claude image attachment could not be read")
    })?;
    Ok(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": mime,
            "data": general_purpose::STANDARD.encode(bytes),
        },
    }))
}

pub fn parse_resume_cursor(value: &VersionedJson) -> ChatResult<ClaudeResumeCursor> {
    if value.schema_version != 1 {
        return Err(ChatError::validation(
            "resumeCursor.schemaVersion",
            "Claude resume cursor schema is unsupported",
        ));
    }
    let object = value
        .value
        .as_object()
        .ok_or_else(|| ChatError::validation("resumeCursor", "Claude resume cursor is invalid"))?;
    let session_uuid = required_text(object, "sessionUuid")?.to_string();
    validate_uuid(&session_uuid, "resumeCursor.sessionUuid")?;
    let last_assistant_uuid = object
        .get("lastAssistantUuid")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(uuid) = last_assistant_uuid.as_deref() {
        validate_uuid(uuid, "resumeCursor.lastAssistantUuid")?;
    }
    let turn_count = object.get("turnCount").and_then(Value::as_u64).unwrap_or(0);
    Ok(ClaudeResumeCursor {
        session_uuid,
        last_assistant_uuid,
        turn_count,
    })
}

pub fn resume_cursor(cursor: &ClaudeResumeCursor) -> VersionedJson {
    VersionedJson {
        schema_version: 1,
        value: json!({
            "sessionUuid": cursor.session_uuid,
            "lastAssistantUuid": cursor.last_assistant_uuid,
            "turnCount": cursor.turn_count,
        }),
    }
}

/// The Claude model value that follows the account's default model instead of naming one.
const CLAUDE_DEFAULT_MODEL_ROUTE: &str = "default";

/// Replaces Claude's default route with the model it resolves to, since the route follows
/// whatever the account default becomes. The resolved model is kept only when no other entry
/// already runs it, such as the `opus` alias.
fn without_default_route(models: Vec<ClaudeModel>) -> Vec<ClaudeModel> {
    let without_context = |id: &str| id.strip_suffix("[1m]").unwrap_or(id).to_string();
    let resolved_elsewhere: BTreeSet<String> = models
        .iter()
        .filter(|model| model.value != CLAUDE_DEFAULT_MODEL_ROUTE)
        .flat_map(|model| [Some(model.value.as_str()), model.resolved_model.as_deref()])
        .flatten()
        .map(without_context)
        .collect();
    models
        .into_iter()
        .filter_map(|model| {
            if model.value != CLAUDE_DEFAULT_MODEL_ROUTE {
                return Some(model);
            }
            let resolved = model.resolved_model.clone()?;
            // A context-window suffix is a launch setting, not a model id the picker can name.
            if resolved.contains('[') || resolved_elsewhere.contains(&without_context(&resolved)) {
                return None;
            }
            let label = resolved_claude_model_label(&resolved)?;
            Some(ClaudeModel {
                value: resolved.clone(),
                resolved_model: Some(resolved),
                display_name: label,
                description: None,
                ..model
            })
        })
        .collect()
}

pub fn provider_models(models: Vec<ClaudeModel>) -> ChatResult<Vec<ProviderModel>> {
    let mut output = Vec::new();
    let mut seen = BTreeSet::new();
    for model in without_default_route(models) {
        validate_model_id(&model.value)?;
        if !seen.insert(model.value.clone()) {
            continue;
        }
        let display_name = claude_model_display_name(&model);
        let options = claude_model_options(
            model.supports_effort,
            &model.supported_effort_levels,
            model.supports_fast_mode,
        );
        output.push(ProviderModel {
            id: ModelId::new(model.value.clone()).map_err(|_| protocol_error("model ID"))?,
            display_name: if display_name.is_empty() {
                model.value
            } else {
                display_name
            },
            description: model.description.filter(|value| !value.trim().is_empty()),
            context_limit: None,
            availability: ModelAvailability::Available,
            capabilities: claude_model_capabilities(),
            options,
        });
    }
    Ok(output)
}

fn claude_model_capabilities() -> Vec<ProviderCapability> {
    vec![
        ProviderCapability::NativeResume,
        ProviderCapability::NativePlan,
        ProviderCapability::DynamicModelChange,
        ProviderCapability::Images,
        ProviderCapability::FileReferences,
        ProviderCapability::Approvals,
        ProviderCapability::StructuredQuestions,
        ProviderCapability::ReasoningSummaries,
        ProviderCapability::StructuredPlans,
        ProviderCapability::ContextUsage,
        ProviderCapability::CostReporting,
        ProviderCapability::TaskActivity,
    ]
}

fn claude_model_display_name(model: &ClaudeModel) -> String {
    let display_name = model.display_name.trim();
    if display_name.is_empty() {
        return String::new();
    }
    if display_name
        .chars()
        .any(|character| character.is_ascii_digit())
    {
        return display_name.to_string();
    }
    if let Some(versioned_name) = model
        .description
        .as_deref()
        .and_then(|description| description.split('·').next())
        .map(str::trim)
        .filter(|candidate| {
            candidate.len() > display_name.len()
                && candidate
                    .get(..display_name.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(display_name))
                && candidate
                    .get(display_name.len()..)
                    .is_some_and(|suffix| suffix.chars().next().is_some_and(char::is_whitespace))
        })
    {
        return versioned_name.to_string();
    }
    display_name.to_string()
}

fn resolved_claude_model_label(model_id: &str) -> Option<String> {
    let without_context = model_id.strip_suffix("[1m]").unwrap_or(model_id);
    let mut parts = without_context.strip_prefix("claude-")?.split('-');
    let family = parts.next()?;
    if family.is_empty()
        || !family
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    let version = parts
        .take(2)
        .take_while(|part| {
            !part.is_empty()
                && part.len() <= 2
                && part.chars().all(|character| character.is_ascii_digit())
        })
        .collect::<Vec<_>>();
    if version.is_empty() {
        return None;
    }
    let mut label = family.to_string();
    if let Some(first) = label.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    label.push(' ');
    label.push_str(&version.join("."));
    Some(label)
}

fn claude_model_options(
    supports_effort: bool,
    supported_effort_levels: &[String],
    supports_fast_mode: bool,
) -> Vec<ModelOptionDefinition> {
    let mut definitions = Vec::new();
    if supports_effort {
        let mut seen = BTreeSet::new();
        let levels = supported_effort_levels
            .iter()
            .filter(|level| is_claude_effort(level))
            .filter(|level| seen.insert(level.as_str()))
            .map(|level| ModelChoiceOption {
                value: level.clone(),
                label: level.clone(),
                description: None,
            })
            .collect::<Vec<_>>();
        if !levels.is_empty() {
            let default_value = levels
                .iter()
                .find(|level| level.value == "high")
                .map(|level| level.value.clone());
            definitions.push(ModelOptionDefinition::Choice {
                key: "effort".to_string(),
                label: "Effort".to_string(),
                description: Some("Provider-supported reasoning effort".to_string()),
                options: levels,
                default_value,
            });
        }
    }
    if supports_fast_mode {
        definitions.push(ModelOptionDefinition::Boolean {
            key: "fastMode".to_string(),
            label: "Fast mode".to_string(),
            description: Some("Lower latency with higher usage cost".to_string()),
            default_value: Some(false),
        });
    }
    definitions
}

pub fn selected_effort(options: &[ModelOptionSelection]) -> ChatResult<Option<String>> {
    validate_model_options(options)?;
    for option in options {
        if option.key == "effort" {
            let ModelOptionValue::Choice(value) = &option.value else {
                return Err(ChatError::validation(
                    "modelOptions",
                    "Claude effort must use a single choice",
                ));
            };
            return Ok(Some(value.clone()));
        }
    }
    Ok(None)
}

pub fn selected_fast_mode(options: &[ModelOptionSelection]) -> ChatResult<Option<bool>> {
    validate_model_options(options)?;
    for option in options {
        if option.key == "fastMode" {
            let ModelOptionValue::Boolean(value) = &option.value else {
                return Err(ChatError::validation(
                    "modelOptions",
                    "Claude Fast mode must use a boolean",
                ));
            };
            return Ok(Some(*value));
        }
    }
    Ok(None)
}

/// Effort levels Ganbaru passes to Claude, at launch and in a running session.
pub const CLAUDE_EFFORT_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Whether `value` is one of [`CLAUDE_EFFORT_LEVELS`].
pub fn is_claude_effort(value: &str) -> bool {
    CLAUDE_EFFORT_LEVELS.contains(&value)
}

/// Builds the `apply_flag_settings` payload that changes effort and Fast mode in a running
/// session. The CLI acknowledges an unknown effort level, so the level is checked here.
///
/// # Errors
///
/// Returns a `modelOptions` validation error for an unsupported effort level.
pub fn apply_flag_settings_payload(
    effort: Option<&str>,
    fast_mode: Option<bool>,
) -> ChatResult<Value> {
    if effort.is_some_and(|effort| !is_claude_effort(effort)) {
        return Err(ChatError::validation(
            "modelOptions",
            "Claude effort value is unsupported",
        ));
    }
    Ok(json!({ "settings": { "effortLevel": effort, "fastMode": fast_mode } }))
}

fn validate_model_options(options: &[ModelOptionSelection]) -> ChatResult<()> {
    if options.len() > MAX_MODEL_OPTIONS {
        return Err(ChatError::validation(
            "modelOptions",
            "Claude model options exceed the supported limit",
        ));
    }
    let mut keys = BTreeSet::new();
    for option in options {
        if !keys.insert(option.key.as_str()) {
            return Err(ChatError::validation(
                "modelOptions",
                "Claude model option is unsupported or duplicated",
            ));
        }
        match option.key.as_str() {
            "effort" => {
                let ModelOptionValue::Choice(value) = &option.value else {
                    return Err(ChatError::validation(
                        "modelOptions",
                        "Claude effort must use a single choice",
                    ));
                };
                if !is_claude_effort(value) {
                    return Err(ChatError::validation(
                        "modelOptions",
                        "Claude effort value is unsupported",
                    ));
                }
            }
            "fastMode" if matches!(&option.value, ModelOptionValue::Boolean(_)) => {}
            _ => {
                return Err(ChatError::validation(
                    "modelOptions",
                    "Claude model option is unsupported or has the wrong type",
                ));
            }
        }
    }
    Ok(())
}

/// Configured arguments refused for every run: they would replace the managed transport,
/// permission handling, or session continuation, or select, relocate, or detach the session.
const PROTECTED_LAUNCH_ARGUMENTS: &[&str] = &[
    "-p",
    "--print",
    "--output-format",
    "--input-format",
    "--permission-prompt-tool",
    "--include-partial-messages",
    "--replay-user-messages",
    "--permission-mode",
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "-c",
    "--continue",
    "-r",
    "--fork-session",
    "--resume",
    "--resume-session-at",
    "--session-id",
    "--no-session-persistence",
    "--from-pr",
    "--teleport",
    "--cloud",
    "--environment",
    "--bg",
    "--background",
    "--desktop",
    "-w",
    "--worktree",
    "--model",
    "--effort",
    "--settings",
    "--mcp-config",
];

fn validate_launch_arguments(arguments: &[String]) -> ChatResult<()> {
    if arguments.len() > MAX_LAUNCH_ARGUMENTS
        || arguments.iter().any(|argument| {
            argument.len() > MAX_LAUNCH_ARGUMENT_BYTES
                || argument.contains('\0')
                || names_flag(argument, PROTECTED_LAUNCH_ARGUMENTS)
        })
    {
        return Err(ChatError::validation(
            "launchArguments",
            "Claude launch arguments conflict with the managed transport",
        ));
    }
    Ok(())
}

/// Refuses every configured argument on an organizational launch. Each flag the CLI accepts
/// could select another session or widen the tool, settings, agent, prompt, or folder
/// surface, and a deny list cannot keep up with new CLI flags.
fn validate_organizational_launch_arguments(arguments: &[String]) -> ChatResult<()> {
    if !arguments.is_empty() {
        return Err(ChatError::validation(
            "launchArguments",
            "Launch arguments do not apply to organizational runs; remove them from this provider instance or use another instance for agents",
        ));
    }
    Ok(())
}

/// Whether `argument` names one of `flags`.
///
/// Long flags match in the spaced and the `--flag=value` form. A single-letter flag matches
/// any single-dash argument that contains its letter, because the CLI reads `-pc` as two
/// flags and `-r<id>` as `-r` with an attached value. This also refuses some harmless
/// clusters, which keeps the check independent of which letters take values.
fn names_flag(argument: &str, flags: &[&str]) -> bool {
    flags.iter().any(|flag| match short_flag_letter(flag) {
        Some(letter) => argument
            .strip_prefix('-')
            .is_some_and(|cluster| !cluster.starts_with('-') && cluster.contains(letter)),
        None => argument
            .strip_prefix(flag)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('=')),
    })
}

/// The letter of a single-letter flag such as `-p`, or none for a long flag.
fn short_flag_letter(flag: &str) -> Option<char> {
    let mut letters = flag.strip_prefix('-')?.chars();
    match (letters.next(), letters.next()) {
        (Some(letter), None) if letter != '-' => Some(letter),
        _ => None,
    }
}

fn validate_model_id(value: &str) -> ChatResult<()> {
    if value.trim().is_empty()
        || value.len() > MAX_MODEL_ID_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "modelId",
            "Claude model identifier is invalid",
        ));
    }
    Ok(())
}

pub fn validate_uuid(value: &str, field: &str) -> ChatResult<()> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());
    if !valid {
        return Err(ChatError::validation(field, "Claude UUID is invalid"));
    }
    Ok(())
}

fn validate_protocol_id(value: &str, field: &str) -> ChatResult<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            field,
            "Claude protocol identifier is invalid",
        ));
    }
    Ok(())
}

fn required_text<'a>(object: &'a Map<String, Value>, key: &str) -> ChatResult<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| protocol_error(key))
}

fn unsupported_version_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::UnsupportedVersion,
        "Claude Code version could not be verified",
        true,
    )
}

pub fn protocol_error(label: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("Claude returned an invalid {label}"),
        false,
    )
}
