import type {
  CalendarEvent,
  CalendarViewMode,
  EventColor,
  PositionedAllDayEvent,
  PositionedEvent,
} from "./types";
import { Temporal } from "@js-temporal/polyfill";
import { FALLBACK_COLOR_INDEX, PALETTE_SIZE } from "./types";
import { blendHex } from "$lib/color/math";
import {
  DEFAULT_CALENDAR_TIME_FORMAT,
  type CalendarTimeFormat,
} from "$lib/stores/preference-options";

export { blendHex };
export { createSmoothScroll } from "./smooth-scroll";

const MIN_EVENT_HEIGHT = 4;

// Date parsing and formatting

export function parseCalendarDate(str: string): Date {
  // "YYYY-MM-DD HH:MM" or "YYYY-MM-DD HH:MM:SS" -> Date
  const [datePart, timePart] = str.split(" ");
  const [y, m, d] = datePart.split("-").map(Number);
  const [h, min, sec] = (timePart ?? "00:00").split(":").map(Number);
  return new Date(y, m - 1, d, h, min, sec || 0);
}

export function formatCalendarDate(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  const h = String(date.getHours()).padStart(2, "0");
  const min = String(date.getMinutes()).padStart(2, "0");
  return `${y}-${m}-${d} ${h}:${min}`;
}

export function formatCalendarDateWithSeconds(date: Date): string {
  const base = formatCalendarDate(date);
  const sec = String(date.getSeconds()).padStart(2, "0");
  return `${base}:${sec}`;
}

export function formatDatePart(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/**
 * Detect that a stored datetime string is already in UTC ISO 8601 form
 * (ends with Z, with or without seconds and millis). Row mapping uses it to
 * keep wall-clock-shaped values from malformed or hand-edited rows as text.
 */
export function isUtcIso(value: string): boolean {
  return typeof value === "string" && value.endsWith("Z") && value.includes("T");
}

/**
 * Convert a wall clock string ("YYYY-MM-DD HH:MM" or "YYYY-MM-DD HH:MM:SS")
 * interpreted in `zone` to a UTC ISO 8601 instant ending in Z. DST ambiguity
 * resolves with `disambiguation: "compatible"`: in fall-back ambiguity (the
 * 1:30 am that happens twice) pick the earlier instant; in spring-forward
 * gaps shift forward by the gap. Matches RFC 5545 conventions.
 */
export function wallClockToUtcIso(wallClock: string, zone: string): string {
  const normalized = wallClock.includes("T") ? wallClock : wallClock.replace(" ", "T");
  const padded = /T\d{2}:\d{2}$/.test(normalized) ? `${normalized}:00` : normalized;
  const plain = Temporal.PlainDateTime.from(padded);
  const zoned = plain.toZonedDateTime(zone, { disambiguation: "compatible" });
  return zoned.toInstant().toString();
}

/**
 * Convert a UTC ISO 8601 instant to a wall clock in `zone`. Minute precision
 * stays the default shape, but non-zero seconds are preserved for exact event
 * cuts that must stop scheduler behavior immediately.
 */
export function utcIsoToWallClock(utcIso: string, zone: string): string {
  const instant = Temporal.Instant.from(utcIso);
  const zoned = instant.toZonedDateTimeISO(zone);
  const y = String(zoned.year).padStart(4, "0");
  const m = String(zoned.month).padStart(2, "0");
  const d = String(zoned.day).padStart(2, "0");
  const h = String(zoned.hour).padStart(2, "0");
  const min = String(zoned.minute).padStart(2, "0");
  const base = `${y}-${m}-${d} ${h}:${min}`;
  if (zoned.second === 0 && zoned.millisecond === 0 && zoned.microsecond === 0 && zoned.nanosecond === 0) {
    return base;
  }
  return `${base}:${String(zoned.second).padStart(2, "0")}`;
}

/**
 * Sanitize a calendar time string to ensure clean "YYYY-MM-DD HH:MM" or
 * "YYYY-MM-DD HH:MM:SS" format.
 * Handles:
 * - Floating point minutes (rounds them)
 * - Explicit seconds (preserves them for exact cut timestamps)
 * - Missing time part (defaults to 00:00)
 * - Out of range values (clamps them)
 * Returns null if the input is completely invalid.
 */
export function sanitizeCalendarTime(str: string): string | null {
  if (typeof str !== "string" || !str.trim()) return null;

  const parts = str.trim().split(" ");
  const datePart = parts[0];
  const timePart = parts[1] ?? "00:00";

  const dateMatch = datePart.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!dateMatch) return null;

  // Parse time part, handling potential floats
  const timeParts = timePart.split(":");
  if (timeParts.length < 2) return null;

  let hour = parseFloat(timeParts[0]);
  let minute = parseFloat(timeParts[1]);
  let second = timeParts[2] !== undefined ? parseFloat(timeParts[2]) : null;

  if (isNaN(hour)) hour = 0;
  if (isNaN(minute)) minute = 0;
  if (second !== null && isNaN(second)) second = 0;

  const roundedMinute = Math.round(minute);
  const roundedSecond = second !== null ? Math.round(second) : null;

  if (hour === 24 && roundedMinute === 0 && (roundedSecond ?? 0) === 0) {
    const nextDay = parseCalendarDate(`${datePart} 00:00`);
    nextDay.setDate(nextDay.getDate() + 1);
    const nextDatePart = formatDatePart(nextDay);
    return roundedSecond === null ? `${nextDatePart} 00:00` : `${nextDatePart} 00:00:00`;
  }

  hour = Math.max(0, Math.min(23, Math.round(hour)));
  minute = Math.max(0, Math.min(59, roundedMinute));
  if (second !== null) second = Math.max(0, Math.min(59, roundedSecond ?? 0));

  const hh = String(hour).padStart(2, "0");
  const mm = String(minute).padStart(2, "0");
  const ss = second !== null ? String(second).padStart(2, "0") : null;

  return ss === null ? `${datePart} ${hh}:${mm}` : `${datePart} ${hh}:${mm}:${ss}`;
}

