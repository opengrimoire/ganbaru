import { describe, it, expect } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import {
  buildCreatePanelInitialChanges,
  buildEditPanelInitialChanges,
  fieldEqual,
  isDirtyDiff,
  minuteOffsetFromDateStart,
} from "./edit-session.svelte";
import { createCustomCountPomodoroConfig, createPresetPomodoroConfig } from "$lib/pomodoro/rhythm";

function countConfig(
  focusDurationMinutes = 40,
  shortBreakMinutes = 5,
  longBreakMinutes = 10,
  longBreakAfterFocusCount = 4,
  idleTimeoutMinutes: number | null = null,
) {
  return createCustomCountPomodoroConfig({
    focusDurationMinutes,
    shortBreakMinutes,
    longBreakMinutes,
    longBreakAfterFocusCount,
  }, idleTimeoutMinutes);
}

describe("fieldEqual", () => {
  it("treats two undefined values as equal", () => {
    expect(fieldEqual(undefined, undefined)).toBe(true);
  });

  it("treats two null values as equal", () => {
    expect(fieldEqual(null, null)).toBe(true);
  });

  it("distinguishes null from undefined", () => {
    expect(fieldEqual(null, undefined)).toBe(false);
  });

  it("compares primitives strictly", () => {
    expect(fieldEqual("a", "a")).toBe(true);
    expect(fieldEqual("a", "b")).toBe(false);
    expect(fieldEqual(1, 1)).toBe(true);
    expect(fieldEqual(1, "1")).toBe(false);
    expect(fieldEqual(false, false)).toBe(true);
    expect(fieldEqual(true, false)).toBe(false);
  });

  it("distinguishes empty string from undefined", () => {
    // Baseline and changes use the same normalization rules, so "" should
    // only ever appear when the field is literally empty.
    expect(fieldEqual("", undefined)).toBe(false);
    expect(fieldEqual("", "")).toBe(true);
  });

  it("deep-compares arrays", () => {
    expect(fieldEqual([0, 5, 10], [0, 5, 10])).toBe(true);
    expect(fieldEqual([0, 5], [0, 5, 10])).toBe(false);
    expect(fieldEqual([], [])).toBe(true);
  });

  it("deep-compares objects", () => {
    const a = { focusDurationMinutes: 40, shortBreakMinutes: 5 };
    const b = { focusDurationMinutes: 40, shortBreakMinutes: 5 };
    const c = { focusDurationMinutes: 25, shortBreakMinutes: 5 };
    expect(fieldEqual(a, b)).toBe(true);
    expect(fieldEqual(a, c)).toBe(false);
  });

  it("treats object vs undefined as unequal", () => {
    expect(fieldEqual({ a: 1 }, undefined)).toBe(false);
    expect(fieldEqual(undefined, { a: 1 })).toBe(false);
  });
});

