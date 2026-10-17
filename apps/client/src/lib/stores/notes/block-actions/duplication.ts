import { notesSelectionRootBlockIds } from "$lib/notes/blocks/selection-operations";
import { planNotesInsertedBlockFocus } from "$lib/notes/editor/focus";
import type { NotesTreeState } from "$lib/notes/blocks/tree";
import type { NotesBlock, NotesParent } from "$lib/notes/types";
import type { NotesUndoSnapshot } from "$lib/notes/history/undo-history";

interface DuplicateSubtreesInput {
  rootBlockIds: readonly string[];
  sourceSubtreeBlockIds: readonly string[];
  parent: NotesParent;
  after: string | null;
  before?: string | null;
  includeTrashedSources?: boolean;
  showDatabasePasteChoices?: boolean;
}

interface NotesBlockDuplicationActionsContext {
  readSelectedPageId: () => string | null;
  blockById: (blockId: string) => NotesBlock | undefined;
  treeState: () => NotesTreeState;
  outlineSubtreeIds: (rootBlockIds: readonly string[]) => string[];
  flushBlockSave: (blockId: string) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
  requestBlockFocus: (blockId: string | null) => void;
  duplicateSubtreesAndApply: (input: DuplicateSubtreesInput) => Promise<NotesBlock[]>;
  undoSnapshot: (focusBlockId: string | null) => NotesUndoSnapshot | null;
  recordUndoAfter: (
    kind: "duplicate" | "paste",
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
  ) => void;
}

export interface NotesBlockDuplicationActions {
  pasteBlockSelection: (
    sourceRootBlockIds: readonly string[],
    sourceSubtreeBlockIds: readonly string[],
    targetBlockId: string,
    includeTrashedSources?: boolean,
  ) => Promise<string | null>;
  duplicateBlock: (blockId: string) => Promise<void>;
  duplicateBlockSelection: (blockIds: readonly string[]) => Promise<string | null>;
}

function parentStorageKey(parent: NotesParent): string {
  if (parent.type === "page_id") return `page:${parent.page_id}`;
  if (parent.type === "block_id") return `block:${parent.block_id}`;
  if (parent.type === "data_source_id") return `data-source:${parent.data_source_id}`;
  return "workspace";
}

/** Create block paste-copy and duplication mutations. */
export function createNotesBlockDuplicationActions(
  context: NotesBlockDuplicationActionsContext,
): NotesBlockDuplicationActions {
  async function pasteBlockSelection(
    sourceRootBlockIds: readonly string[],
    sourceSubtreeBlockIds: readonly string[],
    targetBlockId: string,
    includeTrashedSources = false,
  ): Promise<string | null> {
    const target = context.blockById(targetBlockId);
    if (!context.readSelectedPageId() || !target || sourceRootBlockIds.length === 0) return null;
    if (sourceSubtreeBlockIds.length === 0) return null;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(targetBlockId);
    const duplicates = await context.duplicateSubtreesAndApply({
      rootBlockIds: sourceRootBlockIds,
      sourceSubtreeBlockIds,
      parent: target.parent,
      after: targetBlockId,
      before: null,
      includeTrashedSources,
      showDatabasePasteChoices: !includeTrashedSources,
    });
    const focusBlockId = planNotesInsertedBlockFocus([duplicates[0]?.id], targetBlockId);
    context.requestBlockFocus(focusBlockId);
    context.recordUndoAfter("paste", before, focusBlockId);
    return focusBlockId;
  }

  async function duplicateBlock(blockId: string): Promise<void> {
    if (!context.readSelectedPageId()) return;
    await context.flushBlockSave(blockId);
    const before = context.undoSnapshot(blockId);
    const subtreeIds = context.outlineSubtreeIds([blockId]);
    const block = context.blockById(blockId);
    if (subtreeIds.length === 0 || !block) return;
    const duplicates = await context.duplicateSubtreesAndApply({
      rootBlockIds: [blockId],
      sourceSubtreeBlockIds: subtreeIds,
      parent: block.parent,
      after: blockId,
    });
    const focusBlockId = planNotesInsertedBlockFocus([duplicates[0]?.id], blockId);
    context.requestBlockFocus(focusBlockId);
    context.recordUndoAfter("duplicate", before, focusBlockId);
  }

  async function duplicateBlockSelection(blockIds: readonly string[]): Promise<string | null> {
    if (!context.readSelectedPageId()) return null;
    const rootBlockIds = notesSelectionRootBlockIds(context.treeState(), blockIds);
    if (rootBlockIds.length === 0) return null;
    const first = rootBlockIds[0] ? context.blockById(rootBlockIds[0]) : undefined;
    if (!first) return null;
    const key = parentStorageKey(first.parent);
    if (!rootBlockIds.every((id) => {
      const block = context.blockById(id);
      return block !== undefined && parentStorageKey(block.parent) === key;
    })) return null;
    const lastRootId = rootBlockIds.at(-1);
    if (!lastRootId) return null;
    const subtreeIds = context.outlineSubtreeIds(rootBlockIds);
    if (subtreeIds.length === 0) return null;
    await context.flushPendingBlockSaves();
    const before = context.undoSnapshot(rootBlockIds[0] ?? null);
    const duplicates = await context.duplicateSubtreesAndApply({
      rootBlockIds,
      sourceSubtreeBlockIds: subtreeIds,
      parent: first.parent,
      after: lastRootId,
      before: null,
    });
    const focusBlockId = planNotesInsertedBlockFocus([duplicates[0]?.id], rootBlockIds[0] ?? null);
    context.requestBlockFocus(focusBlockId);
    context.recordUndoAfter("duplicate", before, focusBlockId);
    return focusBlockId;
  }

  return { pasteBlockSelection, duplicateBlock, duplicateBlockSelection };
}
