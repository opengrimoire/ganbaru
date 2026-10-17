import type { CalendarEvent, PomodoroConfig } from "$lib/calendar/types";
import { parseCalendarDate } from "$lib/calendar/utils";
import { isSameConcreteOccurrence } from "$lib/components/calendar/occurrence-protection";

export { isSameConcreteOccurrence };

export function eventCoversInstant(event: CalendarEvent, instant: Date): boolean {
  const startMs = parseCalendarDate(event.start).getTime();
  const endMs = parseCalendarDate(event.end).getTime();
  const instantMs = instant.getTime();
  return Number.isFinite(startMs)
    && Number.isFinite(endMs)
    && startMs <= instantMs
    && instantMs < endMs;
}

export function isActiveTimedCalendarEvent(
  event: CalendarEvent,
  now: Date = new Date(),
): boolean {
  return event.allDay !== true && eventCoversInstant(event, now);
}

export function endActiveEventWouldStopProductivity(
  selectedEvent: CalendarEvent,
  visibleEvents: readonly CalendarEvent[],
  now: Date = new Date(),
): boolean {
  if (!selectedEvent.pomodoroConfig) return false;
  return !visibleEvents.some((event) =>
    !!event.pomodoroConfig
    && !event.allDay
    && !isSameConcreteOccurrence(event, selectedEvent)
    && eventCoversInstant(event, now),
  );
}

export function activePomodoroSaveWouldStopSession(
  data: {
    start: string;
    end: string;
    pomodoroConfig?: PomodoroConfig;
    allDay?: boolean;
  },
  now: Date = new Date(),
): boolean {
  if (data.allDay === true || !data.pomodoroConfig) return true;

  const newStart = parseCalendarDate(data.start);
  const newEnd = parseCalendarDate(data.end);
  return !(now >= newStart && now < newEnd);
}
