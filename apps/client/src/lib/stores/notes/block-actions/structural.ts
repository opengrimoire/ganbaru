import { invalidateReplacedNotesMediaAsset } from "./media";
import { SvelteSet } from "svelte/reactivity";
import { createNotesCompoundPersistence, enqueueNotesCompoundEdit } from "$lib/stores/notes/compound-edits";
import type { NotesEditOperation } from "$lib/api/notes/compound-edits";
import {
  createNotesLinkedDatabaseView,
} from "$lib/api/notes";
import { blockColor, blockWithColor, canBlockHaveColor } from "$lib/notes/blocks/color";
import {
  blockConvertedToType,
  blockPlainText,
  blockWithCodeLanguage,
  blockIndent,
  blockUpdateWithIndent,
  blockWithHeadingToggleable,
  blockWithHeadingToggleOpen,
  blockWithTodoChecked,
  blockWithToggleOpen,
  createBlockUpdate,
  createBlockWrite,
  createColumnPayload,
  createEmptyTableRowPayload,
  DEFAULT_TABLE_ROW_COUNT,
  DEFAULT_TABLE_WIDTH,
  type NotesHeadingBlockType,
} from "$lib/notes/blocks/factory";
import {
  createBlockWriteFromInsertCommand,
  normalizeNotesBlockInsertCommand,
  type NotesBlockInsertRequest,
} from "$lib/notes/blocks/insertion";
import { createNotesLinkedDatabaseViewRequest } from "$lib/notes/database/linked";
import { planNotesInsertedBlockFocus } from "$lib/notes/editor/focus";
import {
  unsupportedBlockJsonText,
  unsupportedBlockSummaryText,
  type NotesUnsupportedConversionTarget,
} from "$lib/notes/block-types/unsupported";
import type {
  NotesAppendBlockChildrenRequest,
  NotesBlock,
  NotesBlockType,
  NotesBlockUpdate,
  NotesBlockWrite,
  NotesColor,
  NotesIcon,
  NotesColumnBlockItems,
  NotesTabBlockItems,
  NotesTableRowBlock,
  NotesParent,
} from "$lib/notes/types";
import type { NotesTextSelection } from "$lib/notes/editor/selection";
import type { NotesUndoKind, NotesUndoSnapshot } from "$lib/notes/history/undo-history";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";

const DEFAULT_DATABASE_TITLE = "Untitled database";
const START_OF_BLOCK_SELECTION: NotesTextSelection = { start: 0, end: 0 };

interface NotesStructuralBlockActionsContext {
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  readSelectedPageId: () => string | null;
  readChildIdsByParentId: () => Record<string, string[]>;
  blockById: (blockId: string) => NotesBlock | undefined;
  columnItemsForBlock: (blockId: string) => NotesColumnBlockItems[];
  tabItemsForBlock: (blockId: string) => NotesTabBlockItems[];
  tableRowsForBlock: (blockId: string) => NotesTableRowBlock[];
  createChildPageFromBlock: (blockId: string, clearText?: boolean) => Promise<void>;
  createChildPageAfterBlock: (blockId: string) => Promise<void>;
  requestBlockFocus: (blockId: string | null, selection?: NotesTextSelection | null) => void;
  flushBlockSave: (blockId: string) => Promise<void>;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  replaceBlockWithUpdate: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  appendAndApply: (request: NotesAppendBlockChildrenRequest) => Promise<NotesBlock[]>;
  optimisticBlockFromWrite: (write: NotesBlockWrite, parent: NotesParent) => NotesBlock;
  localInsertBlockAfter: (block: NotesBlock, afterBlockId: string | null) => void;
  undoSnapshot: (focusBlockId: string | null) => NotesUndoSnapshot | null;
  recordUndoAfter: (
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    focusBlockId: string | null,
  ) => void;
}