// Week / day helpers

export function startOfWeek(date: Date): Date {
  const d = new Date(date);
  const day = d.getDay();
  // Monday = 1, Sunday = 0 -> shift Sunday to 7
  const diff = (day === 0 ? 7 : day) - 1;
  d.setDate(d.getDate() - diff);
  d.setHours(0, 0, 0, 0);
  return d;
}

export function getWeekDays(anchorDate: Date): Date[] {
  const monday = startOfWeek(anchorDate);
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(monday);
    d.setDate(monday.getDate() + i);
    return d;
  });
}

export function workCycleRangeForDate(anchorDate: Date): { start: Date; end: Date } {
  const day = anchorDate.getDay();
  const start = new Date(anchorDate);
  const offset = day === 0 ? -1 : day === 6 ? 0 : 1 - day;
  start.setDate(anchorDate.getDate() + offset);
  start.setHours(0, 0, 0, 0);

  const end = new Date(start);
  end.setDate(start.getDate() + (start.getDay() === 6 ? 1 : 4));
  return { start, end };
}

export function getWorkCycleDays(anchorDate: Date): Date[] {
  const { start, end } = workCycleRangeForDate(anchorDate);
  const days: Date[] = [];
  const cursor = new Date(start);
  while (cursor <= end) {
    days.push(new Date(cursor));
    cursor.setDate(cursor.getDate() + 1);
  }
  return days;
}

export function adjacentWorkCycleAnchor(
  anchorDate: Date,
  direction: "back" | "forward",
): Date {
  const { start } = workCycleRangeForDate(anchorDate);
  const weekendRange = start.getDay() === 6;
  const delta = direction === "forward"
    ? weekendRange ? 2 : 5
    : weekendRange ? -5 : -2;
  return addDays(start, delta);
}

export function addDays(date: Date, n: number): Date {
  const d = new Date(date);
  d.setDate(d.getDate() + n);
  return d;
}

export function startOfDay(date: Date): Date {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  return d;
}

export function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export function isToday(date: Date): boolean {
  return isSameDay(date, new Date());
}

export function isPastDay(date: Date): boolean {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  return d.getTime() < today.getTime();
}

// Month helpers

export function getMonthGrid(year: number, month: number): Date[][] {
  const first = new Date(year, month, 1);
  const start = startOfWeek(first);
  const weeks: Date[][] = [];
  const cursor = new Date(start);
  for (let w = 0; w < 6; w++) {
    const week: Date[] = [];
    for (let d = 0; d < 7; d++) {
      week.push(new Date(cursor));
      cursor.setDate(cursor.getDate() + 1);
    }
    weeks.push(week);
  }
  return weeks;
}

