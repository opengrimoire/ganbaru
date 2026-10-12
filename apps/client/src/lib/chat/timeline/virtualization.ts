import type { ChatTimelineItemRead, ChatTimelinePageRead } from "$lib/chat/contracts";
import type { TimelineActivityRow, TimelineDisplayRow, TimelineMessageRow } from "./model";

export interface TimelineVirtualItem {
  row: TimelineDisplayRow;
  index: number;
  top: number;
  height: number;
}

export interface TimelineVirtualWindow {
  items: TimelineVirtualItem[];
  startIndex: number;
  endIndex: number;
  totalHeight: number;
  paddingTop: number;
  paddingBottom: number;
}

export interface TimelineScrollbarThumbGeometry {
  offset: number;
  size: number;
}

export interface TimelineScrollOverflowState {
  scrollable: boolean;
  canScrollUp: boolean;
  canScrollDown: boolean;
}

export type TimelineScrollIntent = "following" | "near_end" | "anchored";
export type TimelineMinimapRow = TimelineMessageRow | TimelineActivityRow;

/** Selects a bounded representative set of messages and errors for the minimap. */
export function timelineMinimapRows(
  rows: readonly TimelineDisplayRow[],
  maximumMarkers = 120,
): TimelineMinimapRow[] {
  const candidates = rows.filter((row): row is TimelineMinimapRow =>
    row.kind === "message" || (row.kind === "activity" && (row.status === "failed" || row.activityKind === "error")),
  );
  const limit = Math.max(2, Math.floor(maximumMarkers));
  if (candidates.length <= limit) return candidates;
  const selected = new Set<number>([0, candidates.length - 1]);
  for (let index = 0; index < candidates.length && selected.size < limit; index += 1) {
    if (candidates[index]?.kind === "activity") selected.add(index);
  }
  const available = limit - selected.size;
  if (available > 0) {
    const stride = (candidates.length - 1) / (available + 1);
    for (let marker = 1; marker <= available; marker += 1) selected.add(Math.round(marker * stride));
  }
  return [...selected].sort((left, right) => left - right).slice(0, limit).map((index) => candidates[index]).filter((row): row is TimelineMinimapRow => row !== undefined);
}

/**
 * Computes the visible timeline window from stable row measurements.
 *
 * @param rows Timeline display rows in sequence order.
 * @param measuredHeights Known row heights keyed by stable row ID.
 * @param scrollTop Current scroll offset.
 * @param viewportHeight Visible container height.
 * @param estimatedHeight Height used until a row is measured.
 * @param overscanPx Extra pixels rendered above and below the viewport.
 * @returns Bounded virtual rows and spacer dimensions.
 */
export function computeTimelineVirtualWindow(
  rows: readonly TimelineDisplayRow[],
  measuredHeights: ReadonlyMap<string, number>,
  scrollTop: number,
  viewportHeight: number,
  estimatedHeight = 88,
  overscanPx = 500,
): TimelineVirtualWindow {
  const safeEstimate = positiveDimension(estimatedHeight, 88);
  const safeViewport = Math.max(0, viewportHeight);
  const startOffset = Math.max(0, scrollTop - Math.max(0, overscanPx));
  const endOffset = Math.max(startOffset, scrollTop + safeViewport + Math.max(0, overscanPx));
  const offsets = new Array<number>(rows.length + 1);
  offsets[0] = 0;
  for (let index = 0; index < rows.length; index += 1) {
    const row = rows[index];
    const measured = row ? measuredHeights.get(row.id) : undefined;
    offsets[index + 1] = (offsets[index] ?? 0) + positiveDimension(measured, safeEstimate);
  }
  const totalHeight = offsets.at(-1) ?? 0;
  const startIndex = lowerBound(offsets, startOffset, rows.length);
  const endIndex = Math.min(rows.length, Math.max(startIndex, lowerBound(offsets, endOffset, rows.length) + 1));
  const items: TimelineVirtualItem[] = [];
  for (let index = startIndex; index < endIndex; index += 1) {
    const row = rows[index];
    if (!row) continue;
    items.push({
      row,
      index,
      top: offsets[index] ?? 0,
      height: (offsets[index + 1] ?? 0) - (offsets[index] ?? 0),
    });
  }
  const paddingTop = offsets[startIndex] ?? 0;
  const renderedBottom = offsets[endIndex] ?? paddingTop;
  return {
    items,
    startIndex,
    endIndex,
    totalHeight,
    paddingTop,
    paddingBottom: Math.max(0, totalHeight - renderedBottom),
  };
}

/** Preserves the same visible anchor after timeline geometry changes. */
export function scrollTopForPreservedAnchor(scrollTop: number, anchorTopBefore: number, anchorTopAfter: number): number {
  return Math.max(0, scrollTop + anchorTopAfter - anchorTopBefore);
}