export interface NotesStructuralBlockActions {
  /** True while the block's database is still being created; callers keep the placeholder and defer database reads. */
  isDatabaseCreationPending: (blockId: string) => boolean;
  /** Forget reserved database creations after their queued writes were abandoned and will never retry. */
  discardPendingDatabaseCreations: () => void;
  convertBlock: (blockId: string, type: NotesBlockType, clearText?: boolean, selection?: NotesTextSelection) => Promise<void>;
  toggleTodo: (blockId: string, checked: boolean) => Promise<void>;
  updateCodeLanguage: (blockId: string, language: string) => Promise<void>;
  updateBlockColor: (blockId: string, color: NotesColor) => Promise<void>;
  updateCalloutIcon: (blockId: string, icon: NotesIcon | null) => Promise<void>;
  updateToggleOpen: (blockId: string, open: boolean) => Promise<void>;
  convertBlockToToggleHeading: (
    blockId: string,
    headingType: NotesHeadingBlockType,
    clearText?: boolean,
  ) => Promise<void>;
  createSiblingAfter: (blockId: string, request?: NotesBlockInsertRequest) => Promise<void>;
  createLinkedDatabaseViewAfter: (blockId: string) => Promise<void>;
  convertUnsupportedBlock: (
    blockId: string,
    target: NotesUnsupportedConversionTarget,
  ) => Promise<void>;
}

