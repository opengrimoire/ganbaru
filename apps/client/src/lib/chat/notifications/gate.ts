import type {
  ChatChannelId,
  ChatReplyThreadId,
  ChatReviewQueueKind,
  ChatReviewQueueRead,
  ChatWorkAssignmentId,
  UtcTimestamp,
} from "$lib/chat/contracts";

/** What a review queue item asks of the person; the queue never carries working or queued work. */
export type ChatAttentionKind = ChatReviewQueueKind;

/** One assignment that waits for the person, as the notification gate and announcements see it. */
export interface ChatAttentionSignal {
  assignmentId: ChatWorkAssignmentId;
  revision: number;
  kind: ChatAttentionKind;
  projectId: string;
  channelId: ChatChannelId;
  replyThreadId: ChatReplyThreadId;
  teammateName: string;
  channelName: string;
  /** Whether this device executed the assignment, which decides who sends its notification. */
  executedHere: boolean;
  /** The clipped state reason, shown only for failures and only when content inclusion is on. */
  stateReason: string | null;
  updatedAt: UtcTimestamp;
}

/** One review queue read as signals, with whether items past the cap were left out. */
export interface ChatAttentionSnapshot {
  signals: ChatAttentionSignal[];
  truncated: boolean;
}

/** Whether Chat is quiet on this device now, and when that ends when the end is known. */
export interface ChatQuietState {
  quiet: boolean;
  endsAtMs: number | null;
}

/** What the gate hands to the system: one notification, or one summary of work deferred while quiet. */
export type ChatNotificationDelivery =
  | { kind: "single"; signal: ChatAttentionSignal }
  | { kind: "summary"; counts: Record<ChatAttentionKind, number>; signals: ChatAttentionSignal[] };

/** How long a new attention change waits before it may notify, so short-lived states stay silent. */
export const CHAT_NOTIFICATION_GRACE_MS = 5_000;

/** Clocks, state reads, and delivery the gate depends on, injected so the gate stays pure. */
export interface ChatNotificationGateDependencies<TimerId> {
  now(): number;
  setTimer(callback: () => void, timeoutMs: number): TimerId;
  clearTimer(id: TimerId): void;
  /** The current quiet state, or `null` while it is unknown, which holds delivery as quiet does. */
  quietState(): ChatQuietState | null;
  isThreadVisible(replyThreadId: ChatReplyThreadId): boolean;
  /** Whether the person keeps this kind on, or `null` while the Chat settings are unknown, which holds delivery. */
  isKindEnabled(kind: ChatAttentionKind): boolean | null;
  deliver(delivery: ChatNotificationDelivery): void;
}

/** Decides which review queue changes become system notifications, and when. */
export interface ChatNotificationGate {
  /** Takes the latest queue snapshot; the first one after creation or `reset` is a silent baseline. */
  observe(snapshot: ChatAttentionSnapshot): void;
  /** Releases overdue keys and delivers what was deferred once quiet ends. Call after Focus, quiet, or settings changes. */
  reevaluate(): void;
  /** Forgets every key, so the next snapshot is a silent baseline again, as after a vault switch. */
  reset(): void;
  /** Cancels every timer and ignores later calls. */
  dispose(): void;
}

interface PendingKey<TimerId> {
  timer: TimerId;
  dueAtMs: number;
}

/**
 * Turns a review queue read into attention signals.
 *
 * @param queue The review queue read of the current vault.
 */
export function attentionSignals(queue: ChatReviewQueueRead): ChatAttentionSnapshot {
  return {
    truncated: queue.truncated,
    signals: queue.items.map((item) => ({
      assignmentId: item.assignmentId,
      revision: item.revision,
      kind: item.kind,
      projectId: item.projectId,
      channelId: item.channelId,
      replyThreadId: item.replyThreadId,
      teammateName: item.teammateDisplayName,
      channelName: item.channelName,
      executedHere: item.executedHere,
      stateReason: item.stateReason,
      updatedAt: item.updatedAt,
    })),
  };
}

/** The dedupe key of one attention change: a new revision of the same assignment notifies again. */
export function attentionKey(signal: Pick<ChatAttentionSignal, "assignmentId" | "revision">): string {
  return `${signal.assignmentId}:${signal.revision}`;
}

function emptyCounts(): Record<ChatAttentionKind, number> {
  return { answer: 0, approval: 0, review: 0, failure: 0 };
}

