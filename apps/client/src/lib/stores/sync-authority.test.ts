import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage: (value: unknown) => void = () => {};
  },
}));

function authority(ownIsAdmin: boolean): Record<string, unknown> {
  return {
    anchorDeviceId: "anchor",
    epoch: 1,
    revision: 3,
    frozen: false,
    ownIsAdmin,
    devices: [{
      device: { deviceId: "own", deviceLabel: "Desk", ownDevice: true },
      kind: "desktop",
      role: ownIsAdmin ? "admin" : "member",
      enrolledRevision: 1,
      removedRevision: null,
      quarantined: false,
      anchor: false,
    }],
  };
}

function answer(responses: Record<string, unknown>): void {
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command in responses) return responses[command];
    return undefined;
  });
}

function calls(command: string): number {
  return vi.mocked(invoke).mock.calls.filter(([name]) => name === command).length;
}

async function load() {
  const { getSyncAuthorityState } = await import("./sync-authority.svelte");
  return getSyncAuthorityState();
}

describe("sync authority state", () => {
  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
    setActiveVaultIdentity("vault");
  });

  it("loads the pending kit only when this desktop is an admin", async () => {
    answer({ sync_authority: authority(false), sync_contests: null, sync_outbox_list: [], sync_late_changes: { changes: [], total: 0 } });
    const state = await load();
    await state.refresh();
    expect(state.authority?.ownIsAdmin).toBe(false);
    expect(calls("sync_recovery_kit_state")).toBe(0);
    expect(state.pendingKit).toBeNull();

    answer({
      sync_authority: authority(true),
      sync_contests: null,
      sync_outbox_list: [],
      sync_late_changes: { changes: [], total: 0 },
      sync_recovery_kit_state: { stored: true, pending: { code: "ABCD-EFGH", qr: { width: 1, modules: [true] }, active: true } },
    });
    await state.refresh();
    expect(calls("sync_recovery_kit_state")).toBe(1);
    expect(state.pendingKit).toEqual({ code: "ABCD-EFGH", qr: { width: 1, modules: [true] }, active: true });
  });

  it("keeps the last state and reports a failure when a response is invalid", async () => {
    answer({ sync_authority: authority(false), sync_contests: null, sync_outbox_list: [], sync_late_changes: { changes: [], total: 0 } });
    const state = await load();
    await state.refresh();
    answer({ sync_authority: authority(false), sync_contests: null, sync_outbox_list: [{ change: "remove" }], sync_late_changes: { changes: [], total: 0 } });
    await state.refresh();
    expect(state.loadFailed).toBe(true);
    expect(state.authority?.devices).toHaveLength(1);
  });

  it("discards a load that finishes after the vault changed", async () => {
    let release: (value: unknown) => void = () => {};
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "sync_authority") return new Promise((resolve) => { release = resolve; });
      return Promise.resolve([]);
    });
    const state = await load();
    const pending = state.refresh();
    const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
    setActiveVaultIdentity("other");
    release(authority(true));
    await pending;
    expect(state.authority).toBeNull();
  });

  it("sends late decisions by removal, table, and row, then reloads", async () => {
    answer({ sync_authority: null, sync_contests: null, sync_outbox_list: [], sync_late_changes: { changes: [], total: 0 } });
    const state = await load();
    await state.discardLateChange({
      removalRevision: 4,
      device: { deviceId: "b", deviceLabel: null, ownDevice: false },
      table: "quick_notes",
      rowKey: "note-1",
      kind: "values",
      title: "",
      preview: "",
      changedAtMs: null,
      decisionPending: false,
    });
    expect(invoke).toHaveBeenCalledWith("sync_late_change_discard", {
      target: { removalRevision: 4, table: "quick_notes", rowKey: "note-1" },
    });
    expect(calls("sync_authority")).toBe(1);
  });
});
