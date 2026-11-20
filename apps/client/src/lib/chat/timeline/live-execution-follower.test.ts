import { describe, expect, it, vi } from "vitest";
import type { ChatLiveExecutionRead, ChatLiveFollowRead } from "$lib/chat/contracts";
import { LiveExecutionFollower, type LiveExecutionView } from "./live-execution-follower";
import { LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS, LIVE_EXECUTION_RETRY_BASE_MS } from "./live-execution";

interface ReadCall {
  assignmentId: string;
  afterChange: number | null;
  wait: boolean;
  answer: (value: ChatLiveFollowRead) => void;
  fail: (error: unknown) => void;
}

/** A follower over a controlled reader, clock, and timer queue. */
function harness() {
  const calls: ReadCall[] = [];
  const timers: Array<{ at: number; callback: () => void; cancelled: boolean }> = [];
  const views: LiveExecutionView[] = [];
  let clock = 0;
  const follower = new LiveExecutionFollower({
    read: (assignmentId, afterChange, wait) => new Promise<ChatLiveFollowRead>((resolve, reject) => {
      calls.push({ assignmentId, afterChange, wait, answer: resolve, fail: reject });
    }),
    onUpdate: (view) => views.push(view),
    schedule: (callback, delayMs) => {
      const timer = { at: clock + delayMs, callback, cancelled: false };
      timers.push(timer);
      return () => { timer.cancelled = true; };
    },
    now: () => clock,
  });
  return {
    follower,
    calls,
    views,
    /** Moves the clock and runs every timer that comes due. */
    advance(ms: number) {
      clock += ms;
      for (const timer of timers.filter((entry) => !entry.cancelled && entry.at <= clock)) {
        timer.cancelled = true;
        timer.callback();
      }
    },
    pendingTimers: () => timers.filter((entry) => !entry.cancelled).length,
    last: () => views.at(-1),
  };
}

function page(overrides: Partial<ChatLiveExecutionRead> = {}): ChatLiveFollowRead {
  return {
    kind: "page",
    read: {
      assignmentId: "assignment-1",
      threadId: "thread-1",
      turnId: "turn-1",
      providerFamilyId: "codex",
      modelId: null,
      items: [{
        activityId: "row-1",
        turnId: "turn-1",
        sequenceAnchor: 1,
        changeSequence: 1,
        kind: "message",
        data: { schemaVersion: 1, value: { role: "assistant", markdown: "Hi" } },
      }],
      turns: [],
      removedItemIds: [],
      changeCursor: 5,
      hasMore: false,
      resetRequired: false,
      settled: false,
      ...overrides,
    },
  };
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("LiveExecutionFollower", () => {
  it("reads the whole run first, then waits on the desktop from the returned cursor", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    expect(test.calls).toHaveLength(1);
    expect(test.calls[0]).toMatchObject({ assignmentId: "assignment-1", afterChange: null, wait: false });
    expect(test.last()?.status).toBe("connecting");

    test.calls[0]?.answer(page());
    await settle();
    expect(test.last()?.status).toBe("live");
    expect(test.last()?.state?.items).toHaveLength(1);
    // The first read returned at once, so the waiting read is spaced out.
    expect(test.calls).toHaveLength(1);
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    expect(test.calls[1]).toMatchObject({ afterChange: 5, wait: true });
  });

  it("follows a long wait at once and reads the next page at once while more changes follow", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page({ hasMore: true }));
    await settle();
    expect(test.calls[1]).toMatchObject({ afterChange: 5, wait: false });

    test.calls[1]?.answer(page({ changeCursor: 7 }));
    await settle();
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    expect(test.calls[2]).toMatchObject({ afterChange: 7, wait: true });
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS * 3);
    test.calls[2]?.answer(page({ changeCursor: 8 }));
    await settle();
    expect(test.calls[3]).toMatchObject({ afterChange: 8, wait: true });
  });

  it("stops reading once the run settles and keeps the rows", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page({ settled: true }));
    await settle();
    test.advance(60_000);
    expect(test.calls).toHaveLength(1);
    expect(test.last()?.status).toBe("settled");
    expect(test.last()?.state?.items).toHaveLength(1);
    test.follower.follow("assignment-1");
    expect(test.calls).toHaveLength(1);
  });

  it("reads again from the start when the cursor no longer applies", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page());
    await settle();
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    test.calls[1]?.answer(page({ resetRequired: true, items: [] }));
    await settle();
    expect(test.last()).toMatchObject({ state: null, status: "connecting" });
    expect(test.calls[2]).toMatchObject({ afterChange: null, wait: false });
  });

  it("backs off while the desktop is unreachable, keeps the rows held, and recovers", async () => {
    const test = harness();
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page());
    await settle();
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    test.calls[1]?.answer({ kind: "unreachable" });
    await settle();
    expect(test.last()?.status).toBe("retrying");
    expect(test.last()?.state?.items).toHaveLength(1);
    test.advance(LIVE_EXECUTION_RETRY_BASE_MS - 1);
    expect(test.calls).toHaveLength(2);
    test.advance(1);
    expect(test.calls[2]).toMatchObject({ afterChange: 5, wait: false });

    test.calls[2]?.fail(new Error("invoke failed"));
    await settle();
    test.advance(LIVE_EXECUTION_RETRY_BASE_MS * 2 - 1);
    expect(test.calls).toHaveLength(3);
    test.advance(1);
    test.calls[3]?.answer(page({ changeCursor: 6 }));
    await settle();
    expect(test.last()?.status).toBe("live");

    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    test.calls[4]?.answer({ kind: "unreachable" });
    await settle();
    test.advance(LIVE_EXECUTION_RETRY_BASE_MS);
    expect(test.calls).toHaveLength(6);
    vi.restoreAllMocks();
  });

  it("gives up and drops the rows when no other device serves the work", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer({ kind: "notServed" });
    await settle();
    test.advance(60_000);
    expect(test.calls).toHaveLength(1);
    expect(test.last()).toEqual({ state: null, status: "notServed" });
  });

  it("drops a read that returns after a pause and resumes from the held cursor", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page());
    await settle();
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    const viewsBefore = test.views.length;
    test.follower.pause();
    test.calls[1]?.answer(page({ changeCursor: 9 }));
    await settle();
    expect(test.views).toHaveLength(viewsBefore);
    expect(test.pendingTimers()).toBe(0);

    test.follower.follow("assignment-1");
    expect(test.calls[2]).toMatchObject({ afterChange: 5, wait: false });
  });

  it("cancels a scheduled read when paused", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page());
    await settle();
    test.follower.pause();
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS * 5);
    expect(test.calls).toHaveLength(1);
  });

  it("starts over for another assignment", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page());
    await settle();
    test.follower.follow("assignment-2");
    expect(test.last()).toEqual({ state: null, status: "connecting" });
    expect(test.calls[1]).toMatchObject({ assignmentId: "assignment-2", afterChange: null, wait: false });
  });
});
