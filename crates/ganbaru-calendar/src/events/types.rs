use serde::{Deserialize, Serialize};

pub(super) struct CalendarEventMutationTarget {
    pub(super) id: String,
}

pub(super) struct CalendarEventMutationContext {
    pub(super) id: String,
    /// Canonical alias can be the bare anchor even for a qualified selection.
    pub(super) canonical_id: String,
    pub(super) source_event_id: String,
    pub(super) occurrence_date: Option<String>,
    pub(super) start_time: String,
    pub(super) end_time: String,
    pub(super) rrule: Option<String>,
    pub(super) repeat_until: Option<String>,
    pub(super) synthetic: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg(test)]
pub struct CalendarEventCreate {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) start_time: String,
    pub(super) end_time: String,
    pub(super) timezone: String,
    pub(super) calendar_id: String,
    pub(super) project_id: Option<String>,
    pub(super) environment_id: Option<String>,
    pub(super) playlist_id: Option<String>,
    pub(super) color: Option<i64>,
    pub(super) description: String,
    pub(super) rrule: Option<String>,
    pub(super) notifications: Option<String>,
    pub(super) exceptions: Option<String>,
    pub(super) repeat_until: Option<String>,
    pub(super) all_day: bool,
    pub(super) location: String,
    pub(super) url: String,
    pub(super) transparency: String,
    pub(super) status: String,
    pub(super) source_uid: Option<String>,
    pub(super) visibility: String,
    pub(super) priority: Option<i64>,
    pub(super) categories: Option<String>,
    pub(super) geo: Option<String>,
    pub(super) sequence: i64,
    pub(super) rdate: Option<String>,
    pub(super) extended_properties: Option<String>,
    pub(super) organizer: Option<String>,
    pub(super) meeting_enabled: bool,
    pub(super) local_rsvp_status: Option<String>,
    pub(super) guest_can_modify: bool,
    pub(super) guest_can_invite_others: bool,
    pub(super) guest_can_see_other_guests: bool,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    pub(super) pomodoro_config: Option<CalendarPomodoroConfig>,
    pub(super) attendees: Vec<CalendarEventAttendee>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CalendarPomodoroConfig {
    pub(super) rhythm: CalendarPomodoroRhythm,
    pub(super) rhythm_source: String,
    pub(super) preset_key: Option<String>,
    pub(super) idle_timeout_minutes: Option<i64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(super) enum CalendarPomodoroRhythm {
    Count {
        focus_duration_minutes: i64,
        short_break_minutes: i64,
        long_break_minutes: i64,
        long_break_after_focus_count: i64,
    },
    Sequence {
        steps: Vec<CalendarPomodoroSequenceStep>,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CalendarPomodoroSequenceStep {
    pub(super) focus_duration_minutes: i64,
    pub(super) break_phase: String,
    pub(super) break_duration_minutes: i64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CalendarEventAttendee {
    pub(super) id: String,
    pub(super) name: Option<String>,
    pub(super) email: String,
    pub(super) role: String,
    pub(super) status: String,
    pub(super) rsvp: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CalendarEventAlarm {
    pub(super) id: String,
    pub(super) action: String,
    pub(super) trigger_type: String,
    pub(super) trigger_value: String,
    pub(super) description: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "field", content = "value")]
pub(super) enum CalendarEventUpdateField {
    #[serde(rename = "title")]
    Title(String),
    #[serde(rename = "startTime")]
    StartTime(String),
    #[serde(rename = "endTime")]
    EndTime(String),
    #[serde(rename = "timezone")]
    Timezone(String),
    #[serde(rename = "calendarId")]
    CalendarId(String),
    #[serde(rename = "projectId")]
    ProjectId(Option<String>),
    #[serde(rename = "environmentId")]
    EnvironmentId(Option<String>),
    #[serde(rename = "playlistId")]
    PlaylistId(Option<String>),
    #[serde(rename = "color")]
    Color(Option<i64>),
    #[serde(rename = "description")]
    Description(String),
    #[serde(rename = "rrule")]
    Rrule(Option<String>),
    #[serde(rename = "repeatUntil")]
    RepeatUntil(Option<String>),
    #[serde(rename = "notifications")]
    Notifications(Option<String>),
    #[serde(rename = "exceptions")]
    Exceptions(Option<String>),
    #[serde(rename = "allDay")]
    AllDay(bool),
    #[serde(rename = "location")]
    Location(String),
    #[serde(rename = "url")]
    Url(String),
    #[serde(rename = "transparency")]
    Transparency(String),
    #[serde(rename = "status")]
    Status(String),
    #[serde(rename = "sourceUid")]
    SourceUid(Option<String>),
    #[serde(rename = "visibility")]
    Visibility(String),
    #[serde(rename = "priority")]
    Priority(Option<i64>),
    #[serde(rename = "categories")]
    Categories(Option<String>),
    #[serde(rename = "geo")]
    Geo(Option<String>),
    #[serde(rename = "sequence")]
    Sequence(i64),
    #[serde(rename = "rdate")]
    Rdate(Option<String>),
    #[serde(rename = "extendedProperties")]
    ExtendedProperties(Option<String>),
    #[serde(rename = "organizer")]
    Organizer(Option<String>),
    #[serde(rename = "meetingEnabled")]
    MeetingEnabled(bool),
    #[serde(rename = "localRsvpStatus")]
    LocalRsvpStatus(Option<String>),
    #[serde(rename = "guestPermissions")]
    GuestPermissions(CalendarGuestPermissions),
    #[serde(rename = "musicSnapshotAssignments")]
    MusicSnapshotAssignments(Vec<ganbaru_music::assignments::MusicContextAssignmentDraft>),
    #[serde(rename = "musicOverrideAssignments")]
    MusicOverrideAssignments(Vec<ganbaru_music::assignments::MusicContextAssignmentDraft>),
}

impl CalendarEventUpdateField {
    pub(super) fn field_name(&self) -> &'static str {
        match self {
            CalendarEventUpdateField::Title(_) => "title",
            CalendarEventUpdateField::StartTime(_) => "startTime",
            CalendarEventUpdateField::EndTime(_) => "endTime",
            CalendarEventUpdateField::Timezone(_) => "timezone",
            CalendarEventUpdateField::CalendarId(_) => "calendarId",
            CalendarEventUpdateField::ProjectId(_) => "projectId",
            CalendarEventUpdateField::EnvironmentId(_) => "environmentId",
            CalendarEventUpdateField::PlaylistId(_) => "playlistId",
            CalendarEventUpdateField::Color(_) => "color",
            CalendarEventUpdateField::Description(_) => "description",
            CalendarEventUpdateField::Rrule(_) => "rrule",
            CalendarEventUpdateField::RepeatUntil(_) => "repeatUntil",
            CalendarEventUpdateField::Notifications(_) => "notifications",
            CalendarEventUpdateField::Exceptions(_) => "exceptions",
            CalendarEventUpdateField::AllDay(_) => "allDay",
            CalendarEventUpdateField::Location(_) => "location",
            CalendarEventUpdateField::Url(_) => "url",
            CalendarEventUpdateField::Transparency(_) => "transparency",
            CalendarEventUpdateField::Status(_) => "status",
            CalendarEventUpdateField::SourceUid(_) => "sourceUid",
            CalendarEventUpdateField::Visibility(_) => "visibility",
            CalendarEventUpdateField::Priority(_) => "priority",
            CalendarEventUpdateField::Categories(_) => "categories",
            CalendarEventUpdateField::Geo(_) => "geo",
            CalendarEventUpdateField::Sequence(_) => "sequence",
            CalendarEventUpdateField::Rdate(_) => "rdate",
            CalendarEventUpdateField::ExtendedProperties(_) => "extendedProperties",
            CalendarEventUpdateField::Organizer(_) => "organizer",
            CalendarEventUpdateField::MeetingEnabled(_) => "meetingEnabled",
            CalendarEventUpdateField::LocalRsvpStatus(_) => "localRsvpStatus",
            CalendarEventUpdateField::GuestPermissions(_) => "guestPermissions",
            CalendarEventUpdateField::MusicSnapshotAssignments(_) => "musicSnapshotAssignments",
            CalendarEventUpdateField::MusicOverrideAssignments(_) => "musicOverrideAssignments",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CalendarGuestPermissions {
    pub(super) guest_can_modify: bool,
    pub(super) guest_can_invite_others: bool,
    pub(super) guest_can_see_other_guests: bool,
}

#[derive(Deserialize)]
pub(super) struct CalendarGeoPayload {
    pub(super) lat: f64,
    pub(super) lng: f64,
}

#[derive(Deserialize)]
pub(super) struct CalendarOrganizerPayload {
    pub(super) name: Option<String>,
    pub(super) email: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "action", content = "value")]
pub(super) enum CalendarPomodoroConfigPatch {
    #[serde(rename = "set")]
    Set(CalendarPomodoroConfig),
    #[serde(rename = "clear")]
    Clear,
}
