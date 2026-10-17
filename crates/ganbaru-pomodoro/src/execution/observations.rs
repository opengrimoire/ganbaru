use sqlx::{Sqlite, Transaction};

use super::decisions::*;
use super::models::*;
use super::mutations::{Continuation, Session};
use super::persistence::*;

const ACTIVITY_OBSERVATION_MAX_AGE_MS: i64 = 15_000;

impl Session {
    pub(super) async fn observe(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        observation: &FocusObservation,
        context: &FocusExecutionContext,
    ) -> Result<bool, FocusExecutionError> {
        let now_ms = context.now_ms;
        if matches!(observation, FocusObservation::Recover) {
            return self.recover(tx, context).await;
        }
        // Suspend evidence is recorded before expiry, excluding the away interval.
        if let FocusObservation::Suspend {
            started_at_ms,
            returned_at_ms,
        } = observation
        {
            return self
                .suspend(tx, *started_at_ms, *returned_at_ms, now_ms)
                .await;
        }
        let expired = self.expire(tx, now_ms).await?;
        let reconciled = if expired {
            false
        } else {
            self.reconcile_calendar(tx, context).await?
        };
        match observation {
            FocusObservation::AutomaticAdmission(activity) => {
                if self
                    .run
                    .as_ref()
                    .is_some_and(|run| run.ended_at_ms.is_none())
                    || context.platform != FocusPlatform::Desktop
                    || self.state.automatic_admission_suppressed
                {
                    return Ok(expired || reconciled);
                }
                let Some(commitment) = &context.commitment else {
                    return Ok(expired || reconciled);
                };
                if self.state.dismissed_occurrence_id.as_deref() == Some(&commitment.occurrence_id)
                    || !is_fresh_observation(activity, now_ms)
                {
                    return Ok(expired || reconciled);
                }
                let boundary = self.state.last_transition_at_ms.max(commitment.start_ms);
                if !crate::admission::automatic_start_allowed(
                    boundary,
                    activity.observed_at_ms,
                    activity.idle_ms.and_then(|value| u64::try_from(value).ok()),
                ) {
                    return Ok(expired || reconciled);
                }
                let continuation = self.adjacent_continuation(commitment, now_ms)?;
                self.start_run(
                    tx,
                    commitment,
                    now_ms,
                    if continuation.is_some() {
                        "block_transition"
                    } else {
                        "block_auto"
                    },
                    continuation,
                )
                .await?;
                Ok(true)
            }
            FocusObservation::CalendarChanged => {
                if expired {
                    return Ok(true);
                }
                Ok(reconciled)
            }
            FocusObservation::Activity(activity) => {
                if expired {
                    return Ok(true);
                }
                Ok(self.observe_activity(tx, activity, context).await? || reconciled)
            }
            FocusObservation::IdleOverlayVisible {
                run_id,
                segment_id,
                detected_at_ms,
            } => {
                if expired
                    || self.state.mode != FocusMode::IdlePause
                    || self.state.run_id.as_ref() != Some(run_id)
                    || self.state.segment_id.as_ref() != Some(segment_id)
                    || self.state.idle_detected_at_ms != Some(*detected_at_ms)
                    || *detected_at_ms > now_ms
                    || self.state.idle_overlay_visible_at_ms.is_some()
                {
                    return Ok(expired || reconciled);
                }
                self.state.idle_overlay_visible_at_ms = Some(now_ms);
                Ok(true)
            }
            FocusObservation::IdleGraceElapsed {
                run_id,
                segment_id,
                visible_at_ms,
                elapsed_ms,
            } => {
                if expired
                    || self.state.mode != FocusMode::IdlePause
                    || self.state.run_id.as_ref() != Some(run_id)
                    || self.state.segment_id.as_ref() != Some(segment_id)
                    || self.state.idle_overlay_visible_at_ms != Some(*visible_at_ms)
                    || *elapsed_ms < FOCUS_IDLE_FAILURE_GRACE_MS
                {
                    return Ok(expired || reconciled);
                }
                let failed_at_ms = self
                    .state
                    .idle_started_at_ms
                    .ok_or_else(|| invalid_state("Idle pause has no accepted start"))?;
                self.close_segment(tx, failed_at_ms, "interrupted", "focus_failed")
                    .await?;
                self.state.focus_failed_at_ms = Some(failed_at_ms);
                self.state.mode = FocusMode::IdleFailed;
                Ok(true)
            }
            FocusObservation::Deadline | FocusObservation::ForegroundChanged { .. } => {
                if expired {
                    return Ok(true);
                }
                if self.state.mode == FocusMode::Running {
                    let segment = self.active_segment()?;
                    if now_ms >= segment.planned_end_ms {
                        // Foreground recovery reconciles the accepted phase only.
                        // Opening the application is not acceptance of its successor.
                        if context.platform == FocusPlatform::Android
                            && matches!(observation, FocusObservation::ForegroundChanged { .. })
                        {
                            let ended_at_ms = segment.planned_end_ms;
                            self.close_segment(tx, ended_at_ms, "completed", "completed")
                                .await?;
                            self.state.mode = FocusMode::ReturnWait;
                            self.state.return_started_at_ms = Some(ended_at_ms);
                        } else {
                            self.advance(tx, now_ms, false, context).await?;
                        }
                        return Ok(true);
                    }
                }
                Ok(reconciled)
            }
            FocusObservation::Heartbeat => {
                if !expired {
                    if let Some(run) = self.run.as_ref().filter(|run| run.ended_at_ms.is_none()) {
                        sqlx::query("UPDATE pomodoro_runs SET last_heartbeat = ? WHERE id = ? AND ended_at IS NULL")
                            .bind(format_timestamp(now_ms)?).bind(&run.id).execute(&mut **tx).await
                            .map_err(|error| format!("Persist native Focus heartbeat: {error}"))?;
                    }
                }
                Ok(expired || reconciled)
            }
            FocusObservation::Recover | FocusObservation::Suspend { .. } => {
                unreachable!("handled before expiry")
            }
        }
    }

