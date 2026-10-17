import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";

export const CALENDAR_VIEW_MODES = ["day", "workweek", "week", "month"] as const;
export type CalendarViewMode = (typeof CALENDAR_VIEW_MODES)[number];
export const DEFAULT_CALENDAR_VIEW_MODE: CalendarViewMode = "week";

export function isCalendarViewMode(value: unknown): value is CalendarViewMode {
  return typeof value === "string"
    && CALENDAR_VIEW_MODES.includes(value as CalendarViewMode);
}

/**
 * Index into a theme's eventPalette array. Valid range is 0..PALETTE_SIZE-1
 * (currently 0..31). Stored as INTEGER on calendar_events.color so palette
 * order is intrinsic and storage stays compact. The render layer falls back
 * to the FALLBACK_COLOR_INDEX slot for unknown values.
 */
export type EventColor = number;

/** Number of slots every theme palette must provide. */
export const PALETTE_SIZE = 32;

/** Index of the render-layer fallback slot. Every theme must fill it. */
export const FALLBACK_COLOR_INDEX = 30;

export type RecurrenceFrequency = "daily" | "weekly" | "monthly" | "yearly";

export type Weekday = "MO" | "TU" | "WE" | "TH" | "FR" | "SA" | "SU";

export type RecurrenceEnd =
  | { type: "never" }
  | { type: "until"; date: string }
  | { type: "count"; count: number };

export interface OrdinalWeekday {
  day: Weekday;
  /** e.g. 2 = "2nd", -1 = "last". Omit for simple weekday membership. */
  ordinal?: number;
}

export interface RecurrenceConfig {
  frequency: RecurrenceFrequency;
  interval: number;
  /** Simple BYDAY weekdays for weekly recurrence. */
  weekdays?: Weekday[];
  /** BYDAY with ordinal prefixes (2TU, -1FR) for monthly/yearly. */
  ordinalWeekdays?: OrdinalWeekday[];
  /** BYMONTHDAY: day-of-month numbers (1-31, negative for end-of-month). */
  byMonthDay?: number[];
  /** BYMONTH: month numbers (1-12). */
  byMonth?: number[];
  /** BYSETPOS: position indices into the expanded candidate set. */
  bySetPos?: number[];
  /** BYYEARDAY: day-of-year numbers (1-366). */
  byYearDay?: number[];
  /** BYWEEKNO: ISO week numbers (1-53). */
  byWeekNo?: number[];
  /** WKST: which day starts the week (default MO per RFC 5545). */
  wkst?: Weekday;
  end: RecurrenceEnd;
}

export type RecurringScope = "this" | "following" | "all";

export type { PomodoroConfig } from "$lib/pomodoro/rhythm";

export type EventTransparency = "opaque" | "transparent";
export type EventStatus = "confirmed" | "tentative" | "cancelled";
export type EventSurfaceStatus = EventStatus | AttendeeStatus;
export type EventVisibility = "public" | "private";
export type IcalendarPreservationStatus =
  | "lossless"
  | "partial"
  | "unsupported"
  | "needs-review"
  | "regenerated"
  | "invalid";

// Attendee types (RFC 5545 ATTENDEE/ORGANIZER)

export type AttendeeRole = "chair" | "req-participant" | "opt-participant" | "non-participant";
export type AttendeeStatus = "needs-action" | "accepted" | "declined" | "tentative" | "delegated";

export interface EventAttendee {
  id: string;
  name?: string;
  email: string;
  role: AttendeeRole;
  status: AttendeeStatus;
  rsvp: boolean;
  /** Preserved VEVENT component that contains this ATTENDEE property. */
  icalendarComponentId?: string;
  /** Zero-based property index inside the preserved VEVENT component. */
  icalendarPropertyIndex?: number;
}

export interface EventSurfaceAttendee {
  email: string;
  status: AttendeeStatus;
}

export interface EventOrganizer {
  name?: string;
  email: string;
}

// Alarm types (RFC 5545 VALARM)

export type AlarmAction = "display" | "audio" | "email";

export interface EventAlarm {
  id: string;
  action: AlarmAction;
  triggerType: "relative" | "absolute";
  /** Duration string for relative ("-PT15M") or ISO datetime for absolute. */
  triggerValue: string;
  description?: string;
  /** Preserved VALARM component that produced this alarm. */
  icalendarComponentId?: string;
}

// Geo coordinates (RFC 5545 GEO)

