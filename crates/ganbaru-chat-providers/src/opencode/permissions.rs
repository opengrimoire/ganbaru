//! Canonical safety-mode mappings for OpenCode permission rules.

use crate::models::{
    ApprovalDecisionKind, ChatError, ChatErrorCode, ChatResult, InteractionMode,
    ProviderFolderCapability, ProviderOrganizationalScope, SafetyMode, TurnModeSnapshot,
};
use ganbaru_chat_contracts::web_fetch::fetchable_url;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Edit targets that keep asking while workspace edits are approved
/// automatically. OpenCode applies the last matching rule, `*` spans path
/// separators, and matching folds case on Windows. Edits to repository
/// metadata can make later Git runs execute programs, so they stay reviewed.
pub const REPOSITORY_METADATA_EDIT_PATTERNS: &[&str] = &[".git", ".git/*", "*/.git", "*/.git/*"];
/// Edit paths that name a hidden file or folder at any depth. Organizational edits there ask,
/// because hidden paths hold provider and tool configuration that can change what later runs
/// execute.
const HIDDEN_EDIT_PATTERNS: &[&str] = &[".*", "*/.*"];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodePermissionRule {
    pub permission: String,
    pub pattern: String,
    pub action: OpenCodePermissionAction,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenCodePermissionAction {
    Allow,
    Ask,
    /// OpenCode fails the call, and a `*` deny drops the tool from the model's tool list.
    Deny,
}

/// Native permissions an organizational run uses without asking: workspace reads and searches,
/// its own task list, and questions to the person. Reads outside the working directory ask
/// `external_directory` first, which organizational rules deny.
const ORGANIZATIONAL_READ_PERMISSIONS: &[&str] =
    &["read", "glob", "grep", "list", "todowrite", "question"];

/// Native permissions no organizational run uses: paths outside the working directory, code
/// search, subagents, skills, language servers, and agent switches. Listed last so a later rule
/// never reopens them.
const ORGANIZATIONAL_DENIED_PERMISSIONS: &[&str] = &[
    "external_directory",
    "codesearch",
    "task",
    "skill",
    "lsp",
    "plan_enter",
    "plan_exit",
    "doom_loop",
];

/// The session rules of an organizational run, which OpenCode evaluates after the agent and
/// configuration rules so the last match here decides.
///
/// Every permission starts denied. Reads stay inside the working directory. In Build in a
/// scope that edits, edits inside it run without asking, edits of hidden files and folders
/// (provider configuration and the like) ask, repository metadata edits are denied, and the
/// shell acts as [`organizational_shell_action`] decides in a scope that runs commands. Web
/// searches run without asking, and every web fetch asks, because OpenCode fetches from this
/// device and can reach its local network. Internal host tools ask unless the launch pre-approves them; OpenCode names an MCP tool
/// `<server>_<tool>`.
pub fn organizational_rules(
    scope: ProviderOrganizationalScope,
    modes: TurnModeSnapshot,
    internal_server: &str,
    pre_approved_tools: &[String],
    commands_confined: bool,
) -> ChatResult<Vec<OpenCodePermissionRule>> {
    let shell_action = if organizational_runs_commands(scope) {
        Some(organizational_shell_action(
            modes.safety_mode,
            commands_confined,
        )?)
    } else {
        None
    };
    let mut rules = vec![rule("*", OpenCodePermissionAction::Deny)];
    rules.extend(
        ORGANIZATIONAL_READ_PERMISSIONS
            .iter()
            .map(|permission| rule(permission, OpenCodePermissionAction::Allow)),
    );
    if organizational_edits(scope, modes) {
        rules.push(rule("edit", OpenCodePermissionAction::Allow));
        rules.extend(
            HIDDEN_EDIT_PATTERNS
                .iter()
                .map(|pattern| OpenCodePermissionRule {
                    permission: "edit".to_string(),
                    pattern: (*pattern).to_string(),
                    action: OpenCodePermissionAction::Ask,
                }),
        );
        rules.extend(REPOSITORY_METADATA_EDIT_PATTERNS.iter().map(|pattern| {
            OpenCodePermissionRule {
                permission: "edit".to_string(),
                pattern: (*pattern).to_string(),
                action: OpenCodePermissionAction::Deny,
            }
        }));
    }
    if let Some(action) = shell_action {
        rules.push(rule("bash", action));
    }
    rules.push(rule("websearch", OpenCodePermissionAction::Allow));
    rules.push(rule("webfetch", OpenCodePermissionAction::Ask));
    let server = opencode_tool_key_part(internal_server);
    rules.push(rule(&format!("{server}_*"), OpenCodePermissionAction::Ask));
    let claude_prefix = format!("mcp__{internal_server}__");
    rules.extend(
        pre_approved_tools
            .iter()
            .filter_map(|tool| tool.strip_prefix(&claude_prefix))
            .filter(|tool| !tool.is_empty())
            .map(|tool| {
                rule(
                    &format!("{server}_{}", opencode_tool_key_part(tool)),
                    OpenCodePermissionAction::Allow,
                )
            }),
    );
    rules.extend(
        ORGANIZATIONAL_DENIED_PERMISSIONS
            .iter()
            .map(|permission| rule(permission, OpenCodePermissionAction::Deny)),
    );
    Ok(rules)
}

/// How the shell of a scope that runs commands acts under `safety_mode`.
///
/// Every command asks the person in AskForApproval. When the sandbox helper confines every
/// command, ApproveForMe (which the Work on their own access level runs) and FullAccess run
/// them without asking. Unconfined commands need AskForApproval, where each approval is the
/// boundary, and a Custom mode is never proven.
pub fn organizational_shell_action(
    safety_mode: SafetyMode,
    commands_confined: bool,
) -> ChatResult<OpenCodePermissionAction> {
    match (safety_mode, commands_confined) {
        (SafetyMode::AskForApproval, _) => Ok(OpenCodePermissionAction::Ask),
        (SafetyMode::ApproveForMe | SafetyMode::FullAccess, true) => {
            Ok(OpenCodePermissionAction::Allow)
        }
        _ => Err(organizational_permission_error()),
    }
}

/// Whether the scope loads the shell.
pub fn organizational_runs_commands(scope: ProviderOrganizationalScope) -> bool {
    matches!(
        scope,
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Execute | ProviderFolderCapability::Publish,
        } | ProviderOrganizationalScope::Scratch
    )
}

