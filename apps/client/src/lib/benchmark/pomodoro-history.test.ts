import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import {
  buildDensePomodoroHistoryPayload,
  DENSE_TIMED_POMODORO_CONFIG,
} from "./pomodoro-history";

function event(id: string, start: string, end: string): CalendarEvent {
  return {
    id,
    title: id,
    start,
    end,
    timezone: "UTC",
    calendarId: "benchmark",
    sourceUid: id,
    pomodoroConfig: DENSE_TIMED_POMODORO_CONFIG,
  };
}

describe("benchmark Pomodoro history seed", () => {
  it("adds configs for timed Pomodoro events and completed history only before the cutoff", () => {
    const payload = buildDensePomodoroHistoryPayload([
      event("past", "2026-04-29 23:00", "2026-04-30 00:00"),
      event("future", "2026-04-30 00:00", "2026-04-30 01:00"),
      {
        ...event("all-day", "2026-04-29 00:00", "2026-04-29 00:00"),
        allDay: true,
      },
    ], "2026-04-30 00:00");

    expect(payload.configs.map((config) => config.eventId)).toEqual(["past", "future"]);
    expect(payload.segments.map((segment) => ({
      eventId: segment.eventId,
      phase: segment.phase,
      status: segment.status,
      plannedStart: segment.plannedStart,
      plannedEnd: segment.plannedEnd,
    }))).toEqual([
      {
        eventId: "past",
        phase: "focus",
        status: "completed",
        plannedStart: "2026-04-29T23:00:00Z",
        plannedEnd: "2026-04-29T23:40:00Z",
      },
      {
        eventId: "past",
        phase: "short_break",
        status: "completed",
        plannedStart: "2026-04-29T23:40:00Z",
        plannedEnd: "2026-04-29T23:45:00Z",
      },
      {
        eventId: "past",
        phase: "focus",
        status: "completed",
        plannedStart: "2026-04-29T23:45:00Z",
        plannedEnd: "2026-04-30T00:00:00Z",
      },
    ]);
  });
});
