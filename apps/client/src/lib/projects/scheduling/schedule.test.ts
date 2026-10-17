import { Temporal } from "@js-temporal/polyfill";
import { describe, expect, it } from "vitest";
import {
  formatProjectScheduleWindowStart,
  projectCalendarCreateDefaults,
  projectCalendarEventRootId,
  projectDefaultScheduleStart,
  projectEventDurationMinutes,
  projectEventDurationMinutesInDateRange,
  projectScheduleWindowFor,
} from "./schedule";
import type { Project } from "$lib/projects/types";

function project(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-a",
    groupId: "group-a",
    name: "Launch",
    icon: "lucide:folder",
    color: 2,
    sortOrder: 1000,
    status: "active",
    defaultEventName: "Deep work",
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: 60,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "creative",
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: true,
    defaultIdleThresholdMinutes: 15,
    focusPlaylistId: "playlist-a",
    workEnvironmentId: "environment-a",
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
    ...overrides,
  };
}

describe("project scheduling helpers", () => {
  it("rounds the default start to the next quarter hour", () => {
    const start = projectDefaultScheduleStart(Temporal.PlainDateTime.from("2026-06-21T10:07"));

    expect(start).toEqual({
      date: "2026-06-21",
      time: "10:15",
    });
  });

  it("rolls the default start into the next day when rounding passes midnight", () => {
    const start = projectDefaultScheduleStart(Temporal.PlainDateTime.from("2026-06-21T23:59"));

    expect(start).toEqual({
      date: "2026-06-22",
      time: "00:00",
    });
  });

  it("formats schedule windows with date rollover", () => {
    const window = formatProjectScheduleWindowStart(
      Temporal.PlainDateTime.from("2026-06-21T23:30"),
      90,
    );

    expect(window).toEqual({
      start: "2026-06-21 23:30",
      end: "2026-06-22 01:00",
    });
  });

  it("rejects invalid schedule windows", () => {
    expect(projectScheduleWindowFor("", "10:00", 30)).toBeNull();
    expect(projectScheduleWindowFor("2026-06-21", "", 30)).toBeNull();
    expect(projectScheduleWindowFor("2026-06-21", "10:00", 0)).toBeNull();
    expect(projectScheduleWindowFor("not-a-date", "10:00", 30)).toBeNull();
    expect(projectScheduleWindowFor("2026-06-21", "not-a-time", 30)).toBeNull();
  });

  it("computes event duration safely", () => {
    expect(projectEventDurationMinutes({
      start: "2026-06-21 09:15",
      end: "2026-06-21 10:45",
    })).toBe(90);
    expect(projectEventDurationMinutes({
      start: "bad",
      end: "2026-06-21 10:45",
    })).toBe(0);
  });

  it("builds timed calendar defaults from project settings", () => {
    expect(projectCalendarCreateDefaults({
      project: project(),
      start: "2026-06-21 09:00",
      end: "2026-06-21 09:30",
      globalIdleDefaults: {
        idlePauseEnabled: true,
        idleThresholdMinutes: 5,
      },
    })).toEqual({
      title: "Deep work",
      start: "2026-06-21 09:00",
      end: "2026-06-21 10:00",
      allDay: undefined,
      projectId: "project-a",
      color: 2,
      environmentId: "environment-a",
      playlistId: "playlist-a",
      pomodoroConfig: {
        rhythm: {
          kind: "count",
          focusDurationMinutes: 25,
          shortBreakMinutes: 5,
          longBreakMinutes: 15,
          longBreakAfterFocusCount: 4,
        },
        rhythmSource: "preset",
        presetKey: "creative",
        idleTimeoutMinutes: 5,
      },
    });
  });

  it("uses the localized built-in project name as its untouched event title", () => {
    const defaults = projectCalendarCreateDefaults({
      project: project({
        id: "project-routine-eat",
        name: "Comer",
        defaultEventName: null,
      }),
      start: "2026-06-21 10:00",
      end: "2026-06-21 11:00",
      allDay: false,
      globalIdleDefaults: {
        idlePauseEnabled: true,
        idleThresholdMinutes: 5,
      },
    });

    expect(defaults.title).toBe("Comer");
  });

  it("builds all-day calendar defaults without Pomodoro config", () => {
    expect(projectCalendarCreateDefaults({
      project: project({
        defaultEventTimeMode: "all_day",
      }),
      start: "2026-06-21 09:00",
      end: "2026-06-21 09:30",
      globalIdleDefaults: {
        idlePauseEnabled: true,
        idleThresholdMinutes: 5,
      },
    })).toMatchObject({
      start: "2026-06-21 00:00",
      end: "2026-06-21 00:00",
      allDay: true,
      pomodoroConfig: undefined,
    });
  });

  it("derives recurrence root ids for project event filtering", () => {
    expect(projectCalendarEventRootId({ id: "event-a::2026-06-21", recurringParentId: undefined })).toBe("event-a");
    expect(projectCalendarEventRootId({ id: "override-a", recurringParentId: "event-a" })).toBe("event-a");
  });

  it("sums scheduled event durations inside a date range", () => {
    expect(projectEventDurationMinutesInDateRange([
      { start: "2026-06-20 09:00", end: "2026-06-20 10:00" },
      { start: "2026-06-21 09:00", end: "2026-06-21 10:30" },
      { start: "2026-06-22 09:00", end: "2026-06-22 09:45" },
      { start: "2026-06-23 09:00", end: "2026-06-23 10:00" },
    ], "2026-06-21", "2026-06-22")).toBe(135);
  });
});
