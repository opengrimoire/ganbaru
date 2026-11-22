import { describe, expect, it } from "vitest";
import {
  IDLE_NATIVE_COMMIT,
  commitRunning,
  commitUncertain,
  retainedCommit,
  transitionNativeCommit,
  type NativeCommitEvent,
  type NativeCommitState,
} from "./native-commit-state";

interface Request { id: string }

const request: Request = { id: "edit-1" };
const other: Request = { id: "edit-2" };
const idle: NativeCommitState<Request> = IDLE_NATIVE_COMMIT;

function run(events: NativeCommitEvent<Request>[], from: NativeCommitState<Request> = idle): NativeCommitState<Request> {
  return events.reduce(transitionNativeCommit<Request>, from);
}

const reviewing = run([{ type: "reviewStarted" }]);
const sending = run([{ type: "sendStarted", pending: request }], reviewing);
const uncertain = run([{ type: "sendLost" }], sending);
const retrying = run([{ type: "sendStarted", pending: request }], uncertain);

describe("transitionNativeCommit", () => {
  it("reviews, sends, and forgets the request once its reply is known", () => {
    expect(reviewing).toEqual({ phase: "reviewing" });
    expect(sending).toEqual({ phase: "sending", pending: request, retry: false });
    expect(transitionNativeCommit(sending, { type: "sendResolved" })).toEqual(IDLE_NATIVE_COMMIT);
  });

  it("sends a reviewed deletion or an Undo without a review of its own", () => {
    expect(run([{ type: "sendStarted", pending: request }])).toEqual({ phase: "sending", pending: request, retry: false });
  });

  it("keeps the request after a lost reply and retries exactly that request", () => {
    expect(uncertain).toEqual({ phase: "uncertain", pending: request });
    expect(retrying).toEqual({ phase: "sending", pending: request, retry: true });
    expect(transitionNativeCommit(retrying, { type: "sendLost" })).toEqual(uncertain);
    expect(transitionNativeCommit(retrying, { type: "sendResolved" })).toEqual(IDLE_NATIVE_COMMIT);
  });

  it("never replaces an uncertain request with another one", () => {
    expect(transitionNativeCommit(uncertain, { type: "sendStarted", pending: other })).toBe(uncertain);
    expect(transitionNativeCommit(uncertain, { type: "reviewStarted" })).toBe(uncertain);
  });

  it("ignores a second start while a write or its review runs", () => {
    for (const state of [reviewing, sending, retrying]) {
      expect(transitionNativeCommit(state, { type: "reviewStarted" })).toBe(state);
    }
    expect(transitionNativeCommit(sending, { type: "sendStarted", pending: other })).toBe(sending);
  });

  it("returns to idle when a review ends without a request and ignores the end elsewhere", () => {
    expect(transitionNativeCommit(reviewing, { type: "reviewEnded" })).toEqual(IDLE_NATIVE_COMMIT);
    for (const state of [idle, sending, uncertain]) {
      expect(transitionNativeCommit(state, { type: "reviewEnded" })).toBe(state);
    }
  });

  it("ignores reply events when nothing is being sent", () => {
    for (const state of [idle, reviewing, uncertain]) {
      expect(transitionNativeCommit(state, { type: "sendResolved" })).toBe(state);
      expect(transitionNativeCommit(state, { type: "sendLost" })).toBe(state);
    }
  });
});

describe("native commit projections", () => {
  it("reports a running write from review until the reply", () => {
    expect([idle, reviewing, sending, uncertain, retrying].map(commitRunning)).toEqual([false, true, true, false, true]);
  });

  it("reports an uncertain request until a retry's reply lands", () => {
    expect([idle, reviewing, sending, uncertain, retrying].map(commitUncertain)).toEqual([false, false, false, true, true]);
  });

  it("retains the request only while it is sent or uncertain", () => {
    expect([idle, reviewing].map(retainedCommit)).toEqual([null, null]);
    expect([sending, uncertain, retrying].map(retainedCommit)).toEqual([request, request, request]);
  });
});
