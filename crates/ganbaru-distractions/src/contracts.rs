//! Distraction blocker wire shapes shared by native state files, commands, and usage records.

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsRuntimeState {
    pub active: bool,
    pub paused: bool,
    pub pause_reason: Option<String>,
    pub phase: String,
    pub active_run_id: Option<String>,
    pub active_occurrence_id: Option<String>,
    pub remaining_seconds: Option<i64>,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until_ms: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsExtensionConnectionFile {
    pub last_seen_at: String,
    pub last_message_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsExtensionStatus {
    pub connected: bool,
    pub last_seen_at: Option<String>,
    pub last_message_type: Option<String>,
    pub checked_at: String,
    pub stale_seconds: i64,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsUsageSampleInput {
    pub id: Option<String>,
    pub source_type: String,
    pub source_key: String,
    pub display_name: Option<String>,
    pub started_at_ms: i64,
    pub elapsed_seconds: i64,
    pub local_date: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsDesktopBlockEventInput {
    pub app_name: String,
    pub process_name: Option<String>,
    pub process_id: Option<u32>,
}

pub struct NormalizedDesktopBlockEvent {
    pub source_key: String,
    pub display_name: Option<String>,
    pub process_id: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsUsageSampleRow {
    pub id: String,
    pub source_type: String,
    pub source_key: String,
    pub display_name: Option<String>,
    pub started_at_ms: i64,
    pub elapsed_seconds: i64,
    pub local_date: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsLimitState {
    pub local_date: String,
    pub week_start_local_date: String,
    pub updated_at: String,
    #[serde(default)]
    pub database_path: Option<String>,
    #[serde(default)]
    pub configuration_digest: Option<String>,
    pub limits: Vec<DistractionsLimitStateItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsLimitStateItem {
    pub id: String,
    pub period: String,
    pub window_start_local_date: String,
    pub window_end_local_date: String,
    pub used_seconds: i64,
    pub limit_seconds: i64,
    pub remaining_seconds: i64,
    pub exhausted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsForegroundDesktopAppStatus {
    pub available: bool,
    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub process_id: Option<u32>,
    #[serde(skip_serializing)]
    pub process_identity: Option<String>,
    pub match_names: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsForegroundDesktopAppExpectation {
    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub process_id: Option<u32>,
    #[serde(default)]
    pub process_identity: Option<String>,
    pub match_names: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsDesktopAppCandidate {
    pub name: String,
    pub source: String,
    pub detail: Option<String>,
    pub process_names: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsDesktopAppRuleInput {
    pub rule_identity: DistractionsDesktopRuleIdentity,
    pub name: String,
    pub match_names: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DistractionsDesktopRuleIdentity {
    DesktopApp { rule_id: String },
    UsageLimit { rule_id: String, entry_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsRunningDesktopAppMatch {
    pub app_name: String,
    pub process_name: String,
    pub process_id: u32,
    pub process_identity: String,
    pub rule_identity: DistractionsDesktopRuleIdentity,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionsCloseDesktopAppRequest {
    pub process_id: u32,
    pub process_name: String,
    pub process_identity: String,
    pub rule_identity: DistractionsDesktopRuleIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedDesktopProcess {
    pub process_name: String,
    pub match_names: Vec<String>,
    pub process_identity: String,
}

#[derive(Clone, Debug)]
pub struct DesktopRuleMatcher {
    pub app_name: String,
    pub rule_identity: DistractionsDesktopRuleIdentity,
}

#[cfg(test)]
mod tests;
