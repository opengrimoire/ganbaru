import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";
import {
  notesButtonWithIcon,
  notesButtonWithPrimaryInsertPosition,
} from "$lib/notes/block-types/button";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { planNotesInsertedBlockFocus } from "$lib/notes/editor/focus";
import type { NotesTreeState } from "$lib/notes/blocks/tree";
import type {
  NotesAppendBlockChildrenRequest,
  NotesBlock,
  NotesBlockUpdate,
  NotesButtonInsertPosition,
  NotesIcon,
  NotesParent,
} from "$lib/notes/types";
import type { NotesUndoSnapshot } from "$lib/notes/history/undo-history";

interface DuplicateSubtreesInput {
  sourceParentBlockId?: string;
  rootBlockIds: readonly string[];
  sourceSubtreeBlockIds: readonly string[];
  parent: NotesParent;
  after: string | null;
  before?: string | null;
}

interface NotesTemplateBlockActionsContext {
  readSelectedPageId: () => string | null;
  readBlocksById: () => Record<string, NotesBlock>;
  readChildIdsByParentId: () => Record<string, string[]>;
  blockById: (blockId: string) => NotesBlock | undefined;
  treeState: () => NotesTreeState;
  flushPendingBlockSaves: () => Promise<void>;
  requestBlockFocus: (blockId: string | null) => void;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  scheduleBlockSave: (blockId: string, update: NotesBlockUpdate) => void;
  appendAndApply: (request: NotesAppendBlockChildrenRequest) => Promise<NotesBlock[]>;
  duplicateSubtreesAndApply: (input: DuplicateSubtreesInput) => Promise<NotesBlock[]>;
  undoSnapshot: (focusBlockId: string | null) => NotesUndoSnapshot | null;
  recordUndoAfter: (
    kind: "button" | "create" | "template" | "update",
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
    groupKey?: string | null,
  ) => void;
}

export interface NotesTemplateBlockActions {
  addTemplateChild: (blockId: string) => Promise<void>;
  useTemplateBlock: (blockId: string) => Promise<void>;
  addButtonChild: (blockId: string) => Promise<void>;
  updateButtonIcon: (blockId: string, icon: NotesIcon | null) => Promise<void>;
  updateButtonInsertPosition: (
    blockId: string,
    position: NotesButtonInsertPosition,
  ) => Promise<void>;
  useButtonBlock: (blockId: string) => Promise<void>;
}

