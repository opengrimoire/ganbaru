// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { getActiveNotesBlockDragId } from "$lib/notes/blocks/drag";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesBlock } from "$lib/notes/types";
import { createNotesBlockDragController } from "./block-drag-controller.svelte";

describe("Notes block drag controller", () => {
  it("drops a block inside an empty toggle using the center of its row", () => {
    const source = { ...createBlockWrite("source", "paragraph", "Move"),
      parent: { type: "page_id", page_id: "page" } } as NotesBlock;
    const toggle = { ...createBlockWrite("toggle", "toggle", "Details"),
      parent: { type: "page_id", page_id: "page" } } as NotesBlock;
    const dropBlock = vi.fn();
    const controller = createNotesBlockDragController({
      readTreeState: () => ({ blocksById: { source, toggle }, childIdsByParentId: { page: ["source", "toggle"] } }),
      dropBlock,
    });
    const row = document.createElement("div");
    row.style.setProperty("--notes-depth", "0");
    row.getBoundingClientRect = vi.fn(() => ({ height: 100, top: 0, left: 0 }) as DOMRect);
    controller.start("source", { dataTransfer: { setData: vi.fn() } } as unknown as DragEvent);
    const event = { currentTarget: row, target: row, clientY: 50, clientX: 50,
      preventDefault: vi.fn(), dataTransfer: { getData: () => "source" } } as unknown as DragEvent;
    controller.over("toggle", event);
    expect(controller.dropPositionForBlock("toggle")).toBe("inside");
    controller.drop("toggle", event);
    expect(dropBlock).toHaveBeenCalledWith("source", "toggle", "inside");
  });

  it("clears local and shared drag state after drag end", () => {
    const setData = vi.fn();
    const controller = createNotesBlockDragController({
      readTreeState: () => ({ blocksById: {}, childIdsByParentId: {} }),
      dropBlock: vi.fn(),
    });
    controller.start("block-1", {
      dataTransfer: { effectAllowed: "none", setData },
    } as unknown as DragEvent);
    expect(controller.draggingBlockId).toBe("block-1");
    expect(getActiveNotesBlockDragId()).toBe("block-1");
    controller.end();
    expect(controller.draggingBlockId).toBeNull();
    expect(getActiveNotesBlockDragId()).toBeNull();
  });
});
