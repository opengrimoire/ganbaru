use super::super::PomodoroRunRhythm;
use super::models::*;

pub(super) const FOCUS_EXTENSION_MS: i64 = 180_000;
pub(super) const MAX_BREAK_EXTENSION_MS: i64 = 180_000;
pub(super) const MAX_COMMAND_ID_BYTES: usize = 128;

pub(super) fn validate_command(command: &FocusCommand) -> Result<(), FocusExecutionError> {
    if command.command_id.trim().is_empty()
        || command.command_id.len() > MAX_COMMAND_ID_BYTES
        || command.command_id.chars().any(char::is_control)
        || command.expected_revision < 0
    {
        return Err(execution_error(
            FocusErrorCode::InvalidIntent,
            "Focus command identity or revision is invalid",
        ));
    }
    match &command.intent {
        FocusIntent::StartScheduled {
            occurrence_id: Some(id),
        } if id.trim().is_empty() || id.len() > 1024 => Err(execution_error(
            FocusErrorCode::InvalidIntent,
            "Focus occurrence identity is invalid",
        )),
        FocusIntent::ExtendFocus { seconds }
            if *seconds <= 0 || *seconds > FOCUS_EXTENSION_MS / 1000 =>
        {
            Err(execution_error(
                FocusErrorCode::InvalidIntent,
                "Focus extension exceeds its accepted limit",
            ))
        }
        FocusIntent::ExtendBreak { seconds }
            if *seconds <= 0 || *seconds > MAX_BREAK_EXTENSION_MS / 1000 =>
        {
            Err(execution_error(
                FocusErrorCode::InvalidIntent,
                "Break extension exceeds its accepted limit",
            ))
        }
        FocusIntent::SetIdleTimeout {
            minutes: Some(minutes),
        } if *minutes <= 0 || *minutes > 120 => Err(execution_error(
            FocusErrorCode::InvalidIntent,
            "Focus idle timeout must be between 1 and 120 minutes",
        )),
        _ => Ok(()),
    }
}

pub(super) fn validate_configuration(
    config: &FocusConfiguration,
) -> Result<(), FocusExecutionError> {
    super::super::validation::validate_run_rhythm(&config.rhythm)
        .map_err(|message| execution_error(FocusErrorCode::InvalidIntent, message))?;
    if !matches!(config.rhythm_source.as_str(), "preset" | "custom")
        || config.preset_key.as_deref().is_some_and(|key| {
            !matches!(
                key,
                "adaptive" | "creative" | "balanced" | "deep" | "extended"
            )
        })
        || (config.rhythm_source == "preset") != config.preset_key.is_some()
        || config
            .idle_timeout_minutes
            .is_some_and(|minutes| minutes <= 0 || minutes > 120)
    {
        return Err(execution_error(
            FocusErrorCode::InvalidIntent,
            "Focus configuration is invalid",
        ));
    }
    Ok(())
}

pub(super) fn validate_commitment(
    commitment: &FocusCommitment,
    now_ms: i64,
) -> Result<(), FocusExecutionError> {
    validate_configuration(&commitment.configuration)?;
    if commitment.event_id.trim().is_empty()
        || commitment.occurrence_id.trim().is_empty()
        || commitment.calendar_revision.trim().is_empty()
        || commitment.start_ms >= commitment.end_ms
        || now_ms < commitment.start_ms
        || now_ms >= commitment.end_ms
        || super::super::validation::canonical_event_id(&commitment.occurrence_id)
            .map_err(|message| execution_error(FocusErrorCode::IneligibleCommitment, message))?
            != commitment.event_id
    {
        return Err(execution_error(
            FocusErrorCode::IneligibleCommitment,
            "The scheduled Focus commitment is no longer eligible",
        ));
    }
    chrono::NaiveDate::parse_from_str(&commitment.event_date, "%Y-%m-%d").map_err(|_| {
        execution_error(
            FocusErrorCode::IneligibleCommitment,
            "Focus commitment date is invalid",
        )
    })?;
    Ok(())
}

pub(super) fn position_count(config: &FocusConfiguration) -> i64 {
    match &config.rhythm {
        PomodoroRunRhythm::Count {
            long_break_after_focus_count,
            ..
        } => *long_break_after_focus_count,
        PomodoroRunRhythm::Sequence { steps } => steps.len() as i64,
    }
}

pub(super) fn normalize_position(config: &FocusConfiguration, position: i64) -> i64 {
    (position.saturating_sub(1)).rem_euclid(position_count(config).max(1)) + 1
}

pub(super) fn next_position(config: &FocusConfiguration, position: i64) -> i64 {
    normalize_position(config, position.saturating_add(1))
}

pub(super) fn phase_duration_ms(
    config: &FocusConfiguration,
    phase: FocusPhase,
    position: i64,
) -> i64 {
    let minutes = match &config.rhythm {
        PomodoroRunRhythm::Count {
            focus_duration_minutes,
            short_break_minutes,
            long_break_minutes,
            ..
        } => match phase {
            FocusPhase::Focus => *focus_duration_minutes,
            FocusPhase::ShortBreak => *short_break_minutes,
            FocusPhase::LongBreak => *long_break_minutes,
        },
        PomodoroRunRhythm::Sequence { steps } => {
            let index = (normalize_position(config, position) - 1) as usize;
            steps
                .get(index)
                .map(|step| {
                    if phase == FocusPhase::Focus {
                        step.focus_duration_minutes
                    } else {
                        step.break_duration_minutes
                    }
                })
                .unwrap_or(0)
        }
    };
    minutes.saturating_mul(60_000)
}

pub(super) fn break_phase(config: &FocusConfiguration, position: i64) -> FocusPhase {
    let position = normalize_position(config, position);
    match &config.rhythm {
        PomodoroRunRhythm::Count {
            long_break_after_focus_count,
            ..
        } => {
            if position == *long_break_after_focus_count {
                FocusPhase::LongBreak
            } else {
                FocusPhase::ShortBreak
            }
        }
        PomodoroRunRhythm::Sequence { steps } => {
            if steps
                .get((position - 1) as usize)
                .is_some_and(|step| step.break_phase == "long_break")
            {
                FocusPhase::LongBreak
            } else {
                FocusPhase::ShortBreak
            }
        }
    }
}

/// A first segment may continue work accepted by the previous run. Later segments start fresh.
pub(super) fn inherited_phase_ms(run: &FocusRunSnapshot, segment: &FocusSegmentSnapshot) -> i64 {
    if segment.actual_start_ms == run.started_at_ms {
        run.inherited_phase_ms
    } else {
        0
    }
}

pub(super) fn invalid_state(message: &str) -> FocusExecutionError {
    execution_error(FocusErrorCode::InvalidState, message)
}
