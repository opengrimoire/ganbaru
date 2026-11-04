import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  MAX_BLOCK_FRONTIER_PARENT_IDS,
  getNotesBlockFrontier,
  getNotesBlockOutlineFrontier,
} from "./content";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: async () => "sqlite:test" }));

const invokeMock = vi.mocked(invoke);
const pageId = "00000000-0000-4000-8000-000000000001";

function parentIds(count: number): string[] {
  return Array.from({ length: count }, (_, index) => `00000000-0000-4000-9000-${index.toString(16).padStart(12, "0")}`);
}

function outline(id: string, parentId: string) {
  return {
    id,
    page_id: pageId,
    parent: { type: "block_id", block_id: parentId },
    type: "paragraph",
    sort_order: 1,
    has_children: false,
    retained_height: 36,
  };
}

function sentParentIds(): string[][] {
  return invokeMock.mock.calls.map(([, args]) => (args as { parentIds: string[] }).parentIds);
}

describe("Notes block frontier reads", () => {
  beforeEach(() => vi.clearAllMocks());

  it("sends a level at the native cap in one read", async () => {
    invokeMock.mockResolvedValue({ blocks: [] });
    const ids = parentIds(MAX_BLOCK_FRONTIER_PARENT_IDS);

    await expect(getNotesBlockFrontier(ids)).resolves.toEqual({ blocks: [] });

    expect(sentParentIds()).toEqual([ids]);
  });

  it("splits a larger level into ordered reads within the cap and joins the results", async () => {
    const ids = parentIds(MAX_BLOCK_FRONTIER_PARENT_IDS * 2 + 1);
    invokeMock
      .mockResolvedValueOnce([outline("00000000-0000-4000-8000-0000000000a1", ids[0])])
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([outline("00000000-0000-4000-8000-0000000000a2", ids.at(-1) ?? "")]);

    const outlines = await getNotesBlockOutlineFrontier(pageId, ids);

    expect(outlines.map((item) => item.id)).toEqual([
      "00000000-0000-4000-8000-0000000000a1",
      "00000000-0000-4000-8000-0000000000a2",
    ]);
    const sent = sentParentIds();
    expect(sent.map((batch) => batch.length)).toEqual([MAX_BLOCK_FRONTIER_PARENT_IDS, MAX_BLOCK_FRONTIER_PARENT_IDS, 1]);
    expect(sent.flat()).toEqual(ids);
  });

  it("skips the native read when a level has no parents", async () => {
    await expect(getNotesBlockFrontier([])).resolves.toEqual({ blocks: [] });
    await expect(getNotesBlockOutlineFrontier(pageId, [])).resolves.toEqual([]);

    expect(invokeMock).not.toHaveBeenCalled();
  });
});