/**
 * Visible date range for a given view mode anchored on `anchor`. Returns
 * Temporal.PlainDate bounds with a 1-day margin on each side so off-by-one
 * edges (DST, week boundaries) don't drop overlapping events. Used by the
 * calendar store's window-scoped expansion cache.
 *
 * - `day`: anchor day, plus margin.
 * - `week`: 7 days starting from week-start of anchor (Monday), plus margin.
 * - `month`: the 6x7 month grid, plus margin.
 */
export function computeViewWindow(
  anchor: Date,
  mode: CalendarViewMode,
): { start: Temporal.PlainDate; end: Temporal.PlainDate } {
  if (mode === "day") {
    const d = Temporal.PlainDate.from(formatDatePart(anchor));
    return { start: d.subtract({ days: 1 }), end: d.add({ days: 1 }) };
  }
  if (mode === "workweek") {
    const { start, end } = workCycleRangeForDate(anchor);
    return {
      start: Temporal.PlainDate.from(formatDatePart(start)).subtract({ days: 1 }),
      end: Temporal.PlainDate.from(formatDatePart(end)).add({ days: 1 }),
    };
  }
  if (mode === "week") {
    const monday = startOfWeek(anchor);
    const sunday = addDays(monday, 6);
    return {
      start: Temporal.PlainDate.from(formatDatePart(monday)).subtract({ days: 1 }),
      end: Temporal.PlainDate.from(formatDatePart(sunday)).add({ days: 1 }),
    };
  }
  const grid = getMonthGrid(anchor.getFullYear(), anchor.getMonth());
  const first = grid[0][0];
  const last = grid[5][6];
  return {
    start: Temporal.PlainDate.from(formatDatePart(first)).subtract({ days: 1 }),
    end: Temporal.PlainDate.from(formatDatePart(last)).add({ days: 1 }),
  };
}

// Time helpers

export function minuteOfDay(dateStr: string): number {
  const timePart = dateStr.split(" ")[1] ?? "00:00";
  const [h, m, s] = timePart.split(":").map(Number);
  return h * 60 + m + (s || 0) / 60;
}

export function durationMinutes(start: string, end: string): number {
  const s = parseCalendarDate(start);
  const e = parseCalendarDate(end);
  return Math.max(0, (e.getTime() - s.getTime()) / 60000);
}

function parseClockTime(value: string): { hour: number; minute: number } | undefined {
  const timePart = value.includes(" ") ? value.split(" ")[1] : value;
  const match = timePart.match(/^(\d{1,2}):(\d{2})(?::\d{2})?$/);
  if (!match) return undefined;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (!Number.isInteger(hour) || !Number.isInteger(minute)) return undefined;
  if (hour < 0 || hour > 23 || minute < 0 || minute > 59) return undefined;
  return { hour, minute };
}

export function formatTimeLabel(
  value: string,
  timeFormat: CalendarTimeFormat = DEFAULT_CALENDAR_TIME_FORMAT,
  variant: "short" | "compact" = "short",
): string {
  const parsed = parseClockTime(value);
  if (!parsed) return value;

  if (timeFormat === "24h") {
    return `${String(parsed.hour).padStart(2, "0")}:${String(parsed.minute).padStart(2, "0")}`;
  }

  const period = parsed.hour < 12 ? "am" : "pm";
  const displayHour = parsed.hour % 12 === 0 ? 12 : parsed.hour % 12;
  const minute = String(parsed.minute).padStart(2, "0");
  if (variant === "compact") {
    if (parsed.minute === 0) return `${displayHour}${period}`;
    return `${displayHour}:${minute}${period}`;
  }
  if (parsed.minute === 0) return `${displayHour} ${period}`;
  return `${displayHour}:${minute} ${period}`;
}

export function formatTimeRange(
  start: string,
  end: string,
  timeFormat: CalendarTimeFormat = DEFAULT_CALENDAR_TIME_FORMAT,
  variant: "short" | "compact" = "short",
): string {
  const startLabel = formatTimeLabel(start, timeFormat, variant);
  const endLabel = formatTimeLabel(end, timeFormat, variant);
  if (timeFormat !== "12h" || variant !== "compact") return `${startLabel} - ${endLabel}`;

  const startTime = parseClockTime(start);
  const endTime = parseClockTime(end);
  if (!startTime || !endTime) return `${startLabel} - ${endLabel}`;

  const startPeriod = startTime.hour < 12 ? "am" : "pm";
  const endPeriod = endTime.hour < 12 ? "am" : "pm";
  if (startPeriod !== endPeriod) return `${startLabel} - ${endLabel}`;

  return `${startLabel.slice(0, -startPeriod.length)} - ${endLabel}`;
}

