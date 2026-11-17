import { describe, expect, it, vi } from "vitest";
import type { ProviderUpdateRead, ProviderUpdateRunRead } from "$lib/chat/contracts";
import { UPDATE_AUTO_CHECK_INTERVAL_MS } from "$lib/stores/update-policy";
import { ProviderUpdatesStore, type ProviderUpdatesApi } from "./provider-updates.svelte";

vi.mock("$lib/api/chat/settings", () => ({}));

const claude: ProviderUpdateRead = {
  instanceId: "claude",
  currentVersion: "2.1.311",
  latestVersion: "2.1.326",
  updateCommand: "claude update",
  runsInApp: true,
};

function api(overrides: Partial<ProviderUpdatesApi> = {}): ProviderUpdatesApi {
  return {
    checkChatProviderUpdates: vi.fn(async () => [claude]),
    runChatProviderUpdate: vi.fn(async () => ({ successful: true, output: null })),
    ...overrides,
  };
}

describe("ProviderUpdatesStore", () => {
  it("checks once at startup and then at most daily while notifications are on", async () => {
    const calls = api();
    const store = new ProviderUpdatesStore(calls);
    const start = Date.parse("2026-11-17T08:00:00Z");
    await store.checkAutomatically(false, "startup", start);
    expect(calls.checkChatProviderUpdates).not.toHaveBeenCalled();
    await store.checkAutomatically(true, "startup", start);
    await store.checkAutomatically(true, "periodic", start + 60_000);
    expect(calls.checkChatProviderUpdates).toHaveBeenCalledTimes(1);
    await store.checkAutomatically(true, "periodic", start + UPDATE_AUTO_CHECK_INTERVAL_MS);
    expect(calls.checkChatProviderUpdates).toHaveBeenCalledTimes(2);
    expect(store.forInstance("claude")).toEqual(claude);
    expect(store.forInstance("codex")).toBeNull();
  });

  it("keeps the previous results when a check fails", async () => {
    let fail = false;
    const store = new ProviderUpdatesStore(api({
      checkChatProviderUpdates: vi.fn(async () => {
        if (fail) throw new Error("offline");
        return [claude];
      }),
    }));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    await store.check();
    fail = true;
    await store.check();
    expect(store.updates).toEqual([claude]);
    expect(error).toHaveBeenCalledOnce();
    error.mockRestore();
  });

  it("joins a check already in flight", async () => {
    let finish: (updates: ProviderUpdateRead[]) => void = () => {};
    const calls = api({
      checkChatProviderUpdates: vi.fn(() => new Promise<ProviderUpdateRead[]>((resolve) => { finish = resolve; })),
    });
    const store = new ProviderUpdatesStore(calls);
    const first = store.check();
    const second = store.check();
    finish([claude]);
    await Promise.all([first, second]);
    expect(calls.checkChatProviderUpdates).toHaveBeenCalledOnce();
  });

  it("forgets an update only after its command succeeds", async () => {
    const runs = [
      { successful: false, output: "permission denied" },
      { successful: true, output: null },
    ];
    const run = vi.fn(async () => runs.shift() ?? { successful: true, output: null });
    const store = new ProviderUpdatesStore(api({
      runChatProviderUpdate: run,
    }));
    await store.check();
    await expect(store.run("codex")).rejects.toThrow("No update is known");
    expect(await store.run("claude")).toEqual({ successful: false, output: "permission denied" });
    expect(run).toHaveBeenCalledWith("claude", "2.1.326");
    expect(store.forInstance("claude")).toEqual(claude);
    expect(store.running).toBeNull();
    await store.run("claude");
    expect(store.forInstance("claude")).toBeNull();
  });

  it("refuses a second update while one runs and clears the running state after a failure", async () => {
    let finish: () => void = () => {};
    const store = new ProviderUpdatesStore(api({
      runChatProviderUpdate: vi.fn(() => new Promise<ProviderUpdateRunRead>((_resolve, reject) => {
        finish = () => reject(new Error("could not start"));
      })),
    }));
    await store.check();
    const first = store.run("claude");
    expect(store.running).toBe("claude");
    await expect(store.run("claude")).rejects.toThrow("already running");
    finish();
    await expect(first).rejects.toThrow("could not start");
    expect(store.running).toBeNull();
  });
});
