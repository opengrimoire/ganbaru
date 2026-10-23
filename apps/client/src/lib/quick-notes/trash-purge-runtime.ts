import { purgeExpiredQuickNotesTrash } from "$lib/api/quick-notes";
import type { LifecycleScheduler } from "$lib/scheduling/lifecycle-scheduler";
import { createQuickNotesTrashPurgeScheduler } from "$lib/quick-notes/trash-purge";
import {
  listenForQuickNotesChanges,
  onLocalQuickNotesChange,
  publishQuickNotesChanged,
} from "$lib/quick-notes/window-sync";

/** The app-wide trash purge job and its Quick notes change subscriptions. */
export interface QuickNotesTrashPurgeRuntime {
  readonly scheduler: LifecycleScheduler;
  dispose(): void;
}

/**
 * Creates the trash purge job for the window that owns background work. The job starts disabled;
 * it reruns after local or foreign Quick notes changes and announces each purge to open views.
 */
export function startQuickNotesTrashPurge(): QuickNotesTrashPurgeRuntime {
  let announcing = false;
  let disposed = false;
  let unlistenForeign: (() => void) | null = null;
  const scheduler = createQuickNotesTrashPurgeScheduler({
    purge: purgeExpiredQuickNotesTrash,
    onPurged: () => {
      announcing = true;
      try {
        publishQuickNotesChanged();
      } finally {
        announcing = false;
      }
    },
    onError: (error) => console.warn("Quick notes trash purge failed", error),
  });
  const unsubscribeLocal = onLocalQuickNotesChange(() => {
    if (!announcing) scheduler.invalidate();
  });
  void listenForQuickNotesChanges(() => scheduler.invalidate())
    .then((unlisten) => {
      if (disposed) unlisten();
      else unlistenForeign = unlisten;
    })
    .catch((error: unknown) => console.warn("Quick notes trash purge cannot follow other windows", error));
  return {
    scheduler,
    dispose: () => {
      disposed = true;
      unsubscribeLocal();
      unlistenForeign?.();
      unlistenForeign = null;
      scheduler.dispose();
    },
  };
}
