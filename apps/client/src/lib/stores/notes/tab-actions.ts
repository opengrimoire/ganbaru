import { createNotesCompoundPersistence, enqueueNotesCompoundEdit } from "./compound-edits";
import { reconcileNotesCompoundUndo } from "$lib/notes/history/undo-compound";
import {
  moveNotesBlock,
} from "$lib/api/notes";
import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";
import {
  createNotesTabLabelWrite,
  notesTabCanAdd,
  notesTabCanMove,
  notesTabCanRemove,
  notesTabLabelWithIcon,
  notesTabLabelWithText,
  type NotesTabMoveDirection,
} from "$lib/notes/navigation/tabs";
import type {
  NotesBlock,
  NotesAppendBlockChildrenRequest,
  NotesBlockUpdate,
  NotesIcon,
  NotesTabBlockItems,
} from "$lib/notes/types";
import type {
  NotesUndoKind,
  NotesUndoRecordOptions,
  NotesUndoSnapshot,
} from "$lib/notes/history/undo-history";
import type { NotesTreeState } from "$lib/notes/blocks/tree";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import {
  notesPostMoveResult,
  type NotesPostMutationResult,
} from "$lib/notes/post-mutation";

export interface NotesTabActionsContext {
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  readSelectedPageId: () => string | null;
  readChildIdsByParentId: () => Record<string, string[]>;
  treeState: () => NotesTreeState;
  blockById: (blockId: string) => NotesBlock | undefined;
  tabItemsForBlock: (blockId: string) => NotesTabBlockItems[];
  requestBlockFocus: (blockId: string | null) => void;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  saveBlockNow: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
  createUndoSnapshot: (
    focusBlockId: string | null,
    extraBlocks?: readonly NotesBlock[],
  ) => NotesUndoSnapshot | null;
  recordUndo: (options: Omit<NotesUndoRecordOptions, "id">) => void;
}

export interface NotesTabActions {
  updateTabLabel: (labelBlockId: string, label: string) => Promise<void>;
  updateTabIcon: (labelBlockId: string, icon: NotesIcon | null) => Promise<void>;
  addTab: (tabBlockId: string, afterTabIndex: number) => Promise<void>;
  removeTab: (tabBlockId: string, labelBlockId: string) => Promise<void>;
  moveTab: (
    tabBlockId: string,
    labelBlockId: string,
    direction: NotesTabMoveDirection,
  ) => Promise<void>;
  moveBlockToTab: (blockId: string, labelBlockId: string) => Promise<void>;
}

/**
 * Create Notes tab layout mutation and UI action methods.
 */
