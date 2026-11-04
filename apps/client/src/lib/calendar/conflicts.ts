import { Temporal } from "@js-temporal/polyfill";
import type {
  CalendarConflictField,
  CalendarFieldConflict,
  CalendarTimingValue,
} from "$lib/api/calendar-conflicts";
import { formatDateTime } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";
import type { Translate } from "$lib/i18n/translator.svelte";
import { formatRecurrenceLabel, rruleToRecurrence } from "./rrule";

/** Characters of a text version shown before it is cut, so a long description does not fill the dialog. */
export const CALENDAR_CONFLICT_PREVIEW_CHARS = 240;

/** Order the fields are listed in a dialog. */
const FIELD_ORDER: readonly CalendarConflictField[] = ["title", "timing", "description"];

/** The open conflict on one field of an event, or undefined when the field has none. */
export function findEventFieldConflict(
  conflicts: readonly CalendarFieldConflict[],
  eventId: string,
  field: CalendarConflictField,
): CalendarFieldConflict | undefined {
  return conflicts.find((conflict) =>
    conflict.eventId === eventId && conflict.field === field && conflict.versions.length > 0);
}

/** The open conflicts of one event, in dialog order. */
export function eventConflicts(
  conflicts: readonly CalendarFieldConflict[],
  eventId: string,
): CalendarFieldConflict[] {
  return conflicts
    .filter((conflict) => conflict.eventId === eventId && conflict.versions.length > 0)
    .sort((a, b) => FIELD_ORDER.indexOf(a.field) - FIELD_ORDER.indexOf(b.field));
}

/** Ids of the events that have at least one open conflict. */
export function conflictedEventIds(conflicts: readonly CalendarFieldConflict[]): Set<string> {
  return new Set(conflicts.filter((conflict) => conflict.versions.length > 0).map((conflict) => conflict.eventId));
}

/** Whether an event has an open timing conflict, which also pauses automatic Focus starts for it. */
export function hasTimingConflict(conflicts: readonly CalendarFieldConflict[], eventId: string): boolean {
  return findEventFieldConflict(conflicts, eventId, "timing") !== undefined;
}

/** Text of a version cut to `maxChars` characters with an ellipsis. */
export function previewConflictText(text: string, maxChars = CALENDAR_CONFLICT_PREVIEW_CHARS): string {
  const characters = Array.from(text);
  return characters.length <= maxChars ? text : `${characters.slice(0, maxChars).join("").trimEnd()}…`;
}

/** A timing version as shown in the dialog. */
export interface CalendarTimingSummary {
  /** Start formatted for the locale, in the event's own time zone for timed events. */
  start: string;
  end: string;
  /** Time zone name for timed events, or null for all-day events, which have no zone. */
  timezone: string | null;
  allDay: boolean;
  /** Recurrence summary, or null when the event does not repeat. */
  recurrence: string | null;
  excludedDates: number;
  addedDates: number;
  overrides: number;
}

function formatAllDayDate(locale: AppLocale, stored: string): string {
  const date = Temporal.PlainDate.from(stored.slice(0, 10));
  const utcMidnight = Date.UTC(date.year, date.month - 1, date.day);
  return formatDateTime(locale, utcMidnight, { dateStyle: "medium", timeZone: "UTC" });
}

function formatTimedInstant(locale: AppLocale, stored: string, timezone: string): string {
  const instant = Temporal.Instant.from(stored);
  return formatDateTime(locale, instant.epochMilliseconds, {
    dateStyle: "medium",
    timeStyle: "short",
    timeZone: timezone,
  });
}

/**
 * Formats a stored instant for display. An unparsable value or unknown zone falls back to the stored text, so a
 * dialog never fails to open over one odd value.
 */
function formatTimingBound(locale: AppLocale, stored: string, timezone: string, allDay: boolean): string {
  try {
    return allDay ? formatAllDayDate(locale, stored) : formatTimedInstant(locale, stored, timezone);
  } catch {
    return stored;
  }
}

function recurrenceSummary(value: CalendarTimingValue, locale: AppLocale, t: Translate): string | null {
  if (!value.rrule) return null;
  try {
    return formatRecurrenceLabel(
      rruleToRecurrence(value.rrule, value.repeatUntil ?? undefined),
      { t, locale },
    );
  } catch {
    return value.rrule;
  }
}

/**
 * Describes a timing version for the resolution dialog: start, end, recurrence, and the counts of excluded dates,
 * added dates, and overrides that travel with it.
 */
export function summarizeTiming(value: CalendarTimingValue, locale: AppLocale, t: Translate): CalendarTimingSummary {
  return {
    start: formatTimingBound(locale, value.startTime, value.timezone, value.allDay),
    end: formatTimingBound(locale, value.endTime, value.timezone, value.allDay),
    timezone: value.allDay ? null : value.timezone,
    allDay: value.allDay,
    recurrence: recurrenceSummary(value, locale, t),
    excludedDates: value.excludedDates,
    addedDates: value.addedDates,
    overrides: value.overrides,
  };
}
