import type { ChatInteractionStateRead, ChatThreadId } from "$lib/chat/contracts";

/**
 * The interaction state shown for the selected provider thread. A read in flight carries its request token and its
 * thread, so a late result of an earlier read or of another thread never replaces what is shown.
 */
export type InteractionLoad =
  | { phase: "none" }
  | {
      phase: "loading";
      threadId: ChatThreadId;
      request: number;
      /** What stays on screen during the read: the same thread's previous state, never another thread's. */
      shown: ChatInteractionStateRead | null;
    }
  | {
      phase: "shown";
      /** The thread the state was read for, or null when a caller set it directly. */
      threadId: ChatThreadId | null;
      interaction: ChatInteractionStateRead;
    };

export type InteractionLoadEvent =
  | { type: "started"; threadId: ChatThreadId; request: number }
  | { type: "loaded"; request: number; interaction: ChatInteractionStateRead }
  | { type: "failed"; request: number }
  /** The read landed after its thread left the selection, so nothing of that thread stays shown. */
  | { type: "abandoned"; request: number }
  /** A caller replaced the shown state; a read in flight still applies when it lands. */
  | { type: "replaced"; interaction: ChatInteractionStateRead | null }
  /** No thread is selected, which also abandons any read in flight. */
  | { type: "cleared" };

export const NO_INTERACTION: InteractionLoad = { phase: "none" };

/** The interaction state on screen, or null. */
export function shownInteraction(state: InteractionLoad): ChatInteractionStateRead | null {
  switch (state.phase) {
    case "none":
      return null;
    case "loading":
      return state.shown;
    case "shown":
      return state.interaction;
  }
}

/** The thread the shown or loading state belongs to, or null. */
function interactionThreadId(state: InteractionLoad): ChatThreadId | null {
  return state.phase === "none" ? null : state.threadId;
}

/** Apply one interaction load event; events that do not apply return the same state object. */
export function transitionInteractionLoad(state: InteractionLoad, event: InteractionLoadEvent): InteractionLoad {
  switch (event.type) {
    case "started":
      return {
        phase: "loading",
        threadId: event.threadId,
        request: event.request,
        shown: interactionThreadId(state) === event.threadId ? shownInteraction(state) : null,
      };
    case "loaded":
      if (state.phase !== "loading" || state.request !== event.request) return state;
      return { phase: "shown", threadId: state.threadId, interaction: event.interaction };
    case "failed":
      if (state.phase !== "loading" || state.request !== event.request) return state;
      return state.shown ? { phase: "shown", threadId: state.threadId, interaction: state.shown } : NO_INTERACTION;
    case "abandoned":
      if (state.phase !== "loading" || state.request !== event.request) return state;
      return NO_INTERACTION;
    case "replaced":
      if (state.phase === "loading") return { ...state, shown: event.interaction };
      return event.interaction
        ? { phase: "shown", threadId: interactionThreadId(state), interaction: event.interaction }
        : NO_INTERACTION;
    case "cleared":
      return state.phase === "none" ? state : NO_INTERACTION;
  }
}
