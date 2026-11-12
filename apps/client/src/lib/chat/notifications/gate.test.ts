import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReviewQueueItemRead } from "$lib/chat/contracts";
import {
  attentionSignals,
  CHAT_NOTIFICATION_GRACE_MS,
  createChatNotificationGate,
  type ChatAttentionKind,
  type ChatAttentionSignal,
  type ChatAttentionSnapshot,
  type ChatNotificationDelivery,
  type ChatNotificationGate,
  type ChatQuietState,
} from "./gate";

function signal(assignmentId: string, overrides: Partial<ChatAttentionSignal> = {}): ChatAttentionSignal {
  return {
    assignmentId,
    revision: 1,
    kind: "review",
    projectId: "project-1",
    channelId: "channel-1",
    replyThreadId: `thread-${assignmentId}`,
    teammateName: "Ada",
    channelName: "General",
    executedHere: true,
    stateReason: null,
    updatedAt: "2026-11-12T10:00:00Z",
    ...overrides,
  };
}

function snapshot(signals: ChatAttentionSignal[], truncated = false): ChatAttentionSnapshot {
  return { signals, truncated };
}

interface Harness {
  gate: ChatNotificationGate;
  delivered: ChatNotificationDelivery[];
  quiet: ChatQuietState;
  visible: Set<string>;
  disabled: Set<ChatAttentionKind>;
}

function harness(): Harness {
  const state: Omit<Harness, "gate"> = {
    delivered: [],
    quiet: { quiet: false, endsAtMs: null },
    visible: new Set(),
    disabled: new Set(),
  };
  const gate = createChatNotificationGate<ReturnType<typeof setTimeout>>({
    now: () => Date.now(),
    setTimer: (callback, timeoutMs) => setTimeout(callback, timeoutMs),
    clearTimer: (id) => clearTimeout(id),
    quietState: () => state.quiet,
    isThreadVisible: (replyThreadId) => state.visible.has(replyThreadId),
    isKindEnabled: (kind) => !state.disabled.has(kind),
    deliver: (delivery) => state.delivered.push(delivery),
  });
  return Object.assign(state, { gate });
}

function deliveredKeys(deliveries: readonly ChatNotificationDelivery[]): string[] {
  return deliveries.flatMap((delivery) => (delivery.kind === "single" ? [delivery.signal] : delivery.signals))
    .map((entry) => `${entry.assignmentId}:${entry.revision}`);
}

