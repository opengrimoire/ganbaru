import { describe, expect, it, vi } from "vitest";
import type { LoadedNotesAdvancedBlock } from "$lib/components/notes/editor-component-registry";

const requests: Array<Promise<LoadedNotesAdvancedBlock>> = [];

vi.mock("$lib/components/notes/editor-component-registry", () => ({
  loadNotesAdvancedBlock: vi.fn(() => requests.shift()),
  retryNotesAdvancedBlock: vi.fn(() => requests.shift()),
}));

import { createNotesStructuralBlockLoader } from "./structural-block-loader.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

describe("Notes structural block loader", () => {
  it("does not let an older structural import replace a retried result", async () => {
    const first = deferred<LoadedNotesAdvancedBlock>();
    const second = deferred<LoadedNotesAdvancedBlock>();
    requests.push(first.promise, second.promise);
    const loader = createNotesStructuralBlockLoader();
    loader.request("tab");
    loader.request("tab", true);
    const newer = { kind: "tab", component: {} } as unknown as LoadedNotesAdvancedBlock;
    const stale = { kind: "column-list", component: {} } as unknown as LoadedNotesAdvancedBlock;
    second.resolve(newer);
    await second.promise;
    await Promise.resolve();
    first.resolve(stale);
    await first.promise;
    await Promise.resolve();

    const state = loader.stateFor("tab");
    expect(state?.status).toBe("ready");
    if (state?.status === "ready") expect(state.component.kind).toBe("tab");
  });
});
