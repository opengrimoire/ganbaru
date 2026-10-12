import { Temporal } from "@js-temporal/polyfill";
import type { CalendarEvent } from "$lib/calendar/types";
import { localTimezone } from "./event-payloads";
import { loadNativeCalendarWindow } from "./native-window";

let pomodoroSchedulerWindowCache: {
  key: string;
  version: number;
  promise: Promise<CalendarEvent[]>;
} | null = null;

function calendarWindowKey(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  renderZone: string,
): string {
  return `${renderZone}:${windowStart.toString()}:${windowEnd.toString()}`;
}

/** Invalidate cached canonical commitments after Calendar or render-zone changes. */
export function clearPomodoroSchedulerWindowCache(): void {
  pomodoroSchedulerWindowCache = null;
}

/** Coalesce one native expanded Focus window per canonical Calendar version. */
export async function loadPomodoroSchedulerEventsFromDb(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  version: number,
): Promise<CalendarEvent[]> {
  const renderZone = localTimezone();
  const key = calendarWindowKey(windowStart, windowEnd, renderZone);
  if (
    pomodoroSchedulerWindowCache &&
    pomodoroSchedulerWindowCache.key === key &&
    pomodoroSchedulerWindowCache.version === version
  ) {
    return pomodoroSchedulerWindowCache.promise;
  }

  const promise = loadNativeCalendarWindow({
    windowStartDate: windowStart.toString(),
    windowEndDate: windowEnd.toString(),
    renderZone,
    includeTotalEventCount: false,
  }, "focus").then((snapshot) => snapshot.windowEvents.filter((event) => event.pomodoroConfig))
    .catch((error: unknown) => {
      if (pomodoroSchedulerWindowCache?.promise === promise) {
        pomodoroSchedulerWindowCache = null;
      }
      throw error;
    });

  pomodoroSchedulerWindowCache = { key, version, promise };
  return promise;
}
