import { describe, expect, it } from "vitest";
import { createWorkspaceTab, kindLocation, moveWorkspaceTab, type WorkspaceTabsState } from "$lib/workspace-tabs/model";
import { mobileTabBarDropMoves, mobileTabBarSlots, mobileTabsButtonBadge } from "./tab-bar";

const tabs = ["a", "b", "c", "d", "e", "f"].map((id) => ({ id }));
const ids = (slots: readonly { id: string }[]) => slots.map((slot) => slot.id);

describe("mobileTabBarSlots", () => {
  it("shows the first tabs in order while the active one is among them", () => {
    expect(ids(mobileTabBarSlots(tabs, "c"))).toEqual(["a", "b", "c", "d"]);
    expect(ids(mobileTabBarSlots(tabs, "d"))).toEqual(["a", "b", "c", "d"]);
  });

  it("puts an active tab past the bar in the last slot", () => {
    expect(ids(mobileTabBarSlots(tabs, "f"))).toEqual(["a", "b", "c", "f"]);
    expect(ids(mobileTabBarSlots(tabs, "e"))).toEqual(["a", "b", "c", "e"]);
  });

  it("shows every tab when there are fewer than the bar holds", () => {
    expect(ids(mobileTabBarSlots(tabs.slice(0, 2), "b"))).toEqual(["a", "b"]);
  });

  it("keeps the leading tabs when the active id is unknown", () => {
    expect(ids(mobileTabBarSlots(tabs, "missing"))).toEqual(["a", "b", "c", "d"]);
  });

  it("follows a custom capacity and shows nothing without room", () => {
    expect(ids(mobileTabBarSlots(tabs, "e", 1))).toEqual(["e"]);
    expect(mobileTabBarSlots(tabs, "a", 0)).toEqual([]);
  });
});

describe("mobileTabsButtonBadge", () => {
  const format = (value: number) => new Intl.NumberFormat("en").format(value);

  it("offers to add a tab while the bar shows every tab", () => {
    expect(mobileTabsButtonBadge(1, 1, format)).toEqual({ kind: "add" });
    expect(mobileTabsButtonBadge(4, 4, format)).toEqual({ kind: "add" });
  });

  it("counts the tabs past the bar and caps large counts", () => {
    expect(mobileTabsButtonBadge(5, 4, format)).toEqual({ kind: "more", label: "+1" });
    expect(mobileTabsButtonBadge(103, 4, format)).toEqual({ kind: "more", label: "+99" });
    expect(mobileTabsButtonBadge(1_004, 4, (value) => new Intl.NumberFormat("es").format(value))).toEqual({ kind: "more", label: "+99" });
  });
});

describe("mobileTabBarDropMoves", () => {
  function state(order: readonly string[], activeTabId: string): WorkspaceTabsState {
    return {
      tabs: order.map((id) => createWorkspaceTab(id, kindLocation("calendar"))),
      activeTabId,
      closed: [],
      splits: [],
    };
  }

  /** Applies the drop to a real tab list and returns the order and what the bar then shows. */
  function drop(order: readonly string[], activeTabId: string, id: string, targetIndex: number) {
    const before = state(order, activeTabId);
    const slots = mobileTabBarSlots(before.tabs, activeTabId);
    const after = mobileTabBarDropMoves(before.tabs, slots, id, targetIndex)
      .reduce((current, move) => moveWorkspaceTab(current, move.id, move.targetId, move.placement), before);
    return { order: ids(after.tabs), bar: ids(mobileTabBarSlots(after.tabs, activeTabId)) };
  }

  it("moves a tab forward or back among the leading tabs and leaves the rest alone", () => {
    expect(drop(["a", "b", "c", "d", "e"], "a", "a", 2)).toEqual({ order: ["b", "c", "a", "d", "e"], bar: ["b", "c", "a", "d"] });
    expect(drop(["a", "b", "c", "d", "e"], "a", "d", 0)).toEqual({ order: ["d", "a", "b", "c", "e"], bar: ["d", "a", "b", "c"] });
  });

  it("keeps the dropped tab on the bar when the active tab borrows the last slot", () => {
    expect(drop(["a", "b", "c", "d", "e", "f"], "f", "a", 3)).toEqual({
      order: ["b", "c", "f", "a", "d", "e"],
      bar: ["b", "c", "f", "a"],
    });
    expect(drop(["a", "b", "c", "d", "e", "f"], "f", "f", 0)).toEqual({
      order: ["f", "a", "b", "c", "d", "e"],
      bar: ["f", "a", "b", "c"],
    });
  });

  it("does nothing for a drop in place or an unknown tab or slot", () => {
    const order = ["a", "b", "c"];
    const slots = order.map((id) => ({ id }));
    expect(mobileTabBarDropMoves(slots, slots, "b", 1)).toEqual([]);
    expect(mobileTabBarDropMoves(slots, slots, "missing", 0)).toEqual([]);
    expect(mobileTabBarDropMoves(slots, slots, "a", 5)).toEqual([]);
  });
});
