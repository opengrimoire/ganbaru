use std::collections::BTreeSet;

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::adaptive::experiments::analysis::analyze_experiment;
use crate::adaptive::experiments::models::{
    AssignmentHistory, ExperimentAssignment, ExperimentLane, ExperimentOutcome, ExperimentState,
    ExperimentVariant, SelectionInput,
};
use crate::adaptive::experiments::selection::{
    experiment_context_key, experiment_cooldown_state, select_run_start_assignment,
};
use crate::adaptive::features::{derive_context_bucket, extract_adaptive_features};
use crate::adaptive::models::{
    BlockEventInput, ContextBucket, CountRhythm, FeatureInput, FeatureVector, LocalTimeFacts,
    RunEventInput, SegmentInput, StateScores,
};
use crate::adaptive::policy::select_adaptive_rhythm;
use crate::adaptive::state::derive_adaptive_state;

pub const POLICY_ID: &str = "local-adaptive-policy-v1";
pub const POLICY_VERSION: i64 = 1;
pub const MODEL_VERSION: i64 = 1;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextState {
    pub context_key: String,
    #[serde(flatten)]
    pub scores: StateScores,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveHistory {
    pub segments: Vec<SegmentInput>,
    pub run_events: Vec<RunEventInput>,
    pub block_events: Vec<BlockEventInput>,
    pub previous_states: Vec<ContextState>,
    pub experiment_states: Vec<ExperimentState>,
    pub experiment_outcomes: Vec<ExperimentOutcome>,
    #[serde(default)]
    pub experiment_assignments: Vec<AssignmentHistory>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveDecisionInput {
    pub started_at: String,
    pub planned_start: String,
    pub planned_end: String,
    pub current_rhythm: CountRhythm,
    pub idle_detection_enabled: bool,
    pub history: Option<AdaptiveHistory>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentUpdate {
    pub id: String,
    pub policy_id: String,
    pub parameter_key: String,
    pub assignment_unit: String,
    pub status: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub variants: Vec<ExperimentVariant>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdaptiveDecision {
    pub policy_id: String,
    pub policy_version: i64,
    pub model_version: i64,
    pub occurred_at: String,
    pub current_rhythm: CountRhythm,
    pub selected_rhythm: CountRhythm,
    pub context: ContextBucket,
    pub features: FeatureVector,
    pub decision_mode: String,
    pub reason_codes: Vec<String>,
    pub candidate_id: Option<String>,
    pub state_scores: StateScores,
    pub experiment_update: Option<ExperimentUpdate>,
    pub experiment_assignment: Option<ExperimentAssignment>,
}

/// Compute the shared boundary policy from immutable evidence and local facts.
/// Boundary decisions never enroll a run in a new experiment.
pub fn decide_boundary(
    input: &AdaptiveDecisionInput,
    facts: &LocalTimeFacts,
) -> Result<AdaptiveDecision, String> {
    let mut quality = vec!["diary_missing".to_owned()];
    if !input.idle_detection_enabled {
        quality.push("idle_detection_disabled".to_owned());
    }
    if input.history.is_none() {
        quality.push("extension_unavailable".to_owned());
    }
    let empty = AdaptiveHistory::default();
    let history = input.history.as_ref().unwrap_or(&empty);
    let features = extract_adaptive_features(&FeatureInput {
        segments: history.segments.clone(),
        run_events: history.run_events.clone(),
        block_events: history.block_events.clone(),
        data_quality_flags: quality,
        observation_ended_at: Some(input.started_at.clone()),
    });
    let started_at_ms = parse_timestamp_ms(&input.started_at)?;
    let local = facts
        .get(&started_at_ms)
        .ok_or_else(|| "Native local-time facts are missing for adaptive decision".to_owned())?;
    let mut same_day = Vec::new();
    let mut run_ids = BTreeSet::new();
    for segment in &history.segments {
        let start = segment
            .actual_start
            .as_deref()
            .unwrap_or(&segment.planned_start);
        let segment_local = facts
            .get(&parse_timestamp_ms(start)?)
            .ok_or_else(|| "Native local-time facts are missing for adaptive history".to_owned())?;
        if segment_local.date_key != local.date_key {
            continue;
        }
        run_ids.insert(
            segment
                .run_id
                .clone()
                .unwrap_or_else(|| format!("{}:{}", segment.planned_start, segment.planned_end)),
        );
        if segment.phase == "focus" {
            same_day.push(segment.clone());
        }
    }
    let clean_focus_today = extract_adaptive_features(&FeatureInput {
        segments: same_day,
        observation_ended_at: Some(input.started_at.clone()),
        ..FeatureInput::default()
    })
    .clean_focus_seconds
        / 60.0;
    let duration = parse_timestamp_ms(&input.planned_end)?
        .saturating_sub(parse_timestamp_ms(&input.planned_start)?)
        .max(0) as f64
        / 60_000.0;
    let context = derive_context_bucket(
        local.hour,
        duration,
        run_ids.len() + 1,
        clean_focus_today,
        None,
        None,
    );
    let key = experiment_context_key(&context);
    let previous = history
        .previous_states
        .iter()
        .find(|state| state.context_key == key)
        .map(|state| state.scores);
    let state = derive_adaptive_state(&features, previous);
    let selected = select_adaptive_rhythm(input.current_rhythm, &features, state, &context);
    Ok(AdaptiveDecision {
        policy_id: POLICY_ID.to_owned(),
        policy_version: POLICY_VERSION,
        model_version: MODEL_VERSION,
        occurred_at: input.started_at.clone(),
        current_rhythm: input.current_rhythm,
        selected_rhythm: selected.selected_rhythm,
        context,
        features,
        decision_mode: selected.mode,
        reason_codes: selected.reason_codes,
        candidate_id: None,
        state_scores: selected.state_scores,
        experiment_update: None,
        experiment_assignment: None,
    })
}

/// Apply all seven experiment guardrails in `ExperimentLane::SELECTION_ORDER`, then explore.
pub fn decide_run_start(
    input: &AdaptiveDecisionInput,
    facts: &LocalTimeFacts,
) -> Result<AdaptiveDecision, String> {
    let mut decision = decide_boundary(input, facts)?;
    let empty = AdaptiveHistory::default();
    let history = input.history.as_ref().unwrap_or(&empty);
    let key = experiment_context_key(&decision.context);
    for lane in ExperimentLane::SELECTION_ORDER {
        let definition = lane.definition();
        let cooldown = experiment_cooldown_state(
            &history.experiment_states,
            &definition.id,
            &input.started_at,
        );
        let abandoned = cooldown.is_some_and(|state| state.status == "abandoned");
        let completed = cooldown.is_some_and(|state| state.status == "completed");
        if abandoned && rhythm_exposes_treatment(lane, decision.selected_rhythm) {
            apply_guardrail(&mut decision, lane, None);
            return Ok(decision);
        }
        let analysis = analyze_experiment(lane, &history.experiment_outcomes, Some(&key));
        if analysis.decision == "prefer_control"
            && rhythm_exposes_treatment(lane, decision.selected_rhythm)
        {
            let update =
                (!abandoned).then(|| experiment_update(lane, &input.started_at, "abandoned"));
            apply_guardrail(&mut decision, lane, update);
            return Ok(decision);
        }
        let bundle = matches!(
            lane,
            ExperimentLane::FocusSupport | ExperimentLane::LongRecovery
        );
        if bundle
            && analysis.decision == "prefer_control"
            && !abandoned
            && decision.experiment_update.is_none()
        {
            decision.experiment_update =
                Some(experiment_update(lane, &input.started_at, "abandoned"));
        }
        if analysis.decision != "prefer_treatment" || completed {
            continue;
        }
        if lane == ExperimentLane::FocusSupport {
            if decision.selected_rhythm.focus_duration_minutes == 45
                && decision.selected_rhythm.short_break_minutes < 7
            {
                decision.selected_rhythm.short_break_minutes = 7;
            }
            // The focus support bundle always replaces a pending experiment update.
            decision.experiment_update =
                Some(experiment_update(lane, &input.started_at, "completed"));
        } else {
            if lane == ExperimentLane::LongRecovery
                && decision.selected_rhythm.long_break_minutes == 15
                && decision.selected_rhythm.long_break_after_focus_count > 3
            {
                decision.selected_rhythm.long_break_after_focus_count = 3;
            }
            if decision.experiment_update.is_none() {
                decision.experiment_update =
                    Some(experiment_update(lane, &input.started_at, "completed"));
            }
        }
    }
    let assignment = select_run_start_assignment(
        &SelectionInput {
            occurred_at: input.started_at.clone(),
            current_rhythm: decision.current_rhythm,
            selected_rhythm: decision.selected_rhythm,
            context: decision.context.clone(),
            features: decision.features.clone(),
            state: decision.state_scores,
            experiment_outcomes: history.experiment_outcomes.clone(),
            experiment_states: history.experiment_states.clone(),
            experiment_assignments: history.experiment_assignments.clone(),
        },
        facts,
    )?;
    if let Some(assignment) = assignment {
        decision.selected_rhythm = assignment.selected_rhythm;
        decision.decision_mode = "explore".to_owned();
        push_reason(&mut decision.reason_codes, "experiment_assignment");
        decision.experiment_assignment = Some(assignment);
    }
    Ok(decision)
}

/// Collect every instant whose device-local date participates in this decision.
pub fn decision_local_instants(input: &AdaptiveDecisionInput) -> Result<BTreeSet<i64>, String> {
    let mut instants = BTreeSet::from([parse_timestamp_ms(&input.started_at)?]);
    if let Some(history) = &input.history {
        for segment in &history.segments {
            instants.insert(parse_timestamp_ms(
                segment
                    .actual_start
                    .as_deref()
                    .unwrap_or(&segment.planned_start),
            )?);
        }
    }
    Ok(instants)
}

pub fn experiment_update(
    lane: ExperimentLane,
    occurred_at: &str,
    status: &str,
) -> ExperimentUpdate {
    let definition = lane.definition();
    ExperimentUpdate {
        id: definition.id,
        policy_id: POLICY_ID.to_owned(),
        parameter_key: definition.parameter_key,
        assignment_unit: definition.assignment_unit,
        status: status.to_owned(),
        started_at: Some(occurred_at.to_owned()),
        ended_at: matches!(status, "completed" | "abandoned").then(|| occurred_at.to_owned()),
        variants: definition.variants,
    }
}

fn rhythm_exposes_treatment(lane: ExperimentLane, rhythm: CountRhythm) -> bool {
    match lane {
        ExperimentLane::FocusSupport => {
            rhythm.focus_duration_minutes > 40 && rhythm.short_break_minutes > 5
        }
        ExperimentLane::FocusDuration => rhythm.focus_duration_minutes > 40,
        ExperimentLane::ShortBreak => rhythm.short_break_minutes > 5,
        ExperimentLane::LongRecovery => {
            rhythm.long_break_minutes > 10 && rhythm.long_break_after_focus_count < 4
        }
        ExperimentLane::LongBreak => rhythm.long_break_minutes > 10,
        ExperimentLane::EarlierCadence => rhythm.long_break_after_focus_count < 4,
        ExperimentLane::LaterCadence => rhythm.long_break_after_focus_count > 4,
    }
}

fn apply_guardrail(
    decision: &mut AdaptiveDecision,
    lane: ExperimentLane,
    update: Option<ExperimentUpdate>,
) {
    match lane {
        ExperimentLane::FocusSupport | ExperimentLane::ShortBreak => {
            decision.selected_rhythm.short_break_minutes = 5
        }
        ExperimentLane::FocusDuration => decision.selected_rhythm.focus_duration_minutes = 40,
        ExperimentLane::LongBreak => decision.selected_rhythm.long_break_minutes = 10,
        ExperimentLane::EarlierCadence
        | ExperimentLane::LaterCadence
        | ExperimentLane::LongRecovery => decision.selected_rhythm.long_break_after_focus_count = 4,
    }
    decision.decision_mode = "guardrail".to_owned();
    push_reason(&mut decision.reason_codes, "experiment_guardrail");
    decision.experiment_update = update;
    decision.experiment_assignment = None;
}

fn push_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|value| value == reason) {
        reasons.push(reason.to_owned());
    }
}

fn parse_timestamp_ms(value: &str) -> Result<i64, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp_millis())
        .map_err(|error| format!("Invalid canonical adaptive timestamp: {error}"))
}
