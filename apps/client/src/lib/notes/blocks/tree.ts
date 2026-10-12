import {
  blockEditableRichText,
  blockIndent,
  canBlockHaveChildren,
  headingIsToggleable,
  headingToggleOpen,
  isHeadingBlockType,
} from "./factory";
import {
  mergeRichTextForBackspace,
  notesBackspaceCanMergeBlockTypes,
  notesParentCanAcceptBlockType,
} from "./backspace";
import { richTextPlainText } from "$lib/notes/rich-text/core";
import type { NotesBlock, NotesBlockOutline, NotesBlockTreeItem, NotesRichText } from "$lib/notes/types";

export type NotesBlocksById = Readonly<Record<string, NotesBlock>>;
export type NotesChildIdsByParent = Readonly<Record<string, readonly string[]>>;

export interface NotesTreeState {
  blocksById: NotesBlocksById;
  childIdsByParentId: NotesChildIdsByParent;
}

/** Find the bodies needed to change row indentation without skipping unloaded neighbours. */
export function notesIndentationContextIds(
  outlines: readonly NotesBlockOutline[],
  ids: readonly string[],
  direction: "nest" | "outdent",
): string[] {
  const byId = new Map(outlines.map((outline) => [outline.id, outline]));
  const children = new Map<string, NotesBlockOutline[]>();
  for (const outline of outlines) {
    const parentId = outline.parent.type === "page_id" ? outline.parent.page_id : outline.parent.block_id;
    const siblings = children.get(parentId) ?? [];
    siblings.push(outline);
    children.set(parentId, siblings);
  }
  for (const siblings of children.values()) siblings.sort((a, b) => a.sort_order - b.sort_order || a.id.localeCompare(b.id));
  const required = new Set(ids);
  for (const id of ids) {
    const outline = byId.get(id);
    if (!outline) continue;
    const seen = new Set<string>();
    let ancestor = outline;
    while (ancestor.parent.type === "block_id" && !seen.has(ancestor.parent.block_id)) {
      const parentId = ancestor.parent.block_id;
      seen.add(parentId);
      required.add(parentId);
      const parent = byId.get(parentId);
      if (!parent) break;
      ancestor = parent;
    }
    for (const child of children.get(id) ?? []) required.add(child.id);
    if ((outline.ganbaru_indent ?? 0) > 0) continue;
    const parentId = outline.parent.type === "page_id" ? outline.parent.page_id : outline.parent.block_id;
    const siblings = children.get(parentId) ?? [];
    const index = siblings.findIndex((sibling) => sibling.id === id);
    if (direction === "outdent" && outline.parent.type === "block_id") {
      for (const sibling of siblings.slice(index + 1)) required.add(sibling.id);
    } else if (direction === "nest" && index > 0) {
      const previous = siblings[index - 1];
      required.add(previous.id);
      for (const child of children.get(previous.id) ?? []) required.add(child.id);
    }
  }
  return [...required];
}

export interface NotesNestPlan {
  blockId: string;
  parentId: string;
  after: string | null;
}

export interface NotesOutdentPlan {
  blockId: string;
  parentId: string;
  after: string | null;
}

export type NotesMoveDirection = "up" | "down";

export interface NotesMoveWithinSiblingsPlan {
  blockId: string;
  parentId: string;
  after: string | null;
  before: string | null;
}

export type NotesSiblingDropPosition = "before" | "after";

export interface NotesDeletePlan {
  deleteBlockId: string | null;
  focusBlockId: string;
  keepOnlyBlockAsParagraph: boolean;
}

export interface NotesMergePlan {
  sourceBlockId: string;
  targetBlockId: string;
  targetCursorOffset: number;
  mergedRichText: NotesRichText[];
}

export interface NotesChildReparentPlan {
  childIds: string[];
  parentId: string;
  after: string | null;
}

export function parentIdForBlock(block: NotesBlock): string {
  if (block.parent.type === "page_id") return block.parent.page_id;
  if (block.parent.type === "block_id") return block.parent.block_id;
  return "workspace";
}

export function childIdsForParent(
  state: NotesTreeState,
  parentId: string,
): readonly string[] {
  return state.childIdsByParentId[parentId] ?? [];
}

/** Flatten a page block tree in render order while keeping parent metadata. */
export function flattenNotesBlockTree(
  state: NotesTreeState,
  pageId: string,
): NotesBlockTreeItem[] {
  return flattenNotesBlockChildren(state, pageId, 0);
}

