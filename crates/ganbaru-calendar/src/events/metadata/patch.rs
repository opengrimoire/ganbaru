//! Apply validated editor intent to captured rows without database access.

use super::super::children::{
    parse_geo, parse_i64_list, parse_organizer, parse_string_list, parse_string_map,
};
use super::super::edit::EventDraft;
use super::super::types::{
    CalendarEventUpdateField as Field, CalendarPomodoroConfigPatch, CalendarPomodoroRhythm,
};
use super::Metadata;
use super::copy::copied_id;
use super::rows::*;

impl Metadata {
    /// Omitted fields retain their canonical values. Explicit nulls and empty
    /// child lists clear only the named field. Unchanged children keep provenance.
    pub(in crate::events) fn apply_draft(
        &mut self,
        draft: &EventDraft,
        now_ms: i64,
    ) -> Result<bool, String> {
        let mut changed = false;
        let event = self
            .events
            .first_mut()
            .ok_or("Calendar metadata source is missing")?;
        macro_rules! assign {
            ($field:ident, $value:expr) => {{
                let value = $value;
                if event.$field != value {
                    event.$field = value;
                    changed = true;
                }
            }};
        }
        for field in &draft.fields {
            match field {
                Field::Title(value) => assign!(title, value.clone()),
                Field::CalendarId(value) => assign!(calendar_id, value.clone()),
                Field::ProjectId(value) => assign!(project_id, value.clone()),
                Field::EnvironmentId(value) => assign!(environment_id, value.clone()),
                Field::PlaylistId(value) => assign!(playlist_id, value.clone()),
                Field::Color(value) => assign!(color, *value),
                Field::Description(value) => assign!(description, value.clone()),
                Field::Location(value) => assign!(location, value.clone()),
                Field::Url(value) => assign!(url, value.clone()),
                Field::Transparency(value) => assign!(transparency, value.clone()),
                Field::Status(value) => assign!(status, value.clone()),
                Field::Visibility(value) => assign!(visibility, value.clone()),
                Field::Priority(value) => assign!(priority, *value),
                Field::MeetingEnabled(value) => assign!(meeting_enabled, i64::from(*value)),
                Field::LocalRsvpStatus(value) => assign!(local_rsvp_status, value.clone()),
                Field::GuestPermissions(value) => {
                    assign!(guest_can_modify, i64::from(value.guest_can_modify));
                    assign!(
                        guest_can_invite_others,
                        i64::from(value.guest_can_invite_others)
                    );
                    assign!(
                        guest_can_see_other_guests,
                        i64::from(value.guest_can_see_other_guests)
                    );
                }
                Field::Geo(value) => {
                    let geo = parse_geo(value)?;
                    assign!(geo_lat, geo.map(|pair| pair.0));
                    assign!(geo_lng, geo.map(|pair| pair.1));
                }
                Field::Organizer(value) => {
                    let rows = parse_organizer(value)?
                        .into_iter()
                        .map(|row| Organizer {
                            event_id: event.id.clone(),
                            name: row.name,
                            email: row.email,
                        })
                        .collect::<Vec<_>>();
                    if self.organizers != rows {
                        self.organizers = rows;
                        changed = true;
                    }
                }
                Field::Notifications(value) => {
                    let values = parse_i64_list(value, "notifications")?;
                    self.notifications.sort_by_key(|row| row.sort_order);
                    if !self
                        .notifications
                        .iter()
                        .map(|row| row.offset_minutes)
                        .eq(values.iter().copied())
                    {
                        self.notifications = values
                            .into_iter()
                            .enumerate()
                            .map(|(index, value)| {
                                Ok(Notification {
                                    id: copied_id(&event.id, "notification", &index.to_string())?,
                                    event_id: event.id.clone(),
                                    offset_minutes: value,
                                    sort_order: index as i64,
                                })
                            })
                            .collect::<Result<_, String>>()?;
                        changed = true;
                    }
                }
                Field::Categories(value) => {
                    let values = parse_string_list(value, "categories")?;
                    self.categories.sort_by_key(|row| row.sort_order);
                    if !self
                        .categories
                        .iter()
                        .map(|row| &row.category)
                        .eq(values.iter())
                    {
                        self.categories = values
                            .into_iter()
                            .enumerate()
                            .map(|(index, value)| {
                                Ok(Category {
                                    id: copied_id(&event.id, "category", &index.to_string())?,
                                    event_id: event.id.clone(),
                                    category: value,
                                    sort_order: index as i64,
                                })
                            })
                            .collect::<Result<_, String>>()?;
                        changed = true;
                    }
                }
                Field::ExtendedProperties(value) => {
                    let values = parse_string_map(value, "extended properties")?;
                    self.properties
                        .sort_by(|a, b| a.property_key.cmp(&b.property_key));
                    if !self
                        .properties
                        .iter()
                        .map(|row| (&row.property_key, &row.property_value))
                        .eq(values.iter().map(|(key, value)| (key, value)))
                    {
                        self.properties = values
                            .into_iter()
                            .enumerate()
                            .map(|(index, (key, value))| {
                                Ok(Property {
                                    id: copied_id(&event.id, "extended-property", &key)?,
                                    event_id: event.id.clone(),
                                    property_key: key,
                                    property_value: value,
                                    sort_order: index as i64,
                                })
                            })
                            .collect::<Result<_, String>>()?;
                        changed = true;
                    }
                }
                Field::MusicSnapshotAssignments(values)
                | Field::MusicOverrideAssignments(values) => {
                    let owner_kind = if matches!(field, Field::MusicSnapshotAssignments(_)) {
                        "event-snapshot"
                    } else {
                        "event-override"
                    };
                    let mut rows = Vec::with_capacity(values.len());
                    for value in values {
                        let existing = self.music_assignments.iter().find(|row| {
                            row.owner_kind == owner_kind && row.phase == value.phase.as_ref()
                        });
                        let mut row = MusicAssignment {
                            owner_kind: owner_kind.into(),
                            owner_id: event.id.clone(),
                            phase: value.phase.as_ref().into(),
                            behavior: value.behavior.as_ref().into(),
                            playlist_id: value.playlist_id.clone(),
                            soundscape_id: value.soundscape_id.clone(),
                            soundscape_behavior: value.soundscape_behavior.as_ref().into(),
                            provenance_kind: value.provenance_kind.as_ref().into(),
                            provenance_id: value.provenance_id.clone(),
                            updated_at_ms: existing.map_or(now_ms, |row| row.updated_at_ms),
                            version: existing.map_or(1, |row| row.version),
                        };
                        if existing != Some(&row) {
                            row.updated_at_ms = now_ms;
                            row.version = existing.map_or(Ok(1), |row| {
                                row.version
                                    .checked_add(1)
                                    .ok_or("Calendar Music version overflow")
                            })?;
                            changed = true;
                        }
                        rows.push(row);
                    }
                    if self
                        .music_assignments
                        .iter()
                        .filter(|row| row.owner_kind == owner_kind)
                        .count()
                        != rows.len()
                    {
                        changed = true;
                    }
                    self.music_assignments
                        .retain(|row| row.owner_kind != owner_kind);
                    self.music_assignments.extend(rows);
                }
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
                    return Err("Calendar metadata cannot apply recurrence authority fields".into());
                }
            }
        }
        if let Some(values) = &draft.attendees {
            let mut rows = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let existing = self.attendees.iter().find(|row| row.id == value.id);
                rows.push(Attendee {
                    id: existing
                        .map(|row| Ok(row.id.clone()))
                        .unwrap_or_else(|| copied_id(&event.id, "attendee", &value.id))?,
                    event_id: event.id.clone(),
                    name: value.name.clone(),
                    email: value.email.clone(),
                    role: value.role.clone(),
                    status: value.status.clone(),
                    rsvp: i64::from(value.rsvp),
                    sort_order: index as i64,
                    icalendar_component_id: existing
                        .and_then(|row| row.icalendar_component_id.clone()),
                    icalendar_property_index: existing.and_then(|row| row.icalendar_property_index),
                });
            }
            self.attendees.sort_by_key(|row| row.sort_order);
            if self.attendees != rows {
                self.attendees = rows;
                changed = true;
            }
        }
        if let Some(values) = &draft.alarms {
            let mut rows = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let existing = self.alarms.iter().find(|row| row.id == value.id);
                rows.push(Alarm {
                    id: existing
                        .map(|row| Ok(row.id.clone()))
                        .unwrap_or_else(|| copied_id(&event.id, "alarm", &value.id))?,
                    event_id: event.id.clone(),
                    action: value.action.clone(),
                    trigger_type: value.trigger_type.clone(),
                    trigger_value: value.trigger_value.clone(),
                    description: value.description.clone(),
                    sort_order: index as i64,
                    icalendar_component_id: existing
                        .and_then(|row| row.icalendar_component_id.clone()),
                });
            }
            self.alarms.sort_by_key(|row| row.sort_order);
            if self.alarms != rows {
                self.alarms = rows;
                changed = true;
            }
        }
        if let Some(patch) = &draft.pomodoro_config {
            let mut configs = Vec::new();
            let mut counts = Vec::new();
            let mut steps = Vec::new();
            if let CalendarPomodoroConfigPatch::Set(value) = patch {
                let rhythm_kind = match &value.rhythm {
                    CalendarPomodoroRhythm::Count {
                        focus_duration_minutes,
                        short_break_minutes,
                        long_break_minutes,
                        long_break_after_focus_count,
                    } => {
                        counts.push(CountRhythm {
                            event_id: event.id.clone(),
                            focus_duration_minutes: *focus_duration_minutes,
                            short_break_minutes: *short_break_minutes,
                            long_break_minutes: *long_break_minutes,
                            long_break_after_focus_count: *long_break_after_focus_count,
                        });
                        "count"
                    }
                    CalendarPomodoroRhythm::Sequence { steps: values } => {
                        steps = values
                            .iter()
                            .enumerate()
                            .map(|(index, value)| SequenceStep {
                                event_id: event.id.clone(),
                                step_index: index as i64,
                                focus_duration_minutes: value.focus_duration_minutes,
                                break_phase: value.break_phase.clone(),
                                break_duration_minutes: value.break_duration_minutes,
                            })
                            .collect();
                        "sequence"
                    }
                };
                configs.push(FocusConfig {
                    event_id: event.id.clone(),
                    rhythm_kind: rhythm_kind.into(),
                    rhythm_source: value.rhythm_source.clone(),
                    preset_key: value.preset_key.clone(),
                    idle_timeout_minutes: value.idle_timeout_minutes,
                });
            }
            if self.focus_configs != configs
                || self.count_rhythms != counts
                || self.sequence_steps != steps
            {
                self.focus_configs = configs;
                self.count_rhythms = counts;
                self.sequence_steps = steps;
                changed = true;
            }
        }
        Ok(changed)
    }
}
