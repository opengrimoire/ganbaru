import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  parseCalendarDate,
  formatCalendarDate,
  formatCalendarDateWithSeconds,
  formatDatePart,
  formatMonthYear,
  startOfWeek,
  getWeekDays,
  getWorkCycleDays,
  adjacentWorkCycleAnchor,
  addDays,
  isSameDay,
  minuteOfDay,
  durationMinutes,
  snapToGrid,
  snapSimpleClickStartMinute,
  clampMinute,
  eventsForDay,
  allDayEventsForDay,
  allDayEventsForWeek,
  layoutAllDayEventsForWeek,
  layoutEventsForDay,
  effectiveMinuteRange,
  minuteOffsetToDateStr,
  formatTimeLabel,
  formatTimeRange,
  getHourInTimezone,
  sanitizeCalendarTime,
  normalizeEventColor,
  EVENT_COLOR_OPTIONS,
  getEventColor,
  getEventStatusPatternClass,
  getEventSurfaceStatusForIdentity,
  getPastEventColor,
  getOutsideMonthEventColor,
  isEventSurfaceCancelled,
  getTimezoneCity,
  getTimezoneRegion,
  getTimezoneOffsetMinutes,
  getTimezoneInfo,
  listAllTimezones,
  searchTimezones,
  deriveAcronymFromLongName,
  compactOffsetFromLong,
  computeViewWindow,
  visibleMinuteRangeForScroll,
} from "./utils";
import { FALLBACK_COLOR_INDEX, PALETTE_SIZE, type CalendarEvent } from "./types";
import { lightTheme, darkTheme, type Theme } from "$lib/themes";

/** Build a CalendarEvent with required timezone/calendarId defaults. */
function evt(base: Omit<CalendarEvent, "timezone" | "calendarId">): CalendarEvent {
  return { timezone: "UTC", calendarId: "local", ...base };
}

describe("parseCalendarDate", () => {
  it("parses YYYY-MM-DD HH:MM to a local Date", () => {
    const d = parseCalendarDate("2026-03-13 14:30");
    expect(d.getFullYear()).toBe(2026);
    expect(d.getMonth()).toBe(2); // 0-indexed
    expect(d.getDate()).toBe(13);
    expect(d.getHours()).toBe(14);
    expect(d.getMinutes()).toBe(30);
  });

  it("parses optional seconds", () => {
    const d = parseCalendarDate("2026-03-13 14:30:45");
    expect(d.getHours()).toBe(14);
    expect(d.getMinutes()).toBe(30);
    expect(d.getSeconds()).toBe(45);
  });

  it("defaults time to 00:00 if missing", () => {
    const d = parseCalendarDate("2026-01-01");
    expect(d.getHours()).toBe(0);
    expect(d.getMinutes()).toBe(0);
  });
});

describe("formatCalendarDate", () => {
  it("formats a Date to YYYY-MM-DD HH:MM", () => {
    const d = new Date(2026, 2, 13, 14, 30);
    expect(formatCalendarDate(d)).toBe("2026-03-13 14:30");
  });

  it("zero-pads single digit values", () => {
    const d = new Date(2026, 0, 5, 3, 7);
    expect(formatCalendarDate(d)).toBe("2026-01-05 03:07");
  });
});

describe("formatMonthYear", () => {
  it("supports full and abbreviated localized month labels while preserving the year", () => {
    const date = new Date(2026, 8, 1);
    expect(formatMonthYear(date, "en-US")).toBe("September 2026");
    expect(formatMonthYear(date, "en-US", "short")).toBe("Sep 2026");
  });
});

describe("formatCalendarDateWithSeconds", () => {
  it("formats a Date to YYYY-MM-DD HH:MM:SS", () => {
    const d = new Date(2026, 2, 13, 14, 30, 5, 999);
    expect(formatCalendarDateWithSeconds(d)).toBe("2026-03-13 14:30:05");
  });
});

describe("parseCalendarDate and formatCalendarDate round-trip", () => {
  it("round-trips without data loss", () => {
    const original = "2026-07-04 09:15";
    const parsed = parseCalendarDate(original);
    const formatted = formatCalendarDate(parsed);
    expect(formatted).toBe(original);
  });
});

describe("formatDatePart", () => {
  it("returns YYYY-MM-DD", () => {
    const d = new Date(2026, 11, 25, 18, 45);
    expect(formatDatePart(d)).toBe("2026-12-25");
  });
});

describe("startOfWeek", () => {
  it("returns Monday for any day of the week", () => {
    // 2026-03-13 is a Friday
    const friday = new Date(2026, 2, 13);
    const monday = startOfWeek(friday);
    expect(monday.getDay()).toBe(1); // Monday
    expect(monday.getDate()).toBe(9);
  });

  it("returns same day if already Monday", () => {
    const monday = new Date(2026, 2, 9);
    const result = startOfWeek(monday);
    expect(result.getDate()).toBe(9);
  });

  it("handles Sunday correctly (shifts back to previous Monday)", () => {
    const sunday = new Date(2026, 2, 15);
    const monday = startOfWeek(sunday);
    expect(monday.getDate()).toBe(9);
  });
});

describe("getWeekDays", () => {
  it("returns 7 days starting from Monday", () => {
    const days = getWeekDays(new Date(2026, 2, 13));
    expect(days).toHaveLength(7);
    expect(days[0].getDay()).toBe(1); // Monday
    expect(days[6].getDay()).toBe(0); // Sunday
  });
});

describe("getWorkCycleDays", () => {
  it("returns Monday through Friday for weekday anchors", () => {
    const days = getWorkCycleDays(new Date(2026, 3, 29));
    expect(days.map(formatDatePart)).toEqual([
      "2026-04-27",
      "2026-04-28",
      "2026-04-29",
      "2026-04-30",
      "2026-05-01",
    ]);
  });

  it("returns Saturday and Sunday for weekend anchors", () => {
    expect(getWorkCycleDays(new Date(2026, 4, 2)).map(formatDatePart)).toEqual([
      "2026-05-02",
      "2026-05-03",
    ]);
    expect(getWorkCycleDays(new Date(2026, 4, 3)).map(formatDatePart)).toEqual([
      "2026-05-02",
      "2026-05-03",
    ]);
  });
});