export function flattenNotesBlockChildren(
  state: NotesTreeState,
  parentId: string,
  initialDepth = 0,
): NotesBlockTreeItem[] {
  const items: NotesBlockTreeItem[] = [];
  const visit = (parentId: string, depth: number, previousVisibleId: string | null): string | null => {
    let previous = previousVisibleId;
    const childIds = childIdsForParent(state, parentId);
    for (let index = 0; index < childIds.length; index += 1) {
      const id = childIds[index];
      const block = state.blocksById[id];
      if (!block || block.in_trash) continue;
      if (block.type === "table_row" || block.type === "column") continue;
      if (block.type === "paragraph" && parentBlockIsTab(state, parentId)) continue;
      const previousSiblingId = previousVisibleSiblingId(state, parentId, index);
      items.push({
        block,
        depth: depth + blockIndent(block),
        parentId,
        previousSiblingId,
        previousVisibleId: previous,
      });
      previous = id;
      if (blockChildrenAreVisible(block)) {
        previous = visit(id, depth + blockIndent(block) + 1, previous);
      }
    }
    return previous;
  };
  visit(parentId, initialDepth, null);
  return items;
}

/** Decide whether a hydrated block exposes its children in the editor. */
export function blockChildrenAreVisible(block: NotesBlock): boolean {
  if (block.type === "child_page") return false;
  if (block.type === "column_list" || block.type === "column") return false;
  if (block.type === "table" || block.type === "table_row") return false;
  if (block.type === "tab") return false;
  if (block.type === "synced_block") return canBlockHaveChildren(block);
  if (isHeadingBlockType(block.type)) {
    return headingIsToggleable(block) && headingToggleOpen(block);
  }
  return block.type !== "toggle" || block.toggle.ganbaru_open !== false;
}

export function buildNotesChildIdsByParent(blocks: readonly NotesBlock[]): Record<string, string[]> {
  const childIdsByParentId: Record<string, string[]> = {};
  for (const block of blocks) {
    if (block.in_trash) continue;
    const parentId = parentIdForBlock(block);
    childIdsByParentId[parentId] = [...(childIdsByParentId[parentId] ?? []), block.id];
  }
  return childIdsByParentId;
}

export function planNestBlock(
  state: NotesTreeState,
  blockId: string,
): NotesNestPlan | null {
  const block = state.blocksById[blockId];
  if (!block) return null;
  const parentId = parentIdForBlock(block);
  const siblings = childIdsForParent(state, parentId);
  const index = siblings.indexOf(blockId);
  if (index <= 0) return null;
  const previousSiblingId = siblings[index - 1];
  const previousSibling = state.blocksById[previousSiblingId];
  if (previousSibling?.type === "tab") return null;
  if (!previousSibling || !notesParentCanAcceptBlockType(previousSibling, block.type)) return null;
  const children = childIdsForParent(state, previousSiblingId);
  return {
    blockId,
    parentId: previousSiblingId,
    after: children.at(-1) ?? null,
  };
}

export function planOutdentBlock(
  state: NotesTreeState,
  blockId: string,
): NotesOutdentPlan | null {
  const block = state.blocksById[blockId];
  if (!block || block.parent.type !== "block_id") return null;
  const parentBlock = state.blocksById[block.parent.block_id];
  if (!parentBlock) return null;
  if (parentBlock.parent.type === "block_id") {
    const grandparentBlock = state.blocksById[parentBlock.parent.block_id];
    if (grandparentBlock?.type === "tab") return null;
  }
  const targetParentId = parentIdForBlock(parentBlock);
  const targetParentBlock = state.blocksById[targetParentId] ?? null;
  if (!notesParentCanAcceptBlockType(targetParentBlock, block.type)) return null;
  return {
    blockId,
    parentId: targetParentId,
    after: parentBlock.id,
  };
}

function parentBlockIsTab(state: NotesTreeState, parentId: string): boolean {
  return state.blocksById[parentId]?.type === "tab";
}

export function planMoveBlockWithinSiblings(
  state: NotesTreeState,
  blockId: string,
  direction: NotesMoveDirection,
): NotesMoveWithinSiblingsPlan | null {
  const block = state.blocksById[blockId];
  if (!block || block.type === "table_row" || block.type === "column") return null;
  const parentId = parentIdForBlock(block);
  const siblings = childIdsForParent(state, parentId);
  const index = siblings.indexOf(blockId);
  if (index < 0) return null;
  if (direction === "up") {
    if (index === 0) return null;
    return {
      blockId,
      parentId,
      after: null,
      before: siblings[index - 1] ?? null,
    };
  }
  if (index >= siblings.length - 1) return null;
  return {
    blockId,
    parentId,
    after: siblings[index + 1] ?? null,
    before: null,
  };
}

