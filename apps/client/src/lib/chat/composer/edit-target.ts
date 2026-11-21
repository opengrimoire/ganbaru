import type { ChatAssignmentInputRead, ChatScheduledMessageRead } from "$lib/chat/contracts";

/**
 * What the composer editor holds: the destination draft, a waiting reply being edited, or a scheduled message
 * being edited. The draft stays in the store while either edit runs, so ending an edit restores it. Only one edit
 * runs at a time.
 */
export type ComposerEditTarget =
  | { phase: "draft" }
  /** A waiting reply; an edit replaces only its text. */
  | { phase: "waitingReply"; input: ChatAssignmentInputRead }
  /** A scheduled message, with the share choice of the draft it set aside. */
  | { phase: "scheduled"; message: ChatScheduledMessageRead; draftAlsoSendToChannel: boolean };

export type ComposerEditEvent =
  | { type: "replyEditStarted"; input: ChatAssignmentInputRead }
  | { type: "scheduledEditStarted"; message: ChatScheduledMessageRead; draftAlsoSendToChannel: boolean }
  /** The edit was saved, discarded, or abandoned; the draft comes back. */
  | { type: "editEnded" };

export const DRAFT_EDIT_TARGET: ComposerEditTarget = { phase: "draft" };

/**
 * Apply one edit event. An edit starts only from the draft, so starting another while one runs returns the same
 * state object.
 */
export function transitionComposerEditTarget(
  state: ComposerEditTarget,
  event: ComposerEditEvent,
): ComposerEditTarget {
  switch (event.type) {
    case "replyEditStarted":
      return state.phase === "draft" ? { phase: "waitingReply", input: event.input } : state;
    case "scheduledEditStarted":
      return state.phase === "draft"
        ? { phase: "scheduled", message: event.message, draftAlsoSendToChannel: event.draftAlsoSendToChannel }
        : state;
    case "editEnded":
      return state.phase === "draft" ? state : DRAFT_EDIT_TARGET;
  }
}

/** The waiting reply the editor holds, or null. */
export function editedReply(state: ComposerEditTarget): ChatAssignmentInputRead | null {
  return state.phase === "waitingReply" ? state.input : null;
}

/** The scheduled message edit the editor holds, or null. */
export function editedScheduled(
  state: ComposerEditTarget,
): Extract<ComposerEditTarget, { phase: "scheduled" }> | null {
  return state.phase === "scheduled" ? state : null;
}

/** What submitting the composer does for each edit target. */
export type ComposerSubmitAction = "saveReply" | "saveScheduled" | "send";

/**
 * What a submit does and whether it first waits for the mentioned agent's assignment preview. A send and a
 * scheduled message save both check agent blockers that stay hidden while the preview loads, so both wait; a
 * waiting reply edit replaces only text and checks no agent blockers.
 */
export function composerSubmitPlan(state: ComposerEditTarget): { action: ComposerSubmitAction; awaitsPreview: boolean } {
  switch (state.phase) {
    case "waitingReply":
      return { action: "saveReply", awaitsPreview: false };
    case "scheduled":
      return { action: "saveScheduled", awaitsPreview: true };
    case "draft":
      return { action: "send", awaitsPreview: true };
  }
}
