import { describe, expect, it } from "vitest";
import {
  adjacentPlacement,
  compareProjectOrder,
  compareTaskSectionOrder,
  dropPlacement,
  placementInOrder,
  projectOrderRanks,
  provisionalOrderKeys,
} from "./order";

describe("compareProjectOrder", () => {
  it("compares order keys by code unit and breaks ties by id", () => {
    const rows = [
      { id: "b", orderKey: "a0" },
      { id: "c", orderKey: "Z" },
      { id: "a", orderKey: "a0" },
      { id: "d", orderKey: "a" },
    ];
    expect(rows.toSorted(compareProjectOrder).map((row) => row.id)).toEqual(["c", "d", "a", "b"]);
  });

  it("does not consult names or locale collation", () => {
    const rows = [
      { id: "x", orderKey: "b", name: "Alpha" },
      { id: "y", orderKey: "B", name: "Zulu" },
    ];
    expect(rows.toSorted(compareProjectOrder).map((row) => row.id)).toEqual(["y", "x"]);
  });
});

describe("compareTaskSectionOrder", () => {
  it("orders tasks by section key, then id", () => {
    const tasks = [
      { id: "t2", sectionOrderKey: "a1" },
      { id: "t1", sectionOrderKey: "a1" },
      { id: "t3", sectionOrderKey: "a0" },
    ];
    expect(tasks.toSorted(compareTaskSectionOrder).map((task) => task.id)).toEqual(["t3", "t1", "t2"]);
  });
});

describe("projectOrderRanks", () => {
  it("ranks rows by native order regardless of input order", () => {
    const ranks = projectOrderRanks([{ id: "b", orderKey: "a2" }, { id: "a", orderKey: "a1" }]);
    expect(ranks.get("a")).toBe(0);
    expect(ranks.get("b")).toBe(1);
  });
});

describe("provisionalOrderKeys", () => {
  it("produces equal-width keys that sort in the given order", () => {
    const ids = Array.from({ length: 40 }, (_, index) => `id-${index}`);
    const keys = provisionalOrderKeys(ids);
    const values = ids.map((id) => keys.get(id)!);
    expect(new Set(values.map((value) => value.length)).size).toBe(1);
    expect(values.toSorted()).toEqual(values);
  });
});

describe("adjacentPlacement", () => {
  const ordered = [{ id: "a" }, { id: "b" }, { id: "c" }];

  it("moves toward the start before the previous sibling", () => {
    expect(adjacentPlacement(ordered, "b", -1)).toEqual({ kind: "before", id: "a" });
  });

  it("moves toward the end after the next sibling", () => {
    expect(adjacentPlacement(ordered, "b", 1)).toEqual({ kind: "after", id: "c" });
  });

  it("returns null at either end and for unknown rows", () => {
    expect(adjacentPlacement(ordered, "a", -1)).toBeNull();
    expect(adjacentPlacement(ordered, "c", 1)).toBeNull();
    expect(adjacentPlacement(ordered, "missing", 1)).toBeNull();
  });
});

describe("dropPlacement", () => {
  it("places a row after its new previous neighbour in ascending lists", () => {
    expect(dropPlacement({ visibleIds: ["a", "b", "c"], draggedId: "x", overId: "b", position: "after", direction: "asc" }))
      .toEqual({ kind: "after", id: "b" });
  });

  it("places a row before the first row when nothing precedes it", () => {
    expect(dropPlacement({ visibleIds: ["a", "b"], draggedId: "x", overId: "a", position: "before", direction: "asc" }))
      .toEqual({ kind: "before", id: "a" });
  });

  it("appends after the last visible row when no target is supplied", () => {
    expect(dropPlacement({ visibleIds: ["a", "b"], draggedId: "x", direction: "asc" })).toEqual({ kind: "after", id: "b" });
  });

  it("appends to the native end of an empty list", () => {
    expect(dropPlacement({ visibleIds: [], draggedId: "x", direction: "asc" })).toEqual({ kind: "end" });
    expect(dropPlacement({ visibleIds: ["x"], draggedId: "x", overId: "x", direction: "desc" })).toBeNull();
  });

  it("swaps before and after in descending lists", () => {
    expect(dropPlacement({ visibleIds: ["c", "b", "a"], draggedId: "x", overId: "b", position: "after", direction: "desc" }))
      .toEqual({ kind: "before", id: "b" });
    expect(dropPlacement({ visibleIds: ["b", "a"], draggedId: "x", overId: "b", position: "before", direction: "desc" }))
      .toEqual({ kind: "after", id: "b" });
  });

  it("ignores the dragged row when finding neighbours within the same list", () => {
    expect(dropPlacement({ visibleIds: ["a", "x", "b"], draggedId: "x", overId: "b", position: "after", direction: "asc" }))
      .toEqual({ kind: "after", id: "b" });
  });

  it("returns null when the drop leaves the row in place", () => {
    expect(dropPlacement({ visibleIds: ["a", "x", "b"], draggedId: "x", overId: "a", position: "after", direction: "asc" })).toBeNull();
    expect(dropPlacement({ visibleIds: ["a", "x", "b"], draggedId: "x", overId: "b", position: "before", direction: "asc" })).toBeNull();
    expect(dropPlacement({ visibleIds: ["a", "b", "x"], draggedId: "x", direction: "asc" })).toBeNull();
  });
});

describe("placementInOrder", () => {
  it("anchors a row to its previous sibling, or its next sibling when first", () => {
    expect(placementInOrder(["a", "b", "c"], "b")).toEqual({ kind: "after", id: "a" });
    expect(placementInOrder(["a", "b", "c"], "a")).toEqual({ kind: "before", id: "b" });
  });

  it("returns null for a lone or missing row", () => {
    expect(placementInOrder(["a"], "a")).toBeNull();
    expect(placementInOrder(["a"], "b")).toBeNull();
  });
});
