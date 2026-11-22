import type { NotesBlock, NotesBlockUpdate } from "$lib/notes/types";

/** An unsaved block edit that cannot be replayed on the block's canonical content without the user choosing. */
export interface NotesBlockEditConflict {
  blockId: string;
  /** The page the block was open on when its edit was held, so its canonical content is read from that page. */
  pageId: string | null;
  /** `text` when both sides changed the text differently; `removed` when the block was trashed or deleted elsewhere. */
  kind: "text" | "removed";
  /** The editor's unsaved content, including typing made while the edit was held. */
  mine: NotesBlockUpdate;
  /** The canonical block, or `null` when it was removed. */
  theirs: NotesBlock | null;
}

/** A debounced typing save waiting for its timer, with the edit revision it captures. */
export interface NotesScheduledBlockSave {
  update: NotesBlockUpdate;
  revision: number;
}

/** What every unsaved block knows, whichever phase holds it. */
interface NotesUnsavedBlock {
  /** The canonical content the unsaved edit started from, or `null` when the block was not loaded then. */
  base: NotesBlockUpdate | null;
  /** The edit sequence number of the block's latest local change; a save of an older one leaves it unsaved. */
  revision: number;
}

/**
 * The save lifecycle of one block. In-flight saves are entries of the shared write queue (a block can have several),
 * and a failed write pauses that queue, so neither is a block phase: a block stays `dirty` until a save of its latest
 * revision is acknowledged.
 */
export type NotesBlockSaveState =
  | { phase: "clean" }
  | (NotesUnsavedBlock & { phase: "dirty"; scheduled: NotesScheduledBlockSave | null })
  | (NotesUnsavedBlock & { phase: "held"; conflict: NotesBlockEditConflict });

export type NotesBlockSaveEvent =
  /** The user changed the block locally; `base` is its content before this change. */
  | { type: "edited"; base: NotesBlockUpdate | null; revision: number }
  /** A debounced save was scheduled. In a held block it becomes the held text instead. */
  | { type: "scheduled"; update: NotesBlockUpdate; revision: number }
  /** The scheduled save left its timer for the write queue. */
  | { type: "flushed" }
  /** A save of `revision` was acknowledged with the saved content. */
  | { type: "saved"; revision: number; saved: NotesBlockUpdate }
  /** A structural edit that captured the block at `revision` committed. */
  | { type: "committed"; revision: number }
  /** A refused save cannot be replayed without the user choosing. */
  | { type: "held"; conflict: NotesBlockEditConflict }
  /** A refused write without a replay path dropped the local edit, so the canonical reload replaces it. */
  | { type: "abandoned" }
  /** The user's choice for a held edit was saved. */
  | { type: "settled" };

export const CLEAN_BLOCK_SAVE: NotesBlockSaveState = { phase: "clean" };

/**
 * Apply one save event to a block. Events that do not apply to the current phase return the same state object, so
 * callers can detect an ignored move by identity.
 */
export function transitionBlockSave(state: NotesBlockSaveState, event: NotesBlockSaveEvent): NotesBlockSaveState {
  switch (event.type) {
    case "edited":
      if (state.phase === "clean") return { phase: "dirty", base: event.base, revision: event.revision, scheduled: null };
      return { ...state, revision: event.revision };
    case "scheduled": {
      const scheduled = { update: event.update, revision: event.revision };
      if (state.phase === "held") return { ...state, conflict: { ...state.conflict, mine: event.update } };
      if (state.phase === "clean") return { phase: "dirty", base: null, revision: event.revision, scheduled };
      return { ...state, scheduled };
    }
    case "flushed":
      return state.phase === "dirty" && state.scheduled ? { ...state, scheduled: null } : state;
    case "saved":
      if (state.phase !== "dirty") return state;
      return state.revision === event.revision ? CLEAN_BLOCK_SAVE : { ...state, base: event.saved };
    case "committed":
      return state.phase === "dirty" && state.revision === event.revision && !state.scheduled ? CLEAN_BLOCK_SAVE : state;
    case "held":
      if (state.phase === "clean") return { phase: "held", base: null, revision: 0, conflict: event.conflict };
      return { phase: "held", base: state.base, revision: state.revision, conflict: event.conflict };
    case "abandoned":
      return state.phase === "dirty" ? CLEAN_BLOCK_SAVE : state;
    case "settled":
      return state.phase === "held" ? CLEAN_BLOCK_SAVE : state;
  }
}

/** The shared write queue of one editor session. `generation` changes when its writes are discarded. */
export type NotesSaveQueueState =
  | { phase: "running"; generation: number }
  | { phase: "failed"; generation: number; error: string };

export type NotesSaveQueueEvent =
  /** A queued write of `generation` failed and pauses the writes after it. */
  | { type: "write-failed"; generation: number; error: string }
  /** The user retried the retained writes. */
  | { type: "retried" }
  /** The writes of the open page were abandoned once the page was confirmed inactive. */
  | { type: "discarded" };

export const INITIAL_SAVE_QUEUE: NotesSaveQueueState = { phase: "running", generation: 0 };

/** Apply one queue event; a failure reported for an earlier generation is ignored. */
export function transitionSaveQueue(state: NotesSaveQueueState, event: NotesSaveQueueEvent): NotesSaveQueueState {
  switch (event.type) {
    case "write-failed":
      if (event.generation !== state.generation) return state;
      return { phase: "failed", generation: state.generation, error: event.error };
    case "retried":
      return state.phase === "failed" ? { phase: "running", generation: state.generation } : state;
    case "discarded":
      return { phase: "running", generation: state.generation + 1 };
  }
}

/**
 * Editor notices that do not come from the write queue: a write refused because the note changed elsewhere, and a
 * failed reload of the canonical page after such a refusal or a settled held edit.
 */
export type NotesEditorRecovery =
  | { kind: "none" }
  | { kind: "changed-elsewhere" }
  | { kind: "reload-failed"; error: string };

export type NotesEditorRecoveryEvent =
  | { type: "write-refused" }
  | { type: "reload-failed"; error: string }
  | { type: "reloaded" }
  | { type: "dismissed" }
  | { type: "page-opened" };

export const NO_EDITOR_RECOVERY: NotesEditorRecovery = { kind: "none" };

/** Apply one recovery event. Dismissing only hides the informational notice; a failed reload stays until it succeeds. */
export function transitionEditorRecovery(state: NotesEditorRecovery, event: NotesEditorRecoveryEvent): NotesEditorRecovery {
  switch (event.type) {
    case "write-refused":
      return state.kind === "reload-failed" ? state : { kind: "changed-elsewhere" };
    case "reload-failed":
      return { kind: "reload-failed", error: event.error };
    case "reloaded":
      return state.kind === "reload-failed" ? NO_EDITOR_RECOVERY : state;
    case "dismissed":
      return state.kind === "changed-elsewhere" ? NO_EDITOR_RECOVERY : state;
    case "page-opened":
      return NO_EDITOR_RECOVERY;
  }
}

/** The one notice the editor shows above the page, with the action that clears it. */
export type NotesEditorBanner =
  | { kind: "none" }
  | { kind: "save-failed"; error: string }
  | { kind: "reload-failed"; error: string }
  | { kind: "changed-elsewhere" };

/**
 * A paused write queue, reported by its error, outranks a failed reload, which outranks the changed-elsewhere
 * notice.
 */
export function notesEditorBanner(saveError: string | null, recovery: NotesEditorRecovery): NotesEditorBanner {
  if (saveError !== null) return { kind: "save-failed", error: saveError };
  return recovery;
}