describe("adjacentWorkCycleAnchor", () => {
  it("moves from weekdays to weekend and back", () => {
    expect(formatDatePart(adjacentWorkCycleAnchor(new Date(2026, 3, 29), "forward")))
      .toBe("2026-05-02");
    expect(formatDatePart(adjacentWorkCycleAnchor(new Date(2026, 3, 29), "back")))
      .toBe("2026-04-25");
  });

  it("moves from weekend to neighboring weekdays", () => {
    expect(formatDatePart(adjacentWorkCycleAnchor(new Date(2026, 4, 2), "forward")))
      .toBe("2026-05-04");
    expect(formatDatePart(adjacentWorkCycleAnchor(new Date(2026, 4, 2), "back")))
      .toBe("2026-04-27");
  });
});

describe("addDays", () => {
  it("adds positive days", () => {
    const d = addDays(new Date(2026, 2, 13), 5);
    expect(d.getDate()).toBe(18);
  });

  it("subtracts with negative days", () => {
    const d = addDays(new Date(2026, 2, 13), -3);
    expect(d.getDate()).toBe(10);
  });

  it("handles month boundaries", () => {
    const d = addDays(new Date(2026, 0, 31), 1);
    expect(d.getMonth()).toBe(1); // February
    expect(d.getDate()).toBe(1);
  });
});

describe("isSameDay", () => {
  it("returns true for same date different times", () => {
    const a = new Date(2026, 2, 13, 8, 0);
    const b = new Date(2026, 2, 13, 22, 30);
    expect(isSameDay(a, b)).toBe(true);
  });

  it("returns false for different dates", () => {
    const a = new Date(2026, 2, 13);
    const b = new Date(2026, 2, 14);
    expect(isSameDay(a, b)).toBe(false);
  });
});

describe("minuteOfDay", () => {
  it("parses HH:MM from calendar date string", () => {
    expect(minuteOfDay("2026-03-13 14:30")).toBe(870);
    expect(minuteOfDay("2026-03-13 00:00")).toBe(0);
    expect(minuteOfDay("2026-03-13 23:59")).toBe(1439);
  });

  it("includes seconds as fractional minutes", () => {
    expect(minuteOfDay("2026-03-13 14:30:30")).toBe(870.5);
  });

  it("defaults to 0 if no time part", () => {
    expect(minuteOfDay("2026-03-13")).toBe(0);
  });
});

describe("durationMinutes", () => {
  it("calculates duration between two calendar dates", () => {
    expect(durationMinutes("2026-03-13 14:00", "2026-03-13 15:30")).toBe(90);
    expect(durationMinutes("2026-03-13 08:00", "2026-03-13 08:30")).toBe(30);
  });

  it("preserves second-level duration", () => {
    expect(durationMinutes("2026-03-13 14:00:00", "2026-03-13 14:00:30")).toBe(0.5);
  });

  it("returns 0 for same start and end", () => {
    expect(durationMinutes("2026-03-13 14:00", "2026-03-13 14:00")).toBe(0);
  });

  it("returns 0 for negative duration", () => {
    expect(durationMinutes("2026-03-13 15:00", "2026-03-13 14:00")).toBe(0);
  });
});

describe("snapToGrid", () => {
  it("snaps to 10-minute grid by default", () => {
    expect(snapToGrid(4)).toBe(0);
    expect(snapToGrid(5)).toBe(10);
    expect(snapToGrid(14)).toBe(10);
    expect(snapToGrid(15)).toBe(20);
    expect(snapToGrid(25)).toBe(30);
  });

  it("snaps to custom grid size", () => {
    expect(snapToGrid(20, 30)).toBe(30);
    expect(snapToGrid(14, 30)).toBe(0);
  });
});

describe("snapSimpleClickStartMinute", () => {
  it("uses the hour start for simple clicks in the first half of an hour", () => {
    expect(snapSimpleClickStartMinute(600)).toBe(600);
    expect(snapSimpleClickStartMinute(614.9)).toBe(600);
    expect(snapSimpleClickStartMinute(629.9)).toBe(600);
  });

  it("uses the half-hour start for simple clicks in the second half of an hour", () => {
    expect(snapSimpleClickStartMinute(630)).toBe(630);
    expect(snapSimpleClickStartMinute(644.9)).toBe(630);
    expect(snapSimpleClickStartMinute(659.9)).toBe(630);
  });

  it("does not round a simple click to the nearest grid mark", () => {
    expect(snapSimpleClickStartMinute(615)).toBe(600);
    expect(snapSimpleClickStartMinute(645)).toBe(630);
  });

  it("keeps the last possible simple click inside the current day", () => {
    expect(snapSimpleClickStartMinute(1440)).toBe(1410);
  });
});

describe("clampMinute", () => {
  it("clamps to 0-1440 range", () => {
    expect(clampMinute(-10)).toBe(0);
    expect(clampMinute(720)).toBe(720);
    expect(clampMinute(1500)).toBe(1440);
  });
});

describe("visibleMinuteRangeForScroll", () => {
  it("subtracts sticky chrome before converting pixels to minutes", () => {
    expect(visibleMinuteRangeForScroll({
      scrollTop: 300,
      viewportHeight: 600,
      stickyTop: 120,
      hourHeight: 60,
    })).toEqual({ startMinute: 180, endMinute: 780 });
  });

  it("clamps to the day bounds", () => {
    expect(visibleMinuteRangeForScroll({
      scrollTop: 0,
      viewportHeight: 2000,
      stickyTop: 0,
      hourHeight: 60,
    })).toEqual({ startMinute: 0, endMinute: 1440 });
  });

  it("falls back to the full day for invalid geometry", () => {
    expect(visibleMinuteRangeForScroll({
      scrollTop: 0,
      viewportHeight: 0,
      stickyTop: 0,
      hourHeight: 60,
    })).toEqual({ startMinute: 0, endMinute: 1440 });
  });
});

describe("formatTimeLabel", () => {
  it("keeps 24-hour labels canonical", () => {
    expect(formatTimeLabel("9:05", "24h")).toBe("09:05");
    expect(formatTimeLabel("2026-05-21 16:30", "24h")).toBe("16:30");
  });

  it("formats 12-hour labels with meridiem", () => {
    expect(formatTimeLabel("00:00", "12h")).toBe("12 am");
    expect(formatTimeLabel("09:30", "12h")).toBe("9:30 am");
    expect(formatTimeLabel("16:45", "12h")).toBe("4:45 pm");
  });

  it("supports compact meridiem for narrow controls", () => {
    expect(formatTimeLabel("00:00", "12h", "compact")).toBe("12am");
    expect(formatTimeLabel("16:45", "12h", "compact")).toBe("4:45pm");
  });

  it("formats time ranges", () => {
    expect(formatTimeRange("09:00", "17:30", "12h")).toBe("9 am - 5:30 pm");
    expect(formatTimeRange("07:00", "09:30", "12h", "compact")).toBe("7 - 9:30am");
    expect(formatTimeRange("09:30", "12:30", "12h", "compact")).toBe("9:30am - 12:30pm");
    expect(formatTimeRange("13:00", "17:30", "12h", "compact")).toBe("1 - 5:30pm");
  });

  it("leaves malformed labels unchanged", () => {
    expect(formatTimeLabel("bad", "12h")).toBe("bad");
    expect(formatTimeLabel("24:00", "12h")).toBe("24:00");
  });
});

