import { notesParentCanAcceptBlockType } from "./backspace";
import { childIdsForParent, parentIdForBlock, type NotesTreeState } from "./tree";
import type { NotesBlock } from "$lib/notes/types";

export const NOTES_BLOCK_DRAG_MIME = "application/x-ganbaru-notes-block";

let activeNotesBlockDragId: string | null = null;

export type NotesBlockDropIntent = "before" | "after" | "inside" | "outdent";
export type NotesBlockDropIndicator = NotesBlockDropIntent;

export interface NotesBlockDropPlan {
  blockId: string;
  parentId: string;
  after: string | null;
  before: string | null;
  indicator: NotesBlockDropIndicator;
}

export function setActiveNotesBlockDragId(blockId: string | null): void {
  activeNotesBlockDragId = blockId;
}

export function getActiveNotesBlockDragId(): string | null {
  return activeNotesBlockDragId;
}

export function planNotesBlockDrop(
  state: NotesTreeState,
  sourceBlockId: string,
  targetBlockId: string,
  intent: NotesBlockDropIntent,
): NotesBlockDropPlan | null {
  if (sourceBlockId === targetBlockId) return null;
  const source = state.blocksById[sourceBlockId];
  const target = state.blocksById[targetBlockId];
  if (!source || !target || source.in_trash || target.in_trash) return null;
  if (!notesBlockCanBeDragged(source) || !notesBlockCanBeDroppedNear(target)) return null;
  if (blockSubtreeContains(state, sourceBlockId, targetBlockId)) return null;
  if (intent === "inside") return planInsideDrop(state, source, target);
  if (intent === "outdent") return planOutdentDrop(state, source, target);
  return planSiblingDrop(state, source, target, intent);
}

export function planNotesBlockPageDrop(
  state: NotesTreeState,
  sourceBlockId: string,
  targetPageId: string,
  currentPageId: string | null,
): { blockId: string; pageId: string } | null {
  const source = state.blocksById[sourceBlockId];
  if (!source || source.in_trash || !notesBlockCanBeDragged(source)) return null;
  if (!targetPageId || targetPageId === currentPageId) return null;
  if (source.type === "child_page" && source.id === targetPageId) return null;
  if (blockSubtreeContains(state, sourceBlockId, targetPageId)) return null;
  return { blockId: sourceBlockId, pageId: targetPageId };
}

function planSiblingDrop(
  state: NotesTreeState,
  source: NotesBlock,
  target: NotesBlock,
  intent: "before" | "after",
): NotesBlockDropPlan | null {
  const parentId = parentIdForBlock(target);
  if (!parentCanAcceptSource(state, parentId, source)) return null;
  const siblings = activeChildIdsForParent(state, parentId);
  const sourceIndex = siblings.indexOf(source.id);
  const targetIndex = siblings.indexOf(target.id);
  if (targetIndex < 0) return null;
  if (intent === "before") {
    if (sourceIndex === targetIndex - 1) return null;
    return {
      blockId: source.id,
      parentId,
      after: null,
      before: target.id,
      indicator: "before",
    };
  }
  if (sourceIndex === targetIndex + 1) return null;
  return {
    blockId: source.id,
    parentId,
    after: target.id,
    before: null,
    indicator: "after",
  };
}

function planInsideDrop(
  state: NotesTreeState,
  source: NotesBlock,
  target: NotesBlock,
): NotesBlockDropPlan | null {
  if (!parentCanAcceptSource(state, target.id, source)) return null;
  const childIds = activeChildIdsForParent(state, target.id);
  if (parentIdForBlock(source) === target.id && childIds.at(-1) === source.id) return null;
  return {
    blockId: source.id,
    parentId: target.id,
    after: childIds.at(-1) ?? null,
    before: null,
    indicator: "inside",
  };
}

function planOutdentDrop(
  state: NotesTreeState,
  source: NotesBlock,
  target: NotesBlock,
): NotesBlockDropPlan | null {
  if (target.parent.type !== "block_id") return null;
  const parentBlock = state.blocksById[target.parent.block_id];
  if (!parentBlock || parentBlock.in_trash) return null;
  if (blockSubtreeContains(state, source.id, parentBlock.id)) return null;
  const grandparentId = parentIdForBlock(parentBlock);
  if (!parentCanAcceptSource(state, grandparentId, source)) return null;
  const siblings = activeChildIdsForParent(state, grandparentId);
  const sourceIndex = siblings.indexOf(source.id);
  const parentIndex = siblings.indexOf(parentBlock.id);
  if (sourceIndex === parentIndex + 1) return null;
  return {
    blockId: source.id,
    parentId: grandparentId,
    after: parentBlock.id,
    before: null,
    indicator: "outdent",
  };
}

function parentCanAcceptSource(
  state: NotesTreeState,
  parentId: string,
  source: NotesBlock,
): boolean {
  const parentBlock = state.blocksById[parentId] ?? null;
  return notesParentCanAcceptBlockType(parentBlock, source.type);
}

function activeChildIdsForParent(state: NotesTreeState, parentId: string): string[] {
  return childIdsForParent(state, parentId).filter((blockId) => {
    const block = state.blocksById[blockId];
    return block !== undefined && !block.in_trash;
  });
}

function notesBlockCanBeDragged(block: NotesBlock): boolean {
  return block.type !== "table_row" && block.type !== "column";
}

function notesBlockCanBeDroppedNear(block: NotesBlock): boolean {
  return block.type !== "table_row" && block.type !== "column";
}

function blockSubtreeContains(
  state: NotesTreeState,
  rootBlockId: string,
  searchedBlockId: string,
): boolean {
  const visit = (blockId: string): boolean => {
    if (blockId === searchedBlockId) return true;
    for (const childId of childIdsForParent(state, blockId)) {
      const child = state.blocksById[childId];
      if (child && !child.in_trash && visit(childId)) return true;
    }
    return false;
  };
  return visit(rootBlockId);
}
