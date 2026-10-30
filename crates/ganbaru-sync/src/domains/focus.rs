//! Focus history and activity adapter: enumerated values and identifiers the column types cannot
//! express, and the hook that reports Focus changes so the app refreshes its native projections.

use ganbaru_sync_contracts::bounds::MAX_TEXT_FIELD_BYTES;
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};
use sqlx::SqliteConnection;

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::codec;
use crate::hooks::{ChangeSink, HookFuture, Hooks, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::focus::{
    ADAPTER, DEVICE_ACTIVITY_TABLE, DOMAIN, PAUSES, RUN_EVENTS_TABLE, RUNS_TABLE, SEGMENTS_TABLE,
    SEQUENCE_STEPS, activity_group, run_event_group, run_group, segment_group,
};

/// Most characters of a free text column: a title, date, occurrence id, or reason.
pub const MAX_TEXT_CHARS: usize = MAX_TEXT_FIELD_BYTES;

/// Most characters of an enumerated value.
pub const MAX_ENUM_CHARS: usize = 32;

/// Most steps of a sequence rhythm.
pub const MAX_RHYTHM_STEPS: usize = 12;

/// Most pauses Focus keeps on one segment.
pub const MAX_SEGMENT_PAUSES: usize = 1024;

/// Change topic of every Focus table, so the app refreshes Focus once per apply.
pub const TOPIC: &str = "focus";

/// Values the Focus schema allows for each enumerated column.
pub mod allowed {
    /// `pomodoro_runs.end_reason`.
    pub const RUN_END_REASONS: &[&str] = &[
        "completed",
        "stopped",
        "interrupted",
        "reconfigured",
        "block_transition",
    ];
    /// `pomodoro_runs.rhythm_kind`.
    pub const RHYTHM_KINDS: &[&str] = &["count", "sequence"];
    /// `pomodoro_runs.rhythm_source`.
    pub const RHYTHM_SOURCES: &[&str] = &["preset", "custom"];
    /// `pomodoro_runs.preset_key`.
    pub const PRESET_KEYS: &[&str] = &["adaptive", "creative", "balanced", "deep", "extended"];
    /// `pomodoro_runs.start_trigger`.
    pub const START_TRIGGERS: &[&str] = &[
        "manual",
        "block_auto",
        "block_transition",
        "reconfigure",
        "crash_recovery",
    ];
    /// `pomodoro_segments.phase` and `pomodoro_run_events.phase`.
    pub const PHASES: &[&str] = &["focus", "short_break", "long_break"];
    /// `pomodoro_run_sequence_steps.break_phase`.
    pub const BREAK_PHASES: &[&str] = &["short_break", "long_break"];
    /// `pomodoro_segments.status`.
    pub const SEGMENT_STATUSES: &[&str] = &["active", "completed", "interrupted"];
    /// `pomodoro_segments.end_reason`.
    pub const SEGMENT_END_REASONS: &[&str] = &[
        "completed",
        "stopped",
        "skipped_by_user",
        "event_expired",
        "focus_failed",
        "reconfigured",
        "block_transition",
        "crash_recovery",
    ];
    /// `pomodoro_pauses.reason`.
    pub const PAUSE_REASONS: &[&str] = &["idle", "manual", "suspend"];
    /// `pomodoro_run_events.event_type`.
    pub const RUN_EVENT_TYPES: &[&str] = &[
        "start",
        "phase_start",
        "phase_complete",
        "pause_start",
        "pause_end",
        "idle_detected",
        "focus_failed",
        "suspend_detected",
        "skip_break",
        "extend_focus",
        "go_to_break_now",
        "start_focus_now",
        "reconfigure",
        "block_transition",
        "stop",
        "complete",
        "crash_recovery",
    ];
    /// `pomodoro_device_activity.state`.
    pub const ACTIVITY_STATES: &[&str] = &["idle", "focus", "short_break", "long_break", "paused"];
    /// Activity states of a device with an open run.
    pub const RUNNING_ACTIVITY_STATES: &[&str] = &["focus", "short_break", "long_break", "paused"];
}

fn one_of(field: &Field, allowed: &[&str], what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Text(text) if allowed.contains(&text.as_str()) => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

fn optional_one_of(
    field: &Field,
    allowed: &[&str],
    what: &'static str,
) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => one_of(field, allowed, what),
    }
}

/// A null or non-blank text, as the schema requires of optional identifiers.
fn optional_non_blank(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        Field::Text(text) if !text.trim().is_empty() => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

fn fields<'a, const N: usize>(
    value: &'a Value,
    what: &'static str,
) -> Result<&'a [Field; N], AdapterError> {
    value.fields().try_into().map_err(|_| AdapterError(what))
}

