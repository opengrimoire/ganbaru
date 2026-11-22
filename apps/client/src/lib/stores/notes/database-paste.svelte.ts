import { notesErrorDetail } from "$lib/notes/failure";
import { SvelteSet } from "svelte/reactivity";
import { getNotesDatabaseReference } from "$lib/api/notes";
import { createNotesCompoundPersistence } from "./compound-edits";
import type { NotesEditOperation } from "$lib/api/notes/compound-edits";
import { blockEditableRichText, blockPlainText, blockWithRichText, createBlockWrite } from "$lib/notes/blocks/factory";
import { notesLinkTarget } from "$lib/notes/links/navigation";
import { createDatabaseMentionRichText, replaceRichTextRange, richTextRangeSlice } from "$lib/notes/rich-text/core";
import type { NotesBlock, NotesBlockWrite, NotesChildDatabaseBlock, NotesCreatedDatabase, NotesParent, NotesRichText } from "$lib/notes/types";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";
import type { NotesUndoSnapshot } from "$lib/notes/history/undo-history";
import {
  IDLE_DATABASE_PASTE,
  transitionDatabasePaste,
  type NotesDatabasePasteEvent,
  type NotesDatabasePastePrompt,
  type NotesDatabasePasteState,
} from "./database-paste-state";

export type { NotesDatabasePastePrompt } from "./database-paste-state";

interface DatabasePasteContext {
  readPageId: () => string | null;
  blockById: (id: string) => NotesBlock | undefined;
  enqueue: (mutation: () => Promise<void>) => Promise<void>;
  retry: () => Promise<void>;
  apply: (result: NotesPostMutationResult) => void;
  updateLocal: (id: string, richText: readonly NotesRichText[]) => void;
  optimisticBlock: (write: NotesBlockWrite, parent: NotesParent) => NotesBlock;
  requestFocus: (id: string) => void;
  updateRichText: (id: string, richText: readonly NotesRichText[]) => Promise<void>;
  snapshot: (focusId: string | null) => NotesUndoSnapshot | null;
  recordUndo: (before: NotesUndoSnapshot | null, after: NotesUndoSnapshot | null) => void;
  reconcileIdentity?: (block: NotesChildDatabaseBlock) => void;
  reconcileCanonicalBlocks?: (blocks: readonly NotesBlock[]) => void;
}

