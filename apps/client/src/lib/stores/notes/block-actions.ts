import { blockWithRichText, createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesRichTextAnnotationName } from "$lib/notes/rich-text/core";
import { createNotesDocumentEdit, createNotesDocumentFormatting, createNotesDocumentLinks } from "./document-edit";
import {
  collectLoadedBlockSubtreeIds,
} from "$lib/notes/blocks/duplicate";
import { cloneNotesJson } from "$lib/notes/json-clone";
import type { NotesDocumentSelection, NotesTextSelection } from "$lib/notes/editor/selection";
import type { NotesTreeState } from "$lib/notes/blocks/tree";
import {
  type NotesUndoKind,
  type NotesUndoRecordOptions,
  type NotesUndoSnapshot,
} from "$lib/notes/history/undo-history";
import type {
  NotesBlock,
  NotesAppendBlockChildrenRequest,
  NotesBlockTreeItem,
  NotesBlockUpdate,
  NotesBlockWrite,
  NotesMoveBlockRequest,
  NotesMoveBlocksRequest,
  NotesColumnBlockItems,
  NotesParent,
  NotesTabBlockItems,
  NotesTableRowBlock,
  NotesChildDatabaseBlock,
} from "$lib/notes/types";
import {
  createNotesColumnActions,
  type NotesColumnActions,
} from "./column-actions";
import {
  createNotesTabActions,
  type NotesTabActions,
} from "./tab-actions";
import {
  createNotesMediaBlockActions,
  invalidateReplacedNotesMediaAsset,
  type NotesMediaBlockActions,
} from "$lib/stores/notes/block-actions/media";
import {
  createNotesRichTextBlockActions,
  type NotesRichTextBlockActions,
} from "$lib/stores/notes/block-actions/rich-text";
import {
  createNotesTableBlockActions,
  type NotesTableBlockActions,
} from "$lib/stores/notes/block-actions/table";
import {
  createNotesTemplateBlockActions,
  type NotesTemplateBlockActions,
} from "$lib/stores/notes/block-actions/template";
import {
  createNotesBlockDuplicationActions,
  type NotesBlockDuplicationActions,
} from "$lib/stores/notes/block-actions/duplication";
import {
  createNotesBlockMovementActions,
  type NotesBlockMovementActions,
} from "$lib/stores/notes/block-actions/movement";
import {
  createNotesBlockPasteActions,
  type NotesBlockPasteActions,
} from "$lib/stores/notes/block-actions/paste";
import {
  createNotesStructuralBlockActions,
  type NotesStructuralBlockActions,
} from "$lib/stores/notes/block-actions/structural";
import {
  notesPostMoveManyResult,
  notesPostMoveResult,
  notesPostTrashResult,
  type NotesPostMutationResult,
} from "$lib/notes/post-mutation";
import { createNotesOptimisticWriteTracker } from "./optimistic-writes";
import { createNotesDatabasePasteController, type NotesDatabasePasteController } from "./database-paste.svelte";
import type { NotesCompoundEditResult, NotesEditOperation } from "$lib/api/notes/compound-edits";
import { createNotesCompoundPersistence } from "./compound-edits";

export interface NotesBlockReadCapabilities {
  readPageGeneration?: () => number;
  /** Omitted by fully hydrated callers; partial page projections load affected neighbours first. */
  prepareIndentation?: (ids: readonly string[], direction: "nest" | "outdent") => void | Promise<void>;
  /** Hydrate descendants before a keyboard delete can reparent them. */
  prepareBlockDeletion?: (blockId: string) => void | Promise<void>;
  readSelectedPageId: () => string | null;
  /** Includes unloaded root outlines, so partial hydration never looks like an empty page. */
  readPageRootBlockIds: () => readonly string[];
  readBlocksById: () => Record<string, NotesBlock>;
  readChildIdsByParentId: () => Record<string, string[]>;
  treeState: () => NotesTreeState;
  outlineSubtreeIds: (rootBlockIds: readonly string[]) => string[];
  blockById: (blockId: string) => NotesBlock | undefined;
  flatBlockItemsForBlockContext: (blockId: string) => NotesBlockTreeItem[];
  tableRowsForBlock: (blockId: string) => NotesTableRowBlock[];
  columnItemsForBlock: (blockId: string) => NotesColumnBlockItems[];
  tabItemsForBlock: (blockId: string) => NotesTabBlockItems[];
}