describe("isDirtyDiff", () => {
  it("returns false when both sides are empty", () => {
    expect(isDirtyDiff({}, {})).toBe(false);
  });

  it("returns false when every field matches the baseline", () => {
    const baseline = {
      title: "Meeting",
      start: "2026-04-16 10:00",
      end: "2026-04-16 11:00",
      color: 14 as const,
      notifications: [0, 5],
    };
    const changes = { ...baseline };
    expect(isDirtyDiff(changes, baseline)).toBe(false);
  });

  it("returns true when a single field differs", () => {
    const baseline = { title: "Meeting", start: "2026-04-16 10:00" };
    const changes = { title: "Meeting!", start: "2026-04-16 10:00" };
    expect(isDirtyDiff(changes, baseline)).toBe(true);
  });

  it("returns false after a field is reverted to baseline", () => {
    const baseline = { title: "Meeting" };

    // User types a change
    let changes: Partial<typeof baseline> = { title: "Meet" };
    expect(isDirtyDiff(changes, baseline)).toBe(true);

    // User reverts
    changes = { title: "Meeting" };
    expect(isDirtyDiff(changes, baseline)).toBe(false);
  });

  it("detects changes via partial patches against a baseline", () => {
    // Simulates a drag emitting { start, end } only, while baseline has full shape.
    const baseline = {
      title: "Focus",
      start: "2026-04-16 09:00",
      end: "2026-04-16 10:00",
    };
    // The drag moved the event an hour later before the panel emitted a title.
    const changes = { start: "2026-04-16 10:00", end: "2026-04-16 11:00" };
    expect(isDirtyDiff(changes, baseline)).toBe(true);
  });

  it("returns false when a drag is undone by reverting to original times", () => {
    const baseline = {
      title: "Focus",
      start: "2026-04-16 09:00",
      end: "2026-04-16 10:00",
    };
    // Drag then drag-back to original positions.
    const changes = { start: "2026-04-16 09:00", end: "2026-04-16 10:00" };
    expect(isDirtyDiff(changes, baseline)).toBe(false);
  });

  it("deep-compares nested pomodoro config", () => {
    const pomodoro = countConfig(40, 5, 10, 4, 1);
    const baseline = { pomodoroConfig: pomodoro };
    const same = { pomodoroConfig: countConfig(40, 5, 10, 4, 1) };
    const different = { pomodoroConfig: countConfig(25, 5, 10, 4, 1) };
    expect(isDirtyDiff(same, baseline)).toBe(false);
    expect(isDirtyDiff(different, baseline)).toBe(true);
  });

  it("treats equivalent recurrence selector order as clean", () => {
    const baseline: Partial<CalendarEvent> = {
      recurrence: {
        frequency: "weekly",
        interval: 1,
        weekdays: ["FR", "MO"],
        end: { type: "never" },
      },
    };
    const changes: Partial<CalendarEvent> = {
      recurrence: {
        frequency: "weekly",
        interval: 1,
        weekdays: ["MO", "FR"],
        wkst: "MO",
        end: { type: "never" },
      },
    };

    expect(isDirtyDiff(changes, baseline)).toBe(false);
  });

  it("treats a toggled-off-then-on notification set as clean", () => {
    const baseline = { notifications: [0, 30] };
    // User toggled notifications off: changes has undefined.
    let changes: Partial<typeof baseline> = { notifications: undefined };
    expect(isDirtyDiff(changes, baseline)).toBe(true);
    // User toggled back on with the same values.
    changes = { notifications: [0, 30] };
    expect(isDirtyDiff(changes, baseline)).toBe(false);
  });

  it("flags dirty when changes add a key absent in baseline", () => {
    const baseline = { title: "Meeting" };
    const changes = { title: "Meeting", color: 2 as const };
    expect(isDirtyDiff(changes, baseline)).toBe(true);
  });

  it("does not flag dirty for baseline keys absent from changes", () => {
    // Merge semantics: the final event is {...baseline, ...changes}, so
    // a baseline field not overridden by changes keeps its baseline value
    // and must not count as a diff. This matters for partial patches such
    // as a drag that only emits start/end.
    const baseline = { title: "Meeting", description: "Weekly sync" };
    const changes = { title: "Meeting" };
    expect(isDirtyDiff(changes, baseline)).toBe(false);
  });

  it("ignores keys where both sides are undefined", () => {
    // The panel's emit shape always includes undefined for uncustomized fields.
    // Matching undefineds on both sides must not count as a diff.
    const baseline = { title: "X", color: undefined, description: undefined };
    const changes = { title: "X", color: undefined, description: undefined };
    expect(isDirtyDiff(changes, baseline)).toBe(false);
  });
});