    async fn observe_activity(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        activity: &FocusActivityObservation,
        context: &FocusExecutionContext,
    ) -> Result<bool, FocusExecutionError> {
        let unavailable = activity.idle_ms.is_none_or(|idle| idle < 0)
            || !is_fresh_observation(activity, context.now_ms);
        let changed = self.state.activity_source_unavailable != unavailable;
        self.state.activity_source_unavailable = unavailable;
        if unavailable
            || self.state.mode != FocusMode::Running
            || activity.webcam_in_use
            || context.platform != FocusPlatform::Desktop
        {
            return Ok(changed);
        }
        let Some(run) = self.run.as_ref().filter(|run| run.ended_at_ms.is_none()) else {
            return Ok(changed);
        };
        let timeout_minutes = if self.state.idle_timeout_override_set {
            self.state.idle_timeout_override_minutes
        } else {
            run.configuration.idle_timeout_minutes
        };
        let Some(timeout_minutes) = timeout_minutes else {
            return Ok(changed);
        };
        if self.active_segment()?.phase != FocusPhase::Focus {
            return Ok(changed);
        }
        let Some(idle_ms) = activity.idle_ms else {
            return Ok(changed);
        };
        if idle_ms < timeout_minutes.saturating_mul(60_000) {
            return Ok(changed);
        }
        let pause_started_at_ms = self
            .pause(
                tx,
                activity.observed_at_ms.saturating_sub(idle_ms),
                context.now_ms,
                "idle",
            )
            .await?;
        self.record_event(
            tx,
            "idle_detected",
            context.now_ms,
            Some("idle"),
            Some(context.now_ms.saturating_sub(pause_started_at_ms)),
        )
        .await?;
        self.state.mode = FocusMode::IdlePause;
        self.state.idle_started_at_ms = Some(pause_started_at_ms);
        self.state.idle_detected_at_ms = Some(context.now_ms);
        self.state.idle_overlay_visible_at_ms = None;
        self.state.paused_prompts_dismissed = false;
        Ok(true)
    }

