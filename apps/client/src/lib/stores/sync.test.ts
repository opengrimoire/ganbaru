import { beforeEach, describe, expect, it, vi } from "vitest";
import { OFF_SYNC_STATUS, type SyncStatusView } from "$lib/api/sync";

const api = vi.hoisted(() => ({
  statusListener: null as ((status: SyncStatusView) => void) | null,
  unlisten: vi.fn(),
  getSyncStatus: vi.fn(),
  setSyncPaused: vi.fn(),
}));

vi.mock("$lib/api/sync", async (importOriginal) => {
  const original = await importOriginal<typeof import("$lib/api/sync")>();
  return {
    ...original,
    getSyncStatus: api.getSyncStatus,
    setSyncPaused: api.setSyncPaused,
    syncNow: vi.fn(),
    listenSyncStatus: vi.fn(async (callback: (status: SyncStatusView) => void) => {
      api.statusListener = callback;
      return api.unlisten;
    }),
  };
});

const { getSync } = await import("./sync.svelte");
const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");

function status(overrides: Partial<SyncStatusView>): SyncStatusView {
  return { ...OFF_SYNC_STATUS, held: { ...OFF_SYNC_STATUS.held }, ...overrides };
}

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
}

describe("sync status store", () => {
  beforeEach(() => {
    api.statusListener = null;
    api.unlisten.mockReset();
    api.getSyncStatus.mockReset();
    api.setSyncPaused.mockReset();
  });

  it("loads the status, follows events, and stops listening after the last subscriber", async () => {
    api.getSyncStatus.mockResolvedValue(status({ state: "idle", role: "client" }));
    const sync = getSync();
    const first = sync.subscribe();
    const second = sync.subscribe();
    await settle();
    expect(sync.status.state).toBe("idle");
    expect(sync.loaded).toBe(true);

    api.statusListener?.(status({ state: "syncing", role: "client" }));
    expect(sync.status.state).toBe("syncing");

    first();
    first();
    expect(api.unlisten).not.toHaveBeenCalled();
    second();
    expect(api.unlisten).toHaveBeenCalledOnce();
  });

  it("drops a status that arrives after the vault changed", async () => {
    let resolveStale: (value: SyncStatusView) => void = () => undefined;
    api.getSyncStatus
      .mockReturnValueOnce(new Promise<SyncStatusView>((resolve) => { resolveStale = resolve; }))
      .mockResolvedValueOnce(status({ state: "offline", role: "client" }));
    const sync = getSync();
    const stop = sync.subscribe();
    setActiveVaultIdentity("vault-b");
    await settle();
    resolveStale(status({ state: "error", role: "hub" }));
    await settle();
    expect(sync.status.state).toBe("offline");
    stop();
  });

  it("refreshes after a pause change", async () => {
    api.setSyncPaused.mockResolvedValue(undefined);
    api.getSyncStatus.mockResolvedValue(status({ state: "paused", paused: true }));
    const sync = getSync();
    await sync.setPaused(true);
    expect(api.setSyncPaused).toHaveBeenCalledWith(true);
    expect(sync.status.paused).toBe(true);
  });
});