describe("getHourInTimezone", () => {
  it("uses the selected calendar time format", () => {
    const localTimezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    expect(getHourInTimezone(new Date(2026, 4, 21), 13, localTimezone, "12h")).toBe("1 pm");
  });
});

describe("eventsForDay", () => {
  const events: CalendarEvent[] = [
    evt({ id: "1", title: "A", start: "2026-03-13 09:00", end: "2026-03-13 10:00" }),
    evt({ id: "2", title: "B", start: "2026-03-14 09:00", end: "2026-03-14 10:00" }),
    evt({ id: "3", title: "C", start: "2026-03-13 14:00", end: "2026-03-13 15:00" }),
  ];

  it("filters events for a specific day", () => {
    const result = eventsForDay(events, new Date(2026, 2, 13));
    expect(result).toHaveLength(2);
    expect(result.map((e) => e.id)).toEqual(["1", "3"]);
  });

  it("returns empty array for days with no events", () => {
    const result = eventsForDay(events, new Date(2026, 2, 15));
    expect(result).toHaveLength(0);
  });

  it("includes cross-midnight events on the next day", () => {
    const crossEvents: CalendarEvent[] = [
      evt({ id: "1", title: "Night", start: "2026-03-13 22:00", end: "2026-03-14 02:00" }),
    ];
    // Should appear on both Mar 13 and Mar 14
    expect(eventsForDay(crossEvents, new Date(2026, 2, 13))).toHaveLength(1);
    expect(eventsForDay(crossEvents, new Date(2026, 2, 14))).toHaveLength(1);
    expect(eventsForDay(crossEvents, new Date(2026, 2, 15))).toHaveLength(0);
  });

  it("excludes events ending exactly at midnight of the queried day", () => {
    const crossEvents: CalendarEvent[] = [
      evt({ id: "1", title: "Until midnight", start: "2026-03-13 22:00", end: "2026-03-14 00:00" }),
    ];
    // Ends at midnight of Mar 14, so it should NOT appear on Mar 14
    expect(eventsForDay(crossEvents, new Date(2026, 2, 13))).toHaveLength(1);
    expect(eventsForDay(crossEvents, new Date(2026, 2, 14))).toHaveLength(0);
  });

  it("includes multi-day events on intermediate days", () => {
    const multiDay: CalendarEvent[] = [
      evt({ id: "1", title: "Long", start: "2026-03-13 22:00", end: "2026-03-16 06:00" }),
    ];
    expect(eventsForDay(multiDay, new Date(2026, 2, 13))).toHaveLength(1);
    expect(eventsForDay(multiDay, new Date(2026, 2, 14))).toHaveLength(1);
    expect(eventsForDay(multiDay, new Date(2026, 2, 15))).toHaveLength(1);
    expect(eventsForDay(multiDay, new Date(2026, 2, 16))).toHaveLength(1);
    expect(eventsForDay(multiDay, new Date(2026, 2, 17))).toHaveLength(0);
  });
});

describe("effectiveMinuteRange", () => {
  it("returns actual minutes for same-day events", () => {
    const event: CalendarEvent = evt({
      id: "1", title: "A", start: "2026-03-13 09:00", end: "2026-03-13 11:00",
    });
    const range = effectiveMinuteRange(event, "2026-03-13");
    expect(range.startMinute).toBe(540);
    expect(range.endMinute).toBe(660);
  });

  it("clips cross-midnight event on start day", () => {
    const event: CalendarEvent = evt({
      id: "1", title: "Night", start: "2026-03-13 22:00", end: "2026-03-14 02:00",
    });
    const range = effectiveMinuteRange(event, "2026-03-13");
    expect(range.startMinute).toBe(1320); // 22:00
    expect(range.endMinute).toBe(1440); // fills to bottom
  });

  it("clips cross-midnight event on end day", () => {
    const event: CalendarEvent = evt({
      id: "1", title: "Night", start: "2026-03-13 22:00", end: "2026-03-14 02:00",
    });
    const range = effectiveMinuteRange(event, "2026-03-14");
    expect(range.startMinute).toBe(0); // starts at top
    expect(range.endMinute).toBe(120); // 02:00
  });

  it("fills full day for intermediate days of multi-day events", () => {
    const event: CalendarEvent = evt({
      id: "1", title: "Long", start: "2026-03-13 22:00", end: "2026-03-16 06:00",
    });
    const range = effectiveMinuteRange(event, "2026-03-14");
    expect(range.startMinute).toBe(0);
    expect(range.endMinute).toBe(1440);
  });
});

describe("minuteOffsetToDateStr", () => {
  it("returns same day for normal minutes", () => {
    expect(minuteOffsetToDateStr("2026-03-13", 540)).toBe("2026-03-13 09:00");
  });

  it("rolls to next day for minutes > 1440", () => {
    expect(minuteOffsetToDateStr("2026-03-13", 1560)).toBe("2026-03-14 02:00");
  });

  it("handles exactly midnight (1440)", () => {
    expect(minuteOffsetToDateStr("2026-03-13", 1440)).toBe("2026-03-14 00:00");
  });

  it("handles month boundary rollover", () => {
    expect(minuteOffsetToDateStr("2026-01-31", 1500)).toBe("2026-02-01 01:00");
  });
});

