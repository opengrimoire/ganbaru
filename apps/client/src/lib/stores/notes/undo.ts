import { notesUndoSnapshotOperations } from "$lib/notes/history/undo-operations";
import { createNotesCompoundPersistence, notesEditReferences } from "./compound-edits";
import type { NotesCompoundEditResult } from "$lib/api/notes/compound-edits";
import { reconcileNotesUndoSnapshot } from "$lib/notes/history/undo-compound";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";
import {
  clearNotesUndoState,
  loadNotesUndoState,
  saveNotesUndoState,
} from "$lib/api/notes";
import type { NotesTreeState } from "$lib/notes/blocks/tree";
import type { NotesDocumentSelection, NotesTextSelection } from "$lib/notes/editor/selection";
import {
  createNotesUndoSnapshot,
  createNotesUndoSnapshotForBlocks,
  parseNotesUndoStateJson,
  recordNotesUndoEntry,
  serializeNotesUndoState,
  trimNotesUndoStateToByteLimit,
  type NotesUndoEntry,
  type NotesUndoRecordOptions,
  type NotesUndoSnapshot,
  type NotesUndoState,
} from "$lib/notes/history/undo-history";
import type { NotesBlock, NotesChildDatabaseBlock } from "$lib/notes/types";

const EMPTY_UNDO_STATE: NotesUndoState = { undo: [], redo: [] };

export interface NotesUndoControllerContext {
  loadUndoReferences?: (pageId: string, blockIds: readonly string[]) => Promise<NotesBlock[]>;
  readCanonicalRevision?: (id: string) => string | undefined;
  acknowledgeCanonicalBlocks?: (blocks: readonly NotesBlock[]) => void;
  applyPostMutation: (result: NotesPostMutationResult) => void;
  enqueueEditorMutation: (mutation: () => Promise<void>) => Promise<void>;
  readSelectedPageId: () => string | null;
  readTreeState: () => NotesTreeState;
  loadPageTreeForUndo: (pageId: string) => Promise<void>;
  requestBlockFocus: (
    blockId: string | null,
    selection?: NotesTextSelection | null,
  ) => void;
  restoreDocumentSelection?: (pageId: string, selection: NotesDocumentSelection | null) => void;
  flushPendingMutations: () => Promise<void>;
  applyLocalSnapshot: (target: NotesUndoSnapshot, source: NotesUndoSnapshot) => void;
}

export interface NotesUndoController {
  persist: () => Promise<void>;
  dispose: () => void;
  reset: (pageId: string | null) => void;
  hydrate: (pageId: string | null) => Promise<void>;
  snapshot: (
    focusBlockId: string | null,
    extraBlocks?: readonly NotesBlock[],
    focusSelection?: NotesTextSelection | null,
  ) => NotesUndoSnapshot | null;
  snapshotBlocks: (
    focusBlockId: string | null,
    blockIds: readonly string[],
    extraBlocks?: readonly NotesBlock[],
    focusSelection?: NotesTextSelection | null,
  ) => NotesUndoSnapshot | null;
  record: (options: Omit<NotesUndoRecordOptions, "id">) => void;
  reconcileDatabaseIdentity: (block: NotesChildDatabaseBlock) => void;
  reconcileCanonicalBlocks: (blocks: readonly NotesBlock[]) => void;
  reconcileCompoundUndo: (result: NotesCompoundEditResult) => void;
  undo: () => Promise<boolean>;
  redo: () => Promise<boolean>;
  canUndo: () => boolean;
  canRedo: () => boolean;
}

function stackWithLimit(entries: readonly NotesUndoEntry[]): NotesUndoEntry[] {
  return entries.slice(-40);
}

/**
 * Create per-page Notes undo and redo history with SQLite-backed recovery state.
 */
