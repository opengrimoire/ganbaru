/**
 * The Notes workspace shell load. `hasShell` keeps a reload or a failed reload from hiding the shell already shown.
 */
export type NotesShellLoad =
  | { phase: "unloaded" }
  | { phase: "loading"; hasShell: boolean }
  | { phase: "loaded" }
  | { phase: "failed"; hasShell: boolean; error: string };

export type NotesShellLoadEvent =
  | { type: "started" }
  | { type: "applied" }
  | { type: "failed"; error: string }
  /** A newer request replaced the load in flight. */
  | { type: "superseded" };

export const UNLOADED_SHELL: NotesShellLoad = { phase: "unloaded" };

/** Whether a shell has been applied, whatever the latest load did. */
export function notesShellApplied(state: NotesShellLoad): boolean {
  if (state.phase === "loaded") return true;
  return state.phase !== "unloaded" && state.hasShell;
}

/** Apply one shell load event; events that do not apply return the same state object. */
export function transitionShellLoad(state: NotesShellLoad, event: NotesShellLoadEvent): NotesShellLoad {
  switch (event.type) {
    case "started":
      return { phase: "loading", hasShell: notesShellApplied(state) };
    case "applied":
      return state.phase === "loaded" ? state : { phase: "loaded" };
    case "failed":
      return { phase: "failed", hasShell: notesShellApplied(state), error: event.error };
    case "superseded":
      if (state.phase !== "loading") return state;
      return state.hasShell ? { phase: "loaded" } : UNLOADED_SHELL;
  }
}

/**
 * The selected page's load. A failure belongs to its page, covering the page tree, its outlines, history, and undo, so
 * it never shows on another page and a later successful load of the same page clears it.
 */
export type NotesPageLoad =
  | { phase: "idle" }
  | { phase: "loading"; pageId: string }
  | { phase: "ready"; pageId: string }
  | { phase: "failed"; pageId: string; error: string };

export type NotesPageLoadEvent =
  | { type: "started"; pageId: string }
  /** The page's tree was applied, so any earlier failure of that page is resolved. */
  | { type: "loaded"; pageId: string }
  /** The selection request ended without its own result, as when a newer request applied the page. */
  | { type: "settled"; pageId: string }
  | { type: "failed"; pageId: string; error: string }
  /** No page is selected. */
  | { type: "cleared" };

export const IDLE_PAGE_LOAD: NotesPageLoad = { phase: "idle" };

/** Apply one page load event; events that do not apply return the same state object. */
export function transitionPageLoad(state: NotesPageLoad, event: NotesPageLoadEvent): NotesPageLoad {
  switch (event.type) {
    case "started":
      return { phase: "loading", pageId: event.pageId };
    case "loaded":
      return state.phase === "ready" && state.pageId === event.pageId ? state : { phase: "ready", pageId: event.pageId };
    case "settled":
      return state.phase === "loading" && state.pageId === event.pageId ? { phase: "ready", pageId: event.pageId } : state;
    case "failed":
      return { phase: "failed", pageId: event.pageId, error: event.error };
    case "cleared":
      return state.phase === "idle" ? state : IDLE_PAGE_LOAD;
  }
}

/** The error shown for the workspace: a failed shell load first, then a failure of the selected page. */
export function notesWorkspaceError(shell: NotesShellLoad, page: NotesPageLoad, selectedPageId: string | null): string | null {
  if (shell.phase === "failed") return shell.error;
  return page.phase === "failed" && page.pageId === selectedPageId ? page.error : null;
}

/** Whether the shell or the selected page is loading. */
export function notesWorkspaceLoading(shell: NotesShellLoad, page: NotesPageLoad): boolean {
  return shell.phase === "loading" || page.phase === "loading";
}
