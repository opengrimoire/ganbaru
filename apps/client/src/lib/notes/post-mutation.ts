import { parentIdForBlock, type NotesTreeState } from "$lib/notes/blocks/tree";
import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";
import type {
  NotesAppendBlockChildrenRequest,
  NotesBlock,
  NotesLoadedPage,
  NotesMoveBlockRequest,
  NotesMoveBlocksRequest,
  NotesPage,
  NotesPaginatedBlockList,
  NotesParent,
} from "./types";

export type NotesSidebarMetadataImpact = "none" | "visible-metadata" | "hierarchy";

export interface NotesBlockPlacement {
  blockId: string;
  parent: NotesParent;
  after: string | null;
  before?: string | null;
}

export interface NotesPostMutationResult {
  /** Project an accepted native operation without creating new local draft markers. */
  canonical?: boolean;
  blocks?: readonly NotesBlock[];
  placements?: readonly NotesBlockPlacement[];
  removedBlockIds?: readonly string[];
  pages?: readonly NotesPage[];
  removedPageIds?: readonly string[];
  loadedPage?: NotesLoadedPage | null;
  sidebarImpact?: NotesSidebarMetadataImpact;
}

/** Converts an append response into ordered local placements. */
export function notesPostAppendResult(
  request: NotesAppendBlockChildrenRequest,
  response: NotesPaginatedBlockList,
): NotesPostMutationResult {
  let after = request.after;
  const placements = response.results.map((block) => {
    const placement = { blockId: block.id, parent: request.parent, after };
    after = block.id;
    return placement;
  });
  return { blocks: response.results, placements };
}

/** Converts one move response into an authoritative local placement. */
export function notesPostMoveResult(
  block: NotesBlock,
  request: NotesMoveBlockRequest,
): NotesPostMutationResult {
  return {
    blocks: [block],
    placements: [{
      blockId: block.id,
      parent: request.parent,
      after: request.after ?? null,
      before: request.before ?? null,
    }],
  };
}

/** Converts a bulk move response into ordered local placements. */
export function notesPostMoveManyResult(
  request: NotesMoveBlocksRequest,
  response: NotesPaginatedBlockList,
): NotesPostMutationResult {
  let after = request.after ?? null;
  const destinationParentId = request.parent.type === "page_id"
    ? request.parent.page_id
    : request.parent.type === "block_id"
      ? request.parent.block_id
      : "workspace";
  const rootBlocks = response.results.filter(
    (block) => parentIdForBlock(block) === destinationParentId,
  );
  const placements = rootBlocks.map((block) => {
    const placement = {
      blockId: block.id,
      parent: request.parent,
      after,
      before: after ? null : request.before ?? null,
    };
    after = block.id;
    return placement;
  });
  return { blocks: response.results, placements };
}

/** Removes the loaded subtree for each trashed root without a page reload. */
export function notesPostTrashResult(
  state: NotesTreeState,
  rootBlockIds: readonly string[],
): NotesPostMutationResult {
  return {
    removedBlockIds: [...new Set(rootBlockIds.flatMap((blockId) => (
      collectLoadedBlockSubtreeIds(state, blockId)
    )))],
  };
}

/** Applies authoritative returned blocks without re-reading the page tree. */
export function applyNotesPostMutationToTree(
  state: NotesTreeState,
  result: NotesPostMutationResult,
): NotesTreeState {
  const nextBlocksById: Record<string, NotesBlock> = { ...state.blocksById };
  const nextChildren: Record<string, string[]> = Object.fromEntries(
    Object.entries(state.childIdsByParentId).map(([parentId, childIds]) => [parentId, [...childIds]]),
  );
  const removed = new Set(result.removedBlockIds ?? []);
  const placements = new Map((result.placements ?? []).map((placement) => [
    placement.blockId,
    placement,
  ]));
  const returnedIds = new Set((result.blocks ?? []).map((block) => block.id));

  for (const blockId of removed) delete nextBlocksById[blockId];
  for (const [parentId, childIds] of Object.entries(nextChildren)) {
    nextChildren[parentId] = childIds.filter((blockId) => (
      !removed.has(blockId) && !placements.has(blockId)
    ));
  }

  for (const block of result.blocks ?? []) {
    const previous = state.blocksById[block.id];
    nextBlocksById[block.id] = block;
    if (placements.has(block.id)) continue;
    const nextParentId = parentIdForBlock(block);
    const previousParentId = previous ? parentIdForBlock(previous) : null;
    if (!previous || previousParentId !== nextParentId) {
      if (previousParentId) {
        nextChildren[previousParentId] = (nextChildren[previousParentId] ?? [])
          .filter((blockId) => blockId !== block.id);
      }
      const siblings = nextChildren[nextParentId] ?? [];
      if (!siblings.includes(block.id) && !block.in_trash) {
        nextChildren[nextParentId] = [...siblings, block.id];
      }
    }
  }

  for (const placement of result.placements ?? []) {
    const block = nextBlocksById[placement.blockId];
    if (!block || block.in_trash || removed.has(block.id)) continue;
    const parentId = placement.parent.type === "page_id"
      ? placement.parent.page_id
      : placement.parent.type === "block_id"
        ? placement.parent.block_id
        : "workspace";
    const siblings = (nextChildren[parentId] ?? []).filter((id) => id !== block.id);
    const beforeIndex = placement.before ? siblings.indexOf(placement.before) : -1;
    const afterIndex = placement.after ? siblings.indexOf(placement.after) : -1;
    const insertIndex = beforeIndex >= 0
      ? beforeIndex
      : afterIndex >= 0
        ? afterIndex + 1
        : siblings.length;
    nextChildren[parentId] = [
      ...siblings.slice(0, insertIndex),
      block.id,
      ...siblings.slice(insertIndex),
    ];
  }

  for (const parentId of Object.keys(nextChildren)) {
    if (removed.has(parentId) && !returnedIds.has(parentId)) delete nextChildren[parentId];
  }
  return { blocksById: nextBlocksById, childIdsByParentId: nextChildren };
}

/** Returns the stronger of two sidebar refresh impacts. */
export function mergeNotesSidebarMetadataImpact(
  current: NotesSidebarMetadataImpact,
  next: NotesSidebarMetadataImpact,
): NotesSidebarMetadataImpact {
  if (current === "hierarchy" || next === "hierarchy") return "hierarchy";
  if (current === "visible-metadata" || next === "visible-metadata") return "visible-metadata";
  return "none";
}
