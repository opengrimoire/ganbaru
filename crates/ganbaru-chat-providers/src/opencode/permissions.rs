//! Canonical safety-mode mappings for OpenCode permission rules.

use crate::models::{ApprovalDecisionKind, SafetyMode};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Edit targets that keep asking while workspace edits are approved
/// automatically. OpenCode applies the last matching rule, `*` spans path
/// separators, and matching folds case on Windows. Edits to repository
/// metadata can make later Git runs execute programs, so they stay reviewed.
pub const REPOSITORY_METADATA_EDIT_PATTERNS: &[&str] = &[".git", ".git/*", "*/.git", "*/.git/*"];

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
