import type { AppLocale } from "./locales";
import type { Translate } from "./translator.svelte";

export function formatDateTime(
  locale: AppLocale,
  value: Date | number,
  options: Intl.DateTimeFormatOptions,
): string {
  return new Intl.DateTimeFormat(locale, options).format(value);
}

export function formatNumber(
  locale: AppLocale,
  value: number,
  options: Intl.NumberFormatOptions = {},
): string {
  return new Intl.NumberFormat(locale, options).format(value);
}

export function formatList(
  locale: AppLocale,
  values: readonly string[],
  options: Intl.ListFormatOptions = {},
): string {
  return new Intl.ListFormat(locale, options).format(values);
}

export function pluralCategory(
  locale: AppLocale,
  value: number,
  options: Intl.PluralRulesOptions = {},
): Intl.LDMLPluralRule {
  return new Intl.PluralRules(locale, options).select(value);
}

export function formatRelativeMinutes(
  t: Translate,
  minutesFromNow: number,
): string {
  const rounded = Math.round(minutesFromNow);
  if (rounded === 0) return t("format.relativeMinutesNow");
  if (rounded > 0) return t("format.relativeMinutesFuture", rounded);
  return t("format.relativeMinutesPast", Math.abs(rounded));
}

export function formatRelativeDays(
  t: Translate,
  daysFromNow: number,
): string {
  const rounded = Math.round(daysFromNow);
  if (rounded === 0) return t("format.relativeDaysToday");
  if (rounded > 0) return t("format.relativeDaysFuture", rounded);
  return t("format.relativeDaysPast", Math.abs(rounded));
}