    async fn suspend(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        started_at_ms: i64,
        returned_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, FocusExecutionError> {
        if started_at_ms < 0 || returned_at_ms < started_at_ms || returned_at_ms > now_ms {
            return Err(invalid_state("Native Focus suspend observation is invalid"));
        }
        if self
            .run
            .as_ref()
            .is_none_or(|run| run.ended_at_ms.is_some())
        {
            return Ok(false);
        }
        let end_ms = self.live_run()?.planned_end_ms;
        if self.state.mode != FocusMode::Running {
            return self.expire(tx, now_ms).await;
        }
        let pause_started_at_ms = self
            .pause(
                tx,
                started_at_ms.min(end_ms),
                returned_at_ms.min(end_ms),
                "suspend",
            )
            .await?;
        self.record_event(
            tx,
            "suspend_detected",
            pause_started_at_ms,
            Some("suspend"),
            Some(returned_at_ms.saturating_sub(pause_started_at_ms)),
        )
        .await?;
        self.state.mode = FocusMode::Suspended;
        self.state.suspend_started_at_ms = Some(pause_started_at_ms);
        self.state.suspend_returned_at_ms = Some(returned_at_ms);
        self.state.paused_prompts_dismissed = false;
        self.expire(tx, now_ms).await?;
        Ok(true)
    }

    pub(super) async fn reconcile_calendar(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        context: &FocusExecutionContext,
    ) -> Result<bool, FocusExecutionError> {
        let Some(run) = self.run.as_ref().filter(|run| run.ended_at_ms.is_none()) else {
            return Ok(false);
        };
        let Some(commitment) = context
            .commitment
            .as_ref()
            .filter(|commitment| commitment.occurrence_id == run.occurrence_id)
        else {
            self.close_run(
                tx,
                context.now_ms,
                "interrupted",
                "stopped",
                "stop",
                FocusMode::Stopped,
            )
            .await?;
            return Ok(true);
        };
        if commitment.end_ms <= context.now_ms {
            self.close_run(
                tx,
                commitment.end_ms,
                "completed",
                "event_expired",
                "complete",
                FocusMode::Expired,
            )
            .await?;
            return Ok(true);
        }
        validate_commitment(commitment, context.now_ms)?;
        let deadline_changed = run.planned_end_ms != commitment.end_ms;
        let config_changed = self
            .state
            .calendar_configuration
            .as_ref()
            .unwrap_or(&run.configuration)
            != &commitment.configuration;
        if deadline_changed {
            sqlx::query(
                "UPDATE pomodoro_runs SET planned_end = ? WHERE id = ? AND ended_at IS NULL",
            )
            .bind(format_timestamp(commitment.end_ms)?)
            .bind(&run.id)
            .execute(&mut **tx)
            .await
            .map_err(|error| format!("Refresh canonical Focus event deadline: {error}"))?;
            self.run
                .as_mut()
                .ok_or_else(|| invalid_state("Focus run disappeared"))?
                .planned_end_ms = commitment.end_ms;
        }
        if !config_changed
            || matches!(
                self.state.mode,
                FocusMode::ReturnWait | FocusMode::IdleFailed
            )
        {
            if deadline_changed
                && self
                    .segment
                    .as_ref()
                    .is_some_and(|segment| segment.status == "active")
            {
                self.update_deadline(tx, context.now_ms).await?;
            }
            return Ok(deadline_changed);
        }
        let segment = self.active_segment()?;
        let run = self.live_run()?;
        let elapsed_ms = segment_elapsed_ms(segment, context.now_ms)?
            .saturating_add(inherited_phase_ms(run, segment));
        let position = normalize_position(&commitment.configuration, segment.rhythm_position);
        let prior_mode = self.state.mode;
        let prior_idle_detected = self.state.idle_detected_at_ms;
        let prior_idle_visible = self.state.idle_overlay_visible_at_ms;
        let prior_suspend_returned = self.state.suspend_returned_at_ms;
        let prior_prompts_dismissed = self.state.paused_prompts_dismissed;
        let mut continuation = Continuation {
            run_id: Some(run.id.clone()),
            phase: segment.phase,
            position,
            focus_elapsed_ms: if segment.phase == FocusPhase::Focus {
                elapsed_ms
            } else {
                0
            },
            phase_elapsed_ms: elapsed_ms,
        };
        let due =
            elapsed_ms >= phase_duration_ms(&commitment.configuration, segment.phase, position);
        if due && segment.phase == FocusPhase::Focus {
            continuation.phase = break_phase(&commitment.configuration, position);
            continuation.phase_elapsed_ms = 0;
        } else if due {
            // A shorter break ends the already accepted phase. The next focus is
            // still gated by explicit return; no zero-duration phase is fabricated.
            self.close_segment(tx, context.now_ms, "completed", "completed")
                .await?;
            self.state.mode = FocusMode::ReturnWait;
            self.state.return_started_at_ms = Some(context.now_ms);
            return Ok(true);
        }
        self.close_run(
            tx,
            context.now_ms,
            "reconfigured",
            "reconfigured",
            "reconfigure",
            FocusMode::Stopped,
        )
        .await?;
        self.start_run(
            tx,
            commitment,
            context.now_ms,
            "reconfigure",
            Some(continuation),
        )
        .await?;
        if prior_mode != FocusMode::Running && !due {
            let reason = match prior_mode {
                FocusMode::IdlePause => "idle",
                FocusMode::Suspended => "suspend",
                _ => "manual",
            };
            self.pause(tx, context.now_ms, context.now_ms, reason)
                .await?;
            self.state.mode = prior_mode;
            self.state.paused_prompts_dismissed = prior_prompts_dismissed;
            if prior_mode == FocusMode::IdlePause {
                self.state.idle_started_at_ms = Some(context.now_ms);
                self.state.idle_detected_at_ms = prior_idle_detected.or(Some(context.now_ms));
                self.state.idle_overlay_visible_at_ms = prior_idle_visible;
            } else if prior_mode == FocusMode::Suspended {
                self.state.suspend_started_at_ms = Some(context.now_ms);
                self.state.suspend_returned_at_ms = prior_suspend_returned
                    .map(|returned| returned.max(context.now_ms))
                    .or(Some(context.now_ms));
            }
        }
        Ok(true)
    }

    fn adjacent_continuation(
        &self,
        commitment: &FocusCommitment,
        now_ms: i64,
    ) -> Result<Option<Continuation>, FocusExecutionError> {
        let Some(run) = &self.run else {
            return Ok(None);
        };
        let Some(segment) = &self.segment else {
            return Ok(None);
        };
        if run.occurrence_id == commitment.occurrence_id
            || run.planned_end_ms < commitment.start_ms
            || self.state.mode != FocusMode::Expired
            || segment.phase != FocusPhase::Focus
        {
            return Ok(None);
        }
        let elapsed_ms =
            segment_elapsed_ms(segment, now_ms)?.saturating_add(inherited_phase_ms(run, segment));
        let position = normalize_position(&commitment.configuration, segment.rhythm_position);
        let due =
            elapsed_ms >= phase_duration_ms(&commitment.configuration, FocusPhase::Focus, position);
        Ok(Some(Continuation {
            run_id: Some(run.id.clone()),
            phase: if due {
                break_phase(&commitment.configuration, position)
            } else {
                FocusPhase::Focus
            },
            position,
            focus_elapsed_ms: elapsed_ms,
            phase_elapsed_ms: if due { 0 } else { elapsed_ms },
        }))
    }

    async fn recover(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        context: &FocusExecutionContext,
    ) -> Result<bool, FocusExecutionError> {
        // Visibility belongs to the live controller, never a previous process or device.
        self.state.idle_overlay_visible_at_ms = None;
        let Some(run) = self.run.as_ref().filter(|run| run.ended_at_ms.is_none()) else {
            return Ok(false);
        };
        if context.platform == FocusPlatform::Desktop {
            let heartbeat = sqlx::query_scalar::<_, String>(
                "SELECT last_heartbeat FROM pomodoro_runs WHERE id = ?",
            )
            .bind(&run.id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| format!("Read Focus recovery heartbeat: {error}"))?;
            let cutoff = parse_timestamp_ms(&heartbeat)?
                .max(run.started_at_ms)
                .min(context.now_ms)
                .min(run.planned_end_ms);
            self.close_run(
                tx,
                cutoff,
                "interrupted",
                "crash_recovery",
                "crash_recovery",
                FocusMode::Stopped,
            )
            .await?;
            return Ok(true);
        }
        if self.expire(tx, context.now_ms).await? {
            return Ok(true);
        }
        if self.segment.is_none() {
            self.close_run(
                tx,
                context.now_ms,
                "interrupted",
                "crash_recovery",
                "crash_recovery",
                FocusMode::Stopped,
            )
            .await?;
            return Ok(true);
        }
        if self.state.mode == FocusMode::Running {
            let segment = self.active_segment()?;
            if context.now_ms >= segment.planned_end_ms {
                let ended_at_ms = segment.planned_end_ms;
                self.close_segment(tx, ended_at_ms, "completed", "completed")
                    .await?;
                self.state.mode = FocusMode::ReturnWait;
                self.state.return_started_at_ms = Some(ended_at_ms);
            }
        }
        Ok(true)
    }
}

fn is_fresh_observation(activity: &FocusActivityObservation, now_ms: i64) -> bool {
    activity.observed_at_ms <= now_ms
        && now_ms.saturating_sub(activity.observed_at_ms) <= ACTIVITY_OBSERVATION_MAX_AGE_MS
        && activity
            .idle_ms
            .is_some_and(|idle| idle >= 0 && idle <= activity.observed_at_ms)
}
