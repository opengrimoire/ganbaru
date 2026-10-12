import { describe, it, expect } from "vitest";
import { Temporal } from "@js-temporal/polyfill";
import type { CalendarEvent } from "$lib/calendar/types";
import {
  closedDisplay,
  buildCreateDisplay,
  isPendingCreateEventId,
  PENDING_CREATE_ID,
} from "./display-events";

const TEST_WINDOW = {
  start: Temporal.PlainDate.from("2025-01-01"),
  end: Temporal.PlainDate.from("2028-12-31"),
};

function makeEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "evt1",
    title: "Test event",
    start: "2026-03-20 10:00",
    end: "2026-03-20 11:00",
    timezone: "America/New_York",
    calendarId: "local",
    ...overrides,
  };
}

describe("closedDisplay", () => {
  it("returns store events unchanged", () => {
    const events = [makeEvent()];
    const result = closedDisplay(events);
    expect(result.events).toBe(events);
    expect(result.previewedIds.size).toBe(0);
    expect(result.editingId).toBeUndefined();
  });
});

describe("isPendingCreateEventId", () => {
  it("matches the create preview id and its expanded instances only", () => {
    expect(isPendingCreateEventId(PENDING_CREATE_ID)).toBe(true);
    expect(isPendingCreateEventId(`${PENDING_CREATE_ID}::2026-03-20`)).toBe(true);
    expect(isPendingCreateEventId("evt1")).toBe(false);
  });
});

describe("buildCreateDisplay", () => {
  it("injects create pseudo-event", () => {
    const events = [makeEvent()];
    const preview = { dateStr: "2026-03-20", startMinute: 720, endMinute: 780 };
    const result = buildCreateDisplay(events, preview, {}, TEST_WINDOW);
    expect(result.events.length).toBe(2);
    expect(result.editingId).toBe(PENDING_CREATE_ID);
    expect(result.previewedIds.has(PENDING_CREATE_ID)).toBe(true);

    const created = result.events.find((e) => e.id === PENDING_CREATE_ID);
    expect(created).toBeDefined();
    expect(created!.start).toBe("2026-03-20 12:00");
    expect(created!.end).toBe("2026-03-20 13:00");
  });

  it("carries local RSVP status into the create preview", () => {
    const preview = { dateStr: "2026-03-20", startMinute: 720, endMinute: 780 };
    const result = buildCreateDisplay([], preview, {
      localParticipationStatus: "tentative",
    }, TEST_WINDOW);

    const created = result.events.find((e) => e.id === PENDING_CREATE_ID);
    expect(created?.localParticipationStatus).toBe("tentative");
  });

  it("carries enabled meeting state into the create preview", () => {
    const preview = { dateStr: "2026-03-20", startMinute: 720, endMinute: 780 };
    const result = buildCreateDisplay([], preview, {
      meetingEnabled: true,
    }, TEST_WINDOW);

    const created = result.events.find((e) => e.id === PENDING_CREATE_ID);
    expect(created?.meetingEnabled).toBe(true);
  });

  it("rolls a timed create preview ending at 1440 to next-day midnight", () => {
    const preview = { dateStr: "2026-03-20", startMinute: 1320, endMinute: 1440 };
    const result = buildCreateDisplay([], preview, {}, TEST_WINDOW);

    const created = result.events.find((e) => e.id === PENDING_CREATE_ID);
    expect(created?.start).toBe("2026-03-20 22:00");
    expect(created?.end).toBe("2026-03-21 00:00");
  });

  it("lets a timed toggle override an all-day create preview", () => {
    const preview = {
      dateStr: "2026-03-20",
      startMinute: 0,
      endMinute: 0,
      allDay: true,
      endDateStr: "2026-03-20",
    };
    const result = buildCreateDisplay([], preview, {
      start: "2026-03-20 14:00",
      end: "2026-03-20 15:00",
      allDay: undefined,
    }, TEST_WINDOW);

    const created = result.events.find((e) => e.id === PENDING_CREATE_ID);
    expect(created?.allDay).toBeUndefined();
    expect(created?.start).toBe("2026-03-20 14:00");
    expect(created?.end).toBe("2026-03-20 15:00");
  });

  it("keeps only the immediate authored card while native recurrence review is pending", () => {
    const events: CalendarEvent[] = [];
    const preview = {
      dateStr: "2026-03-16",
      startMinute: 600,
      endMinute: 660,
      recurrence: { frequency: "daily" as const, interval: 1, end: { type: "never" as const } },
    };
    const result = buildCreateDisplay(events, preview, {}, {
      start: Temporal.PlainDate.from("2026-03-16"),
      end: Temporal.PlainDate.from("2026-03-22"),
    });
    const recurrenceDates = result.events.map((e) => e.start.split(" ")[0]);

    expect(recurrenceDates).toEqual(["2026-03-16"]);
    expect(result.previewedIds).toEqual(new Set([PENDING_CREATE_ID]));
  });

  it("keeps a coincident real event beside the unsaved draft", () => {
    const real = makeEvent();
    const result = buildCreateDisplay([real], { dateStr: "2026-03-20", startMinute: 600, endMinute: 660 }, {}, TEST_WINDOW);
    expect(result.events.map((event) => event.id)).toEqual([real.id, PENDING_CREATE_ID]);
    expect(result.events[1].calendarId).toBe(real.calendarId);
  });

  it("lets cleared recurrence override stale create preview recurrence", () => {
    const preview = {
      dateStr: "2026-03-20",
      startMinute: 600,
      endMinute: 660,
      recurrence: { frequency: "daily" as const, interval: 1, end: { type: "never" as const } },
    };

    const result = buildCreateDisplay([], preview, { recurrence: undefined }, TEST_WINDOW);

    expect(result.events).toHaveLength(1);
    expect(result.previewedIds).toEqual(new Set([PENDING_CREATE_ID]));
    expect(result.events[0]?.recurrence).toBeUndefined();
  });
});
