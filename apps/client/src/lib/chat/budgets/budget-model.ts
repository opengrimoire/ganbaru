import type { AppLocale } from "$lib/i18n/locales";
import { formatList, formatNumber } from "$lib/i18n/formatters";
import type {
  ChatAssignmentBudgetRead,
  ChatMessageRead,
  ChatWorkAssignmentId,
  UtcTimestamp,
} from "$lib/chat/contracts";

/** Smallest token limit the backend accepts. Mirrors the Rust bound. */
export const MIN_TOKEN_LIMIT = 1_000;
/** Largest token limit the backend accepts. Mirrors the Rust bound. */
export const MAX_TOKEN_LIMIT = 1_000_000_000_000;
/** Smallest working time limit, in seconds, the backend accepts. Mirrors the Rust bound. */
export const MIN_WORKING_SECONDS_LIMIT = 60;
/** Largest working time limit, in seconds, the backend accepts. Mirrors the Rust bound. */
export const MAX_WORKING_SECONDS_LIMIT = 8_640_000;
/** Percents of a set limit at which the reply thread shows a warning. Mirrors the Rust constants. */
export const BUDGET_WARNING_PERCENTS = [75, 90] as const;
/** Percent of a set limit at which the executing device stops the work. */
export const BUDGET_STOP_PERCENT = 100;

const SECONDS_PER_MINUTE = 60;
const MINUTES_PER_HOUR = 60;

/** How far usage has come toward one limit. */
export type BudgetThreshold = "none" | "warning75" | "warning90" | "reached";

/** Which limit a meter measures. */
export type BudgetMeterKind = "tokens" | "workingTime";

/** Usage of one set limit of an assignment budget. */
export interface BudgetMeter {
  kind: BudgetMeterKind;
  used: number;
  limit: number;
  threshold: BudgetThreshold;
  /** Used share of the limit, clamped to 0 through 1, for a progress bar. */
  fraction: number;
}

/** Why the latest failure of an assignment stopped it, when a limit did. */
export type ChatAssignmentLimitStop =
  | { kind: "budget"; limit: BudgetMeterKind; used: number; limitValue: number }
  | { kind: "provider"; resetsAt: UtcTimestamp | null };

/** What a limit field holds: nothing, a value within the bounds, or something the backend refuses. */
export type BudgetLimitInput =
  | { kind: "empty" }
  | { kind: "valid"; value: number }
  | { kind: "invalid" };

const THRESHOLD_RANK: Record<BudgetThreshold, number> = {
  none: 0,
  warning75: 1,
  warning90: 2,
  reached: 3,
};

/**
 * Returns the highest threshold `used` has reached of `limit`, with the integer comparison the
 * backend uses (`used * 100 >= limit * percent`) so every device agrees on the boundary.
 */
export function budgetThreshold(used: number, limit: number): BudgetThreshold {
  if (limit <= 0) return "none";
  // Recorded usage and limits stay at or below MAX_TOKEN_LIMIT, so both products are exact.
  const reached = (percent: number) => Math.max(0, used) * 100 >= limit * percent;
  if (reached(BUDGET_STOP_PERCENT)) return "reached";
  if (reached(BUDGET_WARNING_PERCENTS[1])) return "warning90";
  if (reached(BUDGET_WARNING_PERCENTS[0])) return "warning75";
  return "none";
}

/** Returns a meter for each limit the budget sets, tokens first. */
export function budgetMeters(budget: ChatAssignmentBudgetRead): BudgetMeter[] {
  const meters: BudgetMeter[] = [];
  if (budget.tokenLimit !== null) {
    meters.push(meter("tokens", budget.usedCountedTokens, budget.tokenLimit));
  }
  if (budget.workingSecondsLimit !== null) {
    meters.push(meter("workingTime", budget.usedWorkingSeconds, budget.workingSecondsLimit));
  }
  return meters;
}

/** Returns the meter closest to its limit that has passed a threshold, or `null` when none has. */
export function budgetAttention(meters: readonly BudgetMeter[]): BudgetMeter | null {
  let attention: BudgetMeter | null = null;
  for (const candidate of meters) {
    if (candidate.threshold === "none") continue;
    if (!attention || THRESHOLD_RANK[candidate.threshold] > THRESHOLD_RANK[attention.threshold]) {
      attention = candidate;
    }
  }
  return attention;
}

