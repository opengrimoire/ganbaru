use sqlx::{Sqlite, Transaction};

use super::super::{PomodoroRunClosure, PomodoroRunWrite, PomodoroSegmentWrite, writes};
use super::decisions::*;
use super::models::*;
use super::persistence::*;

/// A command-local projection. It is published only after its transaction commits.
pub(super) struct Session {
    pub state: ExecutionState,
    pub run: Option<FocusRunSnapshot>,
    pub segment: Option<FocusSegmentSnapshot>,
    pub changed_segments: Vec<String>,
    pub local_time: Option<std::sync::Arc<dyn FocusLocalTimeResolver>>,
    pub planned_blocks: Vec<super::super::PomodoroAdaptivePlannedBlockWrite>,
}

impl Session {
    pub async fn load(
        tx: &mut Transaction<'_, Sqlite>,
        state: ExecutionState,
        context: &FocusExecutionContext,
    ) -> Result<Self, FocusExecutionError> {
        let run = match &state.run_id {
            Some(id) => Some(load_run(tx, id).await?),
            None => None,
        };
        let segment = match &state.segment_id {
            Some(id) => Some(load_segment(tx, id).await?),
            None => None,
        };
        if segment
            .as_ref()
            .is_some_and(|segment| run.as_ref().is_none_or(|run| run.id != segment.run_id))
        {
            return Err(invalid_state(
                "Focus state references a phase from another run",
            ));
        }
        Ok(Self {
            state,
            run,
            segment,
            changed_segments: Vec::new(),
            local_time: context.local_time.clone(),
            planned_blocks: context.planned_blocks.clone(),
        })
    }

    pub fn live_run(&self) -> Result<&FocusRunSnapshot, FocusExecutionError> {
        self.run
            .as_ref()
            .filter(|run| run.ended_at_ms.is_none())
            .ok_or_else(|| invalid_state("There is no open Focus run"))
    }

    pub fn active_segment(&self) -> Result<&FocusSegmentSnapshot, FocusExecutionError> {
        self.live_run()?;
        self.segment
            .as_ref()
            .filter(|segment| segment.status == "active" && segment.actual_end_ms.is_none())
            .ok_or_else(|| invalid_state("There is no active Focus phase"))
    }

    pub fn mark_changed(&mut self, id: &str) {
        if !self.changed_segments.iter().any(|existing| existing == id) {
            self.changed_segments.push(id.to_owned());
        }
    }

    pub async fn record_event(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        event_type: &str,
        at_ms: i64,
        reason: Option<&str>,
        duration_ms: Option<i64>,
    ) -> Result<(), FocusExecutionError> {
        let run = self.live_run()?;
        let occurred_at = format_timestamp(at_ms)?;
        writes::insert_run_event_tx(
            tx,
            writes::RunEventInsert {
                run_id: &run.id,
                segment_id: self.segment.as_ref().map(|segment| segment.id.as_str()),
                event_type,
                occurred_at: &occurred_at,
                phase: self.segment.as_ref().map(|segment| segment.phase.as_str()),
                reason,
                duration_seconds: duration_ms.map(|milliseconds| milliseconds / 1000),
            },
        )
        .await
        .map_err(Into::into)
    }

    pub async fn pause(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        requested_start_ms: i64,
        detected_at_ms: i64,
        reason: &str,
    ) -> Result<i64, FocusExecutionError> {
        let segment = self.active_segment()?;
        if segment.pauses.len() >= MAX_SEGMENT_PAUSES as usize {
            return Err(invalid_state("Focus segment has reached its pause limit"));
        }
        if segment
            .pauses
            .iter()
            .any(|pause| pause.ended_at_ms.is_none())
        {
            return Err(invalid_state("Focus phase is already paused"));
        }
        let lower_bound = segment
            .pauses
            .last()
            .and_then(|pause| pause.ended_at_ms)
            .unwrap_or(segment.actual_start_ms)
            .max(segment.actual_start_ms);
        let start_ms = requested_start_ms
            .max(lower_bound)
            .min(detected_at_ms.max(lower_bound));
        let id = segment.id.clone();
        sqlx::query("INSERT INTO pomodoro_pauses (id, segment_id, started_at, ended_at, reason, detected_at) VALUES (lower(hex(randomblob(16))), ?, ?, NULL, ?, ?)")
            .bind(&id).bind(format_timestamp(start_ms)?).bind(reason).bind(format_timestamp(detected_at_ms.max(start_ms))?)
            .execute(&mut **tx).await.map_err(|error| format!("Start Focus pause: {error}"))?;
        self.record_event(tx, "pause_start", start_ms, Some(reason), None)
            .await?;
        self.segment
            .as_mut()
            .ok_or_else(|| invalid_state("Focus phase disappeared"))?
            .pauses
            .push(FocusPauseSnapshot {
                started_at_ms: start_ms,
                ended_at_ms: None,
                reason: reason.to_owned(),
            });
        self.mark_changed(&id);
        Ok(start_ms)
    }

