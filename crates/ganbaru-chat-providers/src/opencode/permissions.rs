//! Canonical safety-mode mappings for OpenCode permission rules.

use crate::models::{ApprovalDecisionKind, SafetyMode};
use serde::{Deserialize, Serialize};

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

pub fn permission_reply(decision: ApprovalDecisionKind) -> &'static str {
    match decision {
        ApprovalDecisionKind::AllowOnce => "once",
        ApprovalDecisionKind::AllowSession => "always",
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
