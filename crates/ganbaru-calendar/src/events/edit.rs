//! Typed user draft preparation. Source scope and execution evidence stay native.

use std::collections::BTreeSet;
use std::io::{self, Write};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::description::sanitize_calendar_description_html;
use crate::recurrence::canonical::{
    EditGeometryPlan, EditScope, GeometryDraft, RecurrenceIntent, ResolvedTiming, ScopeClock,
    ScopePlan, TimingIntent,
};

use super::children::{
    parse_geo, parse_i64_list, parse_organizer, parse_string_list, parse_string_map,
};
use super::scope::{ScopeRequest, ScopeSnapshot};
use super::types::{
    CalendarEventAlarm, CalendarEventAttendee, CalendarEventUpdateField,
    CalendarPomodoroConfigPatch,
};
use super::validation::{
    validate_alarm, validate_attendee, validate_pomodoro_config, validate_update_field,
};

const MAX_DRAFT_BYTES: usize = 1024 * 1024;
const MAX_FIELDS: usize = 32;
const MAX_CHILDREN: usize = 256;

/// User edits omit derived exceptions, split boundaries, IDs, clocks and timestamps.
#[derive(Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EventDraft {
    #[serde(default)]
    pub(super) timing: TimingIntent,
    #[serde(default)]
    pub(super) recurrence: RecurrenceIntent,
    #[serde(default)]
    pub(super) fields: Vec<CalendarEventUpdateField>,
    pub(super) attendees: Option<Vec<CalendarEventAttendee>>,
    pub(super) alarms: Option<Vec<CalendarEventAlarm>>,
    pub(super) pomodoro_config: Option<CalendarPomodoroConfigPatch>,
}

/// Original source selection plus user intent, without derived write operations.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EditRequest {
    pub(super) selection: ScopeRequest,
    pub(super) draft: EventDraft,
    #[serde(default, skip_serializing_if = "EditAction::is_save")]
    pub(super) action: EditAction,
}

/// Compound user actions share Calendar's review, receipt and transaction.
#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EditAction {
    #[default]
    Save,
    EndNow,
    EnableFocus,
}

impl EditAction {
    fn is_save(&self) -> bool {
        *self == Self::Save
    }
}

/// Validated intent and its native protection decision. This intermediate read
/// model is not a visible preview or a client-authorized commit operation list.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedEdit {
    #[serde(skip)]
    pub(super) action: EditAction,
    source_revision: String,
    pub(super) review_revision: String,
    pub(super) scope: ScopePlan,
    pub(super) selected_after: ResolvedTiming,
    pub(super) draft: EventDraft,
    pub(super) plan: EditGeometryPlan,
    #[serde(skip)]
    pub(super) metadata: super::metadata::Metadata,
    #[serde(skip)]
    pub(super) evidence: super::scope::ExecutionEvidence,
    #[serde(skip)]
    pub(super) source_anchor: NaiveDate,
    #[serde(skip)]
    pub(super) source_id: String,
}

struct SizeLimit(usize);

