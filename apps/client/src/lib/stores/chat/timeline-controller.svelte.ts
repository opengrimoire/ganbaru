import * as chatApi from "$lib/api/chat";
import type {
  ChatThreadId,
  ChatTimelineDeltaRead,
  ChatTimelineItemRead,
  ChatTimelinePageRead,
  ChatTimelineTurnRead,
} from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";
import { evictTimelinePages, mergeTimelineItems } from "$lib/chat/timeline/virtualization";

const MAX_TIMELINE_PAGES = 8;
/** Bounds the since-reads one live refresh pages through before it falls back to a full load. */
const MAX_SINCE_READ_PASSES = 8;

export interface ChatTimelineControllerOptions {
  selectedThreadId: () => ChatThreadId | null;
}

/** Owns provider-thread timeline paging and stale-request protection. */
export class ChatTimelineController {
  pages = $state<ChatTimelinePageRead[]>([]);
  items = $state<ChatTimelineItemRead[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);

  /** Paging request token; a newer full or older-page load discards an older one. */
  private request = 0;
  /** Bumped by every reset and full load, so a live refresh started before one is discarded. */
  private generation = 0;
  /** Live refresh token; a newer refresh discards an older one. */
  private refreshRequest = 0;
  /** Whether the loaded pages still end at the thread's newest row, so a live refresh can attach to them. */
  private holdsNewest = false;
  /** The change clock the loaded rows are current at, or null before a full load; a live refresh reads after it. */
  private changeCursor: number | null = null;

  constructor(private readonly options: ChatTimelineControllerOptions) {}

  reset(): void {
    this.request += 1;
    this.generation += 1;
    this.clear();
    this.loading = false;
  }

  clear(): void {
    this.pages = [];
    this.items = [];
    this.error = null;
    this.holdsNewest = false;
    this.changeCursor = null;
  }

  async loadOlder(selectedSequence: number | null = null): Promise<void> {
    const threadId = this.options.selectedThreadId();
    const cursor = this.pages[0]?.previousCursor;
    if (!threadId || !cursor || this.loading) return;
    await this.load(threadId, cursor, true, selectedSequence);
  }

  /**
   * Replaces the loaded pages with one page read at `cursor`, or the newest page when it is null. With `prepend`,
   * adds an older page in front of the loaded ones instead.
   */
  async load(
    threadId: ChatThreadId,
    cursor: string | null = null,
    prepend = false,
    selectedSequence: number | null = null,
  ): Promise<void> {
    const request = ++this.request;
    if (!prepend) this.generation += 1;
    this.loading = true;
    this.error = null;
    try {
      const page = await chatApi.readChatTimelinePage(threadId, cursor);
      if (request !== this.request || this.options.selectedThreadId() !== threadId) return;
      if (prepend) {
        const newest = this.pages.at(-1);
        const pages = evictTimelinePages([page, ...this.pages], selectedSequence, MAX_TIMELINE_PAGES);
        // A page read before the held cursor may predate changes in its range, so the next since-read covers them.
        if (this.changeCursor !== null) this.changeCursor = Math.min(this.changeCursor, page.changeCursor);
        this.holdsNewest &&= pages.at(-1) === newest;
        this.setPages(pages);
      } else {
        this.holdsNewest = cursor === null;
        this.changeCursor = page.changeCursor;
        this.setPages([page]);
      }
    } catch (error: unknown) {
      if (request !== this.request) return;
      this.error = chatErrorMessage(error, "Chat could not be loaded");
      throw error;
    } finally {
      if (request === this.request) this.loading = false;
    }
  }

