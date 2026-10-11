import { Temporal } from "@js-temporal/polyfill";
import { toCalendarDate } from "$lib/calendar/db-rows";
import { formatTimeRange, parseCalendarDate } from "$lib/calendar/utils";
import type { NotesDateMentionValue, NotesMentionScopeEvent } from "$lib/notes/types";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";

/** The route above a mention preview's title and the title itself. */
export interface NotesMentionPreviewHeading {
  route: string[];
  title: string | null;
}

/** Label untitled route segments the same way the mention menu does. */
export function notesLabeledRoute(segments: readonly string[], untitled: string): string[] {
  return segments.map((segment) => segment.trim() || untitled);
}

/**
 * Split a page location, which ends with the page's own title, into the route shown above the preview title and
 * that title.
 */
export function notesPageLocationHeading(location: readonly string[], untitled: string): NotesMentionPreviewHeading {
  if (location.length === 0) return { route: [], title: null };
  const labeled = notesLabeledRoute(location, untitled);
  return { route: labeled.slice(0, -1), title: labeled.at(-1) ?? null };
}

/** Wall-clock times of a Calendar event or series, as the Calendar renders them, and whether it repeats. */
export interface NotesCalendarEventPreviewTimes {
  start: string;
  end: string;
  allDay?: boolean;
  recurring: boolean;
}

/** Labels for Calendar event preview times, taken from the Calendar notification catalog. */
export interface NotesCalendarEventPreviewLabels {
  allDay: string;
  allDayOn: (date: string) => string;
  datedTimeRange: (date: string, range: string) => string;
}

/**
 * When a Calendar event happens, in the user's time format. A single event shows its date with the time range or
 * all-day label; a recurring series shows only its daily time range, since its first date says little about the next
 * occurrence. Returns null when the wall-clock start cannot be read.
 */
export function notesCalendarEventPreviewWhen(
  event: NotesCalendarEventPreviewTimes,
  options: { locale: string; timeFormat: CalendarTimeFormat; labels: NotesCalendarEventPreviewLabels },
): string | null {
  const start = parseCalendarDate(event.start);
  if (Number.isNaN(start.getTime())) return null;
  const range = event.allDay ? null : formatTimeRange(event.start, event.end, options.timeFormat);
  if (event.recurring) return range ?? options.labels.allDay;
  const date = previewDateFormat(options.locale).format(start);
  return range === null ? options.labels.allDayOn(date) : options.labels.datedTimeRange(date, range);
}

/**
 * The wall-clock times of a stored mention scope event in `renderZone`, converted the same way the Calendar
 * renders stored rows.
 */
export function notesMentionScopeEventTimes(
  event: Pick<NotesMentionScopeEvent, "start_time" | "end_time" | "all_day" | "recurring">,
  renderZone: string,
): NotesCalendarEventPreviewTimes {
  return {
    start: toCalendarDate(event.start_time, renderZone, event.all_day),
    end: toCalendarDate(event.end_time, renderZone, event.all_day),
    allDay: event.all_day,
    recurring: event.recurring,
  };
}

/** The spelled-out date or date range a date mention refers to, or null when its stored date cannot be read. */
export function notesDateMentionPreviewWhen(value: NotesDateMentionValue, locale: string): string | null {
  try {
    const format = previewDateFormat(locale);
    const start = localDate(Temporal.PlainDate.from(value.start));
    if (!value.end) return format.format(start);
    return format.formatRange(start, localDate(Temporal.PlainDate.from(value.end)));
  } catch {
    return null;
  }
}

function previewDateFormat(locale: string): Intl.DateTimeFormat {
  return new Intl.DateTimeFormat(locale, { weekday: "short", month: "short", day: "numeric", year: "numeric" });
}

function localDate(date: Temporal.PlainDate): Date {
  return new Date(date.year, date.month - 1, date.day);
}
