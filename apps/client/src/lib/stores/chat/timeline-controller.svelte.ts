import * as chatApi from "$lib/api/chat";
import type {
  ChatThreadId,
  ChatTimelineItemRead,
  ChatTimelinePageRead,
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

  private request = 0;

  constructor(private readonly options: ChatTimelineControllerOptions) {}

  reset(): void {
    this.request += 1;
    this.clear();
    this.loading = false;
  }

  clear(): void {
    this.pages = [];
    this.items = [];
    this.error = null;
  }

  async loadOlder(selectedSequence: number | null = null): Promise<void> {
    const threadId = this.options.selectedThreadId();
    const cursor = this.pages[0]?.previousCursor;
    if (!threadId || !cursor || this.loading) return;
    await this.load(threadId, cursor, true, selectedSequence);
  }

  async load(
    threadId: ChatThreadId,
    cursor: string | null = null,
    prepend = false,
    selectedSequence: number | null = null,
  ): Promise<void> {
    const request = ++this.request;
    this.loading = true;
    this.error = null;
    try {
      const page = await chatApi.readChatTimelinePage(threadId, cursor);
      if (request !== this.request || this.options.selectedThreadId() !== threadId) return;
      this.pages = prepend
        ? evictTimelinePages([page, ...this.pages], selectedSequence, MAX_TIMELINE_PAGES)
        : [page];
      this.items = mergeTimelineItems([], this.pages.flatMap((entry) => entry.items));
    } catch (error: unknown) {
      if (request !== this.request) return;
      this.error = chatErrorMessage(error, "Chat could not be loaded");
      throw error;
    } finally {
      if (request === this.request) this.loading = false;
    }
  }
}
