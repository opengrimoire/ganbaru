//! New source rows use native geometry, timestamps and owned child identities.

use super::super::edit::EventDraft;
use super::super::types::CalendarPomodoroConfigPatch;
use super::Metadata;
use super::rows::Event;
use crate::recurrence::canonical::ResolvedTiming;
use chrono::{DateTime, SecondsFormat};

/// This is the built-in calendar seeded by the schema, also its event default.
const LOCAL_CALENDAR_ID: &str = "local";

impl Metadata {
    pub(in crate::events) fn create(
        id: String,
        draft: &EventDraft,
        timing: ResolvedTiming,
        now_ms: i64,
    ) -> Result<Self, String> {
        if timing.all_day
            && matches!(
                draft.pomodoro_config,
                Some(CalendarPomodoroConfigPatch::Set(_))
            )
        {
            return Err("Floating Calendar events cannot enable Focus".into());
        }
        let now = DateTime::from_timestamp_millis(now_ms)
            .ok_or("Calendar creation clock is outside its instant range")?
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut metadata = Self {
            events: vec![Event {
                id,
                title: String::new(),
                start_time: timing.start_time,
                end_time: timing.end_time,
                timezone: timing.timezone,
                calendar_id: LOCAL_CALENDAR_ID.into(),
                project_id: None,
                color: None,
                description: String::new(),
                rrule: draft.recurrence.for_creation()?,
                repeat_until: None,
                environment_id: None,
                playlist_id: None,
                all_day: i64::from(timing.all_day),
                location: String::new(),
                url: String::new(),
                transparency: "opaque".into(),
                status: "confirmed".into(),
                source_uid: None,
                visibility: "public".into(),
                priority: None,
                geo_lat: None,
                geo_lng: None,
                sequence: 0,
                guest_can_modify: 0,
                guest_can_invite_others: 1,
                guest_can_see_other_guests: 1,
                created_at: now.clone(),
                updated_at: now,
                icalendar_component_id: None,
                local_rsvp_status: None,
                meeting_enabled: 0,
            }],
            ..Self::default()
        };
        metadata.apply_draft(draft, now_ms)?;
        Ok(metadata)
    }
}
