import { createNotesCompoundPersistence } from "$lib/stores/notes/compound-edits";
import { notesPasteOperations } from "$lib/stores/notes/paste-persistence";
import type { NotesDatabasePasteController } from "$lib/stores/notes/database-paste.svelte";
import { normalizeRichTextLinkUrl } from "$lib/notes/rich-text/core";
import { cloneNotesJson } from "$lib/notes/json-clone";
import { notesPasteAppendRequests, planNotesPlainTextPaste } from "$lib/notes/clipboard/blocks";
import { planNotesRichHtmlPaste } from "$lib/notes/rich-text/paste";
import {
  blockColor,
} from "$lib/notes/blocks/color";
import {
  applyBlockUpdate,
  blockEditableRichText,
  blockIndent,
  blockUpdateWithIndent,
  blockPlainText,
  blockWithRichText,
  blockWithToggleOpen,
  createBlockWrite,
} from "$lib/notes/blocks/factory";
import { createBlockWriteFromRichText } from "$lib/notes/blocks/rich-text-write";
import {
  notesEnterSiblingBlockType,
  notesEnterSplitsRichTextBlock,
} from "$lib/notes/blocks/enter";
import { planNotesInsertedBlockFocus } from "$lib/notes/editor/focus";
import { parentIdForBlock } from "$lib/notes/blocks/tree";
import { splitRichTextForBlock } from "$lib/notes/rich-text/split";
import type { NotesTextSelection } from "$lib/notes/editor/selection";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";
import type {
  NotesAppendBlockChildrenRequest,
  NotesBlock,
  NotesBlockUpdate,
  NotesBlockWrite,
  NotesParent,
} from "$lib/notes/types";
import type {
  NotesUndoSnapshot,
} from "$lib/notes/history/undo-history";

const START_OF_BLOCK_SELECTION: NotesTextSelection = { start: 0, end: 0 };

interface OptimisticPastePlan {
  currentUpdate: NotesBlockUpdate;
  appendedBlocks: NotesBlockWrite[];
  blockDepths: number[];
  focusBlockId: string;
  focusOffset: number;
  copiedPageIds?: Readonly<Record<string, string>>;
  copiedDatabaseIds?: Readonly<Record<string, string>>;
}

interface NotesBlockPasteActionsContext {
  databasePaste?: NotesDatabasePasteController;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  readSelectedPageId: () => string | null;
  readChildIdsByParentId: () => Readonly<Record<string, readonly string[]>>;
  blockById: (blockId: string) => NotesBlock | undefined;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  localInsertBlockAfter: (block: NotesBlock, afterBlockId: string | null) => void;
  localInsertBlockBefore: (block: NotesBlock, beforeBlockId: string) => void;
  requestBlockFocus: (blockId: string | null, selection?: NotesTextSelection | null) => void;
  scheduleBlockSave: (blockId: string, update: NotesBlockUpdate) => void;
  saveBlockNow: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  flushBlockSave: (blockId: string) => Promise<void>;
  loadPageTree: (pageId: string) => Promise<void>;
  appendAndApply: (request: NotesAppendBlockChildrenRequest) => Promise<NotesBlock[]>;
  pendingOptimisticWrite: (blockId: string) => Promise<void> | null;
  trackOptimisticBlockWrites: (blockIds: readonly string[], persistence: Promise<void>) => void;
  optimisticBlockFromWrite: (write: NotesBlockWrite, parent: NotesParent) => NotesBlock;
  undoSnapshotForBlocks: (
    blockIds: readonly string[],
    focusBlockId: string | null,
    focusSelection?: NotesTextSelection | null,
  ) => NotesUndoSnapshot | null;
  recordUndo: (
    kind: "create" | "paste",
    before: NotesUndoSnapshot | null,
    after: NotesUndoSnapshot | null,
    groupKey?: string | null,
  ) => void;
}

export interface NotesBlockPasteActions {
  insertParagraphAdjacent: (blockId: string, direction: "previous" | "next") => Promise<void>;
  splitTextBlockAtSelection: (
    blockId: string,
    selectionStart: number,
    selectionEnd: number,
  ) => Promise<void>;
  pastePlainTextIntoBlock: (
    blockId: string,
    selectionStart: number,
    selectionEnd: number,
    plainText: string,
  ) => Promise<boolean>;
  pasteRichHtmlIntoBlock: (
    blockId: string,
    selectionStart: number,
    selectionEnd: number,
    html: string,
  ) => Promise<boolean>;
}