/** Own transient database paste choices without retaining copied row data. */
export function createNotesDatabasePasteController(context: DatabasePasteContext) {
  let state = $state<NotesDatabasePasteState>(IDLE_DATABASE_PASTE);
  /** Names the latest prompt or link inspection; bumped by every new prompt and every dismissal. */
  let generation = 0;
  let writeToken = 0;
  const creating = new SvelteSet<string>();
  const compoundContext = { readSelectedPageId: context.readPageId, blockById: context.blockById, applyPostMutation: context.apply, reconcileCanonicalBlocks: context.reconcileCanonicalBlocks };

  function dispatch(event: NotesDatabasePasteEvent): void {
    state = transitionDatabasePaste(state, event);
  }

  function offer(prompt: NotesDatabasePastePrompt, promptGeneration = ++generation): void {
    dispatch({ type: "offered", prompt, generation: promptGeneration });
  }

  /** Close the prompt. A write already queued from it keeps running and reports through the editor's save banner. */
  function dismiss(): void {
    generation += 1;
    dispatch({ type: "dismissed" });
  }

  function beginCopies(copies: Readonly<Record<string, string>>, pending = true): void {
    const pageId = context.readPageId();
    const ids = Object.keys(copies);
    if (!pageId || !ids.length) return;
    if (pending) for (const id of ids) creating.add(id);
    offer({ kind: "copy", pageId, blockId: ids[ids.length - 1], copies });
  }

  function acceptCopy(id: string, created: NotesCreatedDatabase): void {
    const current = context.blockById(id);
    if (current?.type === "child_database") {
      context.apply({ blocks: [{ ...created.block, child_database: { ...created.block.child_database, title: current.child_database.title } }] });
    }
    context.reconcileIdentity?.(created.block);
    creating.delete(id);
  }

  /** Offer local database choices only while the pasted text range still exists. */
  async function inspectLink(blockId: string, start: number, end: number, url: string): Promise<void> {
    const target = notesLinkTarget(url, typeof window === "undefined" ? undefined : window.location.href);
    const pageId = context.readPageId();
    const block = context.blockById(blockId);
    if (!pageId || !block || !target?.blockId) return;
    const known = context.blockById(target.blockId);
    if (known && known.type !== "child_database") return;
    const text = blockPlainText(block).slice(start, end);
    const inspection = ++generation;
    try {
      const reference = await getNotesDatabaseReference(target.blockId);
      const latest = context.blockById(blockId);
      if (!latest || inspection !== generation || context.readPageId() !== pageId
        || reference.page_id !== target.pageId || blockPlainText(latest).slice(start, end) !== text) return;
      offer({ kind: "link", pageId, blockId, start, end, text, url, reference }, inspection);
    } catch (caught: unknown) {
      // Ordinary local block links retain their hyperlink when no database exists.
      console.debug("Pasted Notes block reference has no available database", caught);
    }
  }

  function currentLink(value: Extract<NotesDatabasePastePrompt, { kind: "link" }>): NotesBlock {
    const block = context.blockById(value.blockId);
    if (!block || context.readPageId() !== value.pageId || blockPlainText(block).slice(value.start, value.end) !== value.text) {
      throw new Error("The pasted database reference has changed");
    }
    return block;
  }

  /** Run one chosen action for the prompt offered now, ignoring a second choice while one runs. */
  async function runChoice(
    allowed: (current: Extract<NotesDatabasePasteState, { phase: "offered" | "failed" }>) => boolean,
    run: (prompt: NotesDatabasePastePrompt, promptGeneration: number, token: number) => Promise<void>,
  ): Promise<void> {
    const current = state;
    if ((current.phase !== "offered" && current.phase !== "failed") || !allowed(current)) return;
    const token = ++writeToken;
    dispatch({ type: "write-started", token });
    try {
      await run(current.prompt, current.generation, token);
      dispatch({ type: "write-settled", token });
    } catch (caught: unknown) {
      dispatch({ type: "write-failed", token, error: notesErrorDetail(caught) });
    }
  }

  function mention(): Promise<void> {
    return runChoice((current) => current.prompt.kind === "link" && !(current.phase === "failed" && current.started), async (current, promptGeneration) => {
      if (current.kind !== "link") return;
      const block = currentLink(current);
      const mention = createDatabaseMentionRichText(current.reference.block_id, current.reference.title);
      mention.href = current.url;
      const original = richTextRangeSlice(blockEditableRichText(block), current.start, current.end)[0];
      if (original) mention.annotations = { ...original.annotations };
      await context.updateRichText(block.id, replaceRichTextRange(blockEditableRichText(block), current.start, current.end, [mention]));
      dispatch({ type: "completed", generation: promptGeneration });
    });
  }

  function syncCopies(current: Extract<NotesDatabasePastePrompt, { kind: "copy" }>, promptGeneration: number, token: number): Promise<void> {
    const before = context.snapshot(current.blockId);
    const items = Object.entries(current.copies).map(([copyId, sourceId]) => {
      const block = context.blockById(copyId);
      if (!block || block.type !== "child_database") throw new Error("The pasted database is no longer available");
      return { block, sourceId, id: crypto.randomUUID(), viewId: crypto.randomUUID() };
    });
    dispatch({ type: "enqueued", token });
    const operations = items.flatMap(({ block, sourceId, id, viewId }): NotesEditOperation[] => [
      { type: "link_database", request: { id, view_id: viewId, source_block_id: sourceId, title: block.child_database.title, parent: block.parent, after_block_id: block.id } },
      { type: "trash", block_id: block.id, in_trash: true },
    ]);
    const persist = createNotesCompoundPersistence(compoundContext, "paste", operations);
    return context.enqueue(async () => {
      if (context.readPageId() !== current.pageId) throw new Error("The destination note has changed");
      const result = await persist();
      context.apply({ blocks: result.blocks.filter((block) => !block.in_trash), placements: result.placements, removedBlockIds: items.map((item) => item.block.id) });
      if (items.length) context.requestFocus(items[items.length - 1].id);
      context.recordUndo(before, context.snapshot(items.at(-1)?.id ?? null));
      dispatch({ type: "completed", generation: promptGeneration });
    });
  }

  function linkView(current: Extract<NotesDatabasePastePrompt, { kind: "link" }>, promptGeneration: number, token: number): Promise<void> {
    const block = currentLink(current);
    dispatch({ type: "enqueued", token });
    const before = context.snapshot(block.id);
    const whole = current.start === 0 && current.end === blockPlainText(block).length;
    const id = crypto.randomUUID();
    const viewId = crypto.randomUUID();
    const prefix = richTextRangeSlice(blockEditableRichText(block), 0, current.start);
    const suffix = richTextRangeSlice(blockEditableRichText(block), current.end, blockPlainText(block).length);
    const write = createBlockWrite(id, "child_database", current.reference.title);
    const suffixWrite = suffix.length ? { ...createBlockWrite(crypto.randomUUID(), "paragraph", ""),
      type: "paragraph" as const, paragraph: { rich_text: suffix, color: "default" as const } } : null;
    if (!whole) context.updateLocal(block.id, prefix);
    context.apply({ blocks: [context.optimisticBlock(write, block.parent)], placements: [{ blockId: id, parent: block.parent, after: block.id }] });
    if (suffixWrite) context.apply({ blocks: [context.optimisticBlock(suffixWrite, block.parent)], placements: [{ blockId: suffixWrite.id, parent: block.parent, after: id }] });
    if (whole) context.apply({ removedBlockIds: [block.id] });
    creating.add(id);
    dispatch({ type: "retargeted", generation: promptGeneration, blockId: id });
    context.requestFocus(id);
    context.recordUndo(before, context.snapshot(id));
    const operations: NotesEditOperation[] = [];
    if (!whole) operations.push({ type: "update", block_id: block.id, update: blockWithRichText(block, prefix) });
    operations.push({ type: "link_database", request: { id, view_id: viewId, source_block_id: current.reference.block_id, parent: block.parent, after_block_id: block.id } });
    if (suffixWrite) operations.push({ type: "append", request: { parent: block.parent, after: id, children: [suffixWrite] } });
    if (whole) operations.push({ type: "trash", block_id: block.id, in_trash: true });
    const persist = createNotesCompoundPersistence(compoundContext, "paste", operations, [block]);
    return context.enqueue(async () => {
      const result = await persist();
      for (const created of result.databases) acceptCopy(created.block.id, created);
      dispatch({ type: "completed", generation: promptGeneration });
    });
  }

  /** Create linked views for the prompt, or retry the editor queue once its writes were queued. */
  function linkedView(): Promise<void> {
    return runChoice(() => true, async (current, promptGeneration, token) => {
      if (state.phase === "writing" && state.started) {
        await context.retry();
        return;
      }
      await (current.kind === "copy" ? syncCopies(current, promptGeneration, token) : linkView(current, promptGeneration, token));
    });
  }

  /** Forget pending database copies after their queued writes were abandoned, closing a prompt that queued them. */
  function discardPendingCreations(): void {
    creating.clear();
    if (state.phase !== "idle" && state.phase !== "offered" && state.started) dismiss();
  }

  return {
    get prompt(): NotesDatabasePastePrompt | null { return state.phase === "idle" ? null : state.prompt; },
    get busy(): boolean { return state.phase === "writing"; },
    get error(): string | null { return state.phase === "failed" ? state.error : null; },
    get started(): boolean { return (state.phase === "writing" || state.phase === "failed") && state.started; },
    isCreating: (id: string) => creating.has(id), beginCopies, acceptCopy, inspectLink, dismiss, mention, linkedView, discardPendingCreations,
  };
}

export type NotesDatabasePasteController = ReturnType<typeof createNotesDatabasePasteController>;
