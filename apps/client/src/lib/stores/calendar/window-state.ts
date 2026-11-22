import type { Temporal } from "@js-temporal/polyfill";

/** The dates a foreground window load reads. */
export interface CalendarWindowTarget {
  start: Temporal.PlainDate;
  end: Temporal.PlainDate;
}

/**
 * How a foreground load was started: `boot` shows the store's first window and forgets any earlier one, `apply`
 * shows the window it reads, and `ensure` only warms the shared cache.
 */
export type CalendarWindowLoadMode = "boot" | "apply" | "ensure";

/** A phase with no foreground load in flight. */
export type CalendarWindowSettledPhase =
  /** Nothing shown since the last boot. */
  | { kind: "idle" }
  /** A window is shown; `stale` while a committed change cleared the cache and its reload has not settled. */
  | { kind: "shown"; stale: boolean }
  /** The last foreground load failed; `shown` keeps whether a window was shown since the last boot. */
  | { kind: "failed"; error: unknown; mode: Exclude<CalendarWindowLoadMode, "ensure">; target: CalendarWindowTarget; shown: boolean };

/** The window lifecycle of one Calendar store. Only the load holding `token` may settle a `loading` phase. */
export type CalendarWindowPhase =
  | CalendarWindowSettledPhase
  | {
    kind: "loading";
    token: symbol;
    mode: CalendarWindowLoadMode;
    target: CalendarWindowTarget;
    /** What the store shows while the load runs, and what it returns to unless the load fails. */
    previous: CalendarWindowSettledPhase;
  };

/** How a foreground load ended. */
export type CalendarWindowLoadResult = { type: "settled" } | { type: "failed"; error: unknown };

export const IDLE_CALENDAR_WINDOW: CalendarWindowPhase = { kind: "idle" };

/** The phase the store shows, looking through an in-flight load. */
export function settledCalendarWindowPhase(phase: CalendarWindowPhase): CalendarWindowSettledPhase {
  return phase.kind === "loading" ? phase.previous : phase;
}

/** Whether a window was shown since the last boot. */
export function isCalendarWindowLoaded(phase: CalendarWindowPhase): boolean {
  const settled = settledCalendarWindowPhase(phase);
  return settled.kind === "shown" || (settled.kind === "failed" && settled.shown);
}

/** Whether the last foreground load failed and no newer load is running. */
export function isCalendarWindowFailed(phase: CalendarWindowPhase): phase is Extract<CalendarWindowPhase, { kind: "failed" }> {
  return phase.kind === "failed";
}

/** Whether a foreground load is in flight. */
export function isCalendarWindowBusy(phase: CalendarWindowPhase): boolean {
  return phase.kind === "loading";
}

/** Whether the shown window is kept on screen after a committed change, until its reload settles. */
export function retainsStaleCalendarWindow(phase: CalendarWindowPhase): boolean {
  const settled = settledCalendarWindowPhase(phase);
  return settled.kind === "shown" && settled.stale;
}

/** Whether `token` still owns the phase, so its load may settle it or request again. */
export function isCurrentCalendarWindowLoad(phase: CalendarWindowPhase, token: symbol): boolean {
  return phase.kind === "loading" && phase.token === token;
}

/** The window a pane should read again after a committed change: an in-flight load's target wins over the shown one. */
export function calendarWindowReloadTarget(
  phase: CalendarWindowPhase,
  shown: CalendarWindowTarget | null,
): CalendarWindowTarget | null {
  return phase.kind === "loading" && phase.mode !== "ensure" ? phase.target : shown;
}

/** Start a foreground load that supersedes any earlier one. A boot forgets the shown window. */
export function startCalendarWindowLoad(
  phase: CalendarWindowPhase,
  mode: CalendarWindowLoadMode,
  target: CalendarWindowTarget,
): { phase: CalendarWindowPhase; token: symbol } {
  const token = Symbol("calendar-window-load");
  const previous = mode === "boot" ? { kind: "idle" as const } : settledCalendarWindowPhase(phase);
  return { phase: { kind: "loading", token, mode, target, previous }, token };
}

/** Show a window read from the database or the cache. An in-flight load keeps running. */
export function calendarWindowShown(phase: CalendarWindowPhase): CalendarWindowPhase {
  const shown = { kind: "shown" as const, stale: false };
  return phase.kind === "loading" ? { ...phase, previous: shown } : shown;
}

/** Show a cached window at once, ending any in-flight foreground load. */
export function calendarWindowShownFromCache(): CalendarWindowPhase {
  return { kind: "shown", stale: false };
}

/** Mark the shown window stale after a committed change cleared the cache. */
export function calendarWindowMarkedStale(phase: CalendarWindowPhase): CalendarWindowPhase {
  const settled = settledCalendarWindowPhase(phase);
  if (settled.kind !== "shown" || settled.stale) return phase;
  const stale = { kind: "shown" as const, stale: true };
  return phase.kind === "loading" ? { ...phase, previous: stale } : stale;
}

/**
 * Settle the load holding `token`. A load the window owner superseded leaves the phase unchanged. A failed cache
 * warm-up returns to the previous phase because nothing was going to be shown.
 */
export function finishCalendarWindowLoad(
  phase: CalendarWindowPhase,
  token: symbol,
  result: CalendarWindowLoadResult,
): CalendarWindowPhase {
  if (!isCurrentCalendarWindowLoad(phase, token) || phase.kind !== "loading") return phase;
  if (result.type === "settled" || phase.mode === "ensure") return phase.previous;
  return {
    kind: "failed",
    error: result.error,
    mode: phase.mode,
    target: phase.target,
    shown: isCalendarWindowLoaded(phase),
  };
}
