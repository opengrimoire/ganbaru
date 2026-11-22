/**
 * Where one native Calendar write stands. A request is retained from the moment it is built until its reply is
 * known, so a lost reply is retried with the exact same request instead of a new review.
 *
 * - `idle`: nothing is being written and no request is retained.
 * - `reviewing`: a Save is reading the review its request will be built from.
 * - `sending`: the retained request is being written; `retry` marks a request whose earlier reply was lost.
 * - `uncertain`: the reply to the retained request was lost, so whether it applied is unknown.
 */
export type NativeCommitState<Retained> =
  | { phase: "idle" }
  | { phase: "reviewing" }
  | { phase: "sending"; pending: Retained; retry: boolean }
  | { phase: "uncertain"; pending: Retained };

export type NativeCommitEvent<Retained> =
  | { type: "reviewStarted" }
  /** The review failed or the editor changed before a request was built. */
  | { type: "reviewEnded" }
  | { type: "sendStarted"; pending: Retained }
  /** The write applied or was rejected, so the request is no longer retained. */
  | { type: "sendResolved" }
  | { type: "sendLost" };

export const IDLE_NATIVE_COMMIT: NativeCommitState<never> = { phase: "idle" };

/**
 * Apply one write event. Only an idle write starts a review, a send starts from a review or retries the uncertain
 * request it carries, and an event out of order returns the same state object.
 */
export function transitionNativeCommit<Retained>(
  state: NativeCommitState<Retained>,
  event: NativeCommitEvent<Retained>,
): NativeCommitState<Retained> {
  switch (event.type) {
    case "reviewStarted":
      return state.phase === "idle" ? { phase: "reviewing" } : state;
    case "reviewEnded":
      return state.phase === "reviewing" ? IDLE_NATIVE_COMMIT : state;
    case "sendStarted":
      if (state.phase === "idle" || state.phase === "reviewing") {
        return { phase: "sending", pending: event.pending, retry: false };
      }
      return state.phase === "uncertain" && state.pending === event.pending
        ? { phase: "sending", pending: state.pending, retry: true }
        : state;
    case "sendResolved":
      return state.phase === "sending" ? IDLE_NATIVE_COMMIT : state;
    case "sendLost":
      return state.phase === "sending" ? { phase: "uncertain", pending: state.pending } : state;
  }
}

/** Whether a write or the review it waits on is running. */
export function commitRunning(state: NativeCommitState<unknown>): boolean {
  return state.phase === "reviewing" || state.phase === "sending";
}

/** Whether the retained request's reply was lost, including while it is retried. */
export function commitUncertain(state: NativeCommitState<unknown>): boolean {
  return state.phase === "uncertain" || (state.phase === "sending" && state.retry);
}

/** The request retained until its reply is known, or null. */
export function retainedCommit<Retained>(state: NativeCommitState<Retained>): Retained | null {
  return state.phase === "sending" || state.phase === "uncertain" ? state.pending : null;
}
