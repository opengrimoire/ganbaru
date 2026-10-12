import { collectLoadedBlockSubtreeIds } from "$lib/notes/blocks/duplicate";
import { childIdsForParent, type NotesTreeState } from "$lib/notes/blocks/tree";

export type NotesTemplateUseUnavailableReason = "empty" | "child_page" | null;

export interface NotesTemplateBlockStatus {
  childCount: number;
  containsChildPage: boolean;
  canUse: boolean;
  useUnavailableReason: NotesTemplateUseUnavailableReason;
}

export const EMPTY_NOTES_TEMPLATE_BLOCK_STATUS: NotesTemplateBlockStatus = {
  childCount: 0,
  containsChildPage: false,
  canUse: false,
  useUnavailableReason: "empty",
};

/** Summarize whether a loaded template block can instantiate its reusable child content. */
export function notesTemplateBlockStatus(
  state: NotesTreeState,
  blockId: string,
): NotesTemplateBlockStatus {
  const childIds = activeTemplateChildIds(state, blockId);
  const containsChildPage = childIds.some((childId) =>
    collectLoadedBlockSubtreeIds(state, childId).some(
      (subtreeId) => state.blocksById[subtreeId]?.type === "child_page",
    ),
  );
  const useUnavailableReason: NotesTemplateUseUnavailableReason = childIds.length === 0
    ? "empty"
    : containsChildPage
      ? "child_page"
      : null;
  return {
    childCount: childIds.length,
    containsChildPage,
    canUse: useUnavailableReason === null,
    useUnavailableReason,
  };
}

export function activeTemplateChildIds(state: NotesTreeState, blockId: string): string[] {
  return childIdsForParent(state, blockId).filter((childId) => {
    const child = state.blocksById[childId];
    return child !== undefined && !child.in_trash;
  });
}
