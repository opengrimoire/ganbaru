import type { NotesDatabaseReference } from "$lib/notes/types";

/** The paste choice shown for a pasted database copy or a pasted database link. */
export type NotesDatabasePastePrompt = {
  kind: "copy";
  pageId: string;
  blockId: string;
  copies: Readonly<Record<string, string>>;
} | {
  kind: "link";
  pageId: string;
  blockId: string;
  start: number;
  end: number;
  text: string;
  url: string;
  reference: NotesDatabaseReference;
};

/**
 * The database paste prompt and its chosen action. Each prompt has a generation so a queued write only closes the
 * prompt it was started from, and the writing variant holds a token so a late end of an earlier attempt is ignored.
 * `started` marks a choice whose writes were queued: it cannot be chosen again, only retried through the queue.
 */
export type NotesDatabasePasteState =
  | { phase: "idle" }
  | { phase: "offered"; prompt: NotesDatabasePastePrompt; generation: number }
  | { phase: "writing"; prompt: NotesDatabasePastePrompt; generation: number; token: number; started: boolean }
  | { phase: "failed"; prompt: NotesDatabasePastePrompt; generation: number; error: string; started: boolean };

export type NotesDatabasePasteEvent =
  | { type: "offered"; prompt: NotesDatabasePastePrompt; generation: number }
  | { type: "write-started"; token: number }
  /** The choice queued its writes, so it can no longer be changed. */
  | { type: "enqueued"; token: number }
  /** The prompt now follows the block that replaced its pasted text. */
  | { type: "retargeted"; generation: number; blockId: string }
  | { type: "write-failed"; token: number; error: string }
  | { type: "write-settled"; token: number }
  /** The chosen action finished and closes its own prompt. */
  | { type: "completed"; generation: number }
  /** Closed by the user or the editor; queued writes continue and report failures through the editor's save banner. */
  | { type: "dismissed" };

export const IDLE_DATABASE_PASTE: NotesDatabasePasteState = { phase: "idle" };

/** Apply one paste event; an event for another prompt or attempt leaves the state unchanged. */
export function transitionDatabasePaste(
  state: NotesDatabasePasteState,
  event: NotesDatabasePasteEvent,
): NotesDatabasePasteState {
  switch (event.type) {
    case "offered":
      return { phase: "offered", prompt: event.prompt, generation: event.generation };
    case "write-started":
      if (state.phase === "offered") return { ...state, phase: "writing", token: event.token, started: false };
      if (state.phase === "failed") {
        return { phase: "writing", prompt: state.prompt, generation: state.generation, token: event.token, started: state.started };
      }
      return state;
    case "enqueued":
      return isCurrentWrite(state, event.token) && !state.started ? { ...state, started: true } : state;
    case "retargeted":
      if (state.phase === "idle" || state.generation !== event.generation) return state;
      return { ...state, prompt: { ...state.prompt, blockId: event.blockId } };
    case "write-failed":
      if (!isCurrentWrite(state, event.token)) return state;
      return { phase: "failed", prompt: state.prompt, generation: state.generation, error: event.error, started: state.started };
    case "write-settled":
      if (!isCurrentWrite(state, event.token)) return state;
      // Queued writes are owned by the editor queue once they settle, so a started prompt has nothing left to offer.
      return state.started ? IDLE_DATABASE_PASTE : { phase: "offered", prompt: state.prompt, generation: state.generation };
    case "completed":
      return state.phase !== "idle" && state.generation === event.generation ? IDLE_DATABASE_PASTE : state;
    case "dismissed":
      return state.phase === "idle" ? state : IDLE_DATABASE_PASTE;
  }
}

function isCurrentWrite(
  state: NotesDatabasePasteState,
  token: number,
): state is Extract<NotesDatabasePasteState, { phase: "writing" }> {
  return state.phase === "writing" && state.token === token;
}
