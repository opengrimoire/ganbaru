import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import {
  buildCalendarViewModel,
  createCalendarViewModelBuilder,
  type CalendarViewModelBuildInput,
} from "./model";
import { buildEventsByDay } from "./events-by-day";
import { allDayEventsForDay, layoutAllDayEventsForWeek, layoutEventsForDay } from "$lib/calendar/utils";

function event(id: string, start: string, end: string, extra: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id,
    title: id,
    start,
    end,
    timezone: "America/New_York",
    calendarId: "visible",
    ...extra,
  };
}

const days = [new Date("2026-03-07T12:00:00"), new Date("2026-03-08T12:00:00")];

function input(events: CalendarEvent[]): CalendarViewModelBuildInput {
  return {
    key: {
      storeVersion: 4,
      windowStart: "2026-03-07",
      windowEnd: "2026-03-09",
      timezone: "America/New_York",
      mode: "week",
    },
    events,
    visibleDays: days,
  };
}

describe("calendar view model", () => {
  it("matches the previous day and layout output for DST, overrides, overlaps, and all-day rows", () => {
    const events = [
      event("before-dst", "2026-03-07 23:30", "2026-03-08 01:30"),
      event("series::2026-03-08", "2026-03-08 09:30", "2026-03-08 10:30", {
        recurringParentId: "series",
        color: 7,
      }),
      event("overlap", "2026-03-08 10:00", "2026-03-08 11:00", {
        attendees: [{ id: "a", email: "a@example.com", role: "req-participant", status: "accepted", rsvp: true }],
      }),
      event("all-day", "2026-03-07 00:00", "2026-03-08 23:59", { allDay: true }),
    ];
    const model = buildCalendarViewModel(input(events));
    const previousBuckets = buildEventsByDay(events);

    expect(model.eventsByDay).toEqual(previousBuckets);
    expect(model.positionedTimedEventsByDay.get("2026-03-08")).toEqual(
      layoutEventsForDay(previousBuckets.get("2026-03-08")!.filter((item) => !item.allDay), "2026-03-08"),
    );
    expect(model.positionedAllDayEvents).toEqual(layoutAllDayEventsForWeek(events, days));
    expect(model.allDayEventsByDay.get("2026-03-08")).toEqual(
      allDayEventsForDay(previousBuckets.get("2026-03-08")!, days[1]),
    );
  });

  it("uses only already-filtered visible events and preserves optimistic event objects", () => {
    const optimistic = event("edited", "2026-03-08 12:00", "2026-03-08 13:30", { title: "Optimistic" });
    const model = buildCalendarViewModel(input([optimistic]));

    expect(model.events).toEqual([optimistic]);
    expect(model.eventsById.get("edited")).toBe(optimistic);
    expect(model.eventsById.has("hidden-calendar-event")).toBe(false);
  });

  it("does not rebuild for pointer movement or unrelated state with the same render key", () => {
    const events = [event("stable", "2026-03-08 09:00", "2026-03-08 10:00")];
    const builder = createCalendarViewModelBuilder();
    const first = builder.build(input(events));

    for (let pointerMove = 0; pointerMove < 20; pointerMove += 1) {
      expect(builder.build(input(events))).toBe(first);
    }
    expect(builder.buildCount).toBe(1);

    builder.build({ ...input(events), key: { ...input(events).key, storeVersion: 5 } });
    expect(builder.buildCount).toBe(2);
  });
});
