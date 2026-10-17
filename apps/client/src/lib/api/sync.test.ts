import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  listenSyncApplied,
  listenSyncStatus,
  listSyncRecovery,
  mapSyncNotice,
  mapSyncStatus,
  OFF_SYNC_STATUS,
  restoreSyncRecovery,
} from "./sync";

const channels = vi.hoisted(() => [] as { onmessage: (value: unknown) => void }[]);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: (value: unknown) => void = () => {};
    constructor() {
      channels.push(this);
    }
  },
}));

function validStatus(): Record<string, unknown> {
  return {
    state: "idle",
    role: "client",
    paused: false,
    lastExchangeAtMs: 1_000,
    lastError: null,
    pendingLocalChanges: 2,
    waiting: 0,
    held: { newerFormat: 0, newerManifest: 1, invalid: 0 },
    conflicts: [{ table: "quick_notes", count: 3 }],
    recoveryCount: 1,
  };
}

describe("sync API boundary", () => {
  it("maps a native status", () => {
    expect(mapSyncStatus(validStatus())).toEqual({
      state: "idle",
      role: "client",
      paused: false,
      lastExchangeAtMs: 1_000,
      lastError: null,
      pendingLocalChanges: 2,
      waiting: 0,
      held: { newerFormat: 0, newerManifest: 1, invalid: 0 },
      conflicts: [{ table: "quick_notes", count: 3 }],
      recoveryCount: 1,
    });
  });

  it("maps the off status of an unlinked vault", () => {
    expect(mapSyncStatus({ ...OFF_SYNC_STATUS, held: { ...OFF_SYNC_STATUS.held } })).toEqual(OFF_SYNC_STATUS);
  });

  it("rejects unknown states, roles, and negative counts", () => {
    expect(() => mapSyncStatus({ ...validStatus(), state: "linked" })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), role: "relay" })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), waiting: -1 })).toThrow();
    expect(() => mapSyncStatus({ ...validStatus(), held: { newerFormat: 0, newerManifest: 0 } })).toThrow();
  });

  it("maps status and applied notices and rejects malformed ones", () => {
    expect(mapSyncNotice({ kind: "applied", tables: ["quick_notes", "quick_note_tags"] })).toEqual({
      kind: "applied",
      tables: ["quick_notes", "quick_note_tags"],
    });
    expect(mapSyncNotice({ kind: "status", status: validStatus() })).toEqual({ kind: "status", status: mapSyncStatus(validStatus()) });
    expect(() => mapSyncNotice({ kind: "applied", tables: [1] })).toThrow();
    expect(() => mapSyncNotice({ kind: "status", status: { ...validStatus(), state: "linked" } })).toThrow();
    expect(() => mapSyncNotice({ kind: "moved", tables: [] })).toThrow();
    expect(() => mapSyncNotice(["quick_notes"])).toThrow();
  });

  it("shares one subscription, routes notices by kind, and unsubscribes after the last listener", async () => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(undefined);
    channels.length = 0;
    const statuses: unknown[] = [];
    const applied: string[][] = [];
    const stopStatus = await listenSyncStatus((status) => statuses.push(status));
    const stopApplied = await listenSyncApplied((tables) => applied.push(tables));

    expect(channels).toHaveLength(1);
    const subscribe = vi.mocked(invoke).mock.calls.filter(([command]) => command === "sync_subscribe");
    expect(subscribe).toHaveLength(1);
    const { subscriptionId } = subscribe[0]?.[1] as { subscriptionId: string };

    channels[0]?.onmessage({ kind: "applied", tables: ["quick_notes"] });
    channels[0]?.onmessage({ kind: "status", status: validStatus() });
    channels[0]?.onmessage({ kind: "applied", tables: "quick_notes" });
    expect(applied).toEqual([["quick_notes"]]);
    expect(statuses).toEqual([mapSyncStatus(validStatus())]);

    stopStatus();
    expect(invoke).not.toHaveBeenCalledWith("sync_unsubscribe", expect.anything());
    stopApplied();
    stopApplied();
    await Promise.resolve();
    await Promise.resolve();
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "sync_unsubscribe")).toEqual([
      ["sync_unsubscribe", { subscriptionId }],
    ]);
  });

  it("drops a listener whose subscription fails so the next one retries", async () => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockRejectedValueOnce(new Error("closed")).mockResolvedValue(undefined);
    await expect(listenSyncApplied(() => {})).rejects.toThrow("closed");
    const stop = await listenSyncApplied(() => {});
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "sync_subscribe")).toHaveLength(2);
    stop();
  });

  it("maps recovery offers with their device", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      {
        table: "quick_notes",
        rowKey: "note-1",
        title: "Plan",
        preview: "Start",
        device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
        editedAtMs: 2_000,
      },
    ]);
    await expect(listSyncRecovery()).resolves.toEqual([
      {
        table: "quick_notes",
        rowKey: "note-1",
        title: "Plan",
        preview: "Start",
        device: { deviceId: "device-b", deviceLabel: null, ownDevice: false },
        editedAtMs: 2_000,
      },
    ]);
  });

  it("sends the recovery target and returns the restored id", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("note-9");
    await expect(restoreSyncRecovery({ table: "quick_notes", rowKey: "note-1" })).resolves.toBe("note-9");
    expect(invoke).toHaveBeenLastCalledWith("sync_recovery_restore", { target: { table: "quick_notes", rowKey: "note-1" } });
  });
});