export function formatMonthYear(
  date: Date,
  locale?: string | readonly string[],
  month: "long" | "short" = "long",
): string {
  return date.toLocaleDateString(locale, { month, year: "numeric" });
}

export type DayNameFormat = "long" | "short" | "narrow" | "none";

export function formatDayName(
  date: Date,
  format: DayNameFormat,
  locale?: string | readonly string[],
): string {
  if (format === "none") return "";
  return date.toLocaleDateString(locale, { weekday: format });
}

// Timezone helpers

export {
  compactOffsetFromLong,
  deriveAcronymFromLongName,
  formatTimezoneName,
  getHourInTimezone,
  getLocalTimezone,
  getTimezoneAbbr,
  getTimezoneCity,
  getTimezoneInfo,
  getTimezoneLongName,
  getTimezoneOffset,
  getTimezoneOffsetMinutes,
  getTimezoneRegion,
  GUTTER_WIDTH_PER_TIMEZONE,
  listAllTimezones,
  searchTimezones,
} from "./timezone-utils";
export type { TimezoneAbbrMode, TimezoneInfo } from "./timezone-utils";

export function snapToGrid(minute: number, gridMinutes: number = 10): number {
  return Math.round(minute / gridMinutes) * gridMinutes;
}

export function snapSimpleClickStartMinute(minute: number): number {
  const bounded = Math.max(0, Math.min(1439, minute));
  return Math.floor(bounded / 30) * 30;
}

export function clampMinute(minute: number): number {
  return Math.max(0, Math.min(1440, Math.round(minute)));
}

/**
 * Convert a scroll viewport over the timed calendar body into minute bounds.
 * Header chrome may occupy scroll space before the body in some layouts, so
 * callers can subtract that space before converting pixels to minutes.
 */
export function visibleMinuteRangeForScroll(opts: {
  scrollTop: number;
  viewportHeight: number;
  stickyTop: number;
  hourHeight: number;
}): { startMinute: number; endMinute: number } {
  const dayHeight = 24 * opts.hourHeight;
  if (
    opts.viewportHeight <= 0 ||
    opts.hourHeight <= 0 ||
    !Number.isFinite(dayHeight)
  ) {
    return { startMinute: 0, endMinute: 1440 };
  }
  const topPx = Math.max(0, opts.scrollTop - opts.stickyTop);
  const bottomPx = Math.max(
    topPx,
    Math.min(dayHeight, opts.scrollTop + opts.viewportHeight - opts.stickyTop),
  );
  return {
    startMinute: clampMinute((topPx / opts.hourHeight) * 60),
    endMinute: clampMinute((bottomPx / opts.hourHeight) * 60),
  };
}

// Cross-midnight helpers

/**
 * Returns the effective minute range for an event within a specific day.
 * For events that span midnight, this clips to [0, 1440] for the given day.
 */
export function effectiveMinuteRange(
  event: CalendarEvent,
  dateStr: string,
): { startMinute: number; endMinute: number } {
  const eventStartDate = event.start.split(" ")[0];
  const eventEndDate = event.end.split(" ")[0];

  const startMinute =
    eventStartDate < dateStr
      ? 0
      : eventStartDate === dateStr
        ? minuteOfDay(event.start)
        : 1440;

  const endMinute =
    eventEndDate > dateStr
      ? 1440
      : eventEndDate === dateStr
        ? minuteOfDay(event.end)
        : 0;

  return { startMinute, endMinute };
}

/**
 * Converts a base date string + minute offset to a full "YYYY-MM-DD HH:MM" string.
 * Handles overflow (minute > 1440 rolls to next day, negative rolls back).
 */
export function minuteOffsetToDateStr(
  baseDate: string,
  minute: number,
): string {
  const [y, m, d] = baseDate.split("-").map(Number);
  const base = new Date(y, m - 1, d, 0, 0, 0, 0);
  base.setMinutes(minute);
  return formatCalendarDate(base);
}

