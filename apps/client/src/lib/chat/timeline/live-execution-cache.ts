import type { ChatAgentRunId, ChatWorkAssignmentId } from "$lib/chat/contracts";
import { liveExecutionShowsWholeMessages, liveExecutionTimelinePage, type LiveExecutionState } from "./live-execution";

/** Settled runs kept for the session; reopening a thread past this many shows replies until the run is read again. */
export const SETTLED_LIVE_EXECUTION_CACHE_LIMIT = 32;

/**
 * Settled live executions a phone read this session, by work assignment, newest last. A settled run no longer changes,
 * so a thread opened again shows its turn at once instead of showing the replies until the run is read again.
 */
export class SettledLiveExecutionCache {
  readonly #entries = new Map<ChatWorkAssignmentId, LiveExecutionState>();

  constructor(private readonly limit = SETTLED_LIVE_EXECUTION_CACHE_LIMIT) {}

  /** Returns the settled run held for `assignmentId`, if any, and marks it as the newest. */
  get(assignmentId: ChatWorkAssignmentId): LiveExecutionState | null {
    const state = this.#entries.get(assignmentId);
    if (!state) return null;
    this.#entries.delete(assignmentId);
    this.#entries.set(assignmentId, state);
    return state;
  }

  /**
   * Returns the run whose whole turn the held execution of `assignmentId` shows, so a thread can set aside the replies
   * projected from it before its live view reports it.
   */
  shownRun(assignmentId: ChatWorkAssignmentId): ChatAgentRunId | null {
    const state = this.#entries.get(assignmentId);
    if (!state || liveExecutionTimelinePage(state) === null || !liveExecutionShowsWholeMessages(state)) return null;
    return state.runId;
  }

  /** Holds `state` when its run settled with rows to show, and otherwise drops what was held for `assignmentId`. */
  remember(assignmentId: ChatWorkAssignmentId, state: LiveExecutionState | null): void {
    this.#entries.delete(assignmentId);
    if (!state?.settled || liveExecutionTimelinePage(state) === null) return;
    this.#entries.set(assignmentId, state);
    while (this.#entries.size > this.limit) {
      const oldest = this.#entries.keys().next();
      if (oldest.done) break;
      this.#entries.delete(oldest.value);
    }
  }
}

/** The session's cache, shared by every thread a phone opens. */
export const settledLiveExecutions = new SettledLiveExecutionCache();