describe("layoutEventsForDay", () => {
  it("positions a single event correctly", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-13 09:00", end: "2026-03-13 10:00" }),
    ];
    const layout = layoutEventsForDay(events, "2026-03-13");
    expect(layout).toHaveLength(1);
    expect(layout[0].left).toBe(0);
    expect(layout[0].width).toBe(88);
    expect(layout[0].column).toBe(0);
    expect(layout[0].totalColumns).toBe(1);
  });

  it("splits overlapping events into columns", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-13 09:00", end: "2026-03-13 11:00" }),
      evt({ id: "2", title: "B", start: "2026-03-13 10:00", end: "2026-03-13 12:00" }),
    ];
    const layout = layoutEventsForDay(events, "2026-03-13");
    expect(layout).toHaveLength(2);
    expect(layout[0].totalColumns).toBe(2);
    expect(layout[1].totalColumns).toBe(2);
    expect(layout[0].width).toBe(44);
    expect(layout[1].width).toBe(44);
  });

  it("returns empty array for no events", () => {
    expect(layoutEventsForDay([], "2026-03-13")).toEqual([]);
  });

  it("positions cross-midnight event on start day from start to bottom", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "Night", start: "2026-03-13 22:00", end: "2026-03-14 02:00" }),
    ];
    const layout = layoutEventsForDay(events, "2026-03-13");
    expect(layout).toHaveLength(1);
    expect(layout[0].startMinute).toBe(1320); // 22:00 = minute 1320
    // Duration covers 22:00 to 24:00 = 120 minutes
    expect(layout[0].durationMinutes).toBe(120);
    expect(layout[0].isClippedTop).toBe(false);
    expect(layout[0].isClippedBottom).toBe(true);
  });

  it("positions cross-midnight event on end day from top to end", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "Night", start: "2026-03-13 22:00", end: "2026-03-14 02:00" }),
    ];
    const layout = layoutEventsForDay(events, "2026-03-14");
    expect(layout).toHaveLength(1);
    expect(layout[0].startMinute).toBe(0);
    // Duration covers 00:00 to 02:00 = 120 minutes
    expect(layout[0].durationMinutes).toBe(120);
    expect(layout[0].isClippedTop).toBe(true);
    expect(layout[0].isClippedBottom).toBe(false);
  });
});

// All-day event helpers

describe("eventsForDay excludes all-day events", () => {
  it("filters out all-day events", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "Timed", start: "2026-03-13 09:00", end: "2026-03-13 10:00" }),
      evt({ id: "2", title: "All day", start: "2026-03-13 00:00", end: "2026-03-13 00:00", allDay: true }),
    ];
    const result = eventsForDay(events, new Date(2026, 2, 13));
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("1");
  });
});

describe("allDayEventsForDay", () => {
  const events: CalendarEvent[] = [
    evt({ id: "1", title: "Timed", start: "2026-03-13 09:00", end: "2026-03-13 10:00" }),
    evt({ id: "2", title: "All day", start: "2026-03-13 00:00", end: "2026-03-13 00:00", allDay: true }),
    evt({ id: "3", title: "Multi day", start: "2026-03-12 00:00", end: "2026-03-14 00:00", allDay: true }),
  ];

  it("returns only all-day events for the given date", () => {
    const result = allDayEventsForDay(events, new Date(2026, 2, 13));
    expect(result).toHaveLength(2);
    expect(result.map((e) => e.id).sort()).toEqual(["2", "3"]);
  });

  it("does not return timed events", () => {
    const result = allDayEventsForDay(events, new Date(2026, 2, 13));
    expect(result.every((e) => e.allDay)).toBe(true);
  });

  it("returns empty for days without all-day events", () => {
    const result = allDayEventsForDay(events, new Date(2026, 2, 15));
    expect(result).toHaveLength(0);
  });

  it("handles multi-day all-day events on intermediate days", () => {
    const result = allDayEventsForDay(events, new Date(2026, 2, 12));
    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("3");
  });
});

describe("allDayEventsForWeek", () => {
  const weekDays = [
    new Date(2026, 2, 9), new Date(2026, 2, 10), new Date(2026, 2, 11),
    new Date(2026, 2, 12), new Date(2026, 2, 13), new Date(2026, 2, 14),
    new Date(2026, 2, 15),
  ];

  const events: CalendarEvent[] = [
    evt({ id: "1", title: "In week", start: "2026-03-11 00:00", end: "2026-03-11 00:00", allDay: true }),
    evt({ id: "2", title: "Before week", start: "2026-03-08 00:00", end: "2026-03-08 00:00", allDay: true }),
    evt({ id: "3", title: "Spanning", start: "2026-03-08 00:00", end: "2026-03-10 00:00", allDay: true }),
    evt({ id: "4", title: "Timed", start: "2026-03-11 09:00", end: "2026-03-11 10:00" }),
  ];

  it("returns all-day events overlapping the week", () => {
    const result = allDayEventsForWeek(events, weekDays);
    expect(result.map((e) => e.id).sort()).toEqual(["1", "3"]);
  });

  it("excludes events entirely before or after the week", () => {
    const result = allDayEventsForWeek(events, weekDays);
    expect(result.find((e) => e.id === "2")).toBeUndefined();
  });

  it("excludes timed events", () => {
    const result = allDayEventsForWeek(events, weekDays);
    expect(result.find((e) => e.id === "4")).toBeUndefined();
  });
});

describe("layoutAllDayEventsForWeek", () => {
  const weekDays = [
    new Date(2026, 2, 9), new Date(2026, 2, 10), new Date(2026, 2, 11),
    new Date(2026, 2, 12), new Date(2026, 2, 13), new Date(2026, 2, 14),
    new Date(2026, 2, 15),
  ];

  it("places a single-day event in row 0 with span 1", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-11 00:00", end: "2026-03-11 00:00", allDay: true }),
    ];
    const result = layoutAllDayEventsForWeek(events, weekDays);
    expect(result).toHaveLength(1);
    expect(result[0].row).toBe(0);
    expect(result[0].startCol).toBe(2); // Wed = index 2
    expect(result[0].spanCols).toBe(1);
  });

  it("spans a multi-day event across correct columns", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-10 00:00", end: "2026-03-12 00:00", allDay: true }),
    ];
    const result = layoutAllDayEventsForWeek(events, weekDays);
    expect(result).toHaveLength(1);
    expect(result[0].startCol).toBe(1); // Tue
    expect(result[0].spanCols).toBe(3); // Tue-Wed-Thu
  });

  it("stacks overlapping events in separate rows", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-11 00:00", end: "2026-03-13 00:00", allDay: true }),
      evt({ id: "2", title: "B", start: "2026-03-12 00:00", end: "2026-03-12 00:00", allDay: true }),
    ];
    const result = layoutAllDayEventsForWeek(events, weekDays);
    expect(result).toHaveLength(2);
    const rows = result.map((r) => r.row).sort();
    expect(rows).toEqual([0, 1]);
  });

  it("clips events that extend beyond the week", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-07 00:00", end: "2026-03-11 00:00", allDay: true }),
    ];
    const result = layoutAllDayEventsForWeek(events, weekDays);
    expect(result).toHaveLength(1);
    expect(result[0].startCol).toBe(0); // Clipped to Mon
    expect(result[0].spanCols).toBe(3); // Mon-Tue-Wed
  });

  it("lays out all-day events across a five-day work range", () => {
    const workDays = weekDays.slice(0, 5);
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-10 00:00", end: "2026-03-13 00:00", allDay: true }),
    ];
    const result = layoutAllDayEventsForWeek(events, workDays);
    expect(result).toHaveLength(1);
    expect(result[0].startCol).toBe(1);
    expect(result[0].spanCols).toBe(4);
  });

  it("lays out all-day events across a two-day weekend range", () => {
    const weekendDays = weekDays.slice(5);
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-13 00:00", end: "2026-03-15 00:00", allDay: true }),
    ];
    const result = layoutAllDayEventsForWeek(events, weekendDays);
    expect(result).toHaveLength(1);
    expect(result[0].startCol).toBe(0);
    expect(result[0].spanCols).toBe(2);
  });

  it("excludes events entirely outside the week", () => {
    const events: CalendarEvent[] = [
      evt({ id: "1", title: "A", start: "2026-03-01 00:00", end: "2026-03-05 00:00", allDay: true }),
    ];
    const result = layoutAllDayEventsForWeek(events, weekDays);
    expect(result).toHaveLength(0);
  });

  it("returns empty for no events", () => {
    const result = layoutAllDayEventsForWeek([], weekDays);
    expect(result).toHaveLength(0);
  });
});

