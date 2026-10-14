import { describe, expect, it } from "vitest";
import {
  compareSidebarItems,
  customOrderRanks,
  isSidebarSortOrder,
  mergeReorderedSubset,
  movedToIndex,
  parseCustomOrders,
  parseSidebarSortOrderOverrides,
  parseStarredIds,
  placedNextTo,
  resolveSidebarSortOrder,
  sameStarredSiblings,
  SIDEBAR_SORT_ORDERS,
  sortSidebarItems,
  starredFirst,
  starredSiblingGroups,
  steppedBy,
  withCustomSiblingOrder,
  withSidebarSortOrderOverride,
  withStarredId,
  type SidebarSortFields,
} from "./sidebar-order";

const early = "2026-07-08T12:00:00.000Z";
const middle = "2026-07-09T12:00:00.000Z";
const late = "2026-07-10T12:00:00.000Z";

const items: SidebarSortFields[] = [
  { id: "b", name: "beta", createdAt: late, modifiedAt: early },
  { id: "a", name: "Alpha 10", createdAt: early, modifiedAt: middle },
  { id: "c", name: "alpha 9", createdAt: middle, modifiedAt: late },
];

function idsFor(order: (typeof SIDEBAR_SORT_ORDERS)[number], starred?: (item: SidebarSortFields) => boolean): string[] {
  return sortSidebarItems(items, order, (item) => item, starred).map((item) => item.id);
}

