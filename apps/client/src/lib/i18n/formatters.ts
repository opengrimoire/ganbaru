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

/** One piece of a localized list: a value by its index in the input, or the separator text between values. */
export type ListPart = { type: "value"; index: number } | { type: "separator"; text: string };

/**
 * Splits a localized list into its values and separators, so each value can render as its own element, such as a
 * button, while the locale still decides the commas and conjunction around them.
 */
export function formatListParts(
  locale: AppLocale,
  values: readonly string[],
  options: Intl.ListFormatOptions = {},
): ListPart[] {
  let index = 0;
  return new Intl.ListFormat(locale, options).formatToParts(values).map((part): ListPart =>
    part.type === "element" ? { type: "value", index: index++ } : { type: "separator", text: part.value },
  );
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

/**
 * Wraps text from another person or device in first-strong isolate marks, so a right-to-left
 * name interpolated into a sentence keeps its own direction without reordering the sentence.
 */
export function isolateText(text: string): string {
  return `\u{2068}${text}\u{2069}`;
}