/** Create optimistic split, plain-text paste, and rich-HTML paste actions. */
export function createNotesBlockPasteActions(
  context: NotesBlockPasteActionsContext,
): NotesBlockPasteActions {
  /** Insert a writable sibling beside an atomic row, including either document boundary. */
  async function insertParagraphAdjacent(blockId: string, direction: "previous" | "next"): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || !context.readSelectedPageId()) return;
    const before = context.undoSnapshotForBlocks([blockId], blockId);
    const write = createBlockWrite(crypto.randomUUID(), "paragraph", "");
    const parent = cloneNotesJson(block.parent);
    const inserted = context.optimisticBlockFromWrite(write, parent);
    if (direction === "previous") context.localInsertBlockBefore(inserted, blockId);
    else context.localInsertBlockAfter(inserted, blockId);
    context.requestBlockFocus(write.id, START_OF_BLOCK_SELECTION);
    context.recordUndo("create", before, context.undoSnapshotForBlocks([blockId, write.id], write.id, START_OF_BLOCK_SELECTION));
    const persist = createNotesCompoundPersistence(context, "split", [
      { type: "append", request: { parent, after: blockId, children: [write] } },
      ...(direction === "previous" ? [{ type: "move" as const, block_id: write.id, request: { parent, after: null, before: blockId } }] : []),
    ]);
    const persistence = context.enqueueEditorMutation(async () => { await persist(); });
    context.trackOptimisticBlockWrites([blockId, write.id], persistence);
  }
  async function splitTextBlockAtSelection(
    blockId: string,
    selectionStart: number,
    selectionEnd: number,
  ): Promise<void> {
    const block = context.blockById(blockId);
    const pageId = context.readSelectedPageId();
    if (!block || !notesEnterSplitsRichTextBlock(block.type) || !pageId) return;
    const parentCallout = block.parent.type === "block_id"
      ? context.blockById(block.parent.block_id) : undefined;
    const firstCalloutChild = parentCallout?.type === "callout"
      && blockPlainText(parentCallout).length === 0
      && context.readChildIdsByParentId()[parentCallout.id]?.[0] === blockId;
    if (selectionStart === 0 && selectionEnd === 0 && blockPlainText(block).length > 0
      && (block.type === "toggle" || block.type === "callout" || firstCalloutChild)) {
      const leadingBlock = firstCalloutChild ? parentCallout : block;
      const beforeBlockId = leadingBlock.id;
      const before = context.undoSnapshotForBlocks([blockId, beforeBlockId], blockId, START_OF_BLOCK_SELECTION);
      const newBlockId = crypto.randomUUID();
      const parent = cloneNotesJson(leadingBlock.parent);
      const write = cloneNotesJson({
        id: newBlockId,
        ...blockUpdateWithIndent(
          createBlockWriteFromRichText(newBlockId, block.type === "toggle" && !firstCalloutChild ? "toggle" : "paragraph", [],
            block.type === "toggle" && !firstCalloutChild ? blockColor(block) : "default"),
          blockIndent(leadingBlock),
        ),
      });
      context.localInsertBlockBefore(context.optimisticBlockFromWrite(write, parent), beforeBlockId);
      context.requestBlockFocus(newBlockId, START_OF_BLOCK_SELECTION);
      context.recordUndo(
        "create",
        before,
        context.undoSnapshotForBlocks([blockId, beforeBlockId, newBlockId], newBlockId, START_OF_BLOCK_SELECTION),
        `create:enter:${parentIdForBlock(leadingBlock)}`,
      );
      const persist = createNotesCompoundPersistence(context, "split", [
        { type: "append", request: { parent, after: beforeBlockId, children: [write] } },
        { type: "move", block_id: newBlockId, request: { parent, after: null, before: beforeBlockId } },
      ]);
      const persistence = context.enqueueEditorMutation(async () => { await persist(); });
      context.trackOptimisticBlockWrites([blockId, beforeBlockId, newBlockId], persistence);
      return;
    }
    const beforeSelection = {
      start: Math.min(selectionStart, selectionEnd),
      end: Math.max(selectionStart, selectionEnd),
    };
    const before = context.undoSnapshotForBlocks([blockId], blockId, beforeSelection);
    const split = splitRichTextForBlock(blockEditableRichText(block), selectionStart, selectionEnd);
    const newBlockId = crypto.randomUUID();
    const textUpdate = blockWithRichText(block, split.before);
    const currentUpdate = cloneNotesJson(block.type === "toggle"
      ? blockWithToggleOpen(applyBlockUpdate(block, textUpdate), true)
      : textUpdate);
    const isContainer = block.type === "toggle" || block.type === "callout";
    const nextPayload = createBlockWriteFromRichText(
      newBlockId,
      notesEnterSiblingBlockType(block.type),
      split.after,
      isContainer ? "default" : blockColor(block),
    );
    const nextWrite = cloneNotesJson({ id: newBlockId, ...blockUpdateWithIndent(nextPayload, isContainer ? 0 : blockIndent(block)) });
    const parent = cloneNotesJson(isContainer
      ? { type: "block_id" as const, block_id: block.id }
      : block.parent);
    const after = isContainer ? null : blockId;
    const nextBlock = context.optimisticBlockFromWrite(nextWrite, parent);
    const focusBlockId = planNotesInsertedBlockFocus([newBlockId], blockId) ?? newBlockId;

    context.localApplyBlockUpdate(blockId, currentUpdate);
    context.localInsertBlockAfter(nextBlock, after);
    context.requestBlockFocus(focusBlockId, START_OF_BLOCK_SELECTION);
    context.recordUndo(
      "create",
      before,
      context.undoSnapshotForBlocks(
        [blockId, newBlockId],
        focusBlockId,
        START_OF_BLOCK_SELECTION,
      ),
      `create:enter:${isContainer ? block.id : parentIdForBlock(block)}`,
    );
    const persist = createNotesCompoundPersistence(context, "split", [
      { type: "update", block_id: blockId, update: currentUpdate },
      { type: "append", request: { parent, after, children: [nextWrite] } },
    ]);
    const persistence = context.enqueueEditorMutation(async () => { await persist(); });
    context.trackOptimisticBlockWrites([blockId, newBlockId], persistence);
  }

  function applyOptimisticPaste(
    pageId: string,
    currentBlock: NotesBlock,
    plan: OptimisticPastePlan,
    beforeFocusSelection: NotesTextSelection,
  ): void {
    const affectedIds = [currentBlock.id, ...plan.appendedBlocks.map((write) => write.id)];
    const before = context.undoSnapshotForBlocks(
      affectedIds,
      currentBlock.id,
      beforeFocusSelection,
    );
    const currentUpdate = cloneNotesJson(blockUpdateWithIndent(plan.currentUpdate, blockIndent(currentBlock)));
    const writes = plan.appendedBlocks.map((write, index) => cloneNotesJson({
      id: write.id,
      ...blockUpdateWithIndent(write, plan.blockDepths[index + 1] === 0 ? blockIndent(currentBlock) : 0),
    }));
    const parent = cloneNotesJson(currentBlock.parent);
    const focusSelection = { start: plan.focusOffset, end: plan.focusOffset };
    context.localApplyBlockUpdate(currentBlock.id, currentUpdate);
    const requests = notesPasteAppendRequests(currentBlock.id, parent, writes, plan.blockDepths);
    for (const request of requests) {
      let after = request.after ?? null;
      for (const write of request.children) {
        context.localInsertBlockAfter(context.optimisticBlockFromWrite(write, request.parent), after);
        after = write.id;
      }
    }
    context.requestBlockFocus(plan.focusBlockId, focusSelection);
    context.recordUndo(
      "paste",
      before,
      context.undoSnapshotForBlocks(affectedIds, plan.focusBlockId, focusSelection),
    );
    if (plan.copiedDatabaseIds) context.databasePaste?.beginCopies(plan.copiedDatabaseIds);
    const persistPaste = createNotesCompoundPersistence(context, "paste", [
      { type: "update", block_id: currentBlock.id, update: currentUpdate },
      ...notesPasteOperations(requests, plan.copiedPageIds, plan.copiedDatabaseIds),
    ]);
    const persistence = context.enqueueEditorMutation(async () => {
      const result = await persistPaste();
      for (const created of result.databases) context.databasePaste?.acceptCopy(created.block.id, created);
      if (plan.copiedPageIds) context.applyPostMutation({ sidebarImpact: "hierarchy" });
    });
    context.trackOptimisticBlockWrites(affectedIds, persistence);
    void persistence;
  }

  /** Inspect a single pasted destination without blocking the immediate text insertion. */
  function inspectPastedLink(blockId: string, start: number, end: number): void {
    const current = context.blockById(blockId);
    if (!current || !context.databasePaste) return;
    const value = blockPlainText(current).slice(start, end);
    const url = normalizeRichTextLinkUrl(value);
    if (url) void context.databasePaste.inspectLink(blockId, start, end, url);
  }

  async function pastePlainTextIntoBlock(
    blockId: string,
    selectionStart: number,
    selectionEnd: number,
    plainText: string,
  ): Promise<boolean> {
    const block = context.blockById(blockId);
    const pageId = context.readSelectedPageId();
    if (!block || !pageId) return false;
    const plan = planNotesPlainTextPaste({
      currentBlockId: blockId,
      currentBlockType: block.type,
      currentText: blockPlainText(block),
      selectionStart,
      selectionEnd,
      plainText,
      createId: () => crypto.randomUUID(),
    });
    if (!plan) return false;
    applyOptimisticPaste(pageId, block, plan, {
      start: Math.min(selectionStart, selectionEnd),
      end: Math.max(selectionStart, selectionEnd),
    });
    if (!plan.appendedBlocks.length) inspectPastedLink(blockId, Math.min(selectionStart, selectionEnd), plan.focusOffset);
    return true;
  }

  async function pasteRichHtmlIntoBlock(
    blockId: string,
    selectionStart: number,
    selectionEnd: number,
    html: string,
  ): Promise<boolean> {
    const block = context.blockById(blockId);
    const pageId = context.readSelectedPageId();
    if (!block || !pageId) return false;
    const plan = planNotesRichHtmlPaste({
      currentBlock: block,
      selectionStart,
      selectionEnd,
      html,
      createId: () => crypto.randomUUID(),
    });
    if (!plan) return false;
    applyOptimisticPaste(pageId, block, plan, {
      start: Math.min(selectionStart, selectionEnd),
      end: Math.max(selectionStart, selectionEnd),
    });
    if (!plan.appendedBlocks.length) inspectPastedLink(blockId, Math.min(selectionStart, selectionEnd), plan.focusOffset);
    return true;
  }

  return { insertParagraphAdjacent, splitTextBlockAtSelection, pastePlainTextIntoBlock, pasteRichHtmlIntoBlock };
}
