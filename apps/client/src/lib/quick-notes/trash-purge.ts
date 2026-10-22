import { Temporal } from "@js-temporal/polyfill";
import {
  createLifecycleScheduler,
  type LifecycleScheduler,
  type SchedulerClock,
} from "$lib/scheduling/lifecycle-scheduler";
import type { QuickNotesTrashPurge } from "$lib/quick-notes/types";

const PURGE_ERROR_RETRY_MS = 60_000;
/** How often a running app looks for image files that no note uses any more. */
export const QUICK_NOTE_IMAGE_COLLECTION_INTERVAL_MS = 6 * 60 * 60_000;

export interface QuickNotesTrashPurgeSchedulerOptions {
  readonly purge: () => Promise<QuickNotesTrashPurge>;
  /** Called after a pass deletes notes, so open views can reload. */
  readonly onPurged: (count: number) => void;
  readonly onError?: (error: unknown) => void;
  /** Deletes image files that nothing references; the native side keeps recently orphaned files for a grace period. */
  readonly collectImages?: () => Promise<number>;
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
 * Runs the trash purge now and again when the oldest remaining trashed note expires, and collects unreferenced image
 * files at most once per interval. Invalidate it after Quick notes change, because new trash can move the deadline.
 */
export function createQuickNotesTrashPurgeScheduler(
  options: QuickNotesTrashPurgeSchedulerOptions,
): LifecycleScheduler {
  let lastCollectedAt: number | null = null;
  return createLifecycleScheduler({
    clock: options.clock,
    errorRetryMs: PURGE_ERROR_RETRY_MS,
    run: async (context) => {
      const result = await options.purge();
      if (result.purged > 0) options.onPurged(result.purged);
      let collectionDeadline: number | null = null;
      if (options.collectImages) {
        if (lastCollectedAt === null || context.now() - lastCollectedAt >= QUICK_NOTE_IMAGE_COLLECTION_INTERVAL_MS) {
          // A failed collection waits for the next interval instead of retrying with every purge.
          lastCollectedAt = context.now();
          try {
            await options.collectImages();
          } catch (error: unknown) {
            options.onError?.(error);
          }
        }
        collectionDeadline = lastCollectedAt + QUICK_NOTE_IMAGE_COLLECTION_INTERVAL_MS;
      }
      if (!context.isCurrent()) return null;
      const purgeDeadline = quickNotesTrashPurgeDeadline(result);
      if (purgeDeadline === null) return collectionDeadline;
      return collectionDeadline === null ? purgeDeadline : Math.min(purgeDeadline, collectionDeadline);
    },
    onError: options.onError,
  });
}
