import { cloneNotesJson } from "$lib/notes/json-clone";
import { createNotesCompoundPersistence } from "$lib/stores/notes/compound-edits";
import type { NotesEditOperation } from "$lib/api/notes/compound-edits";
import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";
import { blockPlainText, canBlockHaveChildren, blockEditableRichText, blockWithRichText, blockIndent, blockUpdateWithIndent, blockUpdateFromBlock, blockWithToggleOpen, blockWithHeadingToggleOpen, headingIsToggleable, headingToggleOpen, createBlockUpdate, isTextEditableBlock } from "$lib/notes/blocks/factory";
import { planNotesBlockDrop, type NotesBlockDropIntent } from "$lib/notes/blocks/drag";
import { planNotesDeletedBlockFocus } from "$lib/notes/editor/focus";
import {
  notesSelectionRootBlockIds,
  notesSelectionSubtreeIds,
  planNotesSelectionMoveWithinSiblings,
  type NotesSelectionMoveDirection,
} from "$lib/notes/blocks/selection-operations";
import {
  planDeleteBlock,
  planMergeWithPrevious,
  planMoveBlockWithinSiblings,
  planNestBlock,
  planOutdentBlock,
  planReparentChildrenAfterMerge,
  planReparentChildrenBeforeDelete,
  type NotesChildReparentPlan,
  type NotesTreeState,
} from "$lib/notes/blocks/tree";
import type { NotesDocumentSelection, NotesTextSelection } from "$lib/notes/editor/selection";
import type {
  NotesBlock,
  NotesBlockTreeItem,
  NotesBlockUpdate,
  NotesMoveBlockRequest,
  NotesMoveBlocksRequest,
  NotesParent,
} from "$lib/notes/types";
import type {
  NotesUndoKind,
  NotesUndoSnapshot,
} from "$lib/notes/history/undo-history";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";

interface NotesBlockMovementActionsContext {
  readPageGeneration?: () => number;
  prepareIndentation?: (ids: readonly string[], direction: "nest" | "outdent") => void | Promise<void>;
  prepareBlockDeletion?: (blockId: string) => void | Promise<void>;
  outlineSubtreeIds?: (rootBlockIds: readonly string[]) => string[];
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  readSelectedPageId: () => string | null;
  ensurePageBody: (pageId: string, operations?: NotesEditOperation[]) => string | null;
  treeState: () => NotesTreeState;
  blockById: (blockId: string) => NotesBlock | undefined;
  flatBlockItemsForBlockContext: (blockId: string) => NotesBlockTreeItem[];
  requestBlockFocus: (blockId: string | null, selection?: NotesTextSelection | null) => void;
  flushBlockSave: (blockId: string) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
  localRemoveLeafBlock: (blockId: string) => boolean;
  loadPageTree: (pageId: string) => Promise<void>;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  createUndoSnapshot: (
    focusBlockId: string | null,
    extraBlocks?: readonly NotesBlock[],
    focusSelection?: NotesTextSelection | null,
  ) => NotesUndoSnapshot | null;
  replaceBlockWithUpdate: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  moveAndApply: (blockId: string, request: NotesMoveBlockRequest) => Promise<NotesBlock>;
  moveManyAndApply: (request: NotesMoveBlocksRequest) => Promise<NotesBlock[]>;
  trashAndApply: (rootBlockIds: readonly string[]) => Promise<void>;
  pendingOptimisticWrite: (blockId: string) => Promise<void> | null;
  trackOptimisticBlockWrites: (blockIds: readonly string[], persistence: Promise<void>) => void;
  undoSnapshot: (focusBlockId: string | null) => NotesUndoSnapshot | null;
  recordUndo: (
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    after: NotesUndoSnapshot | null,
  ) => void;
  recordUndoAfter: (
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
    groupKey?: string | null,
    extraBlocks?: readonly NotesBlock[],
  ) => void;
}

