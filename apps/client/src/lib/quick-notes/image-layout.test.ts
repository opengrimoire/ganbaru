import { describe, expect, it } from "vitest";
import {
  QUICK_NOTE_CARD_IMAGE_ROWS,
  QUICK_NOTE_EDITOR_IMAGE_ROWS,
  QUICK_NOTE_IMAGE_MAX_ASPECT,
  QUICK_NOTE_IMAGE_MIN_ASPECT,
  quickNoteCardImagePreview,
  quickNoteImageAspect,
  quickNoteImageMosaic,
  quickNoteImageRows,
} from "./image-layout";

const size = (width: number, height: number) => ({ width, height });

describe("Quick note image aspect", () => {
  it("keeps ordinary shapes and bounds panoramas and tall strips", () => {
    expect(quickNoteImageAspect(size(1600, 1200))).toBeCloseTo(4 / 3);
    expect(quickNoteImageAspect(size(8000, 1000))).toBe(QUICK_NOTE_IMAGE_MAX_ASPECT);
    expect(quickNoteImageAspect(size(400, 4000))).toBe(QUICK_NOTE_IMAGE_MIN_ASPECT);
  });

  it("treats missing or invalid sizes as square", () => {
    expect(quickNoteImageAspect(size(0, 300))).toBe(1);
    expect(quickNoteImageAspect(size(300, Number.NaN))).toBe(1);
    expect(quickNoteImageAspect(size(-5, 10))).toBe(1);
  });
});

describe("Quick note image rows", () => {
  it("returns no rows without images", () => {
    expect(quickNoteImageRows([], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([]);
  });

  it("keeps a single image on its own row", () => {
    expect(quickNoteImageRows([0.5], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([[0]]);
  });

  it("balances squares into rows near the target and breaks ties at the earlier row", () => {
    expect(quickNoteImageRows([1, 1, 1, 1, 1], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([[0, 1, 2], [3, 4]]);
  });

  it("gives a panorama its own row instead of squeezing it next to others", () => {
    expect(quickNoteImageRows([1, 1, 3, 1.2, 1.2], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([[0, 1], [2], [3, 4]]);
  });

  it("never puts more images on a row than allowed and keeps their order", () => {
    const rows = quickNoteImageRows(Array.from({ length: 11 }, () => 0.5), QUICK_NOTE_CARD_IMAGE_ROWS);
    expect(rows.every((row) => row.length <= QUICK_NOTE_CARD_IMAGE_ROWS.maxPerRow)).toBe(true);
    expect(rows.flat()).toEqual(Array.from({ length: 11 }, (_, index) => index));
  });

  it("treats invalid aspects and options as safe defaults", () => {
    const rows = quickNoteImageRows([Number.NaN, -1, Number.POSITIVE_INFINITY], {
      targetRowAspect: 0,
      maxPerRow: 0,
      minRowAspect: 0,
    });
    expect(rows).toEqual([[0], [1], [2]]);
  });
});

describe("Quick note image mosaic", () => {
  it("widens a lone tall image so its tile is not taller than the minimum row shape", () => {
    expect(quickNoteImageMosaic([size(500, 1000)], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([[{ index: 0, aspect: 0.75 }]]);
    expect(quickNoteImageMosaic([size(500, 1000)], QUICK_NOTE_EDITOR_IMAGE_ROWS)).toEqual([[{ index: 0, aspect: 1 }]]);
  });

  it("widens every tile of a short row by the same factor", () => {
    const [row] = quickNoteImageMosaic([size(500, 1000), size(1000, 4000)], QUICK_NOTE_EDITOR_IMAGE_ROWS);
    expect(row?.map((tile) => tile.index)).toEqual([0, 1]);
    const total = row?.reduce((sum, tile) => sum + tile.aspect, 0) ?? 0;
    expect(total).toBeCloseTo(QUICK_NOTE_EDITOR_IMAGE_ROWS.minRowAspect);
    expect(row?.[0]?.aspect).toBeCloseTo(row?.[1]?.aspect ?? 0);
  });

  it("keeps the natural shapes of rows that are wide enough", () => {
    expect(quickNoteImageMosaic([size(1600, 1200), size(1200, 1200)], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([[
      { index: 0, aspect: 4 / 3 },
      { index: 1, aspect: 1 },
    ]]);
  });
});

describe("Quick note card image preview", () => {
  it("shows the first images and counts the rest", () => {
    expect(quickNoteCardImagePreview([1, 2, 3, 4, 5, 6, 7, 8])).toEqual({ shown: [1, 2, 3, 4, 5, 6], hidden: 2 });
    expect(quickNoteCardImagePreview([1, 2, 3])).toEqual({ shown: [1, 2, 3], hidden: 0 });
  });

  it("always shows at least one image", () => {
    expect(quickNoteCardImagePreview([1, 2], 0)).toEqual({ shown: [1], hidden: 1 });
  });
});
