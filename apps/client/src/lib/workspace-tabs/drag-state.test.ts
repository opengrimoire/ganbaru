import { describe, expect, it } from "vitest";
import {
  CLAIM_INTERVAL_MS,
  IDLE_TAB_DRAG,
  arrivalTarget,
  ownsTabDrag,
  shouldAskForTab,
  transitionTabDrag,
  type LiveTabDrag,
  type TabDragEvent,
  type TabDragState,
} from "./drag-state";
import { createWorkspaceTab, kindLocation } from "./model";

function drag(id: string, sourceLabel: string | null = "a"): LiveTabDrag {
  return { sourceLabel, tab: createWorkspaceTab(id, kindLocation("notes")), grabX: 0, grabY: 0, width: 100 };
}

function run(events: TabDragEvent[], from: TabDragState = IDLE_TAB_DRAG): TabDragState {
  return events.reduce(transitionTabDrag, from);
}

const TARGET = { id: "t", placement: "before" as const };

describe("transitionTabDrag", () => {
  it("draws and owns a drag this window starts, and ends both when it settles", () => {
    const started = run([{ type: "begun", drag: drag("x") }]);
    expect(started.live?.tab.id).toBe("x");
    expect(ownsTabDrag(started)).toBe(true);

    const settled = transitionTabDrag(started, { type: "settled", tabId: "x" });
    expect(settled.live).toBeNull();
    expect(settled.own.phase).toBe("none");
  });

  it("keeps its own drag source when another window's drag replaces the drawn one", () => {
    const state = run([{ type: "begun", drag: drag("x") }, { type: "remoteBegun", drag: drag("y", "b") }]);
    expect(state.live?.tab.id).toBe("y");
    expect(ownsTabDrag(state)).toBe(true);

    const ended = transitionTabDrag(state, { type: "sourceEnded", tabId: "x" });
    expect(ended.live?.tab.id).toBe("y");
    expect(ownsTabDrag(ended)).toBe(false);
  });

  it("closes only after the drag during which the last tab left ends", () => {
    const emptied = run([{ type: "begun", drag: drag("x") }, { type: "leftEmpty" }]);
    expect(emptied.own).toEqual({ phase: "dragging", tabId: "x", leftEmpty: true });
    expect(transitionTabDrag(emptied, { type: "settled", tabId: "other" }).own.phase).toBe("dragging");

    const pending = transitionTabDrag(emptied, { type: "sourceEnded", tabId: "x" });
    expect(pending.own.phase).toBe("closePending");
    expect(transitionTabDrag(pending, { type: "closed" }).own.phase).toBe("none");
  });

  it("ignores a last tab leaving outside this window's own drag", () => {
    expect(transitionTabDrag(IDLE_TAB_DRAG, { type: "leftEmpty" })).toBe(IDLE_TAB_DRAG);
  });

  it("keeps a pending close through a reset", () => {
    const emptied = run([{ type: "begun", drag: drag("x") }, { type: "leftEmpty" }, { type: "reset" }]);
    expect(emptied.own.phase).toBe("closePending");
    expect(emptied.live).toBeNull();
  });

  it("forgets a hovered tab when its drag settles but keeps a dropped one until it arrives or times out", () => {
    const hovered = run([{ type: "claimed", tabId: "x", target: TARGET, dropped: false, askedAt: 0 }]);
    expect(arrivalTarget(transitionTabDrag(hovered, { type: "settled", tabId: "x" }), "x")).toBeNull();

    const dropped = run([{ type: "claimed", tabId: "x", target: TARGET, dropped: true, askedAt: 0 }, { type: "settled", tabId: "x" }]);
    expect(arrivalTarget(dropped, "x")).toEqual(TARGET);
    expect(arrivalTarget(dropped, "y")).toBeNull();
    expect(arrivalTarget(transitionTabDrag(dropped, { type: "arrived", tabId: "x" }), "x")).toBeNull();
    expect(arrivalTarget(transitionTabDrag(dropped, { type: "arrivalTimedOut", tabId: "x" }), "x")).toBeNull();
    expect(transitionTabDrag(dropped, { type: "arrivalTimedOut", tabId: "y" })).toBe(dropped);
  });

  it("asks again for a hovered tab only after the claim interval, and always on a drop", () => {
    const asked = run([{ type: "claimed", tabId: "x", target: null, dropped: false, askedAt: 1000 }]);
    expect(shouldAskForTab(asked, "x", false, 1000 + CLAIM_INTERVAL_MS - 1)).toBe(false);
    expect(shouldAskForTab(asked, "x", false, 1000 + CLAIM_INTERVAL_MS)).toBe(true);
    expect(shouldAskForTab(asked, "y", false, 1001)).toBe(true);
    expect(shouldAskForTab(asked, "x", true, 1001)).toBe(true);

    const heldOff = transitionTabDrag(asked, { type: "claimed", tabId: "x", target: TARGET, dropped: false, askedAt: null });
    expect(heldOff.lastClaim).toEqual({ tabId: "x", at: 1000 });
    expect(arrivalTarget(heldOff, "x")).toEqual(TARGET);
  });

  it("returns the same state for events that do not apply", () => {
    expect(transitionTabDrag(IDLE_TAB_DRAG, { type: "sourceEnded", tabId: "x" })).toBe(IDLE_TAB_DRAG);
    expect(transitionTabDrag(IDLE_TAB_DRAG, { type: "arrived", tabId: "x" })).toBe(IDLE_TAB_DRAG);
    expect(transitionTabDrag(IDLE_TAB_DRAG, { type: "closed" })).toBe(IDLE_TAB_DRAG);
    expect(transitionTabDrag(IDLE_TAB_DRAG, { type: "settled", tabId: "x" })).toBe(IDLE_TAB_DRAG);
    expect(transitionTabDrag(IDLE_TAB_DRAG, { type: "reset" })).toBe(IDLE_TAB_DRAG);
  });
});
