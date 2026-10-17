use sqlx::{Sqlite, Transaction};

use super::decisions::*;
use super::models::*;
use super::mutations::Session;
use super::persistence::*;

/// Recover an accepted result before resolving potentially changed Calendar inputs.
/// The caller must authorize the vault and hold the same transaction used for
/// any subsequent execution. Identity reuse with different intent is an error.
pub async fn focus_read_command_receipt_tx(
    tx: &mut Transaction<'_, Sqlite>,
    command: &FocusCommand,
) -> Result<Option<FocusExecutionSnapshot>, FocusExecutionError> {
    validate_command(command)?;
    load_receipt(tx, command).await
}

/// Apply a user intent in an authorized caller-owned write transaction.
///
/// Calendar resolution must use this same transaction. The caller must roll back
/// on any error and publish the returned snapshot only after a successful commit.
/// Receipts are checked before revisions, making an uncertain response retry safe.
pub async fn focus_execute_command_tx(
    tx: &mut Transaction<'_, Sqlite>,
    command: &FocusCommand,
    context: &FocusExecutionContext,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    if let Some(receipt) = focus_read_command_receipt_tx(tx, command).await? {
        return Ok(receipt);
    }
    format_timestamp(context.now_ms)?;
    if context.now_ms < 0 {
        return Err(invalid_state("Focus clock precedes the supported epoch"));
    }
    let (revision, state) = load_state(tx).await?;
    if command.expected_revision != revision {
        return Err(FocusExecutionError {
            code: FocusErrorCode::StaleRevision,
            message: "Focus execution changed before this action was accepted".to_owned(),
            current_revision: Some(revision),
        });
    }
    let next_revision = reserve_revision(tx, revision).await?;
    let mut session = Session::load(tx, state, context).await?;
    let expired = session.expire(tx, context.now_ms).await?;
    let was_open = session
        .run
        .as_ref()
        .is_some_and(|run| run.ended_at_ms.is_none());
    if !expired {
        session.reconcile_calendar(tx, context).await?;
    }
    let calendar_closed = was_open
        && session
            .run
            .as_ref()
            .is_none_or(|run| run.ended_at_ms.is_some());
    // Expiry is itself the accepted result. A late resume cannot resurrect a run.
    if (!expired && !calendar_closed)
        || matches!(command.intent, FocusIntent::StartScheduled { .. })
    {
        apply_intent(tx, &mut session, &command.intent, context).await?;
    }
    session.state.last_transition_at_ms = session.state.last_transition_at_ms.max(context.now_ms);
    save_state(tx, &session.state, next_revision, context.now_ms).await?;
    let result = load_snapshot(
        tx,
        next_revision,
        &session.state,
        context.now_ms,
        &session.changed_segments,
    )
    .await?;
    save_receipt(tx, command, &result).await?;
    Ok(result)
}

/// Apply a native observation without granting the WebView observation authority.
/// No-op observations preserve the semantic revision. Heartbeats remain history
/// evidence only, and do not turn visual countdowns into writes or events.
pub async fn focus_apply_observation_tx(
    tx: &mut Transaction<'_, Sqlite>,
    observation: &FocusObservation,
    context: &FocusExecutionContext,
) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
    format_timestamp(context.now_ms)?;
    if context.now_ms < 0 {
        return Err(invalid_state("Focus clock precedes the supported epoch"));
    }
    let (revision, state) = load_state(tx).await?;
    let mut session = Session::load(tx, state, context).await?;
    let changed = session.observe(tx, observation, context).await?;
    let revision = if changed {
        let next = reserve_revision(tx, revision).await?;
        session.state.last_transition_at_ms =
            session.state.last_transition_at_ms.max(context.now_ms);
        save_state(tx, &session.state, next, context.now_ms).await?;
        next
    } else {
        revision
    };
    load_snapshot(
        tx,
        revision,
        &session.state,
        context.now_ms,
        &session.changed_segments,
    )
    .await
}