export interface GeoCoordinates {
  lat: number;
  lng: number;
}

// Per-instance override (RFC 5545 RECURRENCE-ID)

export interface EventOverride {
  id: string;
  parentEventId: string;
  /** Original DTSTART of the overridden instance as a UTC ISO 8601 instant. */
  recurrenceId: string;
  /** RFC 5545 RECURRENCE-ID RANGE parameter. */
  recurrenceRange?: "this-and-future";
  title?: string;
  start?: string;
  end?: string;
  description?: string;
  location?: string;
  url?: string;
  color?: EventColor;
  status?: EventStatus;
  transparency?: EventTransparency;
  visibility?: EventVisibility;
  extendedProperties?: Record<string, string>;
  /** Preserved override VEVENT component that produced this row. */
  icalendarComponentId?: string;
  /** Structured preserved VEVENT component JSON for export merging. */
  icalendarRawJcal?: unknown;
}

// Guest permissions (Google Calendar X-properties)

export interface GuestPermissions {
  /** Whether guests can modify the event (X-GOOGLE-GUEST-CAN-MODIFY). */
  canModify: boolean;
  /** Whether guests can invite others (X-GOOGLE-GUEST-CAN-INVITE-OTHERS). */
  canInviteOthers: boolean;
  /** Whether guests can see the attendee list (X-GOOGLE-GUEST-CAN-SEE-OTHER-GUESTS). */
  canSeeOtherGuests: boolean;
}

// Main event interface

export interface CalendarEvent {
  id: string;
  title: string;
  /**
   * Wall clock "YYYY-MM-DD HH:MM" or "YYYY-MM-DD HH:MM:SS" in the current
   * render zone (device zone by default). The DB source of truth is
   * `start_time` as a UTC ISO 8601 instant. `mapRow` converts UTC to
   * render-zone wall clock at load time; round-trip on save uses the event's
   * home zone (`timezone`) to convert back to UTC.
   */
  start: string;
  /**
   * Wall clock "YYYY-MM-DD HH:MM" or "YYYY-MM-DD HH:MM:SS" in the current
   * render zone. See `start`.
   */
  end: string;
  /** IANA zone (e.g. "America/New_York") used for recurrence math and ICS re-export. */
  timezone: string;
  calendarId: string;
  projectId?: string;
  environmentId?: string;
  playlistId?: string;
  musicSnapshotAssignments?: MusicContextAssignmentDraft[];
  musicOverrideAssignments?: MusicContextAssignmentDraft[];
  color?: EventColor;
  description?: string;
  recurrence?: RecurrenceConfig;
  /** Array of notification times in minutes before the event start. */
  notifications?: number[];
  pomodoroConfig?: PomodoroConfig;
  /** Dates excluded from recurrence expansion (YYYY-MM-DD). */
  exceptions?: string[];
  /** Set on virtual recurring instances; points to the DB-backed template event. */
  recurringParentId?: string;
  /** Native home-zone recurrence identity, independent of moved or rendered start dates. */
  recurrenceDate?: string;
  /** Canonical timed occurrence instants supplied by native reads, including fold disambiguation. */
  startInstant?: string;
  endInstant?: string;
  allDay?: boolean;
  /** Keeps the Meeting section enabled even when every meeting field is empty. */
  meetingEnabled?: boolean;
  /** Render-only marker for a stored call link when the URL is not loaded. */
  hasCallLink?: boolean;
  location?: string;
  url?: string;
  transparency?: EventTransparency;
  status?: EventStatus;
  /** Frontend-only visual status. Used for meeting RSVP patterns without changing event STATUS. */
  surfaceStatus?: EventSurfaceStatus;
  /** Slim render-only attendee RSVP data for deriving `surfaceStatus` in calendar views. */
  surfaceAttendees?: EventSurfaceAttendee[];
  /** App-local RSVP for the placeholder user row before an email identity exists. */
  localParticipationStatus?: AttendeeStatus;
  /** Original UID from imported .ics file, used for deduplication. */
  sourceUid?: string;
  /** RFC 5545 CLASS: controls visibility to other users. */
  visibility?: EventVisibility;
  /** RFC 5545 PRIORITY: 0 (undefined) to 9 (lowest). 1 = highest. */
  priority?: number;
  /** RFC 5545 CATEGORIES: tags/labels for the event. */
  categories?: string[];
  /** RFC 5545 GEO: latitude and longitude. */
  geo?: GeoCoordinates;
  /** RFC 5545 SEQUENCE: revision counter for change tracking. */
  sequence?: number;
  /** Database creation timestamp. Used for deterministic scheduler tiebreakers. */
  createdAt?: string;
  /** Frontend-only event panel metadata for project task links. */
  linkedTaskIds?: string[];
  /** RFC 5545 RDATE: additional recurrence dates beyond the RRULE pattern. */
  rdate?: string[];
  /** Arbitrary extended properties (X-* from iCalendar). */
  extendedProperties?: Record<string, string>;
  /** RFC 5545 ORGANIZER: who created/owns the event. */
  organizer?: EventOrganizer;
  /** RFC 5545 ATTENDEE: participants and their RSVP status. */
  attendees?: EventAttendee[];
  /** RFC 5545 VALARM: rich alarm definitions (from imports). */
  alarms?: EventAlarm[];
  /** Per-instance overrides for recurring events (RECURRENCE-ID). */
  overrides?: EventOverride[];
  /** Guest permission flags (Google Calendar X-properties). */
  guestPermissions?: GuestPermissions;
  /** Preserved VEVENT component that produced this row. */
  icalendarComponentId?: string;
  /** Preservation quality for the linked VEVENT component. */
  icalendarPreservationStatus?: IcalendarPreservationStatus;
  /** Lossy projection diagnostics for the linked VEVENT component. */
  icalendarProjectionWarnings?: string[];
  /** Structured preserved VEVENT component JSON for export merging. */
  icalendarRawJcal?: unknown;
}

