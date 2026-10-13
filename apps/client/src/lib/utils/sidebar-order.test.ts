import { describe, expect, it } from "vitest";
import {
  compareSidebarItems,
  isSidebarSortOrder,
  parseSidebarSortOrderOverrides,
  parseStarredIds,
  resolveSidebarSortOrder,
  SIDEBAR_SORT_ORDERS,
  sortSidebarItems,
  starredFirst,
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
});
