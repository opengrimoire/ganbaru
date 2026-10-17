//! Project prepared writes through the same slim rows and expander as persisted reads.

use super::{Metadata, PreparedMutation};
use crate::reads::native_window::{NativeCalendarWindow, WindowSource};
use crate::reads::{DbCalendarEventRow, DbOverrideRow, DbWindowAttendeeRow};
use crate::recurrence::canonical::Window;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SequenceStep<'a> {
    focus_duration_minutes: i64,
    break_phase: &'a str,
    break_duration_minutes: i64,
}

impl PreparedMutation {
    /// Unchanged plans have no writes, so project their captured source instead.
    pub(in crate::events) fn project(
        &self,
        original: &Metadata,
        window: &Window,
    ) -> Result<NativeCalendarWindow, String> {
        let mut events = Vec::new();
        let mut overrides = Vec::new();
        let mut attendees = Vec::new();
        let sources: Vec<&Metadata> = if self.sides.is_empty() {
            vec![original]
        } else {
            self.sides.iter().map(|side| &side.metadata).collect()
        };
        for source in sources {
            events.push(source.window_event()?);
            overrides.extend(source.overrides.iter().map(|row| DbOverrideRow {
                id: row.id.clone(),
                parent_event_id: row.parent_event_id.clone(),
                recurrence_id: row.recurrence_id.clone(),
                recurrence_range: row.recurrence_range.clone(),
                title: row.title.clone(),
                start_time: row.start_time.clone(),
                end_time: row.end_time.clone(),
                color: row.color,
                status: row.status.clone(),
                transparency: row.transparency.clone(),
            }));
            let mut ordered_attendees: Vec<_> = source.attendees.iter().collect();
            ordered_attendees.sort_by_key(|row| (row.sort_order, &row.id));
            attendees.extend(
                ordered_attendees
                    .into_iter()
                    .map(|row| DbWindowAttendeeRow {
                        event_id: row.event_id.clone(),
                        email: row.email.clone(),
                        status: row.status.clone(),
                    }),
            );
        }
        events.sort_by(|a, b| (&a.start_time, &a.id).cmp(&(&b.start_time, &b.id)));
        overrides.sort_by(|a, b| {
            (&a.parent_event_id, &a.recurrence_id, &a.id).cmp(&(
                &b.parent_event_id,
                &b.recurrence_id,
                &b.id,
            ))
        });
        attendees.sort_by(|a, b| a.event_id.cmp(&b.event_id));
        WindowSource::from_prepared(events, overrides, attendees).expand(window)
    }
}

impl Metadata {
    fn window_event(&self) -> Result<DbCalendarEventRow, String> {
        let event = self
            .events
            .first()
            .ok_or("Calendar preview source is missing")?;
        let config = self.focus_configs.first();
        let rhythm = self.count_rhythms.first();
        let mut notifications: Vec<_> = self.notifications.iter().collect();
        notifications.sort_by_key(|row| (row.sort_order, &row.id));
        let mut exdates: Vec<_> = self.exdates.iter().collect();
        exdates.sort_by_key(|row| (row.sort_order, &row.id));
        let mut rdates: Vec<_> = self.rdates.iter().collect();
        rdates.sort_by_key(|row| (row.sort_order, &row.id));
        let sequence_steps = if self.sequence_steps.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(
                    &self
                        .sequence_steps
                        .iter()
                        .map(|step| SequenceStep {
                            focus_duration_minutes: step.focus_duration_minutes,
                            break_phase: &step.break_phase,
                            break_duration_minutes: step.break_duration_minutes,
                        })
                        .collect::<Vec<_>>(),
                )
                .map_err(|error| format!("encode Calendar preview rhythm: {error}"))?,
            )
        };
        Ok(DbCalendarEventRow {
            id: event.id.clone(),
            title: event.title.clone(),
            start_time: event.start_time.clone(),
            end_time: event.end_time.clone(),
            timezone: event.timezone.clone(),
            calendar_id: event.calendar_id.clone(),
            project_id: event.project_id.clone(),
            environment_id: event.environment_id.clone(),
            playlist_id: event.playlist_id.clone(),
            color: event.color,
            rrule: event.rrule.clone(),
            notifications: encode_list(
                notifications.iter().map(|row| row.offset_minutes).collect(),
            )?,
            exceptions: encode_list(exdates.iter().map(|row| &row.occurrence_date).collect())?,
            repeat_until: event.repeat_until.clone(),
            all_day: event.all_day,
            location: event.location.clone(),
            has_call_link: i64::from(!event.url.is_empty()),
            meeting_enabled: event.meeting_enabled,
            transparency: event.transparency.clone(),
            status: event.status.clone(),
            local_rsvp_status: event.local_rsvp_status.clone(),
            created_at: event.created_at.clone(),
            rdate: encode_list(rdates.iter().map(|row| &row.occurrence_start).collect())?,
            rhythm_kind: config.map(|row| row.rhythm_kind.clone()),
            rhythm_source: config.map(|row| row.rhythm_source.clone()),
            preset_key: config.and_then(|row| row.preset_key.clone()),
            count_focus_duration_minutes: rhythm.map(|row| row.focus_duration_minutes),
            count_short_break_minutes: rhythm.map(|row| row.short_break_minutes),
            count_long_break_minutes: rhythm.map(|row| row.long_break_minutes),
            count_long_break_after_focus_count: rhythm.map(|row| row.long_break_after_focus_count),
            sequence_steps,
            idle_timeout_minutes: config.and_then(|row| row.idle_timeout_minutes),
        })
    }
}

fn encode_list<T: Serialize>(values: Vec<T>) -> Result<Option<String>, String> {
    if values.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&values)
        .map(Some)
        .map_err(|error| format!("encode Calendar preview list: {error}"))
}