describe("sanitizeCalendarTime", () => {
  it("passes through valid times unchanged", () => {
    expect(sanitizeCalendarTime("2026-03-13 14:30")).toBe("2026-03-13 14:30");
    expect(sanitizeCalendarTime("2026-01-01 00:00")).toBe("2026-01-01 00:00");
    expect(sanitizeCalendarTime("2026-03-13 14:30:45")).toBe("2026-03-13 14:30:45");
  });

  it("rounds floating point minutes", () => {
    expect(sanitizeCalendarTime("2026-03-13 14:30.5")).toBe("2026-03-13 14:31");
    expect(sanitizeCalendarTime("2026-03-13 14:30.4")).toBe("2026-03-13 14:30");
    expect(sanitizeCalendarTime("2026-03-13 14:30.14999999999977")).toBe("2026-03-13 14:30");
    expect(sanitizeCalendarTime("2026-03-13 13:23.14999999999977")).toBe("2026-03-13 13:23");
  });

  it("rounds floating point hours", () => {
    expect(sanitizeCalendarTime("2026-03-13 14.5:30")).toBe("2026-03-13 15:30");
  });

  it("clamps out of range values", () => {
    expect(sanitizeCalendarTime("2026-03-13 25:30")).toBe("2026-03-13 23:30");
    expect(sanitizeCalendarTime("2026-03-13 -1:30")).toBe("2026-03-13 00:30");
    expect(sanitizeCalendarTime("2026-03-13 14:70")).toBe("2026-03-13 14:59");
    expect(sanitizeCalendarTime("2026-03-13 14:-5")).toBe("2026-03-13 14:00");
    expect(sanitizeCalendarTime("2026-03-13 14:30:70")).toBe("2026-03-13 14:30:59");
  });

  it("defaults missing time to 00:00", () => {
    expect(sanitizeCalendarTime("2026-03-13")).toBe("2026-03-13 00:00");
  });

  it("returns null for invalid inputs", () => {
    expect(sanitizeCalendarTime("")).toBe(null);
    expect(sanitizeCalendarTime("invalid")).toBe(null);
    expect(sanitizeCalendarTime("2026/03/13 14:30")).toBe(null);
    expect(sanitizeCalendarTime(null as unknown as string)).toBe(null);
    expect(sanitizeCalendarTime(undefined as unknown as string)).toBe(null);
  });

  it("handles edge cases", () => {
    expect(sanitizeCalendarTime("2026-03-13 00:00")).toBe("2026-03-13 00:00");
    expect(sanitizeCalendarTime("2026-03-13 23:59")).toBe("2026-03-13 23:59");
    expect(sanitizeCalendarTime("  2026-03-13 14:30  ")).toBe("2026-03-13 14:30"); // trims whitespace
  });

  it("rolls exact 24:00 end-of-day input to next-day midnight", () => {
    expect(sanitizeCalendarTime("2026-03-13 24:00")).toBe("2026-03-14 00:00");
    expect(sanitizeCalendarTime("2026-03-13 24:00:00")).toBe("2026-03-14 00:00:00");
  });
});

describe("normalizeEventColor", () => {
  let warn: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    warn.mockRestore();
  });

  it("accepts integer indices in range unchanged", () => {
    expect(normalizeEventColor(0)).toBe(0);
    expect(normalizeEventColor(22)).toBe(22);
    expect(normalizeEventColor(23)).toBe(23);
    expect(warn).not.toHaveBeenCalled();
  });

  it("accepts numeric strings that parse to in-range integers", () => {
    expect(normalizeEventColor("0")).toBe(0);
    expect(normalizeEventColor("14")).toBe(14);
    expect(warn).not.toHaveBeenCalled();
  });

  it("returns undefined for null, undefined, and empty string without warning", () => {
    expect(normalizeEventColor(null)).toBeUndefined();
    expect(normalizeEventColor(undefined)).toBeUndefined();
    expect(normalizeEventColor("")).toBeUndefined();
    expect(warn).not.toHaveBeenCalled();
  });

  it("returns undefined and warns once for non-numeric strings", () => {
    expect(normalizeEventColor("not-a-color-xyz")).toBeUndefined();
    expect(normalizeEventColor("not-a-color-xyz")).toBeUndefined();
    expect(warn).toHaveBeenCalledTimes(1);
  });

  it("rejects out-of-range numbers", () => {
    expect(normalizeEventColor(-1)).toBeUndefined();
    expect(normalizeEventColor(32)).toBeUndefined();
    expect(normalizeEventColor(99)).toBeUndefined();
  });

  it("rejects non-integer numbers", () => {
    expect(normalizeEventColor(1.5)).toBeUndefined();
    expect(normalizeEventColor(Number.NaN)).toBeUndefined();
    expect(normalizeEventColor(Number.POSITIVE_INFINITY)).toBeUndefined();
  });

  it("rejects non-numeric values with a warning", () => {
    expect(normalizeEventColor({})).toBeUndefined();
    expect(normalizeEventColor([])).toBeUndefined();
    expect(normalizeEventColor(true)).toBeUndefined();
  });

  it("rejects non-numeric strings", () => {
    expect(normalizeEventColor("not-a-number")).toBeUndefined();
    expect(normalizeEventColor("abc")).toBeUndefined();
    expect(normalizeEventColor("__proto__")).toBeUndefined();
  });
});

