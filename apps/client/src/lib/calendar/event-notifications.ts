import type { CalendarEvent } from "./types";
import { formatDatePart, formatTimeRange, parseCalendarDate } from "./utils";
import type { Translate } from "$lib/i18n/translator.svelte";

export type EventNotificationCopyInput = Pick<
  CalendarEvent,
  "start" | "end" | "allDay" | "location"
>;

function plural(value: number, singular: string, pluralLabel: string): string {
  return `${value} ${value === 1 ? singular : pluralLabel}`;
}

interface EventNotificationFormatOptions {
  readonly t?: Translate;
  readonly locale?: string | readonly string[];
}

function formatNotificationDate(
  date: Date,
  locale: string | readonly string[],
): string {
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(date);
}

export function formatEventNotificationLead(
  minutesUntil: number,
  t?: Translate,
): string {
  if (minutesUntil <= 0) {
    return t ? t("calendar.notification.startingNow") : "Starting now";
  }
  if (minutesUntil < 60) {
    if (t) return t("calendar.notification.startsInMinutes", minutesUntil);
    return `Starts in ${plural(minutesUntil, "minute", "minutes")}`;
  }

  if (minutesUntil >= 24 * 60) {
    const days = Math.round(minutesUntil / (24 * 60));
    if (t) return t("calendar.notification.startsInDays", days);
    return `Starts in ${plural(days, "day", "days")}`;
  }

  const hours = Math.floor(minutesUntil / 60);
  const minutes = minutesUntil % 60;
  if (minutes === 0) {
    if (t) return t("calendar.notification.startsInHours", hours);
    return `Starts in ${plural(hours, "hour", "hours")}`;
  }
  if (t) return t("calendar.notification.startsInHoursMinutes", hours, minutes);
  return `Starts in ${plural(hours, "hour", "hours")} ${plural(minutes, "minute", "minutes")}`;
}

export function formatEventNotificationBody(
  event: EventNotificationCopyInput,
  now: Date,
  options: EventNotificationFormatOptions = {},
): string {
  const { t } = options;
  const locale = options.locale ?? "en-US";
  const localized = t !== undefined || options.locale !== undefined;
  const startTime = parseCalendarDate(event.start);
  const minutesUntil = Math.round((startTime.getTime() - now.getTime()) / 60_000);
  const lines = [formatEventNotificationLead(minutesUntil, t)];
  const eventDate = formatDatePart(startTime);
  const today = formatDatePart(now);

  if (event.allDay) {
    if (eventDate === today) {
      lines.push(t ? t("calendar.notification.allDay") : "All day");
    } else {
      const date = localized ? formatNotificationDate(startTime, locale) : eventDate;
      lines.push(t ? t("calendar.notification.allDayOn", date) : `All day on ${date}`);
    }
  } else {
    const range = formatTimeRange(event.start, event.end, "24h", "compact");
    if (eventDate === today) {
      lines.push(range);
    } else {
      const date = localized ? formatNotificationDate(startTime, locale) : eventDate;
      lines.push(
        t ? t("calendar.notification.datedTimeRange", date, range) : `${date} ${range}`,
      );
    }
  }

  const location = event.location?.trim().replace(/\s+/g, " ");
  if (location) lines.push(location);

  return lines.join("\n");
}

/** How long before an event or date a notification fires, in words, such as "2 hours before" or "At start". */
export function formatNotificationOffset(minutes: number, t: Translate): string {
  if (minutes === 0) return t("calendar.notifications.atStart");
  if (minutes < 60) return t("calendar.notifications.minutesBefore", minutes);
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  if (minutes < 1440) {
    if (remainingMinutes === 0) return t("calendar.notifications.hoursBefore", hours);
    return t("calendar.notifications.compactHoursMinutesBefore", hours, remainingMinutes);
  }
  const days = Math.floor(minutes / 1440);
  if (minutes % 10080 === 0) return t("calendar.notifications.weeksBefore", minutes / 10080);
  if (minutes % 1440 === 0) return t("calendar.notifications.daysBefore", days);
  const remainingHours = Math.floor((minutes % 1440) / 60);
  return t("calendar.notifications.compactDaysHoursBefore", days, remainingHours);
}
