import {
  confirmSyncRecoveryKitSaved,
  discardSyncLateChange,
  forceSyncAnchor,
  getPendingSyncRecoveryKit,
  getSyncAuthority,
  liftSyncQuarantine,
  listenSyncApplied,
  listenSyncStatus,
  listSyncLateChanges,
  listSyncOutbox,
  promoteSyncDevice,
  quarantineSyncDevice,
  regenerateSyncRecoveryKit,
  removeSyncDevice,
  resetSyncState,
  restoreSyncLateChange,
  takeSyncAnchor,
  useSyncRecoveryKit,
  type SyncAuthorityView,
  type SyncForceAnchorRequest,
  type SyncLateChange,
  type SyncOutboxEntry,
  type SyncPendingRecoveryKit,
  type SyncStatusView,
} from "$lib/api/sync";
import { BUILD_PLATFORM_PROFILE } from "$lib/platform";
import { onActiveVaultIdentityChange } from "$lib/vault/active-vault";

/** Only desktops hold admin keys and recovery seeds, so only they ask for a pending kit. */
const KIT_CAPABLE = BUILD_PLATFORM_PROFILE.shell === "desktop";

let authority = $state<SyncAuthorityView | null>(null);
let outbox = $state<SyncOutboxEntry[]>([]);
let lateChanges = $state<SyncLateChange[]>([]);
let pendingKit = $state<SyncPendingRecoveryKit | null>(null);
let loadFailed = $state(false);
let generation = 0;
let subscribers = 0;
let lastStatusKey: string | null = null;
let stopListening: (() => void) | null = null;

onActiveVaultIdentityChange(() => {
  generation += 1;
  authority = null;
  outbox = [];
  lateChanges = [];
  pendingKit = null;
  loadFailed = false;
  lastStatusKey = null;
  if (subscribers > 0) void refresh();
});

/**
 * Reloads the authorization state, the outbox, the late changes, and, on an admin desktop, the
 * recovery kit that waits to be saved. A vault change during the load discards its results.
 */
async function refresh(): Promise<void> {
  const expected = generation;
  try {
    const [nextAuthority, nextOutbox, nextLate] = await Promise.all([
      getSyncAuthority(),
      listSyncOutbox(),
      listSyncLateChanges(),
    ]);
    const nextKit = KIT_CAPABLE && nextAuthority?.ownIsAdmin ? await getPendingSyncRecoveryKit() : null;
    if (expected !== generation) return;
    authority = nextAuthority;
    outbox = nextOutbox;
    lateChanges = nextLate;
    pendingKit = nextKit;
    loadFailed = false;
  } catch (cause: unknown) {
    if (expected !== generation) return;
    console.warn("sync authority state could not be loaded", cause);
    loadFailed = true;
  }
}

/** Identifies the status changes after which the authorization state can differ. */
function statusKey(status: SyncStatusView): string {
  return `${status.state}:${status.lastExchangeAtMs ?? "never"}:${status.unconfirmed}`;
}

function onStatus(status: SyncStatusView): void {
  const key = statusKey(status);
  if (key === lastStatusKey) return;
  lastStatusKey = key;
  void refresh();
}

async function startListening(): Promise<void> {
  const stops = await Promise.all([listenSyncStatus(onStatus), listenSyncApplied(() => void refresh())]);
  const stop = () => {
    for (const stopOne of stops) stopOne();
  };
  if (subscribers === 0 || stopListening) {
    stop();
    return;
  }
  stopListening = stop;
}

/**
 * Keeps the authorization state current while at least one caller is subscribed.
 *
 * @returns A function that ends this subscription.
 */
function subscribe(): () => void {
  subscribers += 1;
  if (subscribers === 1) {
    void startListening().catch((cause: unknown) => {
      console.warn("sync authority events could not be observed", cause);
    });
    void refresh();
  }
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    subscribers -= 1;
    if (subscribers > 0) return;
    stopListening?.();
    stopListening = null;
    lastStatusKey = null;
  };
}

/** Runs `action` and reloads the state afterwards, unless the vault changed meanwhile. */
async function thenRefresh<T>(action: () => Promise<T>): Promise<T> {
  const expected = generation;
  try {
    return await action();
  } finally {
    if (expected === generation) await refresh();
  }
}

function lateTarget(change: SyncLateChange) {
  return { removalRevision: change.removalRevision, table: change.table, rowKey: change.rowKey };
}

/** Authorization state of the active vault's sync space and the commands that change it. */
export function getSyncAuthorityState() {
  return {
    get authority(): SyncAuthorityView | null { return authority; },
    get outbox(): SyncOutboxEntry[] { return outbox; },
    get lateChanges(): SyncLateChange[] { return lateChanges; },
    get pendingKit(): SyncPendingRecoveryKit | null { return pendingKit; },
    get loadFailed(): boolean { return loadFailed; },
    subscribe,
    refresh,
    removeDevice: (deviceId: string) => thenRefresh(() => removeSyncDevice(deviceId)),
    quarantineDevice: (deviceId: string) => thenRefresh(() => quarantineSyncDevice(deviceId)),
    liftQuarantine: (deviceId: string) => thenRefresh(() => liftSyncQuarantine(deviceId)),
    promoteDevice: (deviceId: string) => thenRefresh(() => promoteSyncDevice(deviceId)),
    takeAnchor: () => thenRefresh(takeSyncAnchor),
    forceAnchor: (request: SyncForceAnchorRequest) => thenRefresh(() => forceSyncAnchor(request)),
    restoreLateChange: (change: SyncLateChange) => thenRefresh(() => restoreSyncLateChange(lateTarget(change))),
    discardLateChange: (change: SyncLateChange) => thenRefresh(() => discardSyncLateChange(lateTarget(change))),
    regenerateKit: () => thenRefresh(regenerateSyncRecoveryKit),
    confirmKitSaved: () => thenRefresh(confirmSyncRecoveryKitSaved),
    useKit: (code: string) => thenRefresh(() => useSyncRecoveryKit(code)),
    resetSyncState: () => thenRefresh(resetSyncState),
  };
}