describe("sidebar order", () => {
  it("orders names case-insensitively with numbers by value", () => {
    expect(idsFor("name-asc")).toEqual(["c", "a", "b"]);
    expect(idsFor("name-desc")).toEqual(["b", "a", "c"]);
  });

  it("orders by modified or created time in either direction", () => {
    expect(idsFor("modified-desc")).toEqual(["c", "a", "b"]);
    expect(idsFor("modified-asc")).toEqual(["b", "a", "c"]);
    expect(idsFor("created-desc")).toEqual(["b", "c", "a"]);
    expect(idsFor("created-asc")).toEqual(["a", "c", "b"]);
  });

  it("breaks time ties by name and name ties by id", () => {
    const sameTime = { createdAt: early, modifiedAt: early };
    expect(compareSidebarItems({ id: "x", name: "B", ...sameTime }, { id: "y", name: "a", ...sameTime }, "created-desc"))
      .toBeGreaterThan(0);
    expect(compareSidebarItems({ id: "x", name: "Same", ...sameTime }, { id: "y", name: "same", ...sameTime }, "name-desc"))
      .toBeLessThan(0);
  });

  it("puts starred items first and keeps the order within each part", () => {
    expect(idsFor("name-asc", (item) => item.id === "b")).toEqual(["b", "c", "a"]);
    expect(idsFor("created-asc", (item) => item.id !== "a")).toEqual(["c", "b", "a"]);
    expect(starredFirst(["x", "y", "z"], (id) => id === "z")).toEqual(["z", "x", "y"]);
  });

  it("accepts only known orders", () => {
    for (const order of SIDEBAR_SORT_ORDERS) expect(isSidebarSortOrder(order)).toBe(true);
    expect(isSidebarSortOrder("manual")).toBe(false);
    expect(isSidebarSortOrder(undefined)).toBe(false);
  });

  it("uses a project order over the global one only when it is set", () => {
    expect(resolveSidebarSortOrder("name-asc", "created-desc")).toBe("created-desc");
    expect(resolveSidebarSortOrder("name-desc", null)).toBe("name-desc");
    expect(resolveSidebarSortOrder("name-desc", undefined)).toBe("name-desc");
  });

  it("parses and edits stored project orders defensively", () => {
    expect(parseSidebarSortOrderOverrides({ one: "name-desc", two: "manual", " ": "name-asc", three: 3 }))
      .toEqual({ one: "name-desc" });
    expect(parseSidebarSortOrderOverrides(["name-asc"])).toEqual({});
    expect(parseSidebarSortOrderOverrides(null)).toEqual({});
    expect(withSidebarSortOrderOverride({ one: "name-desc" }, "two", "created-asc"))
      .toEqual({ one: "name-desc", two: "created-asc" });
    expect(withSidebarSortOrderOverride({ one: "name-desc" }, "one", null)).toEqual({});
  });

  it("parses and edits starred ids defensively", () => {
    expect(parseStarredIds(["a", "a", "", " ", 3, "b"])).toEqual(["a", "b"]);
    expect(parseStarredIds("a")).toEqual([]);
    expect(withStarredId(["a", "b"], "b", true)).toEqual(["b", "a"]);
    expect(withStarredId(["a", "b"], "a", false)).toEqual(["b"]);
  });

  it("orders by custom rank, putting unranked items after the ranked ones by name", () => {
    const ranks = customOrderRanks(["b", "a"]);
    const ranked = (item: SidebarSortFields) => ({ ...item, rank: ranks.get(item.id) });
    expect(sortSidebarItems(items, "custom", ranked).map((item) => item.id)).toEqual(["b", "a", "c"]);
    expect(sortSidebarItems(items, "custom", (item) => item).map((item) => item.id)).toEqual(["c", "a", "b"]);
    expect(sortSidebarItems(items, "custom", ranked, (item) => item.id === "c").map((item) => item.id))
      .toEqual(["c", "b", "a"]);
  });

  it("rewrites a custom order for one group of siblings without disturbing the others", () => {
    expect(withCustomSiblingOrder(["x", "a", "y", "b"], ["b", "a", "c"])).toEqual(["b", "a", "c", "x", "y"]);
    expect(withCustomSiblingOrder([], ["a"])).toEqual(["a"]);
  });

  it("writes a reordered subset back into the slots it held", () => {
    expect(mergeReorderedSubset(["a", "hidden", "b", "c"], ["c", "a", "b"])).toEqual(["c", "hidden", "a", "b"]);
    expect(mergeReorderedSubset(["a", "b"], [])).toEqual(["a", "b"]);
  });

  it("moves an id to a clamped index", () => {
    expect(movedToIndex(["a", "b", "c"], "a", 2)).toEqual(["b", "c", "a"]);
    expect(movedToIndex(["a", "b", "c"], "c", 0)).toEqual(["c", "a", "b"]);
    expect(movedToIndex(["a", "b", "c"], "b", 9)).toEqual(["a", "c", "b"]);
    expect(movedToIndex(["a", "b"], "z", 0)).toEqual(["a", "b"]);
  });

  it("places an id next to a sibling and reports drops that change nothing", () => {
    expect(placedNextTo(["a", "b", "c"], "c", "a", "before")).toEqual(["c", "a", "b"]);
    expect(placedNextTo(["a", "b", "c"], "a", "b", "after")).toEqual(["b", "a", "c"]);
    expect(placedNextTo(["a", "b", "c"], "a", "b", "before")).toBeNull();
    expect(placedNextTo(["a", "b"], "a", "a", "after")).toBeNull();
    expect(placedNextTo(["a", "b"], "a", "z", "after")).toBeNull();
  });

  it("steps an id one place and stops at either end", () => {
    expect(steppedBy(["a", "b", "c"], "b", -1)).toEqual(["b", "a", "c"]);
    expect(steppedBy(["a", "b", "c"], "b", 1)).toEqual(["a", "c", "b"]);
    expect(steppedBy(["a", "b"], "a", -1)).toBeNull();
    expect(steppedBy(["a", "b"], "b", 1)).toBeNull();
    expect(steppedBy(["a", "b"], "z", 1)).toBeNull();
  });

  it("parses stored custom orders defensively", () => {
    expect(parseCustomOrders({ one: ["a", "a", "b"], two: [], three: "a", " ": ["c"] })).toEqual({ one: ["a", "b"] });
    expect(parseCustomOrders(["a"])).toEqual({});
  });

  it("groups ids by list and starred state", () => {
    const groups = starredSiblingGroups([["a", "b", "c"], ["d"]], (id) => id === "b");
    expect(groups.get("a")).toEqual(["a", "c"]);
    expect(groups.get("b")).toEqual(["b"]);
    expect(groups.get("d")).toEqual(["d"]);
    expect(groups.get("z")).toBeUndefined();
  });

  it("lists only the siblings that share an item's starred state", () => {
    const starred = (id: string) => id === "a" || id === "c";
    expect(sameStarredSiblings(["a", "b", "c", "d"], "c", starred)).toEqual(["a", "c"]);
    expect(sameStarredSiblings(["a", "b", "c", "d"], "d", starred)).toEqual(["b", "d"]);
    expect(sameStarredSiblings(["a", "b"], "b", () => false)).toEqual(["a", "b"]);
  });
});