describe("EVENT_COLOR_OPTIONS", () => {
  it("contains every palette index exactly once", () => {
    expect(EVENT_COLOR_OPTIONS).toHaveLength(PALETTE_SIZE);
    expect(new Set(EVENT_COLOR_OPTIONS).size).toBe(EVENT_COLOR_OPTIONS.length);
    for (const color of EVENT_COLOR_OPTIONS) {
      expect(normalizeEventColor(color)).toBe(color);
    }
    expect([...EVENT_COLOR_OPTIONS].sort((a, b) => a - b)).toEqual(
      Array.from({ length: PALETTE_SIZE }, (_, i) => i),
    );
  });

  it("includes the fallback index", () => {
    expect(EVENT_COLOR_OPTIONS).toContain(FALLBACK_COLOR_INDEX);
  });
});

const HEX = /^#[0-9a-fA-F]{6}$/;

function assertColorEntry(entry: { bg: string; text: string }): void {
  expect(entry.bg).toMatch(HEX);
  expect(entry.text).toMatch(HEX);
}

describe("getEventColor", () => {
  it("returns the theme's palette entry for a known slot", () => {
    const entry = getEventColor(2, lightTheme);
    expect(entry.bg.toLowerCase()).toBe(lightTheme.eventPalette[2].toLowerCase());
    assertColorEntry(entry);
  });

  it("returns the fallback slot for undefined color (render fallback)", () => {
    const entry = getEventColor(undefined, lightTheme);
    expect(entry.bg.toLowerCase()).toBe(
      lightTheme.eventPalette[FALLBACK_COLOR_INDEX].toLowerCase(),
    );
  });

  it("returns different hex values across light and dark themes for the same slot", () => {
    const light = getEventColor(14, lightTheme);
    const dark = getEventColor(14, darkTheme);
    expect(light.bg.toLowerCase()).not.toBe(dark.bg.toLowerCase());
  });

  it("resolves every EVENT_COLOR_OPTIONS entry under both built-in themes", () => {
    for (const theme of [lightTheme, darkTheme]) {
      for (const color of EVENT_COLOR_OPTIONS) {
        assertColorEntry(getEventColor(color, theme));
      }
    }
  });

  it("resolves palettes for a custom theme as long as all slots are filled", () => {
    const customPalette = [...darkTheme.eventPalette];
    customPalette[2] = "#123456";
    const customTheme: Theme = {
      kind: "builtin",
      id: "custom-test",
      displayName: "Custom Test",
      base: "dark",
      iconLabel: "dark",
      blendCanvas: "#000000",
      eventPalette: customPalette,
    };
    const entry = getEventColor(2, customTheme);
    expect(entry.bg.toLowerCase()).toBe("#123456");
  });
});

describe("dimmed color variants", () => {
  it("past and outside-month return valid hex entries", () => {
    for (const theme of [lightTheme, darkTheme]) {
      assertColorEntry(getPastEventColor(2, theme));
      assertColorEntry(getOutsideMonthEventColor(2, theme));
    }
  });

  it("dimmed variants differ from the base color", () => {
    const base = getEventColor(2, lightTheme);
    const past = getPastEventColor(2, lightTheme);
    expect(past.bg.toLowerCase()).not.toBe(base.bg.toLowerCase());
  });

  it("outside-month is more washed out than past (lower main weight)", () => {
    // Outside-month uses 0.25 main weight; past uses 0.3 (light theme).
    // The blended bg should therefore be closer to the canvas color.
    const past = getPastEventColor(2, lightTheme);
    const outside = getOutsideMonthEventColor(2, lightTheme);
    expect(past.bg).not.toBe(outside.bg);
  });

  it("preserves palette alpha when computing dimmed variants", () => {
    const customPalette = [...lightTheme.eventPalette];
    customPalette[2] = "#ff000080";
    const customTheme: Theme = {
      kind: "builtin",
      id: "custom-alpha-test",
      displayName: "Custom alpha test",
      base: "light",
      iconLabel: "light",
      blendCanvas: "#ffffff",
      eventPalette: customPalette,
    };
    expect(getPastEventColor(2, customTheme).bg).toBe("#ffb3b380");
    expect(getOutsideMonthEventColor(2, customTheme).bg.endsWith("80")).toBe(true);
  });

  it("falls back to the dimmed fallback slot when color is undefined", () => {
    assertColorEntry(getPastEventColor(undefined, lightTheme));
    assertColorEntry(getOutsideMonthEventColor(undefined, darkTheme));
  });
});

describe("getEventStatusPatternClass", () => {
  it("returns no pattern for ordinary confirmed events or free availability", () => {
    expect(getEventStatusPatternClass({ status: "confirmed", transparency: "opaque" })).toBe("");
    expect(getEventStatusPatternClass({ status: "confirmed", transparency: "transparent" })).toBe("");
    expect(getEventStatusPatternClass({})).toBe("");
  });

  it("uses vertical pinstripes for tentative events", () => {
    const patternClass = getEventStatusPatternClass({
      status: "tentative",
      transparency: "transparent",
    });
    expect(patternClass).toBe("event-pattern-tentative");
  });

  it("uses the strongest cancelled pattern before availability", () => {
    const patternClass = getEventStatusPatternClass({
      status: "cancelled",
      transparency: "transparent",
    });
    expect(patternClass).toBe("event-pattern-declined");
  });

  it("keeps event-level cancelled stronger than RSVP surface status", () => {
    const patternClass = getEventStatusPatternClass({
      status: "cancelled",
      surfaceStatus: "accepted",
    });
    expect(patternClass).toBe("event-pattern-declined");
  });

  it("uses dots for pending RSVP state", () => {
    const patternClass = getEventStatusPatternClass({
      surfaceStatus: "needs-action",
    });
    expect(patternClass).toBe("event-pattern-pending");
  });

  it("uses RSVP surface status before event-level tentative", () => {
    const patternClass = getEventStatusPatternClass({
      status: "tentative",
      surfaceStatus: "accepted",
    });
    expect(patternClass).toBe("");
  });

  it("treats declined RSVP as a cancelled surface", () => {
    expect(isEventSurfaceCancelled({ surfaceStatus: "declined" })).toBe(true);
    expect(isEventSurfaceCancelled({ status: "cancelled" })).toBe(true);
    expect(isEventSurfaceCancelled({ surfaceStatus: "accepted", status: "cancelled" })).toBe(true);
  });
});