describe("panel initial changes", () => {
  function makeEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
    return {
      id: "evt1",
      title: " Focus ",
      start: "2026-04-16 09:00",
      end: "2026-04-16 10:00",
      timezone: "America/New_York",
      calendarId: "local",
      ...overrides,
    };
  }

  it("normalizes edit baseline to the panel emit shape", () => {
    const result = buildEditPanelInitialChanges(makeEvent({
      notifications: [30, 0, 30],
      pomodoroConfig: countConfig(40, 5, 10, 2, 15),
      location: "Office",
      visibility: "private",
    }));

    expect(result.title).toBe("Focus");
    expect(result.notifications).toEqual([0, 30]);
    expect(result.pomodoroConfig).toEqual(countConfig(40, 5, 10, 2, 3));
    expect(result.location).toBe("Office");
    expect(result.meetingEnabled).toBe(true);
    expect(result.visibility).toBe("private");
  });

  it("keeps explicit empty meeting state in the edit baseline", () => {
    const result = buildEditPanelInitialChanges(makeEvent({ meetingEnabled: true }));

    expect(result.meetingEnabled).toBe(true);
    expect(result.location).toBeUndefined();
    expect(result.url).toBeUndefined();
    expect(result.attendees).toBeUndefined();
  });

  it("omits pomodoro config from all-day edit baselines", () => {
    const result = buildEditPanelInitialChanges(makeEvent({
      allDay: true,
      pomodoroConfig: countConfig(40, 5, 10, 4, 3),
    }));

    expect(result.allDay).toBe(true);
    expect(result.pomodoroConfig).toBeUndefined();
  });

  it("seeds create baseline with default panel values", () => {
    const result = buildCreatePanelInitialChanges("2026-04-16 09:00", "2026-04-16 10:00", true);

    expect(result.title).toBe("");
    expect(result.start).toBe("2026-04-16 09:00");
    expect(result.end).toBe("2026-04-16 10:00");
    expect(result.notifications).toEqual([0]);
    expect(result.allDay).toBe(true);
    expect(result.meetingEnabled).toBeUndefined();
    expect(result.visibility).toBe("private");
    expect(result.pomodoroConfig).toBeUndefined();
  });

  it("uses focus idle defaults when seeding a create baseline", () => {
    const result = buildCreatePanelInitialChanges(
      "2026-04-16 09:00",
      "2026-04-16 10:00",
      false,
      { pauseWhenIdle: false, thresholdMinutes: 7 },
    );

    expect(result.pomodoroConfig).toEqual(createPresetPomodoroConfig("adaptive", null));
  });

  it("merges project-scoped defaults into create baselines", () => {
    const pomodoroConfig = createPresetPomodoroConfig("deep", 5);
    const result = buildCreatePanelInitialChanges(
      "2026-04-16 09:00",
      "2026-04-16 09:30",
      false,
      undefined,
      {
        title: "Ganbaru AI",
        end: "2026-04-16 10:00",
        color: 14,
        projectId: "project-a",
        environmentId: "environment-a",
        playlistId: "playlist-a",
        pomodoroConfig,
      },
    );

    expect(result.title).toBe("Ganbaru AI");
    expect(result.start).toBe("2026-04-16 09:00");
    expect(result.end).toBe("2026-04-16 10:00");
    expect(result.color).toBe(14);
    expect(result.projectId).toBe("project-a");
    expect(result.environmentId).toBe("environment-a");
    expect(result.playlistId).toBe("playlist-a");
    expect(result.pomodoroConfig).toEqual(pomodoroConfig);
  });

  it("normalizes enabled edit baselines to the focus idle threshold", () => {
    const result = buildEditPanelInitialChanges(
      makeEvent({
        pomodoroConfig: countConfig(40, 5, 10, 2, 15),
      }),
      { pauseWhenIdle: true, thresholdMinutes: 5 },
    );

    expect(result.pomodoroConfig?.idleTimeoutMinutes).toBe(5);
  });
});

describe("minuteOffsetFromDateStart", () => {
  it("keeps next-day midnight as the start day's 1440 minute boundary", () => {
    expect(minuteOffsetFromDateStart("2026-04-16", "2026-04-17 00:00")).toBe(1440);
  });

  it("keeps same-day midnight as the zero minute boundary", () => {
    expect(minuteOffsetFromDateStart("2026-04-16", "2026-04-16 00:00")).toBe(0);
  });
});
