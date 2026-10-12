import { invoke } from "@tauri-apps/api/core";
import type { PomodoroPhase } from "$lib/pomodoro/rhythm";
import type { DistractionsLimitTotal, DistractionsDailyLimitEntryTotal } from "$lib/distractions";

export interface DistractionsRuntimeState {
  active: boolean;
  paused: boolean;
  pauseReason: "manual" | "idle" | "suspend" | null;
  phase: PomodoroPhase | "inactive";
  activeRunId: string | null;
  activeOccurrenceId: string | null;
  remainingSeconds: number | null;
  updatedAt: string;
}

export interface DistractionsExtensionStatus {
  connected: boolean;
  lastSeenAt: string | null;
  lastMessageType: string | null;
  checkedAt: string;
  staleSeconds: number;
  reason: string | null;
}

export interface DistractionsDesktopAppCandidate {
  name: string;
  source: string;
  detail: string | null;
  processNames: string[];
}

export interface DistractionsBudgetTotal extends DistractionsLimitTotal {
  period: "day" | "week";
  windowStartLocalDate: string;
  windowEndLocalDate: string;
  entries: DistractionsDailyLimitEntryTotal[];
}

export interface DistractionsUsageProjection {
  vaultId: string;
  localDate: string;
  weekStartLocalDate: string;
  updatedAt: string;
  totals: DistractionsBudgetTotal[];
  foregroundStatus: DistractionsForegroundDesktopAppStatus;
}

export interface DistractionsForegroundDesktopAppStatus {
  available: boolean;
  appName: string | null;
  processName: string | null;
  processId: number | null;
  matchNames: string[];
  reason: string | null;
}

export async function getDistractionsExtensionStatus(
  freshAfter?: string,
): Promise<DistractionsExtensionStatus> {
  return await invoke<DistractionsExtensionStatus>(
    "distractions_extension_status",
    { freshAfter: freshAfter ?? null },
  );
}

export async function listDistractionsDesktopApps(): Promise<DistractionsDesktopAppCandidate[]> {
  return await invoke<DistractionsDesktopAppCandidate[]>("distractions_list_desktop_apps");
}