    pub async fn close_pause(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        at_ms: i64,
    ) -> Result<(), FocusExecutionError> {
        let Some(segment) = &self.segment else {
            return Ok(());
        };
        let Some(pause) = segment
            .pauses
            .iter()
            .find(|pause| pause.ended_at_ms.is_none())
        else {
            return Ok(());
        };
        let end_ms = at_ms.max(pause.started_at_ms);
        let reason = pause.reason.clone();
        let start_ms = pause.started_at_ms;
        let id = segment.id.clone();
        sqlx::query(
            "UPDATE pomodoro_pauses SET ended_at = ? WHERE segment_id = ? AND ended_at IS NULL",
        )
        .bind(format_timestamp(end_ms)?)
        .bind(&id)
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("Close Focus pause: {error}"))?;
        self.record_event(
            tx,
            "pause_end",
            end_ms,
            Some(&reason),
            Some(end_ms - start_ms),
        )
        .await?;
        for pause in &mut self
            .segment
            .as_mut()
            .ok_or_else(|| invalid_state("Focus phase disappeared"))?
            .pauses
        {
            if pause.ended_at_ms.is_none() {
                pause.ended_at_ms = Some(end_ms);
            }
        }
        self.mark_changed(&id);
        Ok(())
    }

    pub async fn close_segment(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        at_ms: i64,
        status: &str,
        reason: &str,
    ) -> Result<(), FocusExecutionError> {
        let segment = self.active_segment()?;
        let end_ms = at_ms.max(segment.actual_start_ms).max(
            segment
                .pauses
                .last()
                .map(|pause| pause.started_at_ms)
                .unwrap_or(segment.actual_start_ms),
        );
        let id = segment.id.clone();
        self.close_pause(tx, end_ms).await?;
        sqlx::query("UPDATE pomodoro_segments SET actual_end = ?, status = ?, end_reason = ? WHERE id = ? AND status = 'active' AND actual_end IS NULL")
            .bind(format_timestamp(end_ms)?).bind(status).bind(reason).bind(&id).execute(&mut **tx).await
            .map_err(|error| format!("Close Focus phase: {error}"))?;
        self.record_event(
            tx,
            if reason == "focus_failed" {
                "focus_failed"
            } else {
                "phase_complete"
            },
            end_ms,
            Some(reason),
            None,
        )
        .await?;
        let segment = self
            .segment
            .as_mut()
            .ok_or_else(|| invalid_state("Focus phase disappeared"))?;
        segment.actual_end_ms = Some(end_ms);
        segment.status = status.to_owned();
        segment.end_reason = Some(reason.to_owned());
        self.mark_changed(&id);
        Ok(())
    }

    pub async fn close_run(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        at_ms: i64,
        reason: &str,
        segment_reason: &str,
        event_type: &str,
        mode: FocusMode,
    ) -> Result<(), FocusExecutionError> {
        let run = self.live_run()?;
        let id = run.id.clone();
        let ended_at_ms = at_ms.max(run.started_at_ms);
        if self
            .segment
            .as_ref()
            .is_some_and(|segment| segment.status == "active")
        {
            let segment = self.active_segment()?;
            let phase_end_ms = if self.state.mode == FocusMode::Running
                && !segment
                    .pauses
                    .iter()
                    .any(|pause| pause.ended_at_ms.is_none())
            {
                ended_at_ms.min(segment.planned_end_ms)
            } else {
                ended_at_ms
            };
            self.close_segment(tx, phase_end_ms, "interrupted", segment_reason)
                .await?;
        }
        writes::close_run_tx(
            tx,
            &PomodoroRunClosure {
                run_id: id,
                ended_at: format_timestamp(ended_at_ms)?,
                end_reason: reason.to_owned(),
                segment_status: "interrupted".to_owned(),
                segment_end_reason: segment_reason.to_owned(),
                event_type: event_type.to_owned(),
            },
        )
        .await?;
        self.run
            .as_mut()
            .ok_or_else(|| invalid_state("Focus run disappeared"))?
            .ended_at_ms = Some(ended_at_ms);
        self.state.mode = mode;
        self.state.last_transition_at_ms = ended_at_ms;
        self.clear_waits();
        Ok(())
    }

    pub fn clear_waits(&mut self) {
        self.state.idle_started_at_ms = None;
        self.state.idle_detected_at_ms = None;
        self.state.idle_overlay_visible_at_ms = None;
        self.state.focus_failed_at_ms = None;
        self.state.suspend_started_at_ms = None;
        self.state.suspend_returned_at_ms = None;
        self.state.return_started_at_ms = None;
        self.state.paused_prompts_dismissed = false;
    }

    pub async fn update_deadline(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
    ) -> Result<(), FocusExecutionError> {
        let run = self.live_run()?;
        let segment = self.active_segment()?;
        let elapsed_ms = segment_elapsed_ms(segment, now_ms)?;
        let end_ms = now_ms
            .saturating_add(segment.chosen_duration_ms.saturating_sub(elapsed_ms).max(0))
            .min(run.planned_end_ms)
            .max(segment.planned_start_ms);
        let id = segment.id.clone();
        sqlx::query(
            "UPDATE pomodoro_segments SET planned_end = ?, chosen_duration_ms = ? WHERE id = ?",
        )
        .bind(format_timestamp(end_ms)?)
        .bind(segment.chosen_duration_ms)
        .bind(&id)
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("Update accepted Focus phase deadline: {error}"))?;
        self.segment
            .as_mut()
            .ok_or_else(|| invalid_state("Focus phase disappeared"))?
            .planned_end_ms = end_ms;
        self.mark_changed(&id);
        Ok(())
    }

    pub async fn start_segment(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
        phase: FocusPhase,
        position: i64,
    ) -> Result<(), FocusExecutionError> {
        let mut run = self.live_run()?.clone();
        if now_ms >= run.planned_end_ms {
            return Err(invalid_state("The Focus commitment has ended"));
        }
        let decision = self
            .adaptive_decision(
                tx,
                &run.configuration,
                run.planned_start_ms,
                run.planned_end_ms,
                now_ms,
                false,
            )
            .await?;
        if let Some(decision) = &decision {
            run.configuration.rhythm = decision.selected_rhythm.into_rhythm();
        }
        let position = normalize_position(&run.configuration, position);
        let phase = if decision.is_some() && phase != FocusPhase::Focus {
            break_phase(&run.configuration, position)
        } else {
            phase
        };
        let duration_ms = phase_duration_ms(&run.configuration, phase, position);
        let segment = build_segment_write(tx, &run, phase, position, now_ms, duration_ms).await?;
        writes::insert_segment_tx(tx, &segment).await?;
        if let Some(decision) = &decision {
            let ids = self.adaptive_snapshot_ids(tx, &run.id, &segment.id).await?;
            let envelope = super::super::adaptive::snapshots::decision_envelope(
                decision,
                ids,
                if phase == FocusPhase::Focus {
                    "focus_start"
                } else {
                    "break_start"
                },
            );
            super::super::validation::validate_adaptive_decision_envelope_for_segment(
                &envelope, &segment,
            )?;
            writes::insert_adaptive_decision_envelope_tx(tx, &envelope).await?;
        }
        sqlx::query("UPDATE pomodoro_segments SET chosen_duration_ms = ? WHERE id = ?")
            .bind(duration_ms)
            .bind(&segment.id)
            .execute(&mut **tx)
            .await
            .map_err(|error| format!("Persist accepted Focus phase duration: {error}"))?;
        self.state.segment_id = Some(segment.id.clone());
        self.segment = Some(load_segment(tx, &segment.id).await?);
        self.run = Some(run);
        self.mark_changed(&segment.id);
        self.state.mode = FocusMode::Running;
        self.state.focus_extension_used = false;
        self.state.break_extension_ms = 0;
        self.clear_waits();
        self.record_event(tx, "phase_start", now_ms, None, None)
            .await?;
        Ok(())
    }

    pub async fn start_run(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        commitment: &FocusCommitment,
        now_ms: i64,
        trigger: &str,
        continuation: Option<Continuation>,
    ) -> Result<(), FocusExecutionError> {
        validate_commitment(commitment, now_ms)?;
        if self
            .run
            .as_ref()
            .is_some_and(|run| run.ended_at_ms.is_none())
        {
            return Err(invalid_state("A Focus run is already open"));
        }
        let decision = self
            .adaptive_decision(
                tx,
                &commitment.configuration,
                commitment.start_ms,
                commitment.end_ms,
                now_ms,
                true,
            )
            .await?;
        let mut configuration = commitment.configuration.clone();
        if let Some(decision) = &decision {
            configuration.rhythm = decision.selected_rhythm.into_rhythm();
        }
        let mut continuation = continuation.unwrap_or_default();
        let position = normalize_position(&configuration, continuation.position);
        if continuation.phase == FocusPhase::Focus
            && continuation.phase_elapsed_ms
                >= phase_duration_ms(&configuration, FocusPhase::Focus, position)
        {
            continuation.phase = break_phase(&configuration, position);
            continuation.phase_elapsed_ms = 0;
        }
        let phase = continuation.phase;
        let duration_ms = phase_duration_ms(&configuration, phase, position)
            .saturating_sub(continuation.phase_elapsed_ms);
        if duration_ms <= 0 {
            return Err(invalid_state(
                "The inherited break has already ended and requires explicit return",
            ));
        }
        let run_id = allocate_id(tx).await?;
        let run = FocusRunSnapshot {
            id: run_id.clone(),
            event_id: Some(commitment.event_id.clone()),
            occurrence_id: commitment.occurrence_id.clone(),
            event_date: commitment.event_date.clone(),
            title: commitment.title.clone(),
            started_at_ms: now_ms,
            planned_start_ms: commitment.start_ms,
            planned_end_ms: commitment.end_ms,
            ended_at_ms: None,
            inherited_focus_ms: continuation.focus_elapsed_ms,
            inherited_phase_ms: continuation.phase_elapsed_ms,
            configuration,
        };
        let segment = build_segment_write(tx, &run, phase, position, now_ms, duration_ms).await?;
        let adaptive_snapshot = match &decision {
            Some(decision) => {
                let ids = self.adaptive_snapshot_ids(tx, &run.id, &segment.id).await?;
                Some(super::super::adaptive::snapshots::run_start_snapshot(
                    decision,
                    ids,
                    self.planned_blocks.clone(),
                ))
            }
            None => None,
        };
        let write = PomodoroRunWrite {
            id: run_id.clone(),
            event_id: commitment.occurrence_id.clone(),
            event_date: commitment.event_date.clone(),
            planned_start: format_timestamp(commitment.start_ms)?,
            planned_end: format_timestamp(commitment.end_ms)?,
            started_at: format_timestamp(now_ms)?,
            rhythm: run.configuration.rhythm.clone(),
            rhythm_source: commitment.configuration.rhythm_source.clone(),
            preset_key: commitment.configuration.preset_key.clone(),
            idle_timeout_minutes: commitment.configuration.idle_timeout_minutes,
            event_title_snapshot: commitment.title.clone(),
            inherited_focus_minutes: continuation.focus_elapsed_ms / 60_000,
            inherited_rhythm_position: position,
            inherited_from_run_id: continuation.run_id,
            start_trigger: trigger.to_owned(),
            adaptive_snapshot,
        };
        writes::insert_run_tx(tx, &write, &segment).await?;
        sqlx::query("UPDATE pomodoro_runs SET inherited_focus_milliseconds = ?, inherited_phase_milliseconds = ? WHERE id = ?")
            .bind(continuation.focus_elapsed_ms).bind(continuation.phase_elapsed_ms).bind(&run_id)
            .execute(&mut **tx).await.map_err(|error| format!("Persist precise Focus inheritance: {error}"))?;
        sqlx::query("UPDATE pomodoro_segments SET chosen_duration_ms = ? WHERE id = ?")
            .bind(duration_ms)
            .bind(&segment.id)
            .execute(&mut **tx)
            .await
            .map_err(|error| format!("Persist accepted Focus duration: {error}"))?;
        self.state.run_id = Some(run_id);
        self.state.calendar_configuration = Some(commitment.configuration.clone());
        self.state.segment_id = Some(segment.id.clone());
        self.state.mode = FocusMode::Running;
        self.state.dismissed_occurrence_id = None;
        self.state.focus_extension_used = false;
        self.state.break_extension_ms = 0;
        self.state.last_transition_at_ms = now_ms;
        self.clear_waits();
        self.run = Some(run);
        self.segment = Some(load_segment(tx, &segment.id).await?);
        self.mark_changed(&segment.id);
        Ok(())
    }
}