fn validate_run(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == run_group::PLAN {
        let [_, _, _, _, kind, source, preset, _, _, trigger, _] = fields(value, "focus run plan")?;
        one_of(kind, allowed::RHYTHM_KINDS, "focus rhythm kind")?;
        one_of(source, allowed::RHYTHM_SOURCES, "focus rhythm source")?;
        optional_one_of(preset, allowed::PRESET_KEYS, "focus preset")?;
        one_of(trigger, allowed::START_TRIGGERS, "focus start trigger")
    } else if group == run_group::OUTCOME {
        let [_, _, reason, _] = fields(value, "focus run outcome")?;
        optional_one_of(reason, allowed::RUN_END_REASONS, "focus run end reason")
    } else if group == run_group::RHYTHM {
        let [_, Field::Blob(steps)] = fields(value, "focus run rhythm")? else {
            return Err(AdapterError("focus run rhythm"));
        };
        let rows = codec::decode(&SEQUENCE_STEPS, steps)
            .map_err(|_| AdapterError("focus sequence steps"))?;
        rows.iter().try_for_each(|row| match row.fields.as_slice() {
            [_, phase, _] => one_of(phase, allowed::BREAK_PHASES, "focus break phase"),
            _ => Err(AdapterError("focus sequence steps")),
        })
    } else if group == run_group::CALENDAR_LINK {
        let [_, occurrence, date, _, _] = fields(value, "focus calendar link")?;
        optional_non_blank(occurrence, "focus occurrence")?;
        optional_non_blank(date, "focus occurrence date")
    } else {
        Ok(())
    }
}

fn validate_segment(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == segment_group::PHASE {
        let [_, phase, _, _, _, _, status, reason, _, _, _] = fields(value, "focus segment")?;
        one_of(phase, allowed::PHASES, "focus segment phase")?;
        one_of(status, allowed::SEGMENT_STATUSES, "focus segment status")?;
        optional_one_of(
            reason,
            allowed::SEGMENT_END_REASONS,
            "focus segment end reason",
        )
    } else if group == segment_group::PAUSES {
        let [Field::Blob(bytes)] = fields(value, "focus pauses")? else {
            return Err(AdapterError("focus pauses"));
        };
        let rows = codec::decode(&PAUSES, bytes).map_err(|_| AdapterError("focus pauses"))?;
        rows.iter().try_for_each(|row| match row.fields.as_slice() {
            [_, _, reason, _, _] => one_of(reason, allowed::PAUSE_REASONS, "focus pause reason"),
            _ => Err(AdapterError("focus pauses")),
        })
    } else {
        Ok(())
    }
}

fn validate_run_event(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group != run_event_group::EVENT {
        return Ok(());
    }
    let [event_type, _, phase, _, _, _] = fields(value, "focus run event")?;
    one_of(event_type, allowed::RUN_EVENT_TYPES, "focus run event type")?;
    optional_one_of(phase, allowed::PHASES, "focus run event phase")
}

/// An idle row names no run; a running row names its run and phase times.
fn validate_activity(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group != activity_group::ACTIVITY {
        return Ok(());
    }
    let [state, run, occurrence, _, started, ends, _, _] = fields(value, "focus activity")?;
    one_of(state, allowed::ACTIVITY_STATES, "focus activity state")?;
    optional_non_blank(run, "focus activity run")?;
    optional_non_blank(occurrence, "focus activity occurrence")?;
    let running = matches!(
        state,
        Field::Text(state) if allowed::RUNNING_ACTIVITY_STATES.contains(&state.as_str())
    );
    let has_run = !matches!(run, Field::Null);
    if running != has_run {
        return Err(AdapterError("focus activity run"));
    }
    if let (Field::Integer(started), Field::Integer(ends)) = (started, ends)
        && ends < started
    {
        return Err(AdapterError("focus activity phase"));
    }
    Ok(())
}

/// Adapter of the Focus history and activity domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct FocusAdapter;

impl DomainAdapter for FocusAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    /// A closed run's Calendar link and a device's activity each have one writer at a time, so
    /// clocks decide.
    fn priority(&self, _table: TableId, _group: GroupId, _value: &Value) -> u8 {
        0
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        if table == RUNS_TABLE {
            validate_run(group, value)
        } else if table == SEGMENTS_TABLE {
            validate_segment(group, value)
        } else if table == RUN_EVENTS_TABLE {
            validate_run_event(group, value)
        } else if table == DEVICE_ACTIVITY_TABLE {
            validate_activity(group, value)
        } else {
            Ok(())
        }
    }

    /// Recovery titles of runs come from the event title they started with.
    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let mut presentation = Presentation::default();
        if table != RUNS_TABLE {
            return presentation;
        }
        for (group, value) in values {
            if *group == run_group::PLAN
                && let Some(Field::Text(title)) = value.fields().get(8)
            {
                presentation.title = title.trim().to_string();
            }
        }
        presentation
    }
}

