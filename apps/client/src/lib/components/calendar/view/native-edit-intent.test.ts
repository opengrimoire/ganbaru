import { describe, expect, it } from "vitest";
import { buildNativeCalendarCreate, buildNativeCalendarEdit } from "./native-edit-intent";
import type { CalendarEvent } from "$lib/calendar/types";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";

function input() {
  const event: CalendarEvent = {
    id: "source::2026-05-15", recurringParentId: "source", recurrenceDate: "2026-05-15",
    title: "Focus", start: "2026-05-16 00:00", end: "2026-05-16 01:00",
    timezone: "America/New_York", calendarId: "local", description: "Stored description", projectId: "project",
  };
  const state: Extract<EditSessionState, { mode: "edit" }> = {
    mode: "edit", sessionKey: 1, originalEvent: event, instanceEvent: event, templateId: "source",
    detailsLoaded: true, anchor: { x: 0, y: 0, width: 1, height: 1 },
  };
  return { state, baseline: { ...event }, changes: { ...event }, scope: "this" as const, renderZone: "Asia/Tokyo" };
}

describe("native Calendar user intent", () => {
  it("sends authored gap labels without browser conversion, IDs or timestamps", () => {
    const create = buildNativeCalendarCreate({ start: "2026-03-08 02:00", end: "2026-03-08 02:30",
      renderZone: "America/New_York", changes: { title: "Gap", description: "", timezone: "America/New_York",
        recurrence: { frequency: "daily", interval: 1, end: { type: "count", count: 3 } } } });
    expect(create).toMatchObject({ kind: "create", draft: {
      timing: { startTime: "2026-03-08T02:00", endTime: "2026-03-08T02:30",
        timezone: "America/New_York", inputZone: "America/New_York", allDay: false },
      recurrence: { kind: "set", value: "FREQ=DAILY;COUNT=3" },
    } });
    expect(create.draft.fields?.map((field) => field.field)).toEqual(["title", "description"]);
    expect(JSON.stringify(create)).not.toMatch(/updatedAt|createdAt|__pending_create__/u);
  });

  it("sends a one-day floating range using the inclusive vault convention", () => {
    const create = buildNativeCalendarCreate({ start: "2026-03-08 00:00", end: "2026-03-08 00:00",
      renderZone: "Pacific/Kiritimati", changes: { allDay: true } });
    expect(create.draft.timing).toMatchObject({ startTime: "2026-03-08", endTime: "2026-03-08", allDay: true });
    expect(create.draft.pomodoroConfig).toEqual({ action: "clear" });
  });

  it("requests End now for one occurrence without supplying a browser cutoff", () => {
    const value = input();
    value.changes.end = "2026-05-16 00:45";
    value.changes.title = "Accepted title";
    const result = buildNativeCalendarEdit({ ...value, action: "end_now", scope: "all" });
    expect(result.action).toBe("end_now");
    expect(result.selection.scope).toBe("this");
    expect(result.draft.timing).toEqual({});
    expect(result.draft.fields).toEqual([{ field: "title", value: "Accepted title" }]);
  });

  it("retains the explicit Enable Focus intent and configuration for native admission", () => {
    const value = input();
    const result = buildNativeCalendarEdit({ ...value, action: "enable_focus", changes: {
      pomodoroConfig: { rhythm: { kind: "count", focusDurationMinutes: 25, shortBreakMinutes: 5,
        longBreakMinutes: 15, longBreakAfterFocusCount: 4 }, rhythmSource: "custom", presetKey: null,
        idleTimeoutMinutes: null },
    } });
    expect(result.action).toBe("enable_focus");
    expect(result.selection.scope).toBe("this");
    expect(result.draft.pomodoroConfig?.action).toBe("set");
  });

  it("omits unchanged geometry and metadata while retaining native home-date identity", () => {
    expect(buildNativeCalendarEdit(input())).toEqual({
      selection: { templateId: "source", recurrenceDate: "2026-05-15", scope: "this" },
      draft: { timing: {}, recurrence: { kind: "unchanged" }, fields: [], attendees: null, alarms: null, pomodoroConfig: null },
    });
  });

  it("sends edited civil labels and their display zone without computing instants or changing the home zone", () => {
    const value = input();
    value.changes.end = "2026-05-16 02:00";
    const result = buildNativeCalendarEdit(value);
    expect(result.draft.timing).toEqual({ endTime: "2026-05-16T02:00", inputZone: "Asia/Tokyo" });
    expect(result.draft.fields).toEqual([]);
  });

  it("retains a drag even when panel initialization used its dragged geometry", () => {
    const value = input();
    value.changes.start = value.baseline.start = "2026-05-16 00:15";
    expect(buildNativeCalendarEdit(value).draft.timing?.startTime).toBe("2026-05-16T00:15");
  });

  it("preserves explicit clears and leaves omitted editor fields alone", () => {
    const value = input();
    const result = buildNativeCalendarEdit({ ...value, changes: { description: "", projectId: undefined, attendees: [] } });
    expect(result.draft.fields).toEqual([{ field: "description", value: "" }, { field: "projectId", value: null }]);
    expect(result.draft.attendees).toEqual([]);
    expect(result.draft.pomodoroConfig).toBeNull();
  });

  it("sends floating date endpoints for both sides of a date-kind conversion", () => {
    const value = input();
    expect(buildNativeCalendarEdit({ ...value, changes: { allDay: true } }).draft.timing).toEqual({
      allDay: true, startTime: "2026-05-16", endTime: "2026-05-16", inputZone: "Asia/Tokyo",
    });
  });

  it("rejects selections missing native provenance rather than guessing from a displayed date", () => {
    const value = input();
    delete value.state.instanceEvent.recurrenceDate;
    expect(() => buildNativeCalendarEdit(value)).toThrow("native recurrence identity");
  });
});