// Event filtering

export function eventsForDay(
  events: CalendarEvent[],
  date: Date,
): CalendarEvent[] {
  const dateStr = formatDatePart(date);
  const nextDateStr = formatDatePart(addDays(date, 1));
  const dayStart = `${dateStr} 00:00`;
  const dayEnd = `${nextDateStr} 00:00`;
  return events.filter((e) => !e.allDay && e.start < dayEnd && e.end > dayStart);
}

/** All events (all-day first, then timed) for a given day. Used by MonthView. */
export function allEventsForDay(
  events: CalendarEvent[],
  date: Date,
): CalendarEvent[] {
  const allDay = allDayEventsForDay(events, date).sort((a, b) => {
    const aTitle = (a.title || "").toLowerCase();
    const bTitle = (b.title || "").toLowerCase();
    if (aTitle !== bTitle) return aTitle < bTitle ? -1 : 1;
    return a.start.localeCompare(b.start);
  });
  const timed = eventsForDay(events, date).sort((a, b) => a.start.localeCompare(b.start));
  return [...allDay, ...timed];
}

export function allDayEventsForDay(
  events: CalendarEvent[],
  date: Date,
): CalendarEvent[] {
  const dateStr = formatDatePart(date);
  return events.filter((e) => {
    if (!e.allDay) return false;
    const startDate = e.start.split(" ")[0];
    const endDate = e.end.split(" ")[0];
    return startDate <= dateStr && endDate >= dateStr;
  }).sort((a, b) => {
    const aTitle = (a.title || "").toLowerCase();
    const bTitle = (b.title || "").toLowerCase();
    if (aTitle !== bTitle) return aTitle < bTitle ? -1 : 1;
    return a.start.localeCompare(b.start);
  });
}

export function allDayEventsForWeek(
  events: CalendarEvent[],
  weekDays: Date[],
): CalendarEvent[] {
  if (weekDays.length === 0) return [];
  const weekStart = formatDatePart(weekDays[0]);
  const weekEnd = formatDatePart(weekDays[weekDays.length - 1]);
  return events.filter((e) => {
    if (!e.allDay) return false;
    const startDate = e.start.split(" ")[0];
    const endDate = e.end.split(" ")[0];
    return startDate <= weekEnd && endDate >= weekStart;
  });
}

export function layoutAllDayEventsForWeek(
  events: CalendarEvent[],
  weekDays: Date[],
): PositionedAllDayEvent[] {
  const allDay = allDayEventsForWeek(events, weekDays);
  if (allDay.length === 0 || weekDays.length === 0) return [];

  const weekStart = formatDatePart(weekDays[0]);
  const weekEnd = formatDatePart(weekDays[weekDays.length - 1]);
  const dayStrs = weekDays.map(formatDatePart);

  // Sort alphabetically by title, then by start date as tiebreaker
  const sorted = [...allDay].sort((a, b) => {
    const aTitle = (a.title || "").toLowerCase();
    const bTitle = (b.title || "").toLowerCase();
    if (aTitle !== bTitle) return aTitle < bTitle ? -1 : 1;
    return a.start.localeCompare(b.start);
  });

  // rows[row][col] = true if occupied
  const rows: boolean[][] = [];
  const result: PositionedAllDayEvent[] = [];

  for (const event of sorted) {
    const startDate = event.start.split(" ")[0];
    const endDate = event.end.split(" ")[0];
    const clippedStart = startDate < weekStart ? weekStart : startDate;
    const clippedEnd = endDate > weekEnd ? weekEnd : endDate;

    const startCol = dayStrs.indexOf(clippedStart);
    const endCol = dayStrs.indexOf(clippedEnd);
    if (startCol < 0 || endCol < 0) continue;
    const spanCols = endCol - startCol + 1;

    // Find lowest row with no overlap
    let row = 0;
    outer: while (true) {
      if (!rows[row]) rows[row] = new Array(weekDays.length).fill(false);
      for (let c = startCol; c <= endCol; c++) {
        if (rows[row][c]) { row++; continue outer; }
      }
      break;
    }

    for (let c = startCol; c <= endCol; c++) {
      rows[row][c] = true;
    }

    result.push({ event, row, startCol, spanCols });
  }

  return result;
}

// Overlap layout algorithm

