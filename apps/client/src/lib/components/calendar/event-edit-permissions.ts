import type { Calendar, CalendarEvent } from "$lib/calendar/types";
import { formatDatePart, parseCalendarDate } from "$lib/calendar/utils";

export type CalendarEventEditLockReason =
  | "read-only-calendar"
  | "past-imported"
  | "past-pomodoro"
  | "started";

export interface CalendarEventEditLock {
  locked: boolean;
  reason?: CalendarEventEditLockReason;
  allowArchive: boolean;
}

export interface CalendarEventEditLockOptions {
  now?: Date;
  isActivePomodoroEvent?: boolean;
}

export function hasCalendarEventEnded(event: CalendarEvent, now: Date = new Date()): boolean {
  if (event.allDay) {
    return event.end.split(" ")[0] < formatDatePart(now);
  }

  const endMs = parseCalendarDate(event.end).getTime();
  return Number.isFinite(endMs) && endMs < now.getTime();
}

export function hasCalendarEventStarted(event: CalendarEvent, now: Date = new Date()): boolean {
  if (event.allDay) {
    return event.start.split(" ")[0] <= formatDatePart(now);
  }

  const startMs = parseCalendarDate(event.start).getTime();
  return Number.isFinite(startMs) && startMs <= now.getTime();
}

export function isImportedCalendar(calendar: Calendar | undefined): boolean {
  return calendar !== undefined && calendar.source !== "local";
}

export function getCalendarEventEditLock(
  event: CalendarEvent,
  calendars: readonly Calendar[],
  options: CalendarEventEditLockOptions = {},
): CalendarEventEditLock {
  const calendar = calendars.find((item) => item.id === event.calendarId);
  if (calendar?.readOnly) {
    return { locked: true, reason: "read-only-calendar", allowArchive: false };
  }

  const ended = hasCalendarEventEnded(event, options.now ?? new Date());
  if (!ended || options.isActivePomodoroEvent === true) {
    return { locked: false, allowArchive: false };
  }

  if (isImportedCalendar(calendar)) {
    return { locked: true, reason: "past-imported", allowArchive: true };
  }

  if (event.pomodoroConfig) {
    return { locked: true, reason: "past-pomodoro", allowArchive: true };
  }

  return { locked: true, reason: "started", allowArchive: true };
}
