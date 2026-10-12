import { childIdsForParent, type NotesTreeState } from "./tree";
import type { NotesDuplicateBlockRequest } from "$lib/notes/types";

/** Build a stable source-to-duplicate ID map for a loaded block subtree. */
export function createDuplicateBlockRequest(
  state: NotesTreeState,
  blockId: string,
  createId: () => string,
): NotesDuplicateBlockRequest {
  const sourceIds = collectLoadedBlockSubtreeIds(state, blockId);
  return {
    duplicated_block_ids: sourceIds.map((sourceId) => ({
      source_id: sourceId,
      duplicate_id: createId(),
    })),
  };
}

export function collectLoadedBlockSubtreeIds(
  state: NotesTreeState,
  blockId: string,
): string[] {
  if (!state.blocksById[blockId]) return [];
  const ids: string[] = [];
  const visit = (id: string): void => {
    ids.push(id);
    for (const childId of childIdsForParent(state, id)) {
      if (state.blocksById[childId]) visit(childId);
    }
  };
  visit(blockId);
  return ids;
}
