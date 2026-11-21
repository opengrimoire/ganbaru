import type { ChatScratchDirectoryPageRead, ChatScratchGenerationId } from "$lib/chat/contracts";

/** A scratch listing read in flight, identified by its request number so an older result never lands. */
export interface ScratchDirectoryLoad {
  phase: "loading";
  request: number;
  generationId: ChatScratchGenerationId;
  relativePath: string;
  /** The cursor of the page being read, or `null` for the first page of a folder. */
  cursor: string | null;
}

/**
 * The scratch folder listing of the manager. The shown page names the generation it was read from, so it is only
 * visible, and its entries only promotable, while that generation is selected.
 */
export interface ScratchDirectoryState {
  page: ChatScratchDirectoryPageRead | null;
  load: { phase: "idle" } | ScratchDirectoryLoad;
}

export type ScratchDirectoryEvent =
  /** A listing read started; it supersedes any read still in flight. */
  | {
      type: "requested";
      request: number;
      generationId: ChatScratchGenerationId;
      relativePath: string;
      cursor: string | null;
    }
  /** The read numbered `request` returned a page. */
  | { type: "loaded"; request: number; page: ChatScratchDirectoryPageRead }
  /** The read numbered `request` failed; the shown page stays. */
  | { type: "failed"; request: number }
  /** The selected scope changed or was reloaded, so the shown page and any read in flight no longer apply. */
  | { type: "cleared" };

export const IDLE_SCRATCH_DIRECTORY: ScratchDirectoryState = { page: null, load: { phase: "idle" } };

/** Whether `request` is the read in flight. */
function isCurrentRequest(state: ScratchDirectoryState, request: number): state is ScratchDirectoryState & {
  load: ScratchDirectoryLoad;
} {
  return state.load.phase === "loading" && state.load.request === request;
}

/** Joins a cursor page onto the shown page when both list the same folder of the same generation. */
function mergedPage(
  shown: ChatScratchDirectoryPageRead | null, load: ScratchDirectoryLoad, page: ChatScratchDirectoryPageRead,
): ChatScratchDirectoryPageRead {
  const continues = load.cursor !== null
    && shown !== null
    && shown.scratchGenerationId === load.generationId
    && shown.relativePath === load.relativePath;
  return continues ? { ...page, entries: [...shown.entries, ...page.entries] } : page;
}

/**
 * Apply one listing event. Results of a read that is no longer in flight return the same state object, so callers
 * can tell a dropped result by identity.
 */
export function transitionScratchDirectory(
  state: ScratchDirectoryState, event: ScratchDirectoryEvent,
): ScratchDirectoryState {
  switch (event.type) {
    case "requested":
      return {
        page: state.page,
        load: {
          phase: "loading",
          request: event.request,
          generationId: event.generationId,
          relativePath: event.relativePath,
          cursor: event.cursor,
        },
      };
    case "loaded":
      if (!isCurrentRequest(state, event.request)) return state;
      return { page: mergedPage(state.page, state.load, event.page), load: { phase: "idle" } };
    case "failed":
      return isCurrentRequest(state, event.request) ? { page: state.page, load: { phase: "idle" } } : state;
    case "cleared":
      return state.page === null && state.load.phase === "idle" ? state : IDLE_SCRATCH_DIRECTORY;
  }
}

/** The shown page when it belongs to the selected generation, otherwise `null`. */
export function visibleScratchDirectory(
  state: ScratchDirectoryState, generationId: ChatScratchGenerationId | null,
): ChatScratchDirectoryPageRead | null {
  return state.page && state.page.scratchGenerationId === generationId ? state.page : null;
}
