import {
  loadDistractionsUsageProjection,
  type DistractionsBudgetTotal,
  type DistractionsForegroundDesktopAppStatus,
} from "$lib/api/distractions";
import { onActiveVaultIdentityChange, requireActiveVaultIdentity } from "$lib/vault/active-vault";
import type { DistractionsLimitPeriod, DistractionsLimitTotal } from "$lib/distractions";
import type { SchedulerRunContext } from "$lib/scheduling/lifecycle-scheduler";

export const DISTRACTIONS_USAGE_REFRESH_INTERVAL_MS = 5_000;

let localDate = $state("");
let weekStartLocalDate = $state("");
let totals = $state<DistractionsBudgetTotal[]>([]);
let foregroundStatus = $state<DistractionsForegroundDesktopAppStatus>(unavailableStatus());
/** Generation of the in-flight refresh, so a vault change admits a new refresh at once. */
let runningRefreshGeneration: number | null = null;
let generation = 0;

function unavailableStatus(): DistractionsForegroundDesktopAppStatus {
  return { available: false, appName: null, processName: null, processId: null, matchNames: [], reason: null };
}

onActiveVaultIdentityChange(() => {
  generation += 1;
  localDate = "";
  weekStartLocalDate = "";
  totals = [];
  foregroundStatus = unavailableStatus();
});

/** Refresh settings presentation. Native execution continues independently of this reader. */
async function refreshUsage(context?: SchedulerRunContext): Promise<void> {
  if (runningRefreshGeneration === generation) return;
  const expectedGeneration = generation;
  runningRefreshGeneration = expectedGeneration;
  try {
    const expectedVault = requireActiveVaultIdentity();
    const projection = await loadDistractionsUsageProjection();
    if (context && !context.isCurrent()) return;
    if (expectedGeneration !== generation || projection.vaultId !== expectedVault) return;
    localDate = projection.localDate;
    weekStartLocalDate = projection.weekStartLocalDate;
    totals = projection.totals;
    foregroundStatus = projection.foregroundStatus;
  } catch (error) {
    if (expectedGeneration !== generation || (context && !context.isCurrent())) return;
    totals = [];
    foregroundStatus = unavailableStatus();
    console.warn("Failed to refresh distraction usage limits:", error);
  } finally {
    if (runningRefreshGeneration === expectedGeneration) runningRefreshGeneration = null;
  }
}

/** Expose canonical budgets and observed adapter availability for settings rendering. */
export function getDistractionsUsage() {
  return {
    get localDate(): string { return localDate; },
    get weekStartLocalDate(): string { return weekStartLocalDate; },
    get totals(): readonly DistractionsLimitTotal[] { return totals; },
    get foregroundStatus(): DistractionsForegroundDesktopAppStatus { return foregroundStatus; },
    entryTotalsFor(limitId: string, period: DistractionsLimitPeriod) {
      return totals.find((total) => total.limitId === limitId && total.period === period)?.entries ?? [];
    },
    totalFor(limitId: string): DistractionsLimitTotal | null {
      return totals.find((total) => total.limitId === limitId) ?? null;
    },
    totalsFor(limitId: string): readonly DistractionsLimitTotal[] {
      return totals.filter((total) => total.limitId === limitId);
    },
    refresh: refreshUsage,
  };
}
