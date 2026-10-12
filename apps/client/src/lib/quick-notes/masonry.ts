export const QUICK_NOTE_MASONRY_MIN_CARD_WIDTH = 210;
/** Mobile cards leave their actions to the editor, so two fit side by side on common phones. */
export const QUICK_NOTE_MOBILE_MASONRY_MIN_CARD_WIDTH = 140;
export const QUICK_NOTE_MASONRY_GAP = 12;
export const QUICK_NOTE_MOBILE_WIDE_MIN_WIDTH = 900;

export interface MasonryPosition {
  left: number;
  top: number;
  width: number;
}

export interface MasonryLayout {
  positions: MasonryPosition[];
  height: number;
  columns: number;
}

export interface MasonryInsertion {
  index: number;
  distanceSquared: number;
}

export interface QuickNoteMasonryDensity {
  minimumCardWidth: number;
  maximumColumns: number;
}

/** Desktop fits as many standard-width columns as the container allows. */
export const QUICK_NOTE_DESKTOP_MASONRY_DENSITY: QuickNoteMasonryDensity = {
  minimumCardWidth: QUICK_NOTE_MASONRY_MIN_CARD_WIDTH,
  maximumColumns: Number.POSITIVE_INFINITY,
};

/** Bound mobile density to two compact columns, or three in wide windows. */
export function quickNoteMobileMasonryDensity(containerWidth: number): QuickNoteMasonryDensity {
  const wide = Number.isFinite(containerWidth) && containerWidth >= QUICK_NOTE_MOBILE_WIDE_MIN_WIDTH;
  return {
    minimumCardWidth: QUICK_NOTE_MOBILE_MASONRY_MIN_CARD_WIDTH,
    maximumColumns: wide ? 3 : 2,
  };
}

export function quickNoteMasonryLayout(
  containerWidth: number,
  heights: readonly number[],
  minimumCardWidth = QUICK_NOTE_MASONRY_MIN_CARD_WIDTH,
  gap = QUICK_NOTE_MASONRY_GAP,
  maximumColumns = Number.POSITIVE_INFINITY,
): MasonryLayout {
  const safeWidth = Math.max(0, containerWidth);
  const boundedMaximum = Number.isFinite(maximumColumns)
    ? Math.max(1, Math.floor(maximumColumns))
    : Number.POSITIVE_INFINITY;
  const columns = Math.min(
    boundedMaximum,
    Math.max(1, Math.floor((safeWidth + gap) / (minimumCardWidth + gap))),
  );
  const cardWidth = columns === 1 ? safeWidth : (safeWidth - gap * (columns - 1)) / columns;
  const columnHeights = Array.from({ length: columns }, () => 0);
  const positions = heights.map((height) => {
    let column = 0;
    for (let index = 1; index < columnHeights.length; index += 1) {
      if ((columnHeights[index] ?? 0) < (columnHeights[column] ?? 0)) column = index;
    }
    const top = columnHeights[column] ?? 0;
    columnHeights[column] = top + Math.max(0, height) + gap;
    return { left: column * (cardWidth + gap), top, width: cardWidth };
  });
  return {
    positions,
    height: Math.max(0, ...columnHeights) - (positions.length > 0 ? gap : 0),
    columns,
  };
}

/** Find the source-order slot whose masonry position best matches a dragged card. */
export function quickNoteMasonryInsertion(
  containerWidth: number,
  heights: readonly number[],
  draggedIndex: number,
  targetLeft: number,
  targetTop: number,
  preferredIndex = draggedIndex,
  density = QUICK_NOTE_DESKTOP_MASONRY_DENSITY,
): MasonryInsertion {
  if (draggedIndex < 0 || draggedIndex >= heights.length) {
    return { index: 0, distanceSquared: Number.POSITIVE_INFINITY };
  }
  const draggedHeight = heights[draggedIndex] ?? 0;
  const remaining = heights.filter((_, index) => index !== draggedIndex);
  let best: MasonryInsertion = { index: 0, distanceSquared: Number.POSITIVE_INFINITY };
  for (let index = 0; index <= remaining.length; index += 1) {
    const candidate = [...remaining.slice(0, index), draggedHeight, ...remaining.slice(index)];
    const position = quickNoteMasonryLayout(
      containerWidth,
      candidate,
      density.minimumCardWidth,
      QUICK_NOTE_MASONRY_GAP,
      density.maximumColumns,
    ).positions[index];
    if (!position) continue;
    const distanceSquared = (position.left - targetLeft) ** 2 + (position.top - targetTop) ** 2;
    const equallyClose = Math.abs(distanceSquared - best.distanceSquared) < 0.01;
    if (distanceSquared < best.distanceSquared || (equallyClose && index === preferredIndex)) {
      best = { index, distanceSquared };
    }
  }
  return best;
}

/** Replace one visible order group without moving notes outside that group. */
export function applyQuickNoteGroupOrder(
  notes: readonly QuickNote[],
  orderedIds: readonly string[],
): QuickNote[] {
  const ids = new Set(orderedIds);
  if (ids.size !== orderedIds.length) return [...notes];
  const byId = new Map(notes.map((note) => [note.id, note]));
  const ordered = orderedIds.map((id) => byId.get(id));
  if (ordered.some((note) => note === undefined)) return [...notes];
  let index = 0;
  return notes.map((note) => ids.has(note.id) ? ordered[index++]! : note);
}

/** Move one id by a single keyboard step and clamp at the group boundary. */
export function moveQuickNoteId(
  orderedIds: readonly string[],
  id: string,
  direction: -1 | 1,
): string[] {
  const source = orderedIds.indexOf(id);
  if (source < 0) return [...orderedIds];
  const destination = Math.max(0, Math.min(orderedIds.length - 1, source + direction));
  if (destination === source) return [...orderedIds];
  const next = [...orderedIds];
  next.splice(source, 1);
  next.splice(destination, 0, id);
  return next;
}
import type { QuickNote } from "$lib/quick-notes/types";
