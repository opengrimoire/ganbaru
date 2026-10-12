import type { NotesDatabaseDateValue } from "$lib/notes/database/date";
import type { NotesDateMentionFormat, NotesDateMentionRichText, NotesDateMentionValue } from "$lib/notes/types";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";
import { notesDateMentionBoundaryDate } from "./mention-preview";

/** The spelling an edited date mention takes when it has never chosen one. */
export const NOTES_DATE_MENTION_DEFAULT_FORMAT: NotesDateMentionFormat = "full";

/** Localized wording around an edited date mention's dates. */
export interface NotesDateMentionTextLabels {
  remindTitle: (dateLabel: string) => string;
  range: (start: string, end: string) => string;
}

/** What spelling a date mention needs besides its value. */
export interface NotesDateMentionTextOptions {
  locale: string;
  timeFormat: CalendarTimeFormat;
  labels: NotesDateMentionTextLabels;
}

const INTL_DATE_OPTIONS: Record<Exclude<NotesDateMentionFormat, "iso">, Intl.DateTimeFormatOptions> = {
  full: { month: "long", day: "numeric", year: "numeric" },
  short: { month: "short", day: "numeric", year: "numeric" },
  numeric: { month: "numeric", day: "numeric", year: "numeric" },
};

/** The format a date mention is spelled with, falling back to the default for mentions that never chose one. */
export function notesDateMentionFormat(value: NotesDateMentionValue): NotesDateMentionFormat {
  return value.ganbaru_format ?? NOTES_DATE_MENTION_DEFAULT_FORMAT;
}

function padded(value: number, length = 2): string {
  return String(value).padStart(length, "0");
}

/** One boundary in a format, with its time when the boundary has one. Offset times show in the device's zone. */
function boundaryText(boundary: string, format: NotesDateMentionFormat, options: NotesDateMentionTextOptions): string {
  const date = notesDateMentionBoundaryDate(boundary);
  const timed = boundary.includes("T");
  if (format === "iso") {
    const day = `${padded(date.getFullYear(), 4)}-${padded(date.getMonth() + 1)}-${padded(date.getDate())}`;
    return timed ? `${day} ${padded(date.getHours())}:${padded(date.getMinutes())}` : day;
  }
  return new Intl.DateTimeFormat(options.locale, {
    ...INTL_DATE_OPTIONS[format],
    ...(timed ? { hour: "numeric", minute: "2-digit", hour12: options.timeFormat === "12h" } : {}),
  }).format(date);
}

/**
 * The chip text of a date mention spelled in its format: one date or a range, with times when it has them, and the
 * reminder wording when its reminder is on. Throws a RangeError when a boundary is not a readable date.
 */
export function notesDateMentionText(value: NotesDateMentionValue, options: NotesDateMentionTextOptions): string {
  const format = notesDateMentionFormat(value);
  const start = boundaryText(value.start, format, options);
  const dates = value.end ? options.labels.range(start, boundaryText(value.end, format, options)) : start;
  return value.ganbaru_reminder?.enabled ? options.labels.remindTitle(dates) : dates;
}

/** A date mention with a new value and its chip text spelled again from that value. */
export function notesDateMentionWithValue(
  mention: NotesDateMentionRichText,
  value: NotesDateMentionValue,
  options: NotesDateMentionTextOptions,
): NotesDateMentionRichText {
  const date: NotesDateMentionValue = { ...value, ganbaru_format: notesDateMentionFormat(value) };
  return {
    ...mention,
    mention: { type: "date", date },
    plain_text: notesDateMentionText(date, options),
  };
}

/** The editable dates of a mention in the shape the date editor works with. */
export function notesDateMentionEditorValue(value: NotesDateMentionValue): NotesDatabaseDateValue {
  return { start: value.start, end: value.end ?? null, time_zone: value.time_zone ?? null };
}

/**
 * A mention or database date with dates from the editor, keeping its format and reminder. A reminder offset the new
 * dates do not offer, such as minutes before a date that lost its time, falls back to the date itself.
 */
export function notesDateMentionValueWithDates<T extends NotesDateMentionValue>(
  value: T,
  dates: NotesDatabaseDateValue,
): T {
  const next: T = { ...value, start: dates.start, end: dates.end, time_zone: dates.time_zone };
  const offered = notesDateMentionReminderOffsets(next).includes(notesDateMentionReminderMinutes(next));
  return offered ? next : notesDateMentionWithReminderMinutes(next, 0);
}

/** Reminder offsets offered for a mention with a time, in minutes before it. */
export const NOTES_TIMED_REMINDER_OFFSETS: readonly number[] = [0, 5, 10, 15, 30, 60, 120, 1440, 2880, 10080];

/** Reminder offsets offered for a date without a time, in whole days so the reminder keeps its usual hour. */
export const NOTES_DATE_ONLY_REMINDER_OFFSETS: readonly number[] = [0, 1440, 2880, 10080];

/** The reminder offsets a mention offers, which depend on whether its start has a time. */
export function notesDateMentionReminderOffsets(value: NotesDateMentionValue): readonly number[] {
  return value.start.includes("T") ? NOTES_TIMED_REMINDER_OFFSETS : NOTES_DATE_ONLY_REMINDER_OFFSETS;
}

/** How many minutes before its date a mention's reminder fires. */
export function notesDateMentionReminderMinutes(value: NotesDateMentionValue): number {
  return value.ganbaru_reminder?.minutes_before ?? 0;
}

/** A date whose reminder fires a number of minutes before it; a date without a reminder is unchanged. */
export function notesDateMentionWithReminderMinutes<T extends NotesDateMentionValue>(value: T, minutes: number): T {
  const reminder = value.ganbaru_reminder;
  if (!reminder) return value;
  return {
    ...value,
    ganbaru_reminder: minutes > 0 ? { enabled: reminder.enabled, minutes_before: minutes } : { enabled: reminder.enabled },
  };
}
