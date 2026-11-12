import { readChatQuietState } from "$lib/api/chat";
import type { ChatQuietStateRead } from "$lib/chat/contracts";
import { createLifecycleScheduler, type LifecycleScheduler, type SchedulerClock } from "$lib/scheduling/lifecycle-scheduler";
import type { ChatQuietState } from "./gate";
import { chatQuietState } from "./quiet";

/** How long after a known quiet end the state is read again, so the read lands after the native boundary. */
export const CHAT_QUIET_END_RECHECK_MS = 1_000;
/**
 * How long a failed quiet state read waits before it retries. The state stays unknown, which holds notifications,
 * so the retry is short enough that a transient failure does not hold them for long.
 */
export const CHAT_QUIET_ERROR_RETRY_MS = 15_000;
/** The window event Settings dispatches after it saves quiet periods. */
export const CHAT_QUIET_PERIODS_SAVED_EVENT = "ganbaru:chat-quiet-periods-saved";

/** Tells every quiet status reader in this window that quiet periods changed. */
export function announceChatQuietPeriodsSaved(): void {
  window.dispatchEvent(new CustomEvent(CHAT_QUIET_PERIODS_SAVED_EVENT));
}

/** The native read and clock a quiet status depends on, injected so tests can drive it. */
export interface ChatQuietStatusDependencies {
  read(): Promise<ChatQuietStateRead>;
  clock?: SchedulerClock;
}

/**
 * The cached native quiet state of this window, shared by the notification gate and background announcements.
 * It is read again when a user asks for it, when quiet periods are saved, and shortly after a known quiet end.
 * The state is `null` (unknown) until the first read succeeds and again after a failed read, so callers fail
 * closed and hold delivery; a failed read retries after {@link CHAT_QUIET_ERROR_RETRY_MS}.
 */
export class ChatQuietStatus {
  state = $state<ChatQuietState | null>(null);

  #listeners = new Set<() => void>();
  #users = 0;
  #scheduler: LifecycleScheduler;

  constructor(dependencies: ChatQuietStatusDependencies) {
    this.#scheduler = createLifecycleScheduler({
      clock: dependencies.clock,
      errorRetryMs: CHAT_QUIET_ERROR_RETRY_MS,
      run: async (context) => {
        let read: ChatQuietStateRead;
        try {
          read = await dependencies.read();
        } catch (error: unknown) {
          if (context.isCurrent()) this.state = null;
          throw error;
        }
        const next = chatQuietState(read);
        if (!context.isCurrent()) return null;
        this.state = next;
        for (const listener of this.#listeners) listener();
        return next.endsAtMs === null ? null : next.endsAtMs + CHAT_QUIET_END_RECHECK_MS;
      },
      onError: (error) => {
        console.error("[chat quiet state] read failed:", error);
      },
    });
  }

  #onSaved = (): void => {
    this.refresh();
  };

  /**
   * Starts reading the quiet state for one user and returns the release function. The listener runs after each
   * successful read.
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

const chatQuietStatus = new ChatQuietStatus({ read: readChatQuietState });

/** Returns the quiet status shared by this window. */
export function getChatQuietStatus(): ChatQuietStatus {
  return chatQuietStatus;
}