/// Whether a Build turn in the scope may propose file edits. Plan turns never edit.
pub fn organizational_edits(scope: ProviderOrganizationalScope, modes: TurnModeSnapshot) -> bool {
    modes.interaction_mode == InteractionMode::Build
        && match scope {
            ProviderOrganizationalScope::Conversation
            | ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Read,
            } => false,
            ProviderOrganizationalScope::Folder { .. } | ProviderOrganizationalScope::Scratch => {
                true
            }
        }
}

/// OpenCode's tool key form of a server or tool name: every character outside ASCII letters,
/// digits, `_`, and `-` becomes `_`.
fn opencode_tool_key_part(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// The error of an organizational OpenCode run whose boundary cannot be proven.
pub fn organizational_permission_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "OpenCode could not prove the required organizational authority boundary",
        false,
    )
}

/// Host-side routing of an organizational session's permission requests, as defense in depth
/// behind the session rules: a request the scope cannot need is rejected before it reaches the
/// person, and nothing grants a session-wide rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeOrganizationalPolicy {
    scope: ProviderOrganizationalScope,
    internal_prefix: String,
}

impl OpenCodeOrganizationalPolicy {
    pub fn new(scope: ProviderOrganizationalScope, internal_server: &str) -> Self {
        Self {
            scope,
            internal_prefix: format!("{}_", opencode_tool_key_part(internal_server)),
        }
    }

    /// Whether a permission request may reach the person: an internal host tool, a shell
    /// command in a scope that runs commands, a web fetch of http or https URLs, or an edit of
    /// relative workspace paths outside repository metadata in a scope and turn that edit.
    pub fn admits(&self, permission: &str, patterns: &[String], modes: TurnModeSnapshot) -> bool {
        if permission.len() > self.internal_prefix.len()
            && permission.starts_with(&self.internal_prefix)
        {
            return true;
        }
        match permission {
            "bash" => organizational_runs_commands(self.scope),
            "webfetch" => !patterns.is_empty() && patterns.iter().all(|url| fetchable_url(url)),
            "edit" => {
                organizational_edits(self.scope, modes)
                    && !patterns.is_empty()
                    && patterns
                        .iter()
                        .all(|pattern| workspace_edit_pattern(pattern))
            }
            _ => false,
        }
    }
}

/// Whether an edit pattern names a path inside the working directory and outside repository
/// metadata. OpenCode reports edit targets relative to the worktree.
fn workspace_edit_pattern(pattern: &str) -> bool {
    let path = std::path::Path::new(pattern);
    !pattern.is_empty()
        && !pattern.starts_with('~')
        && !pattern.contains('\0')
        && path.is_relative()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
        && !REPOSITORY_METADATA_EDIT_PATTERNS
            .iter()
            .any(|metadata| wildcard_matches(metadata, pattern))
}

pub fn permission_override(mode: SafetyMode) -> Option<Vec<OpenCodePermissionRule>> {
    (mode != SafetyMode::Custom).then(|| permission_rules(mode))
}

