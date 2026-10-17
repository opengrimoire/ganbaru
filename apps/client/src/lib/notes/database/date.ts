import { NOTES_DATE_MENTION_MAX_REMINDER_MINUTES_BEFORE, type NotesDateMentionReminder } from "$lib/notes/contracts/core";

export interface NotesDatabaseDateValue {
  start: string;
  end: string | null;
  time_zone: string | null;
  /** A reminder before the start, stored like a date mention's; absent when the date has never reminded. */
  ganbaru_reminder?: NotesDateMentionReminder | null;
}

/** A stored reminder with a boolean switch, keeping its offset only when it is a whole number in range. */
function notesDatabaseDateReminder(value: unknown): NotesDateMentionReminder | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const reminder = value as Record<string, unknown>;
  if (typeof reminder.enabled !== "boolean") return null;
  const minutes = reminder.minutes_before;
  const offered = typeof minutes === "number" && Number.isInteger(minutes) && minutes > 0
    && minutes <= NOTES_DATE_MENTION_MAX_REMINDER_MINUTES_BEFORE;
  return offered ? { enabled: reminder.enabled, minutes_before: minutes } : { enabled: reminder.enabled };
}

/** Identify real ISO calendar dates and optional local or offset time components. */
export function notesDatabaseIsDateBoundaryValid(value: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})(?:T(\d{2}):(\d{2})(?::(\d{2})(?:\.\d{1,9})?)?(?:Z|[+-](\d{2}):(\d{2}))?)?$/.exec(value);
  if (!match) return false;
  const [, year, month, day, hour, minute, second, zoneHour, zoneMinute] = match;
  const date = new Date(`${year}-${month}-${day}T00:00:00Z`);
  if (!Number.isFinite(date.getTime()) || date.getUTCFullYear() !== Number(year)
    || date.getUTCMonth() + 1 !== Number(month) || date.getUTCDate() !== Number(day)) return false;
  return (hour === undefined || Number(hour) < 24) && (minute === undefined || Number(minute) < 60)
    && (second === undefined || Number(second) < 60) && (zoneHour === undefined || Number(zoneHour) < 24)
    && (zoneMinute === undefined || Number(zoneMinute) < 60);
}

/** Validate named zones with the same platform timezone data used for display. */
export function notesDatabaseIsTimeZoneValid(value: string): boolean {
  if (!value || value.length > 100 || /[\u0000-\u001f\u007f]/.test(value)) return false;
  try {
    new Intl.DateTimeFormat("en", { timeZone: value }).format(0);
    return true;
  } catch (error: unknown) {
    if (error instanceof RangeError) return false;
    throw error;
  }
}

/** Read canonical date fields without coercing unknown native values. */
export function notesDatabaseDateValue(value: unknown): NotesDatabaseDateValue | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const date = value as Record<string, unknown>;
  if (typeof date.start !== "string" || !date.start) return null;
  const reminder = notesDatabaseDateReminder(date.ganbaru_reminder);
  return {
    start: date.start,
    end: typeof date.end === "string" && date.end ? date.end : null,
    time_zone: typeof date.time_zone === "string" && date.time_zone ? date.time_zone : null,
    ...(reminder ? { ganbaru_reminder: reminder } : {}),
  };
}

/** Compare boundaries on their ISO timeline, retaining wall time for dates without an offset. */
export function notesDatabaseDateBoundaryOrder(value: string): number {
  return Date.parse(/T/.test(value) ? /(?:Z|[+-]\d{2}:\d{2})$/.test(value) ? value : `${value}Z` : `${value}T00:00:00Z`);
}

/** The boundary of a date range that an editor changes. */
export type NotesDatabaseDateBoundaryKey = "start" | "end";

const BOUNDARY_PARTS = /^(\d{4}-\d{2}-\d{2})(?:T(\d{2}:\d{2})(?::\d{2}(?:\.\d{1,9})?)?(Z|[+-]\d{2}:\d{2})?)?$/;

