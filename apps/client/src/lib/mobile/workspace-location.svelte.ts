import type { WorkspaceLocationAdapter } from "$lib/stores/workspace-tabs.svelte";
import { kindLocation } from "$lib/workspace-tabs/model";

/**
 * A tab location adapter for the phone shell, whose view stores load after the first paint. Calendar needs
 * no store. Restoring any other kind waits for the stores before it resolves, and until they arrive a read
 * reports nothing, so the tabs keep their saved locations instead of recording an empty one. A failed load
 * is retried on the next restore.
 */
export function createDeferredWorkspaceLocationAdapter(
  load: () => Promise<WorkspaceLocationAdapter>,
): WorkspaceLocationAdapter {
  let loaded = $state.raw<WorkspaceLocationAdapter | null>(null);
  let pending: Promise<WorkspaceLocationAdapter> | null = null;

  function ensureLoaded(): Promise<WorkspaceLocationAdapter> {
    if (loaded) return Promise.resolve(loaded);
    pending ??= load()
      .then((adapter) => {
        loaded = adapter;
        return adapter;
      })
      .finally(() => {
        pending = null;
      });
    return pending;
  }

  return {
    read(kind) {
      if (kind === "calendar") return kindLocation(kind);
      return loaded?.read(kind) ?? null;
    },
    title(level, location) {
      return loaded?.title(level, location) ?? null;
    },
    async apply(location, isCurrent) {
      if (location.kind === "calendar") return;
      const adapter = await ensureLoaded();
      if (isCurrent()) await adapter.apply(location, isCurrent);
    },
  };
}
