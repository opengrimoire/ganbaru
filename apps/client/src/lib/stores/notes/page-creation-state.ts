import type { NotesLoadedPage, NotesPageCreate } from "$lib/notes/types";

/**
 * A new page that is not yet usable. A page whose create request failed may be retried or discarded; a page the
 * backend created whose local reconcile failed exists, so its retry only reconciles again and it is never discarded.
 */
export type NotesPageCreation =
  | { phase: "creating"; request: NotesPageCreate }
  | { phase: "create-failed"; request: NotesPageCreate; error: string }
  | { phase: "reconciling"; request: NotesPageCreate; loaded: NotesLoadedPage }
  | { phase: "reconcile-failed"; request: NotesPageCreate; loaded: NotesLoadedPage; error: string };

export type NotesPageCreationEvent =
  | { type: "create-started" }
  | { type: "create-failed"; error: string }
  | { type: "created"; loaded: NotesLoadedPage }
  | { type: "reconcile-started" }
  | { type: "reconcile-failed"; error: string };

/** The first creation state for `request`. */
export function notesPageCreationStarted(request: NotesPageCreate): NotesPageCreation {
  return { phase: "creating", request };
}

/** Apply one creation event; an event that does not fit the current phase leaves it unchanged. */
export function transitionPageCreation(state: NotesPageCreation, event: NotesPageCreationEvent): NotesPageCreation {
  switch (event.type) {
    case "create-started":
      return state.phase === "create-failed" ? { phase: "creating", request: state.request } : state;
    case "create-failed":
      return state.phase === "creating" ? { phase: "create-failed", request: state.request, error: event.error } : state;
    case "created":
      return state.phase === "creating" ? { phase: "reconciling", request: state.request, loaded: event.loaded } : state;
    case "reconcile-started":
      return state.phase === "reconcile-failed"
        ? { phase: "reconciling", request: state.request, loaded: state.loaded }
        : state;
    case "reconcile-failed":
      return state.phase === "reconciling"
        ? { phase: "reconcile-failed", request: state.request, loaded: state.loaded, error: event.error }
        : state;
  }
}

/** The visible failure of a creation, if any. */
export function notesPageCreationError(state: NotesPageCreation | undefined): string | null {
  return state?.phase === "create-failed" || state?.phase === "reconcile-failed" ? state.error : null;
}

/** Whether the backend holds the page, so it must not be discarded as a failed draft. */
export function notesPageCreationSaved(state: NotesPageCreation | undefined): boolean {
  return state?.phase === "reconciling" || state?.phase === "reconcile-failed";
}
