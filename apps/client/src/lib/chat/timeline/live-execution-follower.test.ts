import { describe, expect, it, vi } from "vitest";
import type { ChatLiveExecutionRead, ChatLiveFollowRead } from "$lib/chat/contracts";
import { LiveExecutionFollower, type LiveExecutionFollowerOptions, type LiveExecutionView } from "./live-execution-follower";
import {
  applyLiveExecutionPage,
  LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS,
  LIVE_EXECUTION_NOT_SERVED_BASE_MS,
  LIVE_EXECUTION_NOT_SERVED_MAX_MS,
  LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS,
  LIVE_EXECUTION_RETRY_BASE_MS,
  LIVE_EXECUTION_RETRY_MAX_MS,
  LIVE_EXECUTION_SETTLED_ATTEMPTS,
  type LiveExecutionState,
} from "./live-execution";

interface ReadCall {
  assignmentId: string;
  afterChange: number | null;
  wait: boolean;
  answer: (value: ChatLiveFollowRead) => void;
  fail: (error: unknown) => void;
}

/** A follower over a controlled reader, clock, and timer queue. */
function harness(initial?: LiveExecutionFollowerOptions["initial"]) {
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
    initial,
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
      runId: "run-1",
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

/** The state a follower holds after reading `answer`, as a run read earlier would leave it. */
function heldState(answer: ChatLiveFollowRead): LiveExecutionState {
  if (answer.kind !== "page") throw new Error("expected a page");
  const state = applyLiveExecutionPage(null, answer.read).state;
  if (!state) throw new Error("expected a held state");
  return state;
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

  it("keeps asking while no device serves the work yet and follows the run once a desktop claims it", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer({ kind: "notServed" });
    await settle();
    expect(test.last()).toEqual({ state: null, status: "notServed", claimExpected: true });
    test.advance(LIVE_EXECUTION_NOT_SERVED_BASE_MS);
    expect(test.calls).toHaveLength(2);
    const viewsBefore = test.views.length;
    for (let answer = 2; answer <= LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS; answer += 1) {
      test.calls.at(-1)?.answer({ kind: "notServed" });
      await settle();
      test.advance(LIVE_EXECUTION_NOT_SERVED_BASE_MS);
    }
    expect(test.views, "repeated answers publish nothing new").toHaveLength(viewsBefore);
    const steadyCalls = test.calls.length;
    expect(steadyCalls).toBe(LIVE_EXECUTION_NOT_SERVED_STEADY_ANSWERS + 1);
    test.calls.at(-1)?.answer({ kind: "notServed" });
    await settle();
    test.advance(LIVE_EXECUTION_NOT_SERVED_BASE_MS);
    expect(test.calls, "the delay grows once the claim is late").toHaveLength(steadyCalls);
    expect(test.last()?.claimExpected, "a late claim no longer reads as on its way").toBe(false);

    test.advance(LIVE_EXECUTION_NOT_SERVED_MAX_MS);
    expect(test.calls).toHaveLength(steadyCalls + 1);
    const claimed = test.calls.at(-1);
    expect(claimed).toMatchObject({ afterChange: null, wait: false });
    claimed?.answer(page());
    await settle();
    expect(test.last()?.status).toBe("live");
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    expect(test.calls.at(-1)).toMatchObject({ afterChange: 5, wait: true });
  });

  it("drops the rows and reads the whole run again when the serving desktop stops serving it", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page());
    await settle();
    test.advance(LIVE_EXECUTION_MIN_WAIT_INTERVAL_MS);
    test.calls[1]?.answer({ kind: "notServed" });
    await settle();
    expect(test.last()).toEqual({ state: null, status: "notServed", claimExpected: true });
    test.advance(LIVE_EXECUTION_NOT_SERVED_BASE_MS);
    expect(test.calls[2]).toMatchObject({ afterChange: null, wait: false });
  });

  it("stops asking while paused and asks again when resumed", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer({ kind: "notServed" });
    await settle();
    test.follower.pause();
    test.advance(LIVE_EXECUTION_NOT_SERVED_MAX_MS);
    expect(test.calls).toHaveLength(1);
    test.follower.follow("assignment-1");
    expect(test.calls).toHaveLength(2);
  });

  it("reads finished work once and stops when no device serves it", async () => {
    const test = harness();
    test.follower.follow("assignment-1", false);
    test.calls[0]?.answer({ kind: "notServed" });
    await settle();
    expect(test.last()?.status).toBe("notServed");
    expect(test.pendingTimers()).toBe(0);
  });

  it("tries a read of finished work a few times at most", async () => {
    const test = harness();
    test.follower.follow("assignment-1", false);
    for (let attempt = 1; attempt <= LIVE_EXECUTION_SETTLED_ATTEMPTS; attempt += 1) {
      test.calls.at(-1)?.answer({ kind: "unreachable" });
      await settle();
      test.advance(LIVE_EXECUTION_RETRY_MAX_MS);
    }
    expect(test.calls).toHaveLength(LIVE_EXECUTION_SETTLED_ATTEMPTS);
    expect(test.pendingTimers()).toBe(0);
    expect(test.last()?.status).toBe("retrying");
    test.follower.follow("assignment-1", false);
    expect(test.calls, "following again tries again").toHaveLength(LIVE_EXECUTION_SETTLED_ATTEMPTS + 1);
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

  it("starts from held rows without dropping them and settles once the desktop confirms the run", async () => {
    const test = harness({ assignmentId: "assignment-1", state: heldState(page({ settled: true })) });
    test.follower.follow("assignment-1", false);
    expect(test.views).toHaveLength(0);
    expect(test.calls[0]).toMatchObject({ assignmentId: "assignment-1", afterChange: 5, wait: false });

    test.calls[0]?.answer(page({ items: [], settled: true }));
    await settle();
    expect(test.last()?.status).toBe("settled");
    expect(test.last()?.state?.items.map((item) => item.activityId)).toEqual(["row-1"]);
    test.advance(60_000);
    expect(test.calls).toHaveLength(1);
  });

  it("reads the whole run again when held rows belong to an earlier turn of the work", async () => {
    const test = harness({ assignmentId: "assignment-1", state: heldState(page({ settled: true })) });
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page({ turnId: "turn-2", items: [] }));
    await settle();
    expect(test.last()).toMatchObject({ state: null, status: "connecting" });
    expect(test.calls[1]).toMatchObject({ afterChange: null, wait: false });
  });

  it("drops held rows of another assignment", () => {
    const test = harness({ assignmentId: "assignment-1", state: heldState(page({ settled: true })) });
    test.follower.follow("assignment-2");
    expect(test.last()).toEqual({ state: null, status: "connecting", claimExpected: true });
    expect(test.calls[0]).toMatchObject({ assignmentId: "assignment-2", afterChange: null });
  });

  it("starts over for another assignment", async () => {
    const test = harness();
    test.follower.follow("assignment-1");
    test.calls[0]?.answer(page());
    await settle();
    test.follower.follow("assignment-2");
    expect(test.last()).toEqual({ state: null, status: "connecting", claimExpected: true });
    expect(test.calls[1]).toMatchObject({ assignmentId: "assignment-2", afterChange: null, wait: false });
  });
});