/** Create block conversion, insertion, and advanced database actions. */
export function createNotesStructuralBlockActions(
  context: NotesStructuralBlockActionsContext,
): NotesStructuralBlockActions {
  const pendingDatabaseCreations = new SvelteSet<string>();
  const {
    recordUndoAfter,
    replaceBlockWithUpdate,
    undoSnapshot,
  } = context;

  function applyEditorUpdate(blockId: string, update: NotesBlockUpdate): void {
    const previous = context.blockById(blockId);
    context.localApplyBlockUpdate(blockId, update);
    const persist = createNotesCompoundPersistence(context, "convert", [{ type: "update", block_id: blockId, update }]);
    void context.enqueueEditorMutation(async () => {
      await persist();
      invalidateReplacedNotesMediaAsset(previous, update);
    })
      .catch((error: unknown) => console.warn("Notes block update persistence failed", error));
  }

  /** Convert a block to another type; `selection` places the caret after an in-place conversion. */
  async function convertBlock(
    blockId: string,
    type: NotesBlockType,
    clearText = false,
    selection?: NotesTextSelection,
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type === "child_page" || pendingDatabaseCreations.has(blockId)) return;
    if (type === "child_page") {
      await context.createChildPageFromBlock(blockId, clearText);
      return;
    }
    if (type === "child_database") {
      await createDatabaseFromBlock(blockId, clearText);
      return;
    }
    const before = undoSnapshot(blockId);
    if (type === "table") {
      await createTableFromBlock(blockId);
      recordUndoAfter("convert", before, blockId);
      return;
    }
    if (type === "column_list") {
      await createColumnListFromBlock(blockId);
      recordUndoAfter("convert", before, blockId);
      return;
    }
    if (type === "tab") {
      await createTabFromBlock(blockId);
      recordUndoAfter("convert", before, blockId);
      return;
    }
    if (block.type === "table" || block.type === "table_row") return;
    if (block.type === "column_list" || block.type === "column") return;
    if (block.type === "tab") return;
    const update = blockUpdateWithIndent(clearText ? createBlockUpdate(type, "") : blockConvertedToType(block, type), blockIndent(block));
    applyEditorUpdate(blockId, update);
    context.requestBlockFocus(blockId, selection);
    recordUndoAfter("convert", before, blockId);
  }

  async function convertUnsupportedBlock(
    blockId: string,
    target: NotesUnsupportedConversionTarget,
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "unsupported") return;
    await context.flushBlockSave(blockId);
    const before = undoSnapshot(blockId);
    const text = target === "code"
      ? unsupportedBlockJsonText(block.unsupported)
      : unsupportedBlockSummaryText(block.unsupported);
    const update = createBlockUpdate(target === "code" ? "code" : "paragraph", text);
    await replaceBlockWithUpdate(blockId, update);
    context.requestBlockFocus(blockId);
    recordUndoAfter("convert", before, blockId);
  }

  /** Show the database placeholder immediately, then create the database after earlier queued edits. */
  function createDatabaseFromBlock(blockId: string, clearText: boolean): Promise<void> {
    const selectedPageId = context.readSelectedPageId();
    const block = context.blockById(blockId);
    if (!selectedPageId || !block || !canConvertBlockToDatabase(block)) return Promise.resolve();
    const before = undoSnapshot(blockId);
    const request = {
      id: blockId,
      data_source_id: crypto.randomUUID(),
      view_id: crypto.randomUUID(),
      title: clearText ? "" : blockPlainText(block).trim(),
      replace_block_id: blockId,
    };
    pendingDatabaseCreations.add(blockId);
    context.localApplyBlockUpdate(blockId, {
      type: "child_database",
      child_database: {
        title: request.title,
        database_id: request.id,
        data_source_id: request.data_source_id,
        view_id: request.view_id,
      },
    });
    const persist = createNotesCompoundPersistence(context, "convert", [{ type: "create_database", request }]);
    return context.enqueueEditorMutation(async () => {
      const created = (await persist()).databases[0];
      if (!created) throw new Error("Notes database edit returned no database");
      context.applyPostMutation({ blocks: [created.block], canonical: true });
      pendingDatabaseCreations.delete(blockId);
      context.requestBlockFocus(created.block.id);
      recordUndoAfter("convert", before, created.block.id);
    });
  }

  function canConvertBlockToDatabase(block: NotesBlock): boolean {
    if (
      block.type === "child_page"
      || block.type === "table"
      || block.type === "table_row"
      || block.type === "column_list"
      || block.type === "column"
      || block.type === "tab"
    ) {
      return false;
    }
    return block.type !== "child_database" || block.child_database.database_id === undefined;
  }

  /** Plan the initial editable children together with their structural parent. */
  function initialStructure(blockId: string, type: "table" | "column_list" | "tab", firstLabel = "Tab 1"): { operations: NotesEditOperation[]; focusId: string } {
    const parent: NotesParent = { type: "block_id", block_id: blockId };
    if (type === "table") return {
      focusId: blockId,
      operations: [{ type: "append", request: { parent, after: null, children: Array.from({ length: DEFAULT_TABLE_ROW_COUNT }, () => ({
        id: crypto.randomUUID(), type: "table_row" as const, table_row: createEmptyTableRowPayload(DEFAULT_TABLE_WIDTH),
      })) } }],
    };
    const firstId = crypto.randomUUID();
    const secondId = crypto.randomUUID();
    const firstContentId = crypto.randomUUID();
    const secondContentId = crypto.randomUUID();
    const children: NotesBlockWrite[] = type === "column_list"
      ? [{ id: firstId, type: "column", column: createColumnPayload(0.5) }, { id: secondId, type: "column", column: createColumnPayload(0.5) }]
      : [createBlockWrite(firstId, "paragraph", firstLabel), createBlockWrite(secondId, "paragraph", "Tab 2")];
    return { focusId: firstContentId, operations: [
      { type: "append", request: { parent, after: null, children } },
      { type: "append", request: { parent: { type: "block_id", block_id: firstId }, after: null, children: [createBlockWrite(firstContentId, "paragraph")] } },
      { type: "append", request: { parent: { type: "block_id", block_id: secondId }, after: null, children: [createBlockWrite(secondContentId, "paragraph")] } },
    ] };
  }

  async function createStructureFromBlock(blockId: string, type: "table" | "column_list" | "tab"): Promise<void> {
    const block = context.blockById(blockId);
    if (!context.readSelectedPageId() || !block || block.type === "child_page" || block.type === "table_row" || block.type === "column") return;
    if (block.has_children) {
      if (block.type === type) context.requestBlockFocus(blockId);
      return;
    }
    const initial = initialStructure(blockId, type, blockPlainText(block).trim() || "Tab 1");
    await enqueueNotesCompoundEdit(context, "convert", [{ type: "update", block_id: blockId, update: createBlockUpdate(type, "") }, ...initial.operations]);
    context.requestBlockFocus(initial.focusId);
  }

  const createTableFromBlock = (blockId: string) => createStructureFromBlock(blockId, "table");
  const createColumnListFromBlock = (blockId: string) => createStructureFromBlock(blockId, "column_list");
  const createTabFromBlock = (blockId: string) => createStructureFromBlock(blockId, "tab");

  async function toggleTodo(blockId: string, checked: boolean): Promise<void> {
    const block = context.blockById(blockId);
    if (!block) return;
    const before = undoSnapshot(blockId);
    applyEditorUpdate(blockId, blockWithTodoChecked(block, checked));
    recordUndoAfter("update", before, blockId);
  }

  async function updateCodeLanguage(blockId: string, language: string): Promise<void> {
    const block = context.blockById(blockId);
    if (!block) return;
    const before = undoSnapshot(blockId);
    applyEditorUpdate(blockId, blockWithCodeLanguage(block, language));
    recordUndoAfter("update", before, blockId);
  }

  async function updateBlockColor(blockId: string, color: NotesColor): Promise<void> {
    const block = context.blockById(blockId);
    if (!block) return;
    if (block.type === "callout" && color !== "default" && !color.endsWith("_background")) return;
    const before = undoSnapshot(blockId);
    applyEditorUpdate(blockId, blockWithColor(block, color));
    recordUndoAfter("formatting", before, blockId);
  }

  async function updateCalloutIcon(blockId: string, icon: NotesIcon | null): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type !== "callout") return;
    const before = undoSnapshot(blockId);
    const update = { type: "callout" as const, callout: { ...block.callout, icon } };
    applyEditorUpdate(blockId, update);
    recordUndoAfter("update", before, blockId);
  }

  async function updateToggleOpen(blockId: string, open: boolean): Promise<void> {
    const block = context.blockById(blockId);
    if (!block) return;
    const before = undoSnapshot(blockId);
    if (block.type === "toggle") {
      applyEditorUpdate(blockId, blockWithToggleOpen(block, open));
      recordUndoAfter("update", before, blockId);
      return;
    }
    if (
      (block.type === "heading_1" && block.heading_1.is_toggleable === true)
      || (block.type === "heading_2" && block.heading_2.is_toggleable === true)
      || (block.type === "heading_3" && block.heading_3.is_toggleable === true)
      || (block.type === "heading_4" && block.heading_4.is_toggleable === true)
    ) {
      applyEditorUpdate(blockId, blockWithHeadingToggleOpen(block, open));
      recordUndoAfter("update", before, blockId);
    }
  }

  async function convertBlockToToggleHeading(
    blockId: string,
    headingType: NotesHeadingBlockType,
    clearText = false,
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block) return;
    if (block.type === "child_page") return;
    if (block.type === "table" || block.type === "table_row") return;
    if (block.type === "column_list" || block.type === "column") return;
    const before = undoSnapshot(blockId);
    const update = clearText
      ? createBlockUpdate(
        headingType,
        "",
        canBlockHaveColor(block.type) ? blockColor(block) : "default",
        {
          isToggleable: true,
          open: true,
        },
      )
      : blockWithHeadingToggleable(block, headingType, true);
    applyEditorUpdate(blockId, update);
    context.requestBlockFocus(blockId);
    recordUndoAfter("convert", before, blockId);
  }

  async function createSiblingAfter(
    blockId: string,
    request?: NotesBlockInsertRequest,
  ): Promise<void> {
    const block = context.blockById(blockId);
    const selectedPageId = context.readSelectedPageId();
    if (!block || !selectedPageId) return;
    const command = normalizeNotesBlockInsertCommand(request);
    if (command.kind === "block" && command.blockType === "child_page") {
      await context.createChildPageAfterBlock(blockId);
      return;
    }
    if (command.kind === "block" && command.blockType === "child_database") {
      await createDatabaseAfterBlock(blockId);
      return;
    }
    await context.flushBlockSave(blockId);
    const before = undoSnapshot(blockId);
    const newBlockId = crypto.randomUUID();
    const operations: NotesEditOperation[] = [{ type: "append", request: {
      parent: block.parent, after: blockId, children: [createBlockWriteFromInsertCommand(newBlockId, command)],
    } }];
    const initial = command.kind === "block" && (command.blockType === "table" || command.blockType === "column_list" || command.blockType === "tab")
      ? initialStructure(newBlockId, command.blockType) : null;
    if (initial) operations.push(...initial.operations);
    await enqueueNotesCompoundEdit(context, "convert", operations);
    const focusBlockId = planNotesInsertedBlockFocus([initial?.focusId ?? newBlockId], blockId) ?? newBlockId;
    context.requestBlockFocus(focusBlockId, START_OF_BLOCK_SELECTION);
    recordUndoAfter("create", before, focusBlockId);
  }

  /** Insert a database placeholder immediately and commit it in editor write order. */
  function createDatabaseAfterBlock(blockId: string): Promise<void> {
    const block = context.blockById(blockId);
    const selectedPageId = context.readSelectedPageId();
    if (!block || !selectedPageId) return Promise.resolve();
    const before = undoSnapshot(blockId);
    const request = {
      id: crypto.randomUUID(),
      data_source_id: crypto.randomUUID(),
      view_id: crypto.randomUUID(),
      title: "",
      parent: block.parent,
      after_block_id: blockId,
    };
    pendingDatabaseCreations.add(request.id);
    context.localInsertBlockAfter(context.optimisticBlockFromWrite({
      id: request.id,
      type: "child_database",
      child_database: {
        title: request.title,
        database_id: request.id,
        data_source_id: request.data_source_id,
        view_id: request.view_id,
      },
    }, block.parent), blockId);
    const persist = createNotesCompoundPersistence(context, "convert", [{ type: "create_database", request }]);
    return context.enqueueEditorMutation(async () => {
      const created = (await persist()).databases[0];
      if (!created) throw new Error("Notes database edit returned no database");
      context.applyPostMutation({ blocks: [created.block], canonical: true });
      pendingDatabaseCreations.delete(request.id);
      context.requestBlockFocus(created.block.id);
      recordUndoAfter("create", before, created.block.id);
    });
  }

  async function createLinkedDatabaseViewAfter(blockId: string): Promise<void> {
    const block = context.blockById(blockId);
    const selectedPageId = context.readSelectedPageId();
    if (!block || block.type !== "child_database" || !selectedPageId) return;
    await context.flushBlockSave(blockId);
    const currentBlock = context.blockById(blockId) ?? block;
    if (currentBlock.type !== "child_database") return;
    const before = undoSnapshot(blockId);
    const created = await createNotesLinkedDatabaseView(
      createNotesLinkedDatabaseViewRequest(
        currentBlock,
        {
          databaseId: crypto.randomUUID(),
          viewId: crypto.randomUUID(),
        },
        currentBlock.child_database.title || DEFAULT_DATABASE_TITLE,
      ),
    );
    context.applyPostMutation({
      blocks: [created.block],
      placements: [{
        blockId: created.block.id,
        parent: currentBlock.parent,
        after: blockId,
      }],
    });
    context.requestBlockFocus(created.block.id);
    recordUndoAfter("create", before, created.block.id);
  }


  return {
    isDatabaseCreationPending: (blockId) => pendingDatabaseCreations.has(blockId),
    discardPendingDatabaseCreations: () => pendingDatabaseCreations.clear(),
    convertBlock,
    toggleTodo,
    updateCodeLanguage,
    updateBlockColor,
    updateCalloutIcon,
    updateToggleOpen,
    convertBlockToToggleHeading,
    createSiblingAfter,
    createLinkedDatabaseViewAfter,
    convertUnsupportedBlock,
  };
}
