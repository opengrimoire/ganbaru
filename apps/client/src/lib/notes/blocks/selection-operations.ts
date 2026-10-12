import { blockPlainText } from "./factory";
import { collectLoadedBlockSubtreeIds } from "./duplicate";
import {
  childIdsForParent,
  parentIdForBlock,
  type NotesTreeState,
} from "./tree";
import type {
  NotesBlock,
  NotesDuplicateBlocksRequest,
  NotesParent,
} from "$lib/notes/types";

export type NotesSelectionMoveDirection = "up" | "down";

export interface NotesSelectionMovePlan {
  blockIds: string[];
  parentId: string;
  after: string | null;
  before: string | null;
  focusBlockId: string;
}

export interface NotesSelectionDuplicateTarget {
  parent: NotesParent;
  after: string | null;
  before?: string | null;
  includeTrashedSources?: boolean;
}

export function notesSelectionRootBlockIds(
  state: NotesTreeState,
  selectedBlockIds: readonly string[],
): string[] {
  const selected = new Set(selectedBlockIds);
  const roots: string[] = [];
  for (const blockId of selectedBlockIds) {
    const block = state.blocksById[blockId];
    if (!block || block.in_trash) continue;
    if (selectedAncestorExists(state, block, selected)) continue;
    if (!roots.includes(blockId)) roots.push(blockId);
  }
  return roots;
}

export function notesSelectionSubtreeIds(
  state: NotesTreeState,
  rootBlockIds: readonly string[],
): string[] {
  const ids: string[] = [];
  for (const rootId of rootBlockIds) {
    for (const blockId of collectLoadedBlockSubtreeIds(state, rootId)) {
      if (!ids.includes(blockId)) ids.push(blockId);
    }
  }
  return ids;
}

export function notesSelectionPlainText(
  state: NotesTreeState,
  rootBlockIds: readonly string[],
): string {
  return notesSelectionSubtreeIds(state, rootBlockIds)
    .map((blockId) => state.blocksById[blockId])
    .filter((block): block is NotesBlock => block !== undefined && !block.in_trash)
    .map(blockPlainText)
    .filter((text) => text.trim().length > 0)
    .join("\n");
}

export function planNotesSelectionMoveWithinSiblings(
  state: NotesTreeState,
  rootBlockIds: readonly string[],
  direction: NotesSelectionMoveDirection,
): NotesSelectionMovePlan | null {
  if (rootBlockIds.length === 0) return null;
  const roots = rootBlockIds
    .map((blockId) => state.blocksById[blockId])
    .filter((block): block is NotesBlock => block !== undefined && !block.in_trash);
  if (roots.length !== rootBlockIds.length) return null;
  const firstRoot = roots[0];
  if (!firstRoot) return null;
  const parentId = parentIdForBlock(firstRoot);
  if (!roots.every((block) => parentIdForBlock(block) === parentId)) return null;
  const rootSet = new Set(rootBlockIds);
  const siblings = childIdsForParent(state, parentId).filter((blockId) => {
    const block = state.blocksById[blockId];
    return block !== undefined && !block.in_trash;
  });
  const selectedSiblings = siblings.filter((blockId) => rootSet.has(blockId));
  if (selectedSiblings.length !== rootBlockIds.length) return null;
  const firstSelectedSibling = selectedSiblings[0];
  const lastSelectedSibling = selectedSiblings[selectedSiblings.length - 1];
  if (!firstSelectedSibling || !lastSelectedSibling) return null;
  const firstIndex = siblings.indexOf(firstSelectedSibling);
  const lastIndex = siblings.indexOf(lastSelectedSibling);
  if (firstIndex < 0 || lastIndex < 0) return null;
  if (direction === "up") {
    if (firstIndex === 0) return null;
    return {
      blockIds: selectedSiblings,
      parentId,
      after: null,
      before: siblings[firstIndex - 1] ?? null,
      focusBlockId: firstSelectedSibling,
    };
  }
  if (lastIndex >= siblings.length - 1) return null;
  return {
    blockIds: selectedSiblings,
    parentId,
    after: siblings[lastIndex + 1] ?? null,
    before: null,
    focusBlockId: firstSelectedSibling,
  };
}

export function createDuplicateBlocksRequest(
  state: NotesTreeState,
  sourceRootBlockIds: readonly string[],
  target: NotesSelectionDuplicateTarget,
  createId: () => string,
): NotesDuplicateBlocksRequest {
  const sourceIds = notesSelectionSubtreeIds(state, sourceRootBlockIds);
  return {
    block_ids: [...sourceRootBlockIds],
    duplicated_block_ids: sourceIds.map((sourceId) => ({
      source_id: sourceId,
      duplicate_id: createId(),
    })),
    parent: target.parent,
    after: target.after,
    before: target.before ?? null,
    include_trashed_sources: target.includeTrashedSources ?? false,
  };
}

function selectedAncestorExists(
  state: NotesTreeState,
  block: NotesBlock,
  selected: ReadonlySet<string>,
): boolean {
  let currentParent = block.parent.type === "block_id" ? block.parent.block_id : null;
  while (currentParent) {
    if (selected.has(currentParent)) return true;
    const parentBlock = state.blocksById[currentParent];
    currentParent = parentBlock?.parent.type === "block_id" ? parentBlock.parent.block_id : null;
  }
  return false;
}
