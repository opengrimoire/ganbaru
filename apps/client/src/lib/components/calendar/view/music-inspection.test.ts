import { describe, expect, it } from "vitest";
import {
  calendarMusicInspectionTarget,
  handlesCalendarMusicInspection,
  registerCalendarMusicInspectionTarget,
  type CalendarMusicInspectionTarget,
} from "./music-inspection";

function target(scope: CalendarMusicInspectionTarget["scope"], focused: boolean): CalendarMusicInspectionTarget {
  return { scope, isFocused: () => focused };
}

describe("calendarMusicInspectionTarget", () => {
  it("returns nothing when no Calendar is shown", () => {
    expect(calendarMusicInspectionTarget([])).toBeNull();
  });

  it("prefers the focused pane in a split of two Calendars", () => {
    const primary = target("primary", false);
    const secondary = target("secondary", true);
    expect(calendarMusicInspectionTarget([primary, secondary])).toBe(secondary);
  });

  it("falls back to the primary pane when another kind of tab has focus", () => {
    const secondary = target("secondary", false);
    const primary = target("primary", false);
    expect(calendarMusicInspectionTarget([secondary, primary])).toBe(primary);
  });

  it("uses an unfocused Calendar when it is the only one shown", () => {
    const secondary = target("secondary", false);
    expect(calendarMusicInspectionTarget([secondary])).toBe(secondary);
  });
});

describe("registerCalendarMusicInspectionTarget", () => {
  it("lets exactly one registered Calendar handle an inspection and forgets disposed ones", () => {
    const primary = target("primary", false);
    const secondary = target("secondary", true);
    const disposePrimary = registerCalendarMusicInspectionTarget(primary);
    const disposeSecondary = registerCalendarMusicInspectionTarget(secondary);

    expect(handlesCalendarMusicInspection(secondary)).toBe(true);
    expect(handlesCalendarMusicInspection(primary)).toBe(false);

    disposeSecondary();
    expect(handlesCalendarMusicInspection(primary)).toBe(true);

    disposePrimary();
    expect(handlesCalendarMusicInspection(primary)).toBe(false);
  });
});