/// Reports a Focus table a materialization changed under [`TOPIC`], so the app refreshes its
/// native Focus projections and the windows that render them.
#[derive(Debug, Default, Clone, Copy)]
pub struct FocusHook;

impl MaterializeHook for FocusHook {
    /// Focus derives nothing here: the hook only reports changes.
    fn rebuild_all<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            if !changed_keys.is_empty() {
                changes.report(TOPIC, table.name.to_string());
            }
            Ok(())
        })
    }
}

/// `hooks` with the Focus hook on every table of the domain.
pub fn with_hooks(hooks: Hooks) -> Hooks {
    DOMAIN.tables.iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(FocusHook))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::OwnedRow;
    use crate::manifest::vault::focus::COUNT_RHYTHMS;

    fn text(value: &str) -> Field {
        Field::Text(value.to_string())
    }

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    fn activity(state: &str, run: Field, started: i64, ends: i64) -> Result<(), AdapterError> {
        FocusAdapter.validate(
            DEVICE_ACTIVITY_TABLE,
            activity_group::ACTIVITY,
            &value(vec![
                text(state),
                run,
                Field::Null,
                Field::Null,
                Field::Integer(started),
                Field::Integer(ends),
                Field::Integer(ends),
                Field::Integer(started),
            ]),
        )
    }

    #[test]
    fn focus_activity_validation_ties_the_run_to_the_state() {
        assert_eq!(activity("focus", text("run-1"), 10, 20), Ok(()));
        assert_eq!(activity("paused", text("run-1"), 10, 20), Ok(()));
        assert_eq!(activity("idle", Field::Null, 10, 10), Ok(()));
        assert!(activity("idle", text("run-1"), 10, 20).is_err());
        assert!(activity("focus", Field::Null, 10, 20).is_err());
        assert!(activity("focus", text(" "), 10, 20).is_err());
        assert!(activity("napping", Field::Null, 10, 20).is_err());
        assert!(activity("focus", text("run-1"), 20, 10).is_err());
    }

    #[test]
    fn focus_validation_refuses_enumerated_values_the_schema_refuses() {
        let adapter = FocusAdapter;
        let outcome = |reason: Field| {
            adapter.validate(
                RUNS_TABLE,
                run_group::OUTCOME,
                &value(vec![text("t"), text("t"), reason, text("t")]),
            )
        };
        assert_eq!(outcome(Field::Null), Ok(()));
        assert_eq!(outcome(text("block_transition")), Ok(()));
        assert!(outcome(text("abandoned")).is_err());

        let event = |event_type: &str, phase: Field| {
            adapter.validate(
                RUN_EVENTS_TABLE,
                run_event_group::EVENT,
                &value(vec![
                    text(event_type),
                    text("t"),
                    phase,
                    Field::Null,
                    Field::Null,
                    text("t"),
                ]),
            )
        };
        assert_eq!(event("skip_break", text("short_break")), Ok(()));
        assert!(event("teleport", Field::Null).is_err());
        assert!(event("start", text("nap")).is_err());

        let link = |occurrence: Field| {
            adapter.validate(
                RUNS_TABLE,
                run_group::CALENDAR_LINK,
                &value(vec![
                    Field::Null,
                    occurrence,
                    Field::Null,
                    Field::Null,
                    Field::Null,
                ]),
            )
        };
        assert_eq!(link(text("occurrence-1")), Ok(()));
        assert!(link(text("  ")).is_err());
    }

    #[test]
    fn focus_validation_reads_owned_pauses_and_steps() {
        let adapter = FocusAdapter;
        let pause = |reason: &str| {
            let row = OwnedRow::flat(vec![
                text("t"),
                Field::Null,
                text(reason),
                Field::Null,
                text("t"),
            ]);
            let blob = codec::encode(&PAUSES, &[row]).unwrap();
            adapter.validate(
                SEGMENTS_TABLE,
                segment_group::PAUSES,
                &value(vec![Field::Blob(blob)]),
            )
        };
        assert_eq!(pause("suspend"), Ok(()));
        assert!(pause("coffee").is_err());

        let rhythm = |break_phase: &str| {
            let count = codec::encode(&COUNT_RHYTHMS, &[]).unwrap();
            let step = OwnedRow::flat(vec![
                Field::Integer(25),
                text(break_phase),
                Field::Integer(5),
            ]);
            let steps = codec::encode(&SEQUENCE_STEPS, &[step]).unwrap();
            adapter.validate(
                RUNS_TABLE,
                run_group::RHYTHM,
                &value(vec![Field::Blob(count), Field::Blob(steps)]),
            )
        };
        assert_eq!(rhythm("long_break"), Ok(()));
        assert!(rhythm("focus").is_err());
    }
}