export function createNotesTabActions(context: NotesTabActionsContext): NotesTabActions {
  function undoSnapshot(
    focusBlockId: string | null,
    extraBlocks: readonly NotesBlock[] = [],
  ): NotesUndoSnapshot | null {
    return context.createUndoSnapshot(focusBlockId, extraBlocks);
  }

  function recordUndo(
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    after: NotesUndoSnapshot | null,
    groupKey: string | null = null,
  ): void {
    context.recordUndo({ kind, before, after, groupKey });
  }

  function recordUndoAfter(
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
    groupKey: string | null = null,
    extraBlocks: readonly NotesBlock[] = [],
  ): void {
    recordUndo(kind, before, undoSnapshot(focusBlockId, extraBlocks), groupKey);
  }

  function tabItems(tabBlockId: string): NotesTabBlockItems[] {
    return context.tabItemsForBlock(tabBlockId);
  }

  function activeChildBlockIds(parentBlockId: string): string[] {
    return (context.readChildIdsByParentId()[parentBlockId] ?? []).filter((blockId) => {
      const block = context.blockById(blockId);
      return block !== undefined && !block.in_trash;
    });
  }

  function tabBlockIdForLabel(labelBlockId: string): string | null {
    const label = context.blockById(labelBlockId);
    if (!label || label.type !== "paragraph" || label.parent.type !== "block_id") return null;
    const tab = context.blockById(label.parent.block_id);
    return tab?.type === "tab" ? tab.id : null;
  }

  async function updateTabLabel(labelBlockId: string, label: string): Promise<void> {
    const labelBlock = context.blockById(labelBlockId);
    if (!labelBlock || labelBlock.type !== "paragraph") return;
    if (!tabBlockIdForLabel(labelBlockId)) return;
    const trimmed = label.trim();
    if (!trimmed) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(labelBlockId);
    const update = notesTabLabelWithText(labelBlock, trimmed);
    context.localApplyBlockUpdate(labelBlockId, update);
    await context.saveBlockNow(labelBlockId, update);
    context.requestBlockFocus(labelBlockId);
    recordUndoAfter("update", before, labelBlockId, `update:${labelBlockId}:tab-label`);
  }

  async function updateTabIcon(labelBlockId: string, icon: NotesIcon | null): Promise<void> {
    const labelBlock = context.blockById(labelBlockId);
    if (!labelBlock || labelBlock.type !== "paragraph") return;
    if (!tabBlockIdForLabel(labelBlockId)) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(labelBlockId);
    const update = notesTabLabelWithIcon(labelBlock, icon);
    context.localApplyBlockUpdate(labelBlockId, update);
    await context.saveBlockNow(labelBlockId, update);
    context.requestBlockFocus(labelBlockId);
    recordUndoAfter("update", before, labelBlockId, `update:${labelBlockId}:tab-icon`);
  }

  async function addTab(tabBlockId: string, afterTabIndex: number): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    const tabBlock = context.blockById(tabBlockId);
    if (!selectedPageId || !tabBlock || tabBlock.type !== "tab") return;
    const tabs = tabItems(tabBlockId);
    if (!notesTabCanAdd(tabs.length)) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(tabBlockId);
    const labelBlockId = crypto.randomUUID();
    const contentBlockId = crypto.randomUUID();
    const insertAfter = tabs[Math.max(0, afterTabIndex)]?.label.id ?? tabs.at(-1)?.label.id ?? null;
    const labelRequest = {
      parent: { type: "block_id", block_id: tabBlockId },
      after: insertAfter,
      children: [createNotesTabLabelWrite(labelBlockId, `Tab ${tabs.length + 1}`)],
    } satisfies NotesAppendBlockChildrenRequest;
    const contentRequest = {
      parent: { type: "block_id", block_id: labelBlockId },
      after: null,
      children: [createBlockWrite(contentBlockId, "paragraph")],
    } satisfies NotesAppendBlockChildrenRequest;
    await enqueueNotesCompoundEdit(context, "tab_layout", [
      { type: "append", request: labelRequest },
      { type: "append", request: contentRequest },
    ]);
    context.requestBlockFocus(contentBlockId);
    recordUndoAfter("create", before, contentBlockId);
  }

  async function removeTab(tabBlockId: string, labelBlockId: string): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    const tabBlock = context.blockById(tabBlockId);
    if (!selectedPageId || !tabBlock || tabBlock.type !== "tab") return;
    const tabs = tabItems(tabBlockId);
    if (!notesTabCanRemove(tabs.length)) return;
    const removeIndex = tabs.findIndex((tab) => tab.label.id === labelBlockId);
    if (removeIndex < 0) return;
    const removed = tabs[removeIndex];
    const target = tabs[removeIndex > 0 ? removeIndex - 1 : removeIndex + 1];
    if (!removed || !target) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(tabBlockId);
    const movedChildIds = activeChildBlockIds(removed.label.id);
    const targetChildIds = activeChildBlockIds(target.label.id);
    const persist = createNotesCompoundPersistence(context, "tab_layout", [
      { type: "move_children", source_block_id: removed.label.id, parent: { type: "block_id", block_id: target.label.id }, after: targetChildIds.at(-1) ?? null },
      { type: "trash", block_id: removed.label.id, in_trash: true },
    ], Object.values(context.treeState().blocksById));
    await context.enqueueEditorMutation(async () => {
      const result = await persist();
      context.applyPostMutation({ blocks: result.blocks.filter((block) => !block.in_trash), placements: result.placements, removedBlockIds: [removed.label.id] });
      const after = undoSnapshot(tabBlockId);
      reconcileNotesCompoundUndo(before, after, result);
      context.recordUndo({ kind: "delete", before, after });
    });
    const focusBlockId = movedChildIds[0] ?? targetChildIds.at(-1) ?? target.label.id;
    context.requestBlockFocus(focusBlockId);
  }

  async function moveTab(
    tabBlockId: string,
    labelBlockId: string,
    direction: NotesTabMoveDirection,
  ): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    const tabBlock = context.blockById(tabBlockId);
    if (!selectedPageId || !tabBlock || tabBlock.type !== "tab") return;
    const tabs = tabItems(tabBlockId);
    if (!notesTabCanMove(tabs, labelBlockId, direction)) return;
    const labelIndex = tabs.findIndex((tab) => tab.label.id === labelBlockId);
    const target = tabs[direction === "left" ? labelIndex - 1 : labelIndex + 1];
    if (!target) return;
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(tabBlockId);
    const moveRequest = {
      parent: { type: "block_id", block_id: tabBlockId },
      after: direction === "right" ? target.label.id : null,
      before: direction === "left" ? target.label.id : null,
    } as const;
    context.applyPostMutation(notesPostMoveResult(
      await moveNotesBlock(labelBlockId, moveRequest),
      moveRequest,
    ));
    context.requestBlockFocus(labelBlockId);
    recordUndoAfter("move", before, labelBlockId);
  }

  async function moveBlockToTab(blockId: string, labelBlockId: string): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    const block = context.blockById(blockId);
    const label = context.blockById(labelBlockId);
    if (!selectedPageId || !block || !label || label.type !== "paragraph") return;
    if (!tabBlockIdForLabel(labelBlockId)) return;
    if (block.type === "table_row" || block.type === "column") return;
    if (collectLoadedBlockSubtreeIds(context.treeState(), blockId).includes(labelBlockId)) return;
    const originalChildIds = activeChildBlockIds(labelBlockId);
    if (
      block.parent.type === "block_id"
      && block.parent.block_id === labelBlockId
      && originalChildIds.at(-1) === blockId
    ) {
      return;
    }
    const childIds = originalChildIds.filter((childId) => childId !== blockId);
    await context.flushPendingBlockSaves();
    const before = undoSnapshot(blockId);
    const moveRequest = {
      parent: { type: "block_id", block_id: labelBlockId },
      after: childIds.at(-1) ?? null,
      before: null,
    } as const;
    context.applyPostMutation(notesPostMoveResult(
      await moveNotesBlock(blockId, moveRequest),
      moveRequest,
    ));
    context.requestBlockFocus(blockId);
    recordUndoAfter("move", before, blockId);
  }

  return {
    updateTabLabel,
    updateTabIcon,
    addTab,
    removeTab,
    moveTab,
    moveBlockToTab,
  };
}