/** Returns the percent a warning threshold names, or `null` for the other thresholds. */
export function warningPercent(threshold: BudgetThreshold): number | null {
  if (threshold === "warning75") return BUDGET_WARNING_PERCENTS[0];
  if (threshold === "warning90") return BUDGET_WARNING_PERCENTS[1];
  return null;
}

/**
 * Reads the limit that stopped `assignmentId` from the newest failure update among `replies`.
 * A newer failure for another reason hides an older limit stop, and rich content another device
 * wrote that does not match the expected shape returns `null`.
 */
export function latestAssignmentLimitStop(
  replies: readonly ChatMessageRead[],
  assignmentId: ChatWorkAssignmentId,
): ChatAssignmentLimitStop | null {
  for (let index = replies.length - 1; index >= 0; index -= 1) {
    const value = record(replies[index]?.richContent.value);
    if (value?.type !== "agent_update" || value.updateKind !== "failure") continue;
    if (value.assignmentId !== assignmentId) continue;
    return limitStop(record(value.payload));
  }
  return null;
}

/** Formats a token count compactly in the locale, such as 12K or 1.2M. */
export function formatTokenCount(locale: AppLocale, value: number): string {
  return formatNumber(locale, value, { notation: "compact", maximumFractionDigits: 1 });
}

/** Formats working time in hours and minutes in the locale, or seconds below one minute. */
export function formatWorkingDuration(locale: AppLocale, seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  if (whole < SECONDS_PER_MINUTE) {
    return formatNumber(locale, whole, { style: "unit", unit: "second", unitDisplay: "short" });
  }
  const totalMinutes = Math.floor(whole / SECONDS_PER_MINUTE);
  const hours = Math.floor(totalMinutes / MINUTES_PER_HOUR);
  const minutes = totalMinutes % MINUTES_PER_HOUR;
  const parts: string[] = [];
  if (hours > 0) {
    parts.push(formatNumber(locale, hours, { style: "unit", unit: "hour", unitDisplay: "short" }));
  }
  if (minutes > 0 || hours === 0) {
    parts.push(formatNumber(locale, minutes, { style: "unit", unit: "minute", unitDisplay: "short" }));
  }
  return formatList(locale, parts, { type: "unit", style: "narrow" });
}

/** Formats a meter's used amount in its own unit. */
export function formatMeterAmount(locale: AppLocale, kind: BudgetMeterKind, value: number): string {
  return kind === "tokens" ? formatTokenCount(locale, value) : formatWorkingDuration(locale, value);
}

/** Parses a token limit field; grouping separators and spaces are ignored. */
export function parseTokenLimitInput(text: string): BudgetLimitInput {
  return parseBoundedInteger(text, MIN_TOKEN_LIMIT, MAX_TOKEN_LIMIT, 1);
}

/** Parses a working time field written in minutes into seconds. */
export function parseWorkingMinutesInput(text: string): BudgetLimitInput {
  return parseBoundedInteger(
    text,
    MIN_WORKING_SECONDS_LIMIT,
    MAX_WORKING_SECONDS_LIMIT,
    SECONDS_PER_MINUTE,
  );
}

/** Returns a limit in seconds as whole minutes for a minutes field, or an empty field for none. */
export function workingMinutesFieldValue(seconds: number | null): string {
  return seconds === null ? "" : String(Math.ceil(seconds / SECONDS_PER_MINUTE));
}

function meter(kind: BudgetMeterKind, used: number, limit: number): BudgetMeter {
  return {
    kind,
    used,
    limit,
    threshold: budgetThreshold(used, limit),
    fraction: limit > 0 ? Math.min(1, Math.max(0, used / limit)) : 0,
  };
}