impl Write for SizeLimit {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("Calendar draft exceeds its byte budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl EventDraft {
    /// Bound payload work before SQL without allocating a second serialized copy.
    pub(super) fn check_limits(&self) -> Result<(), String> {
        if self.fields.len() > MAX_FIELDS
            || self
                .attendees
                .as_ref()
                .is_some_and(|rows| rows.len() > MAX_CHILDREN)
            || self
                .alarms
                .as_ref()
                .is_some_and(|rows| rows.len() > MAX_CHILDREN)
        {
            return Err("Calendar draft exceeds its field or child record budget".into());
        }
        serde_json::to_writer(&mut SizeLimit(MAX_DRAFT_BYTES), &self)
            .map_err(|error| format!("measure Calendar draft: {error}"))?;
        Ok(())
    }

    /// Reject ambiguous repeated fields and raw persistence authority. Parsing
    /// and sanitization run on the admitted blocking preparation worker.
    pub(super) fn validate(&mut self) -> Result<(), String> {
        self.check_limits()?;
        let mut seen = BTreeSet::new();
        for field in &mut self.fields {
            if !seen.insert(field.field_name()) {
                return Err(format!(
                    "Calendar draft repeats field '{}'",
                    field.field_name()
                ));
            }
            use CalendarEventUpdateField as Field;
            match field {
                Field::StartTime(_)
                | Field::EndTime(_)
                | Field::Timezone(_)
                | Field::AllDay(_)
                | Field::Rrule(_)
                | Field::RepeatUntil(_)
                | Field::Exceptions(_)
                | Field::Rdate(_)
                | Field::SourceUid(_)
                | Field::Sequence(_) => {
                    return Err(format!(
                        "Calendar scoped draft cannot supply raw field '{}'",
                        field.field_name()
                    ));
                }
                Field::Description(value) => *value = sanitize_calendar_description_html(value),
                Field::Notifications(value) => {
                    let offsets = parse_i64_list(value, "notifications")?;
                    if offsets.len() > MAX_CHILDREN || offsets.iter().any(|value| *value < 0) {
                        return Err(
                            "Calendar notification offsets exceed their range or record budget"
                                .into(),
                        );
                    }
                }
                Field::Categories(value) => {
                    if parse_string_list(value, "categories")?.len() > MAX_CHILDREN {
                        return Err("Calendar categories exceed their record budget".into());
                    }
                }
                Field::ExtendedProperties(value) => {
                    if parse_string_map(value, "extended properties")?.len() > MAX_CHILDREN {
                        return Err("Calendar properties exceed their record budget".into());
                    }
                }
                Field::Geo(value) => {
                    if parse_geo(value)?.is_some_and(|(lat, lng)| {
                        !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng)
                    }) {
                        return Err("Calendar coordinates are outside geographic bounds".into());
                    }
                }
                Field::Organizer(value) => {
                    parse_organizer(value)?;
                }
                Field::MusicSnapshotAssignments(assignments) => {
                    ganbaru_music::assignments::validate_drafts(
                        ganbaru_music::assignments::MusicAssignmentOwnerKind::EventSnapshot,
                        assignments,
                    )
                    .map_err(|error| format!("Calendar Music snapshot: {error}"))?;
                }
                Field::MusicOverrideAssignments(assignments) => {
                    ganbaru_music::assignments::validate_drafts(
                        ganbaru_music::assignments::MusicAssignmentOwnerKind::EventOverride,
                        assignments,
                    )
                    .map_err(|error| format!("Calendar Music override: {error}"))?;
                }
                _ => {}
            }
            validate_update_field(field)?;
        }
        let mut attendees = BTreeSet::new();
        for attendee in self.attendees.iter().flatten() {
            validate_attendee(attendee)?;
            if !attendees.insert(&attendee.id) {
                return Err("Calendar draft repeats an attendee identity".into());
            }
        }
        let mut alarms = BTreeSet::new();
        for alarm in self.alarms.iter().flatten() {
            validate_alarm(alarm)?;
            if !alarms.insert(&alarm.id) {
                return Err("Calendar draft repeats an alarm identity".into());
            }
        }
        if let Some(CalendarPomodoroConfigPatch::Set(config)) = &self.pomodoro_config {
            validate_pomodoro_config(config)?;
        }
        self.check_limits()
    }
}

/// No database writes occur during preparation. Commit must reacquire its own
/// authorized snapshot and repeat both source protection and draft preparation.
#[cfg(test)]
pub(super) fn prepare(
    snapshot: ScopeSnapshot,
    selected: NaiveDate,
    scope: EditScope,
    clock: ScopeClock,
    draft: EventDraft,
) -> Result<PreparedEdit, String> {
    prepare_action(snapshot, selected, scope, clock, draft, EditAction::Save)
}

