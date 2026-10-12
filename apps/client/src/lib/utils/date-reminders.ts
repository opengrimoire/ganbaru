import { Temporal } from "@js-temporal/polyfill";

/** Reminder offsets offered for a date with a time, in minutes before it. */
export const TIMED_DATE_REMINDER_OFFSETS: readonly number[] = [0, 5, 10, 15, 30, 60, 120, 1440, 2880, 10080];

/** Reminder offsets offered for a date without a time, in whole days so the reminder keeps its usual hour. */
export const DATE_ONLY_REMINDER_OFFSETS: readonly number[] = [0, 1440, 2880, 10080];

/** The local hour a reminder for a date without a time is anchored to. */
export const DATE_ONLY_REMINDER_HOUR = 9;

/** The reminder offsets a date offers, which depend on whether it has a time. */
export function dateReminderOffsets(timed: boolean): readonly number[] {
  return timed ? TIMED_DATE_REMINDER_OFFSETS : DATE_ONLY_REMINDER_OFFSETS;
}

/** An offset the date still offers, or the date itself when a change of time removed the chosen one. */
export function offeredDateReminderMinutes(minutes: number, timed: boolean): number {
  return dateReminderOffsets(timed).includes(minutes) ? minutes : 0;
}

/**
 * When a reminder for a floating wall date fires, in epoch milliseconds, or null for an invalid date or time.
 *
 * @param date The `YYYY-MM-DD` date.
 * @param time The `HH:MM` time, or null for a date without a time, which reminds at `dateOnlyHour`.
 * @param minutesBefore How many minutes before the date the reminder fires.
 * @param timeZone The IANA time zone the wall date is placed in.
 * @param dateOnlyHour The local hour a date without a time is anchored to.
 */
export function dateReminderDueAtMs(
  date: string,
  time: string | null,
  minutesBefore: number,
  timeZone: string,
  dateOnlyHour: number = DATE_ONLY_REMINDER_HOUR,
): number | null {
  try {
    const wallTime = time === null ? Temporal.PlainTime.from({ hour: dateOnlyHour }) : Temporal.PlainTime.from(time);
    const anchorMs = Temporal.PlainDate.from(date)
      .toPlainDateTime(wallTime)
      .toZonedDateTime(timeZone)
      .toInstant()
      .epochMilliseconds;
    return anchorMs - minutesBefore * 60_000;
  } catch {
    return null;
  }
}
