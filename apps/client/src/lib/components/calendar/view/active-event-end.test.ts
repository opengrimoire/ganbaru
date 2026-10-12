import { describe, expect, it } from "vitest";
import type { CalendarEvent, PomodoroConfig } from "$lib/calendar/types";
import { createPresetPomodoroConfig } from "$lib/pomodoro/rhythm";
import {
  activePomodoroSaveWouldStopSession,
  endActiveEventWouldStopProductivity,
  isActiveTimedCalendarEvent,
  isSameConcreteOccurrence,
} from "./active-event-end";

const pomodoroConfig: PomodoroConfig = createPresetPomodoroConfig("adaptive", 1);

function event(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "event-1",
    title: "Focus",
    start: "2026-05-24 09:00",
    end: "2026-05-24 13:00",
    timezone: "UTC",
    calendarId: "local",
    pomodoroConfig,
    ...overrides,
  };
}

describe("active event end helpers", () => {
  it("requires the stronger confirmation when no other pomodoro event overlaps now", () => {
    const selected = event();
    const future = event({
      id: "future",
      start: "2026-05-24 14:00",
      end: "2026-05-24 18:00",
    });

    expect(endActiveEventWouldStopProductivity(
      selected,
      [selected, future],
      new Date("2026-05-24T10:30:00"),
    )).toBe(true);
  });

  it("does not require focus-session confirmation for a non-pomodoro active event", () => {
    const selected = event({ pomodoroConfig: undefined });

    expect(endActiveEventWouldStopProductivity(
      selected,
      [selected],
      new Date("2026-05-24T10:30:00"),
    )).toBe(false);
  });

  it("uses inline confirmation when another pomodoro event overlaps now", () => {
    const selected = event();
    const stacked = event({
      id: "stacked",
      start: "2026-05-24 10:00",
      end: "2026-05-24 12:00",
    });

    expect(endActiveEventWouldStopProductivity(
      selected,
      [selected, stacked],
      new Date("2026-05-24T10:30:00"),
    )).toBe(false);
  });

  it("treats second-precision event ends as no longer overlapping after the cut", () => {
    const selected = event();
    const cut = event({
      id: "cut",
      start: "2026-05-24 10:00",
      end: "2026-05-24 10:30:15",
    });

    expect(endActiveEventWouldStopProductivity(
      selected,
      [selected, cut],
      new Date("2026-05-24T10:30:16"),
    )).toBe(true);
  });

  it("matches first-occurrence aliases using their native recurrence identity", () => {
    const template = event({
      id: "template-1",
      recurrenceDate: "2026-05-24",
      recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
    });
    const firstSynthetic = event({
      id: "template-1::2026-05-24",
      recurringParentId: "template-1",
    });

    expect(isSameConcreteOccurrence(template, firstSynthetic)).toBe(true);
  });

  it("detects active timed calendar events without requiring pomodoro config", () => {
    const selected = event({ pomodoroConfig: undefined });

    expect(isActiveTimedCalendarEvent(
      selected,
      new Date("2026-05-24T10:30:00"),
    )).toBe(true);

    expect(isActiveTimedCalendarEvent(
      selected,
      new Date("2026-05-24T13:00:00"),
    )).toBe(false);
  });

  it("does not treat all-day events as cuttable active timed events", () => {
    expect(isActiveTimedCalendarEvent(
      event({
        allDay: true,
        start: "2026-05-24 00:00",
        end: "2026-05-25 00:00",
      }),
      new Date("2026-05-24T10:30:00"),
    )).toBe(false);
  });

  it("stops an active pomodoro save when the pomodoro config is removed", () => {
    expect(activePomodoroSaveWouldStopSession(
      {
        start: "2026-05-24 09:00",
        end: "2026-05-24 13:00",
        pomodoroConfig: undefined,
      },
      new Date("2026-05-24T10:30:00"),
    )).toBe(true);
  });

  it("keeps an active pomodoro save running when the config remains and the event still covers now", () => {
    expect(activePomodoroSaveWouldStopSession(
      {
        start: "2026-05-24 09:00",
        end: "2026-05-24 13:00",
        pomodoroConfig,
      },
      new Date("2026-05-24T10:30:00"),
    )).toBe(false);
  });

  it("stops an active pomodoro save when the edited time no longer covers now", () => {
    expect(activePomodoroSaveWouldStopSession(
      {
        start: "2026-05-24 11:00",
        end: "2026-05-24 13:00",
        pomodoroConfig,
      },
      new Date("2026-05-24T10:30:00"),
    )).toBe(true);
  });
});
