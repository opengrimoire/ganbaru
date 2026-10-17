//! Adaptive decisions share the accepted transition's canonical evidence and writes.

use sqlx::{Sqlite, Transaction};

use super::decisions::invalid_state;
use super::models::{FocusConfiguration, FocusExecutionError};
use super::mutations::Session;
use super::persistence::{allocate_id, format_timestamp};
use crate::adaptive::decision::{
    AdaptiveDecision, AdaptiveDecisionInput, POLICY_ID, decide_boundary, decide_run_start,
    decision_local_instants,
};
use crate::adaptive::experiments::selection::experiment_context_key;
use crate::adaptive::models::CountRhythm;
use crate::adaptive::replay::dataset::from_read;
use crate::adaptive::replay::models::GateOptions;
use crate::adaptive::replay::{decide_candidate, default_candidates, evaluate, select_for_context};
use crate::adaptive::snapshots::SnapshotIds;
use crate::reads::{load_adaptive_history_tx, load_adaptive_replay_dataset_tx};

const HISTORY_SEGMENT_LIMIT: i64 = 80;
const MAX_LOCAL_INSTANTS: usize = 4096;
const REPLAY_OPPORTUNITY_LIMIT: i64 = 50;

impl Session {
    pub(super) async fn adaptive_decision(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        configuration: &FocusConfiguration,
        planned_start_ms: i64,
        planned_end_ms: i64,
        now_ms: i64,
        is_run_start: bool,
    ) -> Result<Option<AdaptiveDecision>, FocusExecutionError> {
        if configuration.preset_key.as_deref() != Some("adaptive") {
            return Ok(None);
        }
        let resolver = self
            .local_time
            .as_ref()
            .ok_or_else(|| invalid_state("Native adaptive local-time adapter is unavailable"))?;
        let started_at = format_timestamp(now_ms)?;
        let history =
            load_adaptive_history_tx(tx, &started_at, POLICY_ID, HISTORY_SEGMENT_LIMIT).await?;
        let input = AdaptiveDecisionInput {
            started_at,
            planned_start: format_timestamp(if is_run_start {
                planned_start_ms
            } else {
                now_ms
            })?,
            planned_end: format_timestamp(planned_end_ms)?,
            current_rhythm: CountRhythm::from_rhythm(&configuration.rhythm)
                .ok_or_else(|| invalid_state("Adaptive execution requires a count rhythm"))?,
            idle_detection_enabled: if self.state.idle_timeout_override_set {
                self.state.idle_timeout_override_minutes.is_some()
            } else {
                configuration.idle_timeout_minutes.is_some()
            },
            history: Some(history.into()),
        };
        let dataset = if is_run_start {
            Some(from_read(
                load_adaptive_replay_dataset_tx(
                    tx,
                    &input.started_at,
                    POLICY_ID,
                    REPLAY_OPPORTUNITY_LIMIT,
                    HISTORY_SEGMENT_LIMIT,
                )
                .await?,
            )?)
        } else {
            None
        };
        let mut instants = decision_local_instants(&input)?;
        if let Some(dataset) = &dataset {
            for opportunity in &dataset.opportunities {
                instants.extend(decision_local_instants(&opportunity.input)?);
            }
        }
        if instants.len() > MAX_LOCAL_INSTANTS {
            return Err(invalid_state(
                "Adaptive local-day evidence exceeds its instant limit",
            ));
        }
        let facts = resolver.resolve(instants.clone()).await?;
        if facts.len() != instants.len()
            || instants.iter().any(|instant| {
                facts.get(instant).is_none_or(|fact| {
                    fact.epoch_ms != *instant
                        || fact.hour > 23
                        || fact.date_key.is_empty()
                        || fact.date_string.is_empty()
                })
            })
        {
            return Err(invalid_state(
                "Native adaptive local-time response is incomplete or invalid",
            ));
        }
        let mut decision = if is_run_start {
            decide_run_start(&input, &facts)
        } else {
            decide_boundary(&input, &facts)
        }?;
        if let Some(dataset) = dataset
            .filter(|dataset| !dataset.opportunities.is_empty() && !dataset.outcomes.is_empty())
        {
            let candidates = default_candidates();
            let options = GateOptions {
                observed_candidate_outcome_scores: dataset.observed_scores,
                ..GateOptions::default()
            };
            let workflow = evaluate(
                &dataset.opportunities,
                &candidates,
                &dataset.outcomes,
                &options,
                &facts,
            )?;
            if let Some(review) =
                select_for_context(&workflow, &experiment_context_key(&decision.context))
            {
                let candidate = candidates
                    .iter()
                    .find(|candidate| candidate.id == review.candidate_id)
                    .ok_or_else(|| {
                        invalid_state("Accepted adaptive replay candidate is missing")
                    })?;
                decision = decide_candidate(&input, candidate, &facts)?;
            }
        }
        Ok(Some(decision))
    }

    pub(super) async fn adaptive_snapshot_ids(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        run: &str,
        segment: &str,
    ) -> Result<SnapshotIds, FocusExecutionError> {
        Ok(SnapshotIds {
            run: run.to_owned(),
            segment: segment.to_owned(),
            context: allocate_id(tx).await?,
            decision: allocate_id(tx).await?,
            assignment: allocate_id(tx).await?,
        })
    }
}
