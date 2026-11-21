import { describe, expect, it } from "vitest";
import type { ChatTimelineItemRead } from "$lib/chat/contracts";
import type { LiveExecutionState } from "./live-execution";
import { SettledLiveExecutionCache } from "./live-execution-cache";

function message(markdownTruncated = false): ChatTimelineItemRead {
  return {
    activityId: "row-1",
    turnId: "turn-1",
    sequenceAnchor: 1,
    changeSequence: 1,
    kind: "message",
    data: { schemaVersion: 1, value: { role: "assistant", markdown: "Hi", markdownTruncated } },
  };
}

function state(overrides: Partial<LiveExecutionState> = {}): LiveExecutionState {
  return {
    runId: "run-1",
    threadId: "thread-1",
    turnId: "turn-1",
    providerFamilyId: "codex",
    modelId: null,
    items: [message()],
    turns: [],
    changeCursor: 5,
    settled: true,
    ...overrides,
  };
}

describe("SettledLiveExecutionCache", () => {
  it("holds a settled run and reports the run whose whole turn it shows", () => {
    const cache = new SettledLiveExecutionCache();
    cache.remember("assignment-1", state());
    expect(cache.get("assignment-1")?.runId).toBe("run-1");
    expect(cache.shownRun("assignment-1")).toBe("run-1");
    expect(cache.shownRun("assignment-2")).toBeNull();
  });

  it("does not hold a run that is still active or has no rows, and drops what it held for it", () => {
    const cache = new SettledLiveExecutionCache();
    cache.remember("assignment-1", state());
    cache.remember("assignment-1", state({ settled: false }));
    expect(cache.get("assignment-1")).toBeNull();
    cache.remember("assignment-2", state({ items: [] }));
    expect(cache.get("assignment-2")).toBeNull();
    cache.remember("assignment-3", state());
    cache.remember("assignment-3", null);
    expect(cache.get("assignment-3")).toBeNull();
  });

  it("does not report a shown run while a message of it is cut short", () => {
    const cache = new SettledLiveExecutionCache();
    cache.remember("assignment-1", state({ items: [message(true)] }));
    expect(cache.get("assignment-1")).not.toBeNull();
    expect(cache.shownRun("assignment-1")).toBeNull();
  });

  it("drops the run used least recently past its limit", () => {
    const cache = new SettledLiveExecutionCache(2);
    cache.remember("assignment-1", state());
    cache.remember("assignment-2", state());
    cache.get("assignment-1");
    cache.remember("assignment-3", state());
    expect(cache.get("assignment-2")).toBeNull();
    expect(cache.get("assignment-1")).not.toBeNull();
    expect(cache.get("assignment-3")).not.toBeNull();
  });
});