/// Prepare semantic intent without writes. Commit repeats preparation against
/// an authorized transaction and the native owner's acceptance clock.
pub(super) fn prepare_action(
    snapshot: ScopeSnapshot,
    selected: NaiveDate,
    scope: EditScope,
    clock: ScopeClock,
    mut draft: EventDraft,
    action: EditAction,
) -> Result<PreparedEdit, String> {
    draft.validate()?;
    if action == EditAction::EnableFocus
        && (scope != EditScope::This
            || !snapshot.metadata.can_enable_focus()
            || !matches!(draft.recurrence, RecurrenceIntent::Unchanged)
            || !matches!(
                draft.pomodoro_config,
                Some(CalendarPomodoroConfigPatch::Set(_))
            ))
    {
        return Err("Enable Focus requires a standalone occurrence without Focus configuration and an explicit configuration".into());
    }
    // `EndNow` reviews semantic intent, not a preview's already aging cutoff.
    // Source and protection still participate in the final review digest.
    let action_intent = if action == EditAction::EndNow {
        if scope != EditScope::This
            || draft.timing.end_time.is_some()
            || !matches!(draft.recurrence, RecurrenceIntent::Unchanged)
        {
            return Err("End now requires this occurrence, an unchanged recurrence chain and no caller cutoff".into());
        }
        let intent = super::metadata::revision(&draft)?;
        draft.timing.end_time = Some(
            chrono::DateTime::from_timestamp_millis(clock.epoch_ms)
                .ok_or("Calendar acceptance clock exceeds its supported range")?
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        );
        Some(intent)
    } else {
        None
    };
    let source_revision = snapshot.metadata.revision()?;
    let template = snapshot.geometry.template(true)?;
    let evidence = snapshot
        .evidence
        .resolve(&snapshot.geometry.source.id, template.anchor_date())?;
    let mut metadata = snapshot.metadata.clone();
    if scope == EditScope::This
        || evidence
            .active
            .as_ref()
            .is_some_and(|(_, date)| *date == selected)
    {
        metadata.inherit_occurrence(&selected.to_string())?;
    }
    let metadata_changed = metadata.apply_draft(&draft, clock.epoch_ms)?;
    let (scope, selected_after, plan) = template.prepare_edit_geometry(
        selected,
        scope,
        evidence,
        clock,
        GeometryDraft {
            timing: &draft.timing,
            recurrence: &mut draft.recurrence,
            source_zone: &snapshot.geometry.source.timezone,
            metadata_changed,
        },
    )?;
    if scope.effective_scope == EditScope::This
        && plan.kind != crate::recurrence::canonical::EditKind::Unchanged
    {
        snapshot
            .evidence
            .verify_completed_before(&scope.selected.id, clock.epoch_ms)?;
    }
    if action == EditAction::EndNow
        && (snapshot.geometry.source.all_day != 0
            || selected_after.all_day
            || scope.selected.start_ms >= clock.epoch_ms
            || scope.selected.end_ms <= clock.epoch_ms
            || selected_after.start_ms != scope.selected.start_ms
            || selected_after.end_ms != clock.epoch_ms)
    {
        return Err("End now requires an ongoing timed occurrence and its unchanged start".into());
    }
    if action == EditAction::EnableFocus
        && (snapshot.geometry.source.all_day != 0
            || selected_after.all_day
            || scope.selected.start_ms > clock.epoch_ms
            || scope.selected.end_ms <= clock.epoch_ms
            || selected_after.start_ms != scope.selected.start_ms
            || selected_after.end_ms <= clock.epoch_ms)
    {
        return Err(
            "Enable Focus requires an ongoing timed occurrence and its unchanged start".into(),
        );
    }
    if selected_after.all_day {
        if matches!(
            draft.pomodoro_config,
            Some(CalendarPomodoroConfigPatch::Set(_))
        ) {
            return Err("All-day Calendar drafts cannot enable a Focus configuration".into());
        }
        // A conversion to floating dates must clear any inherited configuration.
        if snapshot.geometry.source.all_day == 0 {
            draft.pomodoro_config = Some(CalendarPomodoroConfigPatch::Clear);
        }
    }
    Ok(PreparedEdit {
        action,
        review_revision: if let Some(intent) = action_intent {
            super::metadata::revision(&(&source_revision, &scope, intent, action))?
        } else if action == EditAction::EnableFocus {
            super::metadata::revision(&(
                &source_revision,
                &scope,
                &selected_after,
                &draft,
                &plan,
                action,
            ))?
        } else {
            super::metadata::revision(&(&source_revision, &scope, &selected_after, &draft, &plan))?
        },
        source_revision,
        scope,
        selected_after,
        draft,
        plan,
        metadata: snapshot.metadata,
        evidence: snapshot.evidence,
        source_anchor: template.anchor_date(),
        source_id: snapshot.geometry.source.id,
    })
}

#[cfg(test)]
mod tests;
