import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, blockPlainText, createBlockUpdate, createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesBlock } from "$lib/notes/types";
import type { NotesCompoundEdit, NotesCompoundEditResult, NotesEditOperation } from "$lib/api/notes/compound-edits";
import { createNotesCompoundPersistence, notesEditReferences } from "./compound-edits";

const apply = vi.hoisted(() => vi.fn<(request: NotesCompoundEdit) => Promise<NotesCompoundEditResult>>());
vi.mock("$lib/api/notes/compound-edits", () => ({ applyNotesCompoundEdit: apply }));
const pageId = "10000000-0000-4000-8000-000000000001";
const blockId = "10000000-0000-4000-8000-000000000002";
const revision = (value: number) => value.toString(16).padStart(64, "0");

function block(text: string, version = 1): NotesBlock {
  return { ...createBlockWrite(blockId, "paragraph", text), object: "block", parent: { type: "page_id", page_id: pageId }, edit_revision: revision(version), created_time: "", last_edited_time: "", has_children: false, in_trash: false, source_provider: null, source_object_id: null, source_last_edited_time: null } as NotesBlock;
}

function harness(initial = block("Original")) {
  let current: NotesBlock | undefined = initial;
  const acknowledged = vi.fn();
  const context = {
    readSelectedPageId: () => pageId,
    blockById: () => current,
    reconcileCanonicalBlocks: acknowledged,
    applyPostMutation: (result: { blocks?: readonly NotesBlock[] }) => { if (result.blocks?.[0]) current = result.blocks[0]; },
  };
  return { context, acknowledged, read: () => current, replace: (value: NotesBlock | undefined) => { current = value; } };
}

function receipt(request: NotesCompoundEdit, saved: NotesBlock): NotesCompoundEditResult {
  return { operation_id: request.operation_id, page_id: request.page_id, blocks: [saved], databases: [], placements: [] };
}

describe("Notes compound edit reconciliation", () => {
  beforeEach(() => { apply.mockReset(); });

  it("retries exactly the same identity, payload and preconditions after an uncertain response", async () => {
    const h = harness();
    const operations: NotesEditOperation[] = [{ type: "update", block_id: blockId, update: createBlockUpdate("paragraph", "Saved") }];
    const persist = createNotesCompoundPersistence(h.context, "format_selection", operations);
    apply.mockRejectedValueOnce(new Error("Response lost")).mockImplementation(async (request) => receipt(request, block("Saved", 2)));
    await expect(persist()).rejects.toThrow("Response lost");
    operations[0] = { type: "trash", block_id: blockId, in_trash: true };
    h.replace(block("Later local draft"));
    await persist();
    expect(apply.mock.calls[1][0]).toEqual(apply.mock.calls[0][0]);
    expect(apply.mock.calls[1][0].operations[0].type).toBe("update");
    expect(blockPlainText(h.read()!)).toBe("Later local draft");
    expect(h.read()?.edit_revision).toBe(revision(2));
  });

  it("uses an earlier queued acknowledgement as its first-dispatch precondition", async () => {
    const h = harness();
    const persist = createNotesCompoundPersistence(h.context, "split", [{ type: "update", block_id: blockId, update: createBlockUpdate("paragraph", "Split") }]);
    h.replace(block("Split", 2));
    apply.mockImplementation(async (request) => receipt(request, block("Split", 3)));
    await persist();
    expect(apply.mock.calls[0][0].expected_blocks[blockId]).toBe(revision(2));
    expect(blockPlainText(h.read()!)).toBe("Split");
  });

  it("does not apply a replayed receipt over a newer canonical refresh", async () => {
    const h = harness();
    const persist = createNotesCompoundPersistence(h.context, "merge", [{ type: "update", block_id: blockId, update: createBlockUpdate("paragraph", "Merged") }]);
    apply.mockRejectedValueOnce(new Error("Response lost")).mockImplementation(async (request) => receipt(request, block("Merged", 2)));
    await expect(persist()).rejects.toThrow();
    h.replace(block("Newer native edit", 3));
    const result = await persist();
    expect(result.blocks).toEqual([]);
    expect(h.acknowledged).toHaveBeenCalledWith([]);
    expect(h.read()).toEqual(block("Newer native edit", 3));
  });

  it("acknowledges the canonical token while preserving typing made during the request", async () => {
    const h = harness();
    let complete!: (value: NotesCompoundEditResult) => void;
    apply.mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
    const persist = createNotesCompoundPersistence(h.context, "format_selection", [{ type: "update", block_id: blockId, update: createBlockUpdate("paragraph", "Saved") }]);
    const pending = persist();
    h.replace(applyBlockUpdate(h.read()!, createBlockUpdate("paragraph", "Saved plus typing")));
    complete(receipt(apply.mock.calls[0][0], block("Saved", 2)));
    const result = await pending;
    expect(blockPlainText(h.read()!)).toBe("Saved plus typing");
    expect(h.read()?.edit_revision).toBe(revision(2));
    expect(blockPlainText(result.blocks[0])).toBe("Saved plus typing");
  });

  it("preserves later typing when a successfully acknowledged receipt is replayed", async () => {
    const h = harness();
    const persist = createNotesCompoundPersistence(h.context, "format_selection", [{ type: "update", block_id: blockId, update: createBlockUpdate("paragraph", "Saved") }]);
    apply.mockImplementation(async (request) => receipt(request, block("Saved", 2)));
    await persist();
    h.replace(applyBlockUpdate(h.read()!, createBlockUpdate("paragraph", "Saved plus later draft")));
    const result = await persist();
    expect(blockPlainText(h.read()!)).toBe("Saved plus later draft");
    expect(blockPlainText(result.blocks[0])).toBe("Saved plus later draft");
    expect(h.read()?.edit_revision).toBe(revision(2));
  });

  it("uses retained acknowledgements for a block already removed from the optimistic tree", async () => {
    const original = block("Deleted");
    const h = harness(original);
    h.replace(undefined);
    const persist = createNotesCompoundPersistence({ ...h.context, readCanonicalRevision: () => revision(2) }, "delete_selection", [{ type: "trash", block_id: blockId, in_trash: true }], [original]);
    apply.mockImplementation(async (request) => receipt(request, { ...block("Deleted", 3), in_trash: true }));
    await persist();
    expect(apply.mock.calls[0][0].expected_blocks[blockId]).toBe(revision(2));
    expect(h.read()).toBeUndefined();
    expect(h.acknowledged.mock.calls[0][0][0].in_trash).toBe(true);
  });

  it("requires persisted anchors but excludes identities created earlier in the same plan", () => {
    const parent = { type: "page_id" as const, page_id: pageId };
    expect(notesEditReferences([
      { type: "append", request: { parent, after: blockId, children: [createBlockWrite("new", "toggle", "")] } },
      { type: "copy_database", request: { id: "copy", source_block_id: "external-source", parent: { type: "block_id", block_id: "new" }, after_block_id: null } },
      { type: "move", block_id: "new", request: { parent, after: null, before: blockId } },
    ])).toEqual([blockId]);
  });
});
