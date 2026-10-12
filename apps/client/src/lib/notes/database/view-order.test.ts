import { describe, expect, it } from "vitest";
import { moveNotesDatabaseView, notesDatabaseViewDropPlacement, orderNotesDatabaseViews, stepNotesDatabaseView } from "./view-order";

const ids = ["a", "b", "c", "d"];

describe("moveNotesDatabaseView", () => {
  it("moves a view before or after a later view", () => {
    expect(moveNotesDatabaseView(ids, "a", "c", "before")).toEqual(["b", "a", "c", "d"]);
    expect(moveNotesDatabaseView(ids, "a", "c", "after")).toEqual(["b", "c", "a", "d"]);
  });

  it("moves a view to the start or the end", () => {
    expect(moveNotesDatabaseView(ids, "d", "a", "before")).toEqual(["d", "a", "b", "c"]);
    expect(moveNotesDatabaseView(ids, "a", "d", "after")).toEqual(["b", "c", "d", "a"]);
  });

  it("reports no change when the view lands where it already is", () => {
    expect(moveNotesDatabaseView(ids, "b", "c", "before")).toBeNull();
    expect(moveNotesDatabaseView(ids, "c", "b", "after")).toBeNull();
    expect(moveNotesDatabaseView(ids, "b", "b", "after")).toBeNull();
  });

  it("ignores views that are not in the order", () => {
    expect(moveNotesDatabaseView(ids, "x", "a", "before")).toBeNull();
    expect(moveNotesDatabaseView(ids, "a", "x", "after")).toBeNull();
  });

  it("does not mutate the original order", () => {
    const original = [...ids];
    moveNotesDatabaseView(original, "a", "d", "after");
    expect(original).toEqual(ids);
  });
});

describe("stepNotesDatabaseView", () => {
  it("swaps a view with its visible neighbor", () => {
    expect(stepNotesDatabaseView(ids, ids, "b", 1)).toEqual(["a", "c", "b", "d"]);
    expect(stepNotesDatabaseView(ids, ids, "b", -1)).toEqual(["b", "a", "c", "d"]);
  });

  it("skips hidden views and keeps them in place", () => {
    expect(stepNotesDatabaseView(ids, ["a", "c", "d"], "a", 1)).toEqual(["b", "c", "a", "d"]);
  });

  it("stops at either end of the visible list", () => {
    expect(stepNotesDatabaseView(ids, ids, "a", -1)).toBeNull();
    expect(stepNotesDatabaseView(ids, ids, "d", 1)).toBeNull();
    expect(stepNotesDatabaseView(ids, ["a", "b"], "c", 1)).toBeNull();
  });
});

describe("notesDatabaseViewDropPlacement", () => {
  const rect = { left: 100, top: 20, width: 80, height: 30 };

  it("places before a tab when the pointer is on its leading half", () => {
    expect(notesDatabaseViewDropPlacement(rect, { x: 120, y: 0 }, "x")).toBe("before");
    expect(notesDatabaseViewDropPlacement(rect, { x: 160, y: 0 }, "x")).toBe("after");
  });

  it("uses the vertical midpoint for list rows", () => {
    expect(notesDatabaseViewDropPlacement(rect, { x: 0, y: 30 }, "y")).toBe("before");
    expect(notesDatabaseViewDropPlacement(rect, { x: 0, y: 40 }, "y")).toBe("after");
  });
});

describe("orderNotesDatabaseViews", () => {
  const views = ids.map((id) => ({ id }));

  it("arranges views by the requested order", () => {
    expect(orderNotesDatabaseViews(views, ["c", "a", "d", "b"]).map((view) => view.id)).toEqual(["c", "a", "d", "b"]);
  });

  it("keeps views the order does not name after the named ones, in their current sequence", () => {
    expect(orderNotesDatabaseViews(views, ["d", "b"]).map((view) => view.id)).toEqual(["d", "b", "a", "c"]);
  });

  it("ignores requested ids that no longer exist", () => {
    expect(orderNotesDatabaseViews(views.slice(0, 2), ["x", "b", "a"]).map((view) => view.id)).toEqual(["b", "a"]);
  });
});