export interface NotesBlockNavigationCapabilities {
  requestBlockFocus: (blockId: string | null, selection?: NotesTextSelection | null) => void;
  createChildPageFromBlock: (blockId: string, clearText?: boolean) => Promise<void>;
  createChildPageAfterBlock: (blockId: string) => Promise<void>;
  loadPageTree: (pageId: string) => Promise<void>;
  refreshOpenLinks: () => Promise<void>;
}

export interface NotesBlockLocalMutationCapabilities {
  applyPostMutation: (result: NotesPostMutationResult) => void;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  localInsertBlockAfter: (block: NotesBlock, afterBlockId: string | null) => void;
  localInsertBlockBefore: (block: NotesBlock, beforeBlockId: string) => void;
  localRemoveLeafBlock: (blockId: string) => boolean;
}

export interface NotesBlockPersistenceCapabilities {
  readCanonicalRevision?: (id: string) => string | undefined;
  retryEditorMutations?: () => Promise<void>;
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  awaitSelectedPageReady: () => Promise<void>;
  saveBlockNow: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  scheduleBlockSave: (blockId: string, update: NotesBlockUpdate) => void;
  flushBlockSave: (blockId: string) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
}

export interface NotesBlockUndoCapabilities {
  reconcileCompoundUndo?: (result: NotesCompoundEditResult) => void;
  reconcileCanonicalBlocks?: (blocks: readonly NotesBlock[]) => void;
  reconcileDatabaseIdentity?: (block: NotesChildDatabaseBlock) => void;
  createUndoSnapshot: (
    focusBlockId: string | null,
    extraBlocks?: readonly NotesBlock[],
    focusSelection?: NotesTextSelection | null,
  ) => NotesUndoSnapshot | null;
  createUndoSnapshotForBlocks: (
    focusBlockId: string | null,
    blockIds: readonly string[],
    extraBlocks?: readonly NotesBlock[],
    focusSelection?: NotesTextSelection | null,
  ) => NotesUndoSnapshot | null;
  recordUndo: (options: Omit<NotesUndoRecordOptions, "id">) => void;
}

export interface NotesBlockActionsContext
  extends NotesBlockReadCapabilities,
    NotesBlockNavigationCapabilities,
    NotesBlockLocalMutationCapabilities,
    NotesBlockPersistenceCapabilities,
    NotesBlockUndoCapabilities {}

export interface NotesBlockActions
  extends NotesColumnActions,
    NotesTabActions,
    NotesMediaBlockActions,
    NotesRichTextBlockActions,
    NotesTableBlockActions,
    NotesTemplateBlockActions,
    NotesBlockDuplicationActions,
    NotesBlockMovementActions,
    NotesBlockPasteActions,
    NotesStructuralBlockActions {
  formatDocumentRange: (ids: readonly string[], start: number, end: number, annotation: NotesRichTextAnnotationName, documentSelection?: NotesDocumentSelection) => Promise<void>;
  linkDocumentRange: (ids: readonly string[], start: number, end: number, url: string, documentSelection?: NotesDocumentSelection) => Promise<void>;
  replaceDocumentRange: (ids: readonly string[], start: number, end: number, text: string, html?: string, documentSelection?: NotesDocumentSelection) => Promise<void>;
  flushOptimisticBlockWrites: () => Promise<void>;
  ensurePageBody: (pageId: string) => string | null;
  databasePaste: NotesDatabasePasteController;
}

/**
 * Create Notes block mutation and UI action methods.
 */