export function planDropBlockWithinSiblings(
  state: NotesTreeState,
  sourceBlockId: string,
  targetBlockId: string,
  position: NotesSiblingDropPosition,
): NotesMoveWithinSiblingsPlan | null {
  if (sourceBlockId === targetBlockId) return null;
  const source = state.blocksById[sourceBlockId];
  const target = state.blocksById[targetBlockId];
  if (!source || !target) return null;
  if (source.type === "table_row" || source.type === "column") return null;
  if (target.type === "table_row" || target.type === "column") return null;
  const sourceParentId = parentIdForBlock(source);
  const targetParentId = parentIdForBlock(target);
  if (sourceParentId !== targetParentId) return null;
  const siblings = childIdsForParent(state, sourceParentId);
  const sourceIndex = siblings.indexOf(sourceBlockId);
  const targetIndex = siblings.indexOf(targetBlockId);
  if (sourceIndex < 0 || targetIndex < 0) return null;
  if (position === "before") {
    if (sourceIndex === targetIndex - 1) return null;
    return {
      blockId: sourceBlockId,
      parentId: sourceParentId,
      after: null,
      before: targetBlockId,
    };
  }
  if (sourceIndex === targetIndex + 1) return null;
  return {
    blockId: sourceBlockId,
    parentId: sourceParentId,
    after: targetBlockId,
    before: null,
  };
}

export function planDeleteBlock(
  flatItems: readonly NotesBlockTreeItem[],
  blockId: string,
): NotesDeletePlan | null {
  const index = flatItems.findIndex((item) => item.block.id === blockId);
  if (index < 0) return null;
  if (flatItems.length === 1) {
    return {
      deleteBlockId: null,
      focusBlockId: blockId,
      keepOnlyBlockAsParagraph: true,
    };
  }
  return {
    deleteBlockId: blockId,
    focusBlockId: (index > 0 ? flatItems[index - 1] : flatItems[1])?.block.id ?? blockId,
    keepOnlyBlockAsParagraph: false,
  };
}

export function planMergeWithPrevious(
  flatItems: readonly NotesBlockTreeItem[],
  blockId: string,
): NotesMergePlan | null {
  const index = flatItems.findIndex((item) => item.block.id === blockId);
  if (index <= 0) return null;
  const source = flatItems[index]?.block;
  const target = flatItems[index - 1]?.block;
  if (!source || !target) return null;
  if (!notesBackspaceCanMergeBlockTypes(source.type, target.type)) return null;
  const targetRichText = blockEditableRichText(target);
  return {
    sourceBlockId: source.id,
    targetBlockId: target.id,
    targetCursorOffset: richTextPlainText(targetRichText).length,
    mergedRichText: mergeRichTextForBackspace(targetRichText, blockEditableRichText(source)),
  };
}

export function planReparentChildrenBeforeDelete(
  state: NotesTreeState,
  blockId: string,
): NotesChildReparentPlan | null {
  const block = state.blocksById[blockId];
  if (!block) return null;
  const childIds = activeChildIdsForParent(state, blockId);
  const parentId = parentIdForBlock(block);
  if (!childrenCanMoveToParent(state, parentId, childIds)) return null;
  return {
    childIds,
    parentId,
    after: childIds.length > 0 ? blockId : null,
  };
}

export function planReparentChildrenAfterMerge(
  state: NotesTreeState,
  sourceBlockId: string,
  targetBlockId: string,
): NotesChildReparentPlan | null {
  const source = state.blocksById[sourceBlockId];
  const target = state.blocksById[targetBlockId];
  if (!source || !target) return null;
  const childIds = activeChildIdsForParent(state, sourceBlockId);
  const parentId = canBlockHaveChildren(target) ? targetBlockId : parentIdForBlock(source);
  if (!childrenCanMoveToParent(state, parentId, childIds)) return null;
  return {
    childIds,
    parentId,
    after: parentId === targetBlockId
      ? activeChildIdsForParent(state, targetBlockId).at(-1) ?? null
      : sourceBlockId,
  };
}

function activeChildIdsForParent(state: NotesTreeState, parentId: string): string[] {
  return childIdsForParent(state, parentId).filter((childId) => {
    const child = state.blocksById[childId];
    return child !== undefined && !child.in_trash;
  });
}

function childrenCanMoveToParent(
  state: NotesTreeState,
  parentId: string,
  childIds: readonly string[],
): boolean {
  const parentBlock = state.blocksById[parentId] ?? null;
  return childIds.every((childId) => {
    const child = state.blocksById[childId];
    return child !== undefined && notesParentCanAcceptBlockType(parentBlock, child.type);
  });
}

function previousVisibleSiblingId(
  state: NotesTreeState,
  parentId: string,
  beforeIndex: number,
): string | null {
  const childIds = childIdsForParent(state, parentId);
  for (let index = beforeIndex - 1; index >= 0; index -= 1) {
    const id = childIds[index];
    const block = state.blocksById[id];
    if (block && !block.in_trash && block.type !== "table_row" && block.type !== "column") return id;
  }
  return null;
}
