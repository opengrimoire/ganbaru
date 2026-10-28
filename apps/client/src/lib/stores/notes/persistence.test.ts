import { describe, expect, it, vi } from "vitest";
import {
  applyBlockUpdate,
  blockConvertedToType,
  blockPlainText,
  blockWithText,
  createBlockWrite,
} from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesBlockUpdate, NotesParent } from "$lib/notes/types";
import { createNotesBlockPersistence, type NotesBlockEditConflict } from "./persistence";

const { updateNotesBlock, appendNotesBlockChildren, trashNotesBlock } = vi.hoisted(() => ({
  updateNotesBlock: vi.fn(),
  appendNotesBlockChildren: vi.fn(),
  trashNotesBlock: vi.fn(),
}));

vi.mock("$lib/api/notes", () => ({ updateNotesBlock, appendNotesBlockChildren, trashNotesBlock }));

const refused = () => new Error(`Notes edit conflict: block ${blockId} changed; reload before retrying`);

/** A Notes block store with one block and a canonical copy another device changed. */
/** Plain text of a block the test expects to exist. */
function presentText(block: NotesBlock | undefined): string {
  if (!block) throw new Error("expected a block");
  return blockPlainText(block);
}

function conflictHarness(local: NotesBlock, canonical: NotesBlock | null) {
  let current: NotesBlock | undefined = local;
  const conflicts: Array<NotesBlockEditConflict | null> = [];
  const onCanonicalChanged = vi.fn();
  const persistence = createNotesBlockPersistence({
    beforeSave: async () => undefined,
    readBlock: () => current,
    replaceBlock: (block) => { current = block; },
    setLoadError: vi.fn(),
    readCanonicalBlock: async () => canonical,
    onBlockConflict: (_id, conflict) => conflicts.push(conflict),
    onCanonicalChanged,
    debounceMs: 250,
  });
  return { persistence, conflicts, onCanonicalChanged, read: () => current };
}

const blockId = "00000000-0000-4000-8000-000000000001";
const pageId = "00000000-0000-4000-8000-000000000002";
const parent: NotesParent = { type: "page_id", page_id: pageId };

function paragraph(text: string): NotesBlock {
  const write = createBlockWrite(blockId, "paragraph", text);
  if (write.type !== "paragraph") throw new Error("expected paragraph write");
  return {
    object: "block",
    id: write.id,
    parent,
    created_time: "2026-07-10T12:00:00.000Z",
    last_edited_time: "2026-07-10T12:00:00.000Z",
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: "paragraph",
    paragraph: write.paragraph,
  };
}

