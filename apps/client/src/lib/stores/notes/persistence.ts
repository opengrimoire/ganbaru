import { notesErrorDetail } from "$lib/notes/failure";
import { appendNotesBlockChildren, trashNotesBlock, updateNotesBlock } from "$lib/api/notes";
import { isNotesEditConflict } from "$lib/api/notes/mutation";
import { applyBlockUpdate, blockUpdateFromBlock } from "$lib/notes/blocks/factory";
import { rebaseNotesBlockEdit } from "$lib/notes/editor/block-rebase";
import { NotesHeldEditError } from "./held-edit-notice.svelte";
import {
  CLEAN_BLOCK_SAVE,
  INITIAL_SAVE_QUEUE,
  transitionBlockSave,
  transitionSaveQueue,
  type NotesBlockEditConflict,
  type NotesBlockSaveEvent,
  type NotesBlockSaveState,
  type NotesSaveQueueEvent,
  type NotesSaveQueueState,
  type NotesScheduledBlockSave,
} from "./block-save-state";
import type { NotesBlock, NotesBlockUpdate, NotesBlockWrite } from "$lib/notes/types";

export type { NotesBlockEditConflict };

/**
 * How the user settles a held block edit: keep the editor's content, take the canonical content, or keep the
 * canonical block and insert the editor's content as a new block after it.
 */
export type NotesBlockConflictChoice = "mine" | "theirs" | "both";

export { NotesHeldEditError };

export interface NotesBlockPersistenceContext {
  reconcileCanonicalBlocks?: (blocks: readonly NotesBlock[]) => void;
  readBlock: (blockId: string) => NotesBlock | undefined;
  beforeSave: (blockId: string) => Promise<void>;
  replaceBlock: (block: NotesBlock) => void;
  /** Called with the write queue's failure when it pauses, and with `null` once it runs again. */
  setLoadError: (message: string | null) => void;
  /** Called after each queued write commits, so other windows can refresh the page. */
  onCommitted?: () => void;
  /** Called when a queued write was refused because another window changed its base first. */
  onConflict?: () => void;
  /**
   * Reads one block's canonical content, or `null` when it is trashed or gone. With it, a refused save is replayed
   * on the canonical content instead of being abandoned.
   */
  readCanonicalBlock?: (blockId: string, pageId: string | null) => Promise<NotesBlock | null>;
  /** The page the editor shows now; a held edit keeps the page it was made on. */
  currentPageId?: () => string | null;
  /** Called when a block edit is held or its held text changes, and with `null` once the user's choice settles it. */
  onBlockConflict?: (blockId: string, conflict: NotesBlockEditConflict | null) => void;
  /** Called after a settled choice changed which blocks exist, so the page reloads its canonical tree. */
  onCanonicalChanged?: () => void;
  debounceMs: number;
}

export interface NotesBlockPersistence {
  readCanonicalRevision: (blockId: string) => string | undefined;
  acknowledgeCanonicalBlocks: (blocks: readonly NotesBlock[]) => void;
  retryEditorMutations: () => Promise<void>;
  hasLocalChanges: (blockId: string) => boolean;
  localApplyBlockUpdate: (blockId: string, update: NotesBlockUpdate) => void;
  markBlockLocallyChanged: (blockId: string) => void;
  saveBlockNow: (blockId: string, update: NotesBlockUpdate) => Promise<void>;
  scheduleBlockSave: (blockId: string, update: NotesBlockUpdate) => void;
  flushBlockSave: (blockId: string) => Promise<void>;
  flushPendingBlockSaves: () => Promise<void>;
  discardPendingEditorWrites: () => void;
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  runBetweenWrites: <T>(read: () => Promise<T>) => Promise<T>;
  /** The held conflict of a block, if its edit waits for the user to choose. */
  readBlockConflict: (blockId: string) => NotesBlockEditConflict | undefined;
  /**
   * Throws {@link NotesHeldEditError} while any block edit waits for the user to choose.
   *
   * @throws NotesHeldEditError when an edit is held.
   */
  assertNoHeldEdits: () => void;
  resolveBlockConflict: (blockId: string, choice: NotesBlockConflictChoice) => Promise<void>;
}

