export interface NotesMeasuredRangeItem {
  id: string;
  estimatedHeight: number;
}

export interface NotesVisibleRange {
  start: number;
  end: number;
  topHeight: number;
  bottomHeight: number;
  totalHeight: number;
}

export interface NotesVisibleRangeOptions {
  viewportStart: number;
  viewportEnd: number;
  overscanPx: number;
  minimumVirtualizedCount: number;
}

export interface NotesScrollAnchor {
  id: string;
  offset: number;
}

const MIN_ITEM_HEIGHT = 20;

/** Return a bounded render range while preserving exact offscreen height. */
export function notesVisibleRange(
  items: readonly NotesMeasuredRangeItem[],
  measuredHeights: ReadonlyMap<string, number>,
  options: NotesVisibleRangeOptions,
): NotesVisibleRange {
  const heights = items.map((item) => notesRetainedItemHeight(item, measuredHeights));
  const totalHeight = heights.reduce((total, height) => total + height, 0);
  if (items.length < options.minimumVirtualizedCount) {
    return { start: 0, end: items.length, topHeight: 0, bottomHeight: 0, totalHeight };
  }
  const startBoundary = Math.max(0, options.viewportStart - options.overscanPx);
  const endBoundary = Math.max(startBoundary, options.viewportEnd + options.overscanPx);
  let offset = 0;
  let start = 0;
  while (start < heights.length && offset + heights[start] <= startBoundary) {
    offset += heights[start] ?? 0;
    start += 1;
  }
  let end = start;
  let retainedEnd = offset;
  while (end < heights.length && retainedEnd < endBoundary) {
    retainedEnd += heights[end] ?? 0;
    end += 1;
  }
  if (end === start && end < items.length) end += 1;
  return {
    start,
    end,
    topHeight: offset,
    bottomHeight: Math.max(0, totalHeight - retainedEnd),
    totalHeight,
  };
}

/** Resolve measured height with a stable lower bound and outline estimate fallback. */
export function notesRetainedItemHeight(
  item: NotesMeasuredRangeItem,
  measuredHeights: ReadonlyMap<string, number>,
): number {
  const measured = measuredHeights.get(item.id);
  const candidate = measured !== undefined && Number.isFinite(measured)
    ? measured
    : item.estimatedHeight;
  return Math.max(MIN_ITEM_HEIGHT, Number.isFinite(candidate) ? candidate : MIN_ITEM_HEIGHT);
}

/** Capture the first rendered row as a scroll anchor before height corrections. */
export function notesScrollAnchor(
  items: readonly NotesMeasuredRangeItem[],
  measuredHeights: ReadonlyMap<string, number>,
  scrollOffset: number,
): NotesScrollAnchor | null {
  let offset = 0;
  for (const item of items) {
    const height = notesRetainedItemHeight(item, measuredHeights);
    if (offset + height > scrollOffset) return { id: item.id, offset: scrollOffset - offset };
    offset += height;
  }
  return items.length > 0 ? { id: items[items.length - 1]?.id ?? "", offset: 0 } : null;
}

/** Recalculate scroll position for an existing anchor after row measurements change. */
export function notesScrollOffsetForAnchor(
  items: readonly NotesMeasuredRangeItem[],
  measuredHeights: ReadonlyMap<string, number>,
  anchor: NotesScrollAnchor | null,
): number | null {
  if (!anchor) return null;
  let offset = 0;
  for (const item of items) {
    if (item.id === anchor.id) return Math.max(0, offset + anchor.offset);
    offset += notesRetainedItemHeight(item, measuredHeights);
  }
  return null;
}