describe("notes block persistence", () => {
  it.each([false, true])("releases committed dirtiness for absent blocks while preserving a later edit (%s)", async (laterEdit) => {
    let complete!: () => void;
    const nativeCommit = vi.fn(() => new Promise<void>((resolve) => { complete = resolve; }));
    const persistence = createNotesBlockPersistence({
      readBlock: () => undefined, beforeSave: async () => undefined,
      replaceBlock: vi.fn(), setLoadError: vi.fn(), debounceMs: 250,
    });
    persistence.markBlockLocallyChanged(blockId);
    const commit = persistence.enqueueEditorMutation(nativeCommit);
    await vi.waitFor(() => expect(nativeCommit).toHaveBeenCalledOnce());
    if (laterEdit) persistence.markBlockLocallyChanged(blockId);
    complete();
    await commit;
    expect(persistence.hasLocalChanges(blockId)).toBe(laterEdit);
  });

  it("does not restore a locally deleted block when an earlier save finishes", async () => {
    let current: NotesBlock | undefined = paragraph("");
    let finishSave!: (block: NotesBlock) => void;
    updateNotesBlock.mockReset().mockImplementation(() => new Promise<NotesBlock>((resolve) => {
      finishSave = resolve;
    }));
    const replaceBlock = vi.fn((block: NotesBlock) => { current = block; });
    const persistence = createNotesBlockPersistence({
      beforeSave: async () => undefined,
      readBlock: () => current,
      replaceBlock,
      setLoadError: () => undefined,
      debounceMs: 250,
    });

    const update = blockWithText(current, "Draft");
    persistence.localApplyBlockUpdate(blockId, update);
    const save = persistence.saveBlockNow(blockId, update);
    await vi.waitFor(() => expect(updateNotesBlock).toHaveBeenCalledOnce());
    current = undefined;
    finishSave(applyBlockUpdate(paragraph(""), update));
    await save;

    expect(current).toBeUndefined();
    expect(replaceBlock).toHaveBeenCalledOnce();
    expect(persistence.hasLocalChanges(blockId)).toBe(false);
  });

  it("serializes saves and ignores acknowledgements older than local text", async () => {
    updateNotesBlock.mockReset();
    let current = paragraph("");
    const requests: Array<{
      update: NotesBlockUpdate;
      resolve: (block: NotesBlock) => void;
    }> = [];
    updateNotesBlock.mockImplementation((_id: string, update: NotesBlockUpdate) =>
      new Promise<NotesBlock>((resolve) => {
        requests.push({ update, resolve });
      })
    );
    const persistence = createNotesBlockPersistence({
      beforeSave: () => Promise.resolve(),
      readBlock: () => current,
      replaceBlock: (block) => {
        current = block;
      },
      setLoadError: () => undefined,
      debounceMs: 250,
    });

    const firstUpdate = blockWithText(current, "a");
    persistence.localApplyBlockUpdate(blockId, firstUpdate);
    expect(persistence.hasLocalChanges(blockId)).toBe(true);
    const firstSave = persistence.saveBlockNow(blockId, firstUpdate);
    const secondUpdate = blockWithText(current, "ab");
    persistence.localApplyBlockUpdate(blockId, secondUpdate);
    const secondSave = persistence.saveBlockNow(blockId, secondUpdate);

    await vi.waitFor(() => expect(requests).toHaveLength(1));
    requests[0].resolve(applyBlockUpdate(paragraph(""), requests[0].update));
    await firstSave;
    expect(blockPlainText(current)).toBe("ab");

    await vi.waitFor(() => expect(requests).toHaveLength(2));
    requests[1].resolve(applyBlockUpdate(paragraph("a"), requests[1].update));
    await secondSave;
    expect(blockPlainText(current)).toBe("ab");
    expect(persistence.hasLocalChanges(blockId)).toBe(false);
  });
  it("retains failed writes and retries them before dependent operations", async () => {
    let current = paragraph("Draft");
    const error = vi.fn();
    const persistence = createNotesBlockPersistence({
      readBlock: () => current, beforeSave: async () => undefined,
      replaceBlock: (block) => { current = block; }, setLoadError: error, debounceMs: 250,
    });
    updateNotesBlock.mockReset().mockRejectedValueOnce(new Error("Disk unavailable"))
      .mockImplementation(async (_id: string, update: NotesBlockUpdate) => applyBlockUpdate(current, update));
    persistence.localApplyBlockUpdate(blockId, blockWithText(current, "Unsaved"));
    const first = persistence.saveBlockNow(blockId, blockWithText(current, "Unsaved"));
    const dependent = vi.fn(async () => undefined);
    const second = persistence.enqueueEditorMutation(dependent);
    await expect(first).rejects.toThrow("Disk unavailable");
    await expect(second).rejects.toThrow("Disk unavailable");
    expect(dependent).not.toHaveBeenCalled();
    expect(blockPlainText(current)).toBe("Unsaved");
    await persistence.retryEditorMutations();
    expect(updateNotesBlock).toHaveBeenCalledTimes(2);
    expect(dependent).toHaveBeenCalledOnce();
    expect(error).toHaveBeenLastCalledWith(null);
  });

  it("releases failed writes after their page is confirmed inactive", async () => {
    let current = paragraph("Draft");
    const error = vi.fn();
    const persistence = createNotesBlockPersistence({
      readBlock: () => current,
      beforeSave: async () => undefined,
      replaceBlock: (block) => { current = block; },
      setLoadError: error,
      debounceMs: 250,
    });
    updateNotesBlock.mockReset().mockRejectedValueOnce(new Error("notes page not found"))
      .mockImplementation(async (_id: string, update: NotesBlockUpdate) => applyBlockUpdate(current, update));
    const draft = blockWithText(current, "Unsaved");
    persistence.localApplyBlockUpdate(blockId, draft);
    await expect(persistence.saveBlockNow(blockId, draft)).rejects.toThrow("notes page not found");
    expect(persistence.hasLocalChanges(blockId)).toBe(true);

    persistence.discardPendingEditorWrites();
    expect(persistence.hasLocalChanges(blockId)).toBe(false);
    expect(error).toHaveBeenLastCalledWith(null);
    await expect(persistence.flushPendingBlockSaves()).resolves.toBeUndefined();
    await persistence.saveBlockNow(blockId, blockWithText(current, "Next page"));
    expect(updateNotesBlock).toHaveBeenCalledTimes(2);
  });

  it("ignores a removed page's late save failure after releasing its writes", async () => {
    let rejectSave!: (error: Error) => void;
    updateNotesBlock.mockReset().mockImplementation(() => new Promise<NotesBlock>((_resolve, reject) => {
      rejectSave = reject;
    }));
    let current = paragraph("Draft");
    const error = vi.fn();
    const persistence = createNotesBlockPersistence({
      readBlock: () => current,
      beforeSave: async () => undefined,
      replaceBlock: (block) => { current = block; },
      setLoadError: error,
      debounceMs: 250,
    });

    const save = persistence.saveBlockNow(blockId, blockWithText(current, "Unsaved"));
    await vi.waitFor(() => expect(updateNotesBlock).toHaveBeenCalledOnce());
    persistence.discardPendingEditorWrites();
    rejectSave(new Error("notes page not found"));
    await expect(save).rejects.toThrow("notes page not found");
    expect(error).toHaveBeenLastCalledWith(null);
    await expect(persistence.flushPendingBlockSaves()).resolves.toBeUndefined();
  });

  it("sends the content revision and abandons a refused edit instead of holding the queue", async () => {
    const current: NotesBlock = { ...paragraph("Base"), content_revision: "a".repeat(64) };
    updateNotesBlock.mockReset().mockRejectedValueOnce(new Error(`Notes edit conflict: block ${blockId} changed; reload before retrying`));
    const setLoadError = vi.fn();
    const onConflict = vi.fn();
    const onCommitted = vi.fn();
    const persistence = createNotesBlockPersistence({
      beforeSave: async () => undefined, readBlock: () => current, replaceBlock: vi.fn(),
      setLoadError, onConflict, onCommitted, debounceMs: 250,
    });
    const update = blockWithText(current, "Local");
    persistence.localApplyBlockUpdate(blockId, update);

    await expect(persistence.saveBlockNow(blockId, update)).resolves.toBeUndefined();

    expect(updateNotesBlock).toHaveBeenCalledWith(blockId, update, "a".repeat(64));
    expect(persistence.hasLocalChanges(blockId)).toBe(false);
    expect(onConflict).toHaveBeenCalledOnce();
    expect(onCommitted).not.toHaveBeenCalled();
    expect(setLoadError).not.toHaveBeenCalledWith(expect.any(String));
    const later = vi.fn(async () => undefined);
    await persistence.enqueueEditorMutation(later);
    expect(later).toHaveBeenCalledOnce();
    expect(onCommitted).toHaveBeenCalledOnce();
  });

  it("runs a canonical read after earlier writes without blocking later ones when it fails", async () => {
    const order: string[] = [];
    let finishWrite!: () => void;
    const persistence = createNotesBlockPersistence({
      beforeSave: async () => undefined, readBlock: () => undefined, replaceBlock: vi.fn(),
      setLoadError: vi.fn(), debounceMs: 250,
    });
    const write = persistence.enqueueEditorMutation(() => new Promise<void>((resolve) => {
      finishWrite = () => { order.push("write"); resolve(); };
    }));
    const read = persistence.runBetweenWrites(async () => {
      order.push("read");
      throw new Error("read failed");
    });
    const later = persistence.enqueueEditorMutation(async () => { order.push("later"); });
    await vi.waitFor(() => expect(finishWrite).toBeTypeOf("function"));
    finishWrite();

    await write;
    await expect(read).rejects.toThrow("read failed");
    await later;
    expect(order).toEqual(["write", "read", "later"]);
  });
});