interface ScheduledTimer {
  handle: ReturnType<typeof setTimeout>;
  scheduled: NotesScheduledBlockSave;
}

/**
 * Create debounced block persistence helpers for the Notes store. Each block's save lifecycle and the write queue
 * are explicit states moved only by the transitions in `block-save-state.ts`; this module runs their effects
 * (debounce timers, the ordered write chain, and Notes API calls).
 */
export function createNotesBlockPersistence(
  context: NotesBlockPersistenceContext,
): NotesBlockPersistence {
  /** Unsaved blocks by ID; a block without an entry is clean. */
  const blockStates = new Map<string, NotesBlockSaveState>();
  const timers = new Map<string, ScheduledTimer>();
  /** The latest queued save of each block, so a flush without a scheduled save waits for it. */
  const saveChains = new Map<string, Promise<void>>();
  const canonicalRevisions = new Map<string, string>();
  const canonicalContentRevisions = new Map<string, string>();
  let editSequence = 0;
  let queue: NotesSaveQueueState = INITIAL_SAVE_QUEUE;
  let mutationChain = Promise.resolve();
  /** Queued writes not yet committed, kept in order so Retry replays them after a failure. */
  const retainedMutations: Array<() => Promise<void>> = [];

  function readState(blockId: string): NotesBlockSaveState {
    return blockStates.get(blockId) ?? CLEAN_BLOCK_SAVE;
  }

  function revisionOf(blockId: string): number {
    const state = readState(blockId);
    return state.phase === "clean" ? 0 : state.revision;
  }

  /** Move one block through its save lifecycle and run the effects the move implies. */
  function dispatch(blockId: string, event: NotesBlockSaveEvent): NotesBlockSaveState {
    const previous = readState(blockId);
    const next = transitionBlockSave(previous, event);
    if (next === previous) return next;
    if (next.phase === "clean") blockStates.delete(blockId);
    else blockStates.set(blockId, next);
    syncTimer(blockId, next);
    const heldBefore = previous.phase === "held" ? previous.conflict : null;
    const heldAfter = next.phase === "held" ? next.conflict : null;
    if (heldBefore !== heldAfter) context.onBlockConflict?.(blockId, heldAfter);
    return next;
  }

  /** Keep exactly one debounce timer per scheduled save, restarting it when a newer save replaces it. */
  function syncTimer(blockId: string, state: NotesBlockSaveState): void {
    const scheduled = state.phase === "dirty" ? state.scheduled : null;
    const timer = timers.get(blockId);
    if (timer?.scheduled === scheduled) return;
    if (timer) clearTimeout(timer.handle);
    timers.delete(blockId);
    if (!scheduled) return;
    const handle = setTimeout(() => {
      timers.delete(blockId);
      void flushScheduledSave(blockId)?.catch(() => undefined);
    }, context.debounceMs);
    timers.set(blockId, { handle, scheduled });
  }

  function dispatchQueue(event: NotesSaveQueueEvent): void {
    const previous = queue;
    queue = transitionSaveQueue(queue, event);
    if (queue !== previous) context.setLoadError(queue.phase === "failed" ? queue.error : null);
  }

  function readCanonicalRevision(blockId: string): string | undefined {
    return context.readBlock(blockId)?.edit_revision ?? canonicalRevisions.get(blockId);
  }

  function readCanonicalContentRevision(blockId: string): string | undefined {
    return context.readBlock(blockId)?.content_revision ?? canonicalContentRevisions.get(blockId);
  }

  function acknowledgeCanonicalBlocks(blocks: readonly NotesBlock[]): void {
    for (const block of blocks) {
      if (block.edit_revision) canonicalRevisions.set(block.id, block.edit_revision);
      if (block.content_revision) canonicalContentRevisions.set(block.id, block.content_revision);
    }
  }

  function markBlockLocallyChanged(blockId: string): void {
    const block = readState(blockId).phase === "clean" ? context.readBlock(blockId) : undefined;
    dispatch(blockId, { type: "edited", base: block ? blockUpdateFromBlock(block) : null, revision: ++editSequence });
  }

  function hasLocalChanges(blockId: string): boolean {
    return readState(blockId).phase !== "clean";
  }

  function localApplyBlockUpdate(blockId: string, update: NotesBlockUpdate): void {
    const block = context.readBlock(blockId);
    if (!block) return;
    markBlockLocallyChanged(blockId);
    context.replaceBlock(applyBlockUpdate(block, update));
  }

  async function saveBlockNow(blockId: string, update: NotesBlockUpdate, revision = revisionOf(blockId)): Promise<void> {
    const saveGeneration = queue.generation;
    const pageId = context.currentPageId?.() ?? null;
    const save = enqueue(async () => {
      await context.beforeSave(blockId);
      const state = readState(blockId);
      if (saveGeneration !== queue.generation || state.phase === "held") return;
      const base = state.phase === "dirty" ? state.base : null;
      let saved: NotesBlock;
      let rebased = false;
      try {
        saved = await updateNotesBlock(blockId, update, readCanonicalContentRevision(blockId));
      } catch (error) {
        const readCanonicalBlock = context.readCanonicalBlock;
        if (!isNotesEditConflict(error) || !readCanonicalBlock) {
          if (isNotesEditConflict(error)) dispatch(blockId, { type: "abandoned" });
          throw error;
        }
        const replayed = await replayRefusedSave(blockId, pageId, base, update, readCanonicalBlock);
        if (!replayed) return;
        saved = replayed;
        rebased = true;
      }
      if (saveGeneration !== queue.generation) return;
      acknowledgeCanonicalBlocks([saved]);
      context.reconcileCanonicalBlocks?.([saved]);
      const current = readState(blockId);
      const latest = current.phase === "clean" || (current.phase === "dirty" && current.revision === revision);
      dispatch(blockId, { type: "saved", revision, saved: blockUpdateFromBlock(saved) });
      if (latest) {
        if (context.readBlock(blockId)) context.replaceBlock(saved);
        return;
      }
      const block = context.readBlock(blockId);
      if (!block) return;
      const revisions = { edit_revision: saved.edit_revision, content_revision: saved.content_revision };
      context.replaceBlock({ ...block, ...revisions });
      if (rebased) replayLaterTyping(blockId, pageId, update, block, saved);
    });
    saveChains.set(blockId, save);
    try {
      await save;
    } finally {
      if (saveChains.get(blockId) === save) saveChains.delete(blockId);
    }
  }

  /**
   * Replays a save refused because the block's canonical content changed meanwhile. Returns the saved block, or
   * `null` when the edit is held for the user because both sides changed the text or the block was removed.
   */
  async function replayRefusedSave(
    blockId: string,
    pageId: string | null,
    base: NotesBlockUpdate | null,
    update: NotesBlockUpdate,
    readCanonicalBlock: (blockId: string, pageId: string | null) => Promise<NotesBlock | null>,
  ): Promise<NotesBlock | null> {
    const canonical = await readCanonicalBlock(blockId, pageId);
    const result = rebaseNotesBlockEdit(base ?? undefined, update, canonical ? blockUpdateFromBlock(canonical) : null);
    if (result.kind === "rebased" && canonical) {
      try {
        return await updateNotesBlock(blockId, result.update, canonical.content_revision);
      } catch (error) {
        if (isNotesEditConflict(error)) dispatch(blockId, { type: "abandoned" });
        throw error;
      }
    }
    holdConflict({ blockId, pageId, kind: canonical ? "text" : "removed", mine: update, theirs: canonical });
    return null;
  }

  /** Holds a block edit for the user to choose, keeping typing scheduled after the refused save as the held text. */
  function holdConflict(conflict: NotesBlockEditConflict): void {
    const state = readState(conflict.blockId);
    const later = state.phase === "dirty" ? state.scheduled?.update : undefined;
    dispatch(conflict.blockId, { type: "held", conflict: later ? { ...conflict, mine: later } : conflict });
  }

  /**
   * Carries typing made while a replayed save was in flight onto the merged content it saved, so the next save keeps
   * the canonical changes the replay took. Typing that clashes with what the replay took is held for the user.
   */
  function replayLaterTyping(
    blockId: string,
    pageId: string | null,
    sent: NotesBlockUpdate,
    current: NotesBlock,
    saved: NotesBlock,
  ): void {
    const later = blockUpdateFromBlock(current);
    const result = rebaseNotesBlockEdit(sent, later, blockUpdateFromBlock(saved));
    if (result.kind !== "rebased") {
      holdConflict({ blockId, pageId, kind: "text", mine: later, theirs: saved });
      return;
    }
    context.replaceBlock(applyBlockUpdate({ ...current, edit_revision: saved.edit_revision, content_revision: saved.content_revision }, result.update));
    scheduleBlockSave(blockId, result.update);
  }

  function readBlockConflict(blockId: string): NotesBlockEditConflict | undefined {
    const state = readState(blockId);
    return state.phase === "held" ? state.conflict : undefined;
  }

  function assertNoHeldEdits(): void {
    const blockIds: string[] = [];
    const pageIds = new Set<string>();
    for (const [blockId, state] of blockStates) {
      if (state.phase !== "held") continue;
      blockIds.push(blockId);
      if (state.conflict.pageId) pageIds.add(state.conflict.pageId);
    }
    if (blockIds.length > 0) throw new NotesHeldEditError(blockIds, [...pageIds]);
  }

  /** Saves the user's choice for a held block edit after the queued writes, on the block's current canonical content. */
  function resolveBlockConflict(blockId: string, choice: NotesBlockConflictChoice): Promise<void> {
    if (!readBlockConflict(blockId)) return Promise.resolve();
    return runChoice(async () => {
      const held = readBlockConflict(blockId);
      if (!held) return;
      const local = context.readBlock(blockId);
      const mine = local ? blockUpdateFromBlock(local) : held.mine;
      const canonical = context.readCanonicalBlock
        ? await context.readCanonicalBlock(blockId, held.pageId)
        : held.theirs;
      if (choice === "theirs") {
        dispatch(blockId, { type: "settled" });
        if (canonical) {
          acknowledgeCanonicalBlocks([canonical]);
          context.replaceBlock(canonical);
        }
        context.onCanonicalChanged?.();
        return;
      }
      if (choice === "both" && canonical) {
        const write = { ...mine, id: crypto.randomUUID() } as NotesBlockWrite;
        await appendNotesBlockChildren({ parent: canonical.parent, after: blockId, children: [write] });
        dispatch(blockId, { type: "settled" });
        acknowledgeCanonicalBlocks([canonical]);
        context.replaceBlock(canonical);
        context.onCanonicalChanged?.();
        return;
      }
      const revision = canonical ? canonical.content_revision : (await trashNotesBlock(blockId, false)).content_revision;
      const saved = await updateNotesBlock(blockId, mine, revision);
      dispatch(blockId, { type: "settled" });
      acknowledgeCanonicalBlocks([saved]);
      context.reconcileCanonicalBlocks?.([saved]);
      if (context.readBlock(blockId)) context.replaceBlock(saved);
      if (!canonical) context.onCanonicalChanged?.();
    });
  }

  function scheduleBlockSave(blockId: string, update: NotesBlockUpdate): void {
    dispatch(blockId, { type: "scheduled", update, revision: revisionOf(blockId) });
  }

  /** Sends a block's scheduled save to the write queue, or returns `null` when none is scheduled. */
  function flushScheduledSave(blockId: string): Promise<void> | null {
    const state = readState(blockId);
    if (state.phase !== "dirty" || !state.scheduled) return null;
    const { update, revision } = state.scheduled;
    dispatch(blockId, { type: "flushed" });
    return saveBlockNow(blockId, update, revision);
  }

  async function flushBlockSave(blockId: string): Promise<void> {
    const flushed = flushScheduledSave(blockId);
    if (flushed) return flushed;
    await saveChains.get(blockId);
  }

  /** Serializes content, structural edits, and history writes in input order. */
  function enqueue(mutation: () => Promise<void>): Promise<void> {
    retainedMutations.push(mutation);
    return runQueued(mutation);
  }

  function runQueued(mutation: () => Promise<void>): Promise<void> {
    const operationGeneration = queue.generation;
    const operation = mutationChain.then(async () => {
      if (operationGeneration !== queue.generation) return;
      let conflicted = false;
      try {
        await mutation();
      } catch (error) {
        // A refused base cannot succeed on retry, so the page reloads instead of holding the queue.
        if (!isNotesEditConflict(error)) throw error;
        conflicted = true;
      }
      if (operationGeneration !== queue.generation) return;
      const index = retainedMutations.indexOf(mutation);
      if (index >= 0) retainedMutations.splice(index, 1);
      if (conflicted) context.onConflict?.();
      else context.onCommitted?.();
    });
    mutationChain = operation;
    void operation.catch((error: unknown) => {
      dispatchQueue({ type: "write-failed", generation: operationGeneration, error: notesErrorDetail(error) });
    });
    return operation;
  }

  /**
   * Run a held-edit choice after the writes queued so far. The conflict card reports its failure and keeps the
   * edit held, so a failed choice is neither retained for Retry nor allowed to pause later writes.
   */
  function runChoice(choice: () => Promise<void>): Promise<void> {
    const operationGeneration = queue.generation;
    const previous = mutationChain;
    const result = previous.then(async () => {
      if (operationGeneration !== queue.generation) return;
      try {
        await choice();
      } catch (error) {
        if (!isNotesEditConflict(error)) throw error;
        if (operationGeneration === queue.generation) context.onConflict?.();
        return;
      }
      if (operationGeneration === queue.generation) context.onCommitted?.();
    });
    mutationChain = result.then(() => undefined, () => previous);
    return result;
  }

  /**
   * Run a canonical read after the writes queued so far and before later ones, without retaining it for retry or
   * reporting it as a commit. A failed read leaves the queue as it was.
   */
  function runBetweenWrites<T>(read: () => Promise<T>): Promise<T> {
    const previous = mutationChain;
    const result = previous.then(read);
    mutationChain = result.then(() => undefined, () => previous);
    return result;
  }

  /** Retries retained writes in order without replacing the user's local draft. */
  function retryEditorMutations(): Promise<void> {
    if (queue.phase !== "failed") return flushPendingBlockSaves();
    dispatchQueue({ type: "retried" });
    mutationChain = Promise.resolve();
    for (const mutation of [...retainedMutations]) void runQueued(mutation).catch(() => undefined);
    return flushPendingBlockSaves();
  }

  function flushPendingBlockSaves(): Promise<void> {
    for (const blockId of [...blockStates.keys()]) {
      void flushScheduledSave(blockId)?.catch(() => undefined);
    }
    return mutationChain;
  }

  /** Abandon local writes only after their selected page is confirmed inactive. */
  function discardPendingEditorWrites(): void {
    for (const timer of timers.values()) clearTimeout(timer.handle);
    timers.clear();
    retainedMutations.length = 0;
    const heldBlockIds = [...blockStates].filter(([, state]) => state.phase === "held").map(([blockId]) => blockId);
    blockStates.clear();
    for (const blockId of heldBlockIds) context.onBlockConflict?.(blockId, null);
    canonicalRevisions.clear();
    canonicalContentRevisions.clear();
    saveChains.clear();
    mutationChain = Promise.resolve();
    dispatchQueue({ type: "discarded" });
  }

  /** Captures pending typing before a structural edit enters the same write queue. */
  function enqueueEditorMutation(mutation: () => Promise<void>): Promise<void> {
    void flushPendingBlockSaves().catch(() => undefined);
    const captured = [...blockStates]
      .filter(([, state]) => state.phase === "dirty")
      .map(([blockId]) => [blockId, revisionOf(blockId)] as const);
    return enqueue(async () => {
      try {
        await mutation();
      } catch (error) {
        if (isNotesEditConflict(error)) for (const [blockId] of captured) dispatch(blockId, { type: "abandoned" });
        throw error;
      }
      for (const [blockId, revision] of captured) dispatch(blockId, { type: "committed", revision });
    });
  }

  return {
    readCanonicalRevision,
    acknowledgeCanonicalBlocks,
    retryEditorMutations,
    enqueueEditorMutation,
    runBetweenWrites,
    readBlockConflict,
    assertNoHeldEdits,
    resolveBlockConflict,
    hasLocalChanges,
    localApplyBlockUpdate,
    markBlockLocallyChanged,
    saveBlockNow,
    scheduleBlockSave,
    flushBlockSave,
    flushPendingBlockSaves,
    discardPendingEditorWrites,
  };
}
