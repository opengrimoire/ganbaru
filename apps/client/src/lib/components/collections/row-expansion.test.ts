import { describe, expect, it } from "vitest";
import { collectionRowExpanded, toggledCollectionRowExpansion, type CollectionRowExpansion } from "./row-expansion";

const empty: CollectionRowExpansion = { collapsedIds: new Set(), openedIds: new Set() };

describe("collection row expansion", () => {
  it("opens rows with children and closes rows without children by default", () => {
    expect(collectionRowExpanded("a", true, empty)).toBe(true);
    expect(collectionRowExpanded("a", false, empty)).toBe(false);
  });

  it("collapses a row with children and opens it again", () => {
    const collapsed = toggledCollectionRowExpansion("a", true, empty);
    expect(collectionRowExpanded("a", true, collapsed)).toBe(false);
    expect([...collapsed.collapsedIds]).toEqual(["a"]);
    const reopened = toggledCollectionRowExpansion("a", true, collapsed);
    expect(collectionRowExpanded("a", true, reopened)).toBe(true);
    expect(reopened.collapsedIds.size).toBe(0);
  });

  it("opens a row without children without recording it as collapsed when closed", () => {
    const opened = toggledCollectionRowExpansion("a", false, empty);
    expect(collectionRowExpanded("a", false, opened)).toBe(true);
    const closed = toggledCollectionRowExpansion("a", false, opened);
    expect(collectionRowExpanded("a", false, closed)).toBe(false);
    expect(closed.collapsedIds.size).toBe(0);
    expect(closed.openedIds.size).toBe(0);
  });

  it("keeps an opened row open after it gains its first child", () => {
    const opened = toggledCollectionRowExpansion("a", false, empty);
    expect(collectionRowExpanded("a", true, opened)).toBe(true);
  });

  it("closes a collapsed row whose children were all removed", () => {
    const opened = toggledCollectionRowExpansion("a", false, empty);
    const collapsed = toggledCollectionRowExpansion("a", true, opened);
    expect(collectionRowExpanded("a", false, collapsed)).toBe(false);
  });

  it("leaves other rows untouched", () => {
    const expansion: CollectionRowExpansion = { collapsedIds: new Set(["b"]), openedIds: new Set(["c"]) };
    const next = toggledCollectionRowExpansion("a", true, expansion);
    expect([...next.collapsedIds].sort()).toEqual(["a", "b"]);
    expect([...next.openedIds]).toEqual(["c"]);
    expect(expansion.collapsedIds.has("a")).toBe(false);
  });
});