  /**
   * Brings the loaded pages of the selected thread up to date, keeping older pages. With a held change cursor it
   * pages the rows that changed after it until none are pending, falling back to a full load after
   * `MAX_SINCE_READ_PASSES` reads or when the read requires a reset; without a cursor it rereads the newest page and
   * merges it. A refresh never blocks or discards an older-page load in flight.
   */
  async refreshLatest(threadId: ChatThreadId, selectedSequence: number | null = null): Promise<void> {
    if (this.options.selectedThreadId() !== threadId) return;
    if (this.pages.length === 0 || this.pages[0]?.threadId !== threadId) {
      await this.load(threadId, null, false, selectedSequence);
      return;
    }
    const generation = this.generation;
    const refresh = ++this.refreshRequest;
    try {
      if (this.changeCursor !== null) {
        for (let pass = 0; pass < MAX_SINCE_READ_PASSES; pass += 1) {
          const delta = await chatApi.readChatTimelineSince(threadId, this.changeCursor);
          if (!this.isCurrentRefresh(generation, refresh, threadId)) return;
          if (delta.resetRequired) break;
          this.applyPages(mergeTimelineDelta(this.pages, delta, this.holdsNewest), selectedSequence);
          this.changeCursor = delta.changeCursor;
          if (!delta.hasMore) return;
        }
        // Changes still pending, or pruned removals, may touch any loaded page, so only a full load is current.
        await this.load(threadId, null, false, selectedSequence);
        return;
      }
      if (!this.holdsNewest) return;
      const page = await chatApi.readChatTimelinePage(threadId, null);
      if (!this.isCurrentRefresh(generation, refresh, threadId) || !this.holdsNewest) return;
      this.applyPages(mergeNewestPage(this.pages, page), selectedSequence);
      this.changeCursor = page.changeCursor;
    } catch (error: unknown) {
      if (!this.isCurrentRefresh(generation, refresh, threadId)) return;
      this.error = chatErrorMessage(error, "Chat could not be loaded");
      throw error;
    }
  }

  private isCurrentRefresh(generation: number, refresh: number, threadId: ChatThreadId): boolean {
    return generation === this.generation
      && refresh === this.refreshRequest
      && this.options.selectedThreadId() === threadId;
  }

  /** Evicts distant pages from a live merge and tracks whether the newest merged page survived. */
  private applyPages(merged: ChatTimelinePageRead[], selectedSequence: number | null): void {
    const newest = merged.at(-1);
    const pages = evictTimelinePages(merged, selectedSequence, MAX_TIMELINE_PAGES);
    this.holdsNewest &&= pages.at(-1) === newest;
    this.setPages(pages);
  }

  private setPages(pages: ChatTimelinePageRead[]): void {
    this.pages = pages;
    this.items = mergeTimelineItems([], pages.flatMap((entry) => entry.items));
  }
}

/**
 * Merges a fresh newest page into loaded pages. Rows at or after the fresh page's first row come from the fresh page,
 * so a row it no longer returns is dropped; rows before it stay. When the fresh page starts after the newest loaded
 * row, rows between them are unknown, so the fresh page replaces every loaded page.
 */
export function mergeNewestPage(
  pages: readonly ChatTimelinePageRead[],
  fresh: ChatTimelinePageRead,
): ChatTimelinePageRead[] {
  const newest = pages.at(-1);
  const first = fresh.items[0];
  if (!newest || !first) return [fresh];
  const newestLast = newest.items.at(-1);
  const overlaps = fresh.previousCursor === null
    || (newestLast !== undefined && compareRows(first, newestLast) <= 0);
  if (!overlaps) return [fresh];
  const kept = pages
    .map((page) => ({ ...page, items: page.items.filter((item) => compareRows(item, first) < 0) }));
  const keptNewest = kept.at(-1);
  const older = kept.slice(0, -1).filter((page) => page.items.length > 0);
  const newestFirst = newest.items[0];
  const reachesOlder = newestFirst === undefined || compareRows(first, newestFirst) < 0;
  return [
    ...older,
    {
      ...fresh,
      items: [...(keptNewest?.items ?? []), ...fresh.items],
      turns: mergeTurns(newest.turns, fresh.turns),
      previousCursor: reachesOlder && older.length === 0 ? fresh.previousCursor : newest.previousCursor,
    },
  ];
}