describe("Chat notification gate", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-12T10:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("records the first snapshot as a silent baseline", () => {
    const test = harness();
    test.gate.observe(snapshot([signal("a"), signal("b")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS * 2);
    test.gate.observe(snapshot([signal("a"), signal("b")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS * 2);

    expect(test.delivered).toEqual([]);
  });

  it("delivers a new key after exactly the grace period", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));

    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS - 1);
    expect(test.delivered).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(test.delivered).toEqual([{ kind: "single", signal: signal("a") }]);
  });

  it("notifies again for a new revision of the same assignment but never twice for one revision", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.observe(snapshot([signal("a", { revision: 2, kind: "answer" })]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.observe(snapshot([signal("a", { revision: 2, kind: "answer" })]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);

    expect(deliveredKeys(test.delivered)).toEqual(["a:1", "a:2"]);
  });

  it("never delivers a key resolved during the grace period", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS - 1);
    test.gate.observe(snapshot([]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);

    expect(test.delivered).toEqual([]);
  });

  it("skips a key whose reply thread is visible at release and does not retry it later", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.visible.add("thread-a");
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.visible.clear();
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);

    expect(test.delivered).toEqual([]);
  });

  it("skips a disabled kind and still delivers the enabled ones", () => {
    const test = harness();
    test.disabled.add("failure");
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a", { kind: "failure" }), signal("b", { kind: "approval" })]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);

    expect(deliveredKeys(test.delivered)).toEqual(["b:1"]);
  });

  it("never delivers work another device executed", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a", { executedHere: false })]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.reevaluate();

    expect(test.delivered).toEqual([]);
  });

  it("defers keys while quiet and delivers three of them as one summary when quiet ends", () => {
    const test = harness();
    test.quiet = { quiet: true, endsAtMs: null };
    test.gate.observe(snapshot([]));
    const signals = [signal("a"), signal("b", { kind: "answer" }), signal("c", { kind: "failure" })];
    test.gate.observe(snapshot(signals));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.reevaluate();
    expect(test.delivered).toEqual([]);

    test.quiet = { quiet: false, endsAtMs: null };
    test.gate.reevaluate();
    test.gate.reevaluate();

    expect(test.delivered).toEqual([{
      kind: "summary",
      counts: { answer: 1, approval: 0, review: 1, failure: 1 },
      signals,
    }]);
  });

  it("delivers one deferred key as a single notification", () => {
    const test = harness();
    test.quiet = { quiet: true, endsAtMs: Date.now() + 60_000 };
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.quiet = { quiet: false, endsAtMs: null };
    test.gate.reevaluate();

    expect(test.delivered).toEqual([{ kind: "single", signal: signal("a") }]);
  });

  it("drops a deferred key resolved during quiet", () => {
    const test = harness();
    test.quiet = { quiet: true, endsAtMs: null };
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a"), signal("b")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.observe(snapshot([signal("b")]));
    test.quiet = { quiet: false, endsAtMs: null };
    test.gate.reevaluate();

    expect(test.delivered).toEqual([{ kind: "single", signal: signal("b") }]);
  });

  it("reads the quiet state at release rather than when the key arrived", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS - 1);
    test.quiet = { quiet: true, endsAtMs: null };
    vi.advanceTimersByTime(1);

    expect(test.delivered).toEqual([]);
  });

  it("releases an overdue key on reevaluate when its timer has not fired", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    vi.setSystemTime(Date.now() + CHAT_NOTIFICATION_GRACE_MS);
    test.gate.reevaluate();

    expect(deliveredKeys(test.delivered)).toEqual(["a:1"]);
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    expect(test.delivered).toHaveLength(1);
  });

  it("prunes handled keys of assignments that left the queue, but not while the queue is truncated", () => {
    const test = harness();
    test.gate.observe(snapshot([signal("a")]));
    test.gate.observe(snapshot([], true));
    test.gate.observe(snapshot([signal("a")], true));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    expect(test.delivered).toEqual([]);

    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    expect(deliveredKeys(test.delivered)).toEqual(["a:1"]);
  });

  it("treats the next snapshot after reset as a silent baseline and cancels pending keys", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    test.gate.reset();
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.observe(snapshot([signal("b")]));
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);

    expect(test.delivered).toEqual([]);
  });

  it("ignores every call after dispose", () => {
    const test = harness();
    test.gate.observe(snapshot([]));
    test.gate.observe(snapshot([signal("a")]));
    test.gate.dispose();
    vi.advanceTimersByTime(CHAT_NOTIFICATION_GRACE_MS);
    test.gate.observe(snapshot([signal("b")]));
    test.gate.reevaluate();

    expect(test.delivered).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("attentionSignals", () => {
  it("maps queue items to signals and keeps the truncated flag", () => {
    const item: ChatReviewQueueItemRead = {
      assignmentId: "a",
      revision: 3,
      kind: "failure",
      projectId: "project-1",
      projectName: "Project",
      channelId: "channel-1",
      channelName: "General",
      replyThreadId: "thread-a",
      teammateParticipantId: "participant-1",
      teammateDisplayName: "Ada",
      stateReason: "Provider exited",
      changedFileCount: 0,
      executedHere: false,
      updatedAt: "2026-11-12T10:00:00Z",
    };

    expect(attentionSignals({ items: [item], total: 2, truncated: true })).toEqual({
      truncated: true,
      signals: [signal("a", { revision: 3, kind: "failure", executedHere: false, stateReason: "Provider exited" })],
    });
  });
});
