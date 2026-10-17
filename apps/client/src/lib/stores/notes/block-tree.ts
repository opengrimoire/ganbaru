import {
  flattenNotesBlockChildren,
  flattenNotesBlockTree,
  parentIdForBlock,
  type NotesTreeState,
} from "$lib/notes/blocks/tree";
import type {
  NotesBlock,
  NotesBlockTreeItem,
  NotesBlockType,
  NotesColumnBlockItems,
  NotesParagraphBlock,
  NotesTableRowBlock,
  NotesTabBlockItems,
} from "$lib/notes/types";

export interface NotesBlockTreeSnapshot {
  selectedPageId: string | null;
  blocksById: Record<string, NotesBlock>;
  childIdsByParentId: Record<string, string[]>;
}

export interface NotesMutableBlockTreeState {
  blocksById: Record<string, NotesBlock>;
  childIdsByParentId: Record<string, string[]>;
}

/**
 * Build the tree snapshot consumed by Notes block planning helpers.
 */
export function notesTreeState(snapshot: NotesBlockTreeSnapshot): NotesTreeState {
  return {
    blocksById: snapshot.blocksById,
    childIdsByParentId: snapshot.childIdsByParentId,
  };
}

/**
 * Remove a leaf block from local editor state without rebuilding from persistence.
 */
export function notesTreeStateWithoutLeafBlock(
  state: NotesTreeState,
  blockId: string,
): NotesMutableBlockTreeState | null {
  const block = state.blocksById[blockId];
  if (!block || (state.childIdsByParentId[blockId]?.length ?? 0) > 0) return null;
  const nextBlocksById = { ...state.blocksById };
  delete nextBlocksById[blockId];
  const parentId = parentIdForBlock(block);
  const nextChildIdsByParentId = Object.fromEntries(
    Object.entries(state.childIdsByParentId).map(([currentParentId, childIds]) => [
      currentParentId,
      [...childIds],
    ]),
  );
  nextChildIdsByParentId[parentId] = (state.childIdsByParentId[parentId] ?? []).filter(
    (candidate) => candidate !== blockId,
  );
  delete nextChildIdsByParentId[blockId];
  return {
    blocksById: nextBlocksById,
    childIdsByParentId: nextChildIdsByParentId,
  };
}

/**
 * Return the visible flat block list for the selected page.
 */
export function flatNotesBlockItems(snapshot: NotesBlockTreeSnapshot): NotesBlockTreeItem[] {
  return snapshot.selectedPageId
    ? flattenNotesBlockTree(notesTreeState(snapshot), snapshot.selectedPageId)
    : [];
}

/**
 * Return the nearest ancestor of a specific block type.
 */
export function closestNotesAncestorBlockOfType(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
  type: NotesBlockType,
): NotesBlock | null {
  let parent = snapshot.blocksById[blockId]?.parent;
  while (parent?.type === "block_id") {
    const parentBlock = snapshot.blocksById[parent.block_id];
    if (!parentBlock) return null;
    if (parentBlock.type === type) return parentBlock;
    parent = parentBlock.parent;
  }
  return null;
}

/**
 * Return the tab label paragraph that owns a nested block.
 */
export function tabLabelAncestorForNotesBlock(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
): NotesParagraphBlock | null {
  let parent = snapshot.blocksById[blockId]?.parent;
  while (parent?.type === "block_id") {
    const parentBlock = snapshot.blocksById[parent.block_id];
    if (!parentBlock) return null;
    if (
      parentBlock.type === "paragraph"
      && parentBlock.parent.type === "block_id"
      && snapshot.blocksById[parentBlock.parent.block_id]?.type === "tab"
    ) {
      return parentBlock;
    }
    parent = parentBlock.parent;
  }
  return null;
}

/**
 * Return the flat sibling context used by keyboard operations.
 */
export function flatNotesBlockItemsForContext(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
): NotesBlockTreeItem[] {
  const column = closestNotesAncestorBlockOfType(snapshot, blockId, "column");
  if (column) return flattenNotesBlockChildren(notesTreeState(snapshot), column.id, 0);
  const tabLabel = tabLabelAncestorForNotesBlock(snapshot, blockId);
  if (tabLabel) return flattenNotesBlockChildren(notesTreeState(snapshot), tabLabel.id, 0);
  return flatNotesBlockItems(snapshot);
}

/**
 * Return active table rows for a table block.
 */
export function notesTableRowsForBlock(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
): NotesTableRowBlock[] {
  return (snapshot.childIdsByParentId[blockId] ?? [])
    .map((childId) => snapshot.blocksById[childId])
    .filter((block): block is NotesTableRowBlock => block?.type === "table_row");
}

/**
 * Return editable column items for a column list block.
 */
export function notesColumnItemsForBlock(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
): NotesColumnBlockItems[] {
  return (snapshot.childIdsByParentId[blockId] ?? [])
    .map((childId) => snapshot.blocksById[childId])
    .filter((block): block is NotesColumnBlockItems["column"] => block?.type === "column")
    .map((column) => ({
      column,
      items: flattenNotesBlockChildren(notesTreeState(snapshot), column.id, 0),
    }));
}

/**
 * Return editable tab items for a tab block.
 */
export function notesTabItemsForBlock(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
): NotesTabBlockItems[] {
  return (snapshot.childIdsByParentId[blockId] ?? [])
    .map((childId) => snapshot.blocksById[childId])
    .filter((block): block is NotesParagraphBlock => block?.type === "paragraph")
    .map((label) => ({
      label,
      items: flattenNotesBlockChildren(notesTreeState(snapshot), label.id, 0),
    }));
}

/**
 * Return the previous visible block type for keyboard planning.
 */
export function previousNotesBlockType(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
): NotesBlockType | null {
  const items = flatNotesBlockItemsForContext(snapshot, blockId);
  const item = items.find((candidate) => candidate.block.id === blockId);
  if (!item?.previousVisibleId) return null;
  return snapshot.blocksById[item.previousVisibleId]?.type ?? null;
}

/**
 * Return whether a block is the only editable block in its current context.
 */
export function isOnlyNotesBlockInContext(
  snapshot: NotesBlockTreeSnapshot,
  blockId: string,
): boolean {
  const items = flatNotesBlockItemsForContext(snapshot, blockId);
  return items.length === 1 && items[0]?.block.id === blockId;
}
