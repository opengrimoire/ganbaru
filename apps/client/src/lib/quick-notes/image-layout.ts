import type { QuickNoteImage } from "$lib/quick-notes/types";

/** Narrowest tile shape a mosaic shows; taller images are cropped to it. */
export const QUICK_NOTE_IMAGE_MIN_ASPECT = 0.5;
/** Widest tile shape a mosaic shows; wider images are cropped to it. */
export const QUICK_NOTE_IMAGE_MAX_ASPECT = 3;
/** Images a card shows before the last tile counts the rest. */
export const QUICK_NOTE_CARD_IMAGE_LIMIT = 6;

/** How a mosaic breaks images into rows. */
export interface QuickNoteImageRowOptions {
  /** Summed width-to-height ratio a row aims for; higher values make shorter rows. */
  targetRowAspect: number;
  maxPerRow: number;
  /** Lowest summed ratio a row is shown at; rows of tall images are cropped to it instead of growing taller. */
  minRowAspect: number;
}

/** Cards are narrow, so their rows hold fewer, taller images. */
export const QUICK_NOTE_CARD_IMAGE_ROWS: QuickNoteImageRowOptions = Object.freeze({
  targetRowAspect: 2.4,
  maxPerRow: 3,
  minRowAspect: 0.75,
});

/** The editor is wider, so its rows hold more images. */
export const QUICK_NOTE_EDITOR_IMAGE_ROWS: QuickNoteImageRowOptions = Object.freeze({
  targetRowAspect: 2.8,
  maxPerRow: 3,
  minRowAspect: 1,
});

/**
 * One tile of a mosaic row: the image index, the width-to-height ratio its tile is shown at, and the fraction of
 * the row width it takes. The shares of a row sum to one, so the row always spans the full width.
 */
export interface QuickNoteImageTileLayout {
  index: number;
  aspect: number;
  share: number;
}

/** Width-to-height ratio of an image's tile, bounded so panoramas and tall strips still read as images. */
export function quickNoteImageAspect(image: Pick<QuickNoteImage, "width" | "height">): number {
  if (!(image.width > 0) || !(image.height > 0)) return 1;
  return Math.min(QUICK_NOTE_IMAGE_MAX_ASPECT, Math.max(QUICK_NOTE_IMAGE_MIN_ASPECT, image.width / image.height));
}

/**
 * Splits images, in order, into justified rows whose heights stay close to the target.
 *
 * Every row spans the full mosaic width, so its height is the width divided by the sum of its tile aspects. The
 * split minimizes the squared log ratio between each row's summed aspect and the target, which treats a row twice
 * as tall as wanted the same as one half as tall. Ties keep the earlier break, so the result is deterministic.
 *
 * @returns The image indexes of each row.
 */
export function quickNoteImageRows(
  aspects: readonly number[],
  options: QuickNoteImageRowOptions,
): number[][] {
  const count = aspects.length;
  if (count === 0) return [];
  const maxPerRow = Math.max(1, Math.floor(options.maxPerRow));
  const target = options.targetRowAspect > 0 ? options.targetRowAspect : 1;
  const safe = aspects.map((aspect) => (Number.isFinite(aspect) && aspect > 0 ? aspect : 1));
  // best[end] is the lowest cost of laying out images [0, end); start[end] is where its last row begins.
  const best = Array.from({ length: count + 1 }, () => Number.POSITIVE_INFINITY);
  const start = Array.from({ length: count + 1 }, () => 0);
  best[0] = 0;
  for (let end = 1; end <= count; end += 1) {
    let rowAspect = 0;
    for (let first = end - 1; first >= Math.max(0, end - maxPerRow); first -= 1) {
      rowAspect += safe[first] ?? 1;
      const deviation = Math.log(rowAspect / target);
      const cost = (best[first] ?? Number.POSITIVE_INFINITY) + deviation * deviation;
      if (cost < (best[end] ?? Number.POSITIVE_INFINITY)) {
        best[end] = cost;
        start[end] = first;
      }
    }
  }
  const rows: number[][] = [];
  for (let end = count; end > 0; end = start[end] ?? 0) {
    const first = start[end] ?? 0;
    rows.unshift(Array.from({ length: end - first }, (_, offset) => first + offset));
  }
  return rows;
}

/**
 * Lays out images as justified rows of tiles. A row whose summed ratio is below `minRowAspect` widens every tile by
 * the same factor, so one tall image fills the width at a bounded height and its tile crops it.
 */
export function quickNoteImageMosaic(
  images: readonly Pick<QuickNoteImage, "width" | "height">[],
  options: QuickNoteImageRowOptions,
): QuickNoteImageTileLayout[][] {
  const aspects = images.map(quickNoteImageAspect);
  const minimum = options.minRowAspect > 0 ? options.minRowAspect : 0;
  return quickNoteImageRows(aspects, options).map((row) => {
    const sum = row.reduce((total, index) => total + (aspects[index] ?? 1), 0);
    const widen = sum > 0 && sum < minimum ? minimum / sum : 1;
    return row.map((index) => {
      const aspect = aspects[index] ?? 1;
      return { index, aspect: aspect * widen, share: sum > 0 ? aspect / sum : 1 / row.length };
    });
  });
}

/** The images a card shows and how many it leaves out. */
export interface QuickNoteCardImagePreview<T> {
  shown: T[];
  hidden: number;
}

/** Keeps the first images a card has room for; the last shown tile reports the rest. */
export function quickNoteCardImagePreview<T>(
  images: readonly T[],
  limit = QUICK_NOTE_CARD_IMAGE_LIMIT,
): QuickNoteCardImagePreview<T> {
  const bounded = Math.max(1, Math.floor(limit));
  return { shown: images.slice(0, bounded), hidden: Math.max(0, images.length - bounded) };
}
