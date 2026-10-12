use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::adaptive::decision::{AdaptiveDecision, AdaptiveDecisionInput};
use crate::adaptive::models::{CountRhythm, FeatureVector};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayOpportunity {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub input: AdaptiveDecisionInput,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    pub opportunity_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub decision: AdaptiveDecision,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaySummary {
    pub total_opportunities: usize,
    pub decision_mode_counts: BTreeMap<String, usize>,
    pub reason_code_counts: BTreeMap<String, usize>,
    pub experiment_assignment_counts: BTreeMap<String, usize>,
    pub changed_value_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedOutcome {
    pub opportunity_id: String,
    pub features: FeatureVector,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_rhythm: Option<CountRhythm>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    #[default]
    AllObserved,
    SelectedRhythmMatch,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeScore {
    pub attribution: Attribution,
    pub total_joined_outcomes: usize,
    pub scored_outcome_count: usize,
    pub matched_selected_rhythm_count: usize,
    pub mismatched_selected_rhythm_count: usize,
    pub unknown_selected_rhythm_count: usize,
    pub completed_focus_segments: f64,
    pub interrupted_focus_segments: f64,
    pub focus_failure_count: f64,
    pub stop_count: f64,
    pub clean_focus_seconds: f64,
    pub planned_focus_seconds: f64,
    pub blocked_attempt_count: f64,
    pub break_skipped_count: f64,
    pub short_break_overtime_seconds: f64,
    pub long_break_overtime_seconds: f64,
    pub completion_rate: Option<f64>,
    pub clean_focus_ratio: Option<f64>,
    pub blocked_attempts_mean: Option<f64>,
    pub break_skipped_mean: Option<f64>,
    pub short_break_overtime_seconds_mean: Option<f64>,
    pub long_break_overtime_seconds_mean: Option<f64>,
    pub guardrail_breach_count: usize,
    pub guardrail_breach_rate: Option<f64>,
    pub guardrail_reason_counts: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateObservedScore {
    pub candidate_id: String,
    pub opportunity_ids: Vec<String>,
    pub outcome_score: OutcomeScore,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundedCandidate {
    pub id: String,
    pub focus_duration_minutes: Option<f64>,
    pub short_break_minutes: Option<f64>,
    pub long_break_minutes: Option<f64>,
    pub long_break_after_focus_count: Option<f64>,
    pub focus_duration_delta_minutes: Option<f64>,
    pub short_break_delta_minutes: Option<f64>,
    pub long_break_delta_minutes: Option<f64>,
    pub long_break_after_focus_count_delta: Option<f64>,
    pub allowed_decision_modes: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyEvaluation {
    pub candidate_id: String,
    pub results: Vec<ReplayResult>,
    pub summary: ReplaySummary,
    pub outcome_score: OutcomeScore,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEvaluation {
    pub context_key: String,
    #[serde(flatten)]
    pub evaluation: PolicyEvaluation,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextReport {
    pub context_key: String,
    pub evaluations: Vec<ContextEvaluation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Pass,
    Fail,
    InsufficientEvidence,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GateOptions {
    pub min_matched_outcomes_per_context: Option<f64>,
    pub max_guardrail_breach_rate: Option<f64>,
    #[serde(default)]
    pub observed_candidate_outcome_scores: Vec<CandidateObservedScore>,
    pub min_observed_candidate_outcomes: Option<f64>,
    pub max_observed_candidate_guardrail_breach_rate: Option<f64>,
    pub min_interaction_matched_outcomes: Option<f64>,
    pub max_interaction_guardrail_breach_rate_increase: Option<f64>,
    pub max_interaction_clean_focus_ratio_drop: Option<f64>,
    pub max_interaction_completion_rate_drop: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextGate {
    pub candidate_id: String,
    pub context_key: String,
    pub status: GateStatus,
    pub reasons: Vec<String>,
    pub scored_outcome_count: usize,
    pub min_matched_outcomes_per_context: usize,
    pub guardrail_breach_rate: Option<f64>,
    pub max_guardrail_breach_rate: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyGate {
    pub candidate_id: String,
    pub status: GateStatus,
    pub reasons: Vec<String>,
    pub context_gates: Vec<ContextGate>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedGate {
    pub candidate_id: String,
    pub status: GateStatus,
    pub reasons: Vec<String>,
    pub scored_outcome_count: usize,
    pub min_observed_candidate_outcomes: usize,
    pub guardrail_breach_rate: Option<f64>,
    pub max_observed_candidate_guardrail_breach_rate: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Usable,
    Unsafe,
    Inconclusive,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionStatus {
    Favorable,
    Neutral,
    Antagonistic,
    InsufficientEvidence,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interaction {
    pub candidate_id: String,
    pub parameter_keys: Vec<String>,
    pub component_candidate_ids: Vec<String>,
    pub status: InteractionStatus,
    pub reasons: Vec<String>,
    pub combined_evaluation: PolicyEvaluation,
    pub component_evaluations: Vec<PolicyEvaluation>,
    pub best_component_evaluation: Option<PolicyEvaluation>,
    pub guardrail_breach_rate_increase: Option<f64>,
    pub clean_focus_ratio_drop: Option<f64>,
    pub completion_rate_drop: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateReview {
    pub candidate_id: String,
    pub status: CandidateStatus,
    pub evaluation: PolicyEvaluation,
    pub gate: PolicyGate,
    pub observed_outcome_gate: ObservedGate,
    pub interaction: Option<Interaction>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateWorkflow {
    pub evaluations: Vec<PolicyEvaluation>,
    pub context_reports: Vec<ContextReport>,
    pub gates: Vec<PolicyGate>,
    pub observed_outcome_gates: Vec<ObservedGate>,
    pub interactions: Vec<Interaction>,
    pub reviews: Vec<CandidateReview>,
    pub usable_candidate_ids: Vec<String>,
    pub unsafe_candidate_ids: Vec<String>,
    pub inconclusive_candidate_ids: Vec<String>,
}