describe("notes block persistence after a remote change", () => {
  const base = { ...paragraph("Base"), content_revision: "a".repeat(64) };

  it("replays the edit on canonical content whose kind changed and saves on its revision", async () => {
    const remote = applyBlockUpdate(base, blockConvertedToType(base, "heading_1"));
    const canonical = { ...remote, content_revision: "b".repeat(64) };
    updateNotesBlock.mockReset().mockRejectedValueOnce(refused())
      .mockImplementation(async (_id: string, update: NotesBlockUpdate) => applyBlockUpdate(canonical, update));
    const { persistence, conflicts, read } = conflictHarness(base, canonical);
    const draft = blockWithText(base, "Base typed");
    persistence.localApplyBlockUpdate(blockId, draft);

    await persistence.saveBlockNow(blockId, draft);

    expect(updateNotesBlock).toHaveBeenCalledTimes(2);
    const [, merged, revision] = updateNotesBlock.mock.calls[1] as [string, NotesBlockUpdate, string];
    expect(revision).toBe("b".repeat(64));
    expect(merged.type).toBe("heading_1");
    expect(read()?.type).toBe("heading_1");
    expect(presentText(read())).toBe("Base typed");
    expect(persistence.hasLocalChanges(blockId)).toBe(false);
    expect(conflicts).toEqual([]);
  });

  it("holds an edit whose text changed on both sides and keeps the draft until the user chooses", async () => {
    const canonical = { ...paragraph("Theirs"), content_revision: "b".repeat(64) };
    updateNotesBlock.mockReset().mockRejectedValueOnce(refused())
      .mockImplementation(async (_id: string, update: NotesBlockUpdate) => applyBlockUpdate(canonical, update));
    const { persistence, conflicts, read } = conflictHarness(base, canonical);
    const draft = blockWithText(base, "Mine");
    persistence.localApplyBlockUpdate(blockId, draft);

    await persistence.saveBlockNow(blockId, draft);
    await persistence.saveBlockNow(blockId, draft);

    expect(updateNotesBlock).toHaveBeenCalledOnce();
    expect(conflicts).toEqual([{ blockId, kind: "text", mine: draft, theirs: canonical }]);
    expect(persistence.readBlockConflict(blockId)?.kind).toBe("text");
    expect(persistence.hasLocalChanges(blockId)).toBe(true);
    expect(presentText(read())).toBe("Mine");

    await persistence.resolveBlockConflict(blockId, "mine");

    expect(updateNotesBlock).toHaveBeenLastCalledWith(blockId, expect.objectContaining({ type: "paragraph" }), "b".repeat(64));
    expect(presentText(read())).toBe("Mine");
    expect(conflicts.at(-1)).toBeNull();
    expect(persistence.readBlockConflict(blockId)).toBeUndefined();
    expect(persistence.hasLocalChanges(blockId)).toBe(false);
  });

  it("keeps both versions by inserting the draft as a new block after the canonical one", async () => {
    const canonical = { ...paragraph("Theirs"), content_revision: "b".repeat(64) };
    updateNotesBlock.mockReset().mockRejectedValueOnce(refused());
    appendNotesBlockChildren.mockReset().mockResolvedValue({ results: [] });
    const { persistence, onCanonicalChanged, read } = conflictHarness(base, canonical);
    const draft = blockWithText(base, "Mine");
    persistence.localApplyBlockUpdate(blockId, draft);
    await persistence.saveBlockNow(blockId, draft);

    await persistence.resolveBlockConflict(blockId, "both");

    const request = appendNotesBlockChildren.mock.calls[0]?.[0] as { parent: NotesParent; after: string; children: NotesBlock[] };
    expect(request.parent).toEqual(parent);
    expect(request.after).toBe(blockId);
    expect(request.children).toHaveLength(1);
    expect(request.children[0]?.id).not.toBe(blockId);
    expect(presentText(request.children[0])).toBe("Mine");
    expect(presentText(read())).toBe("Theirs");
    expect(onCanonicalChanged).toHaveBeenCalledOnce();
    expect(updateNotesBlock).toHaveBeenCalledOnce();
  });

  it("takes the canonical content when the user discards the draft", async () => {
    const canonical = { ...paragraph("Theirs"), content_revision: "b".repeat(64) };
    updateNotesBlock.mockReset().mockRejectedValueOnce(refused());
    const { persistence, read } = conflictHarness(base, canonical);
    const draft = blockWithText(base, "Mine");
    persistence.localApplyBlockUpdate(blockId, draft);
    await persistence.saveBlockNow(blockId, draft);

    await persistence.resolveBlockConflict(blockId, "theirs");

    expect(presentText(read())).toBe("Theirs");
    expect(persistence.hasLocalChanges(blockId)).toBe(false);
    expect(updateNotesBlock).toHaveBeenCalledOnce();
  });

  it("restores a block removed elsewhere when the user keeps the draft", async () => {
    updateNotesBlock.mockReset().mockRejectedValueOnce(refused())
      .mockImplementation(async (_id: string, update: NotesBlockUpdate) => applyBlockUpdate(base, update));
    trashNotesBlock.mockReset().mockResolvedValue({ ...base, content_revision: "c".repeat(64) });
    const { persistence, conflicts, onCanonicalChanged } = conflictHarness(base, null);
    const draft = blockWithText(base, "Mine");
    persistence.localApplyBlockUpdate(blockId, draft);
    await persistence.saveBlockNow(blockId, draft);
    expect(conflicts[0]?.kind).toBe("removed");

    await persistence.resolveBlockConflict(blockId, "mine");

    expect(trashNotesBlock).toHaveBeenCalledWith(blockId, false);
    expect(updateNotesBlock).toHaveBeenLastCalledWith(blockId, expect.anything(), "c".repeat(64));
    expect(onCanonicalChanged).toHaveBeenCalledOnce();
  });
});
