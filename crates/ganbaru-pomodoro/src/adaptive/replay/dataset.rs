//! Convert canonical replay reads into typed opportunities and run outcomes.

use std::collections::BTreeMap;

use crate::adaptive::decision::AdaptiveDecisionInput;
use crate::adaptive::models::{CountRhythm, FeatureVector};
use crate::adaptive::replay::models::{CandidateObservedScore, ObservedOutcome, ReplayOpportunity};
use crate::adaptive::replay::scoring::score_by_candidate;
use crate::{PomodoroAdaptiveReplayDatasetRead, PomodoroAdaptiveReplayOutcomeRowRead};

pub(crate) struct ReplayDataset {
    pub opportunities: Vec<ReplayOpportunity>,
    pub outcomes: Vec<ObservedOutcome>,
    pub observed_scores: Vec<CandidateObservedScore>,
}

/// Preserve historical evidence and associate each outcome with its actual rhythm.
pub(crate) fn from_read(
    dataset: PomodoroAdaptiveReplayDatasetRead,
) -> Result<ReplayDataset, String> {
    let mut histories = dataset
        .histories
        .into_iter()
        .map(|entry| (entry.opportunity_id, entry.history))
        .collect::<BTreeMap<_, _>>();
    let mut selected_rhythms = BTreeMap::new();
    let mut candidate_ids = BTreeMap::new();
    let opportunities = dataset
        .opportunities
        .into_iter()
        .map(|entry| {
            let selected = CountRhythm::from_rhythm(&entry.selected_rhythm).ok_or_else(|| {
                format!(
                    "Replay opportunity {} has a non-count selected rhythm",
                    entry.id
                )
            })?;
            let current = CountRhythm::from_rhythm(&entry.current_rhythm).ok_or_else(|| {
                format!(
                    "Replay opportunity {} has a non-count current rhythm",
                    entry.id
                )
            })?;
            selected_rhythms.insert(entry.id.clone(), selected);
            candidate_ids.insert(entry.id.clone(), entry.candidate_id);
            Ok(ReplayOpportunity {
                label: Some(entry.run_id),
                input: AdaptiveDecisionInput {
                    started_at: entry.started_at,
                    planned_start: entry.planned_start,
                    planned_end: entry.planned_end,
                    current_rhythm: current,
                    idle_detection_enabled: true,
                    history: histories.remove(&entry.id).map(Into::into),
                },
                id: entry.id,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut groups: Vec<(
        String,
        BTreeMap<String, PomodoroAdaptiveReplayOutcomeRowRead>,
    )> = Vec::new();
    for row in dataset.outcomes {
        if row.outcome_window != "run" {
            continue;
        }
        let index = match groups.iter().position(|(id, _)| *id == row.opportunity_id) {
            Some(index) => index,
            None => {
                groups.push((row.opportunity_id.clone(), BTreeMap::new()));
                groups.len() - 1
            }
        };
        if groups[index].1.contains_key(&row.outcome_key) {
            return Err(format!(
                "Duplicate adaptive replay run outcome {} for opportunity {}",
                row.outcome_key, row.opportunity_id
            ));
        }
        groups[index].1.insert(row.outcome_key.clone(), row);
    }
    let outcomes = groups
        .into_iter()
        .map(|(id, values)| {
            let numeric = |key: &str| {
                values
                    .get(key)
                    .and_then(|row| row.numeric_value)
                    .filter(|value| value.is_finite())
                    .unwrap_or(0.0)
            };
            ObservedOutcome {
                observed_rhythm: selected_rhythms.get(&id).copied(),
                opportunity_id: id,
                features: FeatureVector {
                    completed_focus_segments: numeric("completed_focus_segments"),
                    interrupted_focus_segments: numeric("interrupted_focus_segments"),
                    focus_failure_count: numeric("focus_failure_count"),
                    clean_focus_seconds: numeric("clean_focus_seconds"),
                    planned_focus_seconds: numeric("planned_focus_seconds"),
                    idle_pause_count: numeric("idle_pause_count"),
                    idle_pause_seconds: numeric("idle_pause_seconds"),
                    manual_pause_count: numeric("manual_pause_count"),
                    manual_pause_seconds: numeric("manual_pause_seconds"),
                    suspend_pause_count: numeric("suspend_pause_count"),
                    suspend_pause_seconds: numeric("suspend_pause_seconds"),
                    break_started_count: numeric("break_started_count"),
                    break_completed_count: numeric("break_completed_count"),
                    break_skipped_count: numeric("break_skipped_count"),
                    short_break_overtime_seconds: numeric("short_break_overtime_seconds"),
                    long_break_overtime_seconds: numeric("long_break_overtime_seconds"),
                    blocked_attempt_count: numeric("blocked_attempt_count"),
                    stop_count: if values
                        .get("run_stopped")
                        .is_some_and(|row| row.boolean_value == Some(true))
                    {
                        1.0
                    } else {
                        0.0
                    },
                    ..FeatureVector::default()
                },
            }
        })
        .collect::<Vec<_>>();
    let observed_scores = score_by_candidate(&outcomes, &candidate_ids);
    Ok(ReplayDataset {
        opportunities,
        outcomes,
        observed_scores,
    })
}