/** Create template and button block actions over the shared mutation runtime. */
export function createNotesTemplateBlockActions(
  context: NotesTemplateBlockActionsContext,
): NotesTemplateBlockActions {
  const blocksById = context.readBlocksById;
  const childIdsByParentId = context.readChildIdsByParentId;

  function activeChildIdsForBlock(blockId: string): string[] {
    return (childIdsByParentId()[blockId] ?? []).filter((childId) => {
      const child = blocksById()[childId];
      return child !== undefined && !child.in_trash;
    });
  }

  async function insertCanonicalTemplateChildren(
    sourceParentBlockId: string,
    childIds: readonly string[],
    target: { parent: NotesParent; after: string | null; before: string | null },
  ): Promise<string | null> {
    const state = context.treeState();
    const sourceSubtreeIds = [...new Set(childIds.flatMap((id) => collectLoadedBlockSubtreeIds(state, id)))];
    const duplicates = await context.duplicateSubtreesAndApply({
      sourceParentBlockId,
      rootBlockIds: childIds,
      sourceSubtreeBlockIds: sourceSubtreeIds,
      parent: target.parent,
      after: target.after,
      before: target.before,
    });
    return duplicates[0]?.id ?? null;
  }

  async function useTemplateBlock(blockId: string): Promise<void> {
    if (!context.readSelectedPageId()) return;
    const template = context.blockById(blockId);
    if (!template || template.type !== "template") return;
    const childIds = activeChildIdsForBlock(blockId);
    if (childIds.length === 0 && !template.has_children) return;
    const state = context.treeState();
    const containsChildPage = childIds.some((childId) =>
      collectLoadedBlockSubtreeIds(state, childId).some(
        (subtreeId) => blocksById()[subtreeId]?.type === "child_page",
      )
    );
    if (containsChildPage) return;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(blockId);
    const firstDuplicateId = await insertCanonicalTemplateChildren(blockId, childIds, {
      parent: template.parent,
      after: blockId,
      before: null,
    });
    const focusBlockId = planNotesInsertedBlockFocus([firstDuplicateId], blockId);
    context.requestBlockFocus(focusBlockId);
    context.recordUndoAfter("template", before, focusBlockId);
  }

  async function addChild(blockId: string, type: "template" | "button"): Promise<void> {
    if (!context.readSelectedPageId()) return;
    const owner = context.blockById(blockId);
    if (!owner || owner.type !== type) return;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(blockId);
    const childIds = activeChildIdsForBlock(blockId);
    const newBlockId = crypto.randomUUID();
    await context.appendAndApply({
      parent: { type: "block_id", block_id: blockId },
      after: childIds.at(-1) ?? null,
      children: [createBlockWrite(newBlockId, "paragraph", "")],
    });
    context.requestBlockFocus(newBlockId);
    context.recordUndoAfter("create", before, newBlockId);
  }

  function firstActiveSiblingId(parentId: string): string | null {
    return (childIdsByParentId()[parentId] ?? []).find((childId) => {
      const child = blocksById()[childId];
      return child !== undefined && !child.in_trash;
    }) ?? null;
  }

  function buttonInsertTarget(
    blockId: string,
    position: NotesButtonInsertPosition,
  ): { parent: NotesParent; after: string | null; before: string | null } | null {
    const button = context.blockById(blockId);
    const pageId = context.readSelectedPageId();
    if (!pageId || !button || button.type !== "button") return null;
    if (position === "below_button") return { parent: button.parent, after: blockId, before: null };
    if (position === "above_button") return { parent: button.parent, after: null, before: blockId };
    const parent = { type: "page_id", page_id: pageId } as const satisfies NotesParent;
    return position === "bottom_of_page"
      ? { parent, after: null, before: null }
      : { parent, after: null, before: firstActiveSiblingId(pageId) };
  }

  async function useButtonBlock(blockId: string): Promise<void> {
    if (!context.readSelectedPageId()) return;
    const button = context.blockById(blockId);
    if (!button || button.type !== "button") return;
    const action = button.button.actions.find((candidate) => candidate.type === "insert_blocks");
    if (!action) return;
    const childIds = activeChildIdsForBlock(blockId);
    if (childIds.length === 0 && !button.has_children) return;
    const state = context.treeState();
    if (childIds.some((childId) =>
      collectLoadedBlockSubtreeIds(state, childId).some(
        (subtreeId) => blocksById()[subtreeId]?.type === "child_page",
      )
    )) return;
    const target = buttonInsertTarget(blockId, action.position);
    if (!target) return;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(blockId);
    const firstDuplicateId = await insertCanonicalTemplateChildren(blockId, childIds, target);
    const focusBlockId = planNotesInsertedBlockFocus([firstDuplicateId], blockId);
    context.requestBlockFocus(focusBlockId);
    context.recordUndoAfter("button", before, focusBlockId);
  }

  async function updateButtonIcon(blockId: string, icon: NotesIcon | null): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "button") return;
    const before = context.undoSnapshot(blockId);
    const update = { type: "button" as const, button: notesButtonWithIcon(block.button, icon) };
    context.localApplyBlockUpdate(blockId, update);
    context.scheduleBlockSave(blockId, update);
    context.recordUndoAfter("update", before, blockId, `update:${blockId}`);
  }

  async function updateButtonInsertPosition(
    blockId: string,
    position: NotesButtonInsertPosition,
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "button") return;
    const before = context.undoSnapshot(blockId);
    const update = {
      type: "button" as const,
      button: notesButtonWithPrimaryInsertPosition(block.button, position),
    };
    context.localApplyBlockUpdate(blockId, update);
    context.scheduleBlockSave(blockId, update);
    context.recordUndoAfter("update", before, blockId, `update:${blockId}`);
  }

  return {
    addTemplateChild: (blockId) => addChild(blockId, "template"),
    useTemplateBlock,
    addButtonChild: (blockId) => addChild(blockId, "button"),
    updateButtonIcon,
    updateButtonInsertPosition,
    useButtonBlock,
  };
}