pub fn permission_rules(mode: SafetyMode) -> Vec<OpenCodePermissionRule> {
    let broad_action = match mode {
        SafetyMode::FullAccess => OpenCodePermissionAction::Allow,
        SafetyMode::AskForApproval | SafetyMode::ApproveForMe | SafetyMode::Custom => {
            OpenCodePermissionAction::Ask
        }
    };
    let mut rules = vec![rule("*", broad_action)];
    if mode == SafetyMode::AskForApproval {
        rules.push(rule("edit", OpenCodePermissionAction::Allow));
        rules.extend(REPOSITORY_METADATA_EDIT_PATTERNS.iter().map(|pattern| {
            OpenCodePermissionRule {
                permission: "edit".to_string(),
                pattern: (*pattern).to_string(),
                action: OpenCodePermissionAction::Ask,
            }
        }));
    }
    if mode != SafetyMode::FullAccess {
        for permission in [
            "bash",
            "webfetch",
            "websearch",
            "codesearch",
            "external_directory",
            "doom_loop",
        ] {
            rules.push(rule(permission, OpenCodePermissionAction::Ask));
        }
        rules.push(rule("question", OpenCodePermissionAction::Allow));
    }
    rules
}

/// Maps a Chat approval decision to the OpenCode permission reply.
///
/// Allow for session replies `once` because OpenCode persists `always` as a project rule; the
/// driver remembers the session approval in [`OpenCodeSessionRules`] instead.
pub fn permission_reply(decision: ApprovalDecisionKind) -> &'static str {
    match decision {
        ApprovalDecisionKind::AllowOnce | ApprovalDecisionKind::AllowSession => "once",
        ApprovalDecisionKind::Deny | ApprovalDecisionKind::Cancel => "reject",
    }
}

fn rule(permission: &str, action: OpenCodePermissionAction) -> OpenCodePermissionRule {
    OpenCodePermissionRule {
        permission: permission.to_string(),
        pattern: "*".to_string(),
        action,
    }
}

/// Session approvals one live OpenCode session remembers before the oldest is dropped.
pub const MAX_OPENCODE_SESSION_RULES: usize = 64;

/// One "allow for session" approval kept in driver memory for a live session.
///
/// OpenCode stores an `always` reply as a project-wide rule that outlives the Chat session, so
/// Ganbaru replies `once` and answers later matching requests itself. A rule without a pattern
/// covers every request for its permission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeSessionRule {
    pub permission: String,
    pub pattern: Option<String>,
}

/// Bounded, insertion-ordered session approvals for one live OpenCode session.
#[derive(Clone, Debug, Default)]
pub struct OpenCodeSessionRules {
    rules: VecDeque<OpenCodeSessionRule>,
}

impl OpenCodeSessionRules {
    /// Adds rules, ignoring duplicates, and returns how many of the oldest rules were dropped to
    /// stay within [`MAX_OPENCODE_SESSION_RULES`].
    pub fn grant(&mut self, rules: impl IntoIterator<Item = OpenCodeSessionRule>) -> usize {
        let mut dropped = 0;
        for rule in rules {
            if self.rules.contains(&rule) {
                continue;
            }
            self.rules.push_back(rule);
            while self.rules.len() > MAX_OPENCODE_SESSION_RULES {
                self.rules.pop_front();
                dropped += 1;
            }
        }
        dropped
    }

    /// Whether remembered approvals cover a request for `permission` over every one of
    /// `patterns`. A request without patterns is covered only by a rule without a pattern.
    pub fn covers(&self, permission: &str, patterns: &[String]) -> bool {
        let mut rules = self
            .rules
            .iter()
            .filter(|rule| rule.permission == permission);
        if patterns.is_empty() {
            return rules.any(|rule| rule.pattern.is_none());
        }
        let rules = rules.collect::<Vec<_>>();
        patterns.iter().all(|value| {
            rules.iter().any(|rule| {
                rule.pattern
                    .as_deref()
                    .is_none_or(|pattern| wildcard_matches(pattern, value))
            })
        })
    }

    /// Number of remembered rules.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.rules.len()
    }
}

/// Matches OpenCode permission wildcards: `*` spans any text including path separators, `?`
/// matches one character, and a trailing ` *` also matches the bare command. Matching folds case
/// on Windows, as OpenCode does.
pub fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = fold_case(pattern);
    let value = fold_case(value);
    if let Some(prefix) = pattern.strip_suffix(" *")
        && glob_matches(prefix, &value)
    {
        return true;
    }
    glob_matches(&pattern, &value)
}

fn fold_case(text: &str) -> String {
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_string()
    }
}

/// Iterative glob match with single-star backtracking, linear in practice and quadratic at worst.
fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let value = value.chars().collect::<Vec<_>>();
    let (mut pattern_index, mut value_index) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while value_index < value.len() {
        match pattern.get(pattern_index) {
            Some('*') => {
                backtrack = Some((pattern_index, value_index));
                pattern_index += 1;
            }
            Some(&character) if character == '?' || character == value[value_index] => {
                pattern_index += 1;
                value_index += 1;
            }
            _ => match backtrack {
                Some((star, matched)) => {
                    pattern_index = star + 1;
                    value_index = matched + 1;
                    backtrack = Some((star, matched + 1));
                }
                None => return false,
            },
        }
    }
    pattern[pattern_index..]
        .iter()
        .all(|character| *character == '*')
}