interface EventWithRange {
  event: CalendarEvent;
  startMinute: number;
  endMinute: number;
}

/** Minimum event duration in minutes that maps to MIN_EVENT_HEIGHT at default zoom. */
const MIN_EVENT_MINUTES = (MIN_EVENT_HEIGHT / 67) * 60;

export function layoutEventsForDay(
  events: CalendarEvent[],
  dateStr?: string,
): PositionedEvent[] {
  if (events.length === 0) return [];

  // Compute effective minute ranges for each event on this date
  const items: EventWithRange[] = events.map((event) => {
    if (dateStr) {
      return { event, ...effectiveMinuteRange(event, dateStr) };
    }
    // Fallback for callers that don't pass dateStr (e.g. month view)
    return {
      event,
      startMinute: minuteOfDay(event.start),
      endMinute: minuteOfDay(event.end),
    };
  });

  // Sort by start time, then by duration descending (longer first)
  const sorted = [...items].sort((a, b) => {
    const startDiff = a.startMinute - b.startMinute;
    if (startDiff !== 0) return startDiff;
    return (
      b.endMinute - b.startMinute - (a.endMinute - a.startMinute)
    );
  });

  // Build collision groups
  const groups: EventWithRange[][] = [];
  let currentGroup: EventWithRange[] = [];
  let groupEnd = 0;

  for (const item of sorted) {
    if (currentGroup.length > 0 && item.startMinute >= groupEnd) {
      groups.push(currentGroup);
      currentGroup = [];
      groupEnd = 0;
    }

    currentGroup.push(item);
    groupEnd = Math.max(groupEnd, item.endMinute);
  }
  if (currentGroup.length > 0) groups.push(currentGroup);

  // Build set of all start minutes for adjacency detection
  const startMinutes = new Set(sorted.map((item) => item.startMinute));

  // Assign columns within each group
  const result: PositionedEvent[] = [];

  for (const group of groups) {
    const columns: EventWithRange[][] = [];

    for (const item of group) {
      let placed = false;

      for (let colIdx = 0; colIdx < columns.length; colIdx++) {
        const col = columns[colIdx];
        const lastInCol = col[col.length - 1];
        if (item.startMinute >= lastInCol.endMinute) {
          col.push(item);
          placed = true;
          break;
        }
      }

      if (!placed) {
        columns.push([item]);
      }
    }

    const totalColumns = columns.length;
    const RIGHT_GUTTER_PCT = 12;
    const usable = 100 - RIGHT_GUTTER_PCT;

    for (let colIdx = 0; colIdx < columns.length; colIdx++) {
      for (const item of columns[colIdx]) {
        const dur = item.endMinute - item.startMinute;
        const left = (colIdx / totalColumns) * usable;
        const width = (1 / totalColumns) * usable;

        const isClippedTop = dateStr
          ? item.startMinute === 0 &&
            item.event.start.split(" ")[0] !== dateStr
          : false;
        const isClippedBottom = dateStr
          ? item.endMinute === 1440 &&
            item.event.end.split(" ")[0] !== dateStr
          : false;

        result.push({
          event: item.event,
          startMinute: item.startMinute,
          durationMinutes: Math.max(dur, MIN_EVENT_MINUTES),
          left,
          width,
          column: colIdx,
          totalColumns,
          isClippedTop,
          isClippedBottom,
          hasEventBelow: startMinutes.has(item.endMinute),
        });
      }
    }
  }

  return result;
}

// Event color palette
//
// Palette values live in the theme registry (see themes/definitions.ts). The
// functions below take a Theme and resolve the active palette into
// {bg, text} entries, applying contrast-aware text selection and caching
// the resolved palette per theme ID so switching themes is cheap.

import { isThemeCalendarDark, type Theme } from "$lib/themes";

export interface ColorEntry {
  bg: string;
  text: string;
}

// Contrast text tokens. Light-mode events default to white, pale swatches
// flip to near-black. Dark-mode events default to near-black, very-dark
// blended variants flip to near-white.
const LIGHT_TEXT = "#ffffff";
const LIGHT_TEXT_ALT = "#1f1f1f";
const DARK_TEXT = "#131314";
const DARK_TEXT_ALT = "#e3e3e3";

