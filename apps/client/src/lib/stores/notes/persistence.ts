import { updateNotesBlock } from "$lib/api/notes";
import { applyBlockUpdate } from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesBlockUpdate } from "$lib/notes/types";

interface PendingBlockSave {
  timer: ReturnType<typeof setTimeout>;
  update: NotesBlockUpdate;
  revision: number;
}

export interface NotesBlockPersistenceContext {
  reconcileCanonicalBlocks?: (blocks: readonly NotesBlock[]) => void;
  readBlock: (blockId: string) => NotesBlock | undefined;
  beforeSave: (blockId: string) => Promise<void>;
  replaceBlock: (block: NotesBlock) => void;
  setLoadError: (message: string | null) => void;
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
}

/**
 * Create debounced block persistence helpers for the Notes store.
 */
export function createNotesBlockPersistence(
  context: NotesBlockPersistenceContext,
): NotesBlockPersistence {
  const pendingBlockSaves = new Map<string, PendingBlockSave>();
  const canonicalRevisions = new Map<string, string>();
  const blockRevisions = new Map<string, number>();
  let mutationChain = Promise.resolve();
  let queueGeneration = 0;
  let failed = false;
  const queuedMutations: Array<() => Promise<void>> = [];
  const dirtyBlocks = new Set<string>();
  const saveChains = new Map<string, Promise<void>>();

  function readCanonicalRevision(blockId: string): string | undefined {
    return context.readBlock(blockId)?.edit_revision ?? canonicalRevisions.get(blockId);
  }

  function acknowledgeCanonicalBlocks(blocks: readonly NotesBlock[]): void {
    for (const block of blocks) if (block.edit_revision) canonicalRevisions.set(block.id, block.edit_revision);
  }

  function markBlockLocallyChanged(blockId: string): void {
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
    const save = enqueue(async () => {
      await context.beforeSave(blockId);
      if (saveGeneration !== queueGeneration) return;
      const saved = await updateNotesBlock(blockId, update);
      if (saveGeneration !== queueGeneration) return;
      acknowledgeCanonicalBlocks([saved]);
      context.reconcileCanonicalBlocks?.([saved]);
      if ((blockRevisions.get(blockId) ?? 0) === revision) {
        dirtyBlocks.delete(blockId);
        if (context.readBlock(blockId)) context.replaceBlock(saved);
      } else {
        const current = context.readBlock(blockId);
        if (current) context.replaceBlock({ ...current, edit_revision: saved.edit_revision });
      }
    });
    saveChains.set(blockId, save);
    try {
      await save;
    } finally {
      if (saveChains.get(blockId) === save) saveChains.delete(blockId);
    }
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
      await mutation();
      if (operationGeneration !== queueGeneration) return;
      const index = queuedMutations.indexOf(mutation);
      if (index >= 0) queuedMutations.splice(index, 1);
    });
    mutationChain = operation;
    void operation.catch((error: unknown) => {
      if (operationGeneration !== queueGeneration) return;
      failed = true;
      context.setLoadError(error instanceof Error ? error.message : String(error));
    });
    return operation;
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
      await mutation();
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
