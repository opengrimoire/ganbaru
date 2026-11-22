// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { createNotesBlockSelectionController } from "./selection-controller.svelte";

/** Builds a controller whose page can change while it stays mounted, like a block list that is not keyed by page. */
function pageHarness() {
  let pageId = "first-page";
  const list = document.createElement("div");
  const controller = createNotesBlockSelectionController({
    hydrateSubtrees: async (ids) => ids,
    undo: vi.fn(async () => true), redo: vi.fn(async () => true),
    readPageId: () => pageId, readListElement: () => list,
    readRenderedBlockIds: () => [],
    readTreeState: () => ({ blocksById: {}, childIdsByParentId: {} }),
    blockIdFromEvent: () => null, targetIsEditable: () => false, targetIsSelectionZone: () => true,
    focusTextEditorAtEnd: () => true, focusRow: vi.fn(), handleNavigationKeydown: () => false,
    pasteBlocks: async () => null, duplicateBlocks: async () => null,
    moveBlocks: async () => undefined, deleteBlocks: async () => undefined,
  });
  return { controller, showPage: (next: string) => { pageId = next; } };
}

describe("Notes block selection action failures", () => {
  it("keeps a failure on the page whose action failed", async () => {
    const h = pageHarness();
    await h.controller.run(() => { throw new Error("move failed"); });
    expect(h.controller.error).toBe("move failed");

    h.showPage("second-page");
    expect(h.controller.error).toBeNull();
  });

  it("does not show a failure that lands after another page opened", async () => {
    const h = pageHarness();
    let fail: (reason: Error) => void = () => undefined;
    const running = h.controller.run(() => new Promise<void>((_resolve, reject) => { fail = reject; }));

    h.showPage("second-page");
    fail(new Error("delete failed"));
    await running;

    expect(h.controller.error).toBeNull();
  });
});