async fn apply_intent(
    tx: &mut Transaction<'_, Sqlite>,
    session: &mut Session,
    intent: &FocusIntent,
    context: &FocusExecutionContext,
) -> Result<(), FocusExecutionError> {
    let now_ms = context.now_ms;
    match intent {
        FocusIntent::StartScheduled { occurrence_id } => {
            let commitment = context.commitment.as_ref().ok_or_else(|| {
                execution_error(
                    FocusErrorCode::IneligibleCommitment,
                    "There is no eligible scheduled Focus commitment",
                )
            })?;
            if occurrence_id
                .as_ref()
                .is_some_and(|id| id != &commitment.occurrence_id)
            {
                return Err(execution_error(
                    FocusErrorCode::IneligibleCommitment,
                    "The requested Focus occurrence is no longer eligible",
                ));
            }
            session
                .start_run(tx, commitment, now_ms, "manual", None)
                .await
        }
        FocusIntent::Pause => {
            if session.state.mode != FocusMode::Running
                || session.active_segment()?.phase != FocusPhase::Focus
            {
                return Err(invalid_state("Only a running focus phase can be paused"));
            }
            session.pause(tx, now_ms, now_ms, "manual").await?;
            session.state.mode = FocusMode::ManualPause;
            session.state.paused_prompts_dismissed = false;
            Ok(())
        }
        FocusIntent::Resume => {
            if session.state.mode != FocusMode::ManualPause {
                return Err(invalid_state("Focus is not manually paused"));
            }
            session.resume(tx, now_ms).await
        }
        FocusIntent::Stop => {
            if let Some(run) = session.run.as_ref().filter(|run| run.ended_at_ms.is_none()) {
                session.state.dismissed_occurrence_id = Some(run.occurrence_id.clone());
                session
                    .close_run(tx, now_ms, "stopped", "stopped", "stop", FocusMode::Stopped)
                    .await?;
            }
            Ok(())
        }
        FocusIntent::Advance => {
            if session.state.mode == FocusMode::ReturnWait {
                session.accept_return(tx, now_ms).await
            } else if session.state.mode == FocusMode::Running {
                session.advance(tx, now_ms, true, context).await
            } else {
                Err(invalid_state("A paused Focus phase cannot advance"))
            }
        }
        FocusIntent::SkipBreak => {
            if session.state.mode == FocusMode::ReturnWait {
                session.accept_return(tx, now_ms).await
            } else if session.state.mode == FocusMode::Running
                && session.active_segment()?.phase != FocusPhase::Focus
            {
                session
                    .record_event(tx, "skip_break", now_ms, Some("manual"), None)
                    .await?;
                let position = next_position(
                    &session.live_run()?.configuration,
                    session.active_segment()?.rhythm_position,
                );
                session
                    .close_segment(tx, now_ms, "interrupted", "skipped_by_user")
                    .await?;
                session
                    .start_segment(tx, now_ms, FocusPhase::Focus, position)
                    .await
            } else {
                Err(invalid_state("There is no running break to skip"))
            }
        }
        FocusIntent::SetSkipNextBreak { enabled } => {
            if session.active_segment()?.phase != FocusPhase::Focus {
                return Err(invalid_state("Skip next break requires a focus phase"));
            }
            session.state.skip_next_break = *enabled;
            Ok(())
        }
        FocusIntent::ExtendFocus { seconds } => {
            session.extend(tx, now_ms, *seconds * 1000, true).await
        }
        FocusIntent::ExtendBreak { seconds } => {
            session.extend(tx, now_ms, *seconds * 1000, false).await
        }
        FocusIntent::ResolveIdle { resume } => {
            if !matches!(
                session.state.mode,
                FocusMode::IdlePause | FocusMode::IdleFailed
            ) {
                return Err(invalid_state("There is no idle pause to resolve"));
            }
            if !resume {
                session.state.dismissed_occurrence_id =
                    Some(session.live_run()?.occurrence_id.clone());
                session
                    .close_run(tx, now_ms, "stopped", "stopped", "stop", FocusMode::Stopped)
                    .await
            } else if session.state.mode == FocusMode::IdleFailed {
                let position = session
                    .segment
                    .as_ref()
                    .ok_or_else(|| invalid_state("Failed Focus phase is missing"))?
                    .rhythm_position;
                session
                    .start_segment(tx, now_ms, FocusPhase::Focus, position)
                    .await
            } else {
                session.resume(tx, now_ms).await
            }
        }
        FocusIntent::ResolveSuspend { resume } => {
            if session.state.mode != FocusMode::Suspended {
                return Err(invalid_state("There is no suspend interval to resolve"));
            }
            if *resume {
                session.resume(tx, now_ms).await
            } else {
                session.state.dismissed_occurrence_id =
                    Some(session.live_run()?.occurrence_id.clone());
                session
                    .close_run(tx, now_ms, "stopped", "stopped", "stop", FocusMode::Stopped)
                    .await
            }
        }
        FocusIntent::SetIdleTimeout { minutes } => {
            session.state.idle_timeout_override_set = true;
            session.state.idle_timeout_override_minutes = *minutes;
            Ok(())
        }
        FocusIntent::SetAutomaticAdmissionSuppressed { suppressed } => {
            session.state.automatic_admission_suppressed = *suppressed;
            Ok(())
        }
        FocusIntent::DismissPausedPrompts => {
            if !matches!(
                session.state.mode,
                FocusMode::ManualPause
                    | FocusMode::IdlePause
                    | FocusMode::IdleFailed
                    | FocusMode::Suspended
                    | FocusMode::ReturnWait
            ) {
                return Err(invalid_state("Focus has no paused prompts to dismiss"));
            }
            session.state.paused_prompts_dismissed = true;
            Ok(())
        }
    }
}

impl Session {
    pub(super) async fn expire(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
    ) -> Result<bool, FocusExecutionError> {
        let Some(run) = self
            .run
            .as_ref()
            .filter(|run| run.ended_at_ms.is_none() && now_ms >= run.planned_end_ms)
        else {
            return Ok(false);
        };
        let deadline = run.planned_end_ms;
        self.close_run(
            tx,
            deadline,
            "completed",
            "event_expired",
            "complete",
            FocusMode::Expired,
        )
        .await?;
        Ok(true)
    }

