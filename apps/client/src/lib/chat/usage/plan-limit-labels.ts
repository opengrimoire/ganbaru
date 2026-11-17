import type { AppLocale } from "$lib/i18n/locales";
import { formatDateTime, formatNumber } from "$lib/i18n/formatters";
import type { Translate } from "$lib/i18n/translator.svelte";
import { planLimitWindowName, type ProviderPlanLimitWindow } from "./provider-readiness";

/** Locale and translator a plan limit label is written with. */
export interface PlanLimitLabelContext {
  locale: AppLocale;
  t: Translate;
}

/**
 * Formats an instant as a time when it falls on the same local day as `nowMs`, or with the
 * weekday and day otherwise.
 */
export function planLimitInstantLabel(context: PlanLimitLabelContext, ms: number, nowMs: number): string {
  const sameDay = new Date(ms).toDateString() === new Date(nowMs).toDateString();
  return formatDateTime(context.locale, ms, sameDay
    ? { hour: "numeric", minute: "2-digit" }
    : { weekday: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

/** Writes one window as a single line, such as `5h 93% left (resets 20:00)`. */
export function planLimitWindowLine(
  context: PlanLimitLabelContext,
  window: ProviderPlanLimitWindow,
  nowMs: number,
): string {
  const { t } = context;
  const remaining = t(
    "chat.composer.rateLimitRemaining",
    formatNumber(context.locale, window.remainingPercent, { maximumFractionDigits: 0 }),
  );
  const line = `${planLimitWindowName(window, t("chat.composer.rateLimitWeek"))} ${remaining}`;
  return window.resetsAtMs === null
    ? line
    : `${line} (${t("chat.composer.rateLimitResets", planLimitInstantLabel(context, window.resetsAtMs, nowMs))})`;
}