/**
 * Applies a since-read to loaded pages. Removed and changed rows leave their pages first; each changed row then joins
 * the newest page whose first loaded row is not after it. A changed row older than the oldest loaded row is dropped
 * while older history is not loaded, and one newer than the newest loaded row is dropped unless the pages hold the
 * thread's newest row. Pages a removal empties are dropped, keeping the outer paging cursors.
 */
export function mergeTimelineDelta(
  pages: readonly ChatTimelinePageRead[],
  delta: ChatTimelineDeltaRead,
  holdsNewest: boolean,
): ChatTimelinePageRead[] {
  const oldest = pages[0];
  const newest = pages.at(-1);
  if (!oldest || !newest) return [];
  const leaving = new Set<string>(delta.removedItemIds);
  for (const entry of delta.items) leaving.add(entry.activityId);
  const firstRows = pages.map((page) => page.items[0]);
  const loadedFirst = firstRows.find((row) => row !== undefined);
  const loadedLast = pages.findLast((page) => page.items.length > 0)?.items.at(-1);
  const merged = pages.map((page) => ({
    ...page,
    items: page.items.filter((entry) => !leaving.has(entry.activityId)),
    turns: page.turns.map((turn) => delta.turns.find((fresh) => fresh.turnId === turn.turnId) ?? turn),
  }));
  const placedTurns = new Set(pages.flatMap((page) => page.turns.map((turn) => turn.turnId)));
  for (const entry of delta.items) {
    if (loadedFirst && oldest.previousCursor !== null && compareRows(entry, loadedFirst) < 0) continue;
    if (loadedLast && !holdsNewest && compareRows(entry, loadedLast) > 0) continue;
    const index = firstRows.findLastIndex((row) => row !== undefined && compareRows(row, entry) <= 0);
    const target = merged[Math.max(0, index)];
    if (!target) continue;
    target.items.push(entry);
    const turn = entry.turnId === null ? undefined : delta.turns.find((fresh) => fresh.turnId === entry.turnId);
    if (turn && !placedTurns.has(turn.turnId)) {
      target.turns.push(turn);
      placedTurns.add(turn.turnId);
    }
  }
  const unplacedTurns = holdsNewest ? delta.turns.filter((turn) => !placedTurns.has(turn.turnId)) : [];
  merged.at(-1)?.turns.push(...unplacedTurns);
  for (const page of merged) page.items.sort(compareRows);
  return dropEmptyPages(merged, oldest, newest);
}

function dropEmptyPages(
  pages: readonly ChatTimelinePageRead[],
  oldest: ChatTimelinePageRead,
  newest: ChatTimelinePageRead,
): ChatTimelinePageRead[] {
  const kept = pages.filter((page) => page.items.length > 0);
  const orphanTurns = pages.filter((page) => page.items.length === 0).flatMap((page) => page.turns);
  if (kept.length === 0) return [{ ...newest, items: [], turns: orphanTurns, previousCursor: oldest.previousCursor }];
  const lastIndex = kept.length - 1;
  return kept.map((page, index) => ({
    ...page,
    previousCursor: index === 0 ? oldest.previousCursor : page.previousCursor,
    nextCursor: index === lastIndex ? newest.nextCursor : page.nextCursor,
    turns: index === lastIndex ? mergeTurns(orphanTurns, page.turns) : page.turns,
  }));
}

function compareRows(left: ChatTimelineItemRead, right: ChatTimelineItemRead): number {
  if (left.sequenceAnchor !== right.sequenceAnchor) return left.sequenceAnchor - right.sequenceAnchor;
  if (left.activityId === right.activityId) return 0;
  return left.activityId < right.activityId ? -1 : 1;
}

function mergeTurns(
  current: readonly ChatTimelineTurnRead[],
  incoming: readonly ChatTimelineTurnRead[],
): ChatTimelineTurnRead[] {
  const turns = new Map(current.map((turn) => [turn.turnId, turn]));
  for (const turn of incoming) turns.set(turn.turnId, turn);
  return [...turns.values()];
}