/** A date boundary split into what a date picker and a time picker edit. */
export interface NotesDatabaseDateBoundaryParts {
  /** The calendar date as `YYYY-MM-DD`. */
  date: string;
  /** The wall time as `HH:MM`, or null for a whole day. */
  time: string | null;
  /** The UTC designator or offset after the time, kept when the date or time changes. */
  offset: string;
}

/** Split a canonical boundary, or return null for text that is not one. */
export function notesDatabaseDateBoundaryParts(boundary: string): NotesDatabaseDateBoundaryParts | null {
  const match = BOUNDARY_PARTS.exec(boundary);
  if (!match) return null;
  return { date: match[1], time: match[2] ?? null, offset: match[3] ?? "" };
}

/** Whether either boundary carries a time of day. */
export function notesDatabaseDateHasTime(value: NotesDatabaseDateValue | null): boolean {
  if (!value) return false;
  return [value.start, value.end].some((boundary) => boundary !== null && notesDatabaseDateBoundaryParts(boundary)?.time != null);
}

/** Put the earlier boundary first, so a range picked backwards still saves. */
export function notesDatabaseDateOrdered(value: NotesDatabaseDateValue): NotesDatabaseDateValue {
  if (!value.end || notesDatabaseDateBoundaryOrder(value.end) >= notesDatabaseDateBoundaryOrder(value.start)) return value;
  return { ...value, start: value.end, end: value.start };
}

/** Replace the calendar date of a boundary, keeping its time and offset. */
function boundaryWithDate(boundary: string, date: string): string {
  return notesDatabaseDateBoundaryParts(boundary) ? `${date}${boundary.slice(10)}` : date;
}

/** Set or remove the time of a boundary; a whole day drops its offset, which only qualifies a time. */
function boundaryWithTime(boundary: string, time: string | null): string {
  const parts = notesDatabaseDateBoundaryParts(boundary);
  const date = parts?.date ?? boundary.slice(0, 10);
  return time === null ? date : `${date}T${time}${parts?.offset ?? ""}`;
}

/**
 * Set the calendar date of one boundary. Without a value this starts a single date, and the end of a single date
 * starts from the start.
 */
export function notesDatabaseDateWithDay(value: NotesDatabaseDateValue | null, key: NotesDatabaseDateBoundaryKey, date: string): NotesDatabaseDateValue {
  if (!value) return { start: date, end: null, time_zone: null };
  const next: NotesDatabaseDateValue = { ...value };
  next[key] = boundaryWithDate(value[key] ?? value.start, date);
  return notesDatabaseDateOrdered(next);
}

/**
 * Set the time of one boundary. The other boundary of a range takes the same time when it has none, so a range
 * never mixes a whole day with a time.
 */
export function notesDatabaseDateWithTime(value: NotesDatabaseDateValue, key: NotesDatabaseDateBoundaryKey, time: string): NotesDatabaseDateValue {
  const otherKey: NotesDatabaseDateBoundaryKey = key === "start" ? "end" : "start";
  const other = value[otherKey];
  const next: NotesDatabaseDateValue = { ...value };
  next[key] = boundaryWithTime(value[key] ?? value.start, time);
  if (other !== null && notesDatabaseDateBoundaryParts(other)?.time == null) next[otherKey] = boundaryWithTime(other, time);
  return notesDatabaseDateOrdered(next);
}

/** Remove the time from both boundaries, together with the time zone that only applies to times. */
export function notesDatabaseDateWithoutTime(value: NotesDatabaseDateValue): NotesDatabaseDateValue {
  return notesDatabaseDateOrdered({
    start: boundaryWithTime(value.start, null),
    end: value.end === null ? null : boundaryWithTime(value.end, null),
    time_zone: null,
  });
}

/** Turn a single date into a range ending on the same boundary, or a range back into its start. */
export function notesDatabaseDateWithEnd(value: NotesDatabaseDateValue, enabled: boolean): NotesDatabaseDateValue {
  return { ...value, end: enabled ? value.end ?? value.start : null };
}

/** Set the named time zone of a value, clearing it for blank text. */
export function notesDatabaseDateWithTimeZone(value: NotesDatabaseDateValue, timeZone: string): NotesDatabaseDateValue {
  return { ...value, time_zone: timeZone.trim() || null };
}
