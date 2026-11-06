import { describe, expect, it } from "vitest";
import {
  CALENDAR_ZOOM_PERCENT_LEVELS,
  calendarGridMinutesForHourHeight,
  calendarZoomGridMinutesForPercent,
  calendarZoomHourHeightForPercent,
} from "./zoom-levels";

describe("calendar zoom levels", () => {
  it("scales the hour height from the 100% level", () => {
    expect(calendarZoomHourHeightForPercent(100)).toBe(50);
    expect(calendarZoomHourHeightForPercent(50)).toBe(25);
    expect(calendarZoomHourHeightForPercent(150)).toBe(75);
    expect(calendarZoomHourHeightForPercent(400)).toBe(200);
  });

  it("snaps finer as the hour grows, switching exactly at each threshold", () => {
    expect(calendarGridMinutesForHourHeight(39.9)).toBe(30);
    expect(calendarGridMinutesForHourHeight(40)).toBe(15);
    expect(calendarGridMinutesForHourHeight(59.9)).toBe(15);
    expect(calendarGridMinutesForHourHeight(60)).toBe(10);
    expect(calendarGridMinutesForHourHeight(119.9)).toBe(10);
    expect(calendarGridMinutesForHourHeight(120)).toBe(5);
  });

  it("gives every zoom level a snap that divides an hour and never coarsens as zoom grows", () => {
    const snaps = CALENDAR_ZOOM_PERCENT_LEVELS.map(calendarZoomGridMinutesForPercent);
    expect(snaps).toEqual([30, 30, 15, 10, 10, 10, 5, 5]);
    for (const snap of snaps) expect(60 % snap).toBe(0);
    for (let i = 1; i < snaps.length; i++) expect(snaps[i]).toBeLessThanOrEqual(snaps[i - 1]);
  });
});
