import * as chatApi from "$lib/api/chat";
import type {
  ChatThreadId,
  ChatTimelineItemRead,
  ChatTimelinePageRead,
  ChatTimelineTurnRead,
} from "$lib/chat/contracts";
import { chatErrorMessage } from "$lib/chat/error-presentation";
import { evictTimelinePages, mergeTimelineItems } from "$lib/chat/timeline/virtualization";

const MAX_TIMELINE_PAGES = 8;

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
        this.holdsNewest &&= pages.at(-1) === newest;
        this.setPages(pages);
      } else {
        this.holdsNewest = cursor === null;
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
   * Rereads the newest page of the selected thread and merges it into the loaded pages, keeping older pages. A
   * refresh never blocks or discards an older-page load in flight.
   */
  async refreshLatest(threadId: ChatThreadId, selectedSequence: number | null = null): Promise<void> {
    if (this.options.selectedThreadId() !== threadId) return;
    if (this.pages.length === 0 || this.pages[0]?.threadId !== threadId) {
      await this.load(threadId, null, false, selectedSequence);
      return;
    }
    if (!this.holdsNewest) return;
    const generation = this.generation;
    const refresh = ++this.refreshRequest;
    try {
      const page = await chatApi.readChatTimelinePage(threadId, null);
      if (!this.isCurrentRefresh(generation, refresh, threadId) || !this.holdsNewest) return;
      this.setPages(evictTimelinePages(mergeNewestPage(this.pages, page), selectedSequence, MAX_TIMELINE_PAGES));
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
