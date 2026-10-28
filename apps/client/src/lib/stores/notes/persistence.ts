import { appendNotesBlockChildren, trashNotesBlock, updateNotesBlock } from "$lib/api/notes";
import { isNotesEditConflict } from "$lib/api/notes/mutation";
import { applyBlockUpdate, blockUpdateFromBlock } from "$lib/notes/blocks/factory";
import { rebaseNotesBlockEdit } from "$lib/notes/editor/block-rebase";
import type { NotesBlock, NotesBlockUpdate, NotesBlockWrite } from "$lib/notes/types";

interface PendingBlockSave {
  timer: ReturnType<typeof setTimeout>;
  update: NotesBlockUpdate;
  revision: number;
}

/** An unsaved block edit that cannot be replayed on the block's canonical content without the user choosing. */
export interface NotesBlockEditConflict {
  blockId: string;
  /** The page the block was open on when its edit was held, so its canonical content is read from that page. */
  pageId: string | null;
  /** `text` when both sides changed the text differently; `removed` when the block was trashed or deleted elsewhere. */
  kind: "text" | "removed";
  /** The editor's unsaved content. */
  mine: NotesBlockUpdate;
  /** The canonical block, or `null` when it was removed. */
  theirs: NotesBlock | null;
}

/**
 * How the user settles a held block edit: keep the editor's content, take the canonical content, or keep the
 * canonical block and insert the editor's content as a new block after it.
 */
export type NotesBlockConflictChoice = "mine" | "theirs" | "both";

/**
 * Raised by a flush while a block edit waits for the user to choose, so navigation and window close keep the page
 * and its draft open instead of leaving the held edit behind.
 */
export class NotesHeldEditError extends Error {
  constructor(readonly blockIds: readonly string[]) {
    super(`Notes block edits wait for a choice: ${blockIds.join(", ")}`);
    this.name = "NotesHeldEditError";
  }
}

