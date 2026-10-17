import { describe, expect, it } from "vitest";
import { createTextPayload, createTodoPayload } from "./factory";
import { createDuplicateBlockRequest, collectLoadedBlockSubtreeIds } from "./duplicate";
import type { NotesTreeState } from "./tree";
import type { NotesBlock } from "$lib/notes/types";

const now = "2026-06-30T09:00:00.000Z";

function loadedBlock(id: string, type: "paragraph" | "to_do" = "paragraph"): NotesBlock {
  const base = {
    object: "block" as const,
    id,
    parent: { type: "page_id" as const, page_id: "page-a" },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
  };
  if (type === "to_do") {
    return { ...base, type: "to_do", to_do: createTodoPayload(id) };
  }
  return { ...base, type: "paragraph", paragraph: createTextPayload(id) };
}

describe("Notes block duplication planning", () => {
  it("collects loaded descendants in render order", () => {
    const state: NotesTreeState = {
      blocksById: {
        a: loadedBlock("a"),
        b: loadedBlock("b"),
        c: loadedBlock("c"),
        d: loadedBlock("d"),
      },
      childIdsByParentId: {
        page: ["a"],
        a: ["b", "d"],
        b: ["c"],
      },
    };

    expect(collectLoadedBlockSubtreeIds(state, "a")).toEqual(["a", "b", "c", "d"]);
  });

  it("creates a duplicate request with caller-provided ids", () => {
    const ids = ["copy-a", "copy-b"];
    const state: NotesTreeState = {
      blocksById: {
        a: loadedBlock("a"),
        b: loadedBlock("b", "to_do"),
      },
      childIdsByParentId: {
        a: ["b"],
      },
    };

    const request = createDuplicateBlockRequest(state, "a", () => ids.shift() ?? "copy-missing");

    expect(request.duplicated_block_ids).toEqual([
      { source_id: "a", duplicate_id: "copy-a" },
      { source_id: "b", duplicate_id: "copy-b" },
    ]);
  });
});
