import { describe, expect, it } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { notesPasteOperations } from "./paste-persistence";

describe("Notes paste transaction planning", () => {
  it("preserves mixed page and database graph copies between text batches in one ordered plan", () => {
    const parent = { type: "page_id" as const, page_id: "page" };
    const operations = notesPasteOperations([{ parent, after: "current", children: [
      createBlockWrite("before", "paragraph", "Before"),
      createBlockWrite("page-copy", "child_page", "Note"),
      createBlockWrite("database-copy", "child_database", "Tasks"),
      createBlockWrite("after", "paragraph", "After"),
    ] }], { "page-copy": "source-page" }, { "database-copy": "source-database" });
    expect(operations).toEqual([
      { type: "append", request: { parent, after: "current", children: [createBlockWrite("before", "paragraph", "Before")] } },
      { type: "duplicate", request: { block_ids: ["source-page"], duplicated_block_ids: [{ source_id: "source-page", duplicate_id: "page-copy" }], parent, after: "before", before: null, include_trashed_sources: true } },
      { type: "copy_database", request: { id: "database-copy", source_block_id: "source-database", parent, after_block_id: "page-copy" } },
      { type: "append", request: { parent, after: "database-copy", children: [createBlockWrite("after", "paragraph", "After")] } },
    ]);
  });

  it("retains nested destination identities without flattening children into the page", () => {
    const parent = { type: "page_id" as const, page_id: "page" };
    const nested = { type: "block_id" as const, block_id: "toggle" };
    const operations = notesPasteOperations([
      { parent, after: "current", children: [createBlockWrite("toggle", "toggle", "Details")] },
      { parent: nested, after: null, children: [createBlockWrite("child", "paragraph", "Child")] },
    ]);
    expect(operations).toHaveLength(2);
    expect(operations[1]).toMatchObject({ type: "append", request: { parent: nested, after: null } });
  });
});