export interface NotesBlockPersistenceContext {
  reconcileCanonicalBlocks?: (blocks: readonly NotesBlock[]) => void;
  readBlock: (blockId: string) => NotesBlock | undefined;
  beforeSave: (blockId: string) => Promise<void>;
  replaceBlock: (block: NotesBlock) => void;
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
  /** Called when a refused save is held for the user to choose, and with `null` once that choice settles it. */
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

/**
 * Create debounced block persistence helpers for the Notes store.
 */
export function createNotesBlockPersistence(
  context: NotesBlockPersistenceContext,
): NotesBlockPersistence {
  const pendingBlockSaves = new Map<string, PendingBlockSave>();
  const canonicalRevisions = new Map<string, string>();
  const canonicalContentRevisions = new Map<string, string>();
  const blockRevisions = new Map<string, number>();
  let mutationChain = Promise.resolve();
  let queueGeneration = 0;
  let failed = false;
  const queuedMutations: Array<() => Promise<void>> = [];
  const dirtyBlocks = new Set<string>();
  const saveChains = new Map<string, Promise<void>>();
  /** The canonical content each dirty block's edit started from, to replay the edit when its save is refused. */
  const baseContents = new Map<string, NotesBlockUpdate>();
  const heldConflicts = new Map<string, NotesBlockEditConflict>();

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

  /** Drop local typing for a block whose base changed elsewhere, so the canonical reload can replace it. */
  function abandonConflictedBlock(blockId: string): void {
    const pending = pendingBlockSaves.get(blockId);
    if (pending) clearTimeout(pending.timer);
    pendingBlockSaves.delete(blockId);
    dirtyBlocks.delete(blockId);
    baseContents.delete(blockId);
  }

  function markBlockLocallyChanged(blockId: string): void {
    if (!dirtyBlocks.has(blockId)) {
      const block = context.readBlock(blockId);
      if (block) baseContents.set(blockId, blockUpdateFromBlock(block));
      else baseContents.delete(blockId);
    }
    dirtyBlocks.add(blockId);
    blockRevisions.set(blockId, (blockRevisions.get(blockId) ?? 0) + 1);
  }

  function hasLocalChanges(blockId: string): boolean {
    return dirtyBlocks.has(blockId);
  }

  function localApplyBlockUpdate(blockId: string, update: NotesBlockUpdate): void {
    const block = context.readBlock(blockId);
    if (!block) return;
    markBlockLocallyChanged(blockId);
    context.replaceBlock(applyBlockUpdate(block, update));
  }

  async function saveBlockNow(blockId: string, update: NotesBlockUpdate, revision = blockRevisions.get(blockId) ?? 0): Promise<void> {
    const saveGeneration = queueGeneration;
    const pageId = context.currentPageId?.() ?? null;
    const save = enqueue(async () => {
      await context.beforeSave(blockId);
      if (saveGeneration !== queueGeneration || heldConflicts.has(blockId)) return;
      const base = baseContents.get(blockId);
      let saved: NotesBlock;
      let rebased = false;
      try {
        saved = await updateNotesBlock(blockId, update, readCanonicalContentRevision(blockId));
      } catch (error) {
        const readCanonicalBlock = context.readCanonicalBlock;
        if (!isNotesEditConflict(error) || !readCanonicalBlock) {
          if (isNotesEditConflict(error)) abandonConflictedBlock(blockId);
          throw error;
        }
        const replayed = await replayRefusedSave(blockId, pageId, base, update, readCanonicalBlock);
        if (!replayed) return;
        saved = replayed;
        rebased = true;
      }
      if (saveGeneration !== queueGeneration) return;
      acknowledgeCanonicalBlocks([saved]);
      context.reconcileCanonicalBlocks?.([saved]);
      if ((blockRevisions.get(blockId) ?? 0) === revision) {
        dirtyBlocks.delete(blockId);
        baseContents.delete(blockId);
        if (context.readBlock(blockId)) context.replaceBlock(saved);
      } else {
        baseContents.set(blockId, blockUpdateFromBlock(saved));
        const current = context.readBlock(blockId);
        if (current) {
          const revisions = { edit_revision: saved.edit_revision, content_revision: saved.content_revision };
          context.replaceBlock({ ...current, ...revisions });
          if (rebased) replayLaterTyping(blockId, pageId, update, current, saved);
        }
      }
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
    base: NotesBlockUpdate | undefined,
    update: NotesBlockUpdate,
    readCanonicalBlock: (blockId: string, pageId: string | null) => Promise<NotesBlock | null>,
  ): Promise<NotesBlock | null> {
    const canonical = await readCanonicalBlock(blockId, pageId);
    const result = rebaseNotesBlockEdit(base, update, canonical ? blockUpdateFromBlock(canonical) : null);
    if (result.kind === "rebased" && canonical) {
      try {
        return await updateNotesBlock(blockId, result.update, canonical.content_revision);
      } catch (error) {
        if (isNotesEditConflict(error)) abandonConflictedBlock(blockId);
        throw error;
      }
    }
    holdConflict({ blockId, pageId, kind: canonical ? "text" : "removed", mine: update, theirs: canonical });
    return null;
  }

  /** Holds a block edit for the user to choose; its scheduled save would only overwrite the canonical content. */
  function holdConflict(conflict: NotesBlockEditConflict): void {
    const pending = pendingBlockSaves.get(conflict.blockId);
    if (pending) clearTimeout(pending.timer);
    pendingBlockSaves.delete(conflict.blockId);
    heldConflicts.set(conflict.blockId, conflict);
    context.onBlockConflict?.(conflict.blockId, conflict);
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
    return heldConflicts.get(blockId);
  }

  function assertNoHeldEdits(): void {
    if (heldConflicts.size > 0) throw new NotesHeldEditError([...heldConflicts.keys()]);
  }

  /** Releases a held block once the user's choice is saved. */
  function settleBlockConflict(blockId: string): void {
    const pending = pendingBlockSaves.get(blockId);
    if (pending) clearTimeout(pending.timer);
    pendingBlockSaves.delete(blockId);
    heldConflicts.delete(blockId);
    dirtyBlocks.delete(blockId);
    baseContents.delete(blockId);
    context.onBlockConflict?.(blockId, null);
  }

  /** Saves the user's choice for a held block edit in the write queue, on the block's current canonical content. */
  function resolveBlockConflict(blockId: string, choice: NotesBlockConflictChoice): Promise<void> {
    const held = heldConflicts.get(blockId);
    if (!held) return Promise.resolve();
    return enqueue(async () => {
      const local = context.readBlock(blockId);
      const mine = local ? blockUpdateFromBlock(local) : held.mine;
      const canonical = context.readCanonicalBlock
        ? await context.readCanonicalBlock(blockId, held.pageId)
        : held.theirs;
      if (choice === "theirs") {
        settleBlockConflict(blockId);
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
        settleBlockConflict(blockId);
        acknowledgeCanonicalBlocks([canonical]);
        context.replaceBlock(canonical);
        context.onCanonicalChanged?.();
        return;
      }
      const revision = canonical ? canonical.content_revision : (await trashNotesBlock(blockId, false)).content_revision;
      const saved = await updateNotesBlock(blockId, mine, revision);
      settleBlockConflict(blockId);
      acknowledgeCanonicalBlocks([saved]);
      context.reconcileCanonicalBlocks?.([saved]);
      if (context.readBlock(blockId)) context.replaceBlock(saved);
      if (!canonical) context.onCanonicalChanged?.();
    });
  }

  function scheduleBlockSave(blockId: string, update: NotesBlockUpdate): void {
    const pending = pendingBlockSaves.get(blockId);
    if (pending) clearTimeout(pending.timer);
    const revision = blockRevisions.get(blockId) ?? 0;
    const saveGeneration = queueGeneration;
    const timer = setTimeout(() => {
      pendingBlockSaves.delete(blockId);
      void saveBlockNow(blockId, update, revision).catch((error: unknown) => {
        if (saveGeneration !== queueGeneration) return;
        context.setLoadError(error instanceof Error ? error.message : String(error));
      });
    }, context.debounceMs);
    pendingBlockSaves.set(blockId, { timer, update, revision });
  }

  async function flushBlockSave(blockId: string): Promise<void> {
    const pending = pendingBlockSaves.get(blockId);
    if (pending) {
      clearTimeout(pending.timer);
      pendingBlockSaves.delete(blockId);
      await saveBlockNow(blockId, pending.update, pending.revision);
      return;
    }
    await saveChains.get(blockId);
  }

  /** Serializes content, structural edits, and history writes in input order. */
  function enqueue(mutation: () => Promise<void>): Promise<void> {
    queuedMutations.push(mutation);
    return runQueued(mutation);
  }

  function runQueued(mutation: () => Promise<void>): Promise<void> {
    const operationGeneration = queueGeneration;
    const operation = mutationChain.then(async () => {
      if (operationGeneration !== queueGeneration) return;
      let conflicted = false;
      try {
        await mutation();
      } catch (error) {
        // A refused base cannot succeed on retry, so the page reloads instead of holding the queue.
        if (!isNotesEditConflict(error)) throw error;
        conflicted = true;
      }
      if (operationGeneration !== queueGeneration) return;
      const index = queuedMutations.indexOf(mutation);
      if (index >= 0) queuedMutations.splice(index, 1);
      if (conflicted) context.onConflict?.();
      else context.onCommitted?.();
    });
    mutationChain = operation;
    void operation.catch((error: unknown) => {
      if (operationGeneration !== queueGeneration) return;
      failed = true;
      context.setLoadError(error instanceof Error ? error.message : String(error));
    });
    return operation;
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
    if (!failed) return flushPendingBlockSaves();
    failed = false;
    mutationChain = Promise.resolve();
    context.setLoadError(null);
    for (const mutation of [...queuedMutations]) void runQueued(mutation).catch(() => undefined);
    return flushPendingBlockSaves();
  }

  function flushPendingBlockSaves(): Promise<void> {
    for (const blockId of [...pendingBlockSaves.keys()]) {
      void flushBlockSave(blockId).catch(() => undefined);
    }
    return mutationChain;
  }

  /** Abandon local writes only after their selected page is confirmed inactive. */
  function discardPendingEditorWrites(): void {
    queueGeneration += 1;
    for (const pending of pendingBlockSaves.values()) clearTimeout(pending.timer);
    pendingBlockSaves.clear();
    queuedMutations.length = 0;
    dirtyBlocks.clear();
    blockRevisions.clear();
    canonicalRevisions.clear();
    canonicalContentRevisions.clear();
    baseContents.clear();
    for (const blockId of heldConflicts.keys()) context.onBlockConflict?.(blockId, null);
    heldConflicts.clear();
    saveChains.clear();
    mutationChain = Promise.resolve();
    failed = false;
    context.setLoadError(null);
  }

  /** Captures pending typing before a structural edit enters the same write queue. */
  function enqueueEditorMutation(mutation: () => Promise<void>): Promise<void> {
    void flushPendingBlockSaves().catch(() => undefined);
    const revisions = [...dirtyBlocks].map((id) => [id, blockRevisions.get(id)] as const);
    return enqueue(async () => {
      try {
        await mutation();
      } catch (error) {
        if (isNotesEditConflict(error)) for (const [id] of revisions) abandonConflictedBlock(id);
        throw error;
      }
      for (const [id, revision] of revisions) {
        if (blockRevisions.get(id) === revision) dirtyBlocks.delete(id);
      }
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