// Luminance flip thresholds tuned against the 32-color built-in palette.
// Light-mode bgs brighter than LIGHT_FLIP_ABOVE cannot carry white text;
// dark-mode bgs darker than DARK_FLIP_BELOW cannot carry near-black text.
// Every theme shares these thresholds for its light or dark calendar canvas;
// palettes outside the tuned range may need custom thresholds.
const LIGHT_FLIP_ABOVE = 0.65;
const DARK_FLIP_BELOW = 0.35;

/**
 * Pick a readable foreground for a given event-tile background under a
 * light or dark calendar surface. Rec. 709 coefficients on raw sRGB
 * approximate perceived luminance; calibrated for the event-palette
 * flips, so `resolvePalette` keeps using this. New callers should
 * prefer `pickReadableForeground` in `$lib/color/math.ts`, which uses
 * gamma-decoded WCAG luminance and guarantees AA contrast.
 *
 * The `darkCalendar` flag is resolved from the calendar canvas's
 * luminance (via `isThemeCalendarDark`), not from the theme's cosmetic
 * `base` label, so the base label does not affect tile text.
 *
 * @deprecated Use `pickReadableForeground` from `$lib/color/math`
 * for new code. This remains because the event palette is calibrated
 * against the threshold constants above.
 */
function pickContrastText(bg: string, darkCalendar: boolean): string {
  const r = parseInt(bg.slice(1, 3), 16);
  const g = parseInt(bg.slice(3, 5), 16);
  const b = parseInt(bg.slice(5, 7), 16);
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  if (darkCalendar) {
    return luminance < DARK_FLIP_BELOW ? DARK_TEXT_ALT : DARK_TEXT;
  }
  return luminance > LIGHT_FLIP_ABOVE ? LIGHT_TEXT_ALT : LIGHT_TEXT;
}

// Cache of resolved palettes keyed on the Theme object reference. Theme
// edits produce a fresh object (see `mergeThemePatch`), so a WeakMap lookup
// misses on edits and rebuilds the palette; old Theme objects get GC'd with
// their cache entries.
const resolvedPaletteCache = new WeakMap<Theme, ColorEntry[]>();

function resolvePalette(theme: Theme): ColorEntry[] {
  const cached = resolvedPaletteCache.get(theme);
  if (cached) return cached;
  const calendarIsDark = isThemeCalendarDark(theme);
  const out: ColorEntry[] = [];
  for (let i = 0; i < theme.eventPalette.length; i++) {
    const bg = theme.eventPalette[i];
    out.push({ bg, text: pickContrastText(bg, calendarIsDark) });
  }
  resolvedPaletteCache.set(theme, out);
  return out;
}

// Dedupes unknown-color warnings so a stray invalid value on many rows logs
// once per session.
const warnedUnknownColors = new Set<string>();

/**
 * Normalize a raw color value read from the database or an external import
 * into a valid EventColor (slot index 0..PALETTE_SIZE-1). Inputs outside
 * the range, or non-numeric inputs, log a single warning and return
 * undefined so the render layer falls back to the FALLBACK_COLOR_INDEX
 * slot without masking data drift.
 *
 * @param raw Value from an untrusted source (DB column, import file).
 * @returns A palette index, or undefined for null/empty/out-of-range inputs.
 */
export function normalizeEventColor(raw: unknown): EventColor | undefined {
  if (raw == null || raw === "") return undefined;
  let n: number;
  if (typeof raw === "number") {
    n = raw;
  } else if (typeof raw === "string") {
    n = Number(raw);
    if (!Number.isFinite(n)) {
      if (!warnedUnknownColors.has(raw)) {
        warnedUnknownColors.add(raw);
        console.warn(`[calendar] non-numeric event color "${raw}", falling back to default`);
      }
      return undefined;
    }
  } else {
    const key = String(raw);
    if (!warnedUnknownColors.has(key)) {
      warnedUnknownColors.add(key);
      console.warn("[calendar] non-numeric event color:", raw);
    }
    return undefined;
  }
  if (!Number.isInteger(n) || n < 0 || n >= PALETTE_SIZE) {
    const key = String(raw);
    if (!warnedUnknownColors.has(key)) {
      warnedUnknownColors.add(key);
      console.warn(`[calendar] event color ${raw} out of range, falling back to default`);
    }
    return undefined;
  }
  return n;
}

/**
 * Resolve an event color within a theme. Out-of-range or undefined slots
 * fall back to FALLBACK_COLOR_INDEX. Pass the active theme from the theme
 * store at the call site.
 */