/** Computes native-style scrollbar thumb geometry for a changing timeline extent. */
export function timelineScrollbarThumbGeometry(
  scrollHeight: number,
  clientHeight: number,
  scrollTop: number,
  minimumSize = 24,
): TimelineScrollbarThumbGeometry | null {
  const contentSize = Math.max(0, scrollHeight);
  const trackSize = Math.max(0, clientHeight);
  if (trackSize === 0 || contentSize <= trackSize) return null;
  const size = Math.min(trackSize, Math.max(minimumSize, trackSize * trackSize / contentSize));
  const maximumOffset = trackSize - size;
  const maximumScroll = contentSize - trackSize;
  const boundedScrollTop = Math.min(maximumScroll, Math.max(0, scrollTop));
  return {
    offset: maximumScroll > 0 ? maximumOffset * boundedScrollTop / maximumScroll : 0,
    size,
  };
}

/** Determines which directional fades a bounded timeline disclosure needs. */
export function timelineScrollOverflowState(
  scrollTop: number,
  scrollHeight: number,
  clientHeight: number,
  tolerance = 1,
): TimelineScrollOverflowState {
  const maximumScrollTop = Math.max(0, scrollHeight - clientHeight);
  const edgeTolerance = Math.max(0, tolerance);
  return {
    scrollable: maximumScrollTop > edgeTolerance,
    canScrollUp: scrollTop > edgeTolerance,
    canScrollDown: scrollTop < maximumScrollTop - edgeTolerance,
  };
}

/** Merges paged projection rows without duplicating stable activity IDs. */
export function mergeTimelineItems(
  current: readonly ChatTimelineItemRead[],
  incoming: readonly ChatTimelineItemRead[],
): ChatTimelineItemRead[] {
  const items = new Map(current.map((item) => [item.activityId, item]));
  for (const item of incoming) items.set(item.activityId, item);
  return [...items.values()].sort((left, right) => left.sequenceAnchor - right.sequenceAnchor || left.activityId.localeCompare(right.activityId));
}

/**
 * Evicts distant loaded pages while retaining the page that owns the selected sequence.
 */
export function evictTimelinePages<Page extends Pick<ChatTimelinePageRead, "items">>(
  pages: readonly Page[],
  selectedSequence: number | null,
  maximumPages: number,
): Page[] {
  const limit = Math.max(1, Math.floor(maximumPages));
  if (pages.length <= limit) return [...pages];
  const ordered = [...pages].sort((left, right) => pageStart(left) - pageStart(right));
  const selectedIndex = selectedSequence === null
    ? ordered.length - 1
    : ordered.findIndex((page) => pageContains(page, selectedSequence));
  const center = selectedIndex < 0 ? ordered.length - 1 : selectedIndex;
  let start = Math.max(0, center - Math.floor((limit - 1) / 2));
  start = Math.min(start, ordered.length - limit);
  return ordered.slice(start, start + limit);
}

/** Derives scroll intent without forcing a reader back to the end. */
export function timelineScrollIntent(
  distanceFromEnd: number,
  previous: TimelineScrollIntent,
  followThreshold = 24,
  nearEndThreshold = 160,
): TimelineScrollIntent {
  const distance = Math.max(0, distanceFromEnd);
  if (distance <= followThreshold) return "following";
  if (previous === "following" && distance <= nearEndThreshold) return "following";
  if (distance <= nearEndThreshold) return "near_end";
  return "anchored";
}

/** Computes unread timeline events while respecting explicit follow and send behavior. */
export function nextTimelineUnreadCount(
  current: number,
  appendedEvents: number,
  intent: TimelineScrollIntent,
  sentByCurrentWindow: boolean,
): number {
  if (intent === "following" || sentByCurrentWindow) return 0;
  return Math.max(0, current) + Math.max(0, appendedEvents);
}

function positiveDimension(value: number | undefined, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : fallback;
}

function lowerBound(offsets: readonly number[], target: number, rowCount: number): number {
  let low = 0;
  let high = rowCount;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if ((offsets[middle + 1] ?? 0) < target) low = middle + 1;
    else high = middle;
  }
  return low;
}

function pageStart(page: Pick<ChatTimelinePageRead, "items">): number {
  return page.items[0]?.sequenceAnchor ?? Number.MAX_SAFE_INTEGER;
}

function pageContains(page: Pick<ChatTimelinePageRead, "items">, sequence: number): boolean {
  const first = page.items[0]?.sequenceAnchor;
  const last = page.items.at(-1)?.sequenceAnchor;
  return first !== undefined && last !== undefined && sequence >= first && sequence <= last;
}
