import { Temporal } from "@js-temporal/polyfill";

export interface CalendarWindowRange {
  start: Temporal.PlainDate;
  end: Temporal.PlainDate;
}

export function calendarWindowCovers(
  outerStart: Temporal.PlainDate,
  outerEnd: Temporal.PlainDate,
  innerStart: Temporal.PlainDate,
  innerEnd: Temporal.PlainDate,
): boolean {
  return Temporal.PlainDate.compare(outerStart, innerStart) <= 0
    && Temporal.PlainDate.compare(outerEnd, innerEnd) >= 0;
}

/**
 * Calendar render windows include a one-day margin on both sides. Adjacent
 * prefetch must move by the visible view stride, not by the loaded span.
 */
export function adjacentCalendarWindowRequests(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
): CalendarWindowRange[] {
  const spanDays = windowStart.until(windowEnd).days + 1;
  if (!Number.isFinite(spanDays)) return [];
  if (spanDays === 3) return shiftWindowByDays(windowStart, windowEnd, 1);
  if (spanDays === 4) return adjacentWeekendWorkCycleWindows(windowStart, windowEnd);
  if (spanDays === 7) return adjacentWeekdayWorkCycleWindows(windowStart, windowEnd);
  if (spanDays === 9) return shiftWindowByDays(windowStart, windowEnd, 7);
  if (spanDays === 44) return adjacentMonthWindows(windowStart);
  return [];
}

function shiftWindowByDays(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
  days: number,
): CalendarWindowRange[] {
  return [
    {
      start: windowStart.add({ days }),
      end: windowEnd.add({ days }),
    },
    {
      start: windowStart.subtract({ days }),
      end: windowEnd.subtract({ days }),
    },
  ];
}

function adjacentWeekdayWorkCycleWindows(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
): CalendarWindowRange[] {
  return [
    {
      start: windowEnd.subtract({ days: 1 }),
      end: windowEnd.add({ days: 2 }),
    },
    {
      start: windowStart.subtract({ days: 2 }),
      end: windowStart.add({ days: 1 }),
    },
  ];
}

function adjacentWeekendWorkCycleWindows(
  windowStart: Temporal.PlainDate,
  windowEnd: Temporal.PlainDate,
): CalendarWindowRange[] {
  return [
    {
      start: windowEnd.subtract({ days: 1 }),
      end: windowEnd.add({ days: 5 }),
    },
    {
      start: windowStart.subtract({ days: 5 }),
      end: windowStart.add({ days: 1 }),
    },
  ];
}

function adjacentMonthWindows(windowStart: Temporal.PlainDate): CalendarWindowRange[] {
  const visibleGridStart = windowStart.add({ days: 1 });
  const anchorMonth = visibleGridStart.add({ days: 20 });
  return [
    monthWindow(anchorMonth.add({ months: 1 })),
    monthWindow(anchorMonth.subtract({ months: 1 })),
  ];
}

function monthWindow(anchor: Temporal.PlainDate): CalendarWindowRange {
  const firstOfMonth = Temporal.PlainDate.from({
    year: anchor.year,
    month: anchor.month,
    day: 1,
  });
  const gridStart = firstOfMonth.subtract({ days: firstOfMonth.dayOfWeek - 1 });
  const gridEnd = gridStart.add({ days: 41 });
  return {
    start: gridStart.subtract({ days: 1 }),
    end: gridEnd.add({ days: 1 }),
  };
}
