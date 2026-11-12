import type { ChatChangeNotification, ChatProjectionKey } from "$lib/chat/contracts";

/** The projection keys and event sequence range of the native changes one thread received in a frame. */
export interface ChatNativeChange {
  keys: ReadonlySet<ChatProjectionKey>;
  /** The first event sequence covered, or null when the change did not come from a notification. */
  sinceSequence: number | null;
  /** The newest event sequence covered, or null when the change did not come from a notification. */
  sequence: number | null;
}

/** What a pane rereads for one thread's native change. */
export interface ChatNativeRefreshPlan {
  shell: boolean;
  organizational: boolean;
  timeline: boolean;
  interaction: boolean;
}

const TIMELINE_KEYS: readonly ChatProjectionKey[] = ["turns", "messages", "activities", "plans"];
const INTERACTION_KEYS: readonly ChatProjectionKey[] = ["pending_requests", "turns", "thread"];

/** A change that names no projection, such as one this window made outside a notification, so everything rereads. */
export function fullNativeChange(): ChatNativeChange {
  return { keys: new Set(["unknown"]), sinceSequence: null, sequence: null };
}

export function nativeChangeFromNotification(notification: ChatChangeNotification): ChatNativeChange {
  return {
    keys: new Set(notification.changedProjectionKeys),
    sinceSequence: notification.sinceSequence,
    sequence: notification.sequence,
  };
}

/** Unions two changes to one thread: every key, the earliest first sequence, and the newest last sequence. */
export function mergeNativeChanges(current: ChatNativeChange, next: ChatNativeChange): ChatNativeChange {
  return {
    keys: new Set([...current.keys, ...next.keys]),
    sinceSequence: minimumSequence(current.sinceSequence, next.sinceSequence),
    sequence: maximumSequence(current.sequence, next.sequence),
  };
}

/**
 * Routes a native change to the rereads it needs. An `unknown` key, or a first sequence that skips past the last one
 * seen for the thread, means a notification was missed or not understood, so everything rereads. A change that names
 * no projection still refreshes the interaction, since runtime state can change without a projected row.
 *
 * @param lastSeenSequence The newest event sequence this pane handled for the thread, or null before the first one.
 */
export function planNativeRefresh(
  change: ChatNativeChange,
  selected: boolean,
  lastSeenSequence: number | null,
): ChatNativeRefreshPlan {
  const skipped = lastSeenSequence !== null
    && change.sinceSequence !== null
    && change.sinceSequence > lastSeenSequence + 1;
  const full = skipped || change.keys.has("unknown");
  const names = (keys: readonly ChatProjectionKey[]) => keys.some((key) => change.keys.has(key));
  return {
    shell: full || change.keys.has("thread"),
    organizational: full || change.keys.has("organizational"),
    timeline: selected && (full || names(TIMELINE_KEYS)),
    interaction: selected && (full || change.keys.size === 0 || names(INTERACTION_KEYS)),
  };
}

function minimumSequence(left: number | null, right: number | null): number | null {
  if (left === null) return right;
  if (right === null) return left;
  return Math.min(left, right);
}

function maximumSequence(left: number | null, right: number | null): number | null {
  if (left === null) return right;
  if (right === null) return left;
  return Math.max(left, right);
}
