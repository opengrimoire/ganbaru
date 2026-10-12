import { Temporal } from "@js-temporal/polyfill";
import {
  createLifecycleScheduler,
  type LifecycleScheduler,
  type SchedulerClock,
} from "$lib/scheduling/lifecycle-scheduler";
import type { QuickNotesTrashPurge } from "$lib/quick-notes/types";

const PURGE_ERROR_RETRY_MS = 60_000;

export interface QuickNotesTrashPurgeSchedulerOptions {
  readonly purge: () => Promise<QuickNotesTrashPurge>;
  /** Called after a pass deletes notes, so open views can reload. */
  readonly onPurged: (count: number) => void;
  readonly onError?: (error: unknown) => void;
  readonly clock?: SchedulerClock;
}

/** Epoch milliseconds of the next purge pass, or null when no trash is waiting to expire. */
export function quickNotesTrashPurgeDeadline(result: QuickNotesTrashPurge): number | null {
  if (result.nextPurgeAt === null) return null;
  try {
    return Temporal.Instant.from(result.nextPurgeAt).epochMilliseconds;
  } catch (error: unknown) {
    throw new Error(`quick notes trash purge deadline is invalid: ${result.nextPurgeAt}`, {
      cause: error,
    });
  }
}

/**
 * Runs the trash purge now and again when the oldest remaining trashed note expires. Invalidate
 * it after Quick notes change, because new trash can move the deadline.
 */
export function createQuickNotesTrashPurgeScheduler(
  options: QuickNotesTrashPurgeSchedulerOptions,
): LifecycleScheduler {
  return createLifecycleScheduler({
    clock: options.clock,
    errorRetryMs: PURGE_ERROR_RETRY_MS,
    run: async (context) => {
      const result = await options.purge();
      if (result.purged > 0) options.onPurged(result.purged);
      return context.isCurrent() ? quickNotesTrashPurgeDeadline(result) : null;
    },
    onError: options.onError,
  });
}
