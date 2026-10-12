import { invoke } from "@tauri-apps/api/core";
import type { Temporal } from "@js-temporal/polyfill";
import { ensureDbUrl } from "$lib/api/db";
import type { CalendarEvent } from "$lib/calendar/types";
import { mapWindowRows, type CalendarWindowRows } from "./event-hydration";
import { normalizeEventColor } from "$lib/calendar/utils";
import { toCalendarDate, type DbCalendarEvent, type DbOverride, type DbWindowAttendee } from "$lib/calendar/db-rows";

interface NativeOccurrence {
  template_id: string;
  id: string;
  recurring_parent_id: string | null;
  recurrence_date: string;
  start_time: string;
  end_time: string;
  override_id: string | null;
}

export interface CalendarExpansionDiagnostic {
  event_id: string;
  message: string;
}

interface NativeCalendarWindow extends CalendarWindowRows {
  occurrences: NativeOccurrence[];
  diagnostics: CalendarExpansionDiagnostic[];
}

export interface NativeCalendarWindowRequest {
  windowStartDate: string;
  windowEndDate: string;
  renderZone: string;
  includeTotalEventCount: boolean;
}

export interface MappedNativeCalendarWindow {
  sourceEvents: CalendarEvent[];
  windowEvents: CalendarEvent[];
  totalEventCount: number | null;
  diagnostics: CalendarExpansionDiagnostic[];
}

type Validator<T> = (value: unknown) => value is T;
type Schema<T> = { [K in keyof T]-?: Validator<T[K]> };

const MAX_RECORDS = 500_000;
const MAX_TEXT_BYTES = 64 * 1024 * 1024;

/** Filter already expanded native occurrences without recomputing recurrence or changing identity. */
export function nativeCalendarEventsInWindow(
  events: readonly CalendarEvent[],
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
): CalendarEvent[] {
  const start = windowStart.toString();
  const endExclusive = windowEnd.add({ days: 1 }).toString();
  return events.filter((event) => event.start.slice(0, 10) < endExclusive
    && event.end.slice(0, 10) >= start);
}

/** Validate every consumed field and all native occurrence/child ownership references. */
export function parseNativeCalendarWindow(value: unknown): NativeCalendarWindow {
  let textBytes = 0;
  let records = 0;
  const text: Validator<string> = (value): value is string => {
    if (typeof value !== "string") return false;
    textBytes += value.length * 3;
    return textBytes <= MAX_TEXT_BYTES;
  };
  const number: Validator<number> = (value): value is number => typeof value === "number" && Number.isSafeInteger(value);
  const flag: Validator<number> = (value): value is number => value === 0 || value === 1;
  const nullable = <T>(check: Validator<T>): Validator<T | null> => (value): value is T | null => value === null || check(value);
  const optional = <T>(check: Validator<T>): Validator<T | undefined> => (value): value is T | undefined => value === undefined || check(value);
  const nullableText = nullable(text);
  const nullableNumber = nullable(number);
  const status: Validator<string> = (value): value is string => value === "confirmed" || value === "tentative" || value === "cancelled";
  const transparency: Validator<string> = (value): value is string => value === "opaque" || value === "transparent";
  const date: Validator<string> = (value): value is string => text(value) && /^\d{4}-\d{2}-\d{2}$/u.test(value)
    && Number.isFinite(Date.parse(`${value}T00:00:00Z`)) && new Date(`${value}T00:00:00Z`).toISOString().slice(0, 10) === value;
  const instant: Validator<string> = (value): value is string => text(value) && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/u.test(value)
    && Number.isFinite(Date.parse(value)) && new Date(value).toISOString() === value;
  function record<T>(schema: Schema<T>): Validator<T> {
    return (value): value is T => {
      if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
      records += 1;
      if (records > MAX_RECORDS) return false;
      const row = value as Record<string, unknown>;
      return (Object.keys(schema) as (keyof T)[]).every((key) => schema[key](row[String(key)]));
    };
  }
  const array = <T>(check: Validator<T>): Validator<T[]> => (value): value is T[] => Array.isArray(value)
    && value.length <= MAX_RECORDS && value.every(check);
  const event = record<DbCalendarEvent>({
    id: text, title: text, start_time: text, end_time: text, timezone: text, calendar_id: text,
    project_id: nullableText, environment_id: nullableText, playlist_id: nullableText, color: nullableNumber,
    rrule: nullableText, notifications: nullableText, exceptions: nullableText, repeat_until: nullableText,
    all_day: flag, location: text, has_call_link: optional(flag), meeting_enabled: flag, transparency, status,
    local_rsvp_status: nullableText, created_at: text, rdate: nullableText, rhythm_kind: nullableText,
    rhythm_source: nullableText, preset_key: nullableText, count_focus_duration_minutes: nullableNumber,
    count_short_break_minutes: nullableNumber, count_long_break_minutes: nullableNumber,
    count_long_break_after_focus_count: nullableNumber, sequence_steps: nullableText, idle_timeout_minutes: nullableNumber,
  });
  const override = record<DbOverride>({
    id: text, parent_event_id: text, recurrence_id: text, recurrence_range: nullableText,
    title: nullableText, start_time: nullableText, end_time: nullableText, color: nullableNumber,
    status: nullable(status), transparency: nullable(transparency),
  });
  const payload = record<NativeCalendarWindow>({
    events: array(event), overrides: array(override),
    attendees: array(record<DbWindowAttendee>({ event_id: text, email: text, status: text })),
    total_event_count: nullable(number),
    occurrences: array(record<NativeOccurrence>({
      template_id: text, id: text, recurring_parent_id: nullableText, recurrence_date: date,
      start_time: instant, end_time: instant, override_id: nullableText,
    })),
    diagnostics: array(record<CalendarExpansionDiagnostic>({ event_id: text, message: text })),
  });
  if (!payload(value)) throw new Error("Invalid native Calendar window");
  const events = new Map(value.events.map((event) => [event.id, event]));
  const overrides = new Map(value.overrides.map((override) => [override.id, override]));
  const ids = new Set<string>();
  if (events.size !== value.events.length || overrides.size !== value.overrides.length
    || value.events.length > 10_000 || value.occurrences.length > 10_000
    || (value.total_event_count !== null && value.total_event_count < value.events.length)
    || value.attendees.some((row) => !events.has(row.event_id))
    || value.overrides.some((row) => !events.has(row.parent_event_id))
    || value.diagnostics.some((row) => !events.has(row.event_id))) {
    throw new Error("Invalid native Calendar row ownership or bounds");
  }
  for (const occurrence of value.occurrences) {
    const event = events.get(occurrence.template_id);
    const override = occurrence.override_id === null ? undefined : overrides.get(occurrence.override_id);
    if (!event || ids.has(occurrence.id)
      || (occurrence.recurring_parent_id !== null && occurrence.recurring_parent_id !== occurrence.template_id)
      || (occurrence.recurring_parent_id === null && occurrence.id !== occurrence.template_id)
      || (occurrence.override_id !== null && override?.parent_event_id !== occurrence.template_id)
      || Date.parse(occurrence.end_time) < Date.parse(occurrence.start_time)
      || (event.all_day === 0 && occurrence.end_time === occurrence.start_time)) {
      throw new Error("Invalid native Calendar occurrence provenance or range");
    }
    ids.add(occurrence.id);
  }
  return value;
}