function limitStop(payload: Record<string, unknown> | null): ChatAssignmentLimitStop | null {
  if (payload?.code === "provider_usage_limit") {
    const resetsAt = typeof payload.resetsAt === "string" && !Number.isNaN(Date.parse(payload.resetsAt))
      ? payload.resetsAt as UtcTimestamp
      : null;
    return { kind: "provider", resetsAt };
  }
  if (payload?.code !== "budget_limit_reached") return null;
  const limit = payload.limit === "tokens"
    ? "tokens"
    : payload.limit === "working_time" ? "workingTime" : null;
  const used = nonNegativeInteger(payload.used);
  const limitValue = nonNegativeInteger(payload.limitValue);
  if (limit === null || used === null || limitValue === null) return null;
  return { kind: "budget", limit, used, limitValue };
}

function parseBoundedInteger(
  text: string,
  minimum: number,
  maximum: number,
  scale: number,
): BudgetLimitInput {
  const digits = text.replace(/[\s,.'_  ]/gu, "");
  if (!digits) return { kind: "empty" };
  if (!/^\d+$/u.test(digits)) return { kind: "invalid" };
  const value = Number(digits) * scale;
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) return { kind: "invalid" };
  return { kind: "valid", value };
}

function nonNegativeInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

/** Days of teammate usage that teammate settings show, today included. */
export const TEAMMATE_USAGE_DAYS = 7;

/** Returns the inclusive local date range of the last `days` days ending on `today`. */
export function teammateUsageRange(
  today: Temporal.PlainDate,
  days: number = TEAMMATE_USAGE_DAYS,
): { fromDate: string; toDate: string } {
  return {
    fromDate: today.subtract({ days: Math.max(1, days) - 1 }).toString(),
    toDate: today.toString(),
  };
}

/** Assignment state reason the executing device writes when a set budget limit stopped the work. */
export const BUDGET_LIMIT_REACHED_REASON = "budget_limit_reached";
/** Assignment state reason the executing device writes when a provider usage limit stopped the work. */
export const PROVIDER_USAGE_LIMIT_REASON = "provider_usage_limit";

/** The parts of an assignment that decide which limit notice its reply thread shows. */
export interface LimitNoticeAssignment {
  id: ChatWorkAssignmentId;
  state: string;
  stateReason: string | null;
  budget: ChatAssignmentBudgetRead | null;
}

/** The parts of a live provider rate limit report the notice reads. */
export interface LiveRateLimit {
  limited: boolean;
  resetsAt: UtcTimestamp | null;
}

/**
 * Returns the limit that stopped a failed assignment at its budget: the one its failure update
 * names, else the meter that reached its limit or the first set limit, and `null` when the work did not
 * stop at a budget or the assignment has no budget left to show.
 */
export function budgetStoppedAt(
  assignment: LimitNoticeAssignment,
  replies: readonly ChatMessageRead[],
): BudgetMeterKind | null {
  if (assignment.state !== "failed" || assignment.stateReason !== BUDGET_LIMIT_REACHED_REASON) return null;
  const stop = latestAssignmentLimitStop(replies, assignment.id);
  if (stop?.kind === "budget") return stop.limit;
  const meters = assignment.budget ? budgetMeters(assignment.budget) : [];
  return (meters.find((entry) => entry.threshold === "reached") ?? meters[0])?.kind ?? null;
}

/**
 * Returns the provider limit notice of a reply thread: the failure every device reads once a provider
 * limit stopped the work, else the live report of the device that runs it. `resetsAt` is `null`
 * when the provider gave no reset time.
 */
export function providerLimitNotice(
  assignment: LimitNoticeAssignment | null,
  replies: readonly ChatMessageRead[],
  live: LiveRateLimit | null,
): { resetsAt: UtcTimestamp | null } | null {
  if (assignment?.state === "failed" && assignment.stateReason === PROVIDER_USAGE_LIMIT_REASON) {
    const stop = latestAssignmentLimitStop(replies, assignment.id);
    return { resetsAt: stop?.kind === "provider" ? stop.resetsAt : null };
  }
  if (live?.limited) {
    const resetsAt = live.resetsAt !== null && !Number.isNaN(Date.parse(live.resetsAt)) ? live.resetsAt : null;
    return { resetsAt };
  }
  return null;
}
