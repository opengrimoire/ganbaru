import { Temporal } from "@js-temporal/polyfill";
import { describe, expect, it } from "vitest";

import {
  IDLE_CALENDAR_WINDOW,
  calendarWindowMarkedStale,
  calendarWindowReloadTarget,
  calendarWindowShown,
  calendarWindowShownFromCache,
  finishCalendarWindowLoad,
  isCalendarWindowBusy,
  isCalendarWindowFailed,
  isCalendarWindowLoaded,
  isCurrentCalendarWindowLoad,
  retainsStaleCalendarWindow,
  startCalendarWindowLoad,
  type CalendarWindowPhase,
} from "./window-state";

const week = { start: Temporal.PlainDate.from("2024-03-09"), end: Temporal.PlainDate.from("2024-03-15") };
const nextWeek = { start: Temporal.PlainDate.from("2024-03-16"), end: Temporal.PlainDate.from("2024-03-22") };
const shown: CalendarWindowPhase = { kind: "shown", stale: false };

describe("calendar window phase", () => {
  it("is busy while loading and shows the window only once one is applied", () => {
    const { phase, token } = startCalendarWindowLoad(IDLE_CALENDAR_WINDOW, "boot", week);
    expect(isCalendarWindowBusy(phase)).toBe(true);
    expect(isCalendarWindowLoaded(phase)).toBe(false);
    const applied = calendarWindowShown(phase);
    expect(isCalendarWindowLoaded(applied)).toBe(true);
    const settled = finishCalendarWindowLoad(applied, token, { type: "settled" });
    expect(settled).toEqual(shown);
    expect(isCalendarWindowBusy(settled)).toBe(false);
  });

  it("forgets the shown window on boot but keeps it for other loads", () => {
    expect(isCalendarWindowLoaded(startCalendarWindowLoad(shown, "boot", week).phase)).toBe(false);
    expect(isCalendarWindowLoaded(startCalendarWindowLoad(shown, "apply", week).phase)).toBe(true);
  });

  it("leaves a boot superseded by the cache owner unsettled so it can request again", () => {
    const { phase, token } = startCalendarWindowLoad(IDLE_CALENDAR_WINDOW, "boot", week);
    const invalidated = calendarWindowMarkedStale(phase);
    expect(isCurrentCalendarWindowLoad(invalidated, token)).toBe(true);
    expect(calendarWindowReloadTarget(invalidated, null)).toEqual(week);
  });

  it("ignores a superseded load once a newer one took over", () => {
    const first = startCalendarWindowLoad(shown, "apply", week);
    const second = startCalendarWindowLoad(first.phase, "apply", nextWeek);
    expect(isCurrentCalendarWindowLoad(second.phase, first.token)).toBe(false);
    expect(finishCalendarWindowLoad(second.phase, first.token, { type: "failed", error: "late" })).toBe(second.phase);
    expect(calendarWindowReloadTarget(second.phase, week)).toEqual(nextWeek);
  });

  it("keeps the stale window through its reload and drops it when the reload fails", () => {
    const stale = calendarWindowMarkedStale(shown);
    expect(retainsStaleCalendarWindow(stale)).toBe(true);
    const { phase, token } = startCalendarWindowLoad(stale, "apply", week);
    expect(retainsStaleCalendarWindow(phase)).toBe(true);
    const failed = finishCalendarWindowLoad(phase, token, { type: "failed", error: new Error("read failed") });
    expect(retainsStaleCalendarWindow(failed)).toBe(false);
    expect(isCalendarWindowFailed(failed)).toBe(true);
    expect(isCalendarWindowLoaded(failed)).toBe(true);
    expect(failed).toMatchObject({ kind: "failed", mode: "apply", target: week, shown: true });
  });

  it("reports a failed boot as not loaded and clears the failure while a retry runs", () => {
    const boot = startCalendarWindowLoad(IDLE_CALENDAR_WINDOW, "boot", week);
    const failed = finishCalendarWindowLoad(boot.phase, boot.token, { type: "failed", error: "boot" });
    expect(isCalendarWindowFailed(failed)).toBe(true);
    expect(isCalendarWindowLoaded(failed)).toBe(false);
    const retry = startCalendarWindowLoad(failed, "boot", week);
    expect(isCalendarWindowFailed(retry.phase)).toBe(false);
  });

  it("returns to the previous phase when a cache warm-up fails", () => {
    const { phase, token } = startCalendarWindowLoad(shown, "ensure", nextWeek);
    expect(finishCalendarWindowLoad(phase, token, { type: "failed", error: "warm" })).toEqual(shown);
    expect(calendarWindowReloadTarget(phase, week)).toEqual(week);
  });

  it("ends an in-flight load when a cached window is shown", () => {
    const { phase, token } = startCalendarWindowLoad(IDLE_CALENDAR_WINDOW, "apply", week);
    const cached = calendarWindowShownFromCache();
    expect(isCalendarWindowBusy(cached)).toBe(false);
    expect(finishCalendarWindowLoad(cached, token, { type: "settled" })).toBe(cached);
    expect(isCurrentCalendarWindowLoad(phase, token)).toBe(true);
  });
});