/** Map canonical instants to render labels, applying only the override selected by native expansion. */
export function mapNativeCalendarWindow(value: unknown, renderZone: string): MappedNativeCalendarWindow {
  const rows = parseNativeCalendarWindow(value);
  const sourceEvents = mapWindowRows(rows, renderZone);
  const templates = new Map(sourceEvents.map((event) => [event.id, event]));
  const overrides = new Map(rows.overrides.map((override) => [override.id, override]));
  const windowEvents = rows.occurrences.map((occurrence): CalendarEvent => {
    const template = templates.get(occurrence.template_id);
    if (!template) throw new Error("Native Calendar occurrence template is missing");
    const event: CalendarEvent = {
      ...template,
      id: occurrence.id,
      start: toCalendarDate(occurrence.start_time, renderZone, template.allDay),
      end: toCalendarDate(occurrence.end_time, renderZone, template.allDay),
      recurrenceDate: occurrence.recurrence_date,
    };
    if (!template.allDay) {
      event.startInstant = occurrence.start_time;
      event.endInstant = occurrence.end_time;
    }
    if (occurrence.recurring_parent_id) event.recurringParentId = occurrence.recurring_parent_id;
    if (occurrence.override_id) {
      const override = overrides.get(occurrence.override_id);
      if (!override) throw new Error("Native Calendar selected override is missing");
      if (override.title !== null) event.title = override.title;
      const color = normalizeEventColor(override.color);
      if (color !== undefined) event.color = color;
      if (override.status === "cancelled" || override.status === "tentative" || override.status === "confirmed") event.status = override.status;
      if (override.transparency === "opaque" || override.transparency === "transparent") event.transparency = override.transparency;
    }
    return event;
  });
  return { sourceEvents, windowEvents, totalEventCount: rows.total_event_count, diagnostics: rows.diagnostics };
}

/** Fetch canonical source rows and occurrences without sending a frontend recurrence projection back to Rust. */
export async function loadNativeCalendarWindow(
  request: NativeCalendarWindowRequest,
  purpose: "render" | "focus" | "notifications" = "render",
): Promise<MappedNativeCalendarWindow> {
  const command = purpose === "focus" ? "calendar_load_native_focus_window"
    : purpose === "notifications" ? "calendar_load_native_notification_window" : "calendar_load_native_window";
  const value = await invoke<unknown>(command, {
    dbUrl: await ensureDbUrl(), request,
  });
  return mapNativeCalendarWindow(value, request.renderZone);
}
