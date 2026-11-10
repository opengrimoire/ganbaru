import { beforeEach, describe, expect, it, vi } from "vitest";

type AppliedListener = (tables: string[]) => void;
type WindowListener = (event: { payload: unknown }) => void;

const transport = vi.hoisted(() => ({
  applied: [] as AppliedListener[],
  windows: [] as { event: string; listener: WindowListener }[],
}));

vi.mock("$lib/api/sync", () => ({
  listenSyncApplied: (listener: AppliedListener) => {
    transport.applied.push(listener);
    return Promise.resolve(() => undefined);
  },
}));

vi.mock("$lib/windows/sync-transport", () => ({
  emitWindowSync: () => Promise.resolve(),
  listenWindowSync: (event: string, listener: WindowListener) => {
    transport.windows.push({ event, listener });
    return Promise.resolve(() => undefined);
  },
}));

async function startedWindowSync(reload = vi.fn(() => Promise.resolve())) {
  const { initCalendarWindowSync } = await import("./window-sync");
  initCalendarWindowSync(reload);
  return reload;
}

describe("calendar window sync", () => {
  beforeEach(() => {
    vi.resetModules();
    transport.applied.length = 0;
    transport.windows.length = 0;
  });

  it("reloads once for each applied notice that names Calendar rows", async () => {
    const reload = await startedWindowSync();

    transport.applied[0]?.(["calendar_events", "icalendar_components"]);

    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("reloads for applied Focus history, which Calendar views render", async () => {
    const reload = await startedWindowSync();

    transport.applied[0]?.(["focus"]);

    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("ignores applied notices for other domains", async () => {
    const reload = await startedWindowSync();

    transport.applied[0]?.(["quick_notes", "vault_preferences"]);

    expect(reload).not.toHaveBeenCalled();
  });

  it("subscribes once however many stores ask", async () => {
    await startedWindowSync();
    await startedWindowSync();

    expect(transport.applied).toHaveLength(1);
    expect(transport.windows.map(({ event }) => event).sort()).toEqual(["calendar-sync-changed", "calendar-window-sync"]);
  });

  it("reloads when another window of the vault changed Calendar rows outside its Calendar", async () => {
    vi.useFakeTimers();
    try {
      const reload = await startedWindowSync();
      const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
      const { VAULT_CHANGE_QUIET_MS } = await import("$lib/windows/vault-change-channel");
      setActiveVaultIdentity("vault-1");
      await Promise.resolve();
      const outsideChanges = transport.windows.find(({ event }) => event === "calendar-sync-changed");

      outsideChanges?.listener({ payload: { sourceId: "other-window", payload: { vaultId: "vault-2", keys: ["calendar"] } } });
      await vi.advanceTimersByTimeAsync(VAULT_CHANGE_QUIET_MS);
      expect(reload).not.toHaveBeenCalled();

      outsideChanges?.listener({ payload: { sourceId: "other-window", payload: { vaultId: "vault-1", keys: ["calendar"] } } });
      await vi.advanceTimersByTimeAsync(VAULT_CHANGE_QUIET_MS);
      expect(reload).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("logs a failed reload instead of leaving the rejection unhandled", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await startedWindowSync(vi.fn(() => Promise.reject(new Error("read failed"))));

    transport.applied[0]?.(["calendars"]);
    await Promise.resolve();

    expect(warn).toHaveBeenCalledWith("calendar window sync reload failed", expect.any(Error));
    warn.mockRestore();
  });
});
