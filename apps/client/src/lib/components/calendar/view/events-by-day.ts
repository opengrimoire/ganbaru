import type { CalendarEvent } from "$lib/calendar/types";

/**
 * Advance a `YYYY-MM-DD` string by one calendar day. This avoids Temporal
 * allocations while bucketing visible calendar events in render hot paths.
 */
function nextDateKey(dateKey: string): string {
  const y = Number(dateKey.substring(0, 4));
  const m = Number(dateKey.substring(5, 7));
  const d = Number(dateKey.substring(8, 10));
  const date = new Date(y, m - 1, d);
  date.setDate(date.getDate() + 1);
  const ny = date.getFullYear();
  const nm = String(date.getMonth() + 1).padStart(2, "0");
  const nd = String(date.getDate()).padStart(2, "0");
  return `${ny}-${nm}-${nd}`;
}

/**
 * Bucket events by every date they touch. Timed events land on each day where
 * their end is after midnight, while all-day events use an inclusive date span.
 */
export function buildEventsByDay(
  events: readonly CalendarEvent[],
): Map<string, CalendarEvent[]> {
  const eventsByDay = new Map<string, CalendarEvent[]>();
  for (const event of events) {
    let dateKey = event.start.substring(0, 10);
    if (event.allDay) {
      const endDay = event.end.substring(0, 10);
      while (dateKey <= endDay) {
        const dayEvents = eventsByDay.get(dateKey);
        if (dayEvents) dayEvents.push(event);
        else eventsByDay.set(dateKey, [event]);
        dateKey = nextDateKey(dateKey);
      }
    } else {
      while (event.end > `${dateKey} 00:00`) {
        const dayEvents = eventsByDay.get(dateKey);
        if (dayEvents) dayEvents.push(event);
        else eventsByDay.set(dateKey, [event]);
        dateKey = nextDateKey(dateKey);
      }
    }
  }
  return eventsByDay;
}