/** Validate compact native budgets and their complete source allocation before rendering. */
export function parseDistractionsUsageProjection(value: unknown): DistractionsUsageProjection {
  function object(value: unknown): Record<string, unknown> {
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      throw new Error("Invalid native Distractions usage object");
    }
    return value as Record<string, unknown>;
  }
  function text(value: unknown, limit: number): string {
    if (typeof value !== "string" || !value.trim() || value.length > limit) {
      throw new Error("Invalid native Distractions usage text");
    }
    return value;
  }
  function id(value: unknown): string {
    const result = text(value, 80);
    if (!/^[a-z0-9][a-z0-9_-]*$/iu.test(result)) throw new Error("Invalid native usage identity");
    return result;
  }
  function date(value: unknown): string {
    const result = text(value, 10);
    const timestamp = Date.parse(`${result}T00:00:00Z`);
    if (!/^\d{4}-\d{2}-\d{2}$/u.test(result) || result.startsWith("0000")
      || !Number.isFinite(timestamp) || new Date(timestamp).toISOString().slice(0, 10) !== result) {
      throw new Error("Invalid native usage date");
    }
    return result;
  }
  function seconds(value: unknown): number {
    if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
      throw new Error("Invalid native usage seconds");
    }
    return value;
  }
  const row = object(value);
  const localDate = date(row.localDate);
  const weekStartLocalDate = date(row.weekStartLocalDate);
  const day = new Date(`${localDate}T00:00:00Z`);
  day.setUTCDate(day.getUTCDate() - (day.getUTCDay() + 6) % 7);
  if (day.toISOString().slice(0, 10) !== weekStartLocalDate) throw new Error("Invalid native usage week");
  const updatedAt = text(row.updatedAt, 40);
  if (!Number.isFinite(Date.parse(updatedAt))) throw new Error("Invalid native usage timestamp");
  if (!Array.isArray(row.totals) || row.totals.length > 512) throw new Error("Native usage budgets exceed their limit");
  const budgetIds = new Set<string>();
  let entryCount = 0;
  const totals = row.totals.map((value): DistractionsBudgetTotal => {
    const total = object(value);
    const limitId = id(total.limitId);
    const period = total.period;
    if (period !== "day" && period !== "week") throw new Error("Invalid native usage period");
    const key = `${limitId}:${period}`;
    if (budgetIds.has(key)) throw new Error("Duplicate native usage budget");
    budgetIds.add(key);
    const windowStartLocalDate = date(total.windowStartLocalDate);
    const windowEndLocalDate = date(total.windowEndLocalDate);
    if (windowStartLocalDate !== (period === "day" ? localDate : weekStartLocalDate)
      || windowEndLocalDate !== localDate) throw new Error("Invalid native usage window");
    const usedSeconds = seconds(total.usedSeconds);
    const limitSeconds = seconds(total.limitSeconds);
    const remainingSeconds = seconds(total.remainingSeconds);
    if (limitSeconds < 60 || limitSeconds > (period === "day" ? 86_400 : 604_800)
      || limitSeconds % 60 !== 0 || remainingSeconds !== Math.max(0, limitSeconds - usedSeconds)
      || total.exhausted !== (usedSeconds >= limitSeconds)) throw new Error("Inconsistent native usage budget");
    if (!Array.isArray(total.entries) || total.entries.length === 0 || total.entries.length > 2_000) {
      throw new Error("Invalid native usage entry count");
    }
    const entryIds = new Set<string>();
    let allocated = 0;
    const entries = total.entries.map((value): DistractionsDailyLimitEntryTotal => {
      entryCount += 1;
      if (entryCount > 4_000) throw new Error("Native usage entries exceed their limit");
      const entry = object(value);
      const entryId = id(entry.entryId);
      if (entryIds.has(entryId)) throw new Error("Duplicate native usage entry");
      entryIds.add(entryId);
      const entryUsed = seconds(entry.usedSeconds);
      allocated += entryUsed;
      if (!Number.isSafeInteger(allocated)) throw new Error("Native usage allocation exceeds its integer limit");
      return { entryId, usedSeconds: entryUsed };
    });
    if (allocated !== usedSeconds) throw new Error("Native usage entries do not match their total");
    return { limitId, period, windowStartLocalDate, windowEndLocalDate, usedSeconds,
      limitSeconds, remainingSeconds, exhausted: usedSeconds >= limitSeconds, entries };
  });
  const observed = object(row.foregroundStatus);
  const nullableText = (value: unknown): string | null => value === null ? null : text(value, 512);
  if (typeof observed.available !== "boolean" || !Array.isArray(observed.matchNames)
    || observed.matchNames.length > 64 || (observed.processId !== null
      && (typeof observed.processId !== "number" || !Number.isSafeInteger(observed.processId)
        || observed.processId <= 0 || observed.processId > 4_294_967_295))) {
    throw new Error("Invalid native foreground status");
  }
  const foregroundStatus: DistractionsForegroundDesktopAppStatus = {
    available: observed.available, appName: nullableText(observed.appName),
    processName: nullableText(observed.processName), processId: observed.processId,
    matchNames: observed.matchNames.map((name) => text(name, 512)),
    reason: nullableText(observed.reason),
  };
  if ((foregroundStatus.available && foregroundStatus.appName === null)
    || (!foregroundStatus.available && (foregroundStatus.appName !== null
      || foregroundStatus.processName !== null || foregroundStatus.processId !== null
      || foregroundStatus.matchNames.length !== 0))) {
    throw new Error("Inconsistent native foreground availability");
  }
  return { vaultId: text(row.vaultId, 1_024), localDate, weekStartLocalDate, updatedAt, totals, foregroundStatus };
}

/** Read native totals without sending frontend budgets, clock values, or exhaustion decisions. */
export async function loadDistractionsUsageProjection(): Promise<DistractionsUsageProjection> {
  const command = typeof __GANBARU_AI_BUILD_PLATFORM__ !== "undefined" && __GANBARU_AI_BUILD_PLATFORM__ === "android"
    ? "distractions_mobile_load_usage_projection" : "distractions_load_usage_projection";
  return parseDistractionsUsageProjection(await invoke<unknown>(command));
}