pub(super) struct Continuation {
    pub run_id: Option<String>,
    pub phase: FocusPhase,
    pub position: i64,
    pub focus_elapsed_ms: i64,
    pub phase_elapsed_ms: i64,
}

impl Default for Continuation {
    fn default() -> Self {
        Self {
            run_id: None,
            phase: FocusPhase::Focus,
            position: 1,
            focus_elapsed_ms: 0,
            phase_elapsed_ms: 0,
        }
    }
}

async fn build_segment_write(
    tx: &mut Transaction<'_, Sqlite>,
    run: &FocusRunSnapshot,
    phase: FocusPhase,
    position: i64,
    now_ms: i64,
    duration_ms: i64,
) -> Result<PomodoroSegmentWrite, FocusExecutionError> {
    Ok(PomodoroSegmentWrite {
        id: allocate_id(tx).await?,
        event_id: run.occurrence_id.clone(),
        event_date: run.event_date.clone(),
        run_id: run.id.clone(),
        rhythm_position: position,
        phase: phase.as_str().to_owned(),
        planned_start: format_timestamp(now_ms)?,
        planned_end: format_timestamp(now_ms.saturating_add(duration_ms).min(run.planned_end_ms))?,
        actual_start: Some(format_timestamp(now_ms)?),
        actual_end: None,
        pauses: Vec::new(),
        status: "active".to_owned(),
        end_reason: None,
    })
}
