/**
 * The save lifecycle of the bound Chat draft as one state, so loading, unsaved edits, an in-flight save, a replacement
 * by another window, and a failure can never combine in ways the composer cannot explain.
 */
export type ChatDraftLifecycle =
  /** The draft is being read; a save of the previous binding may still report an error. */
  | { kind: "loading"; error: string | null }
  /** Nothing is waiting to be stored. */
  | { kind: "clean"; changedElsewhere: boolean; error: string | null }
  /** Edits are waiting for the next save. */
  | { kind: "edited"; error: string | null }
  /** A save is running and the edits stay unsaved until it lands. */
  | { kind: "saving" };

/** Something that happened to the bound draft. */
export type ChatDraftLifecycleEvent =
  | { kind: "bindStarted" }
  | { kind: "loaded"; dirty: boolean }
  | { kind: "loadFailed"; error: string }
  | { kind: "changed" }
  | { kind: "saveStarted" }
  | { kind: "saveLanded" }
  | { kind: "failed"; error: string }
  | { kind: "adopted"; changedElsewhere: boolean }
  | { kind: "emptied" }
  | { kind: "noticeDismissed" };

/** The composer flags a lifecycle state presents. */
export interface ChatDraftLifecycleFlags {
  loading: boolean;
  saving: boolean;
  dirty: boolean;
  changedElsewhere: boolean;
  error: string | null;
}

/** The state of a composer with no draft bound, or one whose draft was just cleared or stored. */
export const CLEAN_DRAFT: ChatDraftLifecycle = { kind: "clean", changedElsewhere: false, error: null };

/**
 * Applies `event` to `state`.
 *
 * Edits are refused while loading, a landed save of the current edits makes the draft clean, and a failure keeps any
 * unsaved edits so the next save can still store them.
 */
export function nextDraftLifecycle(state: ChatDraftLifecycle, event: ChatDraftLifecycleEvent): ChatDraftLifecycle {
  switch (event.kind) {
    case "bindStarted":
      return { kind: "loading", error: null };
    case "loaded":
      return event.dirty ? { kind: "edited", error: null } : CLEAN_DRAFT;
    case "loadFailed":
      return { kind: "clean", changedElsewhere: false, error: event.error };
    case "changed":
      if (state.kind === "loading" || state.kind === "saving") return state;
      return { kind: "edited", error: null };
    case "saveStarted":
      return state.kind === "edited" || state.kind === "saving" ? { kind: "saving" } : state;
    case "saveLanded":
      if (state.kind === "saving") return CLEAN_DRAFT;
      if (state.kind === "edited") return { kind: "clean", changedElsewhere: false, error: state.error };
      return state;
    case "failed":
      if (state.kind === "saving") return { kind: "edited", error: event.error };
      return { ...state, error: event.error };
    case "adopted":
      return { kind: "clean", changedElsewhere: event.changedElsewhere, error: null };
    case "emptied":
      return CLEAN_DRAFT;
    case "noticeDismissed":
      return state.kind === "clean" ? { ...state, changedElsewhere: false } : state;
  }
}

/** Whether `state` holds edits that still need a save. */
export function draftHasUnsavedEdits(state: ChatDraftLifecycle): boolean {
  return state.kind === "edited" || state.kind === "saving";
}

/** The composer flags `state` presents. */
export function draftLifecycleFlags(state: ChatDraftLifecycle): ChatDraftLifecycleFlags {
  switch (state.kind) {
    case "loading":
      return { loading: true, saving: false, dirty: false, changedElsewhere: false, error: state.error };
    case "clean":
      return { loading: false, saving: false, dirty: false, changedElsewhere: state.changedElsewhere, error: state.error };
    case "edited":
      return { loading: false, saving: false, dirty: true, changedElsewhere: false, error: state.error };
    case "saving":
      return { loading: false, saving: true, dirty: true, changedElsewhere: false, error: null };
  }
}
