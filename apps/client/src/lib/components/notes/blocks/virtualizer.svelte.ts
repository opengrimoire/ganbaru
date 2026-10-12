import { untrack } from "svelte";
import { notesHydratedItemsByOutline, type NotesBlockOutlineItem } from "$lib/notes/blocks/outline";
import type { NotesBlockTreeItem } from "$lib/notes/types";
import {
  notesScrollAnchor,
  notesScrollOffsetForAnchor,
  notesVisibleRange,
} from "$lib/notes/editor/visible-range";

export interface NotesBlockVirtualizerOptions {
  readScrollViewport: () => HTMLDivElement | null;
  readListElement: () => HTMLDivElement | null;
  readOutlines: () => readonly NotesBlockOutlineItem[];
  readItems: () => readonly NotesBlockTreeItem[];
  readPinnedBlockIds: () => readonly string[];
  readFocusRequest: () => { blockId: string | null; requestId: number; preventScroll?: boolean };
  hydrateBlockRange: (blockIds: readonly string[]) => Promise<void>;
}

/** Calculate the estimated top offset only when a focused block is not rendered. */
export function notesFocusedBlockEstimatedOffset(
  rangeItems: readonly { id: string; estimatedHeight: number }[],
  measuredHeights: ReadonlyMap<string, number>,
  blockId: string,
  isTargetRendered: boolean,
  preventScroll = false,
): number | null {
  if (isTargetRendered || preventScroll) return null;
  const index = rangeItems.findIndex((item) => item.id === blockId);
  if (index < 0) return null;
  return rangeItems
    .slice(0, index)
    .reduce((total, item) => total + (measuredHeights.get(item.id) ?? item.estimatedHeight), 0);
}

/** Own Notes outline virtualization, hydration, and estimated focus scrolling. */
export function createNotesBlockVirtualizer(options: NotesBlockVirtualizerOptions) {
  let viewportStart = $state(0);
  let viewportHeight = $state(0);
  let measuredBlockHeights = $state(new Map<string, number>());
  let handledFocusRequestId = 0;
  const rangeItems = $derived(options.readOutlines().map((item) => ({
    id: item.outline.id,
    estimatedHeight: item.outline.retained_height,
  })));
  const viewportRange = $derived(notesVisibleRange(rangeItems, measuredBlockHeights, {
    viewportStart,
    viewportEnd: viewportStart + viewportHeight,
    overscanPx: 480,
    minimumVirtualizedCount: 120,
  }));
  const visibleRange = $derived(viewportRange);
  const visibleOutlines = $derived(options.readOutlines().slice(visibleRange.start, visibleRange.end));
  const hydratedItemsById = $derived(notesHydratedItemsByOutline(options.readOutlines(), options.readItems()));

  function updateViewportRange(): void {
    const scrollViewport = options.readScrollViewport();
    const listElement = options.readListElement();
    if (!scrollViewport || !listElement) return;
    const viewportRect = scrollViewport.getBoundingClientRect();
    const listRect = listElement.getBoundingClientRect();
    const listStart = scrollViewport.scrollTop + listRect.top - viewportRect.top;
    viewportStart = Math.max(0, scrollViewport.scrollTop - listStart);
    viewportHeight = scrollViewport.clientHeight;
  }

  $effect(() => {
    const viewport = options.readScrollViewport();
    if (!viewport) return;
    const observer = new ResizeObserver(updateViewportRange);
    observer.observe(viewport);
    viewport.addEventListener("scroll", updateViewportRange, { passive: true });
    updateViewportRange();
    return () => {
      observer.disconnect();
      viewport.removeEventListener("scroll", updateViewportRange);
    };
  });

  $effect(() => {
    const retainedIds = visibleOutlines.map((item) => item.outline.id);
    retainedIds.push(...options.readPinnedBlockIds());
    untrack(() => { void options.hydrateBlockRange(retainedIds); });
  });

  $effect(() => {
    const { blockId, requestId, preventScroll } = options.readFocusRequest();
    const scrollViewport = options.readScrollViewport();
    if (!blockId || requestId === handledFocusRequestId || !scrollViewport) return;
    const isTargetRendered = Array.from(
      options.readListElement()?.querySelectorAll<HTMLElement>("[data-notes-virtual-block]") ?? [],
    ).some((element) => element.dataset.notesVirtualBlock === blockId);
    const targetOffset = notesFocusedBlockEstimatedOffset(
      rangeItems,
      measuredBlockHeights,
      blockId,
      isTargetRendered,
      preventScroll,
    );
    if (targetOffset === null) {
      if (isTargetRendered || preventScroll) handledFocusRequestId = requestId;
      return;
    }
    handledFocusRequestId = requestId;
    scrollViewport.scrollTop += targetOffset - viewportStart;
    updateViewportRange();
  });

  function measureBlock(
    node: HTMLElement,
    blockId: string,
  ): { update: (nextId: string) => void; destroy: () => void } {
    let currentId = blockId;
    const observer = new ResizeObserver(([entry]) => {
      const nextHeight = entry?.borderBoxSize?.[0]?.blockSize ?? entry?.contentRect.height;
      if (!nextHeight || Math.abs((measuredBlockHeights.get(currentId) ?? 0) - nextHeight) < 0.5) return;
      const anchor = notesScrollAnchor(rangeItems, measuredBlockHeights, viewportStart);
      const next = new Map(measuredBlockHeights);
      next.set(currentId, nextHeight);
      measuredBlockHeights = next;
      const corrected = notesScrollOffsetForAnchor(rangeItems, next, anchor);
      const scrollViewport = options.readScrollViewport();
      if (corrected !== null && scrollViewport && Math.abs(corrected - viewportStart) >= 0.5) {
        scrollViewport.scrollTop += corrected - viewportStart;
      }
    });
    observer.observe(node);
    return {
      update(nextId) {
        currentId = nextId;
      },
      destroy() {
        observer.disconnect();
      },
    };
  }

  return {
    get visibleRange() {
      return visibleRange;
    },
    get visibleOutlines() {
      return visibleOutlines;
    },
    get hydratedItemsById() {
      return hydratedItemsById;
    },
    measureBlock,
  };
}
