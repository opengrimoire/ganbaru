// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SyncRecoveryEntry } from "$lib/api/sync";
import SyncRecoveryList from "./SyncRecoveryList.svelte";

const api = vi.hoisted(() => ({
  listSyncRecovery: vi.fn((): Promise<SyncRecoveryEntry[]> => Promise.resolve([])),
  restoreSyncRecovery: vi.fn((): Promise<string | null> => Promise.resolve(null)),
  discardSyncRecovery: vi.fn((): Promise<void> => Promise.resolve()),
}));

vi.mock("$lib/api/sync", () => api);

async function settle(): Promise<void> {
  for (let index = 0; index < 5; index += 1) {
    await Promise.resolve();
    flushSync();
  }
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

describe("Sync recovery list", () => {
  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    api.listSyncRecovery.mockReset();
  });

  afterEach(() => {
    if (component) unmount(component);
    component = undefined;
    target.remove();
  });

  it("clears the failed load notice once a later load succeeds", async () => {
    api.listSyncRecovery.mockRejectedValueOnce(new Error("busy")).mockResolvedValueOnce([]);
    const props = $state({ buttonClass: "", recoveryCount: 1, now: 0 });
    component = mount(SyncRecoveryList, { target, props });
    await settle();
    expect(target.textContent).toContain("Deleted items could not be loaded");

    props.recoveryCount = 0;
    await settle();

    expect(api.listSyncRecovery).toHaveBeenCalledTimes(2);
    expect(target.textContent).not.toContain("Deleted items could not be loaded");
  });
});
