import { describe, expect, it } from "vitest";
import type { Calendar, CalendarEvent } from "$lib/calendar/types";
import {
  getCalendarEventEditLock,
  hasCalendarEventEnded,
  hasCalendarEventStarted,
  isImportedCalendar,
} from "./event-edit-permissions";
import { createPresetPomodoroConfig } from "$lib/pomodoro/rhythm";

const NOW = new Date("2026-05-26T12:00:00");

function calendar(patch: Partial<Calendar> = {}): Calendar {
  return {
    id: "local",
    name: "Local",
    color: "",
    source: "local",
    visible: true,
    readOnly: false,
    ...patch,
  };
}

function event(patch: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "event-1",
    title: "Focus",
    start: "2026-05-26 10:00",
    end: "2026-05-26 11:00",
    timezone: "UTC",
    calendarId: "local",
    ...patch,
  };
}

describe("calendar event edit permissions", () => {
  it("treats non-local calendars as imported", () => {
    expect(isImportedCalendar(calendar())).toBe(false);
    expect(isImportedCalendar(calendar({ id: "ics", source: "ics" }))).toBe(true);
  });

  it("uses the calendar date for all-day past detection", () => {
    expect(hasCalendarEventEnded(event({
      allDay: true,
      start: "2026-05-26 00:00",
      end: "2026-05-26 00:00",
    }), NOW)).toBe(false);

    expect(hasCalendarEventEnded(event({
      allDay: true,
      start: "2026-05-25 00:00",
      end: "2026-05-25 00:00",
    }), NOW)).toBe(true);
  });

  it("detects started cross-midnight events before they end", () => {
    const activeOvernight = event({
      start: "2026-05-25 21:00",
      end: "2026-05-26 05:00",
    });
    const duringEvent = new Date("2026-05-26T01:30:00");

    expect(hasCalendarEventStarted(activeOvernight, duringEvent)).toBe(true);
    expect(hasCalendarEventEnded(activeOvernight, duringEvent)).toBe(false);
  });

  it("uses the calendar date for all-day started detection", () => {
    expect(hasCalendarEventStarted(event({
      allDay: true,
      start: "2026-05-26 00:00",
      end: "2026-05-26 00:00",
    }), NOW)).toBe(true);

    expect(hasCalendarEventStarted(event({
      allDay: true,
      start: "2026-05-27 00:00",
      end: "2026-05-27 00:00",
    }), NOW)).toBe(false);
  });

  it("locks past imported events while allowing archive from the read-only panel", () => {
    const lock = getCalendarEventEditLock(
      event({ calendarId: "ics", start: "2026-05-25 10:00", end: "2026-05-25 11:00" }),
      [calendar({ id: "ics", source: "ics" })],
      { now: NOW },
    );

    expect(lock).toEqual({
      locked: true,
      reason: "past-imported",
      allowArchive: true,
    });
  });

  it("keeps future imported events editable", () => {
    const lock = getCalendarEventEditLock(
      event({ calendarId: "ics", start: "2026-05-27 10:00", end: "2026-05-27 11:00" }),
      [calendar({ id: "ics", source: "ics" })],
      { now: NOW },
    );

    expect(lock.locked).toBe(false);
  });

  it("keeps active local events editable", () => {
    const lock = getCalendarEventEditLock(
      event({
        start: "2026-05-25 21:00",
        end: "2026-05-26 15:00",
      }),
      [calendar()],
      { now: NOW },
    );

    expect(lock).toEqual({ locked: false, allowArchive: false });
  });

  it("locks past local events while allowing archive from the read-only panel", () => {
    const lock = getCalendarEventEditLock(
      event({
        start: "2026-05-25 21:00",
        end: "2026-05-26 05:00",
      }),
      [calendar()],
      { now: NOW },
    );

    expect(lock).toEqual({
      locked: true,
      reason: "started",
      allowArchive: true,
    });
  });

  it("keeps future local events editable", () => {
    const lock = getCalendarEventEditLock(
      event({ start: "2026-05-27 10:00", end: "2026-05-27 11:00" }),
      [calendar()],
      { now: NOW },
    );

    expect(lock.locked).toBe(false);
  });

  it("locks read-only calendars before source-specific rules", () => {
    const lock = getCalendarEventEditLock(
      event({ calendarId: "ics", start: "2026-05-25 10:00", end: "2026-05-25 11:00" }),
      [calendar({ id: "ics", source: "ics", readOnly: true })],
      { now: NOW },
    );

    expect(lock).toEqual({
      locked: true,
      reason: "read-only-calendar",
      allowArchive: false,
    });
  });

  it("keeps the existing past pomodoro archive affordance for local events", () => {
    const lock = getCalendarEventEditLock(
      event({
        start: "2026-05-25 10:00",
        end: "2026-05-25 11:00",
        pomodoroConfig: createPresetPomodoroConfig("creative"),
      }),
      [calendar()],
      { now: NOW },
    );

    expect(lock).toEqual({
      locked: true,
      reason: "past-pomodoro",
      allowArchive: true,
    });
  });

  it("keeps active pomodoro events editable so they can be ended", () => {
    const lock = getCalendarEventEditLock(
      event({
        start: "2026-05-25 10:00",
        end: "2026-05-25 11:00",
        pomodoroConfig: createPresetPomodoroConfig("creative"),
      }),
      [calendar()],
      { now: NOW, isActivePomodoroEvent: true },
    );

    expect(lock).toEqual({ locked: false, allowArchive: false });
  });
});
