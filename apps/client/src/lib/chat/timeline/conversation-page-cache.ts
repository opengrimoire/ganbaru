import type { ChatChannelPageRead, ChatReplyThreadPageRead } from "$lib/chat/contracts";

type ConversationPage = ChatChannelPageRead | ChatReplyThreadPageRead;

interface CacheEntry<T extends ConversationPage> {
  pages: T[];
  byteSize: number;
  touchedAt: number;
}

/**
 * Keeps a bounded in-memory working set for organizational conversations.
 */
export class ChatConversationPageCache<T extends ConversationPage> {
  private readonly entries = new Map<string, CacheEntry<T>>();
  private clock = 0;

  constructor(
    private readonly maximumEntries: number,
    private readonly maximumBytes: number,
    private readonly maximumPagesPerEntry: number,
  ) {}

  get(key: string): T[] | null {
    const entry = this.entries.get(key);
    if (!entry) return null;
    entry.touchedAt = ++this.clock;
    return structuredClone(entry.pages);
  }

  set(key: string, pages: readonly T[]): void {
    const serialized = JSON.stringify(pages.slice(0, this.maximumPagesPerEntry));
    const boundedPages = JSON.parse(serialized) as T[];
    this.entries.set(key, {
      pages: boundedPages,
      byteSize: new TextEncoder().encode(serialized).byteLength,
      touchedAt: ++this.clock,
    });
    this.evict();
  }

  clear(): void {
    this.entries.clear();
  }

  private evict(): void {
    const totalBytes = () => [...this.entries.values()].reduce((sum, entry) => sum + entry.byteSize, 0);
    while (this.entries.size > this.maximumEntries || totalBytes() > this.maximumBytes) {
      const oldest = [...this.entries.entries()].sort((left, right) => (
        left[1].touchedAt - right[1].touchedAt || left[0].localeCompare(right[0])
      ))[0];
      if (!oldest) return;
      this.entries.delete(oldest[0]);
    }
  }
}