    async fn resume(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
    ) -> Result<(), FocusExecutionError> {
        self.active_segment()?;
        self.close_pause(tx, now_ms).await?;
        self.state.mode = FocusMode::Running;
        self.clear_waits();
        self.update_deadline(tx, now_ms).await
    }

    pub(super) async fn accept_return(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
    ) -> Result<(), FocusExecutionError> {
        let previous = self
            .segment
            .as_ref()
            .ok_or_else(|| invalid_state("The completed Focus phase is missing"))?;
        // Android can also wait after focus completion when its host was backgrounded.
        let next_phase = if previous.phase == FocusPhase::Focus {
            if self.state.skip_next_break {
                FocusPhase::Focus
            } else {
                break_phase(&self.live_run()?.configuration, previous.rhythm_position)
            }
        } else {
            FocusPhase::Focus
        };
        let position = if next_phase == FocusPhase::Focus {
            next_position(&self.live_run()?.configuration, previous.rhythm_position)
        } else {
            previous.rhythm_position
        };
        self.record_event(
            tx,
            "start_focus_now",
            now_ms,
            Some("return"),
            self.state
                .return_started_at_ms
                .map(|start| now_ms.saturating_sub(start)),
        )
        .await?;
        self.state.skip_next_break = false;
        self.start_segment(tx, now_ms, next_phase, position).await
    }

    pub(super) async fn advance(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
        is_explicit: bool,
        context: &FocusExecutionContext,
    ) -> Result<(), FocusExecutionError> {
        let segment = self.active_segment()?;
        let phase = segment.phase;
        let position = segment.rhythm_position;
        if is_explicit {
            self.record_event(
                tx,
                if phase == FocusPhase::Focus {
                    "go_to_break_now"
                } else {
                    "start_focus_now"
                },
                now_ms,
                Some("manual"),
                None,
            )
            .await?;
        }
        let close_at = if is_explicit {
            now_ms
        } else {
            segment.planned_end_ms.min(now_ms)
        };
        self.close_segment(tx, close_at, "completed", "completed")
            .await?;
        if !is_explicit
            && (phase != FocusPhase::Focus
                || (context.platform == FocusPlatform::Android && !context.foreground))
        {
            self.state.mode = FocusMode::ReturnWait;
            self.state.return_started_at_ms = Some(close_at);
            self.state.paused_prompts_dismissed = false;
            return Ok(());
        }
        if phase == FocusPhase::Focus && !self.state.skip_next_break {
            let next_phase = break_phase(&self.live_run()?.configuration, position);
            self.start_segment(tx, now_ms, next_phase, position).await
        } else {
            if phase == FocusPhase::Focus {
                self.record_event(tx, "skip_break", now_ms, Some("skip_next_break"), None)
                    .await?;
            }
            self.state.skip_next_break = false;
            let next = next_position(&self.live_run()?.configuration, position);
            self.start_segment(tx, now_ms, FocusPhase::Focus, next)
                .await
        }
    }

    async fn extend(
        &mut self,
        tx: &mut Transaction<'_, Sqlite>,
        now_ms: i64,
        requested_ms: i64,
        is_focus: bool,
    ) -> Result<(), FocusExecutionError> {
        if !matches!(self.state.mode, FocusMode::Running | FocusMode::ManualPause) {
            return Err(invalid_state("This Focus state cannot be extended"));
        }
        let run = self.live_run()?;
        let segment = self.active_segment()?;
        if (segment.phase == FocusPhase::Focus) != is_focus
            || (is_focus && self.state.focus_extension_used)
        {
            return Err(invalid_state(
                "This phase cannot accept the requested extension",
            ));
        }
        let work_remaining_ms = segment
            .chosen_duration_ms
            .saturating_sub(segment_elapsed_ms(segment, now_ms)?)
            .max(0);
        let before = work_remaining_ms.min(run.planned_end_ms.saturating_sub(now_ms));
        let allowed_ms = if is_focus {
            requested_ms
        } else {
            requested_ms.min(MAX_BREAK_EXTENSION_MS - self.state.break_extension_ms)
        };
        let after = work_remaining_ms
            .saturating_add(allowed_ms)
            .min(run.planned_end_ms.saturating_sub(now_ms));
        if after <= before {
            return Err(invalid_state(
                "The event deadline leaves no room for this extension",
            ));
        }
        let added_ms = if is_focus { allowed_ms } else { after - before };
        if is_focus {
            self.record_event(tx, "extend_focus", now_ms, Some("manual"), Some(added_ms))
                .await?;
            self.state.focus_extension_used = true;
        } else {
            self.state.break_extension_ms += added_ms;
        }
        self.segment
            .as_mut()
            .ok_or_else(|| invalid_state("Focus phase disappeared"))?
            .chosen_duration_ms += added_ms;
        self.update_deadline(tx, now_ms).await
    }
}
