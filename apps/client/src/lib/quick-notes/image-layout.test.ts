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
      maxRowCrop: 0,
    });
    expect(rows).toEqual([[0], [1], [2]]);
  });
});

describe("Quick note image mosaic", () => {
  it("shows a lone portrait whole at the tallest row height instead of cropping it to fill the width", () => {
    expect(quickNoteImageMosaic([size(750, 1000)], QUICK_NOTE_EDITOR_IMAGE_ROWS)).toEqual([{
      tiles: [{ index: 0, aspect: 0.75, share: 1 }],
      width: 0.75,
    }]);
    expect(quickNoteImageMosaic([size(1080, 2340)], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([{
      tiles: [{ index: 0, aspect: 1080 / 2340, share: 1 }],
      width: expect.closeTo(1080 / 2340 / 0.75),
    }]);
  });

  it("crops a row only slightly to fill the width when that is enough", () => {
    expect(quickNoteImageMosaic([size(940, 1000)], QUICK_NOTE_EDITOR_IMAGE_ROWS)).toEqual([{
      tiles: [{ index: 0, aspect: expect.closeTo(1), share: 1 }],
      width: 1,
    }]);
  });

  it("splits every row width in proportion to its tile shapes", () => {
    const rows = quickNoteImageMosaic(
      [size(500, 1000), size(1000, 4000), size(1600, 1200), size(900, 1000), size(3000, 1000)],
      QUICK_NOTE_CARD_IMAGE_ROWS,
    );
    for (const { tiles } of rows) {
      expect(tiles.reduce((sum, tile) => sum + tile.share, 0)).toBeCloseTo(1);
      const first = tiles[0];
      for (const tile of tiles) {
        if (first) expect(tile.share / tile.aspect).toBeCloseTo(first.share / first.aspect);
      }
    }
  });

  it("never crops a tile by more than the allowance nor lets a row grow past the tallest height", () => {
    const images = [size(500, 1000), size(1000, 4000), size(750, 1000), size(1600, 1200), size(1080, 2340)];
    for (const options of [QUICK_NOTE_CARD_IMAGE_ROWS, QUICK_NOTE_EDITOR_IMAGE_ROWS]) {
      for (const { tiles, width } of quickNoteImageMosaic(images, options)) {
        expect(width).toBeLessThanOrEqual(1);
        const rowAspect = tiles.reduce((sum, tile) => sum + tile.aspect, 0);
        expect(rowAspect / width).toBeGreaterThanOrEqual(options.minRowAspect - 1e-9);
        for (const tile of tiles) {
          const image = images[tile.index];
          if (image) expect(tile.aspect).toBeLessThanOrEqual(quickNoteImageAspect(image) / (1 - options.maxRowCrop) + 1e-9);
        }
      }
    }
  });

  it("keeps the natural shapes of rows that are wide enough", () => {
    expect(quickNoteImageMosaic([size(1600, 1200), size(1200, 1200)], QUICK_NOTE_CARD_IMAGE_ROWS)).toEqual([{
      tiles: [
        { index: 0, aspect: 4 / 3, share: expect.closeTo(4 / 7) },
        { index: 1, aspect: 1, share: expect.closeTo(3 / 7) },
      ],
      width: 1,
    }]);
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