/**
 * Creates the notification gate. A new key waits {@link CHAT_NOTIFICATION_GRACE_MS}, then is skipped when its
 * reply thread is visible or its kind is turned off, marked handled without delivery when another device executed
 * it, deferred while quiet or while the quiet state or kind preference is unknown, and delivered otherwise. Deferred
 * keys still in the queue are delivered together once quiet ends and both are known.
 *
 * @param dependencies Clocks, state reads, and delivery.
 */
export function createChatNotificationGate<TimerId>(
  dependencies: ChatNotificationGateDependencies<TimerId>,
): ChatNotificationGate {
  let baselined = false;
  let disposed = false;
  let present = new Map<string, ChatAttentionSignal>();
  const handled = new Map<string, ChatWorkAssignmentId>();
  const pending = new Map<string, PendingKey<TimerId>>();
  const deferred = new Map<string, ChatAttentionSignal>();

  function cancelPending(key: string): void {
    const entry = pending.get(key);
    if (!entry) return;
    dependencies.clearTimer(entry.timer);
    pending.delete(key);
  }

  function release(key: string): void {
    const entry = pending.get(key);
    if (!entry) return;
    pending.delete(key);
    const signal = present.get(key);
    if (!signal) return;
    const enabled = dependencies.isKindEnabled(signal.kind);
    if (dependencies.isThreadVisible(signal.replyThreadId) || enabled === false || !signal.executedHere) {
      handled.set(key, signal.assignmentId);
      return;
    }
    if (enabled === null || quietOrUnknown()) {
      deferred.set(key, signal);
      return;
    }
    handled.set(key, signal.assignmentId);
    dependencies.deliver({ kind: "single", signal });
  }

  function quietOrUnknown(): boolean {
    return dependencies.quietState()?.quiet ?? true;
  }

  function flushDeferred(): void {
    if (deferred.size === 0 || quietOrUnknown()) return;
    const signals: ChatAttentionSignal[] = [];
    for (const [key, signal] of [...deferred]) {
      const enabled = dependencies.isKindEnabled(signal.kind);
      if (enabled === null) continue;
      deferred.delete(key);
      handled.set(key, signal.assignmentId);
      if (enabled && !dependencies.isThreadVisible(signal.replyThreadId)) signals.push(signal);
    }
    if (signals.length === 0) return;
    if (signals.length === 1) {
      dependencies.deliver({ kind: "single", signal: signals[0] });
      return;
    }
    const counts = emptyCounts();
    for (const signal of signals) counts[signal.kind] += 1;
    dependencies.deliver({ kind: "summary", counts, signals });
  }

  function clearAll(): void {
    for (const entry of pending.values()) dependencies.clearTimer(entry.timer);
    pending.clear();
    deferred.clear();
    handled.clear();
    present = new Map();
  }

  return {
    observe(snapshot) {
      if (disposed) return;
      present = new Map(snapshot.signals.map((signal) => [attentionKey(signal), signal]));
      if (!baselined) {
        baselined = true;
        for (const [key, signal] of present) handled.set(key, signal.assignmentId);
        return;
      }
      for (const key of [...pending.keys()]) {
        if (!present.has(key)) cancelPending(key);
      }
      for (const key of [...deferred.keys()]) {
        if (!present.has(key)) deferred.delete(key);
      }
      for (const key of present.keys()) {
        if (handled.has(key) || pending.has(key) || deferred.has(key)) continue;
        const timer = dependencies.setTimer(() => release(key), CHAT_NOTIFICATION_GRACE_MS);
        pending.set(key, { timer, dueAtMs: dependencies.now() + CHAT_NOTIFICATION_GRACE_MS });
      }
      if (snapshot.truncated) return;
      const presentAssignments = new Set(snapshot.signals.map((signal) => signal.assignmentId));
      for (const [key, assignmentId] of [...handled]) {
        if (!presentAssignments.has(assignmentId)) handled.delete(key);
      }
    },
    reevaluate() {
      if (disposed) return;
      const now = dependencies.now();
      for (const [key, entry] of [...pending]) {
        if (entry.dueAtMs > now) continue;
        dependencies.clearTimer(entry.timer);
        release(key);
      }
      flushDeferred();
    },
    reset() {
      if (disposed) return;
      clearAll();
      baselined = false;
    },
    dispose() {
      clearAll();
      disposed = true;
    },
  };
}
