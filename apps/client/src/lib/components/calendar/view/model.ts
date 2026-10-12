import type { CalendarEvent, PositionedAllDayEvent, PositionedEvent } from "$lib/calendar/types";
import { buildEventsByDay } from "./events-by-day";
import { formatDatePart, layoutAllDayEventsForWeek, layoutEventsForDay } from "$lib/calendar/utils";

export interface CalendarViewModelKey {
  storeVersion: number;
  windowStart: string;
  windowEnd: string;
  timezone: string;
  mode: string;
}

export interface CalendarViewModel {
  key: CalendarViewModelKey;
  events: CalendarEvent[];
  eventsById: Map<string, CalendarEvent>;
  eventsByDay: Map<string, CalendarEvent[]>;
  timedEventsByDay: Map<string, CalendarEvent[]>;
  allDayEventsByDay: Map<string, CalendarEvent[]>;
  positionedTimedEventsByDay: Map<string, PositionedEvent[]>;
  positionedAllDayEvents: PositionedAllDayEvent[];
}

export interface CalendarViewModelBuildInput {
  key: CalendarViewModelKey;
  events: CalendarEvent[];
  visibleDays: Date[];
}

export interface CalendarViewModelBuilder {
  build(input: CalendarViewModelBuildInput): CalendarViewModel;
  readonly buildCount: number;
}

function isSameBuildInput(previous: CalendarViewModelBuildInput, next: CalendarViewModelBuildInput): boolean {
  if (previous.events !== next.events) return false;
  if (previous.key.storeVersion !== next.key.storeVersion
    || previous.key.windowStart !== next.key.windowStart
    || previous.key.windowEnd !== next.key.windowEnd
    || previous.key.timezone !== next.key.timezone
    || previous.key.mode !== next.key.mode
    || previous.visibleDays.length !== next.visibleDays.length) return false;
  return previous.visibleDays.every(
    (day, index) => formatDatePart(day) === formatDatePart(next.visibleDays[index]),
  );
}

/** Memoize the single full build owned by a Calendar render-key transition. */
export function createCalendarViewModelBuilder(): CalendarViewModelBuilder {
  let previousInput: CalendarViewModelBuildInput | undefined;
  let previousModel: CalendarViewModel | undefined;
  let buildCount = 0;
  return {
    build(input) {
      if (previousInput && previousModel && isSameBuildInput(previousInput, input)) return previousModel;
      previousInput = input;
      previousModel = buildCalendarViewModel(input);
      buildCount += 1;
      return previousModel;
    },
    get buildCount() {
      return buildCount;
    },
  };
}

/** Build the immutable, render-owned indexes and layouts for one Calendar view. */
export function buildCalendarViewModel(input: CalendarViewModelBuildInput): CalendarViewModel {
  const eventsByDay = buildEventsByDay(input.events);
  const timedEventsByDay = new Map<string, CalendarEvent[]>();
  const allDayEventsByDay = new Map<string, CalendarEvent[]>();
  const positionedTimedEventsByDay = new Map<string, PositionedEvent[]>();

  for (const [date, events] of eventsByDay) {
    const timed: CalendarEvent[] = [];
    const allDay: CalendarEvent[] = [];
    for (const event of events) {
      (event.allDay ? allDay : timed).push(event);
    }
    if (timed.length > 0) {
      timedEventsByDay.set(date, timed);
      positionedTimedEventsByDay.set(date, layoutEventsForDay(timed, date));
    }
    if (allDay.length > 0) {
      allDay.sort((a, b) => {
        const titleOrder = (a.title || "").toLowerCase().localeCompare((b.title || "").toLowerCase());
        return titleOrder || a.start.localeCompare(b.start);
      });
      allDayEventsByDay.set(date, allDay);
    }
  }

  return {
    key: { ...input.key },
    events: input.events,
    eventsById: new Map(input.events.map((event) => [event.id, event])),
    eventsByDay,
    timedEventsByDay,
    allDayEventsByDay,
    positionedTimedEventsByDay,
    positionedAllDayEvents: layoutAllDayEventsForWeek(input.events, input.visibleDays),
  };
}

/** Resolve the visible dates used by a view-model build. */
export function calendarViewModelDays(mode: string, anchorDate: Date, multiDayDays: Date[]): Date[] {
  if (mode === "week" || mode === "workweek") return multiDayDays;
  return [new Date(`${formatDatePart(anchorDate)}T12:00:00`)];
}
