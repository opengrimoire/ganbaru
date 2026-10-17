import { createNotesCompoundPersistence, type NotesCompoundEditContext } from "$lib/stores/notes/compound-edits";
import {
  blockPlainText,
  blockWithDateMention,
  blockWithInlineEquation,
  blockWithObjectMention,
  blockWithPageMention,
  blockWithRichText,
  blockWithText,
  blockWithTextAnnotations,
  blockWithTextLink,
} from "$lib/notes/blocks/factory";
import type { NotesTextSelection } from "$lib/notes/editor/selection";
import type {
  NotesObjectMentionTarget,
  NotesRichTextAnnotationPatch,
} from "$lib/notes/rich-text/core";
import { notesTextChangeUndoSelections } from "$lib/notes/history/undo-history";
import type {
  NotesBlock,
  NotesBlockUpdate,
  NotesDateMentionValue,
  NotesRichText,
} from "$lib/notes/types";
import type {
  NotesUndoKind,
  NotesUndoRecordOptions,
  NotesUndoSnapshot,
} from "$lib/notes/history/undo-history";

interface NotesRichTextBlockActionsContext extends NotesCompoundEditContext {
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  blockById: (blockId: string) => NotesBlock | undefined;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  scheduleBlockSave: (blockId: string, update: NotesBlockUpdate) => void;
  saveBlockNow: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  flushBlockSave: (blockId: string) => Promise<void>;
  refreshOpenLinks: () => Promise<void>;
  hasPendingOptimisticWrite: (blockId: string) => boolean;
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

export interface NotesRichTextBlockActions {
  updateBlockText: (
    blockId: string,
    text: string,
    selection?: NotesTextSelection | null,
  ) => Promise<void>;
  updateBlockRichText: (blockId: string, richText: readonly NotesRichText[]) => Promise<void>;
  insertPageMention: (
    blockId: string,
    start: number,
    end: number,
    pageId: string,
    title: string,
    href: string | null,
  ) => Promise<void>;
  insertDateMention: (
    blockId: string,
    start: number,
    end: number,
    date: NotesDateMentionValue,
    title: string,
  ) => Promise<void>;
  insertObjectMention: (
    blockId: string,
    start: number,
    end: number,
    target: NotesObjectMentionTarget,
  ) => Promise<void>;
  insertInlineEquation: (
    blockId: string,
    start: number,
    end: number,
    expression: string,
  ) => Promise<void>;
  updateBlockTextLink: (
    blockId: string,
    start: number,
    end: number,
    url: string | null,
  ) => Promise<void>;
  updateBlockTextAnnotations: (
    blockId: string,
    start: number,
    end: number,
    patch: NotesRichTextAnnotationPatch,
  ) => Promise<void>;
}

/** Create rich-text, mention, link, and inline-formatting mutations. */
export function createNotesRichTextBlockActions(
  context: NotesRichTextBlockActionsContext,
): NotesRichTextBlockActions {
  function snapshot(
    blockId: string,
    selection: NotesTextSelection | null = null,
  ): NotesUndoSnapshot | null {
    return context.createUndoSnapshotForBlocks(blockId, [blockId], [], selection);
  }

  function record(
    kind: NotesUndoKind,
    before: NotesUndoSnapshot | null,
    blockId: string,
    groupKey: string | null = null,
    selection: NotesTextSelection | null = null,
  ): void {
    context.recordUndo({ kind, before, after: snapshot(blockId, selection), groupKey });
  }

  async function updateBlockText(
    blockId: string,
    text: string,
    selection: NotesTextSelection | null = null,
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type === "child_page" || block.type === "child_database") return;
    const currentText = blockPlainText(block);
    if (currentText === text) return;
    const undoSelections = notesTextChangeUndoSelections(currentText, text, selection);
    const before = snapshot(blockId, undoSelections.before);
    const update = blockWithText(block, text);
    context.localApplyBlockUpdate(blockId, update);
    context.scheduleBlockSave(blockId, update);
    record("typing", before, blockId, `typing:${blockId}`, undoSelections.after);
  }

  async function updateBlockRichText(
    blockId: string,
    richText: readonly NotesRichText[],
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block || block.type === "child_page" || block.type === "child_database") return;
    const before = snapshot(blockId);
    const update = blockWithRichText(block, richText);
    context.localApplyBlockUpdate(blockId, update);
    context.scheduleBlockSave(blockId, update);
    record("typing", before, blockId, `typing:${blockId}`);
  }

  async function persistImmediate(
    blockId: string,
    kind: NotesUndoKind,
    build: (block: NotesBlock) => NotesBlockUpdate,
    refreshLinks = false,
  ): Promise<void> {
    const block = context.blockById(blockId);
    if (!block) return;
    const before = snapshot(blockId);
    const update = build(block);
    context.localApplyBlockUpdate(blockId, update);
    const persist = createNotesCompoundPersistence(context, "format_selection", [{ type: "update", block_id: blockId, update }]);
    void context.enqueueEditorMutation(async () => {
      await persist();
      if (refreshLinks) await context.refreshOpenLinks();
    }).catch((error: unknown) => console.warn("Notes formatting persistence failed", error));
    context.recordUndo({
      kind,
      before,
      after: snapshot(blockId),
      groupKey: null,
    });
  }

  const insertPageMention: NotesRichTextBlockActions["insertPageMention"] = async (
    blockId, start, end, pageId, title, href,
  ) => persistImmediate(
    blockId,
    "mention",
    (block) => blockWithPageMention(block, start, end, pageId, title, href),
    true,
  );
  const insertDateMention: NotesRichTextBlockActions["insertDateMention"] = async (
    blockId, start, end, date, title,
  ) => persistImmediate(
    blockId,
    "mention",
    (block) => blockWithDateMention(block, start, end, date, title),
  );
  const insertObjectMention: NotesRichTextBlockActions["insertObjectMention"] = async (
    blockId, start, end, target,
  ) => persistImmediate(
    blockId,
    "mention",
    (block) => blockWithObjectMention(block, start, end, target),
  );
  const insertInlineEquation: NotesRichTextBlockActions["insertInlineEquation"] = async (
    blockId, start, end, expression,
  ) => persistImmediate(
    blockId,
    "equation",
    (block) => blockWithInlineEquation(block, start, end, expression),
  );
  const updateBlockTextLink: NotesRichTextBlockActions["updateBlockTextLink"] = async (
    blockId, start, end, url,
  ) => persistImmediate(
    blockId,
    "link",
    (block) => blockWithTextLink(block, start, end, url),
    true,
  );
  const updateBlockTextAnnotations: NotesRichTextBlockActions["updateBlockTextAnnotations"] = async (
    blockId, start, end, patch,
  ) => persistImmediate(
    blockId,
    "formatting",
    (block) => blockWithTextAnnotations(block, start, end, patch),
  );

  return {
    updateBlockText,
    updateBlockRichText,
    insertPageMention,
    insertDateMention,
    insertObjectMention,
    insertInlineEquation,
    updateBlockTextLink,
    updateBlockTextAnnotations,
  };
}