export interface NotesBlockMovementActions {
  deleteBlock: (blockId: string) => Promise<void>;
  deleteBlockSelection: (blockIds: readonly string[]) => Promise<void>;
  mergeBlockWithPrevious: (blockId: string) => Promise<void>;
  indentBlockSelection: (ids: readonly string[], direction: "nest" | "outdent", selection?: NotesDocumentSelection) => Promise<void>;
  nestBlock: (blockId: string, selection?: NotesTextSelection) => Promise<void>;
  outdentBlock: (blockId: string, selection?: NotesTextSelection) => Promise<void>;
  moveBlockUp: (blockId: string) => Promise<void>;
  moveBlockDown: (blockId: string) => Promise<void>;
  moveBlockSelection: (
    blockIds: readonly string[],
    direction: NotesSelectionMoveDirection,
  ) => Promise<void>;
  dropBlockWithinSiblings: (
    sourceBlockId: string,
    targetBlockId: string,
    position: "before" | "after",
  ) => Promise<void>;
  dropBlockOnBlock: (
    sourceBlockId: string,
    targetBlockId: string,
    intent: NotesBlockDropIntent,
  ) => Promise<void>;
  moveBlockToPage: (blockId: string, pageId: string) => Promise<void>;
}

/** Create delete, merge, reparent, drag, and cross-page move actions. */
export function createNotesBlockMovementActions(
  context: NotesBlockMovementActionsContext,
): NotesBlockMovementActions {
  function selectionAtBlockEnd(blockId: string): NotesTextSelection | null {
    const block = context.blockById(blockId);
    if (!block || !isTextEditableBlock(block.type)) return null;
    const end = blockPlainText(block).length;
    return { start: end, end };
  }

  function parentFromMoveParentId(parentId: string): NotesParent | null {
    const pageId = context.readSelectedPageId();
    if (!pageId) return null;
    return parentId === pageId
      ? { type: "page_id", page_id: pageId }
      : { type: "block_id", block_id: parentId };
  }

  function focusAfterDeletingSelection(rootBlockIds: readonly string[]): string | null {
    const firstRoot = rootBlockIds[0];
    if (!firstRoot) return null;
    return planNotesDeletedBlockFocus({
      visibleBlockIds: context.flatBlockItemsForBlockContext(firstRoot).map((item) => item.block.id),
      removedBlockIds: notesSelectionSubtreeIds(context.treeState(), rootBlockIds),
      firstRemovedBlockId: firstRoot,
    });
  }

  async function deleteBlock(blockId: string): Promise<void> {
    const pageId = context.readSelectedPageId();
    const generation = context.readPageGeneration?.();
    if (!pageId) return;
    await context.prepareBlockDeletion?.(blockId);
    if (pageId !== context.readSelectedPageId() || generation !== context.readPageGeneration?.()) return;
    const plan = planDeleteBlock(context.flatBlockItemsForBlockContext(blockId), blockId);
    if (!plan) return;
    const block = context.blockById(blockId);
    if (plan.keepOnlyBlockAsParagraph && block && !isTextEditableBlock(block.type)) {
      // Structural blocks must be trashed, preserving their identity for undo.
      await deleteBlockSelection([blockId]);
      return;
    }
    const before = context.undoSnapshot(blockId);
    if (plan.keepOnlyBlockAsParagraph) {
      const update = createBlockUpdate("paragraph", "");
      context.localApplyBlockUpdate(blockId, update);
      context.requestBlockFocus(blockId, { start: 0, end: 0 });
      context.recordUndoAfter("delete", before, blockId);
      const persist = createNotesCompoundPersistence(context, "delete_selection", [{ type: "update", block_id: blockId, update }]);
      const persistence = context.enqueueEditorMutation(async () => { await persist(); });
      context.trackOptimisticBlockWrites([blockId], persistence);
      return;
    }
    if (plan.deleteBlockId) {
      const childPlan = planReparentChildrenBeforeDelete(context.treeState(), plan.deleteBlockId);
      if (!childPlan) return;
      const deletedBlock = context.blockById(plan.deleteBlockId);
      const optimistic = childPlan.childIds.length === 0
        && deletedBlock !== undefined
        && isTextEditableBlock(deletedBlock.type)
        && blockPlainText(deletedBlock).length === 0;
      if (optimistic) {
        const focusSelection = selectionAtBlockEnd(plan.focusBlockId);
        if (!context.localRemoveLeafBlock(plan.deleteBlockId)) return;
        context.requestBlockFocus(plan.focusBlockId, focusSelection);
        context.recordUndo(
          "delete",
          before,
          context.createUndoSnapshot(plan.focusBlockId, [], focusSelection),
        );
        const deletedId = plan.deleteBlockId;
        const persist = createNotesCompoundPersistence(context, "delete_selection", [{ type: "trash", block_id: deletedId, in_trash: true }], deletedBlock ? [deletedBlock] : []);
        const persistence = context.enqueueEditorMutation(async () => { await persist(); });
        context.trackOptimisticBlockWrites([plan.deleteBlockId], persistence);
        void persistence;
        return;
      }
      const parent = parentFromMoveParentId(childPlan.parentId);
      if (!parent) return;
      const persist = createNotesCompoundPersistence(context, "delete_selection", [
        { type: "move_children", source_block_id: plan.deleteBlockId, parent, after: childPlan.after },
        { type: "trash", block_id: plan.deleteBlockId, in_trash: true },
      ]);
      await context.enqueueEditorMutation(async () => {
        const result = await persist();
        let after = childPlan.after;
        const placements = childPlan.childIds.map((id) => { const placement = { blockId: id, parent, after }; after = id; return placement; });
        context.applyPostMutation({ blocks: result.blocks.filter((block) => !block.in_trash), placements, removedBlockIds: [plan.deleteBlockId!], canonical: true });
      });
    }
    context.requestBlockFocus(plan.focusBlockId);
    context.recordUndoAfter("delete", before, plan.focusBlockId);
  }

  async function deleteBlockSelection(blockIds: readonly string[]): Promise<void> {
    const pageId = context.readSelectedPageId();
    if (!pageId) return;
    const roots = notesSelectionRootBlockIds(context.treeState(), blockIds);
    if (roots.length === 0) return;
    const state = context.treeState();
    const before = context.undoSnapshot(roots[0] ?? null);
    const focus = focusAfterDeletingSelection(roots);
    const removed = [...new Set([
      ...notesSelectionSubtreeIds(context.treeState(), roots),
      ...(context.outlineSubtreeIds?.(roots) ?? []),
    ])];
    context.applyPostMutation({ removedBlockIds: removed });
    const operations: NotesEditOperation[] = roots.map((id) => ({ type: "trash", block_id: id, in_trash: true }));
    const bodyId = context.ensurePageBody(pageId, operations);
    const nextFocus = focus ?? bodyId;
    context.requestBlockFocus(nextFocus, nextFocus === bodyId && !focus ? { start: 0, end: 0 } : null);
    context.recordUndoAfter("delete", before, nextFocus);
    const persist = createNotesCompoundPersistence(context, "delete_selection", operations, Object.values(state.blocksById));
    const persistence = context.enqueueEditorMutation(async () => { await persist(); });
    context.trackOptimisticBlockWrites(removed, persistence);
  }

  async function mergeBlockWithPrevious(blockId: string): Promise<void> {
    if (!context.readSelectedPageId()) return;
    const plan = planMergeWithPrevious(context.flatBlockItemsForBlockContext(blockId), blockId);
    if (!plan) return;
    const target = context.blockById(plan.targetBlockId);
    if (!target) return;
    const childPlan = planReparentChildrenAfterMerge(context.treeState(), blockId, target.id);
    if (!childPlan) return;
    const parent = parentFromMoveParentId(childPlan.parentId);
    if (!parent) return;
    const originalBlocks = Object.values(context.treeState().blocksById);
    const before = context.createUndoSnapshot(blockId, [], { start: 0, end: 0 });
    const update = blockWithRichText(target, plan.mergedRichText);
    const children = childPlan.childIds.map((id) => context.blockById(id))
      .filter((block): block is NotesBlock => block !== undefined)
      .map((block) => ({ ...block, parent }));
    let after = childPlan.after;
    const placements = children.map((block) => {
      const placement = { blockId: block.id, parent, after };
      after = block.id;
      return placement;
    });
    context.localApplyBlockUpdate(target.id, update);
    context.applyPostMutation({ blocks: children, placements, removedBlockIds: [blockId] });
    const selection = { start: plan.targetCursorOffset, end: plan.targetCursorOffset };
    context.requestBlockFocus(target.id, selection);
    const persist = createNotesCompoundPersistence(context, "merge", [
      { type: "update", block_id: target.id, update },
      { type: "move_children", source_block_id: blockId, parent, after: childPlan.after },
      { type: "trash", block_id: blockId, in_trash: true },
    ], originalBlocks);
    const afterSnapshot = context.createUndoSnapshot(target.id, [], selection);
    if (before) before.nativeEditId = persist.operationId;
    if (afterSnapshot) afterSnapshot.nativeEditId = persist.operationId;
    context.recordUndo("delete", before, afterSnapshot);
    const persistence = context.enqueueEditorMutation(async () => { await persist(); });
    context.trackOptimisticBlockWrites([blockId, target.id, ...childPlan.childIds], persistence);
  }

  /** Change one text row's depth while keeping other rows at their existing visual depths. */
  function applyKeyboardIndent(
    blockId: string,
    direction: "nest" | "outdent",
    selection?: NotesTextSelection,
    record = true,
    batch?: NotesEditOperation[],
  ): boolean {
    if (!context.readSelectedPageId()) return false;
    const block = context.blockById(blockId);
    if (!block || !isTextEditableBlock(block.type) || block.type === "code") return false;
    const indent = blockIndent(block);
    const candidate = direction === "nest"
      ? planNestBlock(context.treeState(), blockId)
      : planOutdentBlock(context.treeState(), blockId);
    const candidateParent = candidate ? context.blockById(candidate.parentId) : undefined;
    const plan = indent === 0 && (direction === "outdent" || !candidateParent || blockIndent(candidateParent) === 0)
      ? candidate : null;
    if (!plan && direction === "outdent" && indent === 0) return false;
    const parent = plan ? parentFromMoveParentId(plan.parentId) : block.parent;
    if (!parent) return false;
    const sourceParent = block.parent.type === "block_id" ? context.blockById(block.parent.block_id) : undefined;
    if (direction === "outdent" && plan && sourceParent?.type === "callout"
      && blockPlainText(block).length === 0
      && (context.treeState().childIdsByParentId[blockId] ?? []).length === 0) {
      const caret = selection ?? { start: 0, end: 0 };
      const before = record ? context.createUndoSnapshot(blockId, [], caret) : null;
      const placement = { blockId, parent, after: sourceParent.id, before: null };
      context.applyPostMutation({ blocks: [{ ...block, parent }], placements: [placement] });
      if (record) {
        context.requestBlockFocus(blockId, caret);
        context.recordUndo("move", before, context.createUndoSnapshot(blockId, [], caret));
      }
      const operations: NotesEditOperation[] = [{ type: "move", block_id: blockId, request: placement }];
      if (batch) batch.push(...operations);
      else {
        const persist = createNotesCompoundPersistence(context, "indent_selection", operations);
        const persistence = context.enqueueEditorMutation(async () => { await persist(); });
        context.trackOptimisticBlockWrites([blockId], persistence);
      }
      return true;
    }
    const nextIndent = plan
      ? direction === "outdent" && sourceParent ? blockIndent(sourceParent) : 0
      : indent + (direction === "nest" ? 1 : -1);
    const indentUpdate = blockUpdateWithIndent(blockUpdateFromBlock(cloneNotesJson(block)), nextIndent);
    const caret = selection ?? selectionAtBlockEnd(blockId);
    const before = record ? context.createUndoSnapshot(blockId, [], caret) : null;
    const parentBlock = direction === "nest" && plan ? context.blockById(plan.parentId) : undefined;
    const openUpdate = parentBlock?.type === "toggle" && parentBlock.toggle.ganbaru_open === false
      ? blockWithToggleOpen(parentBlock, true)
      : parentBlock && headingIsToggleable(parentBlock) && !headingToggleOpen(parentBlock)
        ? blockWithHeadingToggleOpen(parentBlock, true)
        : null;
    const request = { parent, after: plan?.after ?? null, before: null };
    // Following nested siblings retain their document order when an earlier item is promoted.
    const state = context.treeState();
    let childAfter = blockId;
    const children = (state.childIdsByParentId[blockId] ?? []).map((id) => {
      const child = context.blockById(id);
      if (!child) throw new Error("Notes indentation requires loaded child blocks");
      const childIndent = direction === "nest" ? nextIndent + blockIndent(child) : blockIndent(child) + 1;
      const update = blockUpdateWithIndent(blockUpdateFromBlock(cloneNotesJson(child)), childIndent);
      // Indenting a row makes its former children peers. Outdenting leaves their depth unchanged.
      const placement = direction === "nest" ? { blockId: id, parent, after: childAfter } : null;
      childAfter = id;
      return { child, update, placement };
    });
    const sourceSiblings = sourceParent ? state.childIdsByParentId[sourceParent.id] ?? [] : [];
    const followingIds = direction === "outdent" && plan
      ? sourceSiblings.slice(sourceSiblings.indexOf(blockId) + 1) : [];
    const followingParent: NotesParent = canBlockHaveChildren(block)
      ? { type: "block_id", block_id: blockId } : parent;
    let followingAfter = canBlockHaveChildren(block)
      ? state.childIdsByParentId[blockId]?.at(-1) ?? null : blockId;
    const following = followingIds.map((id) => {
      const sibling = context.blockById(id);
      if (!sibling) throw new Error("Notes indentation requires loaded following siblings");
      const update = !canBlockHaveChildren(block)
        ? blockUpdateWithIndent(blockUpdateFromBlock(cloneNotesJson(sibling)), blockIndent(sibling) + nextIndent + 1)
        : null;
      const placement = { blockId: id, parent: followingParent, after: followingAfter };
      followingAfter = id;
      return { sibling, update, placement };
    });
    if (parentBlock && openUpdate) context.localApplyBlockUpdate(parentBlock.id, openUpdate);
    for (const entry of children) context.localApplyBlockUpdate(entry.child.id, entry.update);
    for (const entry of following) if (entry.update) context.localApplyBlockUpdate(entry.sibling.id, entry.update);
    context.localApplyBlockUpdate(blockId, indentUpdate);
    const placements = [
      ...(plan ? [{ blockId, ...request }] : []),
      ...children.flatMap((entry) => entry.placement ? [entry.placement] : []),
      ...following.map((entry) => entry.placement),
    ];
    if (placements.length) context.applyPostMutation({
      blocks: placements.map((placement) => ({ ...context.blockById(placement.blockId)!, parent: placement.parent })),
      placements,
    });
    if (record) {
      context.requestBlockFocus(blockId, caret);
      context.recordUndo("move", before, context.createUndoSnapshot(blockId, [], caret));
    }
    const operations: NotesEditOperation[] = [];
    if (parentBlock && openUpdate) operations.push({ type: "update", block_id: parentBlock.id, update: openUpdate });
    operations.push({ type: "update", block_id: blockId, update: indentUpdate });
    if (plan) operations.push({ type: "move", block_id: blockId, request });
    for (const entry of children) {
      operations.push({ type: "update", block_id: entry.child.id, update: entry.update });
      if (entry.placement) operations.push({ type: "move", block_id: entry.child.id, request: { parent, after: entry.placement.after, before: null } });
    }
    for (const entry of following) {
      if (entry.update) operations.push({ type: "update", block_id: entry.sibling.id, update: entry.update });
      operations.push({ type: "move", block_id: entry.sibling.id, request: { parent: entry.placement.parent, after: entry.placement.after, before: null } });
    }
    if (batch) batch.push(...operations);
    else {
      const persist = createNotesCompoundPersistence(context, "indent_selection", operations);
      const persistence = context.enqueueEditorMutation(async () => { await persist(); });
      context.trackOptimisticBlockWrites([blockId, ...children.map(({ child }) => child.id), ...followingIds, ...(parentBlock && openUpdate ? [parentBlock.id] : [])], persistence);
    }
    return true;
  }

  let pendingIndentation: Promise<void> | null = null;

  /** Apply immediately when loaded; serialize commands while required neighbours are loading. */
  function runIndentation(ids: readonly string[], direction: "nest" | "outdent", apply: () => void): Promise<void> {
    const pageId = context.readSelectedPageId();
    const generation = context.readPageGeneration?.();
    const isCurrent = () => pageId !== null && context.readSelectedPageId() === pageId
      && context.readPageGeneration?.() === generation;
    const run = (): void | Promise<void> => {
      if (!isCurrent()) return;
      const ready = context.prepareIndentation?.(ids, direction);
      if (ready) return ready.then(() => { if (isCurrent()) apply(); });
      apply();
    };
    const pending = pendingIndentation ? pendingIndentation.then(run) : run();
    if (!pending) return Promise.resolve();
    pendingIndentation = pending;
    const clear = () => { if (pendingIndentation === pending) pendingIndentation = null; };
    void pending.then(clear, clear);
    return pending;
  }

  /** Change each explicitly selected row once, retaining its text range and one history entry. */
  function applyIndentBlockSelection(ids: readonly string[], direction: "nest" | "outdent", selection?: NotesDocumentSelection): void {
    const selected = [...new Set(ids)];
    if (!selected.length) return;
    const before = context.createUndoSnapshot(selected[0]);
    if (before && selection) before.documentSelection = selection;
    let changed = false;
    const operations: NotesEditOperation[] = [];
    const ordered = direction === "outdent" ? [...selected].reverse() : selected;
    for (const id of ordered) {
      changed = applyKeyboardIndent(id, direction, undefined, false, operations) || changed;
    }
    if (!changed) return;
    const persist = createNotesCompoundPersistence(context, "indent_selection", operations);
    const persistence = context.enqueueEditorMutation(async () => { await persist(); });
    context.trackOptimisticBlockWrites(selected, persistence);
    const after = context.createUndoSnapshot(selected[0]);
    if (after && selection) after.documentSelection = selection;
    context.recordUndo("move", before, after);
  }

  function indentBlockSelection(ids: readonly string[], direction: "nest" | "outdent", selection?: NotesDocumentSelection): Promise<void> {
    return runIndentation(ids, direction, () => applyIndentBlockSelection(ids, direction, selection));
  }

  function nestBlock(blockId: string, selection?: NotesTextSelection): Promise<void> {
    return runIndentation([blockId], "nest", () => { applyKeyboardIndent(blockId, "nest", selection); });
  }

  function outdentBlock(blockId: string, selection?: NotesTextSelection): Promise<void> {
    return runIndentation([blockId], "outdent", () => { applyKeyboardIndent(blockId, "outdent", selection); });
  }

  async function moveBlockWithinSiblings(blockId: string, direction: "up" | "down"): Promise<void> {
    if (!context.readSelectedPageId()) return;
    await context.flushBlockSave(blockId);
    const plan = planMoveBlockWithinSiblings(context.treeState(), blockId, direction);
    if (!plan) return;
    const parent = parentFromMoveParentId(plan.parentId);
    if (!parent) return;
    const before = context.undoSnapshot(blockId);
    await context.moveAndApply(blockId, { parent, after: plan.after, before: plan.before });
    context.requestBlockFocus(blockId);
    context.recordUndoAfter("move", before, blockId);
  }

  async function moveBlockSelection(
    blockIds: readonly string[],
    direction: NotesSelectionMoveDirection,
  ): Promise<void> {
    if (!context.readSelectedPageId()) return;
    const roots = notesSelectionRootBlockIds(context.treeState(), blockIds);
    const plan = planNotesSelectionMoveWithinSiblings(context.treeState(), roots, direction);
    if (!plan) return;
    const parent = parentFromMoveParentId(plan.parentId);
    if (!parent) return;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(plan.focusBlockId);
    await context.moveManyAndApply({
      block_ids: plan.blockIds,
      parent,
      after: plan.after,
      before: plan.before,
    });
    context.requestBlockFocus(plan.focusBlockId);
    context.recordUndoAfter("move", before, plan.focusBlockId);
  }

  async function dropBlockOnBlock(
    sourceBlockId: string,
    targetBlockId: string,
    intent: NotesBlockDropIntent,
  ): Promise<void> {
    if (!context.readSelectedPageId()) return;
    await context.flushPendingBlockSaves();
    const plan = planNotesBlockDrop(context.treeState(), sourceBlockId, targetBlockId, intent);
    if (!plan) return;
    const parent = parentFromMoveParentId(plan.parentId);
    if (!parent) return;
    const before = context.undoSnapshot(sourceBlockId);
    await context.moveAndApply(sourceBlockId, { parent, after: plan.after, before: plan.before });
    context.requestBlockFocus(sourceBlockId);
    context.recordUndoAfter("move", before, sourceBlockId);
  }

  async function moveBlockToPage(blockId: string, pageId: string): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    if (!selectedPageId || pageId === selectedPageId) return;
    const block = context.blockById(blockId);
    if (!block || (block.type === "child_page" && block.id === pageId)) return;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(blockId);
    const state = context.treeState();
    const subtreeIds = collectLoadedBlockSubtreeIds(state, blockId);
    const focus = planNotesDeletedBlockFocus({
      visibleBlockIds: context.flatBlockItemsForBlockContext(blockId).map((item) => item.block.id),
      removedBlockIds: subtreeIds,
      firstRemovedBlockId: blockId,
    });
    const subtree = subtreeIds
      .map((id) => context.blockById(id))
      .filter((candidate): candidate is NotesBlock => candidate !== undefined);
    const moved = await context.moveAndApply(blockId, {
      parent: { type: "page_id", page_id: pageId },
      after: null,
      before: null,
    });
    context.applyPostMutation({ sidebarImpact: "hierarchy" });
    if (context.readSelectedPageId() !== selectedPageId) return;
    const nextFocus = focus ?? context.ensurePageBody(selectedPageId);
    context.requestBlockFocus(nextFocus);
    context.recordUndoAfter(
      "move",
      before,
      nextFocus,
      null,
      subtree.map((item) => item.id === moved.id ? moved : item),
    );
  }

  return {
    deleteBlock,
    deleteBlockSelection,
    mergeBlockWithPrevious,
    indentBlockSelection,
    nestBlock,
    outdentBlock,
    moveBlockUp: (blockId) => moveBlockWithinSiblings(blockId, "up"),
    moveBlockDown: (blockId) => moveBlockWithinSiblings(blockId, "down"),
    moveBlockSelection,
    dropBlockWithinSiblings: (source, target, position) => dropBlockOnBlock(source, target, position),
    dropBlockOnBlock,
    moveBlockToPage,
  };
}