export function createNotesUndoController(
  context: NotesUndoControllerContext,
): NotesUndoController {
  let state: NotesUndoState = EMPTY_UNDO_STATE;
  let hydratedPageId: string | null = null;
  let hydrateRequestId = 0;
  let persistTimer: ReturnType<typeof setTimeout> | null = null;
  let mutationChain = Promise.resolve();
  const pendingEntries = new Map<NotesUndoEntry, number>();

  function retainedEntries(): NotesUndoEntry[] {
    return [...new Set([...state.undo, ...state.redo, ...pendingEntries.keys()])];
  }

  function persistPageId(): string | null {
    return state.undo.at(-1)?.after.pageId
      ?? state.redo.at(-1)?.before.pageId
      ?? hydratedPageId;
  }

  async function persistNow(): Promise<void> {
    if (persistTimer) {
      clearTimeout(persistTimer);
      persistTimer = null;
    }
    const pageId = persistPageId();
    if (!pageId) return;
    state = trimNotesUndoStateToByteLimit(state);
    try {
      if (state.undo.length === 0 && state.redo.length === 0) {
        await clearNotesUndoState(pageId);
        return;
      }
      await saveNotesUndoState(pageId, serializeNotesUndoState(state));
    } catch (error) {
      console.warn("persist notes undo recovery state failed", error);
    }
  }

  function schedulePersist(): void {
    if (persistTimer) clearTimeout(persistTimer);
    persistTimer = setTimeout(() => {
      void context.flushPendingMutations().then(persistNow)
        .catch((error: unknown) => console.warn("Notes history is waiting for unsaved edits", error));
    }, 250);
  }

  async function hydrate(pageId: string | null): Promise<void> {
    const requestId = ++hydrateRequestId;
    hydratedPageId = pageId;
    if (!pageId) {
      state = EMPTY_UNDO_STATE;
      return;
    }
    try {
      const stateJson = await loadNotesUndoState(pageId);
      if (requestId !== hydrateRequestId) return;
      state = stateJson
        ? trimNotesUndoStateToByteLimit(parseNotesUndoStateJson(stateJson))
        : EMPTY_UNDO_STATE;
    } catch (error) {
      if (requestId !== hydrateRequestId) return;
      state = EMPTY_UNDO_STATE;
      await clearNotesUndoState(pageId).catch(() => undefined);
      console.warn("hydrate notes undo recovery state failed", error);
    }
  }

  function reset(pageId: string | null): void {
    hydrateRequestId += 1;
    hydratedPageId = pageId;
    state = EMPTY_UNDO_STATE;
    pendingEntries.clear();
  }

  function snapshot(
    focusBlockId: string | null,
    extraBlocks: readonly NotesBlock[] = [],
    focusSelection: NotesTextSelection | null = null,
  ): NotesUndoSnapshot | null {
    return createNotesUndoSnapshot(
      context.readSelectedPageId(),
      context.readTreeState(),
      focusBlockId,
      extraBlocks,
      focusSelection,
    );
  }

  function snapshotBlocks(
    focusBlockId: string | null,
    blockIds: readonly string[],
    extraBlocks: readonly NotesBlock[] = [],
    focusSelection: NotesTextSelection | null = null,
  ): NotesUndoSnapshot | null {
    return createNotesUndoSnapshotForBlocks(
      context.readSelectedPageId(),
      context.readTreeState(),
      focusBlockId,
      blockIds,
      extraBlocks,
      focusSelection,
    );
  }

  function record(options: Omit<NotesUndoRecordOptions, "id">): void {
    hydrateRequestId += 1;
    state = recordNotesUndoEntry(state, {
      ...options,
      id: crypto.randomUUID(),
    });
    schedulePersist();
  }

  function applyEntry(entry: NotesUndoEntry, direction: "undo" | "redo"): void {
    const target = direction === "undo" ? entry.before : entry.after;
    const source = direction === "undo" ? entry.after : entry.before;
    const operations = notesUndoSnapshotOperations(target, source);
    const anchorRevisions = new Map<string, string>();
    const expectedRevision = (id: string) => source.blocks.find((block) => block.id === id)?.edit_revision
      ?? target.blocks.find((block) => block.id === id)?.edit_revision
      ?? anchorRevisions.get(id)
      ?? context.readCanonicalRevision?.(id)
      ?? context.readTreeState().blocksById[id]?.edit_revision;
    const persist = operations.length === 0 ? async () => undefined : createNotesCompoundPersistence({
      readSelectedPageId: context.readSelectedPageId,
      blockById: (id) => context.readTreeState().blocksById[id],
      readBlocksById: () => context.readTreeState().blocksById,
      applyPostMutation: context.applyPostMutation,
      readCanonicalRevision: expectedRevision,
      reconcileCanonicalBlocks,
    }, direction, operations, [...target.blocks, ...source.blocks], () => notesUndoSnapshotOperations(target, source));
    context.applyLocalSnapshot(target, source);
    context.requestBlockFocus(target.focusBlockId, target.focusSelection);
    context.restoreDocumentSelection?.(target.pageId, target.documentSelection ?? null);
    pendingEntries.set(entry, (pendingEntries.get(entry) ?? 0) + 1);
    const persistence = context.enqueueEditorMutation(async () => {
      const missing = notesEditReferences(notesUndoSnapshotOperations(target, source)).filter((id) => !expectedRevision(id));
      if (missing.length && context.loadUndoReferences) {
        for (const block of await context.loadUndoReferences(target.pageId, missing)) {
          if (block.edit_revision) anchorRevisions.set(block.id, block.edit_revision);
        }
      }
      await persist();
      const remaining = (pendingEntries.get(entry) ?? 1) - 1;
      if (remaining > 0) pendingEntries.set(entry, remaining);
      else pendingEntries.delete(entry);
    });
    mutationChain = persistence;
    void persistence
      .then(() => mutationChain === persistence ? persistNow() : undefined)
      .catch((error: unknown) => {
        console.warn(`notes ${direction} persistence failed`, error);
      });
  }

  /** Refresh only concurrency tokens, retaining each historical payload for undo. */
  function reconcileCanonicalBlocks(blocks: readonly NotesBlock[]): void {
    context.acknowledgeCanonicalBlocks?.(blocks);
    const revisions = new Map(blocks.map((block) => [block.id, block.edit_revision]));
    for (const entry of retainedEntries()) {
      for (const snapshot of [entry.before, entry.after]) {
        for (const block of snapshot.blocks) {
          const revision = revisions.get(block.id);
          if (revision) block.edit_revision = revision;
        }
      }
    }
    schedulePersist();
  }

  /** Complete only the historical boundaries associated with this committed native edit. */
  function reconcileCompoundUndo(result: NotesCompoundEditResult): void {
    const revisions = new Map(result.blocks.map((block) => [block.id, block.edit_revision]));
    const readRevision = (id: string) => revisions.get(id);
    for (const entry of retainedEntries()) {
      if (entry.before.nativeEditId === result.operation_id) reconcileNotesUndoSnapshot(entry.before, result.before_blocks ?? [], result.before_placements ?? [], readRevision);
      if (entry.after.nativeEditId === result.operation_id) reconcileNotesUndoSnapshot(entry.after, result.blocks, result.placements, readRevision);
    }
    schedulePersist();
  }

  /** Attach canonical database identities without changing historical titles. */
  function reconcileDatabaseIdentity(database: NotesChildDatabaseBlock): void {
    const reconcile = (snapshot: NotesUndoSnapshot): void => {
      snapshot.blocks = snapshot.blocks.map((block) => block.id === database.id && block.type === "child_database"
        ? { ...block, child_database: { ...block.child_database,
          database_id: database.child_database.database_id,
          data_source_id: database.child_database.data_source_id,
          view_id: database.child_database.view_id,
        } } : block);
    };
    // Queued undo writes reference these internal snapshots until persistence finishes.
    for (const entry of retainedEntries()) {
      reconcile(entry.before);
      reconcile(entry.after);
    }
    schedulePersist();
  }

  async function undo(): Promise<boolean> {
    const entry = state.undo.at(-1);
    if (!entry) return false;
    state = {
      undo: state.undo.slice(0, -1),
      redo: stackWithLimit([...state.redo, entry]),
    };
    applyEntry(entry, "undo");
    return true;
  }

  async function redo(): Promise<boolean> {
    const entry = state.redo.at(-1);
    if (!entry) return false;
    state = {
      undo: stackWithLimit([...state.undo, entry]),
      redo: state.redo.slice(0, -1),
    };
    applyEntry(entry, "redo");
    return true;
  }

  return {
    persist: persistNow,
    dispose: () => {
      if (persistTimer) clearTimeout(persistTimer);
      persistTimer = null;
      hydrateRequestId += 1;
      pendingEntries.clear();
    },
    reset,
    hydrate,
    snapshot,
    snapshotBlocks,
    record,
    reconcileDatabaseIdentity,
    reconcileCanonicalBlocks,
    reconcileCompoundUndo,
    undo,
    redo,
    canUndo: () => state.undo.length > 0,
    canRedo: () => state.redo.length > 0,
  };
}
