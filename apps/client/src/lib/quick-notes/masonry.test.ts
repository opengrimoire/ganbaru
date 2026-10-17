import { describe, expect, it } from "vitest";
import type { QuickNote } from "./types";
import {
  applyQuickNoteGroupOrder,
  moveQuickNoteId,
  quickNoteMobileMasonryDensity,
  quickNoteMasonryInsertion,
  quickNoteMasonryLayout,
} from "./masonry";

describe("Quick note masonry layout", () => {
  it("uses the shortest available column and reports the container height", () => {
    const layout = quickNoteMasonryLayout(654, [100, 200, 50, 80], 210, 12);
    expect(layout.columns).toBe(3);
    expect(layout.positions[3]?.top).toBe(62);
    expect(layout.height).toBe(200);
  });

  it("falls back to one column when the container is narrow", () => {
    const layout = quickNoteMasonryLayout(200, [40, 50], 210, 12);
    expect(layout.columns).toBe(1);
    expect(layout.positions[1]?.top).toBe(52);
    expect(layout.height).toBe(102);
  });

  it("limits mobile density to two columns below the wide breakpoint", () => {
    expect(quickNoteMobileMasonryDensity(360).maximumColumns).toBe(2);
    expect(quickNoteMobileMasonryDensity(899).maximumColumns).toBe(2);
    expect(quickNoteMobileMasonryDensity(900).maximumColumns).toBe(3);
    expect(quickNoteMobileMasonryDensity(Number.NaN).maximumColumns).toBe(2);
  });

  it("fits two compact mobile columns on phones and one at the recoverability floor", () => {
    const layoutAt = (containerWidth: number) => {
      const density = quickNoteMobileMasonryDensity(containerWidth);
      return quickNoteMasonryLayout(containerWidth, [40, 40, 40], density.minimumCardWidth, 12, density.maximumColumns);
    };
    expect(layoutAt(296).columns).toBe(2);
    expect(layoutAt(296).positions[1]?.width).toBe(142);
    expect(layoutAt(700).columns).toBe(2);
    expect(layoutAt(256).columns).toBe(1);
    expect(quickNoteMasonryLayout(296, [40, 40, 40]).columns).toBe(1);
  });

  it("targets drag insertion against the same mobile layout it displays", () => {
    const density = quickNoteMobileMasonryDensity(336);
    const heights = [80, 120, 60, 90];
    const target = quickNoteMasonryLayout(336, [80, 60, 120, 90], density.minimumCardWidth, 12, density.maximumColumns).positions[1];
    const insertion = quickNoteMasonryInsertion(336, heights, 2, target?.left ?? 0, target?.top ?? 0, 2, density);
    expect(target?.left).toBeGreaterThan(0);
    expect(insertion.index).toBe(1);
    expect(insertion.distanceSquared).toBe(0);
  });

  it("respects an explicit mobile column limit", () => {
    expect(quickNoteMasonryLayout(700, [40, 40, 40], 210, 12, 1).columns).toBe(1);
    expect(quickNoteMasonryLayout(1_000, [40, 40, 40], 210, 12, 2).columns).toBe(2);
  });

  it("chooses the insertion whose dragged top-left matches an uneven masonry slot", () => {
    const heights = [180, 80, 140, 60];
    const target = quickNoteMasonryLayout(432, [180, 140, 80, 60]).positions[2];
    const insertion = quickNoteMasonryInsertion(
      432,
      heights,
      1,
      target?.left ?? 0,
      target?.top ?? 0,
    );
    expect(insertion.index).toBe(2);
    expect(insertion.distanceSquared).toBe(0);
  });

  it("keeps the preferred slot when masonry candidates are equally close", () => {
    const insertion = quickNoteMasonryInsertion(0, [40, 40], 1, 0, 26, 1);
    expect(insertion.index).toBe(1);
  });
});

describe("Quick note group ordering", () => {
  const notes = ["pinned-a", "pinned-b", "other-a", "other-b"]
    .map((id) => ({ id }) as QuickNote);

  it("reorders only the requested group slots", () => {
    expect(applyQuickNoteGroupOrder(notes, ["other-b", "other-a"]).map((note) => note.id))
      .toEqual(["pinned-a", "pinned-b", "other-b", "other-a"]);
  });

  it("rejects incomplete or duplicate group identities", () => {
    expect(applyQuickNoteGroupOrder(notes, ["missing"]).map((note) => note.id))
      .toEqual(notes.map((note) => note.id));
    expect(applyQuickNoteGroupOrder(notes, ["other-a", "other-a"]).map((note) => note.id))
      .toEqual(notes.map((note) => note.id));
  });

  it("moves keyboard reorders one slot and clamps at the edges", () => {
    expect(moveQuickNoteId(["a", "b", "c"], "b", -1)).toEqual(["b", "a", "c"]);
    expect(moveQuickNoteId(["a", "b", "c"], "c", 1)).toEqual(["a", "b", "c"]);
  });
});