describe("getEventSurfaceStatusForIdentity", () => {
  it("returns the matching attendee RSVP status case-insensitively", () => {
    expect(getEventSurfaceStatusForIdentity({
      surfaceAttendees: [
        { email: "other@example.com", status: "accepted" },
        { email: "Person@Example.com", status: "tentative" },
      ],
    }, "person@example.com")).toBe("tentative");
  });

  it("falls back to local participation status without an identity or matching attendee", () => {
    const event: Pick<CalendarEvent, "surfaceAttendees"> = {
      surfaceAttendees: [{ email: "other@example.com", status: "declined" }],
    };
    expect(getEventSurfaceStatusForIdentity(event, undefined)).toBeUndefined();
    expect(getEventSurfaceStatusForIdentity(event, "person@example.com")).toBeUndefined();
    expect(getEventSurfaceStatusForIdentity({
      ...event,
      localParticipationStatus: "needs-action",
    }, undefined)).toBe("needs-action");
    expect(getEventSurfaceStatusForIdentity({
      ...event,
      localParticipationStatus: "tentative",
    }, "person@example.com")).toBe("tentative");
  });
});

describe("getTimezoneCity", () => {
  it("returns the last segment with underscores converted to spaces", () => {
    expect(getTimezoneCity("Asia/Tehran")).toBe("Tehran");
    expect(getTimezoneCity("America/New_York")).toBe("New York");
    expect(getTimezoneCity("America/Argentina/Buenos_Aires")).toBe("Buenos Aires");
  });

  it("returns the input itself for single-segment IDs", () => {
    expect(getTimezoneCity("UTC")).toBe("UTC");
  });
});

describe("getTimezoneRegion", () => {
  it("returns the first segment of an IANA ID", () => {
    expect(getTimezoneRegion("Asia/Tehran")).toBe("Asia");
    expect(getTimezoneRegion("America/Argentina/Buenos_Aires")).toBe("America");
    expect(getTimezoneRegion("Europe/London")).toBe("Europe");
  });

  it("returns an empty string for single-segment IDs", () => {
    expect(getTimezoneRegion("UTC")).toBe("");
  });
});

describe("getTimezoneOffsetMinutes", () => {
  it("handles whole-hour positive offsets", () => {
    expect(getTimezoneOffsetMinutes("Asia/Tokyo")).toBe(9 * 60);
  });

  it("handles whole-hour negative offsets", () => {
    expect(getTimezoneOffsetMinutes("Pacific/Honolulu")).toBe(-10 * 60);
  });

  it("handles half-hour zones", () => {
    expect(getTimezoneOffsetMinutes("Asia/Kolkata")).toBe(5 * 60 + 30);
  });

  it("handles 45-minute zones", () => {
    expect(getTimezoneOffsetMinutes("Asia/Kathmandu")).toBe(5 * 60 + 45);
  });

  it("returns 0 for UTC", () => {
    expect(getTimezoneOffsetMinutes("UTC")).toBe(0);
  });
});

describe("deriveAcronymFromLongName", () => {
  it("derives standard 3-letter acronyms from typical names", () => {
    expect(deriveAcronymFromLongName("Korean Standard Time")).toBe("KST");
    expect(deriveAcronymFromLongName("Japan Standard Time")).toBe("JST");
    expect(deriveAcronymFromLongName("Iran Standard Time")).toBe("IST");
    expect(deriveAcronymFromLongName("West Africa Time")).toBe("WAT");
    expect(deriveAcronymFromLongName("British Summer Time")).toBe("BST");
  });

  it("derives 4-letter acronyms for compound names", () => {
    expect(deriveAcronymFromLongName("Australian Eastern Standard Time")).toBe(
      "AEST",
    );
    expect(deriveAcronymFromLongName("Central European Summer Time")).toBe(
      "CEST",
    );
    expect(deriveAcronymFromLongName("New Zealand Standard Time")).toBe("NZST");
    expect(deriveAcronymFromLongName("South Africa Standard Time")).toBe(
      "SAST",
    );
  });

  it("derives 5-letter acronyms when needed", () => {
    expect(
      deriveAcronymFromLongName("Australian Central Western Standard Time"),
    ).toBe("ACWST");
  });

  it("treats hyphenated regions as separate words", () => {
    expect(deriveAcronymFromLongName("Hawaii-Aleutian Standard Time")).toBe(
      "HAST",
    );
  });

  it("treats ampersand as a word boundary, not a letter", () => {
    expect(deriveAcronymFromLongName("Wallis & Futuna Time")).toBe("WFT");
    expect(deriveAcronymFromLongName("French Southern & Antarctic Time")).toBe(
      "FSAT",
    );
    expect(
      deriveAcronymFromLongName("St. Pierre & Miquelon Daylight Time"),
    ).toBe("SPMDT");
  });

  it("skips lowercase mid-name particles like 'de'", () => {
    expect(
      deriveAcronymFromLongName("Fernando de Noronha Standard Time"),
    ).toBe("FNST");
  });

  it("returns null when the long name is itself an offset form", () => {
    expect(deriveAcronymFromLongName("GMT+5:30")).toBeNull();
    expect(deriveAcronymFromLongName("UTC+14")).toBeNull();
    expect(deriveAcronymFromLongName("Coordinated Universal Time")).toBeNull();
  });

  it("returns null for empty or whitespace-only input", () => {
    expect(deriveAcronymFromLongName("")).toBeNull();
    expect(deriveAcronymFromLongName("   ")).toBeNull();
  });

  it("returns null when the derivation produces fewer than 2 letters", () => {
    expect(deriveAcronymFromLongName("Time")).toBeNull();
  });

  it("returns null when the derivation would exceed 5 letters", () => {
    expect(
      deriveAcronymFromLongName("Some Very Long Made Up Time Zone Name"),
    ).toBeNull();
  });

  it("skips the stop words 'of', 'the', 'and'", () => {
    expect(deriveAcronymFromLongName("Time of the East")).toBe("TE");
  });
});

describe("compactOffsetFromLong", () => {
  it("strips GMT prefix and leading zero hours", () => {
    expect(compactOffsetFromLong("GMT-06:00")).toBe("-6");
    expect(compactOffsetFromLong("GMT+09:00")).toBe("+9");
  });

  it("preserves non-zero minutes", () => {
    expect(compactOffsetFromLong("GMT+05:30")).toBe("+5:30");
    expect(compactOffsetFromLong("GMT+08:45")).toBe("+8:45");
    expect(compactOffsetFromLong("GMT-09:30")).toBe("-9:30");
  });

  it("keeps the sign on two-digit-hour offsets", () => {
    expect(compactOffsetFromLong("GMT+14:00")).toBe("+14");
    expect(compactOffsetFromLong("GMT-12:00")).toBe("-12");
  });

  it("returns +0 for the UTC zero point", () => {
    expect(compactOffsetFromLong("GMT")).toBe("+0");
    expect(compactOffsetFromLong("")).toBe("+0");
  });

  it("falls back to a stripped form for anything unexpected", () => {
    expect(compactOffsetFromLong("UTC+05:30")).toBe("UTC+05:30");
    expect(compactOffsetFromLong("GMT+5:30")).toBe("+5:30");
  });
});

