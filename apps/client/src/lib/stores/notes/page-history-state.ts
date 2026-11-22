/** A page history action that writes the open page. */
export type NotesPageHistoryActionKind = "restore" | "copy";

/**
 * The page history write in flight or its failure. The running variant carries the page it writes and a token, so a
 * result that arrives after the page changed or the state was reset is not applied.
 */
export type NotesPageHistoryAction =
  | { phase: "idle" }
  | { phase: "running"; kind: NotesPageHistoryActionKind; pageId: string; token: number }
  | { phase: "failed"; kind: NotesPageHistoryActionKind; pageId: string; error: string };

export type NotesPageHistoryActionEvent =
  | { type: "started"; kind: NotesPageHistoryActionKind; pageId: string; token: number }
  | { type: "finished"; token: number }
  | { type: "failed"; token: number; error: string }
  /** The open page changed or its history state was cleared. */
  | { type: "reset" };

export const IDLE_PAGE_HISTORY_ACTION: NotesPageHistoryAction = { phase: "idle" };

/**
 * Apply one action event. A start while another action runs is refused, so a double click sends one write, and the
 * end of a run whose token is no longer current is ignored.
 */
export function transitionPageHistoryAction(
  state: NotesPageHistoryAction,
  event: NotesPageHistoryActionEvent,
): NotesPageHistoryAction {
  switch (event.type) {
    case "started":
      if (state.phase === "running") return state;
      return { phase: "running", kind: event.kind, pageId: event.pageId, token: event.token };
    case "finished":
      return isCurrentPageHistoryAction(state, event.token) ? IDLE_PAGE_HISTORY_ACTION : state;
    case "failed":
      if (state.phase !== "running" || state.token !== event.token) return state;
      return { phase: "failed", kind: state.kind, pageId: state.pageId, error: event.error };
    case "reset":
      return state.phase === "idle" ? state : IDLE_PAGE_HISTORY_ACTION;
  }
}

/** Whether `token` names the action that is running now. */
export function isCurrentPageHistoryAction(state: NotesPageHistoryAction, token: number): boolean {
  return state.phase === "running" && state.token === token;
}
