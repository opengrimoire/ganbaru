import { describe, expect, it } from "vitest";
import type { ChatLiveExecutionRead, ChatTimelineItemRead, ChatTimelineTurnRead } from "$lib/chat/contracts";
import {
  LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS,
  LIVE_EXECUTION_NOT_SERVED_BASE_MS,
  LIVE_EXECUTION_NOT_SERVED_MAX_MS,
  LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS,
  LIVE_EXECUTION_RETRY_BASE_MS,
  LIVE_EXECUTION_RETRY_MAX_MS,
  applyLiveExecutionPage,
  liveExecutionNotServedDelay,
  liveExecutionShowsWholeMessages,
  liveExecutionRetryDelay,
  liveExecutionTimelinePage,
  liveExecutionWaitDelay,
  type LiveExecutionState,
} from "./live-execution";

function item(activityId: string, sequenceAnchor: number, turnId = "turn-1", text = activityId): ChatTimelineItemRead {
  return {
    activityId,
    turnId,
    sequenceAnchor,
    changeSequence: sequenceAnchor,
    kind: "message",
    data: { schemaVersion: 1, value: { role: "assistant", markdown: text } },
  };
}

function turn(turnId = "turn-1", state: ChatTimelineTurnRead["state"] = "active"): ChatTimelineTurnRead {
  return {
    turnId,
    state,
    startedAt: "2026-11-19T10:00:00Z",
    completedAt: null,
    stopReason: null,
    errorCode: null,
    modelId: "model-a",
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    usage: null,
    changedFiles: [],
  };
}

function read(overrides: Partial<ChatLiveExecutionRead> = {}): ChatLiveExecutionRead {
  return {
    assignmentId: "assignment-1",
    runId: "run-1",
    threadId: "thread-1",
    turnId: "turn-1",
    providerFamilyId: "codex",
    modelId: "model-a",
    items: [],
    turns: [turn()],
    removedItemIds: [],
    changeCursor: 1,
    hasMore: false,
    resetRequired: false,
    settled: false,
    ...overrides,
  };
}

function held(step: { state: LiveExecutionState | null }): LiveExecutionState {
  if (!step.state) throw new Error("expected a held execution");
  return step.state;
}

describe("applyLiveExecutionPage", () => {
  it("holds the first page in timeline order and waits for the next change", () => {
    const step = applyLiveExecutionPage(null, read({ items: [item("b", 4), item("a", 2)], changeCursor: 4 }));
    expect(step.next).toBe("wait");
    expect(held(step).items.map((row) => row.activityId)).toEqual(["a", "b"]);
    expect(held(step).changeCursor).toBe(4);
    expect(held(step).providerFamilyId).toBe("codex");
  });

  it("replaces rewritten rows, drops removed rows, and replaces the turn", () => {
    const first = held(applyLiveExecutionPage(null, read({ items: [item("a", 1), item("b", 2), item("c", 3)], changeCursor: 3 })));
    const step = applyLiveExecutionPage(first, read({
      items: [item("b", 2, "turn-1", "b streamed further"), item("d", 5)],
      removedItemIds: ["c"],
      turns: [turn("turn-1", "completed")],
      changeCursor: 6,
    }));
    const state = held(step);
    expect(state.items.map((row) => row.activityId)).toEqual(["a", "b", "d"]);
    expect(state.items[1]?.data.value).toEqual({ role: "assistant", markdown: "b streamed further" });
    expect(state.turns.map((entry) => entry.state)).toEqual(["completed"]);
    expect(state.changeCursor).toBe(6);
  });

  it("keeps only rows and turns of the run's own turn", () => {
    const state = held(applyLiveExecutionPage(null, read({
      items: [item("a", 1), item("other", 2, "turn-2")],
      turns: [turn(), turn("turn-2")],
    })));
    expect(state.items.map((row) => row.activityId)).toEqual(["a"]);
    expect(state.turns.map((entry) => entry.turnId)).toEqual(["turn-1"]);
  });

  it("reads the next page at once while more changes follow", () => {
    expect(applyLiveExecutionPage(null, read({ hasMore: true })).next).toBe("continue");
  });

  it("reads the rest of a settled run, then stops", () => {
    const step = applyLiveExecutionPage(null, read({ items: [item("a", 1)], settled: true, hasMore: true }));
    expect(step.next).toBe("continue");
    expect(held(step).settled).toBe(true);
    const last = applyLiveExecutionPage(held(step), read({ items: [item("b", 2)], settled: true }));
    expect(last.next).toBe("stop");
    expect(held(last).items.map((row) => row.activityId)).toEqual(["a", "b"]);
  });

  it("starts over when the cursor no longer applies", () => {
    const first = held(applyLiveExecutionPage(null, read({ items: [item("a", 1)] })));
    expect(applyLiveExecutionPage(first, read({ resetRequired: true }))).toEqual({ state: null, next: "restart" });
  });

  it("starts over when a newer run or another thread takes over", () => {
    const first = held(applyLiveExecutionPage(null, read({ items: [item("a", 1)] })));
    expect(applyLiveExecutionPage(first, read({ turnId: "turn-2", turns: [turn("turn-2")] }))).toEqual({ state: null, next: "restart" });
    expect(applyLiveExecutionPage(first, read({ threadId: "thread-2" }))).toEqual({ state: null, next: "restart" });
  });

  it("follows a run that starts after the first read without starting over", () => {
    const empty = held(applyLiveExecutionPage(null, read({ threadId: null, turnId: null, turns: [], changeCursor: 0 })));
    expect(liveExecutionTimelinePage(empty)).toBeNull();
    const step = applyLiveExecutionPage(empty, read({ items: [item("a", 1)] }));
    expect(step.next).toBe("wait");
    expect(held(step).items.map((row) => row.activityId)).toEqual(["a"]);
    expect(held(step).threadId).toBe("thread-1");
  });
});

