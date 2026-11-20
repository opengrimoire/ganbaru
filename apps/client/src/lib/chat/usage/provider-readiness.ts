import type { ProviderAccountPlan, ProviderInstanceRead } from "$lib/chat/contracts";
import { composerRateLimitWindows, formatWindowDuration, isProviderAvailable } from "$lib/chat/composer/model";

/** Age after which a limits read names when it was observed, since the provider may have moved on. */
export const STALE_PLAN_LIMITS_MS = 60 * 60 * 1_000;
/** How long a limited report without a reset time keeps a provider limited: the shortest plan window. */
const LIMITED_WITHOUT_RESET_MS = 5 * 60 * 60 * 1_000;
const MILLISECONDS_PER_SECOND = 1_000;

/** One plan usage window of a provider as of now. */
export interface ProviderPlanLimitWindow {
  id: string;
  /** Provider label of the window, used when its length has no localized name. */
  label: string;
  durationMinutes: number | null;
  /** Share of the window still available, 0 to 100. */
  remainingPercent: number;
  /** When the window resets, or `null` when the provider gave no time or it already reset. */
  resetsAtMs: number | null;
  /** Whether the window meters the whole account rather than one model. */
  accountWide: boolean;
}

const MINUTES_PER_WEEK = 7 * 24 * 60;

/**
 * Names a plan window: an account-wide week reads as `weekLabel`, other account-wide windows by
 * their length such as `5h`, and per-model windows keep the provider label.
 */
export function planLimitWindowName(window: ProviderPlanLimitWindow, weekLabel: string): string {
  if (!window.accountWide || window.durationMinutes === null) return window.label;
  return window.durationMinutes === MINUTES_PER_WEEK ? weekLabel : formatWindowDuration(window.durationMinutes);
}

/** The plan limits a provider last reported on this device. */
export interface ProviderPlanLimitsRead {
  windows: ProviderPlanLimitWindow[];
  observedAtMs: number;
  /** Whether the observation is old enough that the provider may report different usage now. */
  stale: boolean;
  /** Whether the provider refuses work until a limit resets. */
  limited: boolean;
}

/** How ready a provider is to take Chat work, from most to least ready. */
export type ProviderReadiness = "ready" | "limited" | "unavailable";

const READINESS_ORDER: Readonly<Record<ProviderReadiness, number>> = {
  ready: 0,
  limited: 1,
  unavailable: 2,
};

/**
 * Reads the plan limits a provider last reported as they stand at `nowMs`.
 *
 * Account-wide windows are preferred over per-model ones when the provider reports both, and a
 * window whose reset passed reads as fully available, since the provider restarted its count.
 *
 * @param provider The stored limits observation of a provider, on this device or shared by a linked desktop.
 * @param nowMs The current instant in milliseconds.
 * @returns The limits, or `null` without a readable observation.
 */
export function providerPlanLimits(
  provider: Pick<ProviderInstanceRead, "usageLimits">,
  nowMs: number,
): ProviderPlanLimitsRead | null {
  const observation = provider.usageLimits;
  if (!observation) return null;
  const observedAtMs = Date.parse(observation.observedAt);
  if (Number.isNaN(observedAtMs)) return null;
  const allWindows = composerRateLimitWindows(observation.status);
  const accountWindows = allWindows.filter((window) => window.accountWide);
  const windows = (accountWindows.length > 0 ? accountWindows : allWindows).map((window) => {
    const resetsAtMs = window.resetsAtSeconds === null ? null : window.resetsAtSeconds * MILLISECONDS_PER_SECOND;
    const reset = resetsAtMs !== null && resetsAtMs <= nowMs;
    return {
      id: window.id,
      label: window.label,
      durationMinutes: window.windowDurationMinutes,
      remainingPercent: reset ? 100 : 100 - window.usedPercent,
      resetsAtMs: reset ? null : resetsAtMs,
      accountWide: window.accountWide,
    };
  });
  return {
    windows,
    observedAtMs,
    stale: nowMs - observedAtMs > STALE_PLAN_LIMITS_MS,
    limited: limitedAt(observation.status.limited, observation.status.resetsAt, observedAtMs, nowMs),
  };
}

function limitedAt(limited: boolean, resetsAt: string | null, observedAtMs: number, nowMs: number): boolean {
  if (!limited) return false;
  const resetsAtMs = resetsAt === null ? Number.NaN : Date.parse(resetsAt);
  return Number.isNaN(resetsAtMs) ? nowMs - observedAtMs < LIMITED_WITHOUT_RESET_MS : resetsAtMs > nowMs;
}

/**
 * Reads how ready a provider is: usable and within its limits, usable but limited, or not usable
 * until setup, sign-in, or a version change.
 */
export function providerReadiness(provider: ProviderInstanceRead, nowMs: number): ProviderReadiness {
  if (!isProviderAvailable(provider)) return "unavailable";
  return providerPlanLimits(provider, nowMs)?.limited ? "limited" : "ready";
}

/**
 * Ranks the account plan a provider reported: paid plans and API keys first, then providers
 * that do not say, then a plan the provider names as free.
 */
const ACCOUNT_PLAN_ORDER: Readonly<Record<ProviderAccountPlan | "unknown", number>> = {
  subscription: 0,
  api_key: 0,
  unknown: 1,
  free: 2,
};

function accountPlanRank(provider: ProviderInstanceRead): number {
  return ACCOUNT_PLAN_ORDER[provider.lastProbe?.accountPlan ?? "unknown"];
}

/**
 * Orders providers by readiness, then by the account plan, then by the most recent use on this
 * device. The plan comes before recent use so one trial turn on a free plan does not outrank a
 * paid one. Returns 0 for equal providers, so callers keep their own fallback order.
 */
export function compareProviderReadiness(
  left: ProviderInstanceRead,
  right: ProviderInstanceRead,
  nowMs: number,
): number {
  return READINESS_ORDER[providerReadiness(left, nowMs)] - READINESS_ORDER[providerReadiness(right, nowMs)]
    || accountPlanRank(left) - accountPlanRank(right)
    || lastUsedMs(right) - lastUsedMs(left);
}

function lastUsedMs(provider: ProviderInstanceRead): number {
  const parsed = provider.lastUsedAt === null ? Number.NaN : Date.parse(provider.lastUsedAt);
  return Number.isNaN(parsed) ? 0 : parsed;
}
