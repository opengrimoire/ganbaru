import {
  getSyncStatus,
  listenSyncStatus,
  OFF_SYNC_STATUS,
  setSyncPaused,
  syncNow,
  type SyncStatusView,
} from "$lib/api/sync";
import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";

let status = $state<SyncStatusView>(OFF_SYNC_STATUS);
let loaded = $state(false);
let generation = 0;
let subscribers = 0;
let unlisten: (() => void) | null = null;

onActiveVaultIdentityChange(() => {
  generation += 1;
  status = OFF_SYNC_STATUS;
  loaded = false;
  if (subscribers > 0) void refresh();
});

async function refresh(): Promise<void> {
  const expected = generation;
  try {
    const next = await getSyncStatus();
    if (expected !== generation) return;
    status = next;
    loaded = true;
  } catch (cause: unknown) {
    console.warn("sync status could not be loaded", cause);
  }
}

async function startListening(): Promise<void> {
  const stop = await listenSyncStatus((next) => {
    status = next;
    loaded = true;
  });
  if (subscribers === 0 || unlisten) {
    stop();
    return;
  }
  unlisten = stop;
}

/**
 * Keeps the sync status current while at least one caller is subscribed.
 *
 * @returns A function that ends this subscription.
 */
function subscribe(): () => void {
  subscribers += 1;
  if (subscribers === 1) {
    void startListening().catch((cause: unknown) => {
      console.warn("sync status events could not be observed", cause);
    });
    void refresh();
  }
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    subscribers -= 1;
    if (subscribers > 0) return;
    unlisten?.();
    unlisten = null;
  };
}

/** Sync service status for the active vault and the commands that change it. */
export function getSync() {
  return {
    get status(): SyncStatusView { return status; },
    get loaded(): boolean { return loaded; },
    subscribe,
    refresh,
    async requestSync(): Promise<void> {
      await syncNow();
    },
    async setPaused(paused: boolean): Promise<void> {
      const expected = generation;
      await setSyncPaused(paused);
      if (expected === generation) await refresh();
    },
  };
}
