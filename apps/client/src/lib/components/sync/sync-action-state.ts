/** The one line a sync settings card shows about its latest action. */
export type SyncActionFeedback =
  | { kind: "none" }
  | { kind: "done"; message: string }
  | { kind: "failed"; message: string };

/**
 * The actions a sync settings card is running and the feedback of the latest one. Every start clears the feedback,
 * so a line never outlives the action that wrote it once another action begins.
 */
export interface SyncActionState<Action extends string> {
  running: readonly Action[];
  feedback: SyncActionFeedback;
}

export type SyncActionEvent<Action extends string> =
  /**
   * The user started `action`. It is refused while the same action runs, and while any action runs when
   * `exclusive` is set.
   */
  | { type: "started"; action: Action; exclusive?: boolean }
  /** `action` succeeded; `message` replaces the feedback, an empty one clears it, and `null` keeps it. */
  | { type: "succeeded"; action: Action; message: string | null }
  /** `action` failed with `message`. */
  | { type: "failed"; action: Action; message: string };

export const IDLE_SYNC_ACTIONS: SyncActionState<never> = { running: [], feedback: { kind: "none" } };

const NO_FEEDBACK: SyncActionFeedback = { kind: "none" };

/**
 * Apply one action event. A refused start or the outcome of an action that is not running returns the same state
 * object, so callers can tell an ignored move by identity.
 */
export function transitionSyncActions<Action extends string>(
  state: SyncActionState<Action>, event: SyncActionEvent<Action>,
): SyncActionState<Action> {
  const running = state.running.includes(event.action);
  switch (event.type) {
    case "started":
      if (running || (event.exclusive && state.running.length > 0)) return state;
      return { running: [...state.running, event.action], feedback: NO_FEEDBACK };
    case "succeeded":
      if (!running) return state;
      return {
        running: state.running.filter((action) => action !== event.action),
        feedback: event.message === null ? state.feedback : event.message ? { kind: "done", message: event.message } : NO_FEEDBACK,
      };
    case "failed":
      if (!running) return state;
      return {
        running: state.running.filter((action) => action !== event.action),
        feedback: { kind: "failed", message: event.message },
      };
  }
}

/** Whether `action` is running. */
export function syncActionRunning<Action extends string>(state: SyncActionState<Action>, action: Action): boolean {
  return state.running.includes(action);
}

/** Reads and writes the action state a component keeps in a rune. */
export interface SyncActionAccess<Action extends string> {
  read: () => SyncActionState<Action>;
  write: (state: SyncActionState<Action>) => void;
}

export interface SyncActionMessages<Result> {
  /** The feedback after success, or `null` to keep the current feedback. Without it, success keeps the feedback. */
  success?: (result: Result) => string | null;
  /** The feedback after failure; it also receives the cause so the caller can log it. */
  failure: (cause: unknown) => string;
  exclusive?: boolean;
}

/**
 * Run `task` as `action` and record its outcome. A refused start never runs the task. Resolves with the task's result
 * only when it ran and succeeded, so callers clear their inputs only then.
 */
export async function runSyncAction<Action extends string, Result>(
  access: SyncActionAccess<Action>,
  action: Action,
  task: () => Promise<Result>,
  messages: SyncActionMessages<Result>,
): Promise<{ ok: true; result: Result } | { ok: false }> {
  const before = access.read();
  const started = transitionSyncActions(before, { type: "started", action, exclusive: messages.exclusive });
  if (started === before) return { ok: false };
  access.write(started);
  try {
    const result = await task();
    access.write(transitionSyncActions(access.read(), {
      type: "succeeded", action, message: messages.success ? messages.success(result) : null,
    }));
    return { ok: true, result };
  } catch (cause: unknown) {
    access.write(transitionSyncActions(access.read(), { type: "failed", action, message: messages.failure(cause) }));
    return { ok: false };
  }
}