export function getEventColor(
  color: EventColor | undefined,
  theme: Theme,
): ColorEntry {
  const palette = resolvePalette(theme);
  if (color !== undefined && palette[color]) return palette[color];
  return palette[FALLBACK_COLOR_INDEX];
}

// Cache for computed dimmed colors (past and outside-month).
// Keyed on the Theme reference so edits (which mint a new Theme object)
// naturally miss and recompute; old entries get GC'd with their themes.
const dimmedColorCache = new WeakMap<Theme, Map<string, ColorEntry>>();

/**
 * Compute a dimmed variant of an event color by blending the bg toward the
 * theme's blend canvas. Used in place of CSS opacity so contrast stays
 * predictable and text sub-pixel antialiasing is preserved.
 *
 * @param mainWeight Fraction of the event color in the result (0..1). The
 *   rest fills from the theme's blend canvas. Lower = more washed out.
 */
function getDimmedEventColor(
  color: EventColor | undefined,
  theme: Theme,
  mainWeight: number,
): ColorEntry {
  let inner = dimmedColorCache.get(theme);
  if (!inner) {
    inner = new Map();
    dimmedColorCache.set(theme, inner);
  }
  const key = `${color ?? FALLBACK_COLOR_INDEX}-${mainWeight}`;
  const cached = inner.get(key);
  if (cached) return cached;

  const base = getEventColor(color, theme);
  const bg = blendHex(base.bg, theme.blendCanvas, mainWeight);
  const text = pickContrastText(bg, isThemeCalendarDark(theme));
  const entry: ColorEntry = { bg, text };
  inner.set(key, entry);
  return entry;
}

/**
 * Past variant: heavier fade on light themes (30% main), lighter on dark
 * themes (50% main) to preserve legibility. Text contrast is recomputed
 * from the dimmed bg.
 */
export function getPastEventColor(
  color: EventColor | undefined,
  theme: Theme,
): ColorEntry {
  return getDimmedEventColor(color, theme, isThemeCalendarDark(theme) ? 0.5 : 0.3);
}

/**
 * Outside-month variant: strongest fade (~25% main). Used for events shown
 * in neighboring-month cells of the month grid.
 */
export function getOutsideMonthEventColor(
  color: EventColor | undefined,
  theme: Theme,
): ColorEntry {
  return getDimmedEventColor(color, theme, 0.25);
}

/**
 * Return the non-color status pattern class for event surfaces. Event-level
 * cancelled wins because it cancels the whole event. Otherwise, meeting RSVP
 * `surfaceStatus` drives the visible surface without changing event `STATUS`.
 * Availability (`TRANSP`) stays metadata-only because it is used for scheduling
 * free/busy checks, not as a visible event state.
 */
export function getEventStatusPatternClass(
  event: Pick<CalendarEvent, "status" | "surfaceStatus" | "transparency">,
): string {
  if (event.status === "cancelled") return "event-pattern-declined";
  const status = event.surfaceStatus ?? event.status;
  if (status === "declined") return "event-pattern-declined";
  if (status === "tentative") return "event-pattern-tentative";
  if (status === "needs-action" || status === "delegated") return "event-pattern-pending";
  return "";
}

export function isEventSurfaceCancelled(
  event: Pick<CalendarEvent, "status" | "surfaceStatus">,
): boolean {
  if (event.status === "cancelled") return true;
  const status = event.surfaceStatus ?? event.status;
  return status === "declined";
}

export function getEventSurfaceStatusForIdentity(
  event: Pick<CalendarEvent, "surfaceAttendees" | "localParticipationStatus">,
  identityEmail: string | undefined,
): CalendarEvent["surfaceStatus"] {
  const normalized = identityEmail?.trim().toLowerCase();
  if (!normalized) return event.localParticipationStatus;
  return event.surfaceAttendees?.find(
    (attendee) => attendee.email.toLowerCase() === normalized,
  )?.status ?? event.localParticipationStatus;
}

/**
 * Event-panel color picker order. Built-in palettes are stored in the same
 * visual order, so this can stay a natural sequence instead of a separate
 * display remap.
 */
export const EVENT_COLOR_OPTIONS: readonly EventColor[] = Object.freeze(
  Array.from({ length: PALETTE_SIZE }, (_, index) => index),
);
