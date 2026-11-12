import { formatTimeLabel } from "$lib/calendar/utils";
import { CHAT_ALL_WEEKDAYS_MASK, type ChatQuietWindow, type ChatRecurrenceSpec } from "$lib/chat/contracts";
import { formatDateTime, formatList } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";
import { CHAT_WORKDAYS_MASK, chatLocalMinuteTime, chatWeekdaysOfMask } from "./message-scheduling";

/** A Monday at noon UTC, so weekday names format to the same day in every zone near UTC. */
const REFERENCE_MONDAY_MS = Date.UTC(2024, 0, 1, 12);
const DAY_MS = 86_400_000;

/** What the summaries need from the person's display settings. */
export interface ChatRecurrenceDisplay {
  t: Translate;
  locale: AppLocale;
  timeFormat: CalendarTimeFormat;
  /** IANA zone of this device; a rule in another zone names its zone. */
  localTimeZone: string;
}

/** Short localized names of the ISO weekdays a mask names, in week order. */
export function chatWeekdayNames(mask: number, locale: AppLocale): string[] {
  return chatWeekdaysOfMask(mask).map((day) => (
    formatDateTime(locale, REFERENCE_MONDAY_MS + (day - 1) * DAY_MS, { weekday: "short", timeZone: "UTC" })
  ));
}

/** The localized days of a weekday mask: every day, weekdays, or a list of day names. */
export function chatWeekdayMaskLabel(mask: number, display: ChatRecurrenceDisplay): string {
  if (mask === CHAT_ALL_WEEKDAYS_MASK) return display.t("chat.organization.repeat.everyDay");
  if (mask === CHAT_WORKDAYS_MASK) return display.t("chat.organization.repeat.mondayToFriday");
  return formatList(display.locale, chatWeekdayNames(mask, display.locale), { style: "short", type: "conjunction" });
}

/** A one-line localized description of when a repeating message posts. */
export function chatRecurrenceSummary(spec: ChatRecurrenceSpec, display: ChatRecurrenceDisplay): string {
  const { t } = display;
  const { rule } = spec;
  if (rule.kind === "interval") {
    return rule.minutes % 60 === 0
      ? t("chat.organization.repeat.everyHours", rule.minutes / 60)
      : t("chat.organization.repeat.everyMinutes", rule.minutes);
  }
  const time = formatTimeLabel(chatLocalMinuteTime(rule.localMinute), display.timeFormat);
  const summary = t("chat.organization.repeat.daysAt", chatWeekdayMaskLabel(rule.weekdays, display), time);
  return spec.timeZone === display.localTimeZone
    ? summary
    : t("chat.organization.repeat.inZone", summary, spec.timeZone);
}

/** A one-line localized description of a quiet window, such as "Every day, 22:00 to 07:00". */
export function chatQuietWindowSummary(window: ChatQuietWindow, display: ChatRecurrenceDisplay): string {
  return display.t(
    "settings.chat.quietPeriods.windowSummary",
    chatWeekdayMaskLabel(window.weekdays, display),
    formatTimeLabel(chatLocalMinuteTime(window.startMinute), display.timeFormat),
    formatTimeLabel(chatLocalMinuteTime(window.endMinute), display.timeFormat),
  );
}