export function createNotesBlockActions(context: NotesBlockActionsContext): NotesBlockActions {
  const optimisticWrites = createNotesOptimisticWriteTracker();
  const trackOptimisticBlockWrites = optimisticWrites.track;
  const enqueueEditorMutation = (mutation: () => Promise<void>) => context.enqueueEditorMutation(async () => {
    await context.awaitSelectedPageReady();
    await mutation();
  });

  async function flushOptimisticBlockWrites(): Promise<void> {
    await optimisticWrites.flush();
  }

  function optimisticBlockFromWrite(write: NotesBlockWrite, parent: NotesParent): NotesBlock {
    const now = new Date().toISOString();
    const plainWrite = cloneNotesJson(write);
    const plainParent = cloneNotesJson(parent);
    return {
      object: "block",
      parent: plainParent,
      created_time: now,
      last_edited_time: now,
      has_children: false,
      in_trash: false,
      source_provider: null,
      source_object_id: null,
      source_last_edited_time: null,
      ...plainWrite,
    } as NotesBlock;
  }

  /** Keep an empty page writable immediately, without adding a separate undo step. */
  function ensurePageBody(pageId: string, operations?: NotesEditOperation[]): string | null {
    if (context.readSelectedPageId() !== pageId) return null;
    const existing = context.readPageRootBlockIds()[0];
    if (existing) return existing;
    const write = createBlockWrite(crypto.randomUUID(), "paragraph", "");
    const parent: NotesParent = { type: "page_id", page_id: pageId };
    context.localInsertBlockAfter(optimisticBlockFromWrite(write, parent), null);
    if (operations) {
      operations.push({ type: "append", request: { parent, after: null, children: [write] } });
      return write.id;
    }
    const persist = createNotesCompoundPersistence(context, "convert", [{ type: "append", request: { parent, after: null, children: [write] } }]);
    const persistence = enqueueEditorMutation(async () => { await persist(); });
    trackOptimisticBlockWrites([write.id], persistence);
    return write.id;
  }

  function undoSnapshot(
    focusBlockId: string | null,
    extraBlocks: readonly NotesBlock[] = [],
  ): NotesUndoSnapshot | null {
    return context.createUndoSnapshot(focusBlockId, extraBlocks);
  }

  function undoSnapshotForBlocks(
    blockIds: readonly string[],
    focusBlockId: string | null,
    focusSelection: NotesTextSelection | null = null,
    extraBlocks: readonly NotesBlock[] = [],
  ): NotesUndoSnapshot | null {
    return context.createUndoSnapshotForBlocks(
      focusBlockId,
      blockIds,
      extraBlocks,
      focusSelection,
    );
  }

  function recordUndo(
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    after: NotesUndoSnapshot | null,
    groupKey: string | null = null,
  ): void {
    context.recordUndo({ kind, before, after, groupKey });
  }

  function recordUndoAfter(
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
    groupKey: string | null = null,
    extraBlocks: readonly NotesBlock[] = [],
  ): void {
    recordUndo(kind, before, undoSnapshot(focusBlockId, extraBlocks), groupKey);
  }

  const columnActions = createNotesColumnActions(context);
  const tabActions = createNotesTabActions(context);
  const mediaActions = createNotesMediaBlockActions(context);
  const richTextActions = createNotesRichTextBlockActions({
    ...context,
    enqueueEditorMutation,
    hasPendingOptimisticWrite: optimisticWrites.has,
  });
  const databasePaste = createNotesDatabasePasteController({
    readPageId: context.readSelectedPageId,
    blockById: context.blockById,
    enqueue: enqueueEditorMutation,
    retry: () => context.retryEditorMutations?.() ?? Promise.resolve(),
    apply: context.applyPostMutation,
    updateLocal: (id, richText) => {
      const block = context.blockById(id);
      if (block) context.localApplyBlockUpdate(id, blockWithRichText(block, richText));
    },
    optimisticBlock: optimisticBlockFromWrite,
    requestFocus: (id) => context.requestBlockFocus(id),
    updateRichText: richTextActions.updateBlockRichText,
    snapshot: (focusId) => context.createUndoSnapshot(focusId),
    recordUndo: (before, after) => context.recordUndo({ kind: "paste", before, after }),
    reconcileIdentity: context.reconcileDatabaseIdentity,
    reconcileCanonicalBlocks: context.reconcileCanonicalBlocks,
  });
  const tableActions = createNotesTableBlockActions({
    ...context,
    appendAndApply,
    trashAndApply,
    replaceBlockWithUpdate,
    undoSnapshot,
    recordUndoAfter,
  });
  const templateActions = createNotesTemplateBlockActions({
    ...context,
    appendAndApply,
    duplicateSubtreesAndApply,
    undoSnapshot,
    recordUndoAfter,
  });
  const duplicationActions = createNotesBlockDuplicationActions({
    ...context,
    duplicateSubtreesAndApply,
    undoSnapshot,
    recordUndoAfter,
  });
  const movementActions = createNotesBlockMovementActions({
    ...context,
    ensurePageBody,
    enqueueEditorMutation,
    replaceBlockWithUpdate,
    moveAndApply,
    moveManyAndApply,
    trashAndApply,
    pendingOptimisticWrite: optimisticWrites.pending,
    trackOptimisticBlockWrites,
    undoSnapshot,
    recordUndo,
    recordUndoAfter,
  });
  const pasteActions = createNotesBlockPasteActions({
    ...context,
    enqueueEditorMutation,
    appendAndApply,
    pendingOptimisticWrite: optimisticWrites.pending,
    trackOptimisticBlockWrites,
    optimisticBlockFromWrite,
    undoSnapshotForBlocks,
    recordUndo,
    databasePaste,
  });
  const structuralActions = createNotesStructuralBlockActions({
    ...context,
    enqueueEditorMutation,
    replaceBlockWithUpdate,
    appendAndApply,
    undoSnapshot,
    recordUndoAfter,
    optimisticBlockFromWrite,
  });

  async function replaceBlockWithUpdate(blockId: string, update: NotesBlockUpdate): Promise<void> {
    const previous = context.blockById(blockId);
    await context.flushBlockSave(blockId);
    context.localApplyBlockUpdate(blockId, update);
    await context.saveBlockNow(blockId, update);
    invalidateReplacedNotesMediaAsset(previous, update);
  }

  async function appendAndApply(request: NotesAppendBlockChildrenRequest): Promise<NotesBlock[]> {
    await context.awaitSelectedPageReady();
    const persist = createNotesCompoundPersistence(context, "convert", [{ type: "append", request }]);
    let appended: NotesBlock[] = [];
    await enqueueEditorMutation(async () => {
      const result = await persist();
      const ids = new Set(request.children.map((child) => child.id));
      appended = result.blocks.filter((block) => ids.has(block.id));
      context.applyPostMutation({ blocks: appended, placements: result.placements, canonical: true });
    });
    return appended;
  }

  async function moveAndApply(
    blockId: string,
    request: NotesMoveBlockRequest,
  ): Promise<NotesBlock> {
    await context.awaitSelectedPageReady();
    const hierarchyChanged = context.blockById(blockId)?.type === "child_page";
    const pageId = context.readSelectedPageId();
    if (!pageId) throw new Error("Notes move requires an active page");
    const operation: NotesEditOperation = request.parent.type === "page_id" && request.parent.page_id !== pageId
      ? { type: "move_between_pages", block_id: blockId, source_page_id: pageId, destination_page_id: request.parent.page_id, request }
      : { type: "move", block_id: blockId, request };
    const operations: NotesEditOperation[] = [operation];
    const rootIds = context.readPageRootBlockIds();
    const replacement = operation.type === "move_between_pages" && rootIds.length === 1 && rootIds[0] === blockId
      ? createBlockWrite(crypto.randomUUID(), "paragraph", "") : null;
    if (replacement) operations.push({ type: "append", request: { parent: { type: "page_id", page_id: pageId }, after: null, children: [replacement] } });
    const persist = createNotesCompoundPersistence(context, "indent_selection", operations);
    let moved: NotesBlock | undefined;
    await enqueueEditorMutation(async () => {
      const result = await persist();
      moved = result.blocks.find((block) => block.id === blockId);
      if (replacement) context.applyPostMutation({ blocks: result.blocks.filter((block) => block.id === replacement.id), placements: result.placements.filter((placement) => placement.blockId === replacement.id), canonical: true });
    });
    if (!moved) throw new Error("Notes move returned no canonical block");
    const block = moved;
    context.applyPostMutation({
      ...notesPostMoveResult(block, request),
      canonical: true,
      sidebarImpact: hierarchyChanged ? "hierarchy" : "none",
    });
    return block;
  }

  async function moveManyAndApply(request: NotesMoveBlocksRequest): Promise<NotesBlock[]> {
    await context.awaitSelectedPageReady();
    const hierarchyChanged = request.block_ids.some(
      (blockId) => context.blockById(blockId)?.type === "child_page",
    );
    const operations: NotesEditOperation[] = [];
    let after = request.after ?? null;
    for (const id of request.block_ids) {
      operations.push({ type: "move", block_id: id, request: { parent: request.parent, after, before: after ? null : request.before ?? null } });
      after = id;
    }
    const persist = createNotesCompoundPersistence(context, "indent_selection", operations);
    let results: NotesBlock[] = [];
    await enqueueEditorMutation(async () => {
      const result = await persist();
      const ids = new Set(request.block_ids);
      results = result.blocks.filter((block) => ids.has(block.id));
    });
    const response = { object: "list" as const, type: "block" as const, block: {}, results, next_cursor: null, has_more: false };
    context.applyPostMutation({
      ...notesPostMoveManyResult(request, response),
      canonical: true,
      sidebarImpact: hierarchyChanged ? "hierarchy" : "none",
    });
    return response.results;
  }

  async function trashAndApply(rootBlockIds: readonly string[]): Promise<void> {
    await context.awaitSelectedPageReady();
    const before = context.treeState();
    const hierarchyChanged = rootBlockIds.some((blockId) => (
      collectLoadedBlockSubtreeIds(before, blockId).some(
        (subtreeId) => before.blocksById[subtreeId]?.type === "child_page",
      )
    ));
    if (!rootBlockIds.length) return;
    const persist = createNotesCompoundPersistence(context, "delete_selection", rootBlockIds.map((id) => ({ type: "trash", block_id: id, in_trash: true })));
    await enqueueEditorMutation(async () => { await persist(); });
    context.applyPostMutation({
      ...notesPostTrashResult(before, rootBlockIds),
      canonical: true,
      sidebarImpact: hierarchyChanged ? "hierarchy" : "none",
    });
  }

  async function duplicateSubtreesAndApply(input: {
    sourceParentBlockId?: string;
    rootBlockIds: readonly string[];
    sourceSubtreeBlockIds: readonly string[];
    parent: NotesParent;
    after: string | null;
    before?: string | null;
    includeTrashedSources?: boolean;
    showDatabasePasteChoices?: boolean;
  }): Promise<NotesBlock[]> {
    await context.awaitSelectedPageReady();
    const request = {
      block_ids: [...input.rootBlockIds],
      duplicated_block_ids: input.sourceSubtreeBlockIds.map((sourceId) => ({
        source_id: sourceId,
        duplicate_id: crypto.randomUUID(),
      })),
      parent: input.parent,
      after: input.after,
      before: input.before ?? null,
      include_trashed_sources: input.includeTrashedSources ?? false,
    };
    const existing = new Set(Object.keys(context.readBlocksById()));
    const operation: NotesEditOperation = input.sourceParentBlockId
      ? { type: "duplicate_children", source_block_id: input.sourceParentBlockId, request }
      : { type: "duplicate", request };
    const persist = createNotesCompoundPersistence(context, input.sourceParentBlockId ? "template" : "paste", [operation]);
    let results: NotesBlock[] = [];
    await enqueueEditorMutation(async () => {
      const copied = (await persist()).blocks.filter((block) => !existing.has(block.id));
      const isRoot = (block: NotesBlock) => block.parent.type === input.parent.type
        && (block.parent.type === "page_id" && input.parent.type === "page_id" ? block.parent.page_id === input.parent.page_id
          : block.parent.type === "block_id" && input.parent.type === "block_id" && block.parent.block_id === input.parent.block_id);
      results = [...copied.filter(isRoot), ...copied.filter((block) => !isRoot(block))];
    });
    const response = { object: "list" as const, type: "block" as const, block: {}, results, next_cursor: null, has_more: false };
    context.applyPostMutation({
      ...notesPostMoveManyResult(request, response),
      canonical: true,
      sidebarImpact: response.results.some((block) => block.type === "child_page") ? "hierarchy" : "none",
    });
    if (input.showDatabasePasteChoices) {
      const sources = new Map<string, string>(request.duplicated_block_ids.map((pair) => [pair.duplicate_id, pair.source_id]));
      const copies = Object.fromEntries(response.results.flatMap((block) => {
        const sourceId = sources.get(block.id);
        return block.type === "child_database" && block.child_database.database_id && sourceId ? [[block.id, sourceId]] : [];
      }));
      databasePaste.beginCopies(copies, false);
    }
    return response.results;
  }

  return {
    formatDocumentRange: createNotesDocumentFormatting(context),
    linkDocumentRange: createNotesDocumentLinks(context),
    replaceDocumentRange: createNotesDocumentEdit(context, optimisticBlockFromWrite, databasePaste),
    databasePaste,
    flushOptimisticBlockWrites,
    ensurePageBody,
    ...richTextActions,
    ...mediaActions,
    ...tableActions,
    ...columnActions,
    ...tabActions,
    ...structuralActions,
    ...pasteActions,
    ...duplicationActions,
    ...movementActions,
    ...templateActions,
    isDatabaseCreationPending: (id) => structuralActions.isDatabaseCreationPending(id) || databasePaste.isCreating(id),
    discardPendingDatabaseCreations: () => {
      structuralActions.discardPendingDatabaseCreations();
      databasePaste.discardPendingCreations();
    },
  };
}