describe("liveExecutionTimelinePage", () => {
  it("is empty until a row of the run arrives", () => {
    expect(liveExecutionTimelinePage(null)).toBeNull();
    expect(liveExecutionTimelinePage(held(applyLiveExecutionPage(null, read())))).toBeNull();
  });

  it("presents the held rows as one page without paging cursors", () => {
    const state = held(applyLiveExecutionPage(null, read({ items: [item("a", 1)], changeCursor: 9 })));
    expect(liveExecutionTimelinePage(state)).toEqual({
      threadId: "thread-1",
      items: state.items,
      turns: state.turns,
      previousCursor: null,
      nextCursor: null,
      threadRevision: 0,
      changeCursor: 9,
    });
  });
});

describe("retry and wait delays", () => {
  it("doubles the retry delay from the base up to the bound", () => {
    expect(liveExecutionRetryDelay(1)).toBe(LIVE_EXECUTION_RETRY_BASE_MS);
    expect(liveExecutionRetryDelay(2)).toBe(LIVE_EXECUTION_RETRY_BASE_MS * 2);
    expect(liveExecutionRetryDelay(3)).toBe(LIVE_EXECUTION_RETRY_BASE_MS * 4);
    expect(liveExecutionRetryDelay(50)).toBe(LIVE_EXECUTION_RETRY_MAX_MS);
    expect(liveExecutionRetryDelay(0)).toBe(LIVE_EXECUTION_RETRY_BASE_MS);
  });

  it("asks again quickly while a claim is usually on its way, then backs off", () => {
    expect(liveExecutionNotServedDelay(1)).toBe(LIVE_EXECUTION_NOT_SERVED_BASE_MS);
    expect(liveExecutionNotServedDelay(LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS)).toBe(LIVE_EXECUTION_NOT_SERVED_BASE_MS);
    expect(liveExecutionNotServedDelay(LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS + 1)).toBe(LIVE_EXECUTION_NOT_SERVED_BASE_MS * 2);
    expect(liveExecutionNotServedDelay(LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS + 50)).toBe(LIVE_EXECUTION_NOT_SERVED_MAX_MS);
  });

  it("spaces waiting reads that return at once and not ones that waited", () => {
    expect(liveExecutionWaitDelay(0)).toBe(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    expect(liveExecutionWaitDelay(400)).toBe(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS - 400);
    expect(liveExecutionWaitDelay(12_000)).toBe(0);
    expect(liveExecutionWaitDelay(-5)).toBe(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
  });
});

describe("liveExecutionShowsWholeMessages", () => {
  it("is false once a message arrived as its tail only", () => {
    const whole = held(applyLiveExecutionPage(null, read({ items: [item("a", 1)] })));
    expect(liveExecutionShowsWholeMessages(whole)).toBe(true);
    const tail = item("b", 2);
    tail.data = { schemaVersion: 1, value: { role: "assistant", markdown: "end", markdownTruncated: true } };
    const cut = held(applyLiveExecutionPage(whole, read({ items: [tail] })));
    expect(liveExecutionShowsWholeMessages(cut)).toBe(false);
  });
});
