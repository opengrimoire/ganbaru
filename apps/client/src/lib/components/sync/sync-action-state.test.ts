import { describe, expect, it, vi } from "vitest";
import {
  IDLE_SYNC_ACTIONS,
  runSyncAction,
  syncActionRunning,
  transitionSyncActions,
  type SyncActionState,
} from "./sync-action-state";

type Action = "reset" | "syncNow" | "pause";

function started(state: SyncActionState<Action>, action: Action, exclusive = false): SyncActionState<Action> {
  return transitionSyncActions(state, { type: "started", action, exclusive });
}

describe("transitionSyncActions", () => {
  it("clears a reset notice when another action starts and later succeeds", () => {
    let state = transitionSyncActions(started(IDLE_SYNC_ACTIONS, "reset"), {
      type: "succeeded", action: "reset", message: "Sync was reset",
    });
    expect(state.feedback).toEqual({ kind: "done", message: "Sync was reset" });
    state = transitionSyncActions(started(state, "syncNow"), { type: "failed", action: "syncNow", message: "Sync failed" });
    expect(state.feedback).toEqual({ kind: "failed", message: "Sync failed" });
    state = transitionSyncActions(started(state, "syncNow"), { type: "succeeded", action: "syncNow", message: null });
    expect(state.feedback).toEqual({ kind: "none" });
  });

  it("runs different actions side by side and tracks each one", () => {
    const both = started(started(IDLE_SYNC_ACTIONS, "syncNow"), "pause");
    expect(syncActionRunning(both, "syncNow")).toBe(true);
    expect(syncActionRunning(both, "pause")).toBe(true);
    const one = transitionSyncActions(both, { type: "succeeded", action: "pause", message: null });
    expect(syncActionRunning(one, "pause")).toBe(false);
    expect(syncActionRunning(one, "syncNow")).toBe(true);
  });

  it("keeps a failure of one action when a concurrent action succeeds without a message", () => {
    const both = started(started(IDLE_SYNC_ACTIONS, "syncNow"), "pause");
    const failed = transitionSyncActions(both, { type: "failed", action: "pause", message: "Pause failed" });
    const done = transitionSyncActions(failed, { type: "succeeded", action: "syncNow", message: null });
    expect(done.feedback).toEqual({ kind: "failed", message: "Pause failed" });
    expect(done.running).toEqual([]);
  });

  it("refuses to start an action that is already running", () => {
    const running = started(IDLE_SYNC_ACTIONS, "reset");
    expect(started(running, "reset")).toBe(running);
  });

  it("refuses an exclusive start while any action runs", () => {
    const running = started(IDLE_SYNC_ACTIONS, "reset");
    expect(started(running, "pause", true)).toBe(running);
    expect(syncActionRunning(started(IDLE_SYNC_ACTIONS, "pause", true), "pause")).toBe(true);
  });

  it("ignores the outcome of an action that is not running", () => {
    expect(transitionSyncActions(IDLE_SYNC_ACTIONS, { type: "succeeded", action: "reset", message: "Done" }))
      .toBe(IDLE_SYNC_ACTIONS);
    expect(transitionSyncActions(IDLE_SYNC_ACTIONS, { type: "failed", action: "reset", message: "Failed" }))
      .toBe(IDLE_SYNC_ACTIONS);
  });
});

describe("runSyncAction", () => {
  function holder(initial: SyncActionState<Action> = IDLE_SYNC_ACTIONS) {
    let state = initial;
    return { read: () => state, write: (next: SyncActionState<Action>) => { state = next; } };
  }

  it("returns the result and shows the success message", async () => {
    const access = holder();
    const outcome = await runSyncAction(access, "reset", async () => 3, {
      success: (count) => `Reset ${count}`, failure: () => "Failed",
    });
    expect(outcome).toEqual({ ok: true, result: 3 });
    expect(access.read()).toEqual({ running: [], feedback: { kind: "done", message: "Reset 3" } });
  });

  it("reports a failure without a result", async () => {
    const access = holder();
    const cause = new Error("offline");
    const failure = vi.fn(() => "Failed");
    const outcome = await runSyncAction(access, "reset", async () => { throw cause; }, { failure });
    expect(outcome).toEqual({ ok: false });
    expect(failure).toHaveBeenCalledWith(cause);
    expect(access.read().feedback).toEqual({ kind: "failed", message: "Failed" });
  });

  it("does not run a task whose start is refused", async () => {
    const access = holder(started(IDLE_SYNC_ACTIONS, "reset"));
    const task = vi.fn(async () => undefined);
    const outcome = await runSyncAction(access, "reset", task, { failure: () => "Failed" });
    expect(outcome).toEqual({ ok: false });
    expect(task).not.toHaveBeenCalled();
  });
});
