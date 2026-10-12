import { describe, expect, it, vi } from "vitest";
import {
  applyBlockUpdate,
  blockPlainText,
  blockWithText,
  createBlockWrite,
} from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesBlockUpdate, NotesParent } from "$lib/notes/types";
import { createNotesBlockPersistence } from "./persistence";

const updateNotesBlock = vi.hoisted(() => vi.fn());

vi.mock("$lib/api/notes", () => ({ updateNotesBlock }));

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

});
