import type { IcalendarPreservationStatus } from "$lib/calendar/types";
import type { CalendarFullEventRows, CalendarIcalendarExportMetadata, DbFullEvent, DbFullOverride } from "./event-hydration";
import type { DbAlarm, DbAttendee } from "$lib/calendar/db-rows";

interface CalendarExportHeader {
  id: string;
  name: string;
  color: string;
  source: string;
  source_url: string | null;
}

export interface CalendarExportSnapshot {
  calendar: CalendarExportHeader;
  events: (CalendarFullEventRows & { event: DbFullEvent })[];
  timezones: unknown[][];
  passthrough_components: unknown[][];
  metadata: CalendarIcalendarExportMetadata;
}

type Validator<T> = (value: unknown) => value is T;
type Schema<T> = { [K in keyof T]-?: Validator<T[K]> };

const text: Validator<string> = (value) => typeof value === "string";
const number: Validator<number> = (value): value is number => typeof value === "number" && Number.isFinite(value);
const boolean: Validator<boolean> = (value) => typeof value === "boolean";
const nullable = <T>(check: Validator<T>): Validator<T | null> => (value): value is T | null => value === null || check(value);
const optional = <T>(check: Validator<T>): Validator<T | undefined> => (value): value is T | undefined => value === undefined || check(value);
const array = <T>(check: Validator<T>): Validator<T[]> => (value): value is T[] => Array.isArray(value) && value.every(check);

/** Build a validator that checks every schema field of a plain object. */
function record<T>(schema: Schema<T>): Validator<T> {
  return (value): value is T => {
    if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
    const entries: Record<string, unknown> = Object.fromEntries(Object.entries(value));
    return (Object.keys(schema) as (keyof T)[]).every((key) => schema[key](entries[String(key)]));
  };
}

const nullableText = nullable(text);
const nullableNumber = nullable(number);
const preservationStatus: Validator<IcalendarPreservationStatus> = (value) => value === "lossless"
  || value === "partial" || value === "unsupported" || value === "needs-review"
  || value === "regenerated" || value === "invalid";

const fullEvent = record<DbFullEvent>({
  id: text, title: text, start_time: text, end_time: text, timezone: text, calendar_id: text,
  project_id: nullableText, environment_id: nullableText, playlist_id: nullableText,
  color: nullableNumber, rrule: nullableText, notifications: nullableText, exceptions: nullableText,
  repeat_until: nullableText, all_day: number, location: text, has_call_link: optional(number),
  meeting_enabled: number, transparency: text, status: text, local_rsvp_status: nullableText,
  created_at: text, rdate: nullableText, rhythm_kind: nullableText, rhythm_source: nullableText,
  preset_key: nullableText, count_focus_duration_minutes: nullableNumber,
  count_short_break_minutes: nullableNumber, count_long_break_minutes: nullableNumber,
  count_long_break_after_focus_count: nullableNumber, sequence_steps: nullableText,
  idle_timeout_minutes: nullableNumber, description: nullableText, url: nullableText,
  source_uid: nullableText, visibility: text, priority: nullableNumber, categories: nullableText,
  geo: nullableText, sequence: number, extended_properties: nullableText, organizer: nullableText,
  guest_can_modify: number, guest_can_invite_others: number, guest_can_see_other_guests: number,
  icalendar_component_id: nullableText, icalendar_preservation_status: nullable(preservationStatus),
  icalendar_projection_warnings: nullableText, icalendar_raw_jcal: nullableText,
});
const attendee = record<DbAttendee>({
  id: text, event_id: text, icalendar_component_id: nullableText, icalendar_property_index: nullableNumber,
  name: nullableText, email: text, role: text, status: text, rsvp: number, sort_order: number,
});
const alarm = record<DbAlarm>({
  id: text, event_id: text, icalendar_component_id: nullableText, action: text, trigger_type: text,
  trigger_value: text, description: nullableText, sort_order: number,
});
const override = record<DbFullOverride>({
  id: text, parent_event_id: text, recurrence_id: text, recurrence_range: nullableText,
  title: nullableText, start_time: nullableText, end_time: nullableText, color: nullableNumber,
  status: nullableText, transparency: nullableText, description: nullableText, location: nullableText,
  url: nullableText, visibility: nullableText, extended_properties: nullableText,
  icalendar_component_id: nullableText, icalendar_raw_jcal: nullableText,
});

/** Preserved values remain inert JSON; the iCalendar codec owns their semantics. */
function json(value: unknown, depth = 0): boolean {
  if (depth > 96) return false;
  if (value === null || text(value) || boolean(value) || number(value)) return true;
  if (Array.isArray(value)) return value.every((child) => json(child, depth + 1));
  return typeof value === "object" && Object.values(value).every((child) => json(child, depth + 1));
}

const component: Validator<unknown[]> = (value): value is unknown[] => Array.isArray(value)
  && value.length === 3 && text(value[0]) && Array.isArray(value[1]) && Array.isArray(value[2]) && json(value);

const snapshot = record<CalendarExportSnapshot>({
  calendar: record<CalendarExportHeader>({ id: text, name: text, color: text, source: text, source_url: nullableText }),
  events: array(record<CalendarFullEventRows & { event: DbFullEvent }>({
    event: fullEvent, attendees: array(attendee), alarms: array(alarm), overrides: array(override),
  })),
  timezones: array(component), passthrough_components: array(component),
  metadata: record<CalendarIcalendarExportMetadata>({ method: nullableText, mixed_methods: boolean }),
});

/** Reject incomplete or cross-calendar snapshots before invoking the serializer. */
export function parseCalendarExportSnapshot(value: unknown, calendarId: string): CalendarExportSnapshot {
  if (!snapshot(value) || value.calendar.id !== calendarId) {
    throw new Error("Invalid Calendar export snapshot");
  }
  const ids = new Set<string>();
  for (const rows of value.events) {
    const id = rows.event.id;
    if (rows.event.calendar_id !== calendarId || ids.has(id)
      || rows.attendees.some((row) => row.event_id !== id)
      || rows.alarms.some((row) => row.event_id !== id)
      || rows.overrides.some((row) => row.parent_event_id !== id)) {
      throw new Error("Invalid Calendar export row ownership");
    }
    ids.add(id);
  }
  return value;
}