describe("listAllTimezones", () => {
  it("excludes Etc/* zones", () => {
    const list = listAllTimezones();
    expect(list.every((tz) => !tz.startsWith("Etc/"))).toBe(true);
  });

  it("includes common multi-segment zones", () => {
    const list = listAllTimezones();
    expect(list).toContain("Asia/Tehran");
    expect(list).toContain("America/New_York");
    expect(list).toContain("Europe/London");
  });

  it("excludes deprecated single-segment aliases", () => {
    const list = listAllTimezones();
    expect(list).not.toContain("EST");
    expect(list).not.toContain("PST8PDT");
    expect(list).not.toContain("Iran");
    expect(list).not.toContain("Japan");
    expect(list).not.toContain("UTC");
  });
});

describe("searchTimezones", () => {
  it("returns the full filtered list for an empty query", () => {
    const result = searchTimezones("", []);
    const filtered = listAllTimezones();
    expect(result.length).toBe(filtered.length);
  });

  it("sorts the empty-query result by UTC offset ascending", () => {
    const result = searchTimezones("", []);
    for (let i = 1; i < result.length; i++) {
      const prev = getTimezoneInfo(result[i - 1]).offsetMinutes;
      const curr = getTimezoneInfo(result[i]).offsetMinutes;
      expect(prev).toBeLessThanOrEqual(curr);
    }
  });

  it("excludes already-active zones from results", () => {
    const result = searchTimezones("", ["Asia/Tehran", "America/New_York"]);
    expect(result).not.toContain("Asia/Tehran");
    expect(result).not.toContain("America/New_York");
  });

  it("excludes Etc/* zones from any query", () => {
    const empty = searchTimezones("", []);
    expect(empty.every((tz) => !tz.startsWith("Etc/"))).toBe(true);
    const named = searchTimezones("gmt", []);
    expect(named.every((tz) => !tz.startsWith("Etc/"))).toBe(true);
  });

  it("excludes deprecated single-segment aliases from any query", () => {
    const result = searchTimezones("", []);
    expect(result).not.toContain("EST");
    expect(result).not.toContain("Japan");
    expect(result).not.toContain("UTC");
  });

  it("finds zones by IANA city name", () => {
    expect(searchTimezones("tehran", [])).toContain("Asia/Tehran");
    expect(searchTimezones("chicago", [])).toContain("America/Chicago");
    expect(searchTimezones("tokyo", [])).toContain("Asia/Tokyo");
  });

  it("finds zones by region prefix", () => {
    const result = searchTimezones("asia", []);
    expect(result.some((tz) => tz.startsWith("Asia/"))).toBe(true);
    expect(result.length).toBeGreaterThan(10);
  });

  it("finds zones by Intl long-name match", () => {
    // "Iran Standard Time" matches Asia/Tehran via long name; the
    // deprecated "Iran" IANA alias is filtered out, so this can
    // only succeed via the long-name tier.
    expect(searchTimezones("iran", [])).toContain("Asia/Tehran");
  });

  it("ranks city-prefix matches above region-only matches", () => {
    // Tokyo's city prefix-matches at tier 2; many Asia/* zones only
    // match via the region "Asia" (tier 4-5) for longer queries. With
    // "tokyo" the only candidate is Asia/Tokyo so it must be first.
    const result = searchTimezones("tokyo", []);
    expect(result[0]).toBe("Asia/Tokyo");
  });

  it("returns matches with case-insensitive query", () => {
    const lower = searchTimezones("tehran", []);
    const upper = searchTimezones("TEHRAN", []);
    const mixed = searchTimezones("Tehran", []);
    expect(lower).toEqual(upper);
    expect(lower).toEqual(mixed);
  });

  it("returns an empty list for a query that matches nothing", () => {
    const result = searchTimezones("zzzzzqqqxxxnomatch", []);
    expect(result).toEqual([]);
  });
});

describe("computeViewWindow", () => {
  it("day mode: returns anchor day with 1-day margin", () => {
    const w = computeViewWindow(new Date(2026, 3, 29), "day");
    expect(w.start.toString()).toBe("2026-04-28");
    expect(w.end.toString()).toBe("2026-04-30");
  });

  it("week mode: returns Monday-Sunday plus margin", () => {
    // 2026-04-29 is a Wednesday. Monday = 04-27, Sunday = 05-03.
    const w = computeViewWindow(new Date(2026, 3, 29), "week");
    expect(w.start.toString()).toBe("2026-04-26");
    expect(w.end.toString()).toBe("2026-05-04");
  });

  it("week mode: anchor on Sunday yields the same week (Mon..Sun)", () => {
    // 2026-05-03 is a Sunday. startOfWeek -> 2026-04-27.
    const w = computeViewWindow(new Date(2026, 4, 3), "week");
    expect(w.start.toString()).toBe("2026-04-26");
    expect(w.end.toString()).toBe("2026-05-04");
  });

  it("workweek mode: weekday anchors return Monday-Friday plus margin", () => {
    const w = computeViewWindow(new Date(2026, 3, 29), "workweek");
    expect(w.start.toString()).toBe("2026-04-26");
    expect(w.end.toString()).toBe("2026-05-02");
  });

  it("workweek mode: weekend anchors return Saturday-Sunday plus margin", () => {
    const w = computeViewWindow(new Date(2026, 4, 3), "workweek");
    expect(w.start.toString()).toBe("2026-05-01");
    expect(w.end.toString()).toBe("2026-05-04");
  });

  it("month mode: returns 6x7 month grid plus margin", () => {
    // April 2026 grid: starts Mon 2026-03-30, ends Sun 2026-05-10.
    const w = computeViewWindow(new Date(2026, 3, 15), "month");
    expect(w.start.toString()).toBe("2026-03-29");
    expect(w.end.toString()).toBe("2026-05-11");
  });

  it("month mode: window reflects target month, not anchor day", () => {
    // Anchor on April 30 vs April 1 should produce identical windows
    // because both fall in April 2026.
    const a = computeViewWindow(new Date(2026, 3, 30), "month");
    const b = computeViewWindow(new Date(2026, 3, 1), "month");
    expect(a.start.toString()).toBe(b.start.toString());
    expect(a.end.toString()).toBe(b.end.toString());
  });
});