export interface Calendar {
  id: string;
  name: string;
  color: string;
  source: string;
  visible: boolean;
  readOnly: boolean;
  sourceUrl?: string;
  lastSynced?: string;
  createdAt?: string;
  updatedAt?: string;
}

export interface PositionedAllDayEvent {
  event: CalendarEvent;
  row: number;
  startCol: number;
  spanCols: number;
}

export interface AllDayDragState {
  eventId: string;
  type: "move" | "resize-start" | "resize-end";
  originStartCol: number;
  originSpanCols: number;
  originRow: number;
  pointerStartX: number;
  pointerStartY: number;
  columnBounds: DOMRect[];
}

export interface PositionedEvent {
  event: CalendarEvent;
  startMinute: number; // minutes since midnight
  durationMinutes: number; // length in minutes (clamped to MIN_EVENT_HEIGHT equivalent)
  left: number; // percentage 0-100
  width: number; // percentage 0-100
  column: number;
  totalColumns: number;
  isClippedTop?: boolean; // event continues from previous day
  isClippedBottom?: boolean; // event continues into next day
  hasEventBelow?: boolean; // another event starts right when this one ends
}

// Pomodoro segment types

export type SegmentPhase = "focus" | "short_break" | "long_break";

export type SegmentStatus = "planned" | "active" | "completed" | "skipped" | "interrupted";

export interface PlannedSegment {
  rhythmPosition: number;
  phase: SegmentPhase;
  startOffsetMinutes: number;
  endOffsetMinutes: number;
}

export type PauseReason = "idle" | "manual" | "suspend";

export interface PauseInterval {
  startedAt: string;
  endedAt: string | null;
  reason: PauseReason;
}

export interface PersistedSegment {
  id: string;
  eventId: string;
  eventDate: string;
  runId: string;
  rhythmPosition: number;
  phase: SegmentPhase;
  plannedStart: string;
  plannedEnd: string;
  actualStart: string | null;
  actualEnd: string | null;
  pauseLog: PauseInterval[];
  status: SegmentStatus;
}

export interface TimelineBand {
  topMinute: number; // minute-of-day (0-1440)
  heightMinutes: number; // duration in minutes
  phase: SegmentPhase;
  status: SegmentStatus;
}

export interface DragState {
  eventId: string;
  type: "move" | "resize-top" | "resize-bottom";
  originDate: string; // event's original start date "YYYY-MM-DD"
  startColumnDate: string; // column date where drag started "YYYY-MM-DD"
  originStartMinute: number;
  originEndMinute: number; // offset from originDate midnight, can be >1440
  pointerStartY: number;
  pointerStartX: number;
  scrollTopAtStart: number; // scroll container scrollTop when drag began
  columnWidth: number;
  startColumnIndex: number;
}
import type { PomodoroConfig } from "$lib/pomodoro/rhythm";
