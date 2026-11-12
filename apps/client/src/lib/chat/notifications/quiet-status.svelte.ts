import { readChatQuietState } from "$lib/api/chat";
import { createLifecycleScheduler } from "$lib/scheduling/lifecycle-scheduler";
import type { ChatQuietState } from "./gate";
import { chatQuietState } from "./quiet";

/** How long after a known quiet end the state is read again, so the read lands after the native boundary. */
export const CHAT_QUIET_END_RECHECK_MS = 1_000;
/** How long a failed quiet state read waits before it retries. */
export const CHAT_QUIET_ERROR_RETRY_MS = 60_000;
/** The window event Settings dispatches after it saves quiet periods. */
export const CHAT_QUIET_PERIODS_SAVED_EVENT = "ganbaru:chat-quiet-periods-saved";

/** Tells every quiet status reader in this window that quiet periods changed. */
export function announceChatQuietPeriodsSaved(): void {
  window.dispatchEvent(new CustomEvent(CHAT_QUIET_PERIODS_SAVED_EVENT));
}

/**
 * The cached native quiet state of this window, shared by the notification gate and background announcements.
 * It is read again when a user asks for it, when quiet periods are saved, and shortly after a known quiet end.
 */
class ChatQuietStatus {
  state = $state<ChatQuietState>({ quiet: false, endsAtMs: null });

  #listeners = new Set<() => void>();
  #users = 0;
  #scheduler = createLifecycleScheduler({
    errorRetryMs: CHAT_QUIET_ERROR_RETRY_MS,
    run: async (context) => {
      const next = chatQuietState(await readChatQuietState());
      if (!context.isCurrent()) return null;
      this.state = next;
      for (const listener of this.#listeners) listener();
      return next.endsAtMs === null ? null : next.endsAtMs + CHAT_QUIET_END_RECHECK_MS;
    },
    onError: (error) => {
      console.error("[chat quiet state] read failed:", error);
    },
  });

  #onSaved = (): void => {
    this.refresh();
  };

  /**
   * Starts reading the quiet state for one user and returns the release function. The listener runs after each
   * completed read.
   *
   * @param listener Called after every read that updates the state.
   */
  use(listener?: () => void): () => void {
    if (listener) this.#listeners.add(listener);
    this.#users += 1;
    if (this.#users === 1) {
      window.addEventListener(CHAT_QUIET_PERIODS_SAVED_EVENT, this.#onSaved);
      this.#scheduler.setEnabled(true);
    }
    let released = false;
    return () => {
      if (released) return;
      released = true;
      if (listener) this.#listeners.delete(listener);
      this.#users -= 1;
      if (this.#users > 0) return;
      window.removeEventListener(CHAT_QUIET_PERIODS_SAVED_EVENT, this.#onSaved);
      this.#scheduler.setEnabled(false);
    };
  }

  /** Reads the quiet state again; overlapping requests collapse into one queued read. */
  refresh(): void {
    this.#scheduler.invalidate();
  }
}

const chatQuietStatus = new ChatQuietStatus();

/** Returns the quiet status shared by this window. */
export function getChatQuietStatus(): ChatQuietStatus {
  return chatQuietStatus;
}
