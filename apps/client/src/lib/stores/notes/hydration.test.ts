import { describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { notesBlockOutlineFromBlock } from "$lib/notes/blocks/outline";
import type { NotesBlock } from "$lib/notes/types";

const backend = vi.hoisted(() => {
  let resolveHydration: (blocks: NotesBlock[]) => void = () => {};
  return {
    hydrateNotesBlocks: vi.fn(() => new Promise<NotesBlock[]>((resolve) => {
      resolveHydration = resolve;
    })),
    resolve(blocks: NotesBlock[]): void {
      resolveHydration(blocks);
    },
  };
});

vi.mock("$lib/api/notes", () => ({
  getNotesBlockFrontier: vi.fn(),
  getNotesBlockOutlineFrontier: vi.fn(),
  hydrateNotesBlocks: backend.hydrateNotesBlocks,
}));

describe("Notes hydration controller", () => {
  it("does not publish a replacement when requested blocks are already loaded", async () => {
    const { createNotesHydrationController } = await import("./hydration");
    const write = createBlockWrite("block-a", "paragraph");
    if (write.type !== "paragraph") throw new Error("expected paragraph test block");
    const block: NotesBlock = {
      object: "block",
      id: write.id,
      parent: { type: "page_id", page_id: "page-a" },
      created_time: "2026-07-12T00:00:00.000Z",
      last_edited_time: "2026-07-12T00:00:00.000Z",
      has_children: false,
      in_trash: false,
      archived: false,
      source_provider: null,
      source_object_id: null,
      source_last_edited_time: null,
      type: "paragraph",
      paragraph: write.paragraph,
    };
    const replaceHydratedBlocks = vi.fn();
    const controller = createNotesHydrationController({
      hasLocalChanges: () => false,
      readPageGeneration: () => 1,
      readSelectedPageId: () => "page-a",
      readBlockOutlines: () => [{
        id: block.id,
        page_id: "page-a",
        parent: { type: "page_id", page_id: "page-a" },
        type: block.type,
        has_children: false,
        sort_order: 1_000,
        retained_height: 36,
      }],
      readFlatBlockOutlines: () => [],
      readBlocksById: () => ({ [block.id]: block }),
      readFocusRequest: () => ({ blockId: null, requestId: 0, selection: null }),
      mergeBlockOutlines: vi.fn(),
      replaceHydratedBlocks,
      setLoadError: vi.fn(),
    });

    await controller.hydrateBlockRange([block.id], 1);

    expect(backend.hydrateNotesBlocks).not.toHaveBeenCalled();
    expect(replaceHydratedBlocks).not.toHaveBeenCalled();
  });

  it("does not apply hydration from a stale page generation", async () => {
    const { createNotesHydrationController } = await import("./hydration");
    let generation = 1;
    let pageId: string | null = "page-a";
    const replaceHydratedBlocks = vi.fn();
    const controller = createNotesHydrationController({
      hasLocalChanges: () => false,
      readPageGeneration: () => generation,
      readSelectedPageId: () => pageId,
      readBlockOutlines: () => [{
        id: "block-a",
        page_id: "page-a",
        parent: { type: "page_id", page_id: "page-a" },
        type: "paragraph",
        has_children: false,
        sort_order: 1_000,
        retained_height: 36,
      }],
      readFlatBlockOutlines: () => [],
      readBlocksById: () => ({}),
      readFocusRequest: () => ({ blockId: null, requestId: 0, selection: null }),
      mergeBlockOutlines: vi.fn(),
      replaceHydratedBlocks,
      setLoadError: vi.fn(),
    });

    const hydration = controller.hydrateBlockRange(["block-a"], generation);
    generation = 2;
    pageId = "page-b";
    backend.resolve([]);
    await hydration;

    expect(replaceHydratedBlocks).not.toHaveBeenCalled();
  });
  it("hydrates a selection longer than one backend batch without dropping its tail", async () => {
    const { createNotesHydrationController } = await import("./hydration");
    const blocks = Array.from({ length: 225 }, (_, index) => ({
      object: "block", parent: { type: "page_id", page_id: "page-a" }, created_time: "", last_edited_time: "",
      has_children: false, in_trash: false, source_provider: null, source_object_id: null, source_last_edited_time: null,
      ...createBlockWrite(`batch-${index}`, "paragraph", `Text ${index}`),
    } as NotesBlock));
    const outlines = blocks.map((block, index) => notesBlockOutlineFromBlock(block, "page-a", index));
    const replaceHydratedBlocks = vi.fn();
    backend.hydrateNotesBlocks.mockResolvedValueOnce(blocks.slice(0, 200)).mockResolvedValueOnce(blocks.slice(200));
    const controller = createNotesHydrationController({
      readPageGeneration: () => 1, readSelectedPageId: () => "page-a", readBlockOutlines: () => outlines,
      readFlatBlockOutlines: () => outlines.map((outline) => ({ outline, depth: 0 })), readBlocksById: () => ({}),
      hasLocalChanges: () => false, readFocusRequest: () => ({ blockId: null, requestId: 0, selection: null }),
      mergeBlockOutlines: vi.fn(), replaceHydratedBlocks, setLoadError: vi.fn(),
    });
    await controller.hydrateBlockRange(blocks.map((block) => block.id));
    expect(replaceHydratedBlocks.mock.calls[0][1]["page-a"]).toEqual(blocks.map((block) => block.id));
    expect(Object.keys(replaceHydratedBlocks.mock.calls[0][0])).toHaveLength(225);
  });

  it("retains unsaved blocks outside the viewport and uses outline order for hydrated siblings", async () => {
    const { createNotesHydrationController } = await import("./hydration");
    const blocks = Array.from({ length: 125 }, (_, index): NotesBlock => ({
      object: "block", parent: { type: "page_id", page_id: "page-a" },
      created_time: "2026-09-24T00:00:00Z", last_edited_time: "2026-09-24T00:00:00Z",
      has_children: false, in_trash: false, source_provider: null, source_object_id: null, source_last_edited_time: null,
      ...createBlockWrite(`block-${index}`, "paragraph", `Text ${index}`),
    } as NotesBlock));
    const outlines = blocks.map((block, index) => notesBlockOutlineFromBlock(block, "page-a", index));
    const replaceHydratedBlocks = vi.fn();
    const controller = createNotesHydrationController({
      readPageGeneration: () => 1, readSelectedPageId: () => "page-a",
      readBlockOutlines: () => outlines, readFlatBlockOutlines: () => outlines.map((outline) => ({ outline, depth: 0 })),
      readBlocksById: () => ({ [blocks[124].id]: blocks[124], [blocks[0].id]: blocks[0], [blocks[1].id]: blocks[1] }),
      hasLocalChanges: (id) => id === blocks[0].id,
      readFocusRequest: () => ({ blockId: null, requestId: 0, selection: null }),
      mergeBlockOutlines: vi.fn(), replaceHydratedBlocks, setLoadError: vi.fn(),
    });
    await controller.hydrateBlockRange([blocks[124].id]);
    expect(replaceHydratedBlocks).toHaveBeenCalledWith(
      { [blocks[0].id]: blocks[0], [blocks[124].id]: blocks[124] },
      { "page-a": [blocks[0].id, blocks[124].id] },
    );
  });

});
